use anyhow::{Context, Result};
use chrono::Utc;
use fs_err as fs;
use std::path::{Path, PathBuf};

use crate::paths::ModelHubPaths;

pub fn backup_file(
    paths: &ModelHubPaths,
    agent: &str,
    source: &Path,
    keep: u32,
) -> Result<Option<PathBuf>> {
    if !source.exists() {
        return Ok(None);
    }
    let stamp = Utc::now().format("%Y%m%d-%H%M%S").to_string();
    let dir = paths.backups_dir().join(agent).join(&stamp);
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
    out.sort_by(|a, b| b.stamp.cmp(&a.stamp));
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

pub fn restore_backup(backup_path: &Path, target: &Path) -> Result<()> {
    if !backup_path.exists() {
        anyhow::bail!("backup file not found");
    }
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp = target.with_extension("restore-tmp");
    fs::copy(backup_path, &tmp)?;
    fs::rename(&tmp, target)?;
    Ok(())
}
