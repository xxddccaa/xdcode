#!/usr/bin/env python3
"""Package a completed xdcode build without downloading or rebuilding anything."""

from __future__ import annotations

import sys

if sys.version_info < (3, 11):
    raise SystemExit("xdcode delivery requires Python 3.11 or newer; run python3.11")

import argparse
import gzip
import json
import os
from pathlib import Path
import shutil
import subprocess
import tarfile
import tempfile

SCRIPT_DIR = Path(__file__).resolve().parent
REPO_ROOT = SCRIPT_DIR.parents[1]
sys.path.insert(0, str(SCRIPT_DIR))
sys.path.insert(0, str(REPO_ROOT / "scripts"))
os.environ["XDCODE_REPO_ROOT"] = str(REPO_ROOT)

from install import (BINARIES, BUILD_INFO, FILES, MANIFEST, PAYLOAD, PLATFORM, TARGET,
                     DeliveryError, checked_path, check_version, require_elf, sha256_file,
                     validate_build_info, validate_manifest, write_json)
from build import source_identity
from xdcode_package.layout import build_package_dir, validate_package_dir
from xdcode_package.targets import PACKAGE_VARIANTS, TARGET_SPECS, PackageInputs


def package(build_dir: Path, output_dir: Path) -> Path:
    info = json.loads((build_dir / BUILD_INFO).read_text(encoding="utf-8"))
    identity = validate_build_info(info)
    source_state = source_identity()
    if any(info.get(field) != value for field, value in source_state.items()):
        raise DeliveryError("package source differs from the completed build receipt; use the original checkout or rebuild")
    for name in BINARIES:
        path = build_dir / name
        if path.is_symlink() or sha256_file(path) != info["binary_sha256"][name]:
            raise DeliveryError(f"binary differs from completed build receipt: {name}")
        require_elf(path)
    output_dir = checked_path(output_dir.absolute())
    stem = f"xdcode-{identity}-{PLATFORM}"
    output = output_dir / f"{stem}.tar.gz"
    sidecar = Path(str(output) + ".sha256")
    installer = output_dir / f"{stem}.install.py"
    for path in (output, sidecar, installer):
        if os.path.lexists(path):
            raise DeliveryError(f"output already exists: {path.name}")
    output_dir.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix=".package-", dir=output_dir) as temp:
        stage = Path(temp) / "package"
        stage.mkdir()
        spec, variant = TARGET_SPECS[TARGET], PACKAGE_VARIANTS["xdcode"]
        inputs = PackageInputs(
            entrypoint_bin=build_dir / "bin/xdcode",
            code_mode_host_bin=build_dir / "bin/xdcode-code-mode-host",
            rg_bin=build_dir / "xdcode-path/rg", bwrap_bin=build_dir / "xdcode-resources/bwrap",
            xdcode_command_runner_bin=None, xdcode_windows_sandbox_setup_bin=None,
        )
        build_package_dir(stage, info["version"], variant, spec, inputs)
        manifest = json.loads((stage / MANIFEST).read_text(encoding="utf-8"))
        manifest["buildId"] = info["build_id"]
        write_json(stage / MANIFEST, manifest)
        validate_manifest(manifest, info)
        validate_package_dir(stage, variant, spec)
        write_json(stage / BUILD_INFO, info)
        for name, source in (
            ("LICENSE", REPO_ROOT / "LICENSE"), ("NOTICE", REPO_ROOT / "NOTICE"),
            ("MODIFICATIONS.md", REPO_ROOT / "MODIFICATIONS.md"),
            ("install.py", SCRIPT_DIR / "install.py"),
            ("config.example.toml", SCRIPT_DIR / "config.example.toml"),
            ("docs/xdcode.md", REPO_ROOT / "docs/xdcode.md"),
        ):
            (stage / name).parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(source, stage / name)
            (stage / name).chmod(0o644)
        if sha256_file(stage / "MODIFICATIONS.md") != info["modifications_sha256"]:
            raise DeliveryError("derivative change notices differ from the build receipt")
        # Check staged bytes too: inputs may have been modified during copying.
        for name in BINARIES:
            if sha256_file(stage / name) != info["binary_sha256"][name]:
                raise DeliveryError("build input changed during packaging")
        check_version(stage, info["version"])
        if source_identity() != source_state:
            raise DeliveryError("source tree changed during packaging")
        (stage / "SHA256SUMS").write_text("".join(
            f"{sha256_file(stage / name)}  {name}\n" for name in sorted(PAYLOAD)
        ), encoding="utf-8")
        # Stable tar headers and gzip timestamp make repackaging the same input deterministic.
        epoch = info.get("source_date_epoch", 0)
        if type(epoch) is not int or not 0 <= epoch <= 2**32 - 1:
            raise DeliveryError("invalid source_date_epoch")
        staged_archive = Path(temp) / output.name
        with staged_archive.open("xb") as raw:
            with gzip.GzipFile(filename="", mode="wb", fileobj=raw, mtime=epoch) as compressed:
                with tarfile.open(fileobj=compressed, mode="w|") as archive:
                    for name in sorted(FILES):
                        path = stage / name
                        member = archive.gettarinfo(str(path), arcname=name)
                        member.uid = member.gid = 0
                        member.uname = member.gname = ""
                        member.mtime = epoch
                        member.mode = 0o755 if name in BINARIES else 0o644
                        with path.open("rb") as source:
                            archive.addfile(member, source)
        # Publish without replacing a competing writer's outputs.
        os.link(staged_archive, output)
        with sidecar.open("x", encoding="utf-8") as stream:
            stream.write(f"{sha256_file(output)}  {output.name}\n")
        with installer.open("xb") as stream:
            stream.write((stage / "install.py").read_bytes())
    return output


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--build-dir", required=True, type=Path, help="output from scripts/xdcode/build.py")
    parser.add_argument("--output-dir", type=Path, default=REPO_ROOT / "target/xdcode-dist")
    args = parser.parse_args(argv)
    try:
        output = package(args.build_dir.absolute(), args.output_dir)
    except (RuntimeError, OSError, ValueError, subprocess.SubprocessError) as error:
        print(f"xdcode package: {error}", file=sys.stderr)
        return 1
    print(f"Package: {output}\nChecksum: {output}.sha256\nOffline installer: {output_dir_installer(output)}")
    return 0


def output_dir_installer(output: Path) -> Path:
    return output.with_name(output.name.removesuffix(".tar.gz") + ".install.py")


if __name__ == "__main__":
    raise SystemExit(main())
