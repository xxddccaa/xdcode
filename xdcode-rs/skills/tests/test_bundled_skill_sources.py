"""Offline behavior checks for bundled skill sources and xdcode metadata."""

import contextlib
import importlib.util
import io
import json
import os
from pathlib import Path
import sys
import tempfile
import unittest
from unittest import mock

import yaml


SAMPLES = Path(__file__).resolve().parents[1] / "src/assets/samples"
INSTALLER_SCRIPTS = SAMPLES / "skill-installer/scripts"
CREATOR_SCRIPTS = SAMPLES / "skill-creator/scripts"


def load_script(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    sys.modules[name] = module
    with mock.patch.object(sys, "path", [str(path.parent), *sys.path]):
        spec.loader.exec_module(module)
    return module


installer = load_script("bundled_skill_install", INSTALLER_SCRIPTS / "install-skill-from-github.py")
listing = load_script("bundled_skill_list", INSTALLER_SCRIPTS / "list-skills.py")
generator = load_script("generate_xdcode_yaml", CREATOR_SCRIPTS / "generate_xdcode_yaml.py")
initializer = load_script("bundled_skill_init", CREATOR_SCRIPTS / "init_skill.py")


class BundledSkillSourceTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory(dir=Path(__file__).parent)
        self.addCleanup(self.directory.cleanup)
        self.root = Path(self.directory.name)
        self.skill = self.root / "source" / "my-skill"
        self.skill.mkdir(parents=True)
        (self.skill / "SKILL.md").write_text(
            "---\nname: my-skill\ndescription: A local fixture for installation checks.\n---\n",
            encoding="utf-8",
        )
        (self.skill / "LICENSE.txt").write_text("Fixture license\n", encoding="utf-8")

    def test_install_requires_selected_source_before_io(self):
        with mock.patch.object(installer, "_request") as request:
            with contextlib.redirect_stderr(io.StringIO()), self.assertRaises(SystemExit) as error:
                installer.main([])
            self.assertEqual(error.exception.code, 2)
            request.assert_not_called()

    def test_local_install_preserves_license_and_does_not_overwrite(self):
        destination = self.root / "installed"
        args = ["--local", str(self.skill), "--dest", str(destination)]
        with mock.patch.object(installer, "_prepare_repo") as remote:
            with contextlib.redirect_stdout(io.StringIO()):
                self.assertEqual(installer.main(args), 0)
            self.assertEqual((destination / "my-skill/LICENSE.txt").read_text(), "Fixture license\n")
            with contextlib.redirect_stderr(io.StringIO()):
                self.assertEqual(installer.main(args), 1)
            remote.assert_not_called()

    def test_local_install_rejects_nested_destination(self):
        with contextlib.redirect_stderr(io.StringIO()):
            result = installer.main(["--local", str(self.skill), "--dest", str(self.skill / "nested")])
        self.assertEqual(result, 1)
        self.assertFalse((self.skill / "nested").exists())

    def test_default_listing_is_local_and_includes_bundled_skills(self):
        home = self.root / "home"
        (home / "skills/.system/built-in").mkdir(parents=True)
        (home / "skills/.system/built-in/SKILL.md").write_text("fixture\n")
        output = io.StringIO()
        with mock.patch.dict(os.environ, {"XDCODE_HOME": str(home)}):
            with mock.patch.object(listing, "_request") as request:
                with contextlib.redirect_stdout(output):
                    self.assertEqual(listing.main(["--format", "json"]), 0)
                request.assert_not_called()
        self.assertEqual(json.loads(output.getvalue()), [{"name": "built-in", "installed": True}])

    def test_remote_listing_requires_repo_and_path(self):
        for args in [["--repo", "example/selected"], ["--path", "skills"]]:
            with contextlib.redirect_stderr(io.StringIO()), self.assertRaises(SystemExit):
                listing.main(args)
        with mock.patch.object(listing, "_request", return_value=b'[{"type":"dir","name":"selected"}]') as request:
            with contextlib.redirect_stdout(io.StringIO()):
                self.assertEqual(listing.main(["--repo", "example/selected", "--path", "skills"]), 0)
            request.assert_called_once_with("https://api.github.com/repos/example/selected/contents/skills?ref=main")

    def test_creator_initializes_xdcode_metadata(self):
        with contextlib.redirect_stdout(io.StringIO()):
            created = initializer.init_skill(
                "new-skill", self.root / "generated", [], False,
                ["short_description=Create locally configured project skills"],
            )
        self.assertIsNotNone(created)
        metadata = created / "agents/xdcode.yaml"
        self.assertTrue(metadata.is_file())
        self.assertEqual(sorted(path.name for path in (created / "agents").iterdir()), ["xdcode.yaml"])
        self.assertEqual(yaml.safe_load(metadata.read_text())["interface"]["display_name"], "New Skill")


if __name__ == "__main__":
    unittest.main()
