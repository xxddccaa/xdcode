"""Apple package architecture and portability rejection checks."""

import importlib.util
from pathlib import Path
import struct
import tempfile
import unittest
from unittest.mock import patch

SPEC = importlib.util.spec_from_file_location("xdcode_macos", Path(__file__).parents[1] / "macos.py")
macos = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(macos)


class MacosPackageTests(unittest.TestCase):
    def test_rejects_intel_linux_and_truncated_binaries(self):
        with tempfile.TemporaryDirectory() as temporary:
            binary = Path(temporary) / "xdcode"
            for content in (b"\x7fELF" + bytes(20), b"", struct.pack("<III", 0xFEEDFACF, 0x01000007, 3)):
                binary.write_bytes(content)
                with self.assertRaises(macos.DeliveryError):
                    macos.require_arm64_macho(binary)
            binary.write_bytes(struct.pack("<III", 0xFEEDFACF, 0x0100000C, 0))
            macos.require_arm64_macho(binary)

    def test_rejects_builder_local_dynamic_libraries(self):
        for dependency in ("/opt/homebrew/lib/libssl.dylib", "@rpath/libcustom.dylib"):
            with patch.object(macos.subprocess, "check_output", return_value=f"xdcode:\n\t{dependency} (compatibility version 1.0.0)\n"):
                with self.assertRaises(macos.DeliveryError):
                    macos.check_dependencies(Path("xdcode"))
        with patch.object(macos.subprocess, "check_output", return_value="xdcode:\n\t/usr/lib/libSystem.B.dylib (compatibility version 1.0.0)\n"):
            macos.check_dependencies(Path("xdcode"))
