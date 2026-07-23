use serde::{Deserialize, Serialize};
use std::collections::HashMap;

pub const STORE_VERSION: u32 = 1;
pub const SECRETS_VERSION: u32 = 1;
pub const CONFIG_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum Protocol {
    AnthropicMessages,
    OpenaiCompletions,
    OpenaiResponses,
}

impl Protocol {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::AnthropicMessages => "anthropic-messages",
            Self::OpenaiCompletions => "openai-completions",
            Self::OpenaiResponses => "openai-responses",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AgentMode {
    ThirdParty,
    Official,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppConfig {
    pub version: u32,
    pub language: String,
    pub backup_keep_count: u32,
    pub paths: PathOverrides,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            version: CONFIG_VERSION,
            language: "zh-CN".into(),
            backup_keep_count: 10,
            paths: PathOverrides::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct PathOverrides {
    pub claude_settings: Option<String>,
    pub codex_config: Option<String>,
    pub opencode_config: Option<String>,
    pub opencode_auth: Option<String>,
    pub pi_models: Option<String>,
    pub pi_settings: Option<String>,
    pub pi_auth: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Provider {
    pub id: String,
    pub name: String,
    pub base_url: String,
    pub protocol: Protocol,
    #[serde(default)]
    pub headers: HashMap<String, String>,
    #[serde(default)]
    pub compat: HashMap<String, serde_json::Value>,
    pub enabled: bool,
    #[serde(default)]
    pub notes: String,
    pub secret_ref: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelCapabilities {
    #[serde(default)]
    pub reasoning: bool,
    #[serde(default)]
    pub vision: bool,
}

impl Default for ModelCapabilities {
    fn default() -> Self {
        Self {
            reasoning: false,
            vision: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Model {
    pub id: String,
    pub provider_id: String,
    pub model_id: String,
    pub display_name: String,
    pub enabled: bool,
    #[serde(default)]
    pub capabilities: ModelCapabilities,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClaudeBinding {
    pub mode: AgentMode,
    pub provider_id: Option<String>,
    pub model_id: Option<String>,
    pub haiku_model_id: Option<String>,
    pub sonnet_model_id: Option<String>,
    pub opus_model_id: Option<String>,
}

impl Default for ClaudeBinding {
    fn default() -> Self {
        Self {
            mode: AgentMode::Official,
            provider_id: None,
            model_id: None,
            haiku_model_id: None,
            sonnet_model_id: None,
            opus_model_id: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexBinding {
    pub mode: AgentMode,
    pub provider_id: Option<String>,
    pub model_id: Option<String>,
    #[serde(default = "default_codex_provider_key")]
    pub provider_key: String,
}

fn default_codex_provider_key() -> String {
    "modelhub".into()
}

impl Default for CodexBinding {
    fn default() -> Self {
        Self {
            mode: AgentMode::Official,
            provider_id: None,
            model_id: None,
            provider_key: default_codex_provider_key(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct OpencodeBinding {
    pub provider_id: Option<String>,
    pub model_id: Option<String>,
    pub small_model_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct PiBinding {
    pub provider_id: Option<String>,
    pub model_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct AgentBindings {
    #[serde(default)]
    pub claude: ClaudeBinding,
    #[serde(default)]
    pub codex: CodexBinding,
    #[serde(default)]
    pub opencode: OpencodeBinding,
    #[serde(default)]
    pub pi: PiBinding,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Store {
    pub version: u32,
    pub providers: Vec<Provider>,
    pub models: Vec<Model>,
    pub agent_bindings: AgentBindings,
}

impl Default for Store {
    fn default() -> Self {
        Self {
            version: STORE_VERSION,
            providers: Vec::new(),
            models: Vec::new(),
            agent_bindings: AgentBindings::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SecretEntry {
    pub api_key: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Secrets {
    pub version: u32,
    pub secrets: HashMap<String, SecretEntry>,
}

impl Default for Secrets {
    fn default() -> Self {
        Self {
            version: SECRETS_VERSION,
            secrets: HashMap::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderInput {
    pub name: String,
    pub base_url: String,
    pub protocol: Protocol,
    pub api_key: String,
    #[serde(default)]
    pub headers: HashMap<String, String>,
    #[serde(default)]
    pub compat: HashMap<String, serde_json::Value>,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub notes: String,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelInput {
    pub provider_id: String,
    pub model_id: String,
    pub display_name: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub capabilities: ModelCapabilities,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplyRequest {
    #[serde(default)]
    pub agents: Vec<String>,
    #[serde(default)]
    pub bindings: Option<AgentBindings>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplyAgentResult {
    pub agent: String,
    pub ok: bool,
    pub message: String,
    pub files: Vec<String>,
    pub restart_required: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplyResult {
    pub results: Vec<ApplyAgentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportPreviewItem {
    pub id: String,
    pub source: String,
    pub name: String,
    pub base_url: String,
    pub protocol: Protocol,
    pub model_count: usize,
    pub already_exists: bool,
    pub existing_provider_id: Option<String>,
    pub existing_name: Option<String>,
    pub name_conflict: bool,
    pub has_api_key: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportPreview {
    pub items: Vec<ImportPreviewItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ImportAction {
    Import,
    Override,
    Skip,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportItemDecision {
    pub id: String,
    pub name: String,
    pub action: ImportAction,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportRequest {
    pub items: Vec<ImportItemDecision>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportResult {
    pub imported_providers: usize,
    pub imported_models: usize,
    pub skipped: usize,
    pub overridden: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteModel {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DetectedPaths {
    pub modelhub_dir: String,
    pub claude_settings: String,
    pub claude_exists: bool,
    pub codex_config: String,
    pub codex_exists: bool,
    pub opencode_config: String,
    pub opencode_exists: bool,
    pub pi_models: String,
    pub pi_exists: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FullState {
    pub config: AppConfig,
    pub store: Store,
    /// secretRef -> masked key (last 4)
    pub secret_masks: HashMap<String, String>,
    pub paths: DetectedPaths,
}
