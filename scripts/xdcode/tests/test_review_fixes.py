"""Regression checks for release provenance, runtime probes and Linux PID identity."""

from __future__ import annotations

import argparse
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest
from unittest import mock

SCRIPT_DIR = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(SCRIPT_DIR))
import build
import install
import package


class ReviewTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="xdcode-review-test-")
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.proc = self.root / "proc"
        self.process = self.proc / "123456789"
        self.process.mkdir(parents=True)
        (self.process / "cmdline").write_bytes(b"python3\0unrelated.py\0")
        (self.process / "exe").symlink_to("/usr/bin/python3")
        self.write_stat("S", 42)
        boot = self.proc / "sys/kernel/random/boot_id"
        boot.parent.mkdir(parents=True)
        boot.write_text("current-boot\n")
        self.runtime = self.root / "runtime"
        self.daemon_dir = self.runtime / "app-server-daemon"
        self.daemon_dir.mkdir(parents=True)
        self.record = {"pid": 123456789, "processStartTime": "legacy wall-clock value",
                       "processIdentity": {"bootId": "current-boot", "startTicks": 42}}

    def write_stat(self, state, ticks):
        (self.process / "stat").write_text(f"123456789 (name with ) parentheses) {state} " + "0 " * 18 + str(ticks))

    def test_all_current_and_legacy_pid_files_block_matching_process(self):
        for name in ("daemon.pid", "daemon-updater.pid", "app-server.pid", "app-server-updater.pid"):
            with self.subTest(name=name):
                path = self.daemon_dir / name
                path.write_text(json.dumps(self.record))
                self.assertEqual(install.active_processes(self.runtime, self.proc), [123456789])
                self.assertEqual(json.loads(path.read_text()), self.record)
                path.unlink()

    def test_native_pid_reuse_previous_boot_zombie_and_absent_process_are_stale(self):
        for alias in ("processIdentity", "linuxProcessIdentity"):
            with self.subTest(alias=alias):
                record = {"pid": 123456789, alias: {"bootId": "old-boot", "startTicks": 42}}
                with mock.patch.object(install, "read_process_stat", side_effect=AssertionError("prior boot needs no stat")):
                    self.assertFalse(install.pid_record_is_live(record, self.proc))
                with mock.patch.object(Path, "stat", side_effect=PermissionError):
                    self.assertFalse(install.pid_record_is_live(record, self.proc))
                record[alias] = {"bootId": "current-boot", "startTicks": 43}
                self.assertFalse(install.pid_record_is_live(record, self.proc))
        self.write_stat("Z", 42)
        self.assertFalse(install.pid_record_is_live(self.record, self.proc))
        self.assertFalse(install.pid_record_is_live({"pid": 99999999}, self.proc))

    def test_stale_record_is_retained_and_does_not_block_installer_gate(self):
        path = self.daemon_dir / "daemon.pid"
        record = {**self.record, "processIdentity": {"bootId": "old-boot", "startTicks": 42}}
        path.write_text(json.dumps(record))
        self.assertEqual(install.active_processes(self.runtime, self.proc), [])
        self.assertEqual(json.loads(path.read_text()), record)

    def test_ambiguous_identity_and_unreadable_stat_fail_closed(self):
        for identity in ({"bootId": "current-boot"}, {"boot_id": "current-boot", "start_ticks": 42},
                         {"bootId": "current-boot", "startTicks": True}, {"startTicks": 42}):
            with self.subTest(identity=identity), self.assertRaises(install.DeliveryError):
                install.pid_record_is_live({"pid": 123456789, "processIdentity": identity}, self.proc)
        with mock.patch.object(install, "read_process_stat", side_effect=PermissionError), self.assertRaises(install.DeliveryError):
            install.pid_record_is_live(self.record, self.proc)
        self.assertTrue(install.pid_record_is_live({"pid": 123456789, "processStartTime": "different clock"}, self.proc))

    def test_exact_version_rejects_prerelease_metadata_and_extra_lines(self):
        for value in ("xdcode 0.162.0-beta", "xdcode 0.162.0+other", "xdcode 0.162.0\nextra",
                      "prefix xdcode 0.162.0", "xdcode 0.162.00"):
            with self.subTest(value=value), mock.patch.object(install, "probe_executable", return_value=value):
                with self.assertRaisesRegex(install.DeliveryError, "exact upstream"):
                    package.check_version(self.root, "0.162.0")

    def test_skip_variable_presence_is_rejected_before_build(self):
        args = argparse.Namespace(upstream=build.REPO_ROOT / "upstream.json")
        for value in ("", "0", "false", "1"):
            with self.subTest(value=value), mock.patch.dict(os.environ, {"XDCODE_SKIP_BWRAP_BUILD": value}):
                with self.assertRaisesRegex(install.DeliveryError, "reject XDCODE_SKIP_BWRAP_BUILD"):
                    build.build(args)

    def test_external_bwrap_source_is_rejected_before_build(self):
        args = argparse.Namespace(upstream=build.REPO_ROOT / "upstream.json")
        with mock.patch.dict(os.environ, {"XDCODE_BWRAP_SOURCE_DIR": str(self.root)}):
            with self.assertRaisesRegex(install.DeliveryError, "external XDCODE_BWRAP_SOURCE_DIR"):
                build.build(args)

    def test_cargo_version_mismatch_is_rejected_before_build(self):
        upstream = self.root / "upstream.json"
        upstream.write_text(json.dumps({"version": "999.0.0", "upstream_tag": "rust-v999.0.0", "upstream_commit": "a" * 40}))
        with self.assertRaisesRegex(install.DeliveryError, "Cargo workspace package version"):
            build.build(argparse.Namespace(upstream=upstream))

    def test_stub_bwrap_probe_prevents_cli_build(self):
        args = argparse.Namespace(upstream=build.REPO_ROOT / "upstream.json", allow_dirty=False, build_id="probe-fail",
                                  output_dir=self.root / "output", target_dir=self.root / "target",
                                  cache_dir=self.root / "cache", cargo="cargo", strip_tool="strip", rg_bin=None)
        bwrap = args.target_dir / install.TARGET / "release/bwrap"
        bwrap.parent.mkdir(parents=True)
        shutil.copy("/bin/true", bwrap)
        identity = {"source_commit": "a" * 40, "source_dirty": False, "source_tree_sha256": "b" * 64}
        with mock.patch.object(build, "source_identity", return_value=identity), \
             mock.patch.object(build, "git", return_value=b""), \
             mock.patch.object(build, "resolve_xdcode_v8_cargo_env", return_value={}), \
             mock.patch.object(build, "run") as run:
            with self.assertRaisesRegex(install.DeliveryError, "bwrap --version"):
                build.build(args)
        self.assertEqual(run.call_count, 2)  # bwrap build and strip only; no CLI build.
        self.assertFalse(args.output_dir.exists())


class ProbeTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        if not shutil.which("cc"):
            raise unittest.SkipTest("cc is required for native probe fixtures")
        cls.temporary = tempfile.TemporaryDirectory(prefix="xdcode-probe-test-")
        cls.root = Path(cls.temporary.name)
        cls.executable = cls.root / "fixture"
        subprocess.run(["cc", str(SCRIPT_DIR / "tests/version_fixture.c"), "-o", str(cls.executable)], check=True)

    @classmethod
    def tearDownClass(cls):
        cls.temporary.cleanup()

    def test_probe_timeout_output_limit_and_private_errors(self):
        for option, message in (("--hang", "timed out"), ("--flood", "output limit"), ("--fail", "exit 7")):
            with self.subTest(option=option), self.assertRaisesRegex(install.DeliveryError, message) as raised:
                install.probe_executable(self.executable, option, self.root, timeout=0.2)
            self.assertNotIn("private diagnostic", str(raised.exception))

    def test_probe_does_not_inherit_credentials_or_loader_overrides(self):
        with mock.patch.dict(os.environ, {"XDCODE_API_KEY": "fixture-only", "LD_LIBRARY_PATH": "/nonexistent", "HTTP_PROXY": "fixture"}):
            self.assertEqual(install.probe_executable(self.executable, "--environment", self.root), "clean")


class BwrapHeaderTrackingTests(unittest.TestCase):
    def test_actual_build_script_watches_resolved_source_headers_without_cargo(self):
        rustc = shutil.which("rustc")
        if not rustc:
            self.skipTest("rustc is required for the direct build-script harness")
        with tempfile.TemporaryDirectory(prefix="xdcode-header-test-") as temp:
            root = Path(temp)
            executable = root / "build-script"
            subprocess.run([rustc, "--edition=2021", str(SCRIPT_DIR / "tests/bwrap_build_harness.rs"),
                            "-o", str(executable)], check=True, timeout=60)
            manifest = root / "bwrap"
            manifest.mkdir()
            vendor = root / "vendor/bubblewrap"
            vendor.mkdir(parents=True)
            external = root / "external"
            external.mkdir()
            output = root / "out"
            output.mkdir()
            for tree in (vendor, external):
                for header in ("utils.h", "network.h", "bind-mount.h"):
                    (tree / header).write_text("/* original */\n")
                environment = {**os.environ, "CARGO_MANIFEST_DIR": str(manifest), "OUT_DIR": str(output),
                               "CARGO_CFG_TARGET_OS": "linux"}
                environment.pop("XDCODE_SKIP_BWRAP_BUILD", None)
                environment.pop("XDCODE_BWRAP_SOURCE_DIR", None)
                if tree == external:
                    environment["XDCODE_BWRAP_SOURCE_DIR"] = str(external)
                directives = subprocess.check_output([str(executable)], env=environment, text=True)
                tracked = [Path(line.split("=", 1)[1]) for line in directives.splitlines()
                           if line.startswith("cargo:rerun-if-changed=")]
                self.assertIn(tree.resolve(), tracked)
                for header in ("utils.h", "network.h", "bind-mount.h"):
                    path = tree / header
                    before = path.stat().st_mtime_ns
                    path.write_text("/* changed header */\n")
                    os.utime(path, ns=(before + 1_000_000_000, before + 1_000_000_000))
                    self.assertTrue(any(path == entry or (entry.is_dir() and path.is_relative_to(entry)) for entry in tracked))
                    self.assertGreater(path.stat().st_mtime_ns, before)
