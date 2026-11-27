//! Account schemas and HTTP behavior for the second batch; no real credentials.
use serde_json::json;
use std::{
    collections::BTreeMap,
    fs,
    io::{Read, Write},
    net::TcpListener,
    os::unix::fs::PermissionsExt,
    thread,
    time::Duration,
};
use token_manager::{
    provider::{Provider, RequestMethod, ResetFormat, providers},
    state::{Account, Credential},
    status,
};
fn catalog() -> BTreeMap<String, Provider> {
    let tmp = tempfile::tempdir().unwrap();
    providers(tmp.path(), false).unwrap()
}
fn subscription() -> serde_json::Value {
    json!({"active":true,"state":"active","limits":{"dailyInputTokens":1000,"weeklyInputTokens":5000,"dailyImages":10},
        "dailyInputTokens":{"used":250,"remaining":750,"percentUsed":0.25,"resetAt":1794861444623_i64},
        "weeklyInputTokens":{"used":1000,"remaining":4000,"percentUsed":0.2,"resetAt":1794861444623_i64},
        "dailyImages":{"used":2,"remaining":8,"percentUsed":0.2,"resetAt":1794861444623_i64}})
}
#[test]
fn new_services_are_embedded_with_distinct_account_contracts() {
    let entries = catalog();
    assert!(entries.len() >= 12); // Additional provider adapters may be embedded.
    for name in ["chutes", "nanogpt", "nanogpt-subscription"] {
        entries[name].validate(false).unwrap();
    }
    assert!(entries["nanogpt"].method == RequestMethod::Post);
    assert!(entries["nanogpt-subscription"].method == RequestMethod::Get);
    assert_eq!(entries["nanogpt"].auth.header, "x-api-key");
    assert_eq!(entries["nanogpt"].auth.prefix, "");
    assert_ne!(
        entries["nanogpt"].fingerprint(),
        entries["nanogpt-subscription"].fingerprint()
    );
}
#[test]
fn nanogpt_wallet_preserves_usd_precision_and_does_not_convert_nano() {
    let entries = catalog();
    let data=entries["nanogpt"].extract(&json!({"usd_balance":"3.123456789","nano_balance":"900.5","nanoDepositAddress":"synthetic-only-address"})).unwrap();
    assert_eq!(data.balances.len(), 1);
    assert_eq!(data.balances[0].currency, "USD");
    assert_eq!(data.balances[0].remaining, "3.123456789");
    assert!(data.quotas.is_empty());
    assert!(
        entries["nanogpt"]
            .extract(&json!({"nano_balance":"9"}))
            .is_err()
    );
}
#[test]
fn subscription_uses_counts_not_fractional_percent_and_normalizes_epoch_milliseconds() {
    let data = catalog()["nanogpt-subscription"]
        .extract(&subscription())
        .unwrap();
    assert!(data.balances.is_empty());
    assert_eq!(data.quotas.len(), 3);
    assert_eq!(data.quotas[0].used_percent.as_deref(), Some("25"));
    assert_eq!(data.quotas[0].used.as_deref(), Some("250"));
    assert_eq!(data.quotas[0].limit.as_deref(), Some("1000"));
    assert_eq!(
        data.quotas[0].reset_at.as_deref(),
        Some("2026-11-16T20:37:24.623Z")
    );
    assert_eq!(data.quotas[2].unit.as_deref(), Some("images"));
}
#[test]
fn degraded_usage_cannot_turn_into_zero_percent_or_keep_a_trusted_reset() {
    let mut payload = subscription();
    payload["dailyInputTokens"]["used"] = json!(0);
    payload["dailyInputTokens"]["degraded"] = json!(true);
    let data = catalog()["nanogpt-subscription"].extract(&payload).unwrap();
    let quota = &data.quotas[0];
    assert!(quota.used.is_none() && quota.used_percent.is_none() && quota.reset_at.is_none());
    assert_eq!(quota.limit.as_deref(), Some("1000"));
    payload["dailyInputTokens"]["degraded"] = json!("true");
    assert!(catalog()["nanogpt-subscription"].extract(&payload).is_err());
}
#[test]
fn timestamps_are_explicit_validated_and_old_text_formats_are_unchanged() {
    let mut provider = catalog()["nanogpt-subscription"].clone();
    let mut payload = subscription();
    payload["dailyInputTokens"]["resetAt"] = json!(0);
    assert_eq!(
        provider.extract(&payload).unwrap().quotas[0]
            .reset_at
            .as_deref(),
        Some("1970-01-01T00:00:00.000Z")
    );
    provider.quotas[0].reset_format = ResetFormat::UnixSeconds;
    payload["dailyInputTokens"]["resetAt"] = json!("1");
    assert_eq!(
        provider.extract(&payload).unwrap().quotas[0]
            .reset_at
            .as_deref(),
        Some("1970-01-01T00:00:01.000Z")
    );
    for invalid in [json!(-1), json!(1.5), json!("not-a-time"), json!(i64::MAX)] {
        payload["dailyInputTokens"]["resetAt"] = invalid;
        assert!(provider.extract(&payload).is_err());
    }
    let go = catalog()["opencode-go"].clone();
    let data = go
        .extract(&json!({"usage":{"rolling":{"percent":2,"resetsAt":"2026-12-01T00:00:00Z"}}}))
        .unwrap();
    assert_eq!(
        data.quotas[0].reset_at.as_deref(),
        Some("2026-12-01T00:00:00Z")
    );
}
#[test]
fn chutes_null_balance_is_not_zero_and_profile_fields_do_not_escape() {
    let provider = catalog()["chutes"].clone();
    assert_eq!(provider.path, "/users/me");
    let data=provider.extract(&json!({"balance":7.125,"username":"fixture-name","payment_address":"fixture-private-address"})).unwrap();
    assert_eq!(data.balances[0].remaining, "7.125");
    assert!(!serde_json::to_string(&data).unwrap().contains("fixture-"));
    assert!(provider.extract(&json!({"balance":null})).is_err());
    assert!(provider.extract(&json!({"error":"forbidden"})).is_err());
}
#[test]
fn wallet_http_is_an_empty_account_post_with_the_configured_auth_header() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let thread = thread::spawn(move || {
        let (mut socket, _) = listener.accept().unwrap();
        socket
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        let mut request = Vec::new();
        let mut buffer = [0; 1024];
        while !request.windows(4).any(|w| w == b"\r\n\r\n") {
            let n = socket.read(&mut buffer).unwrap();
            assert!(n > 0);
            request.extend_from_slice(&buffer[..n]);
        }
        let request = String::from_utf8(request).unwrap().to_ascii_lowercase();
        assert!(request.starts_with("post /api/check-balance http/1.1\r\n"));
        assert!(request.contains("x-api-key: fixture-only-key"));
        assert!(
            !request.contains("authorization:")
                && !request.contains("messages")
                && !request.contains("model")
        );
        assert!(request.ends_with("\r\n\r\n"));
        let body = r#"{"usd_balance":"5.5"}"#;
        write!(
            socket,
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        )
        .unwrap();
    });
    let tmp = tempfile::tempdir().unwrap();
    let key = tmp.path().join("key");
    fs::write(&key, "fixture-only-key").unwrap();
    fs::set_permissions(&key, fs::Permissions::from_mode(0o600)).unwrap();
    let mut provider = catalog()["nanogpt"].clone();
    provider.base_url = url;
    provider.validate(true).unwrap();
    let account = Account {
        name: "wallet".into(),
        provider: provider.id.clone(),
        provider_fingerprint: provider.fingerprint(),
        credential: Credential::File(key),
    };
    let rows = status::collect(
        &[account],
        &BTreeMap::from([(provider.id.clone(), provider)]),
        &tmp.path().join("cache"),
        false,
        true,
        true,
    )
    .unwrap();
    assert_eq!(rows[0].state, "ready");
    assert_eq!(rows[0].data.as_ref().unwrap().balances[0].remaining, "5.5");
    thread.join().unwrap();
}
#[test]
fn mutation_methods_are_not_supported_by_the_provider_schema() {
    let provider = catalog()["nanogpt"].clone();
    let mut value = serde_json::to_value(provider).unwrap();
    for method in ["DELETE", "PATCH", "PUT"] {
        value["method"] = json!(method);
        assert!(serde_json::from_value::<Provider>(value.clone()).is_err());
    }
}
