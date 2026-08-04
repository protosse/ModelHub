mod types;

pub use types::*;

use anyhow::{Context, Result};
use chrono::Utc;
use fs_err as fs;
use std::collections::HashMap;
use std::path::Path;
use uuid::Uuid;

use crate::file_io::write_atomic;
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
        let config: AppConfig = read_json_or_default(&self.paths.config_file())?;
        config.validate()?;
        Ok(config)
    }

    pub fn save_config(&self, config: &AppConfig) -> Result<()> {
        self.ensure_dirs()?;
        config.validate()?;
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

    /// Commit Store and Secrets together. Secrets are written first so Store
    /// never points at a key that has not been persisted; if the Store write
    /// fails, restore the previous Secrets bytes.
    pub fn save_store_and_secrets(&self, store: &Store, secrets: &Secrets) -> Result<()> {
        self.ensure_dirs()?;
        let secrets_path = self.paths.secrets_file();
        let previous_secrets = if secrets_path.exists() {
            Some(fs::read(&secrets_path)?)
        } else {
            None
        };

        self.save_secrets(secrets)?;
        if let Err(store_error) = self.save_store(store) {
            let rollback = match previous_secrets {
                Some(bytes) => write_atomic(&secrets_path, &bytes),
                None => {
                    if secrets_path.exists() {
                        fs::remove_file(&secrets_path).map_err(anyhow::Error::from)
                    } else {
                        Ok(())
                    }
                }
            };
            if let Err(rollback_error) = rollback {
                anyhow::bail!(
                    "save store failed: {store_error}; secrets rollback also failed: {rollback_error}"
                );
            }
            return Err(store_error);
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
        self.save_store_and_secrets(&store, &secrets)?;
        Ok(provider)
    }

    /// Create a provider and its initial model set in one Store/Secrets commit.
    /// Unlike the regular create flow, quick add only joins the OC/Pi catalogs
    /// explicitly selected by the user.
    pub fn quick_add_provider(
        &self,
        input: ProviderInput,
        model_inputs: Vec<QuickAddModelInput>,
        agents: &[String],
    ) -> Result<(Provider, Vec<Model>)> {
        let mut store = self.load_store()?;
        let mut secrets = self.load_secrets()?;
        let name = input.name.trim().to_string();
        let base_url = normalize_base_url(&input.base_url);
        if name.is_empty() {
            anyhow::bail!("提供商名称不能为空");
        }
        if base_url.is_empty() {
            anyhow::bail!("Base URL 不能为空");
        }
        if input.api_key.trim().is_empty() {
            anyhow::bail!("API Key 不能为空");
        }
        if model_inputs.is_empty() {
            anyhow::bail!("请至少添加一个模型");
        }
        if store
            .providers
            .iter()
            .any(|p| p.name.eq_ignore_ascii_case(&name))
        {
            anyhow::bail!("提供商名称已存在：{name}");
        }

        let mut seen = std::collections::HashSet::new();
        for model in &model_inputs {
            let id = model.model_id.trim();
            if id.is_empty() {
                anyhow::bail!("Model ID 不能为空");
            }
            if !seen.insert(id.to_string()) {
                anyhow::bail!("存在重复模型：{id}");
            }
        }

        let now = now_iso();
        let secret_ref = format!("sec_{}", Uuid::new_v4());
        let provider = Provider {
            id: format!("prov_{}", Uuid::new_v4()),
            name,
            base_url,
            protocol: input.protocol,
            enabled: input.enabled,
            notes: input.notes,
            secret_ref: secret_ref.clone(),
            created_at: now.clone(),
            updated_at: now.clone(),
        };
        let models: Vec<Model> = model_inputs
            .into_iter()
            .map(|input| {
                let model_id = input.model_id.trim().to_string();
                Model {
                    id: format!("mdl_{}", Uuid::new_v4()),
                    provider_id: provider.id.clone(),
                    display_name: if input.display_name.trim().is_empty() {
                        model_id.clone()
                    } else {
                        input.display_name.trim().to_string()
                    },
                    model_id,
                    created_at: now.clone(),
                    updated_at: now.clone(),
                }
            })
            .collect();

        store.providers.push(provider.clone());
        store.models.extend(models.iter().cloned());
        let selected = |agent: &str| agents.iter().any(|item| item == agent);
        if selected("opencode") {
            if let Some(entries) = store.agent_catalogs.opencode.as_mut() {
                entries.push(CatalogEntry {
                    provider_id: provider.id.clone(),
                    model_ids: Vec::new(),
                });
            }
        }
        if selected("pi") {
            if let Some(entries) = store.agent_catalogs.pi.as_mut() {
                entries.push(CatalogEntry {
                    provider_id: provider.id.clone(),
                    model_ids: Vec::new(),
                });
            }
        }
        secrets.secrets.insert(
            secret_ref,
            SecretEntry {
                api_key: input.api_key.trim().to_string(),
                updated_at: now,
            },
        );
        self.save_store_and_secrets(&store, &secrets)?;
        Ok((provider, models))
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
        let secrets_changed = !input.api_key.is_empty();
        if secrets_changed {
            secrets.secrets.insert(
                secret_ref,
                SecretEntry {
                    api_key: input.api_key,
                    updated_at: now,
                },
            );
        }
        let out = provider.clone();
        if secrets_changed {
            self.save_store_and_secrets(&store, &secrets)?;
        } else {
            self.save_store(&store)?;
        }
        Ok(out)
    }

    pub fn delete_provider(&self, id: &str) -> Result<()> {
        let n = self.delete_providers(&[id.to_string()])?;
        if n == 0 {
            anyhow::bail!("provider not found");
        }
        Ok(())
    }

    /// Batch-delete providers in one read/modify/write operation. Unknown ids
    /// are ignored, matching `delete_models` behavior.
    pub fn delete_providers(&self, ids: &[String]) -> Result<usize> {
        if ids.is_empty() {
            return Ok(0);
        }
        let mut store = self.load_store()?;
        let mut secrets = self.load_secrets()?;
        let id_set: std::collections::HashSet<&str> = ids.iter().map(String::as_str).collect();
        let removed: Vec<Provider> = store
            .providers
            .iter()
            .filter(|p| id_set.contains(p.id.as_str()))
            .cloned()
            .collect();
        if removed.is_empty() {
            return Ok(0);
        }
        let removed_ids: std::collections::HashSet<&str> =
            removed.iter().map(|p| p.id.as_str()).collect();
        let removed_model_ids: Vec<String> = store
            .models
            .iter()
            .filter(|m| removed_ids.contains(m.provider_id.as_str()))
            .map(|m| m.id.clone())
            .collect();
        store
            .providers
            .retain(|p| !removed_ids.contains(p.id.as_str()));
        store
            .models
            .retain(|m| !removed_ids.contains(m.provider_id.as_str()));
        for provider in &removed {
            secrets.secrets.remove(&provider.secret_ref);
            clear_bindings_for_provider(&mut store.agent_bindings, &provider.id);
            clear_catalogs_for_provider(&mut store.agent_catalogs, &provider.id);
        }
        for mid in removed_model_ids {
            store.model_test_results.remove(&mid);
        }
        self.save_store_and_secrets(&store, &secrets)?;
        Ok(removed.len())
    }

    pub fn clone_provider(&self, id: &str, new_name: &str, new_api_key: &str) -> Result<Provider> {
        let mut store = self.load_store()?;
        let mut secrets = self.load_secrets()?;
        let source = store
            .providers
            .iter()
            .find(|p| p.id == id)
            .context("provider not found")?
            .clone();
        let source_models: Vec<Model> = store
            .models
            .iter()
            .filter(|m| m.provider_id == id)
            .cloned()
            .collect();
        let name = new_name.trim().to_string();
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
        let created = Provider {
            id: format!("prov_{}", Uuid::new_v4()),
            name,
            base_url: source.base_url,
            protocol: source.protocol,
            enabled: source.enabled,
            notes: source.notes,
            secret_ref: secret_ref.clone(),
            created_at: now.clone(),
            updated_at: now.clone(),
        };
        secrets.secrets.insert(
            secret_ref,
            SecretEntry {
                api_key: new_api_key.to_string(),
                updated_at: now.clone(),
            },
        );
        store.providers.push(created.clone());
        for model in source_models {
            store.models.push(Model {
                id: format!("mdl_{}", Uuid::new_v4()),
                provider_id: created.id.clone(),
                model_id: model.model_id,
                display_name: model.display_name,
                created_at: now.clone(),
                updated_at: now.clone(),
            });
        }
        if let Some(entries) = store.agent_catalogs.opencode.as_mut() {
            entries.push(CatalogEntry {
                provider_id: created.id.clone(),
                model_ids: Vec::new(),
            });
        }
        if let Some(entries) = store.agent_catalogs.pi.as_mut() {
            entries.push(CatalogEntry {
                provider_id: created.id.clone(),
                model_ids: Vec::new(),
            });
        }
        self.save_store_and_secrets(&store, &secrets)?;
        Ok(created)
    }

    pub fn add_model(&self, input: ModelInput) -> Result<Model> {
        let mut store = self.load_store()?;
        if !store.providers.iter().any(|p| p.id == input.provider_id) {
            anyhow::bail!("provider not found");
        }
        if store
            .models
            .iter()
            .any(|m| m.provider_id == input.provider_id && m.model_id == input.model_id)
        {
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
            m.id != id && m.provider_id == input.provider_id && m.model_id == input.model_id
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
            clear_model_from_catalogs(&mut store.agent_catalogs, mid);
            store.model_test_results.remove(mid);
        }
        self.save_store(&store)?;
        Ok(before - store.models.len())
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

        if let Some(id) = input
            .id
            .as_ref()
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
        {
            let idx = store
                .test_prompts
                .iter()
                .position(|p| p.id == id)
                .with_context(|| format!("prompt not found: {id}"))?;
            if store
                .test_prompts
                .iter()
                .any(|p| p.id != id && p.name.eq_ignore_ascii_case(&name))
            {
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

/// Remove a deleted model id from every catalog entry's `model_ids`. Only
/// entries that explicitly listed the id are touched — an entry that was `[]`
/// (= all models, dynamic) is left alone. If filtering empties an entry, the
/// whole entry is removed so the provider does not silently switch to
/// "all models" (`[]`), matching the UI rule that zero selected models = remove.
fn clear_model_from_catalogs(c: &mut AgentCatalogs, model_id: &str) {
    for entries in [c.opencode.as_mut(), c.pi.as_mut()].into_iter().flatten() {
        let mut i = 0;
        while i < entries.len() {
            if entries[i].model_ids.iter().any(|id| id == model_id) {
                entries[i].model_ids.retain(|id| id != model_id);
                if entries[i].model_ids.is_empty() {
                    entries.remove(i);
                    continue;
                }
            }
            i += 1;
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

fn read_json_or_default<T>(path: &Path) -> Result<T>
where
    T: serde::de::DeserializeOwned + Default,
{
    if !path.exists() {
        return Ok(T::default());
    }
    let text = fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    if text.trim().is_empty() {
        return Ok(T::default());
    }
    serde_json::from_str(&text).with_context(|| format!("parse {}", path.display()))
}

fn write_json_atomic<T: serde::Serialize>(path: &Path, value: &T) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let text = serde_json::to_string_pretty(value)?;
    write_atomic(path, text.as_bytes())
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
        } else if (c == '-' || c == '_' || c.is_whitespace()) && !s.ends_with('-') {
            s.push('-');
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
/// `reserved` holds on-disk provider keys that are NOT managed by ModelHub
/// (native/user blocks). Generated keys also avoid these so a ModelHub provider
/// whose slug happens to equal a native block's key never overwrites or takes
/// over that native block — it lands at `<slug>-2` instead, and the native block
/// is left untouched (preserved on cleanup). Already-managed disk blocks are not
/// reserved: they carry `_modelhub.providerId` and are matched/reused directly.
pub fn assign_catalog_write_keys_with_reserved(
    providers: &[Provider],
    reserved: &std::collections::HashSet<String>,
) -> HashMap<String, String> {
    let mut used: std::collections::HashSet<String> = reserved.iter().cloned().collect();
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

/// Same endpoint = same provider across agents (ignore key presence differences).
///
/// For OpenAI-style protocols (completions / responses) a trailing `/v1` is
/// stripped, mirroring `provider_base_for_match` and the `/v1` that Apply
/// auto-appends. Otherwise a Provider saved as `https://x` and the same endpoint
/// written back to an agent file as `https://x/v1` would compute different keys,
/// so re-importing/scanning would treat the ModelHub-written block as a brand
/// new provider and produce duplicate imports. Anthropic endpoints keep the
/// full URL (no `/v1` convention).
pub fn provider_endpoint_key(base_url: &str, protocol: &Protocol) -> String {
    let base = normalize_base_url(base_url);
    let canonical = match protocol {
        Protocol::OpenaiCompletions | Protocol::OpenaiResponses => {
            base.strip_suffix("/v1").unwrap_or(&base).to_string()
        }
        Protocol::AnthropicMessages => base,
    };
    format!("{}|{}", canonical, protocol.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn service(label: &str) -> StoreService {
        StoreService::new(ModelHubPaths {
            root: std::env::temp_dir().join(format!("modelhub-store-{label}-{}", Uuid::new_v4())),
        })
    }

    fn provider(name: &str) -> ProviderInput {
        ProviderInput {
            name: name.into(),
            base_url: format!("https://{name}.example.com"),
            protocol: Protocol::OpenaiResponses,
            api_key: format!("key-{name}"),
            enabled: true,
            notes: String::new(),
        }
    }

    #[test]
    fn batch_delete_providers_updates_store_and_secrets_once() {
        let svc = service("delete-providers");
        let a = svc.add_provider(provider("a")).unwrap();
        let b = svc.add_provider(provider("b")).unwrap();
        let removed = svc
            .delete_providers(&[a.id.clone(), "missing".into(), b.id.clone()])
            .unwrap();

        assert_eq!(removed, 2);
        assert!(svc.load_store().unwrap().providers.is_empty());
        assert!(svc.load_secrets().unwrap().secrets.is_empty());
        fs::remove_dir_all(&svc.paths.root).unwrap();
    }

    #[test]
    fn clone_provider_commits_provider_secret_models_and_catalogs_together() {
        let svc = service("clone-provider");
        let source = svc.add_provider(provider("source")).unwrap();
        svc.add_model(ModelInput {
            provider_id: source.id.clone(),
            model_id: "model-a".into(),
            display_name: "Model A".into(),
        })
        .unwrap();

        let cloned = svc
            .clone_provider(&source.id, "cloned", "cloned-key")
            .unwrap();
        let store = svc.load_store().unwrap();
        let secrets = svc.load_secrets().unwrap();

        assert_eq!(store.providers.len(), 2);
        assert!(store
            .models
            .iter()
            .any(|model| model.provider_id == cloned.id && model.model_id == "model-a"));
        assert_eq!(secrets.secrets[&cloned.secret_ref].api_key, "cloned-key");
        for catalog in [
            store.agent_catalogs.opencode.as_ref().unwrap(),
            store.agent_catalogs.pi.as_ref().unwrap(),
        ] {
            assert!(catalog.iter().any(|entry| entry.provider_id == cloned.id));
        }
        fs::remove_dir_all(&svc.paths.root).unwrap();
    }

    #[test]
    fn invalid_backup_keep_count_is_rejected_on_save_and_load() {
        let svc = service("invalid-config");
        let config = AppConfig {
            backup_keep_count: 0,
            ..AppConfig::default()
        };
        assert!(svc.save_config(&config).is_err());

        svc.ensure_dirs().unwrap();
        fs::write(
            svc.paths.config_file(),
            serde_json::to_vec(&config).unwrap(),
        )
        .unwrap();
        assert!(svc.load_config().is_err());
        fs::remove_dir_all(&svc.paths.root).unwrap();
    }

    #[test]
    fn store_failure_rolls_secrets_back() {
        let svc = service("secrets-rollback");
        let mut original = Secrets::default();
        original.secrets.insert(
            "sec_1".into(),
            SecretEntry {
                api_key: "old".into(),
                updated_at: "old".into(),
            },
        );
        svc.save_secrets(&original).unwrap();
        fs::create_dir_all(svc.paths.store_file()).unwrap();

        let mut changed = original.clone();
        changed.secrets.get_mut("sec_1").unwrap().api_key = "new".into();
        assert!(svc
            .save_store_and_secrets(&Store::default(), &changed)
            .is_err());

        assert_eq!(svc.load_secrets().unwrap().secrets["sec_1"].api_key, "old");
        fs::remove_dir_all(&svc.paths.root).unwrap();
    }

    #[test]
    fn quick_add_only_joins_selected_agent_catalogs() {
        let svc = service("quick-add-catalogs");
        let (created, models) = svc
            .quick_add_provider(
                provider("quick"),
                vec![
                    QuickAddModelInput {
                        model_id: "model-a".into(),
                        display_name: "Model A".into(),
                    },
                    QuickAddModelInput {
                        model_id: "model-b".into(),
                        display_name: String::new(),
                    },
                ],
                &["opencode".into()],
            )
            .unwrap();

        let store = svc.load_store().unwrap();
        let opencode = store.agent_catalogs.opencode.unwrap();
        let pi = store.agent_catalogs.pi.unwrap();
        assert_eq!(models.len(), 2);
        assert!(models.iter().all(|model| model.provider_id == created.id));
        assert!(opencode.iter().any(|entry| entry.provider_id == created.id));
        assert!(!pi.iter().any(|entry| entry.provider_id == created.id));
        assert_eq!(
            svc.load_secrets().unwrap().secrets[&created.secret_ref].api_key,
            "key-quick"
        );

        fs::remove_dir_all(&svc.paths.root).unwrap();
    }

    #[test]
    fn provider_endpoint_key_normalizes_v1_for_openai_protocols() {
        // OpenAI-style protocols strip /v1 so the same endpoint written with or
        // without /v1 produces the same key (import deduplication).
        let base = "https://api.example.com";
        let openai = Protocol::OpenaiResponses;
        assert_eq!(
            provider_endpoint_key(base, &openai),
            provider_endpoint_key(&format!("{base}/v1"), &openai),
        );
        // Anthropic keeps the full URL (no /v1 convention); key uses `|` separator.
        let anthropic = Protocol::AnthropicMessages;
        let anthropic_key = provider_endpoint_key(base, &anthropic);
        assert_eq!(anthropic_key, "https://api.example.com|anthropic-messages",);
    }

    #[test]
    fn delete_model_clears_model_ids_from_catalogs() {
        let svc = service("delete-model-catalog");
        let p = svc.add_provider(provider("x")).unwrap();
        let m1 = svc
            .add_model(ModelInput {
                provider_id: p.id.clone(),
                model_id: "gpt-4".into(),
                display_name: "GPT-4".into(),
            })
            .unwrap();
        let m2 = svc
            .add_model(ModelInput {
                provider_id: p.id.clone(),
                model_id: "gpt-3.5".into(),
                display_name: "GPT-3.5".into(),
            })
            .unwrap();

        // OC: explicit subset [m1, m2]; PI: empty = all models.
        svc.set_agent_catalog(
            "opencode",
            &[CatalogEntry {
                provider_id: p.id.clone(),
                model_ids: vec![m1.id.clone(), m2.id.clone()],
            }],
        )
        .unwrap();
        svc.set_agent_catalog(
            "pi",
            &[CatalogEntry {
                provider_id: p.id.clone(),
                model_ids: vec![],
            }],
        )
        .unwrap();

        // Delete m1 — OC subset shrinks to [m2]; PI empty stays empty.
        svc.delete_model(&m1.id).unwrap();
        let store = svc.load_store().unwrap();
        let oc_entry = store
            .agent_catalogs
            .opencode
            .as_ref()
            .and_then(|v| v.iter().find(|e| e.provider_id == p.id));
        let pi_entry = store
            .agent_catalogs
            .pi
            .as_ref()
            .and_then(|v| v.iter().find(|e| e.provider_id == p.id));
        assert_eq!(
            oc_entry.map(|e| e.model_ids.clone()),
            Some(vec![m2.id.clone()])
        );
        assert_eq!(pi_entry.map(|e| e.model_ids.len()), Some(0));

        // Delete m2 — OC entry is now empty and should be removed entirely;
        // PI empty entry stays.
        svc.delete_model(&m2.id).unwrap();
        let store = svc.load_store().unwrap();
        let oc_remaining = store
            .agent_catalogs
            .opencode
            .as_ref()
            .and_then(|v| v.iter().find(|e| e.provider_id == p.id));
        let pi_remaining = store
            .agent_catalogs
            .pi
            .as_ref()
            .and_then(|v| v.iter().find(|e| e.provider_id == p.id));
        assert!(
            oc_remaining.is_none(),
            "empty subset entry should be removed from OC catalog"
        );
        assert!(
            pi_remaining.is_some(),
            "PI entry with empty subset (all models) stays"
        );

        fs::remove_dir_all(&svc.paths.root).unwrap();
    }

    #[test]
    fn write_keys_avoid_native_unmanaged_disk_keys() {
        let providers = vec![Provider {
            id: "prov_1".into(),
            name: "Native Provider".into(),
            base_url: "https://native.example.com".into(),
            protocol: Protocol::OpenaiResponses,
            enabled: true,
            notes: String::new(),
            secret_ref: "sec_1".into(),
            created_at: "now".into(),
            updated_at: "now".into(),
        }];
        // A native (unmanaged) disk block happens to have the same key slug.
        let reserved: std::collections::HashSet<String> =
            ["native-provider"].into_iter().map(String::from).collect();

        let keys = assign_catalog_write_keys_with_reserved(&providers, &reserved);

        // Should land at native-provider-2, not overwrite the native block.
        assert_eq!(keys.get("prov_1"), Some(&"native-provider-2".into()));
    }
}
