import assert from "node:assert/strict";
import { execFileSync, spawn, spawnSync } from "node:child_process";
import { once } from "node:events";
import {
  copyFile,
  mkdir,
  mkdtemp,
  rm,
  symlink,
  writeFile,
} from "node:fs/promises";
import { constants, tmpdir } from "node:os";
import path from "node:path";
import { createInterface } from "node:readline";
import test from "node:test";

for (const fallback of [false, true]) {
  test(
    `npm wrapper preserves exits and signals (${fallback ? "spawn fallback" : "native execve"})`,
    {
      skip:
        process.platform === "win32" ||
        !process.env.XDCODE_TEST_BINARY ||
        (!fallback && typeof process.execve !== "function"),
      timeout: 90_000,
    },
    async () => {
      const directory = await mkdtemp(path.join(tmpdir(), "xdcode-npm-exit-"));
      let child;
      let killTimer;
      try {
        const architecture = process.arch === "arm64" ? "aarch64" : "x86_64";
        const platform =
          process.platform === "darwin" ? "apple-darwin" : "unknown-linux-musl";
        const binaries = path.join(
          directory,
          "vendor",
          `${architecture}-${platform}`,
          "bin",
        );
        const home = path.join(directory, "home");
        await mkdir(binaries, { recursive: true });
        await mkdir(home);
        await mkdir(path.join(directory, "bin"));
        await writeFile(
          path.join(directory, "package.json"),
          '{"type":"module"}',
        );
        const wrapper = path.join(directory, "bin", "xdcode.js");
        await copyFile(new URL("../bin/xdcode.js", import.meta.url), wrapper);
        await symlink(
          path.resolve(process.env.XDCODE_TEST_BINARY),
          path.join(binaries, "xdcode"),
        );
        const preload = path.join(directory, "disable-execve.mjs");
        await writeFile(
          preload,
          'Object.defineProperty(process, "execve", { value: undefined });\n',
        );
        const argumentsPrefix = fallback
          ? ["--import", preload, wrapper]
          : [wrapper];
        const environment = {
          ...process.env,
          XDCODE_HOME: home,
          XDCODE_SQLITE_HOME: home,
        };
        const invalid = spawnSync(
          process.execPath,
          [...argumentsPrefix, "--invalid-xdcode-option"],
          {
            env: environment,
            timeout: 30_000,
            killSignal: "SIGKILL",
          },
        );
        assert.equal(invalid.error, undefined);
        assert.deepEqual([invalid.status, invalid.signal], [2, null]);

        for (const signal of ["SIGHUP", "SIGXFSZ"]) {
          child = spawn(
            process.execPath,
            [...argumentsPrefix, "migrate-nfs-db"],
            {
              env: environment,
              detached: true,
            },
          );
          const processGroup = child.pid;
          killTimer = setTimeout(() => {
            try {
              process.kill(-processGroup, "SIGKILL");
            } catch (error) {
              if (error.code !== "ESRCH") throw error;
            }
          }, 30_000);
          const exited = once(child, "exit");
          const lines = createInterface({ input: child.stderr });
          let prompted = false;
          for await (const line of lines) {
            if (line.endsWith("[y/N]:")) {
              prompted = true;
              break;
            }
          }
          assert.ok(
            prompted,
            "isolated migration reaches its confirmation prompt",
          );
          const processes = execFileSync("ps", ["-axo", "pid=,ppid="], {
            encoding: "utf8",
          });
          const children = processes
            .trim()
            .split("\n")
            .map((line) => line.trim().split(/\s+/).map(Number))
            .filter(([, parent]) => parent === child.pid);
          assert.equal(children.length, fallback ? 1 : 0);
          if (!fallback || signal === "SIGHUP") {
            child.kill(signal);
            assert.deepEqual(await exited, [null, signal]);
          } else {
            process.kill(children[0][0], signal);
            assert.deepEqual(await exited, [
              128 + constants.signals[signal],
              null,
            ]);
          }
          clearTimeout(killTimer);
        }
      } finally {
        clearTimeout(killTimer);
        if (child && child.exitCode === null && child.signalCode === null) {
          process.kill(-child.pid, "SIGKILL");
          await once(child, "exit");
        }
        await rm(directory, { recursive: true, force: true });
      }
    },
  );
}
