use std::path::Path;
use std::process::Command;

use crate::paths::{AUTO_COMMIT_PREFIX, LEGACY_AUTO_COMMIT_PREFIX};
use crate::{Error, Result};

fn run(args: &[&str], cwd: &Path) -> Result<std::process::Output> {
    let output = Command::new(args[0])
        .args(&args[1..])
        .current_dir(cwd)
        .output()?;
    Ok(output)
}

fn is_auto_commit_subject(subject: &str) -> bool {
    subject.starts_with(&format!("{AUTO_COMMIT_PREFIX} "))
        || subject.starts_with(&format!("{LEGACY_AUTO_COMMIT_PREFIX} "))
}

fn has_pre_staged_changes(vault: &Path) -> bool {
    match run(&["git", "diff", "--cached", "--name-only"], vault) {
        Ok(out) => !String::from_utf8_lossy(&out.stdout).trim().is_empty(),
        Err(_) => false,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommitResult {
    Committed,
    Nothing,
    Blocked,
    Failed,
}

impl std::fmt::Display for CommitResult {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Committed => write!(f, "committed"),
            Self::Nothing => write!(f, "nothing"),
            Self::Blocked => write!(f, "blocked"),
            Self::Failed => write!(f, "failed"),
        }
    }
}

pub fn git_commit(vault: &Path, message: &str, paths: Option<&[&str]>) -> CommitResult {
    let paths = paths.unwrap_or(&["wiki/", "raw/", "vault-schema.md", ".synto/"]);
    if has_pre_staged_changes(vault) {
        tracing::warn!("git_commit: pre-staged changes detected — skipping auto-commit");
        return CommitResult::Blocked;
    }
    let mut args = vec!["git", "add"];
    args.extend(paths.iter().copied());
    if run(&args, vault).is_err() {
        return CommitResult::Failed;
    }
    match run(&["git", "status", "--porcelain"], vault) {
        Ok(out) if String::from_utf8_lossy(&out.stdout).trim().is_empty() => {
            return CommitResult::Nothing;
        }
        Err(_) => return CommitResult::Failed,
        _ => {}
    }
    let msg = format!("{AUTO_COMMIT_PREFIX} {message}");
    match run(&["git", "commit", "-m", &msg], vault) {
        Ok(out) if out.status.success() => CommitResult::Committed,
        _ => CommitResult::Failed,
    }
}

#[derive(Debug, Clone)]
pub struct AutoCommit {
    pub hash: String,
    pub message: String,
}

pub fn git_log_auto(vault: &Path, n: usize) -> Vec<AutoCommit> {
    let max = (n * 3).to_string();
    let Ok(out) = run(
        &[
            "git",
            "log",
            &format!("--max-count={max}"),
            "--oneline",
            "--format=%H %s",
        ],
        vault,
    ) else {
        return Vec::new();
    };
    if !out.status.success() {
        return Vec::new();
    }
    let mut commits = Vec::new();
    for line in String::from_utf8_lossy(&out.stdout).lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Some((hash, subject)) = line.split_once(' ') {
            if is_auto_commit_subject(subject) {
                commits.push(AutoCommit {
                    hash: hash.to_string(),
                    message: subject.to_string(),
                });
                if commits.len() >= n {
                    break;
                }
            }
        }
    }
    commits
}

pub fn git_undo(vault: &Path, steps: usize) -> Result<Vec<String>> {
    let status = run(&["git", "status", "--porcelain"], vault)?;
    let stdout = String::from_utf8_lossy(&status.stdout);
    let tracked: Vec<_> = stdout
        .lines()
        .filter(|line| !line.trim().is_empty() && !line.starts_with("??"))
        .collect();
    if !tracked.is_empty() {
        return Err(Error::DirtyWorktree);
    }
    let commits = git_log_auto(vault, steps);
    let mut reverted = Vec::new();
    for c in commits {
        let out = run(
            &[
                "git",
                "-c",
                "merge.conflictstyle=merge",
                "revert",
                "--no-edit",
                &c.hash,
            ],
            vault,
        )?;
        if !out.status.success() {
            tracing::warn!(
                "git revert failed for {}: {}",
                c.hash,
                String::from_utf8_lossy(&out.stderr)
            );
            break;
        }
        reverted.push(c.message);
    }
    Ok(reverted)
}

pub fn git_init(vault: &Path) -> Result<()> {
    if !vault.join(".git").exists() {
        let out = run(&["git", "init"], vault)?;
        if !out.status.success() {
            return Err(Error::Git(String::from_utf8_lossy(&out.stderr).into()));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auto_commit_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        git_init(dir.path()).unwrap();
        run(
            &["git", "config", "user.email", "synto@example.com"],
            dir.path(),
        )
        .unwrap();
        run(&["git", "config", "user.name", "synto"], dir.path()).unwrap();
        std::fs::write(dir.path().join("wiki.md"), "hi\n").unwrap();
        std::fs::create_dir_all(dir.path().join("wiki")).unwrap();
        std::fs::write(dir.path().join("wiki/Qubit.md"), "body\n").unwrap();
        assert_eq!(
            git_commit(dir.path(), "test commit", Some(&["wiki/"])),
            CommitResult::Committed
        );
        let log = git_log_auto(dir.path(), 5);
        assert_eq!(log.len(), 1);
        assert!(log[0].message.contains("[synto] test commit"));
    }
}
