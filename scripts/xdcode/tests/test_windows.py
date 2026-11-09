"""Windows package architecture, dependency and PowerShell installer checks."""

from __future__ import annotations

import hashlib
import importlib.util
import json
import os
import shutil
import struct
import subprocess
import tempfile
import unittest
import zipfile
from pathlib import Path
from unittest.mock import patch

REPO_ROOT = Path(__file__).resolve().parents[3]
SPEC = importlib.util.spec_from_file_location(
    "xdcode_windows", REPO_ROOT / "scripts/xdcode/windows.py"
)
windows = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(windows)
POWERSHELL = os.environ.get("XDCODE_TEST_PWSH") or shutil.which("pwsh")
RUSTC = shutil.which("rustc")


def pe_fixture(machine=0x8664, magic=0x20B):
    data = bytearray(154)
    data[:2] = b"MZ"
    struct.pack_into("<I", data, 0x3C, 128)
    data[128:132] = b"PE\0\0"
    struct.pack_into("<H", data, 132, machine)
    struct.pack_into("<H", data, 152, magic)
    return bytes(data)


class WindowsPackageTests(unittest.TestCase):
    def test_accepts_x64_pe32_plus_and_rejects_other_executables(self):
        with tempfile.TemporaryDirectory() as temporary:
            binary = Path(temporary) / "xdcode.exe"
            binary.write_bytes(pe_fixture())
            windows.require_x64_pe(binary)
            for content in (
                b"",
                b"\x7fELF",
                pe_fixture(0xAA64),
                pe_fixture(0x14C),
                pe_fixture(magic=0x10B),
                pe_fixture()[:100],
            ):
                with self.subTest(content=content[:8]):
                    binary.write_bytes(content)
                    with self.assertRaises(windows.DeliveryError):
                        windows.require_x64_pe(binary)

    def test_rejects_out_of_bounds_pe_offset(self):
        with tempfile.TemporaryDirectory() as temporary:
            binary = Path(temporary) / "xdcode.exe"
            content = bytearray(pe_fixture())
            struct.pack_into("<I", content, 0x3C, 0xFFFFFFFF)
            binary.write_bytes(content)
            with self.assertRaisesRegex(windows.DeliveryError, "truncated"):
                windows.require_x64_pe(binary)

    def test_dll_dependencies_must_be_system_or_api_set_libraries(self):
        with tempfile.TemporaryDirectory() as temporary:
            system = Path(temporary)
            (system / "KERNEL32.DLL").touch()
            output = (
                "dependencies:\n    KERNEL32.dll\n    api-ms-win-core-file-l1-1-0.dll\n"
            )
            with patch.object(windows.subprocess, "check_output", return_value=output):
                dependencies = windows.check_dependencies(Path("xdcode.exe"), system)
                self.assertIn("KERNEL32.DLL", dependencies)
            for dependency in ("developer-only.dll", "VCRUNTIME140D.dll"):
                with (
                    patch.object(
                        windows.subprocess,
                        "check_output",
                        return_value=f"    {dependency}\n",
                    ),
                    self.assertRaises(windows.DeliveryError),
                ):
                    windows.check_dependencies(Path("xdcode.exe"), system)

    def test_sandbox_provisioning_helper_is_not_executed_by_native_probes(self):
        with patch.object(windows.subprocess, "run") as run:
            run.return_value.stdout = "xdcode 0.167.0"
            windows.probe(Path("package"), "0.167.0")
        invoked = [str(call.args[0][0]) for call in run.call_args_list]
        self.assertEqual(len(invoked), 3)
        self.assertFalse(
            any(
                "sandbox-setup" in executable or "command-runner" in executable
                for executable in invoked
            )
        )


@unittest.skipUnless(RUSTC, "Rust is required for native launcher checks")
class WindowsLauncherTests(unittest.TestCase):
    def test_local_selection_and_arguments_are_forwarded_without_a_shell(self):
        with tempfile.TemporaryDirectory(prefix="xdcode-launcher-test-") as temporary:
            root = Path(temporary).resolve()
            bin_directory = root / "bin"
            bin_directory.mkdir()
            identity = "0.167.0-gaaaaaaaaaaaa"
            child_directory = root / "builds" / identity / "bin"
            child_directory.mkdir(parents=True)
            entry = bin_directory / "xdcode.exe"
            subprocess.run(
                [
                    RUSTC,
                    "--edition=2021",
                    str(REPO_ROOT / "scripts/xdcode/windows-launcher.rs"),
                    "-o",
                    str(entry),
                ],
                check=True,
                capture_output=True,
            )
            child_source = root / "child.rs"
            child_source.write_text(
                'fn main() { for argument in std::env::args().skip(1) { println!("{argument}"); } std::process::exit(23); }\n'
            )
            subprocess.run(
                [
                    RUSTC,
                    "--edition=2021",
                    str(child_source),
                    "-o",
                    str(child_directory / "xdcode.exe"),
                ],
                check=True,
                capture_output=True,
            )
            (bin_directory / "current.txt").write_text(identity)
            arguments = [
                "literal & not-a-command",
                "%XDCODE_API_KEY%",
                'quotes "stay"',
                "中文参数",
            ]
            result = subprocess.run(
                [str(entry), *arguments],
                capture_output=True,
                encoding="utf-8",
                env={**os.environ, "XDCODE_API_KEY": "launcher-test-fixture"},
                check=False,
            )
            self.assertEqual(result.returncode, 23, result.stderr)
            self.assertEqual(result.stdout.splitlines(), arguments)
            (bin_directory / "current.txt").write_text("../outside")
            rejected = subprocess.run(
                [str(entry)], capture_output=True, text=True, check=False
            )
            self.assertNotEqual(rejected.returncode, 0)
            self.assertIn("invalid local", rejected.stderr)

    def test_launcher_identity_unit_checks(self):
        with tempfile.TemporaryDirectory(prefix="xdcode-launcher-unit-") as temporary:
            executable = Path(temporary) / ("tests.exe" if os.name == "nt" else "tests")
            subprocess.run(
                [
                    RUSTC,
                    "--edition=2021",
                    "--test",
                    str(REPO_ROOT / "scripts/xdcode/windows-launcher.rs"),
                    "-o",
                    str(executable),
                ],
                check=True,
                capture_output=True,
            )
            subprocess.run([str(executable)], check=True, capture_output=True)


@unittest.skipUnless(POWERSHELL, "PowerShell is required for installer checks")
class PowerShellInstallerTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="xdcode-powershell-test-")
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name).resolve()
        self.package = self.root / "package"
        self.identity = "0.167.0-gaaaaaaaaaaaa"
        for relative in windows.BINARIES:
            binary = self.package / relative
            binary.parent.mkdir(parents=True, exist_ok=True)
            binary.write_bytes(pe_fixture())
        (self.package / "build-info.json").write_text(
            json.dumps(
                {
                    "schema_version": 1,
                    "launcher_schema": 1,
                    "version": "0.167.0",
                    "build_id": "gaaaaaaaaaaaa",
                    "source_dirty": False,
                    "source_commit": "a" * 40,
                    "target": windows.TARGET,
                }
            )
        )
        (self.package / "xdcode-package.json").write_text(
            json.dumps(
                {
                    "layoutVersion": 1,
                    "version": "0.167.0",
                    "buildId": "gaaaaaaaaaaaa",
                    "target": windows.TARGET,
                    "variant": "xdcode",
                    "entrypoint": "bin/xdcode.exe",
                    "resourcesDir": "xdcode-resources",
                    "pathDir": "xdcode-path",
                }
            )
        )
        self.write_checksums()

    def write_checksums(self):
        files = sorted(
            path
            for path in self.package.rglob("*")
            if path.is_file() and path.name != "SHA256SUMS"
        )
        (self.package / "SHA256SUMS").write_text(
            "".join(
                f"{hashlib.sha256(path.read_bytes()).hexdigest()}  {path.relative_to(self.package).as_posix()}\n"
                for path in files
            )
        )

    def run_powershell(self, commands, **environment):
        script = "$ErrorActionPreference = 'Stop'; . $env:INSTALLER_PATH; " + commands
        return subprocess.run(
            [
                POWERSHELL,
                "-NoLogo",
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                script,
            ],
            env={
                **os.environ,
                "INSTALLER_PATH": str(REPO_ROOT / "install.ps1"),
                "PACKAGE_PATH": str(self.package),
                "PACKAGE_IDENTITY": self.identity,
                **environment,
            },
            capture_output=True,
            text=True,
            timeout=60,
            check=False,
        )

    def test_parser_and_complete_package_checks(self):
        result = self.run_powershell(
            "Assert-Package $env:PACKAGE_PATH $env:PACKAGE_IDENTITY; Write-Output 'verified'"
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("verified", result.stdout)

    def release_fixture(self, identity=None, **changes):
        identity = identity or self.identity
        archive = f"xdcode-{identity}-windows-x86_64.zip"
        return {
            "tag_name": "v" + identity,
            "draft": False,
            "prerelease": False,
            "published_at": "2026-11-09T00:00:00Z",
            "assets": [{"name": archive}, {"name": archive + ".sha256"}],
            **changes,
        }

    def test_latest_windows_release_filters_assets_and_sorts_publication_dates(self):
        fixture = self.root / "releases.json"
        fixture.write_text(
            json.dumps(
                [
                    self.release_fixture(
                        "0.167.0-gbbbbbbbbbbbb",
                        published_at="2026-11-08T00:00:00Z",
                    ),
                    self.release_fixture(),
                    self.release_fixture(
                        published_at="2026-11-10T00:00:00Z", assets=[]
                    ),
                    self.release_fixture(
                        published_at="2026-11-11T00:00:00Z", draft=True
                    ),
                    self.release_fixture(
                        published_at="2026-11-12T00:00:00Z", prerelease=True
                    ),
                    self.release_fixture(
                        published_at="2026-11-13T00:00:00Z", tag_name="unsafe-tag"
                    ),
                    self.release_fixture(
                        published_at="2026-11-14T00:00:00Z",
                        assets=[{"name": f"xdcode-{self.identity}-windows-x86_64.zip"}],
                    ),
                ]
            )
        )
        result = self.run_powershell(
            """function Invoke-RestMethod {
    param($Uri, $UseBasicParsing, $ErrorAction, $Proxy)
    if ($Proxy -ne 'http://127.0.0.1:10826') { throw 'Download proxy was not forwarded' }
    Get-Content -LiteralPath $env:RELEASE_FIXTURE -Raw | ConvertFrom-Json
}
Find-LatestWindowsRelease @{Proxy='http://127.0.0.1:10826'} 'owner/xdcode'
""",
            RELEASE_FIXTURE=str(fixture),
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout.strip(), "v" + self.identity)

    def test_latest_windows_release_paginates_when_the_first_page_has_no_windows(self):
        fixture = self.root / "releases.json"
        fixture.write_text(json.dumps(self.release_fixture()))
        result = self.run_powershell(
            """function Invoke-RestMethod {
    param($Uri, $UseBasicParsing, $ErrorAction, $Proxy)
    if ($Uri.EndsWith('page=1')) {
        return @(1..100 | ForEach-Object {
            @{tag_name='v0.167.0-gbbbbbbbbbbbb'; draft=$false; prerelease=$false; assets=@()}
        })
    }
    if (-not $Uri.EndsWith('page=2')) { throw 'Unexpected pagination' }
    Get-Content -LiteralPath $env:RELEASE_FIXTURE -Raw | ConvertFrom-Json
}
Find-LatestWindowsRelease @{} 'owner/xdcode'
""",
            RELEASE_FIXTURE=str(fixture),
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout.strip(), "v" + self.identity)

    def test_latest_windows_release_refuses_packages_without_checksums(self):
        fixture = self.root / "releases.json"
        fixture.write_text(json.dumps([self.release_fixture(assets=[])]))
        result = self.run_powershell(
            """function Invoke-RestMethod {
    param($Uri, $UseBasicParsing, $ErrorAction, $Proxy)
    Get-Content -LiteralPath $env:RELEASE_FIXTURE -Raw | ConvertFrom-Json
}
Find-LatestWindowsRelease @{} 'owner/xdcode'
""",
            RELEASE_FIXTURE=str(fixture),
        )
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("No published stable release", result.stderr)

    def test_path_traversal_reserved_names_and_streams_are_rejected(self):
        for name in (
            "../escape",
            "/absolute",
            "C:/absolute",
            "bin\\escape",
            "bin/CON.txt",
            "bin/xdcode.exe:stream",
            "bin/trailing.",
        ):
            with self.subTest(name=name):
                result = self.run_powershell(
                    "Assert-SafeMember $env:MEMBER_PATH", MEMBER_PATH=name
                )
                self.assertNotEqual(result.returncode, 0)

    def test_corrupt_unlisted_and_wrong_source_files_are_rejected(self):
        binary = self.package / windows.BINARIES[0]
        binary.write_bytes(b"corrupt")
        result = self.run_powershell(
            "Assert-Package $env:PACKAGE_PATH $env:PACKAGE_IDENTITY"
        )
        self.assertNotEqual(result.returncode, 0)
        binary.write_bytes(pe_fixture())
        extra = self.package / "unexpected-file.txt"
        extra.write_text("not listed")
        result = self.run_powershell(
            "Assert-Package $env:PACKAGE_PATH $env:PACKAGE_IDENTITY"
        )
        self.assertNotEqual(result.returncode, 0)
        extra.unlink()
        info = self.package / "build-info.json"
        metadata = json.loads(info.read_text())
        metadata["source_commit"] = "b" * 40
        info.write_text(json.dumps(metadata))
        self.write_checksums()
        result = self.run_powershell(
            "Assert-Package $env:PACKAGE_PATH $env:PACKAGE_IDENTITY"
        )
        self.assertNotEqual(result.returncode, 0)

    def test_zip_links_case_duplicates_and_traversal_are_rejected(self):
        archive_path = self.root / "fixture.zip"
        for name, duplicate, mode in (
            ("../escape", False, 0o100644),
            ("link", False, 0o120777),
            ("bin/xdcode.exe", True, 0o100644),
        ):
            with self.subTest(name=name):
                with zipfile.ZipFile(archive_path, "w") as archive:
                    member = zipfile.ZipInfo(name)
                    member.create_system = 3
                    member.external_attr = mode << 16
                    archive.writestr(member, b"")
                    if duplicate:
                        archive.writestr(name.upper(), b"")
                destination = self.root / "unpack"
                destination.mkdir(exist_ok=True)
                result = self.run_powershell(
                    "Expand-CheckedArchive $env:ARCHIVE_PATH $env:UNPACK_PATH",
                    ARCHIVE_PATH=str(archive_path),
                    UNPACK_PATH=str(destination),
                )
                self.assertNotEqual(result.returncode, 0)
                self.assertEqual(list(destination.iterdir()), [])

    def test_checked_zip_extraction_and_nonmanaged_entry_protection(self):
        archive_path = self.root / "fixture.zip"
        with zipfile.ZipFile(archive_path, "w") as archive:
            for path in self.package.rglob("*"):
                archive.write(path, path.relative_to(self.package))
        destination = self.root / "unpacked"
        destination.mkdir()
        result = self.run_powershell(
            "Expand-CheckedArchive $env:ARCHIVE_PATH $env:UNPACK_PATH; Assert-Package $env:UNPACK_PATH $env:PACKAGE_IDENTITY",
            ARCHIVE_PATH=str(archive_path),
            UNPACK_PATH=str(destination),
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        bin_directory = self.root / "bin"
        bin_directory.mkdir()
        entry = bin_directory / "xdcode.cmd"
        entry.write_text("retain unmanaged launcher")
        result = self.run_powershell(
            "Assert-ManagedEntry $env:BIN_PATH $env:BUILDS_PATH",
            BIN_PATH=str(bin_directory),
            BUILDS_PATH=str(self.root / "builds"),
        )
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(entry.read_text(), "retain unmanaged launcher")

    def test_native_launcher_receipt_is_verified_and_orphans_are_rejected(self):
        builds = self.root / "builds"
        destination = builds / self.identity
        shutil.copytree(self.package, destination)
        bin_directory = self.root / "bin"
        bin_directory.mkdir()
        entry = bin_directory / "xdcode.exe"
        shutil.copyfile(
            destination / "xdcode-resources/xdcode-install-launcher.exe", entry
        )
        receipt = bin_directory / "launcher.json"
        receipt.write_text(
            json.dumps(
                {
                    "schema_version": 1,
                    "source_identity": self.identity,
                    "sha256": hashlib.sha256(entry.read_bytes()).hexdigest(),
                }
            )
        )
        (bin_directory / "current.txt").write_text(self.identity)
        parameters = {"BIN_PATH": str(bin_directory), "BUILDS_PATH": str(builds)}
        command = "Assert-ManagedEntry $env:BIN_PATH $env:BUILDS_PATH"
        result = self.run_powershell(command, **parameters)
        self.assertEqual(result.returncode, 0, result.stderr)
        entry.write_bytes(b"corrupt launcher")
        self.assertNotEqual(self.run_powershell(command, **parameters).returncode, 0)
        entry.unlink()
        self.assertNotEqual(self.run_powershell(command, **parameters).returncode, 0)

    @unittest.skipUnless(
        os.name == "nt" and os.environ.get("XDCODE_TEST_PACKAGE_DIR"),
        "native Windows artifact smoke test is enabled by CI",
    )
    def test_installs_the_actual_windows_artifact_in_an_isolated_home(self):
        archives = list(
            Path(os.environ["XDCODE_TEST_PACKAGE_DIR"]).glob(
                "xdcode-*-windows-x86_64.zip"
            )
        )
        self.assertEqual(len(archives), 1)
        with zipfile.ZipFile(archives[0]) as archive:
            info = json.loads(archive.read("build-info.json"))
        identity = f"{info['version']}-{info['build_id']}"
        downloads = self.root / "downloads"
        downloads.mkdir()
        shutil.copyfile(archives[0], downloads / archives[0].name)
        shutil.copyfile(
            Path(str(archives[0]) + ".sha256"),
            downloads / (archives[0].name + ".sha256"),
        )
        home = self.root / "home"
        runtime = home / ".xdcode"
        runtime.mkdir(parents=True)
        sentinel = runtime / "config.toml"
        sentinel.write_text("existing-runtime-data-must-be-retained\n")
        harness = self.root / "native.ps1"
        harness.write_text("""$ErrorActionPreference = 'Stop'
function Invoke-RestMethod {
    param($Uri, $UseBasicParsing, $ErrorAction, $Proxy)
    return @{
        tag_name=$env:FIXTURE_TAG; draft=$false; prerelease=$false
        published_at='2026-11-09T00:00:00Z'
        assets=@(@{name=$env:FIXTURE_ARCHIVE}, @{name=$env:FIXTURE_ARCHIVE + '.sha256'})
    }
}
function Invoke-WebRequest {
    param($Uri, $OutFile, $UseBasicParsing, $ErrorAction, $Proxy)
    Copy-Item -LiteralPath (Join-Path $env:FIXTURE_DOWNLOADS ([Uri]$Uri).Segments[-1]) -Destination $OutFile
}
& $env:INSTALLER_PATH -Force -NoPathUpdate
& $env:INSTALLER_PATH -Force -NoPathUpdate
""")
        environment = {
            key: value
            for key, value in os.environ.items()
            if not key.startswith(("XDCODE_", "CODEX_", "OPENAI_", "OTEL_"))
            and not key.endswith(("_API_KEY", "_TOKEN", "_SECRET"))
        }
        environment.update(
            INSTALLER_PATH=str(REPO_ROOT / "install.ps1"),
            FIXTURE_TAG="v" + identity,
            FIXTURE_ARCHIVE=archives[0].name,
            FIXTURE_DOWNLOADS=str(downloads),
            LOCALAPPDATA=str(home / "local"),
            USERPROFILE=str(home),
            XDCODE_HOME=str(runtime),
        )
        result = subprocess.run(
            [
                POWERSHELL,
                "-NoLogo",
                "-NoProfile",
                "-NonInteractive",
                "-File",
                str(harness),
            ],
            env=environment,
            capture_output=True,
            text=True,
            timeout=120,
            check=False,
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        entry = home / "local/xdcode/bin/xdcode.exe"
        self.assertTrue(entry.is_file())
        version = subprocess.check_output(
            [str(entry), "--version"], env=environment, text=True
        )
        self.assertEqual(version.strip(), f"xdcode {info['version']}")
        self.assertEqual(
            sentinel.read_text(), "existing-runtime-data-must-be-retained\n"
        )


if __name__ == "__main__":
    unittest.main()
