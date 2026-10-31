//! User and built-in skill entry points for the same model catalog operations.
//! Loading metadata never requires a successful model client or a network request.

use anyhow::Context;
use anyhow::Result;
use clap::Parser;
use serde_json::json;
use std::io::Read;
use std::path::Path;
use xdcode_config::CloudConfigBundleLoader;
use xdcode_config::ConfigLoadOptions;
use xdcode_config::LoaderOverrides;
use xdcode_config::model_catalog::CatalogSnapshot;
use xdcode_config::model_catalog::CatalogUpsert;
use xdcode_config::model_catalog::probe_model;
use xdcode_config::model_catalog::refresh_reasoning_layered;
use xdcode_config::model_catalog::static_check;
use xdcode_config::model_catalog::upsert_catalog_layered;
use xdcode_core::config::find_xdcode_home;
use xdcode_core::config::load_config_toml_with_layer_stack;
use xdcode_utils_absolute_path::AbsolutePathBuf;
use xdcode_utils_cli::CliConfigOverrides;

#[derive(Debug, Parser)]
pub(crate) struct ModelsCommand {
    #[command(subcommand)]
    command: ModelCommand,
}

#[derive(Debug, clap::Subcommand)]
enum ModelCommand {
    /// List the effective catalog and its configuration sources, without secrets.
    List {
        #[arg(long)]
        json: bool,
    },
    /// Merge provider and model definitions from a JSON object on standard input.
    Upsert {
        /// Read and return JSON. Credentials must use existing environment references.
        #[arg(long, required = true)]
        json: bool,
        /// Validate and describe the change without writing configuration.
        #[arg(long)]
        preview: bool,
        /// Refuse to write if the target file changed since `list`.
        #[arg(long)]
        expected_version: Option<String>,
    },
    /// Check a catalog entry. Network access requires the explicit --probe flag.
    Check {
        id: String,
        #[arg(long)]
        probe: bool,
        #[arg(long)]
        json: bool,
    },
    /// Discover reasoning levels and save confirmed results to the defining file.
    Refresh {
        id: String,
        /// Explicitly replace a user-maintained reasoning list.
        #[arg(long)]
        overwrite_manual: bool,
        #[arg(long)]
        json: bool,
    },
}

impl ModelsCommand {
    pub(crate) async fn run(
        self,
        overrides: CliConfigOverrides,
        loader_overrides: LoaderOverrides,
        cwd: Option<&Path>,
        strict_config: bool,
    ) -> Result<()> {
        let home = find_xdcode_home()?;
        let target = loader_overrides.user_config_path(&home)?;
        let cwd = match cwd {
            Some(cwd) => AbsolutePathBuf::relative_to_current_dir(cwd)?,
            None => AbsolutePathBuf::current_dir()?,
        };
        let loaded = load_config_toml_with_layer_stack(
            &home,
            Some(&cwd),
            overrides.parse_overrides().map_err(anyhow::Error::msg)?,
            ConfigLoadOptions {
                loader_overrides,
                strict_config,
                cloud_config_bundle: CloudConfigBundleLoader::default(),
            },
        )
        .await?;
        let snapshot = CatalogSnapshot::load_layered(&loaded.config_layer_stack)?;
        let origins = loaded.config_layer_stack.origins();
        match self.command {
            ModelCommand::List { json: as_json } => {
                let mut output = snapshot.redacted_json();
                output["config_file"] = json!(target);
                output["sources"] = json!(loaded.config_layer_stack.layers_low_to_high()
                    .filter(|layer| layer.disabled_reason.is_none())
                    .map(|layer| json!({
                        "source": xdcode_config::format_config_layer_source(&layer.name, "config.toml"),
                        "version": layer.version,
                    })).collect::<Vec<_>>());
                // Record the defining layer, which may differ from the writable profile.
                output["models_source"] = json!(origins.get("models").map(|origin| {
                    xdcode_config::format_config_layer_source(&origin.name, "config.toml")
                }));
                output["current"] = json!({
                    "model": loaded.config_toml.model,
                    "provider": loaded.config_toml.model_provider,
                    "reasoning_effort": loaded.config_toml.model_reasoning_effort,
                });
                if as_json {
                    print_json(&output)?;
                } else {
                    println!("Configuration: {}", target.as_path().display());
                    println!("Version: {}", snapshot.version);
                    for model in &snapshot.models {
                        println!(
                            "{}\t{}\t{}\t{}",
                            model.id, model.provider, model.model, model.name
                        );
                    }
                    if snapshot.models.is_empty() {
                        println!(
                            "No model catalog; the configured model and existing picker remain available."
                        );
                    }
                }
            }
            ModelCommand::Upsert {
                json: _,
                preview,
                expected_version,
            } => {
                let update = read_update(std::io::stdin().lock())?;
                let result = upsert_catalog_layered(
                    &loaded.config_layer_stack,
                    &update,
                    expected_version.as_deref(),
                    preview,
                )?;
                print_json(&serde_json::to_value(result)?)?;
            }
            ModelCommand::Check {
                id,
                probe,
                json: as_json,
            } => {
                let result = if probe {
                    probe_model(&snapshot, &id).await?
                } else {
                    static_check(&snapshot, &id)?
                };
                let result = serde_json::to_value(result)?;
                if as_json {
                    print_json(&result)?;
                } else {
                    println!("{}", serde_json::to_string_pretty(&result)?);
                }
            }
            ModelCommand::Refresh {
                id,
                overwrite_manual,
                json: as_json,
            } => {
                // Refresh must write back to the model's actual source, not create
                // an accidental profile override or modify a session-only value.
                let result =
                    refresh_reasoning_layered(&loaded.config_layer_stack, &id, overwrite_manual)
                        .await?;
                let result = serde_json::to_value(result)?;
                if as_json {
                    print_json(&result)?;
                } else {
                    println!("{}", serde_json::to_string_pretty(&result)?);
                }
            }
        }
        Ok(())
    }
}

fn read_update(reader: impl Read) -> Result<CatalogUpsert> {
    const MAX_UPDATE_BYTES: u64 = 1024 * 1024;
    let mut input = String::new();
    reader
        .take(MAX_UPDATE_BYTES + 1)
        .read_to_string(&mut input)?;
    anyhow::ensure!(
        input.len() as u64 <= MAX_UPDATE_BYTES,
        "model update exceeds 1 MiB"
    );
    serde_json::from_str(&input).context("expected a model catalog JSON update on stdin")
}

fn print_json(value: &serde_json::Value) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_input_preserves_exact_model_ids_and_custom_efforts() {
        let update = read_update(br#"{"models":[{"id":"free","name":"Free","provider":"router","model":"minimax/minimax-m3.1-flash:free","reasoning_efforts":["custom-level"]}]}"#.as_slice()).unwrap();
        assert_eq!(update.models[0].model, "minimax/minimax-m3.1-flash:free");
        assert_eq!(
            update.models[0].reasoning_efforts.as_ref().unwrap()[0].to_string(),
            "custom-level"
        );
    }

    #[test]
    fn model_update_input_is_bounded() {
        let oversized = vec![b' '; 1024 * 1024 + 1];
        assert!(
            read_update(oversized.as_slice())
                .unwrap_err()
                .to_string()
                .contains("1 MiB")
        );
    }

    #[test]
    fn model_cli_network_access_is_explicit() {
        let args = ModelsCommand::try_parse_from(["models", "check", "sample"]).unwrap();
        assert!(matches!(
            args.command,
            ModelCommand::Check { probe: false, .. }
        ));
        let args = ModelsCommand::try_parse_from(["models", "refresh", "sample"]).unwrap();
        assert!(matches!(
            args.command,
            ModelCommand::Refresh {
                overwrite_manual: false,
                ..
            }
        ));
        assert!(ModelsCommand::try_parse_from(["models", "upsert"]).is_err());
    }
}
