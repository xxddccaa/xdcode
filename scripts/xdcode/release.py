"""Publish verified Linux and Apple Silicon artifacts from one clean commit."""

from __future__ import annotations

import argparse
import hashlib
import json
import subprocess
import sys
import tarfile
import tempfile
from pathlib import Path

import install

REPO_ROOT = Path(__file__).resolve().parents[2]
PLATFORMS = (
    ("linux", "linux-x86_64", install.TARGET),
    ("macos", "macos-arm64", "aarch64-apple-darwin"),
)


def read_member(archive: tarfile.TarFile, name: str) -> bytes:
    members = [member for member in archive.getmembers() if member.name == name]
    if len(members) != 1 or not members[0].isfile() or members[0].size > 1024 * 1024:
        raise install.DeliveryError(f"missing or unsafe release member: {name}")
    with archive.extractfile(members[0]) as stream:
        return stream.read()


def collect_assets(root: Path, version: str, source_commit: str) -> list[Path]:
    if not install.STABLE_VERSION.fullmatch(version) or not install.COMMIT.fullmatch(
        source_commit
    ):
        raise install.DeliveryError(
            "release requires a stable version and full source commit"
        )
    assets = []
    for directory_name, platform, target in PLATFORMS:
        directory = root / directory_name
        archives = sorted(directory.glob(f"xdcode-*-{platform}.tar.gz"))
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
        with tarfile.open(archive_path, "r:gz") as archive:
            metadata = json.loads(read_member(archive, install.BUILD_INFO))
            if not isinstance(metadata, dict) or any(
                (
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
            if archive_path.name != f"xdcode-{version}-{build_id}-{platform}.tar.gz":
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
    root: Path, version: str, upstream_version: str, source_commit: str, repository: str
) -> str:
    assets = collect_assets(root, version, source_commit)
    build_id = "g" + source_commit[:12]
    tag = f"v{version}-{build_id}"
    notes = (
        f"Product version: **xdcode {version}**. Recorded OpenAI Codex baseline: **{upstream_version}**.\n\n"
        f"Built from clean source commit `{source_commit}`.\n\n"
        "- GNU/Linux x86_64 archive, SHA-256 sidecar, and standalone offline installer.\n"
        "- macOS Apple Silicon (ARM64) archive and SHA-256 sidecar; Intel Macs are not supported.\n"
        "- The Linux installer accepts explicit `--force` to switch the entrypoint without stopping existing sessions; integrity checks and old builds are retained.\n"
        "- macOS executables are ad-hoc signed, not Developer ID signed or notarized; see the bundled installation guide.\n\n"
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
                "--latest",
            ],
            check=True,
        )
    return tag


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--assets-dir", type=Path, required=True)
    parser.add_argument("--source-commit", required=True)
    parser.add_argument("--repo", required=True)
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
        )
    except (
        install.DeliveryError,
        OSError,
        ValueError,
        tarfile.TarError,
        subprocess.CalledProcessError,
    ) as error:
        print(f"xdcode release: {error}", file=sys.stderr)
        return 1
    print(f"Published {tag}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
