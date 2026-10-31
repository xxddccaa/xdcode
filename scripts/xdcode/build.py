#!/usr/bin/env python3
"""Build a pinned GNU/Linux xdcode delivery, including its bwrap integrity stamp."""

from __future__ import annotations

import sys

if sys.version_info < (3, 11):
    raise SystemExit("xdcode delivery requires Python 3.11 or newer; run python3.11")

import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import tomllib

SCRIPT_DIR = Path(__file__).resolve().parent
REPO_ROOT = SCRIPT_DIR.parents[1]
sys.path.insert(0, str(SCRIPT_DIR))
sys.path.insert(0, str(REPO_ROOT / "scripts"))
os.environ["XDCODE_REPO_ROOT"] = str(REPO_ROOT)

from install import (BINARIES, BUILD_ID, BUILD_INFO, TARGET, DeliveryError,
                     checked_path, host_glibc, probe_bwrap, require_elf, sha256_file,
                     validate_build_info, validate_upstream, write_json)
from xdcode_package.targets import TARGET_SPECS
from xdcode_package.v8 import resolve_xdcode_v8_cargo_env
from xdcode_package.ripgrep import resolve_rg_bin


def git(*args: str, repo: Path = REPO_ROOT) -> bytes:
    return subprocess.check_output(["git", "-C", str(repo), *args], stderr=subprocess.PIPE)


def source_identity(repo: Path = REPO_ROOT) -> dict:
    """Record working bytes without saving diffs or configuration contents."""
    commit = git("rev-parse", "HEAD", repo=repo).decode().strip()
    dirty = bool(git("status", "--porcelain", "--untracked-files=normal", repo=repo))
    paths = sorted(set(git("ls-files", "-c", "-o", "--exclude-standard", "-z", repo=repo).split(b"\0")) - {b""})
    digest = hashlib.sha256()
    for name in paths:
        path = repo / os.fsdecode(name)
        if not os.path.lexists(path):
            continue  # Renamed/deleted upstream files are not build inputs.
        digest.update(name + b"\0")
        if path.is_symlink():
            digest.update(b"symlink\0" + os.fsencode(os.readlink(path)))
        elif path.is_file():
            digest.update(b"executable\0" if path.stat().st_mode & 0o111 else b"file\0")
            digest.update(bytes.fromhex(sha256_file(path)))
        else:
            raise DeliveryError(f"unsupported source entry: {path}")
    return {"source_commit": commit, "source_dirty": dirty, "source_tree_sha256": digest.hexdigest()}


def run(command: list[str], *, env: dict, cwd: Path) -> None:
    # Commands contain tool paths/arguments only; never print the environment.
    print("+", " ".join(command), flush=True)
    subprocess.run(command, env=env, cwd=cwd, check=True)


def build(args: argparse.Namespace) -> Path:
    requires_glibc = host_glibc()
    upstream = json.loads(args.upstream.read_text(encoding="utf-8"))
    validate_upstream(upstream)
    rust_root = REPO_ROOT / "xdcode-rs"
    workspace = tomllib.loads((rust_root / "Cargo.toml").read_text())
    if workspace["workspace"]["package"]["version"] != upstream["version"]:
        raise DeliveryError("Cargo workspace package version must equal upstream.json version")
    if "XDCODE_SKIP_BWRAP_BUILD" in os.environ:
        raise DeliveryError("release builds reject XDCODE_SKIP_BWRAP_BUILD, including empty or '0' values")
    vendor_dir = checked_path(rust_root / "vendor/bubblewrap")
    if not vendor_dir.is_dir():
        raise DeliveryError("release build requires the vendored bubblewrap source directory")
    override = os.environ.get("XDCODE_BWRAP_SOURCE_DIR")
    if override is not None and Path(override).resolve() != vendor_dir:
        raise DeliveryError("release builds reject an external XDCODE_BWRAP_SOURCE_DIR")
    # Validate the chosen snapshot exists, without deriving a version from ancestry:
    # stable upstream tags may be on release branches rather than on main.
    git("cat-file", "-e", upstream["upstream_commit"] + "^{commit}")
    identity = source_identity()
    if identity["source_dirty"] and not args.allow_dirty:
        raise DeliveryError("source tree is dirty; commit it or explicitly use --allow-dirty for a development build")
    build_id = args.build_id or ("g" + identity["source_commit"][:12]
                                + ("-dirty-" + identity["source_tree_sha256"][:12] if identity["source_dirty"] else ""))
    if not BUILD_ID.fullmatch(build_id):
        raise DeliveryError("build ID must be 1-80 safe filename characters")
    output = checked_path((args.output_dir or REPO_ROOT / "target/xdcode-builds" / build_id).absolute())
    if os.path.lexists(output):
        raise DeliveryError("build output already exists; choose a fresh output directory/build ID")
    target_dir = checked_path(args.target_dir.absolute())
    cache_dir = checked_path(args.cache_dir.absolute())
    if output == target_dir or target_dir.is_relative_to(output) or cache_dir.is_relative_to(output):
        raise DeliveryError("build output must be separate from Cargo and dependency caches")
    target_dir.mkdir(parents=True, exist_ok=True)
    cache_dir.mkdir(parents=True, exist_ok=True)
    toolchain = tomllib.loads((rust_root / "rust-toolchain.toml").read_text())["toolchain"]["channel"]
    environment = dict(os.environ)
    environment.update({"CARGO_TARGET_DIR": str(target_dir), "XDCODE_REPO_ROOT": str(REPO_ROOT),
                        "STABLE_GIT_COMMIT": identity["source_commit"],
                        "RUSTUP_TOOLCHAIN": toolchain,
                        "XDCODE_BWRAP_SOURCE_DIR": str(vendor_dir),
                        "CARGO_NET_GIT_FETCH_WITH_CLI": "true"})
    # DotSlash/ripgrep's upstream cache is based on TMPDIR. Keep it and V8 on
    # the configured build filesystem, never the small system /tmp mount.
    cache_temp = cache_dir / "tmp"
    cache_temp.mkdir(exist_ok=True)
    environment["TMPDIR"] = str(cache_temp)
    environment.pop("XDCODE_BWRAP_SHA256", None)
    spec = TARGET_SPECS[TARGET]
    environment.update(resolve_xdcode_v8_cargo_env(spec, environ=environment, cache_root=cache_dir))
    cargo_args = [args.cargo, "build", "--locked", "--target", TARGET, "--release"]
    run([*cargo_args, "--bin", "bwrap"], env=environment, cwd=rust_root)
    bwrap = target_dir / TARGET / "release/bwrap"
    run([args.strip_tool, "--strip-debug", "--strip-unneeded", str(bwrap)], env=environment, cwd=rust_root)
    probe_bwrap(bwrap, cache_temp)
    environment["XDCODE_BWRAP_SHA256"] = sha256_file(bwrap)
    run([*cargo_args, "--bin", "xdcode", "--bin", "xdcode-code-mode-host"], env=environment, cwd=rust_root)
    # Use a subprocess to give the upstream ripgrep resolver its own TMPDIR;
    # tempfile caches this environment setting within each Python process.
    if args.rg_bin:
        rg = resolve_rg_bin(spec, args.rg_bin)
    else:
        code = (f"import sys; sys.path.insert(0, {str(REPO_ROOT / 'scripts')!r}); "
                "from xdcode_package.ripgrep import resolve_rg_bin; "
                "from xdcode_package.targets import TARGET_SPECS; "
                f"print(resolve_rg_bin(TARGET_SPECS[{TARGET!r}], None))")
        rg = Path(subprocess.check_output([sys.executable, "-c", code], env=environment,
                                         cwd=REPO_ROOT / "scripts", text=True).strip().splitlines()[-1])
    # Refuse to attribute artifacts to a source snapshot that changed mid-build.
    if source_identity() != identity:
        raise DeliveryError("source tree changed during the build; rerun from a stable checkout")
    output.mkdir(parents=True)
    sources = (target_dir / TARGET / "release/xdcode",
               target_dir / TARGET / "release/xdcode-code-mode-host", rg, bwrap)
    for name, source in zip(BINARIES, sources, strict=True):
        require_elf(source)
        dest = output / name
        dest.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(source, dest)
        dest.chmod(0o755)
        if name.startswith("bin/"):
            run([args.strip_tool, "--strip-debug", "--strip-unneeded", str(dest)], env=environment, cwd=rust_root)
    info = {
        "schema_version": 1, "version": upstream["version"], "build_id": build_id,
        "target": TARGET, "upstream": upstream, **identity,
        "requires_glibc": requires_glibc,
        "bwrap_sha256": environment["XDCODE_BWRAP_SHA256"],
        "bwrap_source": "xdcode-rs/vendor/bubblewrap",
        "modifications_sha256": sha256_file(REPO_ROOT / "MODIFICATIONS.md"),
        "binary_sha256": {name: sha256_file(output / name) for name in BINARIES},
        "rust_toolchain": toolchain,
        "cargo_profile": "release",
        "source_date_epoch": int(git("show", "-s", "--format=%ct", "HEAD").strip()),
    }
    validate_build_info(info)
    write_json(output / BUILD_INFO, info)
    return output


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--upstream", type=Path, default=REPO_ROOT / "upstream.json")
    parser.add_argument("--build-id", help="personal revision ID; default g<source commit> plus dirty digest")
    parser.add_argument("--output-dir", type=Path, help="new directory for binaries and build receipt")
    parser.add_argument("--target-dir", type=Path, default=REPO_ROOT / "target")
    parser.add_argument("--cache-dir", type=Path, default=REPO_ROOT / ".build-cache")
    parser.add_argument("--rg-bin", type=Path, help="local ripgrep, otherwise fetch using the upstream pinned manifest")
    parser.add_argument("--cargo", default="cargo")
    parser.add_argument("--strip-tool", default="strip")
    parser.add_argument("--allow-dirty", action="store_true", help="record a development source snapshot; never hide dirty state")
    args = parser.parse_args(argv)
    try:
        output = build(args)
    except (RuntimeError, OSError, ValueError, subprocess.SubprocessError) as error:
        print(f"xdcode build: {error}", file=sys.stderr)
        return 1
    print(f"Build receipt and binaries: {output}\nNext: python3.11 scripts/xdcode/package.py --build-dir {output}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
