use serde_json::json;
use std::{
    io::{Read, Write},
    net::TcpListener,
    process::Command,
};
use token_manager::{
    prices::{self, Sort},
    provider::Provider,
};
fn provider() -> Provider {
    toml::from_str(include_str!("../providers/openrouter.toml")).unwrap()
}
fn fixture() -> serde_json::Value {
    json!({"data":[
        {"id":"test/claude-small","name":"Claude Small","pricing":{"prompt":"0.000001","completion":"0.000002"}},
        {"id":"test/claude-big","name":"Claude Big","pricing":{"prompt":"0.0000001","completion":"0.000010"}},
        {"id":"test/claude-free","name":"Claude Free","pricing":{"prompt":"0","completion":"0"}},
        {"id":"test/claude-unknown","name":"Claude Unknown","pricing":{"prompt":"-1","completion":null}},
        {"id":"other/gemini","name":"Gemini","pricing":{"prompt":"1","completion":"1"}}
    ]})
}
#[test]
fn family_sort_exact_model_and_unknown_rates() {
    let p = provider();
    let map = p.pricing.unwrap();
    let rows = prices::extract(&fixture(), &map, "CLAUDE", Sort::Output).unwrap();
    assert_eq!(
        rows.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(),
        vec![
            "test/claude-big",
            "test/claude-small",
            "test/claude-free",
            "test/claude-unknown"
        ]
    );
    assert_eq!(rows[0].output_per_million.as_deref(), Some("10"));
    assert_eq!(rows[2].input_per_million.as_deref(), Some("0"));
    assert!(rows[3].input_per_million.is_none());
    let rows = prices::extract(&fixture(), &map, "claude", Sort::Input).unwrap();
    assert_eq!(rows[0].id, "test/claude-small");
    let rows = prices::extract(&fixture(), &map, "Claude Big", Sort::Output).unwrap();
    assert_eq!(rows.len(), 1);
    assert!(prices::extract(&fixture(), &map, "missing", Sort::Output).is_err());
    assert!(prices::extract(&fixture(), &map, "---", Sort::Output).is_err());
    assert!(prices::extract(&json!({}), &map, "claude", Sort::Output).is_err());
    let bad = json!({"data":[{"id":"test/bad", "name":"Bad", "pricing":{"prompt":"oops","completion":"99999999999999999999999999999"}}]});
    let rows = prices::extract(&bad, &map, "bad", Sort::Output).unwrap();
    assert!(rows[0].input_per_million.is_none());
    assert!(rows[0].output_per_million.is_none());
}
#[test]
fn public_catalog_cli_needs_no_accounts_and_sends_no_auth() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(std::time::Duration::from_secs(5)))
            .unwrap();
        let mut request = Vec::new();
        let mut byte = [0];
        while !request.ends_with(b"\r\n\r\n") {
            stream.read_exact(&mut byte).unwrap();
            request.push(byte[0]);
        }
        let request = String::from_utf8(request).unwrap().to_lowercase();
        assert!(request.starts_with("get /api/v1/models "));
        assert!(!request.contains("authorization"));
        assert!(!request.contains("x-api-key"));
        let body = fixture().to_string();
        write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        )
        .unwrap();
    });
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    std::fs::create_dir(root.join("providers")).unwrap();
    let definition = include_str!("../providers/openrouter.toml")
        .replace("https://openrouter.ai\"", &format!("http://{address}\""));
    std::fs::write(root.join("providers/openrouter.toml"), definition).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_token-manager"))
        .arg("--config-dir")
        .arg(root)
        .args([
            "--allow-local-http",
            "prices",
            "claude",
            "--source",
            "openrouter",
            "--json",
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["currency"], "USD");
    assert_eq!(report["models"][0]["id"], "test/claude-big");
    assert!(!root.join("accounts.json").exists());
    server.join().unwrap();
}
#[test]
fn catalog_urls_are_validated_and_account_binding_is_unchanged() {
    let mut p = provider();
    let fingerprint = p.fingerprint();
    p.pricing.as_mut().unwrap().path = "//elsewhere.example/models".into();
    assert!(p.validate(false).is_err());
    p.pricing = None;
    assert_eq!(fingerprint, p.fingerprint());
}
