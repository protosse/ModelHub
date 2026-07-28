use anyhow::{bail, Context, Result};
use chrono::Utc;
use fs_err as fs;
use std::path::{Path, PathBuf};

use crate::paths::ModelHubPaths;
use crate::store::AppConfig;

/// Stamp shared by all files backed up in one Apply/restore for one Agent.
/// Millisecond precision avoids same-second collisions across separate operations.
pub fn new_stamp() -> String {
    Utc::now().format("%Y%m%d-%H%M%S-%3f").to_string()
}

pub fn backup_file(
    paths: &ModelHubPaths,
    agent: &str,
    source: &Path,
    keep: u32,
    stamp: &str,
) -> Result<Option<PathBuf>> {
    if !source.exists() {
        return Ok(None);
    }
    let dir = paths.backups_dir().join(agent).join(stamp);
    fs::create_dir_all(&dir)?;
    let file_name = source
        .file_name()
        .context("source has no file name")?
        .to_string_lossy()
        .to_string();
    let dest = dir.join(&file_name);
    fs::copy(source, &dest)?;
    rotate_backups(&paths.backups_dir().join(agent), keep)?;
    Ok(Some(dest))
}

fn rotate_backups(agent_dir: &Path, keep: u32) -> Result<()> {
    if !agent_dir.exists() {
        return Ok(());
    }
    let mut entries: Vec<_> = fs::read_dir(agent_dir)?
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_dir())
        .collect();
    entries.sort_by_key(|e| e.file_name());
    let keep = keep as usize;
    if entries.len() <= keep {
        return Ok(());
    }
    let remove_count = entries.len() - keep;
    for e in entries.into_iter().take(remove_count) {
        let _ = fs::remove_dir_all(e.path());
    }
    Ok(())
}

pub fn list_backups(paths: &ModelHubPaths) -> Result<Vec<BackupEntry>> {
    let root = paths.backups_dir();
    if !root.exists() {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    for agent_entry in fs::read_dir(&root)?.filter_map(|e| e.ok()) {
        if !agent_entry.path().is_dir() {
            continue;
        }
        let agent = agent_entry.file_name().to_string_lossy().to_string();
        for stamp_entry in fs::read_dir(agent_entry.path())?.filter_map(|e| e.ok()) {
            if !stamp_entry.path().is_dir() {
                continue;
            }
            let stamp = stamp_entry.file_name().to_string_lossy().to_string();
            for file in fs::read_dir(stamp_entry.path())?.filter_map(|e| e.ok()) {
                if file.path().is_file() {
                    out.push(BackupEntry {
                        agent: agent.clone(),
                        stamp: stamp.clone(),
                        file_name: file.file_name().to_string_lossy().to_string(),
                        path: file.path().display().to_string(),
                    });
                }
            }
        }
    }
    // Newest first: stamp desc, then agent, then file name for stable rows.
    out.sort_by(|a, b| {
        b.stamp
            .cmp(&a.stamp)
            .then_with(|| a.agent.cmp(&b.agent))
            .then_with(|| a.file_name.cmp(&b.file_name))
    });
    Ok(out)
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupEntry {
    pub agent: String,
    pub stamp: String,
    pub file_name: String,
    pub path: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RestoreBackupResult {
    pub agent: String,
    pub stamp: String,
    pub ok: bool,
    pub message: String,
    /// Live config paths that were written.
    pub files: Vec<String>,
    /// Safety backup stamp created from current live files before restore (if any).
    pub safety_stamp: Option<String>,
    pub restart_required: bool,
}

fn validate_agent_stamp(agent: &str, stamp: &str) -> Result<()> {
    if !is_safe_segment(agent) {
        bail!("invalid agent id");
    }
    if !is_safe_segment(stamp) {
        bail!("invalid backup stamp");
    }
    Ok(())
}

fn is_safe_segment(s: &str) -> bool {
    !s.is_empty()
        && !s.contains('/')
        && !s.contains('\\')
        && !s.contains("..")
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
}

/// Map a file name inside a backup stamp dir to the current live target path.
fn resolve_restore_target(
    agent: &str,
    file_name: &str,
    config: &AppConfig,
) -> Result<Option<PathBuf>> {
    let overrides = &config.paths;
    let target = match agent {
        "claude" => match file_name {
            "settings.json" => Some(ModelHubPaths::claude_settings(overrides)?),
            _ => None,
        },
        "codex" => match file_name {
            "config.toml" => Some(ModelHubPaths::codex_config(overrides)?),
            _ => None,
        },
        "opencode" => match file_name {
            // Main config may have been backed up as .json or .jsonc; always
            // restore onto the currently detected live path.
            "opencode.json" | "opencode.jsonc" => {
                Some(ModelHubPaths::opencode_config(overrides)?)
            }
            "auth.json" => Some(ModelHubPaths::opencode_auth(overrides)?),
            _ => None,
        },
        "pi" => match file_name {
            "models.json" => Some(ModelHubPaths::pi_models(overrides)?),
            "settings.json" => Some(ModelHubPaths::pi_settings(overrides)?),
            _ => None,
        },
        _ => None,
    };
    Ok(target)
}

fn restart_required_for(agent: &str) -> bool {
    matches!(agent, "claude" | "codex")
}

pub fn restore_file(backup_path: &Path, target: &Path) -> Result<()> {
    if !backup_path.exists() {
        bail!("backup file not found");
    }
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent)?;
    }
    // Use a unique temp name so concurrent restores / odd extensions don't clash.
    let tmp = target.with_extension(format!(
        "restore-tmp-{}",
        Utc::now().format("%Y%m%d%H%M%S%3f")
    ));
    fs::copy(backup_path, &tmp).with_context(|| {
        format!(
            "copy backup {} → temp {}",
            backup_path.display(),
            tmp.display()
        )
    })?;
    fs::rename(&tmp, target).with_context(|| {
        format!(
            "replace {} with restored content",
            target.display()
        )
    })?;
    Ok(())
}

/// Restore one backup snapshot (agent + stamp) onto current live Agent paths.
///
/// Flow:
/// 1. List files in the stamp directory
/// 2. Map known file names → current live targets
/// 3. Safety-backup existing live targets (new stamp, same keep policy)
/// 4. Copy each mapped backup file onto its live target
///
/// Only known Agent config files are restored; unknown names are reported and skipped.
pub fn restore_snapshot(
    paths: &ModelHubPaths,
    config: &AppConfig,
    agent: &str,
    stamp: &str,
) -> Result<RestoreBackupResult> {
    validate_agent_stamp(agent, stamp)?;

    let dir = paths.backups_dir().join(agent).join(stamp);
    if !dir.is_dir() {
        bail!("backup snapshot not found: {agent}/{stamp}");
    }

    let mut backup_files: Vec<PathBuf> = fs::read_dir(&dir)?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.is_file())
        .collect();
    backup_files.sort();

    if backup_files.is_empty() {
        bail!("backup snapshot is empty: {agent}/{stamp}");
    }

    let mut plan: Vec<(PathBuf, PathBuf, String)> = Vec::new(); // (src, dest, name)
    let mut skipped: Vec<String> = Vec::new();

    for src in &backup_files {
        let name = src
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default();
        match resolve_restore_target(agent, &name, config)? {
            Some(dest) => plan.push((src.clone(), dest, name)),
            None => skipped.push(name),
        }
    }

    if plan.is_empty() {
        bail!(
            "snapshot has no restorable files for agent `{agent}` (found: {})",
            backup_files
                .iter()
                .filter_map(|p| p.file_name().map(|s| s.to_string_lossy().to_string()))
                .collect::<Vec<_>>()
                .join(", ")
        );
    }

    // Safety backup of current live files that will be overwritten.
    let keep = config.backup_keep_count;
    let safety_stamp = new_stamp();
    let mut safety_wrote = false;
    for (_, dest, _) in &plan {
        if dest.exists() {
            if backup_file(paths, agent, dest, keep, &safety_stamp)?.is_some() {
                safety_wrote = true;
            }
        }
    }
    let safety_stamp_out = if safety_wrote {
        Some(safety_stamp)
    } else {
        None
    };

    let mut restored: Vec<String> = Vec::new();
    for (src, dest, _name) in &plan {
        restore_file(src, dest)?;
        restored.push(dest.display().to_string());
    }

    let restart_required = restart_required_for(agent);
    let mut message = format!(
        "已恢复 {} 个文件到 {} 当前配置路径",
        restored.len(),
        agent
    );
    if let Some(ref s) = safety_stamp_out {
        message.push_str(&format!("；恢复前已备份当前文件（{s}）"));
    }
    if !skipped.is_empty() {
        message.push_str(&format!(
            "；已跳过无法识别的文件：{}",
            skipped.join(", ")
        ));
    }
    if restart_required {
        message.push_str("；建议重启对应 Agent 使配置生效");
    }

    Ok(RestoreBackupResult {
        agent: agent.to_string(),
        stamp: stamp.to_string(),
        ok: true,
        message,
        files: restored,
        safety_stamp: safety_stamp_out,
        restart_required,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::PathOverrides;
    use std::fs;
    
    fn cfg() -> AppConfig {
        AppConfig {
            backup_keep_count: 10,
            ..AppConfig::default()
        }
    }

    #[test]
    fn safe_segment_rejects_traversal() {
        assert!(validate_agent_stamp("claude", "20260101-000000-001").is_ok());
        assert!(validate_agent_stamp("../x", "a").is_err());
        assert!(validate_agent_stamp("claude", "a/b").is_err());
        assert!(validate_agent_stamp("claude", "..").is_err());
    }

    fn make_tmp(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "modelhub-backup-test-{}-{}",
            label,
            Uuidish::new()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    // Tiny unique suffix without pulling uuid into tests.
    struct Uuidish;
    impl Uuidish {
        fn new() -> u128 {
            use std::time::{SystemTime, UNIX_EPOCH};
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        }
    }

    #[test]
    fn restore_snapshot_maps_and_safety_backs_up() {
        let tmp = make_tmp("claude");
        let root = tmp.join(".modelhub");
        let live = tmp.join("live");
        fs::create_dir_all(&live).unwrap();
        let settings = live.join("settings.json");
        fs::write(&settings, b"{\"current\":true}").unwrap();

        let paths = ModelHubPaths { root: root.clone() };
        let mut config = cfg();
        config.paths = PathOverrides {
            claude_settings: Some(settings.display().to_string()),
            ..PathOverrides::default()
        };

        // Seed a backup snapshot.
        let stamp = "20260101-120000-000";
        let snap_dir = paths.backups_dir().join("claude").join(stamp);
        fs::create_dir_all(&snap_dir).unwrap();
        fs::write(snap_dir.join("settings.json"), b"{\"restored\":true}").unwrap();

        let res = restore_snapshot(&paths, &config, "claude", stamp).unwrap();
        assert!(res.ok);
        assert_eq!(res.files.len(), 1);
        assert!(res.safety_stamp.is_some());
        assert!(res.restart_required);
        let body = fs::read_to_string(&settings).unwrap();
        assert!(body.contains("restored"));

        // Safety backup should hold previous content.
        let safety = paths
            .backups_dir()
            .join("claude")
            .join(res.safety_stamp.as_ref().unwrap())
            .join("settings.json");
        let safety_body = fs::read_to_string(safety).unwrap();
        assert!(safety_body.contains("current"));
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn restore_skips_unknown_files_but_restores_known() {
        let tmp = make_tmp("pi");
        let root = tmp.join(".modelhub");
        let live = tmp.join("live");
        fs::create_dir_all(&live).unwrap();
        let models = live.join("models.json");
        fs::write(&models, b"{\"old\":1}").unwrap();

        let paths = ModelHubPaths { root };
        let mut config = cfg();
        config.paths = PathOverrides {
            pi_models: Some(models.display().to_string()),
            pi_settings: Some(live.join("settings.json").display().to_string()),
            ..PathOverrides::default()
        };

        let stamp = "20260102-010203-004";
        let snap_dir = paths.backups_dir().join("pi").join(stamp);
        fs::create_dir_all(&snap_dir).unwrap();
        fs::write(snap_dir.join("models.json"), b"{\"new\":2}").unwrap();
        fs::write(snap_dir.join("notes.txt"), b"ignore me").unwrap();

        let res = restore_snapshot(&paths, &config, "pi", stamp).unwrap();
        assert!(res.ok);
        assert_eq!(res.files.len(), 1);
        assert!(res.message.contains("notes.txt"));
        let body = fs::read_to_string(&models).unwrap();
        assert!(body.contains("new"));
        assert!(!res.restart_required);
        let _ = fs::remove_dir_all(&tmp);
    }
}
