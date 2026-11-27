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
PLATFORMS = {
    "linux-x86_64": ("Linux", "x86_64"),
    "macos-arm64": ("Darwin", "arm64"),
}


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
        for directory in (self.home / ".xdcode", self.downloads, self.tools, self.scratch):
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
            PYTHON_MARKER=str(self.root / "python-used"),
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
        # The installer must never need Python on the target machine.
        for name in ("python", "python3", "python3.11"):
            self.make_tool(
                name,
                'import os, pathlib, sys\npathlib.Path(os.environ["PYTHON_MARKER"]).touch()\nsys.exit(1)\n',
            )
        self.use_platform("linux-x86_64")
        self.make_release("0.172.0", "a" * 40)

    def make_tool(self, name: str, body: str):
        tool = self.tools / name
        tool.write_text(f"#!{sys.executable}\n" + body)
        tool.chmod(0o755)

    def use_platform(self, platform_name: str):
        self.platform = platform_name
        system, machine = PLATFORMS[platform_name]
        self.environment.update(MOCK_OS=system, MOCK_MACHINE=machine)

    def make_release(self, version: str, commit: str, *, reported: str | None = None, extra=None):
        self.version = version
        self.identity = f"{version}-g{commit[:12]}"
        self.tag = "v" + self.identity
        self.environment["FIXTURE_TAG"] = self.tag
        payload = self.root / f"payload-{self.identity}"
        shutil.rmtree(payload, ignore_errors=True)
        (payload / "bin").mkdir(parents=True)
        binary = payload / "bin/xdcode"
        binary.write_text(f"#!/bin/sh\nprintf '%s\\n' '{reported or 'xdcode ' + version}'\n")
        binary.chmod(0o755)
        (payload / "build-info.json").write_text(json.dumps({"version": version}))
        self.archive = self.downloads / f"xdcode-{self.identity}-{self.platform}.tar.gz"
        with tarfile.open(self.archive, "w:gz") as archive:
            for path in sorted(payload.rglob("*")):
                archive.add(path, arcname=path.relative_to(payload), recursive=False)
            if extra is not None:
                archive.addfile(extra)
        self.write_sidecar()

    def write_sidecar(self, contents: str | None = None):
        Path(str(self.archive) + ".sha256").write_text(
            contents
            or f"{hashlib.sha256(self.archive.read_bytes()).hexdigest()}  {self.archive.name}\n"
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

    def curl_calls(self):
        log = self.root / "curl.jsonl"
        return [json.loads(line) for line in log.read_text().splitlines()] if log.exists() else []

    def assert_clean_state(self):
        self.assertEqual(self.sentinel.read_text(), "existing-runtime-data-must-be-retained\n")
        self.assertEqual(list(self.scratch.iterdir()), [])
        leftovers = [
            path.name
            for parent in (self.home / ".local/lib/xdcode", self.home / ".local/bin")
            if parent.exists()
            for path in parent.iterdir()
            if path.name.startswith(".")
        ]
        self.assertEqual(leftovers, [])
        self.assertFalse(Path(self.environment["PYTHON_MARKER"]).exists(), "installer invoked Python")

    def test_help_and_syntax_need_no_download(self):
        subprocess.run(["bash", "-n", str(INSTALLER)], check=True)
        result = self.run_installer("--help")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("upgrade", result.stdout)
        self.assertEqual(self.curl_calls(), [])

    def test_install_repeat_and_upgrade_on_each_platform(self):
        for platform_name in PLATFORMS:
            with self.subTest(platform=platform_name):
                self.setUp()
                self.use_platform(platform_name)
                self.make_release("0.172.0", "a" * 40)
                first = self.run_installer()
                self.assertEqual(first.returncode, 0, first.stderr)
                installed = self.entry.resolve()
                self.assertEqual(
                    installed, self.home / f".local/lib/xdcode/{self.identity}/bin/xdcode"
                )
                self.assertEqual(
                    subprocess.check_output([str(self.entry)], text=True).strip(),
                    "xdcode 0.172.0",
                )
                calls = len(self.curl_calls())
                repeated = self.run_installer()
                self.assertEqual(repeated.returncode, 0, repeated.stderr)
                self.assertIn("already installed", repeated.stdout)
                self.assertEqual(len(self.curl_calls()), calls + 1)  # only the latest lookup
                self.make_release("0.173.0", "b" * 40)
                upgraded = self.run_installer()
                self.assertEqual(upgraded.returncode, 0, upgraded.stderr)
                self.assertIn(self.identity, str(self.entry.resolve()))
                self.assertTrue(installed.is_file(), "previous build must be kept")
                self.assert_clean_state()

    def test_old_force_flag_is_accepted(self):
        result = self.run_installer("--force")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertTrue(self.entry.resolve().is_file())

    def test_switching_back_to_a_downloaded_release_needs_no_download(self):
        self.assertEqual(self.run_installer().returncode, 0)
        old_tag = self.tag
        self.make_release("0.173.0", "b" * 40)
        self.assertEqual(self.run_installer().returncode, 0)
        before = len(self.curl_calls())
        result = self.run_installer("--release", old_tag)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn(old_tag.removeprefix("v"), str(self.entry.resolve()))
        self.assertEqual(len(self.curl_calls()), before)

    def test_unmanaged_entry_is_retained(self):
        self.entry.parent.mkdir(parents=True)
        self.entry.write_text("retain-unmanaged-entry")
        result = self.run_installer()
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(self.entry.read_text(), "retain-unmanaged-entry")

    def test_archive_checksum_and_filename_binding(self):
        for contents in (
            f"{'0' * 64}  {self.archive.name}\n",
            f"{hashlib.sha256(self.archive.read_bytes()).hexdigest()}  unrelated.tar.gz\n",
        ):
            with self.subTest(contents=contents):
                self.write_sidecar(contents)
                result = self.run_installer()
                self.assertNotEqual(result.returncode, 0)
                self.assertFalse(self.entry.exists())
                self.assertFalse((self.home / f".local/lib/xdcode/{self.identity}").exists())

    def test_package_reporting_another_version_is_rejected(self):
        self.make_release("0.172.0", "a" * 40, reported="xdcode 0.165.0")
        result = self.run_installer()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("expected 'xdcode 0.172.0'", result.stderr)
        self.assertFalse(self.entry.exists())
        self.assertFalse((self.home / f".local/lib/xdcode/{self.identity}").exists())

    def test_unsafe_archive_members_are_rejected(self):
        for name, kind in (("../escaped", tarfile.REGTYPE), ("symlink", tarfile.SYMTYPE)):
            with self.subTest(name=name):
                member = tarfile.TarInfo(name)
                member.type = kind
                if kind == tarfile.SYMTYPE:
                    member.linkname = "../../outside"
                self.make_release("0.172.0", "a" * 40, extra=member)
                result = self.run_installer()
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
                "MOCK_LATEST_URL": "https://github.com/xxddccaa/xdcode/releases/tag/v0.172.0-../../outside"
            },
        ):
            with self.subTest(environment=environment):
                result = self.run_installer(**environment)
                self.assertNotEqual(result.returncode, 0)
                self.assertFalse(self.entry.exists())

    def test_exact_release_and_proxy_are_forwarded(self):
        result = self.run_installer(
            "--release", self.tag, XDCODE_INSTALL_PROXY="http://127.0.0.1:10826"
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        calls = self.curl_calls()
        self.assertEqual(len(calls), 2)
        self.assertTrue(all("http://127.0.0.1:10826" in call for call in calls))

    @unittest.skipUnless(
        os.environ.get("XDCODE_TEST_PACKAGE_DIR"),
        "native artifact smoke test is enabled by CI",
    )
    def test_installs_the_actual_native_ci_artifact_without_touching_runtime_data(self):
        platform_name = "macos-arm64" if platform.system() == "Darwin" else "linux-x86_64"
        archives = list(
            Path(os.environ["XDCODE_TEST_PACKAGE_DIR"]).glob(f"xdcode-*-{platform_name}.tar.gz")
        )
        self.assertEqual(len(archives), 1)
        with tarfile.open(archives[0]) as archive, archive.extractfile("build-info.json") as stream:
            info = json.load(stream)
        identity = f"{info['version']}-{info['build_id']}"
        shutil.copyfile(archives[0], self.downloads / archives[0].name)
        shutil.copyfile(
            Path(str(archives[0]) + ".sha256"),
            self.downloads / (archives[0].name + ".sha256"),
        )
        result = self.run_installer(
            FIXTURE_TAG="v" + identity,
            MOCK_OS=platform.system(),
            MOCK_MACHINE=platform.machine(),
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertTrue(self.entry.resolve().is_file())
        self.assert_clean_state()
        version = subprocess.check_output(
            [str(self.entry), "--version"], env=self.environment, text=True
        )
        self.assertEqual(version.strip(), f"xdcode {info['version']}")


if __name__ == "__main__":
    unittest.main()
