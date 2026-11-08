#!/usr/bin/env python3
"""Install a local xdcode GNU/Linux package; Python standard library only."""

from __future__ import annotations

import sys

if sys.version_info < (3, 11):
    raise SystemExit("xdcode delivery requires Python 3.11 or newer; run python3.11")

import argparse
import contextlib
import fcntl
import hashlib
import json
import os
from pathlib import Path, PurePosixPath
import platform
import re
import selectors
import shutil
import stat
import subprocess
import tarfile
import tempfile
import time

TARGET = "x86_64-unknown-linux-gnu"
PLATFORM = "linux-x86_64"
MANIFEST = "xdcode-package.json"
BUILD_INFO = "build-info.json"
BINARIES = (
    "bin/xdcode", "bin/xdcode-code-mode-host",
    "xdcode-path/rg", "xdcode-resources/bwrap",
)
PAYLOAD = frozenset((*BINARIES, MANIFEST, BUILD_INFO, "LICENSE", "NOTICE", "MODIFICATIONS.md",
                     "install.py", "config.example.toml", "docs/xdcode.md"))
FILES = PAYLOAD | {"SHA256SUMS"}
DIRECTORIES = frozenset(("bin", "xdcode-path", "xdcode-resources", "docs"))
STABLE_VERSION = re.compile(r"(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)\Z")
BUILD_ID = re.compile(r"[A-Za-z0-9][A-Za-z0-9._-]{0,79}\Z")
DIGEST = re.compile(r"[a-f0-9]{64}\Z")
COMMIT = re.compile(r"[a-f0-9]{40}\Z")
PROBE_TIMEOUT = 10
PROBE_OUTPUT_LIMIT = 64 * 1024


class DeliveryError(RuntimeError):
    pass


def sha256_file(path: Path) -> str:
    with path.open("rb") as stream:
        return hash_stream(stream)


def hash_stream(stream) -> str:
    digest = hashlib.sha256()
    for chunk in iter(lambda: stream.read(1024 * 1024), b""):
        digest.update(chunk)
    return digest.hexdigest()


def write_json(path: Path, value: dict) -> None:
    path.write_text(json.dumps(value, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def validate_upstream(info: dict) -> None:
    if not isinstance(info, dict):
        raise DeliveryError("upstream metadata must be an object")
    version = info.get("version", "")
    if not isinstance(version, str) or not STABLE_VERSION.fullmatch(version) or version == "0.0.0":
        raise DeliveryError("upstream.json must supply a stable product version")
    upstream_version = info.get("upstream_version", version)
    if not isinstance(upstream_version, str) or not STABLE_VERSION.fullmatch(upstream_version) or upstream_version == "0.0.0":
        raise DeliveryError("upstream_version must identify a stable upstream baseline")
    if info.get("upstream_tag") != f"rust-v{upstream_version}":
        raise DeliveryError("upstream version must equal the upstream stable tag")
    if not isinstance(info.get("upstream_commit"), str) or not COMMIT.fullmatch(info["upstream_commit"]):
        raise DeliveryError("upstream_commit must be a full lowercase Git commit")


def validate_build_info(info: dict) -> str:
    if not isinstance(info, dict) or info.get("schema_version") != 1:
        raise DeliveryError("unsupported build metadata schema")
    validate_upstream(info.get("upstream", {}))
    if info.get("version") != info["upstream"]["version"]:
        raise DeliveryError("build version differs from upstream metadata")
    if not isinstance(info.get("build_id"), str) or not BUILD_ID.fullmatch(info["build_id"]):
        raise DeliveryError("unsafe or missing build ID")
    if info.get("target") != TARGET:
        raise DeliveryError(f"only {TARGET} packages are supported")
    if not isinstance(info.get("source_commit"), str) or not COMMIT.fullmatch(info["source_commit"]):
        raise DeliveryError("missing pinned source commit")
    if not isinstance(info.get("source_tree_sha256"), str) or not DIGEST.fullmatch(info["source_tree_sha256"]):
        raise DeliveryError("missing source tree digest")
    if type(info.get("source_dirty")) is not bool:
        raise DeliveryError("missing source dirty flag")
    if not isinstance(info.get("requires_glibc"), str) or not re.fullmatch(r"[0-9]+\.[0-9]+", info["requires_glibc"]):
        raise DeliveryError("missing GNU libc requirement")
    hashes = info.get("binary_sha256", {})
    if not isinstance(hashes, dict) or set(hashes) != set(BINARIES):
        raise DeliveryError("build receipt must cover all four executables")
    if not all(isinstance(value, str) and DIGEST.fullmatch(value) for value in hashes.values()):
        raise DeliveryError("invalid executable digest")
    if info.get("bwrap_sha256") != hashes["xdcode-resources/bwrap"]:
        raise DeliveryError("bwrap digest does not match the build receipt")
    if info.get("bwrap_source") != "xdcode-rs/vendor/bubblewrap":
        raise DeliveryError("build receipt must pin the vendored bubblewrap source")
    if not isinstance(info.get("modifications_sha256"), str) or not DIGEST.fullmatch(info["modifications_sha256"]):
        raise DeliveryError("missing derivative change-notice digest")
    return f"{info['version']}-{info['build_id']}"


def validate_manifest(manifest: dict, info: dict) -> None:
    expected = {
        "layoutVersion": 1, "version": info["version"], "target": TARGET,
        "variant": "xdcode", "entrypoint": "bin/xdcode",
        "resourcesDir": "xdcode-resources", "pathDir": "xdcode-path",
        "buildId": info["build_id"],
    }
    if not isinstance(manifest, dict) or any(manifest.get(k) != v for k, v in expected.items()):
        raise DeliveryError("package manifest does not match the build identity/layout")


def require_elf(path: Path) -> None:
    with path.open("rb") as stream:
        header = stream.read(20)
    if (len(header) != 20 or header[:6] != b"\x7fELF\x02\x01"
            or int.from_bytes(header[16:18], "little") not in (2, 3)
            or header[18:20] != b"\x3e\x00"):
        raise DeliveryError(f"not a Linux x86_64 ELF executable: {path.name}")


def host_glibc() -> str:
    if platform.system() != "Linux" or platform.machine() not in ("x86_64", "amd64"):
        raise DeliveryError("this delivery supports Linux x86_64 only")
    try:
        name, version = os.confstr("CS_GNU_LIBC_VERSION").split()
    except (AttributeError, OSError, TypeError, ValueError):
        raise DeliveryError("GNU libc is required") from None
    if name != "glibc":
        raise DeliveryError("GNU libc is required")
    return version


def probe_executable(executable: Path, option: str, scratch_parent: Path,
                     *, timeout: float = PROBE_TIMEOUT) -> str:
    """Bound time/output and discard stderr; no credentials or loader overrides."""
    with tempfile.TemporaryDirectory(prefix=".probe-", dir=scratch_parent) as temporary:
        environment = {"PATH": os.defpath, "LANG": "C", "LC_ALL": "C",
                       "XDCODE_HOME": str(Path(temporary) / "runtime"), "RUST_BACKTRACE": "0"}
        try:
            with subprocess.Popen([str(executable.absolute()), option], env=environment, cwd=temporary,
                                  stdin=subprocess.DEVNULL, stdout=subprocess.PIPE,
                                  stderr=subprocess.DEVNULL) as process:
                try:
                    deadline = time.monotonic() + timeout
                    output = bytearray()
                    with selectors.DefaultSelector() as selector:
                        selector.register(process.stdout, selectors.EVENT_READ)
                        while True:
                            remaining = deadline - time.monotonic()
                            if remaining <= 0 or not selector.select(remaining):
                                raise DeliveryError(f"runtime probe timed out: {executable.name}")
                            chunk = os.read(process.stdout.fileno(), 8192)
                            if not chunk:
                                break
                            output.extend(chunk)
                            if len(output) > PROBE_OUTPUT_LIMIT:
                                raise DeliveryError(f"runtime probe output limit exceeded: {executable.name}")
                    returncode = process.wait(timeout=max(0.001, deadline - time.monotonic()))
                    if returncode != 0:
                        raise DeliveryError(f"runtime probe failed: {executable.name} (exit {returncode}); check native runtime libraries")
                    return output.decode("utf-8", errors="replace").strip()
                except BaseException:
                    if process.poll() is None:
                        process.kill()
                    process.wait()
                    raise
        except (OSError, subprocess.SubprocessError):
            raise DeliveryError(f"runtime probe could not load/finish: {executable.name}; check native runtime libraries") from None


def probe_bwrap(executable: Path, scratch_parent: Path) -> None:
    # --version runs before bubblewrap attempts capability/namespace setup.
    if probe_executable(executable, "--version", scratch_parent) != "bubblewrap built for Xdcode":
        raise DeliveryError("bwrap --version does not identify the vendored Xdcode build")


def check_version(package_dir: Path, version: str) -> None:
    if probe_executable(package_dir / "bin/xdcode", "--version", package_dir.parent) != f"xdcode {version}":
        raise DeliveryError("packaged xdcode --version does not report the exact upstream product version")


def validate_runtime(package_dir: Path, version: str) -> None:
    check_version(package_dir, version)
    host_help = probe_executable(package_dir / "bin/xdcode-code-mode-host", "--help", package_dir.parent)
    if "Usage:" not in host_help or "--listen" not in host_help:
        raise DeliveryError("code-mode host did not report the expected help")
    if not probe_executable(package_dir / "xdcode-path/rg", "--version", package_dir.parent).startswith("ripgrep "):
        raise DeliveryError("rg did not report a ripgrep version")
    probe_bwrap(package_dir / "xdcode-resources/bwrap", package_dir.parent)


def checked_path(path: Path) -> Path:
    """Do not resolve away a symlink or a '..' supplied by the caller."""
    path = path.expanduser()
    if not path.is_absolute() or ".." in path.parts:
        raise DeliveryError("installation paths must be absolute and contain no '..'")
    for component in (path, *path.parents):
        if component.is_symlink():
            raise DeliveryError(f"symlink in installation path: {component}")
    return path


def validate_targets(install_root: Path, bin_dir: Path, xdcode_home: Path) -> tuple[Path, Path, Path]:
    install_root, bin_dir, xdcode_home = map(checked_path, (install_root, bin_dir, xdcode_home))
    forbidden = {"codex", "sc", ".codex"}
    system_roots = {"bin", "sbin", "usr", "etc", "dev", "proc", "sys", "boot", "lib", "lib64"}
    for path, basename in ((install_root, "xdcode"), (bin_dir, "bin")):
        if path.name != basename or len(path.parts) < 4 or path.parts[1] in system_roots:
            raise DeliveryError(f"unsafe installation target: expected a private {basename} directory")
        # Hidden application data directories are not installation targets.
        # The conventional user-owned .local installation prefix is allowed.
        hidden_app_directory = any(
            part.startswith(".") and part != ".local" for part in path.parts
        )
        if forbidden.intersection(path.parts) or hidden_app_directory:
            raise DeliveryError("refusing another CLI's installation or data directory")
        if path.exists():
            mode = path.stat()
            if not path.is_dir() or mode.st_uid != os.getuid() or mode.st_mode & 0o022:
                raise DeliveryError(f"target must be an owned directory without group/other write access: {path}")
        if path == Path.home() or path.is_relative_to(xdcode_home) or xdcode_home.is_relative_to(path):
            raise DeliveryError("installation directories must be separate from runtime data")
    if install_root.is_relative_to(bin_dir) or bin_dir.is_relative_to(install_root):
        raise DeliveryError("installation root and bin directory must be separate")
    return install_root, bin_dir, xdcode_home


def read_process_stat(process: Path) -> tuple[str, int]:
    # /proc/PID/stat field 2 may itself contain spaces and closing parentheses.
    raw = (process / "stat").read_bytes()
    end = raw.rfind(b")")
    fields = raw[end + 1:].split() if end >= 0 else []
    if len(fields) < 20 or len(fields[0]) != 1 or not fields[19].isdigit():
        raise DeliveryError("cannot establish process identity from Linux stat")
    return fields[0].decode("ascii"), int(fields[19])


def pid_record_is_live(record: object, proc_root: Path) -> bool:
    pid = record.get("pid") if isinstance(record, dict) else record
    if type(pid) is not int or not 0 < pid <= 2**32 - 1:
        raise DeliveryError("invalid daemon PID record; installation refused")
    process = proc_root / str(pid)
    try:
        process.stat()
    except FileNotFoundError:
        return False
    except PermissionError:
        pass  # A different boot can still prove staleness; otherwise fail below.
    identity = None
    if isinstance(record, dict):
        if "processIdentity" in record and "linuxProcessIdentity" in record:
            raise DeliveryError("ambiguous daemon process identity")
        identity = record.get("processIdentity", record.get("linuxProcessIdentity"))
    if identity is not None:
        if (not isinstance(identity, dict) or not isinstance(identity.get("bootId"), str)
                or not identity["bootId"] or type(identity.get("startTicks")) is not int
                or not 0 <= identity["startTicks"] <= 2**64 - 1):
            raise DeliveryError("ambiguous daemon process identity")
        try:
            boot_id = (proc_root / "sys/kernel/random/boot_id").read_text().strip()
        except OSError:
            raise DeliveryError("cannot read Linux boot identity; installation refused") from None
        if not boot_id:
            raise DeliveryError("cannot read Linux boot identity; installation refused")
        if identity["bootId"] != boot_id:
            return False  # A prior boot proves staleness without inspecting the PID.
    try:
        state, ticks = read_process_stat(process)
    except FileNotFoundError:
        if not process.exists():
            return False
        raise DeliveryError("cannot establish daemon process identity") from None
    except OSError:
        raise DeliveryError("cannot establish daemon process identity") from None
    if state == "Z":
        return False
    if identity is not None:
        return ticks == identity["startTicks"]
    # Legacy wall-clock processStartTime is locale/clock dependent. An extant
    # non-zombie PID cannot safely be proven stale from that field alone.
    return True


def active_processes(xdcode_home: Path, proc_root: Path = Path("/proc")) -> list[int]:
    """Observe processes, never stop them. The alternate proc root is test-only."""
    active = set()
    names = {"xdcode", "xdcode-code-mode-host", "xdcode-app-server"}
    for process in proc_root.iterdir():
        if not process.name.isdecimal() or int(process.name) == os.getpid():
            continue
        try:
            if process.stat().st_uid != os.getuid():
                continue
            argv = (process / "cmdline").read_bytes().split(b"\0")
            argv0 = os.fsdecode(argv[0]) if argv else ""
            try:
                exe = os.readlink(process / "exe").removesuffix(" (deleted)")
            except FileNotFoundError:
                exe = ""  # A process may exit during inspection.
            if Path(argv0).name in names or Path(exe).name in names:
                state, _ = read_process_stat(process)
                if state != "Z":
                    active.add(int(process.name))
        except (FileNotFoundError, ProcessLookupError):
            continue
        except PermissionError:
            raise DeliveryError("cannot inspect a same-user process; installation refused") from None
    for name in ("daemon.pid", "daemon-updater.pid", "app-server.pid", "app-server-updater.pid"):
        path = xdcode_home / "app-server-daemon" / name
        if path.is_symlink():
            raise DeliveryError("refusing a symlinked daemon PID record")
        try:
            record = json.loads(path.read_text(encoding="utf-8"))
        except FileNotFoundError:
            continue
        except (ValueError, OSError):
            raise DeliveryError("cannot establish daemon state from its PID record") from None
        if pid_record_is_live(record, proc_root):
            active.add(record["pid"] if isinstance(record, dict) else record)
    return sorted(active)


def require_idle(xdcode_home: Path) -> None:
    active = active_processes(xdcode_home)
    if active:
        raise DeliveryError("active xdcode daemon/tasks; wait for completion and stop xdcode first (PIDs: "
                            + ", ".join(map(str, active)) + ")")


def check_entrypoint(entry: Path, install_root: Path) -> None:
    if not os.path.lexists(entry):
        return
    if not entry.is_symlink() or entry.lstat().st_uid != os.getuid():
        raise DeliveryError("refusing to replace an existing non-managed xdcode entry")
    target = Path(os.readlink(entry))
    if not target.is_absolute():
        raise DeliveryError("refusing to replace an unrecognized xdcode symlink")
    target = checked_path(target)
    if target.parent.name != "bin" or target.name != "xdcode" or target.parent.parent.parent != install_root:
        raise DeliveryError("existing xdcode entry does not point to this installation root")
    build_dir = target.parent.parent
    try:
        info = json.loads((build_dir / BUILD_INFO).read_text(encoding="utf-8"))
        manifest = json.loads((build_dir / MANIFEST).read_text(encoding="utf-8"))
        identity = validate_build_info(info)
        validate_manifest(manifest, info)
    except (OSError, ValueError):
        raise DeliveryError("existing xdcode entry has no valid installation metadata") from None
    if build_dir.name != identity or not target.is_file():
        raise DeliveryError("existing xdcode entry has an invalid installation identity")


def expected_archive_digest(archive: Path, explicit: str | None) -> str:
    if explicit is not None:
        if not DIGEST.fullmatch(explicit):
            raise DeliveryError("--sha256 must be a lowercase SHA-256 digest")
        return explicit
    sidecar = Path(str(archive) + ".sha256")
    try:
        contents = sidecar.read_text(encoding="utf-8")
    except OSError:
        raise DeliveryError("supply the local archive .sha256 sidecar or --sha256") from None
    match = re.fullmatch(r"([a-f0-9]{64})  ([^\n]+)\n?", contents)
    if not match or match[2] != archive.name:
        raise DeliveryError("invalid archive checksum sidecar")
    return match[1]


def unpack_verified(archive: tarfile.TarFile, stage: Path) -> dict:
    members = {}
    total = 0
    for member in archive:
        name = member.name
        if (name in members or str(PurePosixPath(name)) != name or ".." in PurePosixPath(name).parts
                or name.startswith("/") or len(members) >= 64):
            raise DeliveryError("unsafe or duplicate archive member")
        if member.isfile() and name in FILES:
            if member.mode & 0o7022:
                raise DeliveryError("unsafe archive file permissions")
            total += member.size
            if member.size < 0 or total > 4 * 1024**3:
                raise DeliveryError("archive exceeds package size limit")
            if name not in BINARIES and member.size > 1024**2:
                raise DeliveryError("oversized package metadata")
        elif not (member.isdir() and name in DIRECTORIES):
            raise DeliveryError("unexpected member, link or special file in archive")
        members[name] = member
    if {name for name, member in members.items() if member.isfile()} != FILES:
        raise DeliveryError("archive is missing required package files")
    for name in sorted(FILES):
        member = members[name]
        dest = stage / name
        dest.parent.mkdir(parents=True, exist_ok=True)
        with archive.extractfile(member) as source, dest.open("xb") as output:
            shutil.copyfileobj(source, output)
        dest.chmod(0o755 if name in BINARIES else 0o644)
    checksums = {}
    for line in (stage / "SHA256SUMS").read_text(encoding="utf-8").splitlines():
        match = re.fullmatch(r"([a-f0-9]{64})  (.+)", line)
        if not match or match[2] not in PAYLOAD or match[2] in checksums:
            raise DeliveryError("invalid package checksum manifest")
        checksums[match[2]] = match[1]
    if set(checksums) != PAYLOAD:
        raise DeliveryError("package checksum manifest must cover every payload file")
    for name, digest in checksums.items():
        if sha256_file(stage / name) != digest:
            raise DeliveryError(f"package checksum mismatch: {name}")
    info = json.loads((stage / BUILD_INFO).read_text(encoding="utf-8"))
    validate_build_info(info)
    validate_manifest(json.loads((stage / MANIFEST).read_text(encoding="utf-8")), info)
    if checksums["MODIFICATIONS.md"] != info["modifications_sha256"]:
        raise DeliveryError("derivative change notices differ from the build receipt")
    for name in BINARIES:
        require_elf(stage / name)
        if checksums[name] != info["binary_sha256"][name]:
            raise DeliveryError(f"executable differs from build receipt: {name}")
    return info


@contextlib.contextmanager
def install_lock(root: Path):
    descriptor = os.open(root / ".install.lock", os.O_CREAT | os.O_RDWR | os.O_NOFOLLOW, 0o600)
    try:
        metadata = os.fstat(descriptor)
        if not stat.S_ISREG(metadata.st_mode) or metadata.st_uid != os.getuid() or metadata.st_nlink != 1:
            raise DeliveryError("unsafe installer lock")
        try:
            fcntl.flock(descriptor, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError:
            raise DeliveryError("another xdcode installation is in progress") from None
        yield
    finally:
        os.close(descriptor)


def install(archive_path: Path, install_root: Path, bin_dir: Path, xdcode_home: Path,
            *, sha256: str | None = None, force: bool = False) -> Path:
    glibc = host_glibc()
    install_root, bin_dir, xdcode_home = validate_targets(install_root, bin_dir, xdcode_home)
    if force:
        print(
            "xdcode install: warning: --force skips active daemon/task checks; "
            "running processes keep using their current builds. Restart them safely to use the new build.",
            file=sys.stderr,
        )
    else:
        require_idle(xdcode_home)
    entry = bin_dir / "xdcode"
    check_entrypoint(entry, install_root)
    expected = expected_archive_digest(archive_path, sha256)
    # Hash and parse the same open regular file; never follow an archive symlink.
    descriptor = os.open(archive_path, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK)
    with os.fdopen(descriptor, "rb") as source:
        if not stat.S_ISREG(os.fstat(source.fileno()).st_mode):
            raise DeliveryError("archive must be a regular file")
        if hash_stream(source) != expected:
            raise DeliveryError("archive checksum mismatch")
        source.seek(0)
        install_root.mkdir(parents=True, exist_ok=True)
        bin_dir.mkdir(parents=True, exist_ok=True)
        validate_targets(install_root, bin_dir, xdcode_home)
        with install_lock(install_root), tempfile.TemporaryDirectory(prefix=".staging-", dir=install_root) as temp:
            stage = Path(temp) / "package"
            stage.mkdir()
            with tarfile.open(fileobj=source, mode="r:gz") as archive:
                info = unpack_verified(archive, stage)
            if tuple(map(int, glibc.split("."))) < tuple(map(int, info["requires_glibc"].split("."))):
                raise DeliveryError(f"package requires glibc {info['requires_glibc']} or newer")
            identity = validate_build_info(info)
            if archive_path.name != f"xdcode-{identity}-{PLATFORM}.tar.gz":
                raise DeliveryError("archive filename does not match package identity")
            dest = install_root / identity
            if os.path.lexists(dest):
                raise DeliveryError("build is already installed; existing builds are never overwritten")
            validate_runtime(stage, info["version"])
            if not force:
                require_idle(xdcode_home)
            check_entrypoint(entry, install_root)
            stage.rename(dest)
            # Keep a verified version directory on late failure. Never remove an old build.
            # The entry directory may be on a different filesystem than the builds.
            with tempfile.TemporaryDirectory(prefix=".xdcode-entry-", dir=bin_dir) as entry_temp:
                temporary_entry = Path(entry_temp) / "xdcode"
                temporary_entry.symlink_to(dest / "bin/xdcode")
                if not force:
                    require_idle(xdcode_home)
                check_entrypoint(entry, install_root)
                os.replace(temporary_entry, entry)
    return dest


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("archive", type=Path, help="local .tar.gz (and adjacent .sha256)")
    parser.add_argument("--sha256", help="trusted archive digest instead of the sidecar")
    parser.add_argument("--force", action="store_true",
                        help="allow active xdcode processes; keep all integrity and path checks")
    parser.add_argument("--install-root", type=Path, default=Path.home() / ".local/lib/xdcode")
    parser.add_argument("--bin-dir", type=Path, default=Path.home() / ".local/bin")
    parser.add_argument("--xdcode-home", type=Path,
                        default=Path(os.environ.get("XDCODE_HOME", Path.home() / ".xdcode")))
    args = parser.parse_args(argv)
    try:
        dest = install(args.archive, args.install_root, args.bin_dir, args.xdcode_home,
                       sha256=args.sha256, force=args.force)
    except (DeliveryError, OSError, ValueError, tarfile.TarError) as error:
        print(f"xdcode install: {error}", file=sys.stderr)
        return 1
    print(f"Installed {dest}\nEntrypoint: {args.bin_dir / 'xdcode'}\nOld builds and runtime data retained.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
