use std::path::{Path, PathBuf};

pub const APP_NAME: &str = "notus";
pub const APP_DISPLAY_NAME: &str = "Notus";
pub const CLI_NAME: &str = "notus";
pub const PACKAGE_NAME: &str = "notus";
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

pub const CONFIG_FILE_NAME: &str = "notus.toml";
pub const SYNTO_CONFIG_FILE_NAME: &str = "synto.toml";
pub const LEGACY_CONFIG_FILE_NAME: &str = "wiki.toml";
pub const APP_DIR_NAME: &str = ".notus";
pub const SYNTO_APP_DIR_NAME: &str = ".synto";
pub const LEGACY_APP_DIR_NAME: &str = ".olw";
pub const VAULT_ENV_VAR: &str = "NOTUS_VAULT";
pub const LEGACY_VAULT_ENV_VAR: &str = "SYNTO_VAULT";
pub const API_KEY_ENV_VAR: &str = "NOTUS_API_KEY";
pub const LEGACY_API_KEY_ENV_VAR: &str = "SYNTO_API_KEY";
pub const AUTO_COMMIT_PREFIX: &str = "[notus]";
pub const SYNTO_AUTO_COMMIT_PREFIX: &str = "[synto]";
pub const LEGACY_AUTO_COMMIT_PREFIX: &str = "[olw]";

pub const PROJECT_REPO_URL: &str = "https://github.com/rconnelly/notus";
pub const PROJECT_ISSUES_URL: &str = "https://github.com/rconnelly/notus/issues";
pub const PROJECT_DISCUSSIONS_URL: &str = "https://github.com/rconnelly/notus/discussions";

pub fn to_posix(path: &str) -> String {
    path.replace('\\', "/")
}

/// Resolve `path` to a vault-relative POSIX string.
///
/// `Config::from_vault` canonicalizes the vault root. Callers often pass the
/// original path (`/var/...` vs `/private/var/...` on macOS, or without the
/// Windows `\\?\` prefix). Strip-prefix on the raw strings then fails and
/// ingest used to store an absolute path in `raw_notes`.
pub fn rel_posix(path: &Path, base: &Path) -> crate::Result<String> {
    if let Some(rel) = strip_rel(path, base) {
        return Ok(rel);
    }
    let path_c = canonicalize_for_compare(path);
    let base_c = canonicalize_for_compare(base);
    if let Some(rel) = strip_rel(&path_c, &base_c) {
        return Ok(rel);
    }
    if !path.is_absolute() {
        if let Some(rel) = strip_rel(&canonicalize_for_compare(&base.join(path)), &base_c) {
            return Ok(rel);
        }
    }
    Err(crate::Error::msg(format!(
        "{} is not relative to {}",
        path.display(),
        base.display()
    )))
}

fn strip_rel(path: &Path, base: &Path) -> Option<String> {
    path.strip_prefix(base)
        .ok()
        .filter(|rel| !rel.as_os_str().is_empty() || path == base)
        .map(|rel| rel.to_string_lossy().replace('\\', "/"))
}

fn canonicalize_for_compare(path: &Path) -> PathBuf {
    let resolved = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    strip_windows_verbatim(resolved)
}

fn strip_windows_verbatim(path: PathBuf) -> PathBuf {
    #[cfg(windows)]
    {
        use std::path::{Component, Prefix};
        let mut components = path.components();
        match components.next() {
            Some(Component::Prefix(prefix)) => match prefix.kind() {
                Prefix::VerbatimDisk(disk) => {
                    PathBuf::from(format!("{}:\\", disk as char)).join(components.as_path())
                }
                Prefix::VerbatimUNC(server, share) => PathBuf::from(format!(
                    r"\\{}\{}",
                    server.to_string_lossy(),
                    share.to_string_lossy()
                ))
                .join(components.as_path()),
                _ => path,
            },
            _ => path,
        }
    }
    #[cfg(not(windows))]
    {
        path
    }
}

pub fn config_path(vault: &Path) -> PathBuf {
    vault.join(CONFIG_FILE_NAME)
}

pub fn synto_config_path(vault: &Path) -> PathBuf {
    vault.join(SYNTO_CONFIG_FILE_NAME)
}

pub fn legacy_config_path(vault: &Path) -> PathBuf {
    vault.join(LEGACY_CONFIG_FILE_NAME)
}

pub fn app_dir(vault: &Path) -> PathBuf {
    vault.join(APP_DIR_NAME)
}

pub fn synto_app_dir(vault: &Path) -> PathBuf {
    vault.join(SYNTO_APP_DIR_NAME)
}

pub fn legacy_app_dir(vault: &Path) -> PathBuf {
    vault.join(LEGACY_APP_DIR_NAME)
}

pub fn effective_config_path(vault: &Path) -> PathBuf {
    for candidate in [
        config_path(vault),
        synto_config_path(vault),
        legacy_config_path(vault),
    ] {
        if candidate.exists() {
            return candidate;
        }
    }
    config_path(vault)
}

pub fn effective_app_dir(vault: &Path) -> PathBuf {
    for candidate in [app_dir(vault), synto_app_dir(vault), legacy_app_dir(vault)] {
        if candidate.exists() {
            return candidate;
        }
    }
    app_dir(vault)
}

pub fn has_vault_config(vault: &Path) -> bool {
    config_path(vault).exists()
        || synto_config_path(vault).exists()
        || legacy_config_path(vault).exists()
}

pub fn is_legacy_vault(vault: &Path) -> bool {
    legacy_config_path(vault).exists()
        && !config_path(vault).exists()
        && !synto_config_path(vault).exists()
}

pub fn migration_message(vault: &Path) -> String {
    let resolved = vault.canonicalize().unwrap_or_else(|_| vault.to_path_buf());
    format!(
        "This looks like an obsidian-llm-wiki vault: {}\nRun `{CLI_NAME} migrate-olw --vault {}` first.",
        resolved.display(),
        resolved.display()
    )
}

pub fn is_within(path: &Path, root: &Path) -> bool {
    let path = match path.canonicalize() {
        Ok(p) => p,
        Err(_) => return false,
    };
    let root = match root.canonicalize() {
        Ok(p) => p,
        Err(_) => return false,
    };
    path == root || path.starts_with(&root)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn posix_normalizes_backslashes() {
        assert_eq!(to_posix(r"raw\note.md"), "raw/note.md");
        assert_eq!(to_posix("raw/note.md"), "raw/note.md");
    }

    #[test]
    fn rel_posix_plain() {
        let dir = tempfile::tempdir().unwrap();
        let note = dir.path().join("raw").join("note.md");
        std::fs::create_dir_all(note.parent().unwrap()).unwrap();
        std::fs::write(&note, "x").unwrap();
        assert_eq!(rel_posix(&note, dir.path()).unwrap(), "raw/note.md");
    }

    #[test]
    fn rel_posix_when_base_is_canonicalized() {
        let dir = tempfile::tempdir().unwrap();
        let note = dir.path().join("raw").join("note.md");
        std::fs::create_dir_all(note.parent().unwrap()).unwrap();
        std::fs::write(&note, "x").unwrap();
        let base = std::fs::canonicalize(dir.path()).unwrap();
        assert_eq!(rel_posix(&note, &base).unwrap(), "raw/note.md");
    }

    #[cfg(unix)]
    #[test]
    fn rel_posix_resolves_symlink_prefix() {
        let dir = tempfile::tempdir().unwrap();
        let real = dir.path().join("real");
        std::fs::create_dir(&real).unwrap();
        let note_dir = real.join("raw");
        std::fs::create_dir(&note_dir).unwrap();
        let note = note_dir.join("note.md");
        std::fs::write(&note, "x").unwrap();
        let link = dir.path().join("link");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        let via_link = link.join("raw").join("note.md");
        let canon_base = std::fs::canonicalize(&real).unwrap();
        assert_eq!(rel_posix(&via_link, &canon_base).unwrap(), "raw/note.md");
        assert_eq!(rel_posix(&via_link, &real).unwrap(), "raw/note.md");
    }

    #[test]
    fn effective_config_prefers_notus_then_synto() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(
            effective_config_path(dir.path()),
            dir.path().join("notus.toml")
        );
        std::fs::write(dir.path().join("synto.toml"), "").unwrap();
        assert_eq!(
            effective_config_path(dir.path()),
            dir.path().join("synto.toml")
        );
        std::fs::write(dir.path().join("notus.toml"), "").unwrap();
        assert_eq!(
            effective_config_path(dir.path()),
            dir.path().join("notus.toml")
        );
    }
}
