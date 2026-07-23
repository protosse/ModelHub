use anyhow::{Context, Result};
use std::path::PathBuf;

use crate::store::{DetectedPaths, PathOverrides};

#[derive(Debug, Clone)]
pub struct ModelHubPaths {
    pub root: PathBuf,
}

impl ModelHubPaths {
    pub fn default_location() -> Result<Self> {
        let home = dirs::home_dir().context("cannot resolve home directory")?;
        Ok(Self {
            root: home.join(".modelhub"),
        })
    }

    pub fn config_file(&self) -> PathBuf {
        self.root.join("config.json")
    }

    pub fn store_file(&self) -> PathBuf {
        self.root.join("store.json")
    }

    pub fn secrets_file(&self) -> PathBuf {
        self.root.join("secrets.json")
    }

    pub fn backups_dir(&self) -> PathBuf {
        self.root.join("backups")
    }

    pub fn home() -> Result<PathBuf> {
        dirs::home_dir().context("cannot resolve home directory")
    }

    pub fn claude_settings(overrides: &PathOverrides) -> Result<PathBuf> {
        if let Some(p) = &overrides.claude_settings {
            return Ok(PathBuf::from(p));
        }
        Ok(Self::home()?.join(".claude").join("settings.json"))
    }

    pub fn codex_config(overrides: &PathOverrides) -> Result<PathBuf> {
        if let Some(p) = &overrides.codex_config {
            return Ok(PathBuf::from(p));
        }
        Ok(Self::home()?.join(".codex").join("config.toml"))
    }

    pub fn opencode_config(overrides: &PathOverrides) -> Result<PathBuf> {
        if let Some(p) = &overrides.opencode_config {
            return Ok(PathBuf::from(p));
        }
        let home = Self::home()?;
        let base = home.join(".config").join("opencode");
        let json = base.join("opencode.json");
        if json.exists() {
            return Ok(json);
        }
        let jsonc = base.join("opencode.jsonc");
        if jsonc.exists() {
            return Ok(jsonc);
        }
        Ok(json)
    }

    pub fn opencode_auth(overrides: &PathOverrides) -> Result<PathBuf> {
        if let Some(p) = &overrides.opencode_auth {
            return Ok(PathBuf::from(p));
        }
        Ok(Self::home()?
            .join(".local")
            .join("share")
            .join("opencode")
            .join("auth.json"))
    }

    pub fn pi_models(overrides: &PathOverrides) -> Result<PathBuf> {
        if let Some(p) = &overrides.pi_models {
            return Ok(PathBuf::from(p));
        }
        Ok(Self::home()?.join(".pi").join("agent").join("models.json"))
    }

    pub fn pi_settings(overrides: &PathOverrides) -> Result<PathBuf> {
        if let Some(p) = &overrides.pi_settings {
            return Ok(PathBuf::from(p));
        }
        Ok(Self::home()?.join(".pi").join("agent").join("settings.json"))
    }

    pub fn pi_auth(overrides: &PathOverrides) -> Result<PathBuf> {
        if let Some(p) = &overrides.pi_auth {
            return Ok(PathBuf::from(p));
        }
        Ok(Self::home()?.join(".pi").join("agent").join("auth.json"))
    }

    pub fn detect(&self, overrides: &PathOverrides) -> Result<DetectedPaths> {
        let claude = Self::claude_settings(overrides)?;
        let codex = Self::codex_config(overrides)?;
        let opencode = Self::opencode_config(overrides)?;
        let pi = Self::pi_models(overrides)?;
        Ok(DetectedPaths {
            modelhub_dir: self.root.display().to_string(),
            claude_settings: claude.display().to_string(),
            claude_exists: claude.exists(),
            codex_config: codex.display().to_string(),
            codex_exists: codex.exists(),
            opencode_config: opencode.display().to_string(),
            opencode_exists: opencode.exists(),
            pi_models: pi.display().to_string(),
            pi_exists: pi.exists(),
        })
    }
}
