use serde_json::json;
use std::{
    collections::BTreeMap,
    io::{Read, Write},
    net::TcpListener,
};
use token_manager::{costs::CostMap, provider::Provider};
fn mapping(nested: bool) -> CostMap {
    serde_json::from_value(json!({
        "start_parameter":"from", "end_parameter":"until", "time_format":"unix_seconds", "query":{},
        "rows":"/data", "nested_rows":if nested { Some("/results") } else { None },
        "amount":"/amount", "currency":"/currency", "divisor":100,
        "has_more":"/has_more", "next_page":"/next_page", "page_parameter":"page"
    }))
    .unwrap()
}
#[test]
fn costs_preserve_precision_unknowns_and_reject_partial_or_bad_responses() {
    for nested in [false, true] {
        let map = mapping(nested);
        map.validate().unwrap();
        let entries = json!([{"amount":"123.456789", "currency":"USD"}, {"amount":"-1.000001", "currency":"usd"}]);
        let rows = if nested {
            json!([{"results":entries}])
        } else {
            entries
        };
        let (amount, next) = map
            .page(&json!({"data":rows,"has_more":true,"next_page":"cursor"}))
            .unwrap();
        assert_eq!(amount.unwrap().to_string(), "1.22456788");
        assert_eq!(next.as_deref(), Some("cursor"));
        assert!(
            map.page(&json!({"data":[],"has_more":false}))
                .unwrap()
                .0
                .is_none()
        );
        assert!(map.page(&json!({"data":[],"has_more":true})).is_err());
        assert!(map.page(&json!({"data":[]})).is_err());
    }
    let map = mapping(false);
    for entry in [
        json!({"amount":null,"currency":"USD"}),
        json!({"amount":"1","currency":"EUR"}),
        json!({"amount":"NaN","currency":"USD"}),
        json!({"currency":"USD"}),
    ] {
        assert!(map.page(&json!({"data":[entry],"has_more":false})).is_err());
    }
    assert_eq!(
        map.page(&json!({"data":[{"amount":0,"currency":"USD"}],"has_more":false}))
            .unwrap()
            .0
            .unwrap()
            .to_string(),
        "0"
    );
}
#[test]
fn bounded_pagination_uses_encoded_cursors_and_never_returns_partial_costs() {
    for failure in [false, true] {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let mut p: Provider = toml::from_str(include_str!("../providers/vercel.toml")).unwrap();
        p.base_url = format!("http://{}", listener.local_addr().unwrap());
        p.path = "/costs".into();
        p.balance = None;
        p.costs = Some(mapping(false));
        p.headers = BTreeMap::from([("anthropic-version".into(), "2023-06-01".into())]);
        p.validate(true).unwrap();
        let server = std::thread::spawn(move || {
            for page in 0..2 {
                let (mut stream, _) = listener.accept().unwrap();
                stream
                    .set_read_timeout(Some(std::time::Duration::from_secs(5)))
                    .unwrap();
                let mut bytes = Vec::new();
                let mut byte = [0];
                while !bytes.ends_with(b"\r\n\r\n") {
                    stream.read_exact(&mut byte).unwrap();
                    bytes.push(byte[0]);
                }
                let request = String::from_utf8(bytes).unwrap();
                assert!(request.starts_with("GET /costs?from="));
                assert!(request.contains("authorization: Bearer fixture-secret\r\n"));
                assert!(request.contains("anthropic-version: 2023-06-01\r\n"));
                let path = request.split_whitespace().nth(1).unwrap();
                let url = reqwest::Url::parse(&format!("http://localhost{path}")).unwrap();
                let query: BTreeMap<_, _> = url.query_pairs().into_owned().collect();
                let start = query["from"].parse::<u64>().unwrap();
                let end = query["until"].parse::<u64>().unwrap();
                assert_eq!(end - start, 30 * 86400);
                assert_eq!(end % 86400, 0);
                if page == 1 {
                    assert_eq!(query["page"], "a&b=secret");
                }
                let body = if page == 0 {
                    json!({"data":[{"amount":"100","currency":"USD"}],"has_more":true,"next_page":"a&b=secret"}).to_string()
                } else if failure {
                    "fixture-secret raw error".into()
                } else {
                    json!({"data":[{"amount":"23.456","currency":"USD"}],"has_more":false})
                        .to_string()
                };
                let status = if page == 1 && failure {
                    "403 Forbidden"
                } else {
                    "200 OK"
                };
                write!(
                    stream,
                    "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                )
                .unwrap();
            }
        });
        let client = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(5))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .unwrap();
        let result = token_manager::costs::fetch(&client, &p, "fixture-secret", true);
        if failure {
            assert_eq!(
                result.unwrap_err(),
                ("permission_denied", "Credential lacks cost-report access")
            );
        } else {
            let data = result.unwrap();
            assert_eq!(data.costs[0].amount.as_deref(), Some("1.23456"));
            assert!(data.balances.is_empty());
        }
        server.join().unwrap();
    }
}
#[test]
fn version_headers_and_cost_parameters_are_constrained_and_legacy_shape_is_preserved() {
    let mut p: Provider = toml::from_str(include_str!("../providers/vercel.toml")).unwrap();
    let value = serde_json::to_value(&p).unwrap();
    assert!(value.get("costs").is_none());
    assert!(value.get("headers").is_none());
    p.headers.insert("Authorization".into(), "secret".into());
    assert!(p.validate(false).is_err());
    let mut map = mapping(false);
    map.query.insert("from".into(), "0".into());
    assert!(map.validate().is_err());
    map.query.clear();
    map.divisor = 0;
    assert!(map.validate().is_err());
}

#[test]
fn subscription_requests_include_version_headers_and_use_existing_cache() {
    use std::{fs, os::unix::fs::PermissionsExt};
    use token_manager::{
        state::{Account, Credential},
        status,
    };
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let mut p: Provider = toml::from_str(include_str!("../providers/opencode-go.toml")).unwrap();
    p.base_url = format!("http://{}", listener.local_addr().unwrap());
    p.headers
        .insert("example-beta".into(), "usage-version".into());
    p.validate(true).unwrap();
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(std::time::Duration::from_secs(5)))
            .unwrap();
        let mut bytes = Vec::new();
        let mut byte = [0];
        while !bytes.ends_with(b"\r\n\r\n") {
            stream.read_exact(&mut byte).unwrap();
            bytes.push(byte[0]);
        }
        let request = String::from_utf8(bytes).unwrap();
        assert!(request.contains("example-beta: usage-version\r\n"));
        let body = json!({"usage":{"rolling":{"percent":25,"resetsAt":"2026-12-01T00:00:00Z"}}})
            .to_string();
        write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        )
        .unwrap();
    });
    let tmp = tempfile::tempdir().unwrap();
    let key = tmp.path().join("key");
    fs::write(&key, "fixture-secret").unwrap();
    fs::set_permissions(&key, fs::Permissions::from_mode(0o600)).unwrap();
    let account = Account {
        name: "subscription".into(),
        provider: p.id.clone(),
        provider_fingerprint: p.fingerprint(),
        credential: Credential::File(key),
    };
    let definitions = BTreeMap::from([(p.id.clone(), p)]);
    let rows = status::collect(
        std::slice::from_ref(&account),
        &definitions,
        &tmp.path().join("cache"),
        false,
        true,
        true,
    )
    .unwrap();
    assert_eq!(rows[0].state, "ready");
    assert_eq!(
        rows[0].data.as_ref().unwrap().quotas[0]
            .used_percent
            .as_deref(),
        Some("25")
    );
    server.join().unwrap();
    let rows = status::collect(
        &[account],
        &definitions,
        &tmp.path().join("cache"),
        true,
        false,
        true,
    )
    .unwrap();
    assert_eq!(rows[0].state, "cached");
}
