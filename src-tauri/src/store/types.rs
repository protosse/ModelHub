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

/// Saved prompt snippets for model connection tests (store.json).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TestPrompt {
    pub id: String,
    pub name: String,
    pub content: String,
    #[serde(default)]
    pub is_default: bool,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TestPromptInput {
    pub id: Option<String>,
    pub name: String,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TestConnectionRequest {
    /// Library model row id (`Model.id`).
    pub model_id: String,
    pub prompt: String,
    /// Client-generated id to correlate streaming log events.
    #[serde(default)]
    pub run_id: Option<String>,
    /// Request timeout in seconds (clamped server-side). Default 30.
    #[serde(default)]
    pub timeout_secs: Option<u64>,
    /// Optional per-run extra HTTP headers (merged after provider.headers; same key overwrites).
    #[serde(default)]
    pub extra_headers: Option<std::collections::HashMap<String, String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TestConnectionResult {
    pub ok: bool,
    pub latency_ms: u64,
    pub http_status: Option<u16>,
    pub protocol: Protocol,
    pub request_url: String,
    /// Assistant text when parse succeeds; otherwise may mirror raw body snippet.
    pub response_text: Option<String>,
    pub error: Option<String>,
    /// Human-readable timeline for diagnosing failures (no raw secrets).
    #[serde(default)]
    pub logs: Vec<String>,
    #[serde(default)]
    pub request_method: String,
    /// Authorization / x-api-key values are redacted.
    #[serde(default)]
    pub request_headers: Vec<String>,
    #[serde(default)]
    pub request_body: Option<String>,
    #[serde(default)]
    pub response_headers: Vec<String>,
    /// Raw response body (truncated) for failure analysis.
    #[serde(default)]
    pub response_body: Option<String>,
}

/// Last connectivity test outcome for a library model row (persisted in store.json).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelTestResult {
    pub ok: bool,
    /// ISO-8601 timestamp when the test finished.
    pub tested_at: String,
    #[serde(default)]
    pub latency_ms: Option<u64>,
}

pub fn seed_test_prompts() -> Vec<TestPrompt> {
    vec![TestPrompt {
        id: "prompt_default_connectivity".into(),
        name: "连通性探测".into(),
        content: "将123@qq.com转为Base64，直接回复结果".into(),
        is_default: true,
        created_at: "1970-01-01T00:00:00Z".into(),
        updated_at: "1970-01-01T00:00:00Z".into(),
    }]
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
    /// Connection-test prompts (reusable).
    #[serde(default = "seed_test_prompts")]
    pub test_prompts: Vec<TestPrompt>,
    /// Last connectivity test per model row id (`Model.id`).
    #[serde(default)]
    pub model_test_results: std::collections::HashMap<String, ModelTestResult>,
}

impl Default for Store {
    fn default() -> Self {
        Self {
            version: STORE_VERSION,
            providers: Vec::new(),
            models: Vec::new(),
            agent_bindings: AgentBindings::default(),
            test_prompts: seed_test_prompts(),
            model_test_results: std::collections::HashMap::new(),
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
    #[serde(default)]
    pub model_ids: Vec<String>,
    #[serde(default)]
    pub extra_model_count: usize,
    #[serde(default)]
    pub new_model_ids: Vec<String>,
    #[serde(default)]
    pub existing_model_ids: Vec<String>,
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
    #[serde(default)]
    pub scan_notes: Vec<String>,
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
