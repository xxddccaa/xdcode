"""Native fixture package/installer tests; no model access or Cargo/V8 build."""

from __future__ import annotations

import argparse
import copy
import io
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tarfile
import tempfile
import unittest
from unittest import mock

SCRIPT_DIR = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(SCRIPT_DIR))

import build
import install
import package


UPSTREAM = {
    "version": "0.162.0",
    "upstream_tag": "rust-v0.162.0",
    "upstream_commit": "2351d9e1b608e6f9d9a3699b71d7eb39ee41cfa4",
}


def write_receipt(root: Path, build_id: str = "personal1") -> dict:
    info = {
        "schema_version": 1,
        "version": UPSTREAM["version"],
        "build_id": build_id,
        "target": install.TARGET,
        "upstream": UPSTREAM,
        "source_commit": "a" * 40,
        "source_tree_sha256": "b" * 64,
        "source_dirty": False,
        "requires_glibc": "2.17",
        "source_date_epoch": 1700000000,
        "binary_sha256": {
            name: install.sha256_file(root / name) for name in install.BINARIES
        },
        "bwrap_sha256": install.sha256_file(root / "xdcode-resources/bwrap"),
        "bwrap_source": "xdcode-rs/vendor/bubblewrap",
        "modifications_sha256": install.sha256_file(
            package.REPO_ROOT / "MODIFICATIONS.md"
        ),
    }
    install.write_json(root / install.BUILD_INFO, info)
    return info


class DeliveryTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        if not shutil.which("cc"):
            raise unittest.SkipTest("cc is required for the small native ELF fixture")
        cls.fixture_temp = tempfile.TemporaryDirectory(prefix="xdcode-fixture-")
        cls.fixture = Path(cls.fixture_temp.name) / "fixture"
        subprocess.run(
            ["cc", str(SCRIPT_DIR / "tests/version_fixture.c"), "-o", str(cls.fixture)],
            check=True,
        )

    @classmethod
    def tearDownClass(cls):
        cls.fixture_temp.cleanup()

    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="xdcode-delivery-test-")
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.build_dir = self.root / "build"
        for name in install.BINARIES:
            path = self.build_dir / name
            path.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(self.fixture, path)
            path.chmod(0o755)
        self.info = write_receipt(self.build_dir)
        # These native fixtures are not Cargo artifacts; supply their test receipt identity.
        source_mock = mock.patch.object(
            package,
            "source_identity",
            return_value={
                name: self.info[name]
                for name in ("source_commit", "source_dirty", "source_tree_sha256")
            },
        )
        source_mock.start()
        self.addCleanup(source_mock.stop)
        self.archive = package.package(self.build_dir, self.root / "dist")
        self.install_root = self.root / "local/lib/xdcode"
        self.bin_dir = self.root / "local/bin"
        self.data_home = self.root / "runtime"
        self.data_home.mkdir()
        (self.data_home / "keep.txt").write_text("user-owned data")
        # Fixture installs must not depend on unrelated sessions on the development host.
        # Processes launched by each test are still observed by the real inspector below.
        self.existing_host_processes = set(install.active_processes(self.data_home))

    def install_archive(self, archive=None, **kwargs):
        inspect_processes = install.active_processes
        def fixture_processes(home):
            return [pid for pid in inspect_processes(home) if pid not in self.existing_host_processes]
        with mock.patch.object(install, "active_processes", side_effect=fixture_processes):
            return install.install(
                archive or self.archive,
                self.install_root,
                self.bin_dir,
                self.data_home,
                **kwargs,
            )

    def mutate_archive(self, mutate):
        with tarfile.open(self.archive, "r:gz") as archive:
            contents = [
                (copy.copy(member), archive.extractfile(member).read())
                for member in archive.getmembers()
            ]
        contents = mutate(contents)
        with tarfile.open(self.archive, "w:gz") as archive:
            for member, contents in contents:
                if member.isfile():
                    member.size = len(contents)
                archive.addfile(
                    member, io.BytesIO(contents) if member.isfile() else None
                )
        Path(str(self.archive) + ".sha256").write_text(
            f"{install.sha256_file(self.archive)}  {self.archive.name}\n"
        )

    def test_roundtrip_layout_version_and_offline_entrypoint(self):
        with mock.patch(
            "socket.socket", side_effect=AssertionError("network is forbidden")
        ):
            dest = self.install_archive()
        self.assertEqual(dest.name, "0.162.0-personal1")
        self.assertEqual((self.bin_dir / "xdcode").resolve(), dest / "bin/xdcode")
        self.assertEqual(
            subprocess.check_output(
                [str(self.bin_dir / "xdcode"), "--version"], text=True
            ).strip(),
            "xdcode 0.162.0",
        )
        manifest = json.loads((dest / install.MANIFEST).read_text())
        self.assertEqual(manifest["version"], "0.162.0")
        self.assertEqual(manifest["resourcesDir"], "xdcode-resources")
        self.assertEqual(
            install.sha256_file(dest / "MODIFICATIONS.md"),
            self.info["modifications_sha256"],
        )
        self.assertEqual(
            (dest / "xdcode-resources/bwrap").read_bytes(),
            (self.build_dir / "xdcode-resources/bwrap").read_bytes(),
        )
        self.assertEqual((self.data_home / "keep.txt").read_text(), "user-owned data")

    def test_standalone_distributed_installer_cli(self):
        if self.existing_host_processes:
            self.skipTest("standalone installer requires a host without active xdcode sessions")
        installer = package.output_dir_installer(self.archive)
        result = subprocess.run(
            [
                sys.executable,
                str(installer),
                str(self.archive),
                "--install-root",
                str(self.install_root),
                "--bin-dir",
                str(self.bin_dir),
                "--xdcode-home",
                str(self.data_home),
            ],
            capture_output=True,
            text=True,
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("Old builds and runtime data retained", result.stdout)

    def test_upgrade_retains_old_build_and_other_commands(self):
        old = self.install_archive()
        for command in ("codex", "sc"):
            (self.bin_dir / command).write_text(f"original {command}")
        write_receipt(self.build_dir, "personal2")
        second = package.package(self.build_dir, self.root / "dist")
        new = self.install_archive(second)
        self.assertTrue(old.is_dir())
        self.assertNotEqual(old, new)
        self.assertEqual((self.bin_dir / "xdcode").resolve(), new / "bin/xdcode")
        for command in ("codex", "sc"):
            self.assertEqual(
                (self.bin_dir / command).read_text(), f"original {command}"
            )
        with self.assertRaisesRegex(install.DeliveryError, "already installed"):
            self.install_archive(second)

    def test_repack_is_deterministic_and_does_not_overwrite(self):
        second = package.package(self.build_dir, self.root / "second")
        self.assertEqual(install.sha256_file(second), install.sha256_file(self.archive))
        with self.assertRaisesRegex(install.DeliveryError, "already exists"):
            package.package(self.build_dir, self.root / "dist")

    def test_packager_rejects_changed_bwrap_and_wrong_version(self):
        with (self.build_dir / "xdcode-resources/bwrap").open("ab") as out:
            out.write(b"changed after embedding")
        with self.assertRaisesRegex(install.DeliveryError, "differs from completed"):
            package.package(self.build_dir, self.root / "bad")
        with self.assertRaisesRegex(install.DeliveryError, "does not report"):
            package.check_version(self.build_dir, "0.163.0")

    def test_bad_archive_digest_does_not_create_entry(self):
        with self.assertRaisesRegex(install.DeliveryError, "archive checksum mismatch"):
            self.install_archive(sha256="0" * 64)
        self.assertFalse(self.install_root.exists())

    def test_checksum_is_required_and_filename_is_bound(self):
        sidecar = Path(str(self.archive) + ".sha256")
        sidecar.unlink()
        with self.assertRaisesRegex(install.DeliveryError, "sidecar"):
            self.install_archive()
        renamed = self.archive.with_name("xdcode-0.162.0-other-linux-x86_64.tar.gz")
        shutil.copyfile(self.archive, renamed)
        with self.assertRaisesRegex(install.DeliveryError, "filename"):
            self.install_archive(renamed, sha256=install.sha256_file(renamed))
        self.assertFalse((self.bin_dir / "xdcode").exists())

    def test_internal_checksum_detects_payload_tampering(self):
        self.mutate_archive(
            lambda contents: [
                (m, b"tampered" if m.name == "NOTICE" else b) for m, b in contents
            ]
        )
        with self.assertRaisesRegex(
            install.DeliveryError, "package checksum mismatch: NOTICE"
        ):
            self.install_archive()

    def test_archive_links_traversal_duplicates_and_special_files(self):
        original = self.archive.read_bytes()
        for name, kind in (
            ("../escape", tarfile.REGTYPE),
            ("/absolute", tarfile.REGTYPE),
            ("bin/xdcode", tarfile.SYMTYPE),
            ("extra", tarfile.FIFOTYPE),
            ("bin/xdcode", tarfile.REGTYPE),
            ("bin/alias", tarfile.LNKTYPE),
        ):
            with self.subTest(name=name, kind=kind):
                self.archive.write_bytes(original)

                def mutate(contents):
                    member = tarfile.TarInfo(name)
                    member.type = kind
                    member.linkname = "../../escape"
                    return [*contents, (member, b"bad")]

                self.mutate_archive(mutate)
                with self.assertRaises(install.DeliveryError):
                    self.install_archive()
                self.assertFalse((self.bin_dir / "xdcode").exists())
                self.assertFalse((self.root / "escape").exists())

    def test_symlinked_file_and_setuid_mode_are_rejected(self):
        original = self.archive.read_bytes()
        for kind in (tarfile.SYMTYPE, tarfile.LNKTYPE, tarfile.REGTYPE):
            self.archive.write_bytes(original)

            def mutate(contents):
                for member, _ in contents:
                    if member.name == "bin/xdcode":
                        member.type = kind
                        member.linkname = "/bin/true"
                        member.mode = 0o4755
                return contents

            self.mutate_archive(mutate)
            with self.assertRaises(install.DeliveryError):
                self.install_archive()

    def test_unsafe_targets_and_unmanaged_entries(self):
        for path in (
            Path("/"),
            Path("/usr/lib/xdcode"),
            self.root / "sc/lib/xdcode",
            self.root / "bad/../xdcode",
        ):
            with self.subTest(path=path), self.assertRaises(install.DeliveryError):
                install.install(self.archive, path, self.bin_dir, self.data_home)
        self.bin_dir.mkdir(parents=True)
        entry = self.bin_dir / "xdcode"
        entry.write_text("do not replace")
        with self.assertRaisesRegex(install.DeliveryError, "non-managed"):
            self.install_archive()
        self.assertEqual(entry.read_text(), "do not replace")
        entry.unlink()
        entry.symlink_to("/usr/bin/true")
        with self.assertRaises(install.DeliveryError):
            self.install_archive()
        self.assertEqual(os.readlink(entry), "/usr/bin/true")

    def test_hidden_application_data_is_not_an_installation_target(self):
        for directory in (".other-agent", ".private-data"):
            for install_root, bin_dir in (
                (self.root / directory / "lib/xdcode", self.bin_dir),
                (self.install_root, self.root / directory / "bin"),
            ):
                with self.subTest(directory=directory, install_root=install_root, bin_dir=bin_dir):
                    with self.assertRaisesRegex(install.DeliveryError, "another CLI"):
                        install.validate_targets(install_root, bin_dir, self.data_home)
        local_root = self.root / ".local/lib/xdcode"
        local_bin = self.root / ".local/bin"
        self.assertEqual(
            install.validate_targets(local_root, local_bin, self.data_home),
            (local_root, local_bin, self.data_home),
        )

    def test_symlink_parent_and_runtime_overlap_are_rejected(self):
        actual = self.root / "actual"
        actual.mkdir()
        (self.root / "linked").symlink_to(actual, target_is_directory=True)
        with self.assertRaisesRegex(install.DeliveryError, "symlink"):
            install.install(
                self.archive,
                self.root / "linked/lib/xdcode",
                self.bin_dir,
                self.data_home,
            )
        with self.assertRaisesRegex(install.DeliveryError, "runtime data"):
            install.install(
                self.archive,
                self.data_home / "lib/xdcode",
                self.bin_dir,
                self.data_home,
            )

    def test_live_native_xdcode_refuses_installation(self):
        active_binary = self.root / "xdcode"
        shutil.copyfile("/bin/sleep", active_binary)
        active_binary.chmod(0o755)
        child = subprocess.Popen([str(active_binary), "60"])
        try:
            with self.assertRaisesRegex(install.DeliveryError, "active xdcode"):
                self.install_archive()
            self.assertIsNone(child.poll())
            self.assertFalse(self.install_root.exists())
        finally:
            child.terminate()
            child.wait(timeout=5)

    def test_live_daemon_pid_record_refuses_installation(self):
        daemon = self.data_home / "app-server-daemon"
        daemon.mkdir()
        record = daemon / "app-server.pid"
        record.write_text(json.dumps({"pid": os.getpid()}))
        with self.assertRaisesRegex(install.DeliveryError, "active xdcode"):
            self.install_archive()
        self.assertTrue(record.is_file())
        record.write_text("malformed")
        with self.assertRaisesRegex(install.DeliveryError, "daemon state"):
            self.install_archive()

    def test_install_lock_prevents_concurrent_switch(self):
        self.install_root.mkdir(parents=True)
        with install.install_lock(self.install_root):
            with self.assertRaisesRegex(install.DeliveryError, "in progress"):
                self.install_archive()

    def test_late_activity_does_not_switch_or_delete_old_build(self):
        old = self.install_archive()
        write_receipt(self.build_dir, "personal2")
        second = package.package(self.build_dir, self.root / "dist")
        with mock.patch.object(
            install,
            "require_idle",
            side_effect=[None, None, install.DeliveryError("active xdcode")],
        ):
            with self.assertRaisesRegex(install.DeliveryError, "active xdcode"):
                self.install_archive(second)
        self.assertEqual((self.bin_dir / "xdcode").resolve(), old / "bin/xdcode")
        self.assertTrue((self.install_root / "0.162.0-personal2").is_dir())

    def test_manifest_and_receipt_validation(self):
        for field, value in (
            ("version", "0.1.0"),
            ("build_id", "../../sc"),
            ("target", "aarch64-unknown-linux-gnu"),
            ("bwrap_sha256", "0" * 64),
            ("build_id", None),
            ("requires_glibc", 2.17),
            ("source_commit", 1),
            ("modifications_sha256", None),
        ):
            info = copy.deepcopy(self.info)
            info[field] = value
            with self.subTest(field=field), self.assertRaises(install.DeliveryError):
                install.validate_build_info(info)
        with self.assertRaises(install.DeliveryError):
            install.validate_upstream({**UPSTREAM, "upstream_tag": "rust-v0.1.0"})

    def test_change_notices_are_bound_to_build_receipt(self):
        info = copy.deepcopy(self.info)
        info["modifications_sha256"] = "0" * 64
        install.write_json(self.build_dir / install.BUILD_INFO, info)
        with self.assertRaisesRegex(install.DeliveryError, "change notices differ"):
            package.package(self.build_dir, self.root / "wrong-notices")

    def test_packager_refuses_source_drift(self):
        with mock.patch.object(
            package, "source_identity", return_value={"source_tree_sha256": "c" * 64}
        ):
            with self.assertRaisesRegex(install.DeliveryError, "source differs"):
                package.package(self.build_dir, self.root / "drift")

    def test_unsupported_glibc_and_wrong_architecture_fail(self):
        with mock.patch.object(install, "host_glibc", return_value="2.16"):
            with self.assertRaisesRegex(install.DeliveryError, "requires glibc"):
                self.install_archive()
        path = self.build_dir / "bin/xdcode-code-mode-host"
        contents = bytearray(path.read_bytes())
        contents[18:20] = b"\xb7\x00"  # AArch64, not x86_64.
        path.write_bytes(contents)
        write_receipt(self.build_dir)
        with self.assertRaisesRegex(install.DeliveryError, "x86_64 ELF"):
            package.package(self.build_dir, self.root / "wrong-arch")

    def test_proc_inspection_checks_identity_not_prompt_text(self):
        proc = self.root / "proc"
        process = proc / "123456789"
        process.mkdir(parents=True)
        (process / "cmdline").write_bytes(b"/usr/bin/python3\0-c\0print('xdcode')\0")
        (process / "stat").write_text("123456789 (python) S " + "0 " * 18 + "42")
        (process / "exe").symlink_to("/usr/bin/python3")
        self.assertEqual(install.active_processes(self.data_home, proc), [])
        (process / "exe").unlink()
        (process / "exe").symlink_to("/tmp/xdcode (deleted)")
        self.assertEqual(install.active_processes(self.data_home, proc), [123456789])

    def test_missing_native_loader_does_not_activate_new_build(self):
        old = self.install_archive()
        helper = self.build_dir / "bin/xdcode-code-mode-host"
        subprocess.run(
            [
                "cc",
                str(SCRIPT_DIR / "tests/version_fixture.c"),
                "-Wl,--dynamic-linker,/nonexistent/xdcode-test-loader",
                "-o",
                str(helper),
            ],
            check=True,
        )
        write_receipt(self.build_dir, "unloadable")
        archive = package.package(self.build_dir, self.root / "dist")
        with self.assertRaisesRegex(
            install.DeliveryError, "could not load/finish: xdcode-code-mode-host"
        ):
            self.install_archive(archive)
        self.assertEqual((self.bin_dir / "xdcode").resolve(), old / "bin/xdcode")
        self.assertFalse((self.install_root / "0.162.0-unloadable").exists())
        self.assertEqual((self.data_home / "keep.txt").read_text(), "user-owned data")


class BuildTests(unittest.TestCase):
    def test_bwrap_is_finalized_and_stamped_before_cli_build(self):
        with tempfile.TemporaryDirectory(prefix="xdcode-build-test-") as temp:
            root = Path(temp)
            upstream_path = root / "upstream.json"
            # The build validates against the real workspace version; package
            # fixtures above deliberately keep their own independent version.
            install.write_json(
                upstream_path,
                json.loads((build.REPO_ROOT / "upstream.json").read_text()),
            )
            args = argparse.Namespace(
                upstream=upstream_path,
                allow_dirty=False,
                build_id="test1",
                output_dir=root / "output",
                target_dir=root / "target",
                cache_dir=root / "cache",
                cargo="cargo",
                strip_tool="strip",
                rg_bin=root / "rg",
            )
            identity = {
                "source_commit": "a" * 40,
                "source_dirty": False,
                "source_tree_sha256": "b" * 64,
            }
            calls = []

            def run(command, *, env, cwd):
                calls.append((command, env.copy()))
                if command[0] == "cargo":
                    for index, value in enumerate(command):
                        if value == "--bin":
                            output = (
                                args.target_dir
                                / install.TARGET
                                / "release"
                                / command[index + 1]
                            )
                            output.parent.mkdir(parents=True, exist_ok=True)
                            shutil.copyfile("/bin/true", output)
                elif command[-1].endswith("/bwrap"):
                    with Path(command[-1]).open("ab") as file:
                        file.write(b"finalized")

            shutil.copyfile("/bin/true", args.rg_bin)
            args.rg_bin.chmod(0o755)
            with (
                mock.patch.object(build, "source_identity", return_value=identity),
                mock.patch.object(build, "git", return_value=b"1700000000"),
                mock.patch.object(
                    build, "resolve_xdcode_v8_cargo_env", return_value={}
                ) as v8,
                mock.patch.object(build, "probe_bwrap") as probe,
                mock.patch.object(build, "run", side_effect=run),
            ):
                output = build.build(args)
            self.assertEqual(calls[0][0][-2:], ["--bin", "bwrap"])
            self.assertNotIn("XDCODE_BWRAP_SHA256", calls[0][1])
            self.assertEqual(calls[1][0][0], "strip")
            cli, environment = calls[2]
            self.assertEqual(
                cli[-4:], ["--bin", "xdcode", "--bin", "xdcode-code-mode-host"]
            )
            self.assertIn("--locked", cli)
            self.assertEqual(
                environment["XDCODE_BWRAP_SHA256"],
                install.sha256_file(output / "xdcode-resources/bwrap"),
            )
            self.assertEqual(environment["STABLE_GIT_COMMIT"], "a" * 40)
            self.assertEqual(environment["RUSTUP_TOOLCHAIN"], "1.99.1")
            self.assertEqual(
                environment["XDCODE_BWRAP_SOURCE_DIR"],
                str(build.REPO_ROOT / "xdcode-rs/vendor/bubblewrap"),
            )
            probe.assert_called_once_with(
                args.target_dir / install.TARGET / "release/bwrap",
                args.cache_dir / "tmp",
            )
            self.assertEqual(v8.call_args.kwargs["cache_root"], args.cache_dir)
            self.assertEqual(environment["TMPDIR"], str(args.cache_dir / "tmp"))
            self.assertEqual(
                json.loads((output / install.BUILD_INFO).read_text())["version"],
                json.loads(upstream_path.read_text())["version"],
            )

    def test_source_digest_tracks_uncommitted_content(self):
        with tempfile.TemporaryDirectory(prefix="xdcode-source-test-") as temp:
            root = Path(temp)
            subprocess.run(["git", "init", "-q", str(root)], check=True)
            (root / "source.txt").write_text("first")
            subprocess.run(["git", "-C", str(root), "add", "source.txt"], check=True)
            subprocess.run(
                [
                    "git",
                    "-C",
                    str(root),
                    "-c",
                    "user.name=Fixture",
                    "-c",
                    "user.email=fixture@example.invalid",
                    "commit",
                    "-qm",
                    "fixture",
                ],
                check=True,
            )
            initial = build.source_identity(root)
            self.assertFalse(initial["source_dirty"])
            (root / "source.txt").write_text("second")
            changed = build.source_identity(root)
            self.assertTrue(changed["source_dirty"])
            self.assertEqual(initial["source_commit"], changed["source_commit"])
            self.assertNotEqual(
                initial["source_tree_sha256"], changed["source_tree_sha256"]
            )


if __name__ == "__main__":
    unittest.main()
