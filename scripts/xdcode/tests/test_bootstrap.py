"""Universal installer tests with isolated homes and local release fixtures."""

from __future__ import annotations

import hashlib
import json
import os
import platform
import shutil
import subprocess
import sys
import tarfile
import tempfile
import unittest
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[3]
INSTALLER = REPO_ROOT / "install.sh"


@unittest.skipIf(os.name == "nt", "Bash installer is for Linux and macOS")
class BootstrapTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="xdcode-bootstrap-test-")
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name).resolve()
        self.home = self.root / "home"
        self.downloads = self.root / "downloads"
        self.tools = self.root / "tools"
        self.scratch = self.root / "scratch"
        self.payload = self.root / "payload"
        self.version = "0.167.0"
        self.commit = "a" * 40
        self.identity = f"{self.version}-g{self.commit[:12]}"
        self.tag = "v" + self.identity
        for directory in (
            self.home / ".xdcode",
            self.downloads,
            self.tools,
            self.scratch,
        ):
            directory.mkdir(parents=True)
        self.sentinel = self.home / ".xdcode/config.toml"
        self.sentinel.write_text("existing-runtime-data-must-be-retained\n")
        self.environment = {
            key: value
            for key, value in os.environ.items()
            if not key.startswith(("XDCODE_", "CODEX_", "OPENAI_", "OTEL_"))
            and not key.endswith(("_API_KEY", "_TOKEN", "_SECRET"))
        }
        self.environment.update(
            HOME=str(self.home),
            TMPDIR=str(self.scratch),
            PATH=f"{self.tools}{os.pathsep}{os.environ['PATH']}",
            FIXTURE_DOWNLOADS=str(self.downloads),
            FIXTURE_LOG=str(self.root / "curl.jsonl"),
            FIXTURE_ARGS=str(self.root / "linux-arguments.json"),
            FIXTURE_TAG=self.tag,
            MOCK_OS="Darwin",
            MOCK_MACHINE="arm64",
        )
        self.make_tool(
            "curl",
            """import json, os, pathlib, shutil, sys
arguments = sys.argv[1:]
with open(os.environ["FIXTURE_LOG"], "a") as log:
    log.write(json.dumps(arguments) + "\\n")
if os.environ.get("MOCK_DOWNLOAD_FAIL"):
    sys.exit(22)
if "--head" in arguments:
    print(os.environ.get("MOCK_LATEST_URL", "https://github.com/xxddccaa/xdcode/releases/tag/" + os.environ["FIXTURE_TAG"]))
else:
    destination = arguments[arguments.index("--output") + 1]
    shutil.copyfile(pathlib.Path(os.environ["FIXTURE_DOWNLOADS"]) / arguments[-1].rsplit("/", 1)[-1], destination)
""",
        )
        self.make_tool(
            "uname",
            'import os, sys\nprint(os.environ["MOCK_OS" if sys.argv[1] == "-s" else "MOCK_MACHINE"])\n',
        )
        self.make_tool(
            "pgrep",
            'import os, sys\nsys.exit(int(os.environ.get("MOCK_PROCESS_EXIT", "1")))\n',
        )
        self.make_tool(
            "file",
            """import os, sys
if os.environ.get("MOCK_NATIVE_PACKAGE"):
    os.execv("/usr/bin/file", ["file", *sys.argv[1:]])
print(os.environ.get("MOCK_FILE_DESCRIPTION", "Mach-O 64-bit executable arm64"))
""",
        )
        self.make_tool(
            "plutil",
            """import json, os, sys
if os.environ.get("MOCK_NATIVE_PACKAGE"):
    os.execv("/usr/bin/plutil", ["plutil", *sys.argv[1:]])
with open(sys.argv[-1]) as metadata:
    value = json.load(metadata)[sys.argv[sys.argv.index("-extract") + 1]]
print(str(value).lower() if isinstance(value, bool) else value)
""",
        )
        self.make_macos_archive()

    def make_tool(self, name: str, body: str):
        tool = self.tools / name
        tool.write_text(f"#!{sys.executable}\n" + body)
        tool.chmod(0o755)

    def make_macos_archive(self, **changes):
        for directory in ("bin", "xdcode-path", "xdcode-resources"):
            (self.payload / directory).mkdir(parents=True, exist_ok=True)
        for name, output in (
            ("bin/xdcode", f"xdcode {self.version}"),
            ("bin/xdcode-code-mode-host", "code-mode helper"),
            ("xdcode-path/rg", "ripgrep fixture"),
        ):
            binary = self.payload / name
            binary.write_text(f"#!/bin/sh\nprintf '%s\\n' '{output}'\n")
            binary.chmod(0o755)
        metadata = {
            "schema_version": 1,
            "version": self.version,
            "build_id": "g" + self.commit[:12],
            "source_commit": self.commit,
            "source_dirty": False,
            "target": "aarch64-apple-darwin",
            **changes,
        }
        manifest = {
            "layoutVersion": 1,
            "version": self.version,
            "buildId": "g" + self.commit[:12],
            "target": "aarch64-apple-darwin",
            "variant": "xdcode",
            "entrypoint": "bin/xdcode",
            "resourcesDir": "xdcode-resources",
            "pathDir": "xdcode-path",
        }
        (self.payload / "build-info.json").write_text(json.dumps(metadata))
        (self.payload / "xdcode-package.json").write_text(json.dumps(manifest))
        files = sorted(
            path
            for path in self.payload.rglob("*")
            if path.is_file() and path.name != "SHA256SUMS"
        )
        (self.payload / "SHA256SUMS").write_text(
            "".join(
                f"{hashlib.sha256(path.read_bytes()).hexdigest()}  {path.relative_to(self.payload).as_posix()}\n"
                for path in files
            )
        )
        self.archive = self.downloads / f"xdcode-{self.identity}-macos-arm64.tar.gz"
        self.pack_payload()

    def pack_payload(self, extra: tarfile.TarInfo | None = None):
        with tarfile.open(self.archive, "w:gz") as archive:
            for path in sorted(self.payload.rglob("*")):
                archive.add(
                    path, arcname=path.relative_to(self.payload), recursive=False
                )
            if extra is not None:
                archive.addfile(extra)
        self.write_sidecar()

    def write_sidecar(self):
        Path(str(self.archive) + ".sha256").write_text(
            f"{hashlib.sha256(self.archive.read_bytes()).hexdigest()}  {self.archive.name}\n"
        )

    def run_installer(self, *arguments, **environment):
        return subprocess.run(
            ["bash", str(INSTALLER), *arguments],
            cwd=self.root,
            env={**self.environment, **environment},
            capture_output=True,
            text=True,
            timeout=60,
            check=False,
        )

    @property
    def entry(self):
        return self.home / ".local/bin/xdcode"

    def test_help_and_syntax_need_no_download(self):
        subprocess.run(["bash", "-n", str(INSTALLER)], check=True)
        result = self.run_installer("--help")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("--force", result.stdout)
        self.assertFalse((self.root / "curl.jsonl").exists())

    def test_latest_install_and_repeat_retain_build_and_configuration(self):
        result = self.run_installer("--force")
        self.assertEqual(result.returncode, 0, result.stderr)
        destination = self.entry.resolve()
        modified = destination.stat().st_mtime_ns
        repeated = self.run_installer("--force")
        self.assertEqual(repeated.returncode, 0, repeated.stderr)
        self.assertEqual(self.entry.resolve(), destination)
        self.assertEqual(destination.stat().st_mtime_ns, modified)
        self.assertEqual(
            self.sentinel.read_text(), "existing-runtime-data-must-be-retained\n"
        )
        self.assertFalse((self.home / ".local/lib/xdcode/.macos-install.lock").exists())
        self.assertEqual(list(self.scratch.iterdir()), [])

    def test_idle_default_and_force_are_explicit(self):
        blocked = self.run_installer(MOCK_PROCESS_EXIT="0")
        self.assertNotEqual(blocked.returncode, 0)
        self.assertIn("active xdcode", blocked.stderr)
        allowed = self.run_installer("--force", MOCK_PROCESS_EXIT="0")
        self.assertEqual(allowed.returncode, 0, allowed.stderr)

    def test_future_latest_release_switches_entry_and_keeps_the_old_build(self):
        first = self.run_installer("--force")
        self.assertEqual(first.returncode, 0, first.stderr)
        previous = self.entry.resolve()
        previous_metadata = previous.parent.parent / "build-info.json"
        original = previous_metadata.read_bytes()
        self.version = "0.168.0"
        self.commit = "b" * 40
        self.identity = f"{self.version}-g{self.commit[:12]}"
        self.tag = "v" + self.identity
        self.environment["FIXTURE_TAG"] = self.tag
        self.make_macos_archive()
        upgraded = self.run_installer("--force", MOCK_PROCESS_EXIT="0")
        self.assertEqual(upgraded.returncode, 0, upgraded.stderr)
        self.assertNotEqual(self.entry.resolve(), previous)
        self.assertTrue(previous.is_file())
        self.assertEqual(previous_metadata.read_bytes(), original)
        self.assertIn(self.identity, str(self.entry.resolve()))

    def test_existing_lock_is_not_removed_by_another_installer(self):
        lock = self.home / ".local/lib/xdcode/.macos-install.lock"
        lock.mkdir(parents=True)
        result = self.run_installer("--force")
        self.assertNotEqual(result.returncode, 0)
        self.assertTrue(lock.is_dir())
        self.assertFalse(self.entry.exists())

    def test_regular_or_unrecognized_entry_is_retained(self):
        self.entry.parent.mkdir(parents=True)
        self.entry.write_text("retain-unmanaged-entry")
        result = self.run_installer("--force")
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(self.entry.read_text(), "retain-unmanaged-entry")
        self.entry.unlink()
        self.entry.symlink_to(self.sentinel)
        result = self.run_installer("--force")
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(self.entry.resolve(), self.sentinel)

    def test_archive_checksum_and_filename_binding(self):
        sidecar = Path(str(self.archive) + ".sha256")
        for contents in (
            f"{'0' * 64}  {self.archive.name}\n",
            f"{hashlib.sha256(self.archive.read_bytes()).hexdigest()}  unrelated.tar.gz\n",
        ):
            with self.subTest(contents=contents):
                sidecar.write_text(contents)
                result = self.run_installer("--force")
                self.assertNotEqual(result.returncode, 0)
                self.assertFalse(self.entry.exists())

    def test_payload_integrity_and_source_metadata(self):
        for changes in (
            {"source_dirty": True},
            {"source_commit": "b" * 40},
            {"version": "0.165.0"},
        ):
            with self.subTest(changes=changes):
                self.make_macos_archive(**changes)
                result = self.run_installer("--force")
                self.assertNotEqual(result.returncode, 0)
                self.assertFalse(self.entry.exists())
        self.make_macos_archive()
        (self.payload / "bin/xdcode").write_text("corrupt payload")
        self.pack_payload()
        result = self.run_installer("--force")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("checksum", result.stderr)

    def test_unsafe_archive_members_and_duplicate_files_are_rejected(self):
        for name, kind in (
            ("../escaped", tarfile.REGTYPE),
            ("symlink", tarfile.SYMTYPE),
            ("bin/xdcode", tarfile.REGTYPE),
        ):
            with self.subTest(name=name):
                member = tarfile.TarInfo(name)
                member.type = kind
                if kind == tarfile.SYMTYPE:
                    member.linkname = "../../outside"
                self.pack_payload(member)
                result = self.run_installer("--force")
                self.assertNotEqual(result.returncode, 0)
                self.assertFalse(self.entry.exists())
                self.assertFalse((self.root / "escaped").exists())

    def test_missing_download_invalid_platform_and_bad_redirect(self):
        for environment in (
            {"MOCK_DOWNLOAD_FAIL": "1"},
            {"MOCK_OS": "Darwin", "MOCK_MACHINE": "x86_64"},
            {"MOCK_OS": "Linux", "MOCK_MACHINE": "aarch64"},
            {"MOCK_LATEST_URL": "https://example.invalid/releases/tag/" + self.tag},
            {
                "MOCK_LATEST_URL": "https://github.com/xxddccaa/xdcode/releases/tag/v0.167.0-../../outside"
            },
        ):
            with self.subTest(environment=environment):
                result = self.run_installer("--force", **environment)
                self.assertNotEqual(result.returncode, 0)
                self.assertFalse(self.entry.exists())

    def test_exact_release_and_proxy_are_forwarded(self):
        result = self.run_installer(
            "--force",
            "--release",
            self.tag,
            XDCODE_INSTALL_PROXY="http://127.0.0.1:10826",
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        calls = [
            json.loads(line)
            for line in (self.root / "curl.jsonl").read_text().splitlines()
        ]
        self.assertEqual(len(calls), 2)
        self.assertTrue(all("http://127.0.0.1:10826" in call for call in calls))

    def test_linux_executes_only_the_installer_in_the_verified_archive(self):
        self.archive = self.downloads / f"xdcode-{self.identity}-linux-x86_64.tar.gz"
        installer = self.root / "install.py"
        installer.write_text(
            'import json, os, pathlib, sys\npathlib.Path(os.environ["FIXTURE_ARGS"]).write_text(json.dumps(sys.argv[1:]))\n'
        )
        with tarfile.open(self.archive, "w:gz") as archive:
            archive.add(installer, arcname="install.py")
            metadata = self.root / "build-info.json"
            metadata.write_text(
                json.dumps(
                    {
                        "schema_version": 1,
                        "version": self.version,
                        "build_id": "g" + self.commit[:12],
                        "source_commit": self.commit,
                        "source_dirty": False,
                        "target": "x86_64-unknown-linux-gnu",
                    }
                )
            )
            archive.add(metadata, arcname="build-info.json")
        self.write_sidecar()
        result = self.run_installer("--force", MOCK_OS="Linux", MOCK_MACHINE="x86_64")
        self.assertEqual(result.returncode, 0, result.stderr)
        arguments = json.loads((self.root / "linux-arguments.json").read_text())
        self.assertEqual(arguments[0], "--force")
        self.assertTrue(arguments[1].endswith(self.archive.name))
        calls = (self.root / "curl.jsonl").read_text()
        self.assertNotIn(".install.py", calls)

    @unittest.skipUnless(
        platform.system() == "Darwin", "requires the actual macOS plutil"
    )
    def test_native_plutil_reads_json_fields(self):
        result = subprocess.run(
            [
                "/usr/bin/plutil",
                "-extract",
                "source_dirty",
                "raw",
                "-o",
                "-",
                str(self.payload / "build-info.json"),
            ],
            capture_output=True,
            text=True,
            check=True,
        )
        self.assertEqual(result.stdout.strip(), "false")

    @unittest.skipUnless(
        os.environ.get("XDCODE_TEST_PACKAGE_DIR"),
        "native artifact smoke test is enabled by CI",
    )
    def test_installs_the_actual_native_ci_artifact_without_touching_runtime_data(self):
        platform_name = (
            "macos-arm64" if platform.system() == "Darwin" else "linux-x86_64"
        )
        archives = list(
            Path(os.environ["XDCODE_TEST_PACKAGE_DIR"]).glob(
                f"xdcode-*-{platform_name}.tar.gz"
            )
        )
        self.assertEqual(len(archives), 1)
        with (
            tarfile.open(archives[0]) as archive,
            archive.extractfile("build-info.json") as stream,
        ):
            info = json.load(stream)
        identity = f"{info['version']}-{info['build_id']}"
        shutil.copyfile(archives[0], self.downloads / archives[0].name)
        shutil.copyfile(
            Path(str(archives[0]) + ".sha256"),
            self.downloads / (archives[0].name + ".sha256"),
        )
        result = self.run_installer(
            "--force",
            FIXTURE_TAG="v" + identity,
            MOCK_OS=platform.system(),
            MOCK_MACHINE=platform.machine(),
            MOCK_NATIVE_PACKAGE="1",
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertTrue(self.entry.resolve().is_file())
        self.assertEqual(
            self.sentinel.read_text(), "existing-runtime-data-must-be-retained\n"
        )
        version = subprocess.check_output(
            [str(self.entry), "--version"], env=self.environment, text=True
        )
        self.assertEqual(version.strip(), f"xdcode {info['version']}")


if __name__ == "__main__":
    unittest.main()
