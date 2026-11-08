"""Release integrity and publication tests without network access or native builds."""

from __future__ import annotations

import io
import json
import subprocess
import sys
import tarfile
import tempfile
import unittest
from pathlib import Path
from unittest import mock

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

import install
import release


class ReleaseTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="xdcode-release-test-")
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.version = "0.167.0"
        self.commit = "a" * 40
        self.installer = b"standalone installer fixture\n"
        self.archives = {}
        for directory, platform, target in release.PLATFORMS:
            self.make_archive(directory, platform, target)

    def make_archive(
        self, directory: str, platform: str, expected_target: str, **changes
    ):
        metadata = {
            "schema_version": 1,
            "version": self.version,
            "build_id": "g" + self.commit[:12],
            "source_commit": self.commit,
            "source_dirty": False,
            "target": expected_target,
            **changes,
        }
        destination = self.root / directory
        destination.mkdir(exist_ok=True)
        path = (
            destination / f"xdcode-{self.version}-g{self.commit[:12]}-{platform}.tar.gz"
        )
        payload = {install.BUILD_INFO: json.dumps(metadata).encode()}
        if directory == "linux":
            payload["install.py"] = self.installer
            path.with_name(
                path.name.removesuffix(".tar.gz") + ".install.py"
            ).write_bytes(self.installer)
        with tarfile.open(path, "w:gz") as archive:
            for name, contents in payload.items():
                member = tarfile.TarInfo(name)
                member.size = len(contents)
                archive.addfile(member, io.BytesIO(contents))
        Path(str(path) + ".sha256").write_text(
            f"{install.sha256_file(path)}  {path.name}\n"
        )
        self.archives[directory] = path

    def test_collects_both_platforms_and_linux_installer(self):
        assets = release.collect_assets(self.root, self.version, self.commit)
        self.assertEqual(len(assets), 5)
        self.assertTrue(all(path.is_file() for path in assets))
        self.assertEqual(sum(path.suffix == ".py" for path in assets), 1)

    def test_mixed_source_dirty_version_and_target_are_rejected(self):
        for changes in (
            {"source_commit": "b" * 40},
            {"source_dirty": True},
            {"source_dirty": "false"},
            {"version": "0.165.0"},
            {"target": install.TARGET},
        ):
            with self.subTest(changes=changes):
                self.make_archive(
                    "macos", "macos-arm64", "aarch64-apple-darwin", **changes
                )
                with self.assertRaisesRegex(install.DeliveryError, "metadata"):
                    release.collect_assets(self.root, self.version, self.commit)

    def test_archive_checksum_mismatch_is_rejected(self):
        path = self.archives["linux"]
        Path(str(path) + ".sha256").write_text(f"{'0' * 64}  {path.name}\n")
        with self.assertRaisesRegex(install.DeliveryError, "checksum mismatch"):
            release.collect_assets(self.root, self.version, self.commit)

    def test_checksum_filename_binding_is_required(self):
        path = self.archives["linux"]
        Path(str(path) + ".sha256").write_text(
            f"{install.sha256_file(path)}  other.tar.gz\n"
        )
        with self.assertRaisesRegex(install.DeliveryError, "sidecar"):
            release.collect_assets(self.root, self.version, self.commit)

    def test_standalone_installer_must_match_archive(self):
        path = self.archives["linux"]
        path.with_name(path.name.removesuffix(".tar.gz") + ".install.py").write_text(
            "different installer"
        )
        with self.assertRaisesRegex(install.DeliveryError, "installer differs"):
            release.collect_assets(self.root, self.version, self.commit)

    def test_missing_platform_or_extra_files_are_rejected(self):
        path = self.archives["macos"]
        path.unlink()
        with self.assertRaisesRegex(install.DeliveryError, "exactly one"):
            release.collect_assets(self.root, self.version, self.commit)
        self.make_archive("macos", "macos-arm64", "aarch64-apple-darwin")
        (self.root / "linux/private-config.toml").write_text("fixture")
        with self.assertRaisesRegex(install.DeliveryError, "unexpected"):
            release.collect_assets(self.root, self.version, self.commit)

    def test_uploads_draft_before_publishing_pinned_commit(self):
        commands = []

        def capture(command, **kwargs):
            commands.append(command)
            self.assertTrue(kwargs["check"])
            if command[2] == "create":
                notes = Path(command[command.index("--notes-file") + 1]).read_text()
                self.assertIn("0.165.0", notes)
                self.assertIn(self.commit, notes)

        with mock.patch.object(release.subprocess, "run", side_effect=capture):
            tag = release.publish_release(
                self.root, self.version, "0.165.0", self.commit, "owner/xdcode"
            )
        self.assertEqual(tag, "v0.167.0-gaaaaaaaaaaaa")
        self.assertEqual(len(commands), 2)
        self.assertIn("--draft", commands[0])
        self.assertEqual(commands[0][commands[0].index("--target") + 1], self.commit)
        for path in release.collect_assets(self.root, self.version, self.commit):
            self.assertIn(str(path), commands[0])
        self.assertIn("--draft=false", commands[1])
        self.assertIn("--latest", commands[1])

    def test_failed_upload_does_not_publish_release(self):
        with (
            mock.patch.object(
                release.subprocess,
                "run",
                side_effect=subprocess.CalledProcessError(1, ["gh"]),
            ) as command,
            self.assertRaises(subprocess.CalledProcessError),
        ):
            release.publish_release(
                self.root, self.version, "0.165.0", self.commit, "owner/xdcode"
            )
        self.assertEqual(command.call_count, 1)
