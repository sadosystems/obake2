//! obake: stub of Zed's WASM extension host. The real one (wasmtime + cranelift)
//! dominated cold builds and obake doesn't run Zed extensions. Loading a WASM
//! extension now fails; everything else in the extension store keeps working.
//! The original lives in git history (vendored Zed f4fe0be1, crates/extension_host).
#![allow(unused)]

use crate::ExtensionManifest;
use anyhow::{Result, bail};
use async_trait::async_trait;
use dap::{DebugRequest, StartDebuggingRequestArgumentsRequest};
use extension::{
    CodeLabel, Command, Completion, ContextServerConfiguration, DebugAdapterBinary,
    DebugTaskDefinition, ExtensionHostProxy, KeyValueStoreDelegate, ProjectDelegate,
    SlashCommand, SlashCommandArgumentCompletion, SlashCommandOutput, Symbol, WorktreeDelegate,
};
use fs::Fs;
use gpui::{App, AsyncApp, EntityId};
use http_client::HttpClient;
use language::LanguageName;
use lsp::LanguageServerName;
use node_runtime::NodeRuntime;
use semver::Version;
use std::{
    convert::Infallible,
    path::{Path, PathBuf},
    sync::Arc,
};
use task::{DebugScenario, SpawnInTerminal, TaskTemplate, ZedDebugConfig};

pub mod wit {
    use release_channel::ReleaseChannel;
    use semver::Version;
    use std::ops::RangeInclusive;

    pub fn is_supported_wasm_api_version(release_channel: ReleaseChannel, version: Version) -> bool {
        wasm_api_version_range(release_channel).contains(&version)
    }

    pub fn wasm_api_version_range(_release_channel: ReleaseChannel) -> RangeInclusive<Version> {
        Version::new(0, 0, 1)..=Version::new(0, 8, 0)
    }
}

pub struct WasmHost {
    pub work_dir: PathBuf,
}

impl WasmHost {
    pub fn new(
        _fs: Arc<dyn Fs>,
        _http_client: Arc<dyn HttpClient>,
        _node_runtime: NodeRuntime,
        _proxy: Arc<ExtensionHostProxy>,
        work_dir: PathBuf,
        _cx: &mut App,
    ) -> Arc<Self> {
        Arc::new(Self { work_dir })
    }
}

#[derive(Clone, Debug)]
pub struct WasmExtension {
    never: Infallible,
}

impl WasmExtension {
    pub async fn load(
        extension_dir: &Path,
        _manifest: &Arc<ExtensionManifest>,
        _wasm_host: Arc<WasmHost>,
        _cx: &AsyncApp,
    ) -> Result<Self> {
        bail!("WASM extensions are not supported in obake ({extension_dir:?})")
    }
}

#[async_trait]
impl extension::Extension for WasmExtension {
    fn manifest(&self) -> Arc<ExtensionManifest> {
        match self.never {}
    }

    fn work_dir(&self) -> Arc<Path> {
        match self.never {}
    }

    async fn language_server_command(
        &self,
        language_server_id: LanguageServerName,
        language_name: LanguageName,
        worktree: Arc<dyn WorktreeDelegate>,
        status_source: EntityId,
    ) -> Result<Command> {
        match self.never {}
    }

    async fn language_server_initialization_options(
        &self,
        language_server_id: LanguageServerName,
        language_name: LanguageName,
        worktree: Arc<dyn WorktreeDelegate>,
        status_source: EntityId,
    ) -> Result<Option<String>> {
        match self.never {}
    }

    async fn language_server_workspace_configuration(
        &self,
        language_server_id: LanguageServerName,
        worktree: Arc<dyn WorktreeDelegate>,
        status_source: EntityId,
    ) -> Result<Option<String>> {
        match self.never {}
    }

    async fn language_server_initialization_options_schema(
        &self,
        language_server_id: LanguageServerName,
        worktree: Arc<dyn WorktreeDelegate>,
        status_source: EntityId,
    ) -> Result<Option<String>> {
        match self.never {}
    }

    async fn language_server_workspace_configuration_schema(
        &self,
        language_server_id: LanguageServerName,
        worktree: Arc<dyn WorktreeDelegate>,
        status_source: EntityId,
    ) -> Result<Option<String>> {
        match self.never {}
    }

    async fn language_server_additional_initialization_options(
        &self,
        language_server_id: LanguageServerName,
        target_language_server_id: LanguageServerName,
        worktree: Arc<dyn WorktreeDelegate>,
        status_source: EntityId,
    ) -> Result<Option<String>> {
        match self.never {}
    }

    async fn language_server_additional_workspace_configuration(
        &self,
        language_server_id: LanguageServerName,
        target_language_server_id: LanguageServerName,
        worktree: Arc<dyn WorktreeDelegate>,
        status_source: EntityId,
    ) -> Result<Option<String>> {
        match self.never {}
    }

    async fn labels_for_completions(
        &self,
        language_server_id: LanguageServerName,
        completions: Vec<Completion>,
    ) -> Result<Vec<Option<CodeLabel>>> {
        match self.never {}
    }

    async fn labels_for_symbols(
        &self,
        language_server_id: LanguageServerName,
        symbols: Vec<Symbol>,
    ) -> Result<Vec<Option<CodeLabel>>> {
        match self.never {}
    }

    async fn complete_slash_command_argument(
        &self,
        command: SlashCommand,
        arguments: Vec<String>,
    ) -> Result<Vec<SlashCommandArgumentCompletion>> {
        match self.never {}
    }

    async fn run_slash_command(
        &self,
        command: SlashCommand,
        arguments: Vec<String>,
        delegate: Option<Arc<dyn WorktreeDelegate>>,
    ) -> Result<SlashCommandOutput> {
        match self.never {}
    }

    async fn context_server_command(
        &self,
        context_server_id: Arc<str>,
        project: Arc<dyn ProjectDelegate>,
    ) -> Result<Command> {
        match self.never {}
    }

    async fn context_server_configuration(
        &self,
        context_server_id: Arc<str>,
        project: Arc<dyn ProjectDelegate>,
    ) -> Result<Option<ContextServerConfiguration>> {
        match self.never {}
    }

    async fn suggest_docs_packages(&self, provider: Arc<str>) -> Result<Vec<String>> {
        match self.never {}
    }

    async fn index_docs(
        &self,
        provider: Arc<str>,
        package_name: Arc<str>,
        kv_store: Arc<dyn KeyValueStoreDelegate>,
    ) -> Result<()> {
        match self.never {}
    }

    async fn get_dap_binary(
        &self,
        dap_name: Arc<str>,
        config: DebugTaskDefinition,
        user_installed_path: Option<PathBuf>,
        worktree: Arc<dyn WorktreeDelegate>,
    ) -> Result<DebugAdapterBinary> {
        match self.never {}
    }
    async fn dap_request_kind(
        &self,
        dap_name: Arc<str>,
        config: serde_json::Value,
    ) -> Result<StartDebuggingRequestArgumentsRequest> {
        match self.never {}
    }

    async fn dap_config_to_scenario(&self, config: ZedDebugConfig) -> Result<DebugScenario> {
        match self.never {}
    }

    async fn dap_locator_create_scenario(
        &self,
        locator_name: String,
        build_config_template: TaskTemplate,
        resolved_label: String,
        debug_adapter_name: String,
    ) -> Result<Option<DebugScenario>> {
        match self.never {}
    }
    async fn run_dap_locator(
        &self,
        locator_name: String,
        config: SpawnInTerminal,
    ) -> Result<DebugRequest> {
        match self.never {}
    }
}
