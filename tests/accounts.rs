use std::{
    collections::BTreeMap,
    fs,
    io::{Read, Write},
    net::TcpListener,
    os::unix::fs::PermissionsExt,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    thread,
    time::Duration,
};
use tempfile::TempDir;
use token_manager::{
    provider::{self, Provider},
    state::{self, Account, Credential},
    status,
};

fn server(
    responses: Vec<(u16, &'static str, Option<&'static str>)>,
) -> (String, thread::JoinHandle<()>, Arc<AtomicUsize>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let requests = Arc::new(AtomicUsize::new(0));
    let count = requests.clone();
    let handle = thread::spawn(move || {
        for (code, body, location) in responses {
            let (mut socket, _) = listener.accept().unwrap();
            socket
                .set_read_timeout(Some(Duration::from_secs(3)))
                .unwrap();
            let mut request = Vec::new();
            let mut buffer = [0; 1024];
            while !request.windows(4).any(|b| b == b"\r\n\r\n") {
                let read = socket.read(&mut buffer).unwrap();
                if read == 0 {
                    break;
                }
                request.extend_from_slice(&buffer[..read]);
            }
            let request = String::from_utf8(request).unwrap();
            assert!(request.starts_with("GET /credits HTTP/1.1"));
            assert!(
                request
                    .to_ascii_lowercase()
                    .contains("authorization: bearer fixture-secret")
            );
            count.fetch_add(1, Ordering::Relaxed);
            write!(socket,"HTTP/1.1 {code} Test\r\nContent-Length: {}\r\nContent-Type: application/json\r\nConnection: close\r\n{}\r\n{body}",body.len(),location.map(|url|format!("Location: {url}\r\n")).unwrap_or_default()).unwrap();
        }
    });
    (url, handle, requests)
}
fn fixture(url: String) -> (TempDir, Provider, Account) {
    let root = tempfile::tempdir().unwrap();
    let secret = root.path().join("key");
    fs::write(&secret, "fixture-secret").unwrap();
    fs::set_permissions(&secret, fs::Permissions::from_mode(0o600)).unwrap();
    let mut provider: Provider = toml::from_str(include_str!("../providers/vercel.toml")).unwrap();
    provider.base_url = url;
    provider.path = "/credits".into();
    provider.id = "fixture".into();
    provider.validate(true).unwrap();
    let account = Account {
        name: "personal".into(),
        provider: "fixture".into(),
        provider_fingerprint: provider.fingerprint(),
        credential: Credential::File(secret),
    };
    (root, provider, account)
}
#[test]
fn actual_http_auth_decimal_balance_and_cache() {
    let (url, handle, count) = server(vec![(
        200,
        r#"{"balance":"12.340000000000000001","total_used":"0.01"}"#,
        None,
    )]);
    let (root, provider, account) = fixture(url);
    let providers = BTreeMap::from([(provider.id.clone(), provider)]);
    let cache = root.path().join("cache");
    let first = status::collect(
        std::slice::from_ref(&account),
        &providers,
        &cache,
        false,
        false,
        true,
    )
    .unwrap();
    assert_eq!(first[0].state, "ready");
    assert_eq!(
        first[0].data.as_ref().unwrap().balances[0].remaining,
        "12.340000000000000001"
    );
    handle.join().unwrap();
    fs::remove_file(match &account.credential {
        Credential::File(p) => p,
        _ => unreachable!(),
    })
    .unwrap();
    let cached = status::collect(&[account], &providers, &cache, false, false, true).unwrap();
    assert_eq!(cached[0].state, "cached");
    assert_eq!(cached[0].data, first[0].data);
    assert_eq!(count.load(Ordering::Relaxed), 1);
    assert_eq!(
        fs::metadata(cache.join("personal.json"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
    assert!(
        !fs::read_to_string(cache.join("personal.json"))
            .unwrap()
            .contains("fixture-secret")
    );
}
#[test]
fn failed_refresh_preserves_explicitly_stale_data_without_echoing_body() {
    let (url, handle, _) = server(vec![
        (200, r#"{"balance":"4","total_used":"2"}"#, None),
        (403, "fixture-secret should never be printed", None),
    ]);
    let (root, provider, account) = fixture(url);
    let providers = BTreeMap::from([(provider.id.clone(), provider)]);
    status::collect(
        std::slice::from_ref(&account),
        &providers,
        &root.path().join("cache"),
        false,
        true,
        true,
    )
    .unwrap();
    let rows = status::collect(
        &[account],
        &providers,
        &root.path().join("cache"),
        false,
        true,
        true,
    )
    .unwrap();
    assert_eq!(rows[0].state, "permission_denied");
    assert!(rows[0].cached && rows[0].stale);
    assert_eq!(rows[0].data.as_ref().unwrap().balances[0].remaining, "4");
    assert!(
        !serde_json::to_string(&rows)
            .unwrap()
            .contains("fixture-secret")
    );
    handle.join().unwrap();
}
#[test]
fn credentials_are_not_sent_after_provider_definition_changes() {
    let (root, mut provider, account) = fixture("http://127.0.0.1:1".into());
    provider.base_url = "http://127.0.0.1:2".into();
    let rows = status::collect(
        &[account],
        &BTreeMap::from([(provider.id.clone(), provider)]),
        &root.path().join("cache"),
        false,
        true,
        true,
    )
    .unwrap();
    assert_eq!(rows[0].state, "login_required");
    assert!(
        rows[0]
            .detail
            .as_ref()
            .unwrap()
            .contains("definition changed")
    );
}
#[test]
fn http_errors_missing_fields_and_redirects_do_not_become_zero_balance() {
    for (code, body, location, expected) in [
        (401, "fixture-secret", None, "login_required"),
        (429, "fixture-secret", None, "rate_limited"),
        (500, "fixture-secret", None, "provider_error"),
        (200, "{}", None, "response_error"),
        (200, "not json", None, "response_error"),
        (
            302,
            "",
            Some("https://example.invalid/steal"),
            "provider_error",
        ),
    ] {
        let (url, handle, _) = server(vec![(code, body, location)]);
        let (root, provider, account) = fixture(url);
        let rows = status::collect(
            &[account],
            &BTreeMap::from([(provider.id.clone(), provider)]),
            &root.path().join("cache"),
            false,
            true,
            true,
        )
        .unwrap();
        assert_eq!(rows[0].state, expected);
        assert!(rows[0].data.is_none());
        assert!(!status::table(&rows).contains("fixture-secret"));
        handle.join().unwrap();
    }
}
#[test]
fn openrouter_calculates_remaining_and_deepseek_preserves_currencies() {
    let openrouter: Provider =
        toml::from_str(include_str!("../providers/openrouter.toml")).unwrap();
    let data = openrouter
        .extract(&serde_json::json!({"data":{"total_credits":"10.01","total_usage":"0.02"}}))
        .unwrap();
    assert_eq!(data.balances[0].remaining, "9.99");
    let deepseek: Provider = toml::from_str(include_str!("../providers/deepseek.toml")).unwrap();
    let data = deepseek.extract(&serde_json::json!({"balance_infos":[{"currency":"USD","total_balance":"2"},{"currency":"CNY","total_balance":"10"}]})).unwrap();
    assert_eq!(data.balances.len(), 2);
    assert_eq!(data.balances[1].currency, "CNY");
}
#[test]
fn go_and_zen_are_distinct_and_missing_go_windows_remain_unknown() {
    let root = tempfile::tempdir().unwrap();
    let providers = provider::providers(root.path(), false).unwrap();
    assert!(providers.contains_key("opencode-go") && providers.contains_key("opencode-zen"));
    let data = providers["opencode-go"].extract(&serde_json::json!({"usage":{"rolling":{"percent":23,"resetsAt":"2026-11-17T01:00:00Z"}}})).unwrap();
    assert!(data.balances.is_empty());
    assert_eq!(data.quotas[0].used_percent.as_deref(), Some("23"));
    assert_eq!(data.quotas[1].used_percent, None);
    let account = Account {
        name: "zen-wallet".into(),
        provider: "opencode-zen".into(),
        provider_fingerprint: providers["opencode-zen"].fingerprint(),
        credential: Credential::None,
    };
    let rows = status::collect(
        &[account],
        &providers,
        &root.path().join("cache"),
        false,
        true,
        false,
    )
    .unwrap();
    assert_eq!(rows[0].state, "unsupported");
    assert!(rows[0].data.is_none());
}
#[test]
fn private_files_locks_and_symlinks_fail_safely() {
    use std::os::unix::fs::symlink;
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("account.json");
    state::atomic_write(&path, b"private").unwrap();
    assert_eq!(state::read_private(&path).unwrap(), b"private");
    fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
    assert!(state::read_private(&path).is_err());
    let link = root.path().join("link");
    symlink(&path, &link).unwrap();
    assert!(state::atomic_write(&link, b"overwrite").is_err());
    assert_eq!(fs::read(&path).unwrap(), b"private");
    let lock = state::EditLock::acquire(root.path()).unwrap();
    assert!(state::EditLock::acquire(root.path()).is_err());
    drop(lock);
    assert!(state::EditLock::acquire(root.path()).is_ok());
}
#[test]
fn custom_provider_is_entirely_declarative_and_urls_are_restricted() {
    let root = tempfile::tempdir().unwrap();
    let custom =
        include_str!("../providers/vercel.toml").replace("id = \"vercel\"", "id = \"custom-bank\"");
    fs::write(root.path().join("custom.toml"), custom).unwrap();
    let providers = provider::providers(root.path(), false).unwrap();
    assert!(providers.contains_key("custom-bank"));
    let mut p = providers["custom-bank"].clone();
    p.base_url = "http://example.com".into();
    assert!(p.validate(true).is_err());
    p.base_url = "https://secret@example.com".into();
    assert!(p.validate(false).is_err());
    p.base_url = "https://example.com".into();
    p.path = "//other.example/credits".into();
    assert!(p.validate(false).is_err());
}
