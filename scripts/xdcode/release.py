"""Publish verified Linux, Apple Silicon and Windows artifacts from one clean commit."""

from __future__ import annotations

import argparse
import hashlib
import json
import subprocess
import sys
import tarfile
import tempfile
import zipfile
from pathlib import Path

import install

REPO_ROOT = Path(__file__).resolve().parents[2]
PLATFORMS = (
    ("linux", "linux-x86_64", install.TARGET),
    ("macos", "macos-arm64", "aarch64-apple-darwin"),
    ("windows", "windows-x86_64", "x86_64-pc-windows-msvc"),
)


def read_member(archive: tarfile.TarFile | zipfile.ZipFile, name: str) -> bytes:
    if isinstance(archive, zipfile.ZipFile):
        members = [member for member in archive.infolist() if member.filename == name]
        if (
            len(members) != 1
            or members[0].is_dir()
            or members[0].file_size > 1024 * 1024
            or (members[0].external_attr >> 16) & 0o170000 not in (0, 0o100000)
        ):
            raise install.DeliveryError(f"missing or unsafe release member: {name}")
        with archive.open(members[0]) as stream:
            return stream.read()
    members = [member for member in archive.getmembers() if member.name == name]
    if len(members) != 1 or not members[0].isfile() or members[0].size > 1024 * 1024:
        raise install.DeliveryError(f"missing or unsafe release member: {name}")
    with archive.extractfile(members[0]) as stream:
        return stream.read()


def collect_assets(
    root: Path, version: str, source_commit: str, *, windows_only: bool = False
) -> list[Path]:
    if not install.STABLE_VERSION.fullmatch(version) or not install.COMMIT.fullmatch(
        source_commit
    ):
        raise install.DeliveryError(
            "release requires a stable version and full source commit"
        )
    assets = []
    selected_platforms = (PLATFORMS[-1],) if windows_only else PLATFORMS
    for directory_name, platform, target in selected_platforms:
        directory = root / directory_name
        extension = "zip" if directory_name == "windows" else "tar.gz"
        archives = sorted(directory.glob(f"xdcode-*-{platform}.{extension}"))
        if len(archives) != 1:
            raise install.DeliveryError(
                f"release requires exactly one {platform} archive"
            )
        archive_path = archives[0]
        sidecar = Path(str(archive_path) + ".sha256")
        platform_assets = [archive_path, sidecar]
        if any(path.is_symlink() or not path.is_file() for path in platform_assets):
            raise install.DeliveryError(f"missing or unsafe {platform} release file")
        if install.sha256_file(archive_path) != install.expected_archive_digest(
            archive_path, None
        ):
            raise install.DeliveryError(
                f"release archive checksum mismatch: {platform}"
            )
        with (
            zipfile.ZipFile(archive_path)
            if extension == "zip"
            else tarfile.open(archive_path, "r:gz")
        ) as archive:
            metadata = json.loads(read_member(archive, install.BUILD_INFO))
            if not isinstance(metadata, dict) or any(
                (
                    type(metadata.get("schema_version")) is not int,
                    metadata.get("schema_version") != 1,
                    metadata.get("version") != version,
                    metadata.get("source_commit") != source_commit,
                    metadata.get("source_dirty") is not False,
                    metadata.get("target") != target,
                )
            ):
                raise install.DeliveryError(
                    f"release metadata does not match the clean source: {platform}"
                )
            build_id = metadata.get("build_id")
            if not isinstance(build_id, str) or not install.BUILD_ID.fullmatch(
                build_id
            ):
                raise install.DeliveryError(f"unsafe release build ID: {platform}")
            if build_id != "g" + source_commit[:12]:
                raise install.DeliveryError(
                    f"release build ID does not match its tag: {platform}"
                )
            if (
                archive_path.name
                != f"xdcode-{version}-{build_id}-{platform}.{extension}"
            ):
                raise install.DeliveryError(
                    f"release filename does not match metadata: {platform}"
                )
            if platform == "linux-x86_64":
                installer = archive_path.with_name(
                    archive_path.name.removesuffix(".tar.gz") + ".install.py"
                )
                if installer.is_symlink() or not installer.is_file():
                    raise install.DeliveryError(
                        "missing or unsafe standalone Linux installer"
                    )
                if (
                    install.sha256_file(installer)
                    != hashlib.sha256(read_member(archive, "install.py")).hexdigest()
                ):
                    raise install.DeliveryError(
                        "standalone Linux installer differs from the archive"
                    )
                platform_assets.append(installer)
        if set(directory.iterdir()) != set(platform_assets):
            raise install.DeliveryError(f"unexpected release assets: {platform}")
        assets.extend(platform_assets)
    return assets


def publish_release(
    root: Path,
    version: str,
    upstream_version: str,
    source_commit: str,
    repository: str,
    *,
    windows_only: bool = False,
) -> str:
    assets = collect_assets(root, version, source_commit, windows_only=windows_only)
    build_id = "g" + source_commit[:12]
    tag = f"v{version}-{build_id}"
    platform_notes = (
        "This supplemental release contains Windows x86_64 only. Existing Linux/macOS releases are unchanged; this release does not replace GitHub's latest release.\n\n"
        if windows_only
        else "- GNU/Linux x86_64 archive, SHA-256 sidecar, and standalone offline installer.\n"
        "- macOS Apple Silicon (ARM64) archive and SHA-256 sidecar; Intel Macs are not supported.\n"
        "- The Linux installer accepts explicit `--force` to switch the entrypoint without stopping existing sessions; integrity checks and old builds are retained.\n"
        "- macOS executables are ad-hoc signed, not Developer ID signed or notarized; see the bundled installation guide.\n"
    )
    notes = (
        f"Product version: **xdcode {version}**. Recorded OpenAI Codex baseline: **{upstream_version}**.\n\n"
        f"Built from clean source commit `{source_commit}`.\n\n"
        f"{platform_notes}"
        "- Windows x86_64 ZIP archive and SHA-256 sidecar; ARM64 and 32-bit Windows are not supported.\n"
        "- Manual install.sh and install.ps1 entrypoints keep a stable installation command. install.ps1 selects the most recently published stable release with Windows assets.\n\n"
        "Windows executables are not Authenticode signed. The native probes do not provision Windows sandbox accounts; see the Windows delivery guide for prerequisites.\n\n"
        "Existing processes keep their current builds. Finish their tasks and restart them safely; installing does not upgrade a running daemon in place.\n"
    )
    with tempfile.TemporaryDirectory(prefix="xdcode-release-") as temporary:
        notes_path = Path(temporary) / "notes.md"
        notes_path.write_text(notes, encoding="utf-8")
        subprocess.run(
            [
                "gh",
                "release",
                "create",
                tag,
                "--repo",
                repository,
                "--target",
                source_commit,
                "--title",
                f"xdcode {version} · {build_id}",
                "--notes-file",
                str(notes_path),
                "--draft",
                *(["--latest=false"] if windows_only else []),
                *map(str, assets),
            ],
            check=True,
        )
        subprocess.run(
            [
                "gh",
                "release",
                "edit",
                tag,
                "--repo",
                repository,
                "--draft=false",
                "--latest=false" if windows_only else "--latest",
            ],
            check=True,
        )
    return tag


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--assets-dir", type=Path, required=True)
    parser.add_argument("--source-commit", required=True)
    parser.add_argument("--repo", required=True)
    parser.add_argument(
        "--windows-only",
        action="store_true",
        help="Publish only Windows assets without replacing the latest Linux/macOS release",
    )
    args = parser.parse_args(argv)
    try:
        upstream = json.loads((REPO_ROOT / "upstream.json").read_text())
        install.validate_upstream(upstream)
        tag = publish_release(
            args.assets_dir,
            upstream["version"],
            upstream.get("upstream_version", upstream["version"]),
            args.source_commit,
            args.repo,
            windows_only=args.windows_only,
        )
    except (
        install.DeliveryError,
        OSError,
        ValueError,
        tarfile.TarError,
        zipfile.BadZipFile,
        subprocess.CalledProcessError,
    ) as error:
        print(f"xdcode release: {error}", file=sys.stderr)
        return 1
    print(f"Published {tag}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
