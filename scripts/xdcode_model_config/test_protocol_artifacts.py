"""Offline contract checks for shipped model configuration/protocol artifacts.

These tests parse schema resources only. They never execute xdcode or compile Rust.
Run with the existing Python environment: python -m unittest discover
-s scripts/xdcode_model_config -p 'test_*.py' -v.
"""
import ast
import json
from pathlib import Path
import unittest

import jsonschema
import zstandard

ROOT = Path(__file__).resolve().parents[2]
SCHEMAS = ROOT / "xdcode-rs/app-server-protocol/schema"


def read_exports(experimental):
    kind = "experimental" if experimental else "stable"
    path = SCHEMAS / f"precomputed/app-server-exports-{kind}.json.zst"
    with path.open("rb") as source:
        with zstandard.ZstdDecompressor().stream_reader(source) as reader:
            return json.loads(reader.read())


class ModelProtocolArtifacts(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.stable = read_exports(False)
        cls.experimental = read_exports(True)
        cls.config = json.loads((ROOT / "xdcode-rs/core/config.schema.json").read_text())
        cls.request = json.loads(cls.experimental["json_schema"]["v2/ThreadSettingsUpdateParams.json"])

    def test_configuration_accepts_same_wire_model_on_distinct_endpoints(self):
        value = {
            "model_providers": {
                "one": {"name": "One", "base_url": "http://127.0.0.1:1234/v1", "wire_api": "responses", "env_key": "ONE_KEY"},
                "two": {"name": "Two", "base_url": "http://127.0.0.1:5678/api/v1", "wire_api": "responses", "env_key": "TWO_KEY"},
            },
            "models": [
                {"id": "first", "name": "Shared", "provider": "one", "model": "same", "reasoning_efforts": ["vendor-level"]},
                {"id": "second", "name": "Shared", "provider": "two", "model": "same", "reasoning_efforts": []},
            ],
        }
        jsonschema.validate(value, self.config)

    def test_config_schema_preserves_omitted_empty_and_literal_none(self):
        base = {"id": "m", "name": "M", "provider": "p", "model": "exact:free"}
        for efforts in (None, [], ["none"], ["custom-level"]):
            entry = dict(base)
            if efforts is not None:
                entry["reasoning_efforts"] = efforts
            jsonschema.validate({"models": [entry]}, self.config)

    def test_atomic_selection_carries_complete_route_and_mode(self):
        choice = {"provider": "second", "catalogId": "stable-id", "model": "exact:free", "effort": "custom-level", "mode": "plan"}
        jsonschema.validate({"threadId": "thread", "catalogSelection": choice}, self.request)
        for field in ("provider", "catalogId", "model", "mode"):
            incomplete = dict(choice)
            del incomplete[field]
            with self.assertRaises(jsonschema.ValidationError):
                jsonschema.validate({"threadId": "thread", "catalogSelection": incomplete}, self.request)

    def test_effort_omission_clear_literal_none_and_custom_values_are_wire_valid(self):
        for patch in ({}, {"effort": None}, {"effort": "none"}, {"effort": "vendor"}):
            jsonschema.validate({"threadId": "thread", **patch}, self.request)
        with self.assertRaises(jsonschema.ValidationError):
            jsonschema.validate({"threadId": "thread", "effort": 7}, self.request)

    def test_public_exports_preserve_experimental_boundary(self):
        self.assertNotIn("v2/ThreadSettingsUpdateParams.json", self.stable["json_schema"])
        self.assertNotIn("CatalogModelSelection.ts", self.stable["typescript"])
        self.assertIn("CatalogModelSelection.ts", self.experimental["typescript"])
        self.assertIn("catalogSelection?: CatalogModelSelection", self.experimental["typescript"]["v2/ThreadSettingsUpdateParams.ts"])

    def test_checked_in_stable_exports_match_embedded_copies(self):
        for category, directory in (("json_schema", "json"), ("typescript", "typescript")):
            for name, contents in self.stable[category].items():
                file = SCHEMAS / directory / name
                if not file.exists():
                    continue
                if name.endswith(".json"):
                    self.assertEqual(json.loads(file.read_text()), json.loads(contents), name)
                else:
                    self.assertEqual(file.read_text(), contents, name)

    def test_all_embedded_local_json_references_resolve(self):
        for exports in (self.stable, self.experimental):
            for category in ("json_schema", "internal_json_schema"):
                for name, raw in exports[category].items():
                    value = json.loads(raw)
                    def visit(node):
                        if isinstance(node, dict):
                            ref = node.get("$ref", "")
                            if ref.startswith("#/"):
                                target = value
                                for token in ref[2:].split("/"):
                                    target = target[token.replace("~1", "/").replace("~0", "~")]
                            for child in node.values():
                                visit(child)
                        elif isinstance(node, list):
                            for child in node:
                                visit(child)
                    try:
                        visit(value)
                    except KeyError as error:
                        self.fail(f"{category}/{name}: unresolved reference {error}")

    def test_saved_thread_settings_include_optional_catalog_identity(self):
        schema = json.loads(self.stable["internal_json_schema"]["RolloutLine.json"])
        settings = schema["definitions"]["ThreadSettingsSnapshot"]
        self.assertIn("catalog_id", settings["properties"])
        self.assertNotIn("catalog_id", settings["required"])
        self.assertIn("provider_fingerprint", settings["properties"])
        self.assertNotIn("provider_fingerprint", settings["required"])
        turn = schema["definitions"]["TurnContextItem"]
        for field in ("model_provider_id", "provider_fingerprint"):
            self.assertIn(field, turn["properties"])
            self.assertNotIn(field, turn["required"])
        for exports in (self.stable, self.experimental):
            schema = json.loads(exports["json_schema"]["v2/ThreadSettingsUpdatedNotification.json"])
            self.assertIn("catalogId", schema["definitions"]["ThreadSettings"]["properties"])

    def test_start_resume_and_fork_return_exact_optional_catalog_identity(self):
        for exports in (self.stable, self.experimental):
            for name in ("ThreadStartResponse", "ThreadResumeResponse", "ThreadForkResponse"):
                schema = json.loads(exports["json_schema"][f"v2/{name}.json"])
                self.assertIn("catalogId", schema["properties"], name)
                self.assertNotIn("catalogId", schema["required"], name)
                # Absent IDs are omitted by serde rather than emitted as null.
                self.assertIn("catalogId?: string,", exports["typescript"][f"v2/{name}.ts"])
        sdk = ast.parse((ROOT / "sdk/python/src/openai_xdcode/generated/v2_all.py").read_text())
        for name in ("ThreadStartResponse", "ThreadResumeResponse", "ThreadForkResponse"):
            model = next(node for node in sdk.body if isinstance(node, ast.ClassDef) and node.name == name)
            field = next(node for node in model.body if isinstance(node, ast.AnnAssign) and node.target.id == "catalog_id")
            self.assertIsNone(ast.literal_eval(field.value))
            self.assertEqual(ast.unparse(field.annotation.slice.elts[0]), "str | None")
            metadata = field.annotation.slice.elts[1]
            alias = next(value.value for value in metadata.keywords if value.arg == "alias")
            self.assertEqual(ast.literal_eval(alias), "catalogId")

    def test_product_version_remains_upstream_stable(self):
        self.assertEqual(json.loads((ROOT / "upstream.json").read_text())["version"], "0.165.0")
        # Read the manifest as text: no dependency resolution or cargo invocation.
        self.assertIn('version = "0.165.0"', (ROOT / "xdcode-rs/Cargo.toml").read_text())


if __name__ == "__main__":
    unittest.main()
