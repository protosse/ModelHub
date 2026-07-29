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
}
