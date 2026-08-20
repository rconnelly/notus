use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::Path;

use crate::paths::effective_app_dir;
use crate::Result;

const SYNC_DIRS: &[&str] = &["Dropbox", "OneDrive", "iCloud Drive", "Google Drive"];

fn lock_path(vault: &Path) -> std::path::PathBuf {
    effective_app_dir(vault).join("pipeline.lock")
}

fn warn_if_synced(vault: &Path) {
    for component in vault.components() {
        if let Some(s) = component.as_os_str().to_str() {
            if SYNC_DIRS.contains(&s) {
                tracing::warn!(
                    "Vault is inside '{s}' — pipeline lock may be unreliable on synced filesystems."
                );
                break;
            }
        }
    }
}

/// Acquire an exclusive pipeline lock. Returns the held file (keep it alive) plus whether acquired.
pub fn try_pipeline_lock(vault: &Path) -> Result<Option<File>> {
    warn_if_synced(vault);
    let path = lock_path(vault);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut file = OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .truncate(false)
        .open(&path)?;
    match fs2::FileExt::try_lock_exclusive(&file) {
        Ok(()) => {
            file.set_len(0)?;
            write!(file, "{}", std::process::id())?;
            file.flush()?;
            Ok(Some(file))
        }
        Err(_) => Ok(None),
    }
}

pub fn lock_holder_pid(vault: &Path) -> Option<u32> {
    let path = lock_path(vault);
    if !path.exists() {
        return None;
    }
    let mut contents = String::new();
    let Ok(mut f) = File::open(&path) else {
        return None;
    };
    if f.read_to_string(&mut contents).is_err() {
        return None;
    }
    let pid: u32 = contents.trim().parse().ok()?;
    match fs2::FileExt::try_lock_shared(&f) {
        Ok(()) => {
            let _ = fs2::FileExt::unlock(&f);
            None
        }
        Err(_) => Some(pid),
    }
}

pub fn has_invalid_lock_file(vault: &Path) -> bool {
    let path = lock_path(vault);
    if !path.exists() {
        return false;
    }
    match fs::read_to_string(&path) {
        Ok(s) => s.trim().parse::<u32>().is_err(),
        Err(_) => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exclusive_lock_blocks_second_holder() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".notus")).unwrap();
        let first = try_pipeline_lock(dir.path()).unwrap();
        assert!(first.is_some());
        let second = try_pipeline_lock(dir.path()).unwrap();
        assert!(second.is_none());
        drop(first);
        let third = try_pipeline_lock(dir.path()).unwrap();
        assert!(third.is_some());
    }

    #[test]
    fn invalid_lock_file_is_detected() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".notus")).unwrap();
        std::fs::write(dir.path().join(".notus/pipeline.lock"), "not-a-pid").unwrap();
        assert!(has_invalid_lock_file(dir.path()));
    }
}
