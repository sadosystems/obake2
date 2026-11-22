use std::sync::Arc;

use collections::HashMap;
use settings::RegisterSetting;

use crate::provider::{
    anthropic, anthropic::AnthropicSettings, anthropic_compatible::AnthropicCompatibleSettings,
    google::GoogleSettings, ollama::OllamaSettings, resolve_custom_headers,
};

#[derive(Debug, RegisterSetting)]
pub struct AllLanguageModelSettings {
    pub anthropic: AnthropicSettings,
    pub anthropic_compatible: HashMap<Arc<str>, AnthropicCompatibleSettings>,
    pub google: GoogleSettings,
    pub ollama: OllamaSettings,
}

fn custom_headers_from(
    provider_name: &str,
    raw: Option<HashMap<String, String>>,
    reserved: &[&str],
) -> http_client::CustomHeaders {
    raw.as_ref()
        .filter(|map| !map.is_empty())
        .map(|map| resolve_custom_headers(provider_name, map, reserved))
        .unwrap_or_default()
}

impl settings::Settings for AllLanguageModelSettings {
    const PRESERVED_KEYS: Option<&'static [&'static str]> = Some(&["version"]);

    fn from_settings(content: &settings::SettingsContent) -> Self {
        let language_models = content.language_models.clone().unwrap();
        let anthropic = language_models.anthropic.unwrap();
        let anthropic_compatible = language_models.anthropic_compatible.unwrap();
        let google = language_models.google.unwrap();
        let ollama = language_models.ollama.unwrap();
        Self {
            anthropic: AnthropicSettings {
                api_url: anthropic.api_url.unwrap(),
                available_models: anthropic.available_models.unwrap_or_default(),
                custom_headers: custom_headers_from(
                    "Anthropic",
                    anthropic.custom_headers,
                    anthropic::RESERVED_HEADER_NAMES,
                ),
            },
            anthropic_compatible: anthropic_compatible
                .into_iter()
                .map(|(key, value)| {
                    let provider_label = format!("Anthropic Compatible ({key})");
                    (
                        key,
                        AnthropicCompatibleSettings {
                            api_url: value.api_url,
                            available_models: value.available_models,
                            custom_headers: custom_headers_from(
                                &provider_label,
                                value.custom_headers,
                                anthropic::RESERVED_HEADER_NAMES,
                            ),
                        },
                    )
                })
                .collect(),
            google: GoogleSettings {
                api_url: google.api_url.unwrap(),
                available_models: google.available_models.unwrap_or_default(),
                custom_headers: custom_headers_from("Google AI", google.custom_headers, &[]),
            },
            ollama: OllamaSettings {
                api_url: ollama.api_url.unwrap(),
                auto_discover: ollama.auto_discover.unwrap_or(true),
                available_models: ollama.available_models.unwrap_or_default(),
                context_window: ollama.context_window,
                custom_headers: custom_headers_from("Ollama", ollama.custom_headers, &[]),
            },
        }
    }
}
