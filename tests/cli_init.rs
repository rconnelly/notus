use std::process::Command;

#[test]
fn help_lists_core_commands() {
    let bin = env!("CARGO_BIN_EXE_notus");
    let out = Command::new(bin)
        .arg("--help")
        .output()
        .expect("run notus --help");
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    for cmd in [
        "init", "ingest", "compile", "approve", "query", "run", "serve",
    ] {
        assert!(stdout.contains(cmd), "missing {cmd} in --help:\n{stdout}");
    }
}

#[test]
fn init_creates_vault_layout() {
    let tmp = tempfile::tempdir().unwrap();
    let xdg = tmp.path().join("xdg");
    let vault = tmp.path().join("wiki");
    let bin = env!("CARGO_BIN_EXE_notus");
    let out = Command::new(bin)
        .args(["init", vault.to_str().unwrap(), "--non-interactive"])
        .env("XDG_CONFIG_HOME", &xdg)
        .env("HOME", tmp.path())
        .output()
        .expect("run notus init");
    assert!(
        out.status.success(),
        "init failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(vault.join("raw").is_dir());
    assert!(vault.join("wiki/.drafts").is_dir());
    assert!(vault.join("notus.toml").is_file());
    assert!(vault.join("wiki/index.md").is_file());
    assert!(vault.join(".git").exists());
}

#[test]
fn status_on_fresh_vault() {
    let tmp = tempfile::tempdir().unwrap();
    let xdg = tmp.path().join("xdg");
    let vault = tmp.path().join("wiki");
    let bin = env!("CARGO_BIN_EXE_notus");
    let init = Command::new(bin)
        .args(["init", vault.to_str().unwrap(), "--non-interactive"])
        .env("XDG_CONFIG_HOME", &xdg)
        .env("HOME", tmp.path())
        .output()
        .unwrap();
    assert!(init.status.success());
    let status = Command::new(bin)
        .args(["--vault", vault.to_str().unwrap(), "status"])
        .env("XDG_CONFIG_HOME", &xdg)
        .env("HOME", tmp.path())
        .output()
        .unwrap();
    assert!(
        status.status.success(),
        "status failed: {}",
        String::from_utf8_lossy(&status.stderr)
    );
}
