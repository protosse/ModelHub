use anyhow::{Context, Result};
use fs_err as fs;
use std::path::{Path, PathBuf};
use uuid::Uuid;

fn temp_path(target: &Path) -> Result<PathBuf> {
    let parent = target.parent().context("target has no parent directory")?;
    let name = target
        .file_name()
        .context("target has no file name")?
        .to_string_lossy();
    Ok(parent.join(format!(".{name}.tmp-{}", Uuid::new_v4())))
}

#[cfg(not(windows))]
fn replace_file(source: &Path, target: &Path) -> std::io::Result<()> {
    fs::rename(source, target)
}

#[cfg(windows)]
fn replace_file(source: &Path, target: &Path) -> std::io::Result<()> {
    use std::iter;
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
    };

    let source_w: Vec<u16> = source
        .as_os_str()
        .encode_wide()
        .chain(iter::once(0))
        .collect();
    let target_w: Vec<u16> = target
        .as_os_str()
        .encode_wide()
        .chain(iter::once(0))
        .collect();
    let ok = unsafe {
        MoveFileExW(
            source_w.as_ptr(),
            target_w.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };
    if ok == 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}

pub fn write_atomic(path: &Path, contents: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp = temp_path(path)?;
    fs::write(&tmp, contents).with_context(|| format!("write temp file {}", tmp.display()))?;

    if let Ok(metadata) = fs::metadata(path) {
        fs::set_permissions(&tmp, metadata.permissions())?;
    }

    if let Err(error) = replace_file(&tmp, path) {
        let _ = fs::remove_file(&tmp);
        return Err(error).with_context(|| format!("replace {}", path.display()));
    }
    Ok(())
}

/// Replace a related set of files as one best-effort transaction. All payloads
/// must be serialized before calling this function. If a replacement fails,
/// files already replaced in this call are restored to their original bytes.
pub fn write_atomic_group(files: &[(PathBuf, Vec<u8>)]) -> Result<()> {
    let originals = files
        .iter()
        .map(|(path, _)| {
            if path.is_file() {
                fs::read(path)
                    .map(Some)
                    .with_context(|| format!("read {}", path.display()))
            } else {
                Ok(None)
            }
        })
        .collect::<Result<Vec<_>>>()?;

    for (index, (path, contents)) in files.iter().enumerate() {
        if let Err(write_error) = write_atomic(path, contents) {
            let mut rollback_errors = Vec::new();
            for rollback_index in (0..index).rev() {
                let rollback_path = &files[rollback_index].0;
                let rollback = match &originals[rollback_index] {
                    Some(bytes) => write_atomic(rollback_path, bytes),
                    None => {
                        if rollback_path.exists() {
                            fs::remove_file(rollback_path).map_err(anyhow::Error::from)
                        } else {
                            Ok(())
                        }
                    }
                };
                if let Err(error) = rollback {
                    rollback_errors.push(format!("{}: {error}", rollback_path.display()));
                }
            }
            if rollback_errors.is_empty() {
                return Err(write_error).context("write file group; prior files restored");
            }
            anyhow::bail!(
                "write file group failed: {write_error}; rollback failed: {}",
                rollback_errors.join("; ")
            );
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_atomic_replaces_existing_file() {
        let dir = std::env::temp_dir().join(format!("modelhub-atomic-{}", Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        let file = dir.join("config.json");
        fs::write(&file, b"old").unwrap();

        write_atomic(&file, b"new").unwrap();

        assert_eq!(fs::read(&file).unwrap(), b"new");
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 1);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn write_atomic_group_rolls_back_prior_replacements() {
        let dir = std::env::temp_dir().join(format!("modelhub-group-{}", Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        let first = dir.join("first.json");
        let invalid_target = dir.join("target-directory");
        fs::write(&first, b"old").unwrap();
        fs::create_dir(&invalid_target).unwrap();

        let result = write_atomic_group(&[
            (first.clone(), b"new".to_vec()),
            (invalid_target, b"cannot replace a directory".to_vec()),
        ]);

        assert!(result.is_err());
        assert_eq!(fs::read(&first).unwrap(), b"old");
        fs::remove_dir_all(dir).unwrap();
    }
}
