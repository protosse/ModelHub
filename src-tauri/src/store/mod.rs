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
        let mut changed = ensure_default_test_prompt(&mut store);
        changed |= migrate_agent_catalogs(&mut store);
        // Persist seed/migration for older store.json files.
        if path.exists() && changed {
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
        // New providers default into both OC/Pi sync catalogs (empty modelIds =
        // all models), matching the pre-`enabled` behavior of "new = synced".
        // Only when the catalog is already migrated (Some); None is seeded later.
        if let Some(list) = store.agent_catalogs.opencode.as_mut() {
            list.push(CatalogEntry {
                provider_id: provider.id.clone(),
                model_ids: Vec::new(),
            });
        }
        if let Some(list) = store.agent_catalogs.pi.as_mut() {
            list.push(CatalogEntry {
                provider_id: provider.id.clone(),
                model_ids: Vec::new(),
            });
        }
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
        provider.enabled = input.enabled;
        provider.notes = input.notes;
        provider.updated_at = now.clone();
        let secret_ref = provider.secret_ref.clone();
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
        clear_catalogs_for_provider(&mut store.agent_catalogs, id);
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
                created_at: now.clone(),
                updated_at: now.clone(),
            });
        }
        self.save_store(&store)?;
        Ok(created)
    }

    pub fn add_model(&self, input: ModelInput) -> Result<Model> {
        let mut store = self.load_store()?;
        if !store.providers.iter().any(|p| p.id == input.provider_id) {
            anyhow::bail!("provider not found");
        }
        if store.models.iter().any(|m| {
            m.provider_id == input.provider_id && m.model_id == input.model_id
        }) {
            anyhow::bail!("模型已存在：{}", input.model_id);
        }
        let now = now_iso();
        let model = Model {
            id: format!("mdl_{}", Uuid::new_v4()),
            provider_id: input.provider_id,
            model_id: input.model_id,
            display_name: input.display_name,
            created_at: now.clone(),
            updated_at: now,
        };
        store.models.push(model.clone());
        self.save_store(&store)?;
        Ok(model)
    }

    /// Add multiple models in a single store read + write. Bails before saving if
    /// any provider_id is unknown or any model_id duplicates, so the batch is atomic.
    pub fn add_models(&self, inputs: Vec<ModelInput>) -> Result<Vec<Model>> {
        let mut store = self.load_store()?;
        let now = now_iso();
        let mut out = Vec::with_capacity(inputs.len());
        let mut batch_seen = std::collections::HashSet::<(String, String)>::new();
        for input in inputs {
            if !store.providers.iter().any(|p| p.id == input.provider_id) {
                anyhow::bail!("provider not found: {}", input.provider_id);
            }
            let key = (input.provider_id.clone(), input.model_id.clone());
            if !batch_seen.insert(key.clone()) {
                anyhow::bail!("本批存在重复模型：{}", input.model_id);
            }
            if store
                .models
                .iter()
                .any(|m| m.provider_id == input.provider_id && m.model_id == input.model_id)
            {
                anyhow::bail!("模型已存在：{}", input.model_id);
            }
            let model = Model {
                id: format!("mdl_{}", Uuid::new_v4()),
                provider_id: input.provider_id,
                model_id: input.model_id,
                display_name: input.display_name,
                created_at: now.clone(),
                updated_at: now.clone(),
            };
            store.models.push(model.clone());
            out.push(model);
        }
        self.save_store(&store)?;
        Ok(out)
    }

    pub fn update_model(&self, id: &str, input: ModelInput) -> Result<Model> {
        let mut store = self.load_store()?;
        if !store.providers.iter().any(|p| p.id == input.provider_id) {
            anyhow::bail!("provider not found");
        }
        if store.models.iter().any(|m| {
            m.id != id
                && m.provider_id == input.provider_id
                && m.model_id == input.model_id
        }) {
            anyhow::bail!("模型已存在：{}", input.model_id);
        }
        let model = store
            .models
            .iter_mut()
            .find(|m| m.id == id)
            .context("model not found")?;
        model.provider_id = input.provider_id;
        model.model_id = input.model_id;
        model.display_name = input.display_name;
        model.updated_at = now_iso();
        let out = model.clone();
        self.save_store(&store)?;
        Ok(out)
    }

    pub fn delete_model(&self, id: &str) -> Result<()> {
        let n = self.delete_models(&[id.to_string()])?;
        if n == 0 {
            anyhow::bail!("model not found");
        }
        Ok(())
    }

    /// Batch-delete models in a single load/save. Unknown ids are ignored.
    /// Returns how many models were actually removed.
    pub fn delete_models(&self, ids: &[String]) -> Result<usize> {
        if ids.is_empty() {
            return Ok(0);
        }
        let id_set: std::collections::HashSet<&str> = ids.iter().map(|s| s.as_str()).collect();
        let mut store = self.load_store()?;
        let before = store.models.len();
        let removed: Vec<String> = store
            .models
            .iter()
            .filter(|m| id_set.contains(m.id.as_str()))
            .map(|m| m.id.clone())
            .collect();
        if removed.is_empty() {
            return Ok(0);
        }
        store.models.retain(|m| !id_set.contains(m.id.as_str()));
        for mid in &removed {
            clear_bindings_for_model(&mut store.agent_bindings, mid);
            store.model_test_results.remove(mid);
        }
        self.save_store(&store)?;
        Ok(before - store.models.len())
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

    /// Providers (with the models an agent syncs) taken from its persistent
    /// catalog. Order follows the catalog list; unknown provider ids are skipped.
    /// An entry's empty `model_ids` means "all of that provider's models"
    /// (dynamic: includes models added later); a non-empty subset is filtered to
    /// those model row ids (dangling ids skipped). `agent` is "opencode" | "pi".
    pub fn catalog_providers_with_models(
        &self,
        store: &Store,
        agent: &str,
    ) -> Vec<(Provider, Vec<Model>)> {
        let entries = match agent {
            "opencode" => store.agent_catalogs.opencode.as_deref(),
            "pi" => store.agent_catalogs.pi.as_deref(),
            _ => None,
        }
        .unwrap_or(&[]);
        entries
            .iter()
            .filter_map(|entry| {
                let provider = store.providers.iter().find(|p| p.id == entry.provider_id)?;
                let subset: std::collections::HashSet<&str> =
                    entry.model_ids.iter().map(|s| s.as_str()).collect();
                let models = store
                    .models
                    .iter()
                    .filter(|m| m.provider_id == provider.id)
                    .filter(|m| subset.is_empty() || subset.contains(m.id.as_str()))
                    .cloned()
                    .collect();
                Some((provider.clone(), models))
            })
            .collect()
    }

    /// Persist an agent's sync catalog. Unknown/dangling provider ids are
    /// dropped, order preserved, duplicate providers removed (first wins). Each
    /// entry's `model_ids` is scrubbed of ids not belonging to that provider;
    /// empty stays empty (= all models). `agent` must be "opencode" | "pi".
    pub fn set_agent_catalog(&self, agent: &str, entries: &[CatalogEntry]) -> Result<()> {
        let mut store = self.load_store()?;
        let known: std::collections::HashSet<&str> =
            store.providers.iter().map(|p| p.id.as_str()).collect();
        let mut seen = std::collections::HashSet::new();
        let cleaned: Vec<CatalogEntry> = entries
            .iter()
            .filter(|e| known.contains(e.provider_id.as_str()))
            .filter(|e| seen.insert(e.provider_id.clone()))
            .map(|e| {
                let valid: Vec<String> = e
                    .model_ids
                    .iter()
                    .filter(|mid| {
                        store
                            .models
                            .iter()
                            .any(|m| &m.id == *mid && m.provider_id == e.provider_id)
                    })
                    .cloned()
                    .collect();
                CatalogEntry {
                    provider_id: e.provider_id.clone(),
                    model_ids: valid,
                }
            })
            .collect();
        match agent {
            "opencode" => store.agent_catalogs.opencode = Some(cleaned),
            "pi" => store.agent_catalogs.pi = Some(cleaned),
            other => anyhow::bail!("unknown catalog agent: {other}"),
        }
        self.save_store(&store)?;
        Ok(())
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

/// Ensure a default connectivity prompt exists.
/// Returns true if the store was mutated (caller may want to persist).
///
/// Content is only migrated when it still matches the *previous* seed text,
/// so user edits to the default prompt are preserved across loads.
fn ensure_default_test_prompt(store: &mut Store) -> bool {
    /// Previous default prompt body (pre seed-content change).
    const LEGACY_SEED_CONTENT: &str = "请只回复一个单词：ok";

    let seed = seed_test_prompts().into_iter().next().unwrap();
    let mut changed = false;

    if store.test_prompts.is_empty() {
        store.test_prompts = vec![seed];
        return true;
    }

    if let Some(p) = store
        .test_prompts
        .iter_mut()
        .find(|p| p.id == seed.id || p.name.eq_ignore_ascii_case(&seed.name))
    {
        // One-shot migration of the old seed body only.
        if p.content == LEGACY_SEED_CONTENT && p.content != seed.content {
            p.content = seed.content.clone();
            p.updated_at = now_iso();
            changed = true;
        }
        if p.id != seed.id {
            p.id = seed.id.clone();
            changed = true;
        }
    }

    if !store.test_prompts.iter().any(|p| p.is_default) {
        if let Some(p) = store
            .test_prompts
            .iter_mut()
            .find(|p| p.id == seed.id || p.name.eq_ignore_ascii_case(&seed.name))
        {
            p.is_default = true;
            changed = true;
        } else {
            store.test_prompts.insert(0, seed);
            changed = true;
        }
    }

    changed
}

/// Seed per-agent catalogs from the legacy global `Provider.enabled` on first
/// load. `None` means "never migrated" → fill both agents from enabled providers
/// so behavior matches the pre-catalog build. Once `Some`, we never re-seed (an
/// empty list is a deliberate user choice, not a missing migration).
fn migrate_agent_catalogs(store: &mut Store) -> bool {
    let needs_opencode = store.agent_catalogs.opencode.is_none();
    let needs_pi = store.agent_catalogs.pi.is_none();
    if !needs_opencode && !needs_pi {
        return false;
    }
    // Legacy enabled providers seed with an empty model subset = all models.
    let enabled: Vec<CatalogEntry> = store
        .providers
        .iter()
        .filter(|p| p.enabled)
        .map(|p| CatalogEntry {
            provider_id: p.id.clone(),
            model_ids: Vec::new(),
        })
        .collect();
    if needs_opencode {
        store.agent_catalogs.opencode = Some(enabled.clone());
    }
    if needs_pi {
        store.agent_catalogs.pi = Some(enabled);
    }
    true
}

fn clear_catalogs_for_provider(c: &mut AgentCatalogs, provider_id: &str) {
    if let Some(list) = c.opencode.as_mut() {
        list.retain(|e| e.provider_id != provider_id);
    }
    if let Some(list) = c.pi.as_mut() {
        list.retain(|e| e.provider_id != provider_id);
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
    let char_count = key.chars().count();
    if char_count <= 4 {
        return "****".into();
    }
    let tail: String = key.chars().skip(char_count - 4).collect();
    format!("••••{tail}")
}

pub fn normalize_base_url(url: &str) -> String {
    url.trim().trim_end_matches('/').to_string()
}

/// Base URL to write into an agent config, protocol-aware. OpenAI-style
/// endpoints (completions / responses) expect a `/v1` root — mirror the
/// connectivity-test behavior (`api_root`) and native gateway configs, which
/// all carry `/v1`. Anthropic gateways are used bare (no `/v1`), matching their
/// on-disk native configs. Keeps apply consistent with what testing actually
/// hits, so a model that tests OK also works after apply.
pub fn agent_write_base_url(base: &str, protocol: &Protocol) -> String {
    let b = normalize_base_url(base);
    match protocol {
        Protocol::OpenaiCompletions | Protocol::OpenaiResponses => {
            if b.ends_with("/v1") {
                b
            } else {
                format!("{b}/v1")
            }
        }
        Protocol::AnthropicMessages => b,
    }
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

/// Assign a unique write-out key per ModelHub-managed provider in an agent
/// directory (OpenCode / Pi). Keys derive from the provider name slug (names are
/// globally unique) and are de-duplicated within the set by appending `-2`, `-3`
/// so two providers sharing a base_url but differing by protocol (e.g.
/// `jianzhile` responses + `jianzhile-cc` anthropic) never collide. Returns a
/// map provider.id -> key. Both apply and preview must use this same map.
pub fn assign_catalog_write_keys(providers: &[Provider]) -> HashMap<String, String> {
    let mut used: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut out: HashMap<String, String> = HashMap::new();
    for p in providers {
        let base = provider_slug(p);
        let mut key = base.clone();
        let mut n = 2;
        while used.contains(&key) {
            key = format!("{base}-{n}");
            n += 1;
        }
        used.insert(key.clone());
        out.insert(p.id.clone(), key);
    }
    out
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
