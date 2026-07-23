mod types;

pub use types::*;

use anyhow::{Context, Result};
use chrono::Utc;
use fs_err as fs;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use uuid::Uuid;

use crate::paths::ModelHubPaths;

pub struct StoreService {
    paths: ModelHubPaths,
}

impl StoreService {
    pub fn new(paths: ModelHubPaths) -> Self {
        Self { paths }
    }

    pub fn ensure_dirs(&self) -> Result<()> {
        fs::create_dir_all(&self.paths.root)?;
        fs::create_dir_all(self.paths.root.join("backups"))?;
        Ok(())
    }

    pub fn load_config(&self) -> Result<AppConfig> {
        self.ensure_dirs()?;
        read_json_or_default(&self.paths.config_file())
    }

    pub fn save_config(&self, config: &AppConfig) -> Result<()> {
        self.ensure_dirs()?;
        write_json_atomic(&self.paths.config_file(), config)
    }

    pub fn load_store(&self) -> Result<Store> {
        self.ensure_dirs()?;
        let path = self.paths.store_file();
        let mut store: Store = read_json_or_default(&path)?;
        let before = store.test_prompts.len();
        let had_default = store.test_prompts.iter().any(|p| p.is_default);
        ensure_default_test_prompt(&mut store);
        // Persist seed for older store.json files that lack prompts.
        if path.exists()
            && (store.test_prompts.len() != before
                || (!had_default && store.test_prompts.iter().any(|p| p.is_default)))
        {
            let _ = self.save_store(&store);
        }
        Ok(store)
    }

    pub fn save_store(&self, store: &Store) -> Result<()> {
        self.ensure_dirs()?;
        write_json_atomic(&self.paths.store_file(), store)
    }

    pub fn load_secrets(&self) -> Result<Secrets> {
        self.ensure_dirs()?;
        let path = self.paths.secrets_file();
        let secrets: Secrets = read_json_or_default(&path)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if path.exists() {
                let mut perms = fs::metadata(&path)?.permissions();
                perms.set_mode(0o600);
                fs::set_permissions(&path, perms)?;
            }
        }
        Ok(secrets)
    }

    pub fn save_secrets(&self, secrets: &Secrets) -> Result<()> {
        self.ensure_dirs()?;
        let path = self.paths.secrets_file();
        write_json_atomic(&path, secrets)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = fs::metadata(&path)?.permissions();
            perms.set_mode(0o600);
            fs::set_permissions(&path, perms)?;
        }
        Ok(())
    }

    pub fn full_state(&self) -> Result<FullState> {
        let config = self.load_config()?;
        let store = self.load_store()?;
        let secrets = self.load_secrets()?;
        let secret_masks = secrets
            .secrets
            .iter()
            .map(|(k, v)| (k.clone(), mask_key(&v.api_key)))
            .collect();
        let paths = self.paths.detect(&config.paths)?;
        Ok(FullState {
            config,
            store,
            secret_masks,
            paths,
        })
    }

    pub fn add_provider(&self, input: ProviderInput) -> Result<Provider> {
        let mut store = self.load_store()?;
        let mut secrets = self.load_secrets()?;
        let name = input.name.trim().to_string();
        if name.is_empty() {
            anyhow::bail!("提供商名称不能为空");
        }
        if store
            .providers
            .iter()
            .any(|p| p.name.eq_ignore_ascii_case(&name))
        {
            anyhow::bail!("提供商名称已存在：{name}");
        }
        let now = now_iso();
        let secret_ref = format!("sec_{}", Uuid::new_v4());
        let provider = Provider {
            id: format!("prov_{}", Uuid::new_v4()),
            name,
            base_url: normalize_base_url(&input.base_url),
            protocol: input.protocol,
            headers: input.headers,
            compat: input.compat,
            enabled: input.enabled,
            notes: input.notes,
            secret_ref: secret_ref.clone(),
            created_at: now.clone(),
            updated_at: now.clone(),
        };
        secrets.secrets.insert(
            secret_ref,
            SecretEntry {
                api_key: input.api_key,
                updated_at: now,
            },
        );
        store.providers.push(provider.clone());
        self.save_secrets(&secrets)?;
        self.save_store(&store)?;
        Ok(provider)
    }

    pub fn update_provider(&self, id: &str, input: ProviderInput) -> Result<Provider> {
        let mut store = self.load_store()?;
        let mut secrets = self.load_secrets()?;
        let name = input.name.trim().to_string();
        if name.is_empty() {
            anyhow::bail!("提供商名称不能为空");
        }
        if store
            .providers
            .iter()
            .any(|p| p.id != id && p.name.eq_ignore_ascii_case(&name))
        {
            anyhow::bail!("提供商名称已存在：{name}");
        }
        let provider = store
            .providers
            .iter_mut()
            .find(|p| p.id == id)
            .context("provider not found")?;
        let now = now_iso();
        provider.name = name;
        provider.base_url = normalize_base_url(&input.base_url);
        provider.protocol = input.protocol;
        provider.headers = input.headers;
        provider.compat = input.compat;
        provider.enabled = input.enabled;
        provider.notes = input.notes;
        provider.updated_at = now.clone();
        let secret_ref = provider.secret_ref.clone();
        // Allow clearing? no. Empty means keep. Non-empty replaces.
        if !input.api_key.is_empty() {
            secrets.secrets.insert(
                secret_ref,
                SecretEntry {
                    api_key: input.api_key,
                    updated_at: now,
                },
            );
            self.save_secrets(&secrets)?;
        }
        let out = provider.clone();
        self.save_store(&store)?;
        Ok(out)
    }

    pub fn delete_provider(&self, id: &str) -> Result<()> {
        let mut store = self.load_store()?;
        let mut secrets = self.load_secrets()?;
        let Some(idx) = store.providers.iter().position(|p| p.id == id) else {
            anyhow::bail!("provider not found");
        };
        let removed = store.providers.remove(idx);
        secrets.secrets.remove(&removed.secret_ref);
        let removed_model_ids: Vec<String> = store
            .models
            .iter()
            .filter(|m| m.provider_id == id)
            .map(|m| m.id.clone())
            .collect();
        store.models.retain(|m| m.provider_id != id);
        for mid in removed_model_ids {
            store.model_test_results.remove(&mid);
        }
        clear_bindings_for_provider(&mut store.agent_bindings, id);
        self.save_secrets(&secrets)?;
        self.save_store(&store)?;
        Ok(())
    }

    pub fn clone_provider(&self, id: &str, new_name: &str, new_api_key: &str) -> Result<Provider> {
        let store = self.load_store()?;
        let source = store
            .providers
            .iter()
            .find(|p| p.id == id)
            .context("provider not found")?
            .clone();
        let models: Vec<Model> = store
            .models
            .iter()
            .filter(|m| m.provider_id == id)
            .cloned()
            .collect();

        let created = self.add_provider(ProviderInput {
            name: new_name.to_string(),
            base_url: source.base_url,
            protocol: source.protocol,
            api_key: new_api_key.to_string(),
            headers: source.headers,
            compat: source.compat,
            enabled: source.enabled,
            notes: source.notes,
        })?;

        let mut store = self.load_store()?;
        let now = now_iso();
        for m in models {
            store.models.push(Model {
                id: format!("mdl_{}", Uuid::new_v4()),
                provider_id: created.id.clone(),
                model_id: m.model_id,
                display_name: m.display_name,
                enabled: m.enabled,
                capabilities: m.capabilities,
                created_at: now.clone(),
                updated_at: now.clone(),
            });
        }
        self.save_store(&store)?;
        Ok(created)
    }

    pub fn set_provider_enabled(&self, id: &str, enabled: bool) -> Result<()> {
        let mut store = self.load_store()?;
        let provider = store
            .providers
            .iter_mut()
            .find(|p| p.id == id)
            .context("provider not found")?;
        provider.enabled = enabled;
        provider.updated_at = now_iso();
        self.save_store(&store)?;
        Ok(())
    }

    pub fn add_model(&self, input: ModelInput) -> Result<Model> {
        let mut store = self.load_store()?;
        if !store.providers.iter().any(|p| p.id == input.provider_id) {
            anyhow::bail!("provider not found");
        }
        let now = now_iso();
        let model = Model {
            id: format!("mdl_{}", Uuid::new_v4()),
            provider_id: input.provider_id,
            model_id: input.model_id,
            display_name: input.display_name,
            enabled: input.enabled,
            capabilities: input.capabilities,
            created_at: now.clone(),
            updated_at: now,
        };
        store.models.push(model.clone());
        self.save_store(&store)?;
        Ok(model)
    }

    pub fn update_model(&self, id: &str, input: ModelInput) -> Result<Model> {
        let mut store = self.load_store()?;
        if !store.providers.iter().any(|p| p.id == input.provider_id) {
            anyhow::bail!("provider not found");
        }
        let model = store
            .models
            .iter_mut()
            .find(|m| m.id == id)
            .context("model not found")?;
        model.provider_id = input.provider_id;
        model.model_id = input.model_id;
        model.display_name = input.display_name;
        model.enabled = input.enabled;
        model.capabilities = input.capabilities;
        model.updated_at = now_iso();
        let out = model.clone();
        self.save_store(&store)?;
        Ok(out)
    }

    pub fn delete_model(&self, id: &str) -> Result<()> {
        let mut store = self.load_store()?;
        let before = store.models.len();
        store.models.retain(|m| m.id != id);
        if store.models.len() == before {
            anyhow::bail!("model not found");
        }
        clear_bindings_for_model(&mut store.agent_bindings, id);
        store.model_test_results.remove(id);
        self.save_store(&store)?;
        Ok(())
    }

    pub fn save_bindings(&self, bindings: AgentBindings) -> Result<()> {
        let mut store = self.load_store()?;
        store.agent_bindings = bindings;
        self.save_store(&store)?;
        Ok(())
    }

    pub fn get_api_key(&self, secret_ref: &str) -> Result<String> {
        let secrets = self.load_secrets()?;
        let key = secrets
            .secrets
            .get(secret_ref)
            .map(|s| s.api_key.clone())
            .context("secret not found")?;
        if key.is_empty() {
            anyhow::bail!("该提供商未配置 API Key（可能从仅有配置、无密钥的来源导入）");
        }
        Ok(key)
    }

    pub fn resolve_provider_key(&self, provider: &Provider) -> Result<String> {
        self.get_api_key(&provider.secret_ref)
    }

    pub fn enabled_providers_with_models(
        &self,
        store: &Store,
    ) -> Vec<(Provider, Vec<Model>)> {
        store
            .providers
            .iter()
            .filter(|p| p.enabled)
            .map(|p| {
                let models = store
                    .models
                    .iter()
                    .filter(|m| m.provider_id == p.id && m.enabled)
                    .cloned()
                    .collect();
                (p.clone(), models)
            })
            .collect()
    }

    pub fn list_test_prompts(&self) -> Result<Vec<TestPrompt>> {
        let store = self.load_store()?;
        Ok(store.test_prompts)
    }

    pub fn upsert_test_prompt(&self, input: TestPromptInput) -> Result<TestPrompt> {
        let mut store = self.load_store()?;
        let name = input.name.trim().to_string();
        let content = input.content.trim().to_string();
        if name.is_empty() {
            anyhow::bail!("提示词名称不能为空");
        }
        if content.is_empty() {
            anyhow::bail!("提示词内容不能为空");
        }
        let now = now_iso();

        if let Some(id) = input.id.as_ref().map(|s| s.trim()).filter(|s| !s.is_empty()) {
            let idx = store
                .test_prompts
                .iter()
                .position(|p| p.id == id)
                .with_context(|| format!("prompt not found: {id}"))?;
            if store.test_prompts.iter().any(|p| {
                p.id != id && p.name.eq_ignore_ascii_case(&name)
            }) {
                anyhow::bail!("提示词名称已存在：{name}");
            }
            let entry = &mut store.test_prompts[idx];
            entry.name = name;
            entry.content = content;
            entry.updated_at = now;
            let out = entry.clone();
            self.save_store(&store)?;
            return Ok(out);
        }

        if store
            .test_prompts
            .iter()
            .any(|p| p.name.eq_ignore_ascii_case(&name))
        {
            anyhow::bail!("提示词名称已存在：{name}");
        }
        let prompt = TestPrompt {
            id: format!("prompt_{}", Uuid::new_v4()),
            name,
            content,
            is_default: false,
            created_at: now.clone(),
            updated_at: now,
        };
        store.test_prompts.push(prompt.clone());
        self.save_store(&store)?;
        Ok(prompt)
    }

    pub fn delete_test_prompt(&self, id: &str) -> Result<()> {
        let mut store = self.load_store()?;
        let Some(idx) = store.test_prompts.iter().position(|p| p.id == id) else {
            anyhow::bail!("prompt not found");
        };
        if store.test_prompts[idx].is_default {
            anyhow::bail!("默认提示词不可删除，请先将其他提示词设为默认");
        }
        store.test_prompts.remove(idx);
        self.save_store(&store)?;
        Ok(())
    }

    /// Mark one saved prompt as the default (only one default at a time).
    pub fn set_default_test_prompt(&self, id: &str) -> Result<TestPrompt> {
        let mut store = self.load_store()?;
        let Some(idx) = store.test_prompts.iter().position(|p| p.id == id) else {
            anyhow::bail!("prompt not found");
        };
        let now = now_iso();
        for (i, p) in store.test_prompts.iter_mut().enumerate() {
            let want = i == idx;
            if p.is_default != want {
                p.is_default = want;
                p.updated_at = now.clone();
            }
        }
        // Keep default first for stable UX in selectors.
        if idx != 0 {
            let def = store.test_prompts.remove(idx);
            store.test_prompts.insert(0, def);
        }
        let out = store.test_prompts[0].clone();
        self.save_store(&store)?;
        Ok(out)
    }

    pub fn record_model_test_result(
        &self,
        model_id: &str,
        ok: bool,
        latency_ms: Option<u64>,
        tested_at: Option<String>,
    ) -> Result<ModelTestResult> {
        let mut store = self.load_store()?;
        if !store.models.iter().any(|m| m.id == model_id) {
            anyhow::bail!("model not found");
        }
        let entry = ModelTestResult {
            ok,
            tested_at: tested_at
                .filter(|s| !s.trim().is_empty())
                .unwrap_or_else(now_iso),
            latency_ms,
        };
        store
            .model_test_results
            .insert(model_id.to_string(), entry.clone());
        self.save_store(&store)?;
        Ok(entry)
    }
}

fn ensure_default_test_prompt(store: &mut Store) {
    if store.test_prompts.is_empty() {
        store.test_prompts = seed_test_prompts();
        return;
    }
    if !store.test_prompts.iter().any(|p| p.is_default) {
        // Prefer matching seed by id/name; otherwise prepend seed default.
        let seed = seed_test_prompts().into_iter().next().unwrap();
        if let Some(p) = store
            .test_prompts
            .iter_mut()
            .find(|p| p.id == seed.id || p.name.eq_ignore_ascii_case(&seed.name))
        {
            p.is_default = true;
        } else {
            store.test_prompts.insert(0, seed);
        }
    }
}

fn clear_bindings_for_provider(b: &mut AgentBindings, provider_id: &str) {
    if b.claude.provider_id.as_deref() == Some(provider_id) {
        b.claude.provider_id = None;
        b.claude.model_id = None;
    }
    if b.codex.provider_id.as_deref() == Some(provider_id) {
        b.codex.provider_id = None;
        b.codex.model_id = None;
    }
    if b.opencode.provider_id.as_deref() == Some(provider_id) {
        b.opencode.provider_id = None;
        b.opencode.model_id = None;
        b.opencode.small_model_id = None;
    }
    if b.pi.provider_id.as_deref() == Some(provider_id) {
        b.pi.provider_id = None;
        b.pi.model_id = None;
    }
}

fn clear_bindings_for_model(b: &mut AgentBindings, model_id: &str) {
    if b.claude.model_id.as_deref() == Some(model_id) {
        b.claude.model_id = None;
    }
    if b.claude.haiku_model_id.as_deref() == Some(model_id) {
        b.claude.haiku_model_id = None;
    }
    if b.claude.sonnet_model_id.as_deref() == Some(model_id) {
        b.claude.sonnet_model_id = None;
    }
    if b.claude.opus_model_id.as_deref() == Some(model_id) {
        b.claude.opus_model_id = None;
    }
    if b.codex.model_id.as_deref() == Some(model_id) {
        b.codex.model_id = None;
    }
    if b.opencode.model_id.as_deref() == Some(model_id) {
        b.opencode.model_id = None;
    }
    if b.opencode.small_model_id.as_deref() == Some(model_id) {
        b.opencode.small_model_id = None;
    }
    if b.pi.model_id.as_deref() == Some(model_id) {
        b.pi.model_id = None;
    }
}

pub fn now_iso() -> String {
    Utc::now().to_rfc3339()
}

pub fn mask_key(key: &str) -> String {
    if key.len() <= 4 {
        return "****".into();
    }
    format!("••••{}", &key[key.len() - 4..])
}

pub fn normalize_base_url(url: &str) -> String {
    url.trim().trim_end_matches('/').to_string()
}

pub fn key_fingerprint(key: &str) -> String {
    // short non-crypto fingerprint for import dedupe only
    let mut h: u64 = 0xcbf29ce484222325;
    for b in key.as_bytes() {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x100000001b3);
    }
    format!("{h:016x}")
}

fn read_json_or_default<T>(path: &Path) -> Result<T>
where
    T: serde::de::DeserializeOwned + Default,
{
    if !path.exists() {
        return Ok(T::default());
    }
    let text = fs::read_to_string(path)
        .with_context(|| format!("read {}", path.display()))?;
    if text.trim().is_empty() {
        return Ok(T::default());
    }
    serde_json::from_str(&text).with_context(|| format!("parse {}", path.display()))
}

fn write_json_atomic<T: serde::Serialize>(path: &Path, value: &T) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("tmp");
    let text = serde_json::to_string_pretty(value)?;
    fs::write(&tmp, text)?;
    fs::rename(&tmp, path)?;
    Ok(())
}

pub fn find_provider<'a>(store: &'a Store, id: &str) -> Option<&'a Provider> {
    store.providers.iter().find(|p| p.id == id)
}

pub fn find_model<'a>(store: &'a Store, id: &str) -> Option<&'a Model> {
    store.models.iter().find(|m| m.id == id)
}

pub fn resolve_upstream_model_id(store: &Store, model_record_id: &str) -> Option<String> {
    find_model(store, model_record_id).map(|m| m.model_id.clone())
}

pub fn provider_slug(provider: &Provider) -> String {
    let mut s = String::new();
    for c in provider.name.chars() {
        if c.is_ascii_alphanumeric() {
            s.push(c.to_ascii_lowercase());
        } else if c == '-' || c == '_' || c.is_whitespace() {
            if !s.ends_with('-') {
                s.push('-');
            }
        }
    }
    let s = s.trim_matches('-').to_string();
    if s.is_empty() {
        let tail = provider.id.rsplit('_').next().unwrap_or("p");
        format!("p-{tail}")
    } else {
        s
    }
}

pub fn resolve_provider_write_key(
    provider: &Provider,
    existing: &serde_json::Map<String, serde_json::Value>,
) -> String {
    let want = normalize_base_url(&provider.base_url);
    for (key, val) in existing {
        let base = val
            .pointer("/options/baseURL")
            .or_else(|| val.pointer("/options/baseUrl"))
            .or_else(|| val.get("baseUrl"))
            .or_else(|| val.get("baseURL"))
            .and_then(|v| v.as_str())
            .unwrap_or("");
        if !base.is_empty() && normalize_base_url(base) == want {
            return key.clone();
        }
    }
    provider_slug(provider)
}

/// Same endpoint = same provider across agents (ignore key presence differences)
pub fn provider_endpoint_key(base_url: &str, protocol: &Protocol) -> String {
    format!("{}|{}", normalize_base_url(base_url), protocol.as_str())
}

pub fn provider_dedupe_key(base_url: &str, api_key: &str, protocol: &Protocol) -> String {
    format!(
        "{}|{}|{}",
        normalize_base_url(base_url),
        key_fingerprint(api_key),
        protocol.as_str()
    )
}

pub fn existing_endpoint_keys(store: &Store) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for p in &store.providers {
        map.insert(
            provider_endpoint_key(&p.base_url, &p.protocol),
            p.id.clone(),
        );
    }
    map
}

pub fn existing_dedupe_keys(store: &Store, secrets: &Secrets) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for p in &store.providers {
        let key = secrets
            .secrets
            .get(&p.secret_ref)
            .map(|s| s.api_key.as_str())
            .unwrap_or("");
        map.insert(
            provider_dedupe_key(&p.base_url, key, &p.protocol),
            p.id.clone(),
        );
    }
    map
}

pub fn modelhub_root_display(paths: &ModelHubPaths) -> PathBuf {
    paths.root.clone()
}
