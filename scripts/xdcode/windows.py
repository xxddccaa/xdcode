#!/usr/bin/env python3
"""Build and verify a native Windows x86_64 package from a clean source commit."""

from __future__ import annotations

import json
import os
import platform
import re
import shutil
import struct
import subprocess
import sys
import tempfile
from pathlib import Path

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
from xdcode_package.targets import PACKAGE_VARIANTS, TARGET_SPECS, PackageInputs
from xdcode_package.v8 import resolve_xdcode_v8_cargo_env

TARGET = "x86_64-pc-windows-msvc"
BINARIES = (
    "bin/xdcode.exe",
    "bin/xdcode-code-mode-host.exe",
    "xdcode-path/rg.exe",
    "xdcode-resources/xdcode-command-runner.exe",
    "xdcode-resources/xdcode-windows-sandbox-setup.exe",
    "xdcode-resources/xdcode-install-launcher.exe",
)


def require_x64_pe(path: Path) -> None:
    with path.open("rb") as stream:
        header = stream.read(64)
        if len(header) != 64 or header[:2] != b"MZ":
            raise DeliveryError(f"expected native Windows PE executable: {path.name}")
        offset = struct.unpack_from("<I", header, 0x3C)[0]
        if offset < 64 or offset > path.stat().st_size - 26:
            raise DeliveryError(f"truncated PE header: {path.name}")
        stream.seek(offset)
        pe_header = stream.read(26)
    if (
        pe_header[:4] != b"PE\0\0"
        or struct.unpack_from("<H", pe_header, 4)[0] != 0x8664
        or struct.unpack_from("<H", pe_header, 24)[0] != 0x20B
    ):
        raise DeliveryError(
            f"expected native Windows x86_64 PE32+ executable: {path.name}"
        )


def check_dependencies(binary: Path, system_directory: Path | None = None) -> list[str]:
    output = subprocess.check_output(["dumpbin", "/DEPENDENTS", str(binary)], text=True)
    dependencies = sorted(
        {
            name.upper()
            for name in re.findall(
                r"^\s+([\w.-]+\.dll)\s*$", output, re.MULTILINE | re.IGNORECASE
            )
        }
    )
    if not dependencies:
        raise DeliveryError(f"cannot inspect Windows DLL dependencies: {binary.name}")
    system_directory = system_directory or Path(os.environ["SystemRoot"]) / "System32"
    for dependency in dependencies:
        if dependency in {"VCRUNTIME140D.DLL", "MSVCP140D.DLL", "UCRTBASED.DLL"}:
            raise DeliveryError(f"debug runtime dependency: {dependency}")
        if (
            not dependency.startswith(("API-MS-WIN-", "EXT-MS-WIN-"))
            and not (system_directory / dependency).is_file()
        ):
            raise DeliveryError(f"non-system DLL dependency: {dependency}")
    return dependencies


def probe(package: Path, version: str) -> None:
    with tempfile.TemporaryDirectory(prefix="xdcode-windows-probe-") as temporary:
        env = {
            key: value
            for key, value in os.environ.items()
            if not key.startswith(("XDCODE_", "CODEX_", "OPENAI_", "OTEL_"))
            and not key.endswith(("_API_KEY", "_TOKEN", "_SECRET"))
        }
        env["XDCODE_HOME"] = temporary
        for relative, argument in (
            (BINARIES[0], "--version"),
            (BINARIES[1], "--help"),
            (BINARIES[2], "--version"),
        ):
            result = subprocess.run(
                [str(package / relative), argument],
                env=env,
                capture_output=True,
                text=True,
                timeout=30,
                check=True,
            )
            if relative == BINARIES[0] and result.stdout.strip() != f"xdcode {version}":
                raise DeliveryError("staged CLI version differs from package metadata")


def main() -> None:
    if platform.system() != "Windows" or platform.machine().lower() not in (
        "amd64",
        "x86_64",
    ):
        raise DeliveryError("run this builder natively on Windows x86_64")
    identity = source_identity()
    if identity["source_dirty"]:
        raise DeliveryError("release builds require a clean source commit")
    upstream = json.loads((REPO_ROOT / "upstream.json").read_text())
    validate_upstream(upstream)
    rust_root = REPO_ROOT / "xdcode-rs"
    workspace = tomllib.loads((rust_root / "Cargo.toml").read_text())
    if workspace["workspace"]["package"]["version"] != upstream["version"]:
        raise DeliveryError("Cargo version differs from upstream.json")
    toolchain = tomllib.loads((rust_root / "rust-toolchain.toml").read_text())[
        "toolchain"
    ]["channel"]
    build_id = "g" + identity["source_commit"][:12]
    package = REPO_ROOT / "target/xdcode-windows-package"
    dist = REPO_ROOT / "target/xdcode-dist"
    archive = dist / f"xdcode-{upstream['version']}-{build_id}-windows-x86_64.zip"
    checksum = Path(str(archive) + ".sha256")
    if any(path.exists() for path in (package, archive, checksum)):
        raise DeliveryError("package output already exists; use a fresh build checkout")
    spec, variant = TARGET_SPECS[TARGET], PACKAGE_VARIANTS["xdcode"]
    target_dir = REPO_ROOT / "target"
    env = {
        **os.environ,
        "CARGO_TARGET_DIR": str(target_dir),
        "RUSTUP_TOOLCHAIN": toolchain,
        "STABLE_GIT_COMMIT": identity["source_commit"],
        "CARGO_NET_GIT_FETCH_WITH_CLI": "true",
    }
    env.update(
        resolve_xdcode_v8_cargo_env(
            spec, environ=env, cache_root=REPO_ROOT / ".build-cache"
        )
    )
    subprocess.run(
        [
            "cargo",
            "build",
            "--locked",
            "--release",
            "--target",
            TARGET,
            "--bin",
            "xdcode",
            "--bin",
            "xdcode-code-mode-host",
            "--bin",
            "xdcode-command-runner",
            "--bin",
            "xdcode-windows-sandbox-setup",
        ],
        cwd=rust_root,
        env=env,
        check=True,
    )
    outputs = target_dir / TARGET / "release"
    subprocess.run(
        [
            "rustc",
            "--edition=2021",
            "--target",
            TARGET,
            "-O",
            "-C",
            "panic=abort",
            "--remap-path-prefix",
            f"{REPO_ROOT}=.",
            str(SCRIPT_DIR / "windows-launcher.rs"),
            "-o",
            str(outputs / "xdcode-install-launcher.exe"),
        ],
        env=env,
        check=True,
    )
    inputs = PackageInputs(
        outputs / "xdcode.exe",
        outputs / "xdcode-code-mode-host.exe",
        resolve_rg_bin(spec, None),
        None,
        outputs / "xdcode-command-runner.exe",
        outputs / "xdcode-windows-sandbox-setup.exe",
    )
    package.mkdir(parents=True)
    build_package_dir(package, upstream["version"], variant, spec, inputs)
    shutil.copyfile(
        outputs / "xdcode-install-launcher.exe",
        package / "xdcode-resources/xdcode-install-launcher.exe",
    )
    manifest = json.loads((package / "xdcode-package.json").read_text())
    manifest["buildId"] = build_id
    write_json(package / "xdcode-package.json", manifest)
    dependencies = {}
    for relative in BINARIES:
        require_x64_pe(package / relative)
        dependencies[relative] = check_dependencies(package / relative)
    for name, source in (
        ("LICENSE", REPO_ROOT / "LICENSE"),
        ("NOTICE", REPO_ROOT / "NOTICE"),
        ("MODIFICATIONS.md", REPO_ROOT / "MODIFICATIONS.md"),
        ("config.example.toml", SCRIPT_DIR / "config.example.toml"),
        ("INSTALL-windows.md", REPO_ROOT / "docs/xdcode-windows.md"),
    ):
        shutil.copyfile(source, package / name)
    write_json(
        package / "build-info.json",
        {
            "schema_version": 1,
            "version": upstream["version"],
            "build_id": build_id,
            "target": TARGET,
            "upstream": upstream,
            **identity,
            "rust_toolchain": toolchain,
            "cargo_profile": "release",
            "build_windows_version": platform.version(),
            "signing": "unsigned; not Authenticode signed",
            "launcher_schema": 1,
            "binary_sha256": {name: sha256_file(package / name) for name in BINARIES},
            "binary_dependencies": dependencies,
        },
    )
    validate_package_dir(package, variant, spec)
    probe(package, upstream["version"])
    if source_identity() != identity:
        raise DeliveryError("source changed during build")
    payload = sorted(path for path in package.rglob("*") if path.is_file())
    (package / "SHA256SUMS").write_text(
        "".join(
            f"{sha256_file(path)}  {path.relative_to(package).as_posix()}\n"
            for path in payload
        )
    )
    write_archive(package, archive, force=False)
    checksum.write_text(f"{sha256_file(archive)}  {archive.name}\n", newline="\n")
    print(f"Native package: {archive}", flush=True)


if __name__ == "__main__":
    main()
