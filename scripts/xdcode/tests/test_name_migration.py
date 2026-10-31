"""Regression checks for implementation renaming versus external contracts."""

import importlib.util
from pathlib import Path
import unittest

SOURCE = Path(__file__).resolve().parents[1] / "neutralize_names.py"
SPEC = importlib.util.spec_from_file_location("name_migration", SOURCE)
migration = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(migration)


class NameMigrationTests(unittest.TestCase):
    def test_internal_names_and_config_properties_are_neutral(self):
        original = "use api::openai_models::OpenAiModelsManager; requires_openai_auth = false;"
        self.assertEqual(migration.translate(original),
                         "use api::model_types::ProviderModelsManager; requires_hosted_auth = false;")

    def test_form_type_changes_without_changing_wire_tag(self):
        original = 'enum OpenAiForm {} #[serde(rename = "openaiForm")] struct OpenAiFormRequestParams;'
        self.assertEqual(migration.translate(original),
                         'enum ProviderForm {} #[serde(rename = "openaiForm")] struct ProviderFormRequestParams;')

    def test_service_hosts_headers_metadata_and_model_ids_are_preserved(self):
        for value in (
            '"https://api.openai.com/v1"', '"openai.com"', '"OpenAI-Beta"',
            '"x-openai-internal-codex-residency"', '"openai/fileParams"',
            '"openai.gpt-5.6-sol"', '"codex/encryptedContent"',
            '"codex/imageDetail"', '"codex_rollout_budget_units"',
            '"openai.azure."', '"windows.net/openai"',
        ):
            with self.subTest(value=value):
                self.assertEqual(migration.translate(value), value)

    def test_skill_metadata_uses_application_filename(self):
        self.assertEqual(migration.translate('agents/openai.yaml generate_openai_yaml.py'),
                         'agents/xdcode.yaml generate_xdcode_yaml.py')

    def test_portable_metadata_input_constant_is_preserved(self):
        value = 'const SKILLS_PORTABLE_METADATA_FILENAME: &str = "openai.yaml";'
        self.assertEqual(migration.translate(value), value)

    def test_legal_attribution_is_not_rewritten(self):
        notice = "// Copyright 2025 OpenAI\nstruct OpenAiApi;"
        self.assertEqual(migration.translate(notice), "// Copyright 2025 OpenAI\nstruct ProviderApi;")


if __name__ == "__main__":
    unittest.main()
