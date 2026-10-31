#!/usr/bin/env python3

from pathlib import Path
import sys
import tempfile
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from xdcode_package.cargo import build_source_binaries
from xdcode_package.cargo import source_binaries_for_target
from xdcode_package.targets import PACKAGE_VARIANTS
from xdcode_package.targets import TARGET_SPECS


class SourceBinariesForTargetTest(unittest.TestCase):
    def test_macos_package_with_prebuilt_entrypoint_builds_nothing(self) -> None:
        self.assertEqual(
            source_binaries_for_target(
                TARGET_SPECS["aarch64-apple-darwin"],
                PACKAGE_VARIANTS["xdcode"],
                build_entrypoint=False,
                build_code_mode_host=False,
                build_bwrap=False,
                build_xdcode_command_runner=False,
                build_xdcode_windows_sandbox_setup=False,
            ),
            [],
        )

    def test_linux_package_with_prebuilt_entrypoint_and_bwrap_builds_nothing(
        self,
    ) -> None:
        self.assertEqual(
            source_binaries_for_target(
                TARGET_SPECS["x86_64-unknown-linux-musl"],
                PACKAGE_VARIANTS["xdcode"],
                build_entrypoint=False,
                build_code_mode_host=False,
                build_bwrap=False,
                build_xdcode_command_runner=False,
                build_xdcode_windows_sandbox_setup=False,
            ),
            [],
        )

    def test_windows_package_with_prebuilt_entrypoint_and_helpers_builds_nothing(
        self,
    ) -> None:
        self.assertEqual(
            source_binaries_for_target(
                TARGET_SPECS["x86_64-pc-windows-msvc"],
                PACKAGE_VARIANTS["xdcode"],
                build_entrypoint=False,
                build_code_mode_host=False,
                build_bwrap=False,
                build_xdcode_command_runner=False,
                build_xdcode_windows_sandbox_setup=False,
            ),
            [],
        )

    def test_missing_windows_helpers_are_built(self) -> None:
        self.assertEqual(
            source_binaries_for_target(
                TARGET_SPECS["x86_64-pc-windows-msvc"],
                PACKAGE_VARIANTS["xdcode"],
                build_entrypoint=False,
                build_code_mode_host=False,
                build_bwrap=False,
                build_xdcode_command_runner=True,
                build_xdcode_windows_sandbox_setup=True,
            ),
            ["xdcode-command-runner", "xdcode-windows-sandbox-setup"],
        )

    def test_missing_code_mode_host_is_built_for_app_server(self) -> None:
        self.assertEqual(
            source_binaries_for_target(
                TARGET_SPECS["aarch64-apple-darwin"],
                PACKAGE_VARIANTS["xdcode-app-server"],
                build_entrypoint=False,
                build_code_mode_host=True,
                build_bwrap=False,
                build_xdcode_command_runner=False,
                build_xdcode_windows_sandbox_setup=False,
            ),
            ["xdcode-code-mode-host"],
        )

    def test_build_uses_prebuilt_windows_helpers_without_running_cargo(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            root = Path(temp_dir)
            entrypoint = touch_file(root / "xdcode.exe")
            code_mode_host = touch_file(root / "xdcode-code-mode-host.exe")
            command_runner = touch_file(root / "xdcode-command-runner.exe")
            sandbox_setup = touch_file(root / "xdcode-windows-sandbox-setup.exe")

            outputs = build_source_binaries(
                TARGET_SPECS["x86_64-pc-windows-msvc"],
                PACKAGE_VARIANTS["xdcode"],
                cargo=str(root / "cargo-that-should-not-run"),
                profile="release",
                entrypoint_bin=entrypoint,
                code_mode_host_bin=code_mode_host,
                bwrap_bin=None,
                xdcode_command_runner_bin=command_runner,
                xdcode_windows_sandbox_setup_bin=sandbox_setup,
            )

        self.assertEqual(outputs.entrypoint_bin, entrypoint)
        self.assertEqual(outputs.code_mode_host_bin, code_mode_host)
        self.assertEqual(outputs.xdcode_command_runner_bin, command_runner)
        self.assertEqual(outputs.xdcode_windows_sandbox_setup_bin, sandbox_setup)


def touch_file(path: Path) -> Path:
    path.write_text("", encoding="utf-8")
    return path.resolve()


if __name__ == "__main__":
    unittest.main()
