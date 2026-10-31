#!/usr/bin/env python3
"""Build and verify a native Apple Silicon package from a clean source commit."""

from __future__ import annotations

import json
import os
from pathlib import Path
import platform
import shutil
import struct
import subprocess
import sys
import tempfile
import tomllib

SCRIPT_DIR = Path(__file__).resolve().parent
REPO_ROOT = SCRIPT_DIR.parents[1]
sys.path.insert(0, str(SCRIPT_DIR))
sys.path.insert(0, str(REPO_ROOT / "scripts"))
os.environ["XDCODE_REPO_ROOT"] = str(REPO_ROOT)

from build import source_identity
from install import DeliveryError, sha256_file, validate_upstream, write_json
from xdcode_package.archive import write_archive
from xdcode_package.layout import build_package_dir, validate_package_dir
from xdcode_package.ripgrep import resolve_rg_bin
from xdcode_package.symbols import strip_binary
from xdcode_package.targets import PACKAGE_VARIANTS, TARGET_SPECS, PackageInputs
from xdcode_package.v8 import resolve_xdcode_v8_cargo_env

TARGET = "aarch64-apple-darwin"
BINARIES = ("bin/xdcode", "bin/xdcode-code-mode-host", "xdcode-path/rg")


def require_arm64_macho(path: Path) -> None:
    with path.open("rb") as stream:
        header = stream.read(12)
    if len(header) != 12 or struct.unpack("<III", header)[:2] != (0xFEEDFACF, 0x0100000C):
        raise DeliveryError(f"expected native ARM64 Mach-O executable: {path.name}")


def check_dependencies(binary: Path) -> None:
    output = subprocess.check_output(["otool", "-L", str(binary)], text=True)
    for line in output.splitlines()[1:]:
        dependency = line.strip().split(" (", 1)[0]
        if not dependency.startswith(("/usr/lib/", "/System/Library/")):
            raise DeliveryError(f"non-system dynamic dependency in {binary.name}: {dependency}")


def probe(package: Path, version: str) -> None:
    with tempfile.TemporaryDirectory(prefix="xdcode-macos-probe-") as temporary:
        env = {"PATH": "/usr/bin:/bin:/usr/sbin:/sbin", "XDCODE_HOME": temporary}
        for relative, argument in ((BINARIES[0], "--version"),
                                   (BINARIES[1], "--help"), (BINARIES[2], "--version")):
            result = subprocess.run([str(package / relative), argument], env=env,
                                    capture_output=True, text=True, timeout=30, check=True)
            if relative == BINARIES[0] and result.stdout.strip() != f"xdcode {version}":
                raise DeliveryError("staged CLI version differs from package metadata")


def main() -> None:
    if platform.system() != "Darwin" or platform.machine() != "arm64":
        raise DeliveryError("run this builder natively on an Apple Silicon Mac")
    identity = source_identity()
    if identity["source_dirty"]:
        raise DeliveryError("release builds require a clean source commit")
    upstream = json.loads((REPO_ROOT / "upstream.json").read_text())
    validate_upstream(upstream)
    rust_root = REPO_ROOT / "xdcode-rs"
    workspace = tomllib.loads((rust_root / "Cargo.toml").read_text())
    if workspace["workspace"]["package"]["version"] != upstream["version"]:
        raise DeliveryError("Cargo version differs from upstream.json")
    toolchain = tomllib.loads((rust_root / "rust-toolchain.toml").read_text())["toolchain"]["channel"]
    build_id = "g" + identity["source_commit"][:12]
    package = REPO_ROOT / "target/xdcode-macos-package"
    dist = REPO_ROOT / "target/xdcode-dist"
    archive = dist / f"xdcode-{upstream['version']}-{build_id}-macos-arm64.tar.gz"
    checksum = Path(str(archive) + ".sha256")
    if any(path.exists() for path in (package, archive, checksum)):
        raise DeliveryError("package output already exists; use a fresh build checkout")
    spec, variant = TARGET_SPECS[TARGET], PACKAGE_VARIANTS["xdcode"]
    target_dir = REPO_ROOT / "target"
    env = {**os.environ, "CARGO_TARGET_DIR": str(target_dir),
           "RUSTUP_TOOLCHAIN": toolchain, "STABLE_GIT_COMMIT": identity["source_commit"],
           "CARGO_NET_GIT_FETCH_WITH_CLI": "true"}
    env.update(resolve_xdcode_v8_cargo_env(spec, environ=env, cache_root=REPO_ROOT / ".build-cache"))
    subprocess.run(["cargo", "build", "--locked", "--release", "--target", TARGET,
                    "--bin", "xdcode", "--bin", "xdcode-code-mode-host"],
                   cwd=rust_root, env=env, check=True)
    outputs = target_dir / TARGET / "release"
    inputs = PackageInputs(outputs / "xdcode", outputs / "xdcode-code-mode-host",
                           resolve_rg_bin(spec, None), None, None, None)
    package.mkdir(parents=True)
    build_package_dir(package, upstream["version"], variant, spec, inputs)
    manifest = json.loads((package / "xdcode-package.json").read_text())
    manifest["buildId"] = build_id
    write_json(package / "xdcode-package.json", manifest)
    for relative in BINARIES:
        binary = package / relative
        require_arm64_macho(binary)
        if relative.startswith("bin/"):
            strip_binary(binary, spec)
        # Ad-hoc signing makes the packaged bytes runnable on ARM64. This is
        # not Apple Developer ID signing or notarization.
        subprocess.run(["codesign", "--force", "--sign", "-", str(binary)], check=True)
        subprocess.run(["codesign", "--verify", "--strict", str(binary)], check=True)
        check_dependencies(binary)
    for name, source in (
        ("LICENSE", REPO_ROOT / "LICENSE"), ("NOTICE", REPO_ROOT / "NOTICE"),
        ("MODIFICATIONS.md", REPO_ROOT / "MODIFICATIONS.md"),
        ("config.example.toml", SCRIPT_DIR / "config.example.toml"),
        ("INSTALL-macos.md", REPO_ROOT / "docs/xdcode-macos.md"),
    ):
        shutil.copyfile(source, package / name)
    info = {"schema_version": 1, "version": upstream["version"], "build_id": build_id,
            "target": TARGET, "upstream": upstream, **identity,
            "rust_toolchain": toolchain, "cargo_profile": "release",
            "build_macos_version": platform.mac_ver()[0],
            "signing": "ad-hoc; not Developer ID signed or notarized",
            "binary_sha256": {name: sha256_file(package / name) for name in BINARIES}}
    write_json(package / "build-info.json", info)
    validate_package_dir(package, variant, spec)
    probe(package, upstream["version"])
    if source_identity() != identity:
        raise DeliveryError("source changed during build")
    payload = sorted(path for path in package.rglob("*") if path.is_file())
    (package / "SHA256SUMS").write_text("".join(
        f"{sha256_file(path)}  {path.relative_to(package)}\n" for path in payload))
    write_archive(package, archive, force=False)
    checksum.write_text(f"{sha256_file(archive)}  {archive.name}\n")
    print(f"Native package: {archive}", flush=True)


if __name__ == "__main__":
    main()
