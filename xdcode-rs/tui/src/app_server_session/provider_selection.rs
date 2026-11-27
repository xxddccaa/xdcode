//! Provider request overrides honor managed requirements over explicit invocation choices.
//! Omission lets the server resolve the provider for thread creation and forks.
//! History visibility is independent of the selected profile and inference provider.

use super::AppServerSession;
use super::ThreadParamsMode;
use crate::legacy_core::config::Config;
use xdcode_config::ConfigLayerSource;

pub(crate) fn explicit_provider(config: &Config) -> Option<String> {
    config
        .config_layer_stack
        .layers_high_to_low()
        .find(|layer| layer.config.get("model_provider").is_some())
        .filter(|layer| {
            matches!(
                layer.name,
                ConfigLayerSource::SessionFlags
                    | ConfigLayerSource::User {
                        profile: Some(_),
                        ..
                    }
            )
        })
        .map(|_| config.model_provider_id.clone())
}

impl AppServerSession {
    /// A remote client forwards its local named profile. Embedded sessions reload that same
    /// profile from disk before /new, so only invocation flags can pin its provider.
    pub(crate) fn client_provider_override_for_new_chats(&self, config: &Config) -> Option<String> {
        match self.thread_params_mode() {
            ThreadParamsMode::Cloud => None,
            ThreadParamsMode::Remote => self.explicit_model_provider(config),
            ThreadParamsMode::Embedded => {
                let launch_flag = self.model_provider_override.is_some()
                    || config.config_layer_stack.layers_high_to_low().any(|layer| {
                        layer.name == ConfigLayerSource::SessionFlags
                            && layer.config.get("model_provider").is_some()
                    });
                launch_flag
                    .then(|| self.explicit_model_provider(config))
                    .flatten()
            }
        }
    }

    pub(crate) fn explicit_model_provider(&self, config: &Config) -> Option<String> {
        self.model_provider_override
            .as_ref()
            .map(|provider| {
                config
                    .config_layer_stack
                    .required_model_provider()
                    .unwrap_or(provider)
                    .to_owned()
            })
            .or_else(|| explicit_provider(config))
    }

    /// A launch profile chooses inference routing, never the history collection.
    /// List callers translate None into an explicit empty provider filter: omitting
    /// the wire field instead would select the server's current default provider.
    pub(crate) async fn history_model_provider(
        &self,
        _config: &Config,
    ) -> color_eyre::Result<Option<String>> {
        Ok(None)
    }
}
