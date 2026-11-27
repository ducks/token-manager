use std::process::Command;
use token_manager::provider::Provider;
#[test]
fn google_accounts_are_separate_credential_free_and_explicitly_unsupported() {
    let api: Provider = toml::from_str(include_str!("../providers/gemini.toml")).unwrap();
    let chat: Provider =
        toml::from_str(include_str!("../providers/gemini-subscription.toml")).unwrap();
    api.validate(false).unwrap();
    chat.validate(false).unwrap();
    assert_ne!(api.fingerprint(), chat.fingerprint());
    let root = tempfile::tempdir().unwrap();
    for id in ["gemini", "gemini-subscription"] {
        let output = Command::new(env!("CARGO_BIN_EXE_token-manager"))
            .arg("--config-dir")
            .arg(root.path())
            .args(["add", id])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let output = Command::new(env!("CARGO_BIN_EXE_token-manager"))
        .arg("--config-dir")
        .arg(root.path())
        .args(["status", "--json"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    let rows: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(rows.as_array().unwrap().len(), 2);
    for row in rows.as_array().unwrap() {
        assert_eq!(row["state"], "unsupported");
        assert!(row["data"].is_null());
    }
}
