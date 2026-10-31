"""Offline bundled-resource and exact URL behavior checks; no model requests."""

import importlib.util
import json
from pathlib import Path
import re
import subprocess
import sys
import tempfile
import unittest
from urllib import request
from unittest import mock

import yaml


SAMPLES = Path(__file__).resolve().parents[1] / "src/assets/samples"
SKILL = SAMPLES / "xdcode-model-config"
SPEC = importlib.util.spec_from_file_location(
    "bundled_openrouter_model_id", SKILL / "scripts/openrouter_model_id.py"
)
extractor = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(extractor)


class ModelConfigSkillTests(unittest.TestCase):
    def test_extracts_exact_free_id_without_query_or_fragment(self):
        with mock.patch.object(request, "urlopen", side_effect=AssertionError("offline")):
            self.assertEqual(
                extractor.extract_model_id(
                    "https://openrouter.ai/minimax/minimax-m3.1-flash:free?ref=sharing#pricing"
                ),
                "minimax/minimax-m3.1-flash:free",
            )

    def test_decodes_url_path_without_replacing_routing_suffix(self):
        self.assertEqual(
            extractor.extract_model_id("https://openrouter.ai/minimax/minimax-m3.1-flash%3Afree/"),
            "minimax/minimax-m3.1-flash:free",
        )

    def test_rejects_non_model_and_untrusted_urls(self):
        for url in [
            "http://openrouter.ai/vendor/model",
            "https://openrouter.ai.attacker.invalid/vendor/model",
            "https://user:secret@openrouter.ai/vendor/model",
            "https://openrouter.ai:443/vendor/model",
            "https://openrouter.ai/models",
            "https://openrouter.ai/api/v1/models",
            "https://openrouter.ai/vendor/..",
            "https://openrouter.ai/vendor/%0Amodel",
        ]:
            with self.subTest(url=url), self.assertRaises(ValueError):
                extractor.extract_model_id(url)

    def test_helper_runs_from_an_unrelated_directory_and_never_claims_verification(self):
        with tempfile.TemporaryDirectory() as directory:
            completed = subprocess.run(
                [
                    sys.executable,
                    str(SKILL / "scripts/openrouter_model_id.py"),
                    "https://openrouter.ai/minimax/minimax-m3.1-flash:free",
                ],
                cwd=directory,
                check=True,
                capture_output=True,
                text=True,
            )
        self.assertEqual(
            json.loads(completed.stdout),
            {"model": "minimax/minimax-m3.1-flash:free", "verification": "unverified"},
        )

    def test_skill_metadata_allows_implicit_discovery(self):
        text = (SKILL / "SKILL.md").read_text(encoding="utf-8")
        frontmatter = yaml.safe_load(text.split("---", 2)[1])
        self.assertEqual(frontmatter["name"], SKILL.name)
        self.assertTrue(frontmatter["description"])
        metadata = yaml.safe_load((SKILL / "agents/xdcode.yaml").read_text(encoding="utf-8"))
        self.assertTrue(metadata["policy"]["allow_implicit_invocation"])
        self.assertIn("$" + SKILL.name, metadata["interface"]["default_prompt"])

    def test_linked_references_resolve_inside_the_bundled_skill(self):
        markdown = [SKILL / "SKILL.md", *(SKILL / "references").glob("*.md")]
        for source in markdown:
            for target in re.findall(r"\[[^\]]+\]\(([^)]+)\)", source.read_text(encoding="utf-8")):
                if "://" in target or target.startswith("#"):
                    continue
                path = (source.parent / target.split("#")[0]).resolve()
                with self.subTest(source=source.name, target=target):
                    self.assertTrue(path.is_relative_to(SKILL.resolve()))
                    self.assertTrue(path.is_file())

    def test_documented_json_payloads_parse_and_keep_credentials_out(self):
        # This validates actual machine input examples, not wording of advice.
        examples = []
        for reference in (SKILL / "references").glob("*.md"):
            examples.extend(re.findall(r"```json\n(.*?)\n```", reference.read_text(), re.S))
        self.assertGreaterEqual(len(examples), 2)
        for example in examples:
            payload = json.loads(example)
            self.assertIsInstance(payload["models"], list)
            for provider in payload.get("providers", {}).values():
                self.assertEqual(provider["wire_api"], "responses")
                self.assertNotIn("experimental_bearer_token", provider)
                self.assertNotIn("http_headers", provider)
                self.assertNotIn("auth", provider)


if __name__ == "__main__":
    unittest.main()
