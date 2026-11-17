use std::{fs, os::unix::fs::PermissionsExt, process::Command};
fn bin(root: &std::path::Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_token-manager"));
    command.arg("--config-dir").arg(root);
    command
}
#[test]
fn actual_cli_add_status_login_and_provider_catalog_keep_secrets_out_of_state() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("config");
    let key = tmp.path().join("key");
    fs::write(&key, "cli-fixture-secret").unwrap();
    fs::set_permissions(&key, fs::Permissions::from_mode(0o600)).unwrap();
    let output = bin(&root)
        .args(["add", "vercel", "--name", "work", "--key-file"])
        .arg(&key)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let config = fs::read_to_string(root.join("accounts.json")).unwrap();
    assert!(!config.contains("cli-fixture-secret"));
    assert_eq!(
        fs::metadata(root.join("accounts.json"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
    assert!(
        !bin(&root)
            .args(["add", "vercel", "--name", "work", "--key-file"])
            .arg(&key)
            .status()
            .unwrap()
            .success()
    );
    let output = bin(&root)
        .args(["status", "work", "--cached", "--json"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    let rows: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(rows[0]["state"], "unknown");
    assert!(rows[0]["data"].is_null());
    assert!(
        bin(&root)
            .args(["login", "work", "--key-file"])
            .arg(&key)
            .status()
            .unwrap()
            .success()
    );
    assert!(
        bin(&root)
            .args(["add", "opencode-zen", "--name", "zen-wallet"])
            .status()
            .unwrap()
            .success()
    );
    let output = bin(&root)
        .args(["status", "zen-wallet", "--json"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stdout).contains("unsupported"));
    let output = bin(&root).args(["providers", "--json"]).output().unwrap();
    assert!(output.status.success());
    let providers: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(
        providers
            .as_array()
            .unwrap()
            .iter()
            .any(|p| p["id"] == "vercel")
    );
    assert!(
        providers
            .as_array()
            .unwrap()
            .iter()
            .any(|p| p["id"] == "opencode-go")
    );
    assert!(!String::from_utf8_lossy(&output.stdout).contains("cli-fixture-secret"));
}
