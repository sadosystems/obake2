use std::sync::Arc;

use ::settings::{Settings, SettingsStore};
use client::{Client, UserStore};
use collections::{HashMap, HashSet};
use credentials_provider::CredentialsProvider;
use gpui::{App, Context, Entity};
use language_model::{LanguageModelProviderId, LanguageModelRegistry};

pub mod extension;
pub mod provider;
mod settings;

pub use crate::extension::init_proxy as init_extension_proxy;

use crate::provider::anthropic::AnthropicLanguageModelProvider;
use crate::provider::anthropic_compatible::AnthropicCompatibleLanguageModelProvider;
use crate::provider::google::GoogleLanguageModelProvider;
use crate::provider::ollama::OllamaLanguageModelProvider;
pub use crate::settings::*;

pub fn init(user_store: Entity<UserStore>, client: Arc<Client>, cx: &mut App) {
    language_model::set_credentials_read_gate::<SettingsStore>(ai_disabled, cx);
    let credentials_provider = client.credentials_provider();
    let registry = LanguageModelRegistry::global(cx);
    registry.update(cx, |registry, cx| {
        register_language_model_providers(
            registry,
            user_store,
            client.clone(),
            credentials_provider.clone(),
            cx,
        );
    });


    let mut compatible_providers = CompatibleProviders::from_settings(cx);

    registry.update(cx, |registry, cx| {
        register_compatible_providers(
            registry,
            &CompatibleProviders::default(),
            &compatible_providers,
            &client,
            &credentials_provider,
            cx,
        );
    });

    let registry = registry.downgrade();
    cx.observe_global::<SettingsStore>(move |cx| {
        let Some(registry) = registry.upgrade() else {
            return;
        };
        let compatible_providers_new = CompatibleProviders::from_settings(cx);
        if compatible_providers_new != compatible_providers {
            registry.update(cx, |registry, cx| {
                register_compatible_providers(
                    registry,
                    &compatible_providers,
                    &compatible_providers_new,
                    &client,
                    &credentials_provider,
                    cx,
                );
            });
            compatible_providers = compatible_providers_new;
        }
    })
    .detach();
}

fn ai_disabled(cx: &App) -> bool {
    cx.try_global::<SettingsStore>().is_some_and(|store| {
        store
            .merged_settings()
            .project
            .disable_ai
            .is_some_and(|disabled| disabled.0)
    })
}

#[derive(Default, PartialEq, Eq)]
struct CompatibleProviders(HashMap<Arc<str>, CompatibleProviderKind>);

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum CompatibleProviderKind {
    Anthropic,
}

impl CompatibleProviders {
    fn from_settings(cx: &App) -> Self {
        let settings = AllLanguageModelSettings::get_global(cx);
        // obake: OpenAI-compatible providers were removed.
        let mut providers: HashMap<Arc<str>, CompatibleProviderKind> = HashMap::default();
        for id in settings.anthropic_compatible.keys() {
            // The registry has a single provider ID namespace, so a name can
            // only refer to one provider. OpenAI-compatible entries win
            // collisions because they predate Anthropic-compatible ones, so
            // existing configurations keep working.
            if providers.contains_key(id) {
                log::warn!(
                    "ignoring `anthropic_compatible` provider `{id}`: \
                     an `openai_compatible` provider with the same name exists"
                );
            } else {
                providers.insert(id.clone(), CompatibleProviderKind::Anthropic);
            }
        }
        Self(providers)
    }
}

fn register_compatible_providers(
    registry: &mut LanguageModelRegistry,
    old: &CompatibleProviders,
    new: &CompatibleProviders,
    client: &Arc<Client>,
    credentials_provider: &Arc<dyn CredentialsProvider>,
    cx: &mut Context<LanguageModelRegistry>,
) {
    for (provider_id, old_kind) in &old.0 {
        if new.0.get(provider_id) != Some(old_kind) {
            registry.unregister_provider(LanguageModelProviderId::from(provider_id.clone()), cx);
        }
    }

    for (provider_id, kind) in &new.0 {
        if old.0.get(provider_id) != Some(kind) {
            match kind {
                CompatibleProviderKind::Anthropic => registry.register_provider(
                    Arc::new(AnthropicCompatibleLanguageModelProvider::new(
                        provider_id.clone(),
                        client.http_client(),
                        credentials_provider.clone(),
                        cx,
                    )),
                    cx,
                ),
            }
        }
    }
}

fn register_language_model_providers(
    registry: &mut LanguageModelRegistry,
    user_store: Entity<UserStore>,
    client: Arc<Client>,
    credentials_provider: Arc<dyn CredentialsProvider>,
    cx: &mut Context<LanguageModelRegistry>,
) {
    registry.register_provider(
        Arc::new(AnthropicLanguageModelProvider::new(
            client.http_client(),
            credentials_provider.clone(),
            cx,
        )),
        cx,
    );
    registry.register_provider(
        Arc::new(OllamaLanguageModelProvider::new(
            client.http_client(),
            credentials_provider.clone(),
            cx,
        )),
        cx,
    );
    registry.register_provider(
        Arc::new(GoogleLanguageModelProvider::new(
            client.http_client(),
            credentials_provider.clone(),
            cx,
        )),
        cx,
    );
}

