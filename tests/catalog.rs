use serde_json::{Value, json};
use std::{
    io::{Read, Write},
    net::TcpListener,
    process::Command,
};
use token_manager::{
    catalog::{self, CatalogMap, Filter},
    prices::Sort,
};
fn map() -> CatalogMap {
    toml::from_str(include_str!("../providers/catalogs/models-dev.toml")).unwrap()
}
fn model(id: Option<&str>, name: &str, input: Value, output: Value) -> Value {
    let mut m = json!({"name":name,"family":"gemini","modalities":{"output":["text"]},"cost":{"input":input,"output":output},"last_updated":"2026-01-01"});
    if let Some(id) = id {
        m["canonical_model_id"] = json!(id);
    }
    m
}
fn fixture() -> Value {
    let direct = model(Some("google/gemini-pro"), "Gemini Pro", json!(2), json!(10));
    let mut p = json!({
        "models":{
            "google/gemini-pro":{"name":"Gemini Pro","family":"gemini"},
            "google/gemini-pro-preview":{"name":"Gemini Pro Preview","family":"gemini"},
            "anthropic/claude":{"name":"Claude","family":"claude"}
        },
        "providers":{
            "wafer.ai":{"name":"Wafer","models":{}},
            "google":{"name":"Google","doc":"https://example.test/google/pricing","models":{
                "gemini-pro":direct,
                "gemini-pro-preview":model(Some("google/gemini-pro-preview"),"Gemini Pro Preview",json!(3),json!(20)),
                "gemini-zero":model(None,"Gemini Zero",json!(0),json!(0)),
                "gemini-unknown":model(None,"Gemini Unknown",json!(-1),Value::Null),
                "gemini-image":model(None,"Gemini Image",json!(0.05),json!(0.1))
            }},
            "google-vertex":{"name":"Google Vertex","models":{
                "gemini-pro":model(Some("google/gemini-pro"),"Vertex Gemini Pro",json!(5),json!(50)),
                "unlinked-claude":model(None,"Claude on Vertex",json!(2),json!(10))
            }},
            "openrouter":{"name":"OpenRouter","models":{
                "google/gemini-pro":model(Some("google/gemini-pro"),"Google: Gemini Pro",json!(1),json!(8)),
                "google/gemini-pro-preview":model(Some("google/gemini-pro-preview"),"Preview",json!(2),json!(15)),
                "anthropic/claude":model(Some("anthropic/claude"),"Claude",json!(4),json!(15))
            }},
            "reseller":{"name":"Reseller","models":{
                "google-expensive":model(Some("google/gemini-pro"),"Gemini Pro",json!(100),json!(1000)),
                "gemini-pro":model(None,"Gemini Pro unknown identity",json!(0),json!(0))
            }}
        }
    });
    p["providers"]["google"]["models"]["gemini-pro"]["cost"]["tiers"] = json!([
        {"tier":{"type":"context","size":200000},"input":4,"output":20,"cache_read":0.4}
    ]);
    p["providers"]["google"]["models"]["gemini-pro"]["cost"]["context_over_200k"] =
        json!({"input":4,"output":20});
    p["providers"]["google"]["models"]["gemini-image"]["modalities"]["output"] = json!(["image"]);
    p
}
#[test]
fn vendor_lists_models_and_direct_offers_precede_more_expensive_resellers() {
    let report = catalog::extract(
        &fixture(),
        &map(),
        &Filter {
            vendor: Some("GOOGLE"),
            ..Default::default()
        },
        Sort::Output,
    )
    .unwrap();
    assert_eq!(report.models.len(), 5);
    let model = report
        .models
        .iter()
        .find(|m| m.id == "google/gemini-pro")
        .unwrap();
    assert_eq!(
        model
            .offers
            .iter()
            .map(|o| o.provider.as_str())
            .collect::<Vec<_>>(),
        vec!["google", "google-vertex", "reseller", "openrouter"]
    );
    assert!(model.offers[0].direct && model.offers[1].direct && !model.offers[2].direct);
    assert_eq!(
        model.offers[0].rates.input_per_million.as_deref(),
        Some("2")
    );
    assert_eq!(model.offers[0].tiers.len(), 1); // Legacy tier mirror is not duplicated.
    assert_eq!(model.offers[0].tiers[0].context_over, 200000);
    assert_eq!(
        model.offers[0].tiers[0].rates.output_per_million.as_deref(),
        Some("20")
    );
    assert_eq!(report.models[0].id, "google/gemini-pro-preview");
    let zero = report
        .models
        .iter()
        .find(|m| m.id.ends_with("gemini-zero"))
        .unwrap();
    assert_eq!(zero.offers[0].rates.input_per_million.as_deref(), Some("0"));
    let unknown = report
        .models
        .iter()
        .find(|m| m.id.ends_with("gemini-unknown"))
        .unwrap();
    assert!(unknown.offers[0].rates.input_per_million.is_none());
    let image = report
        .models
        .iter()
        .find(|m| m.id.ends_with("gemini-image"))
        .unwrap();
    assert!(image.offers[0].rates.output_per_million.is_none());
    assert!(image.offers[0].note.as_ref().unwrap().contains("Non-text"));
    assert_eq!(report.unlinked_offers, 2);
    let table = catalog::table(&report);
    assert!(table.contains("Google (direct)"));
    assert!(table.contains("context > 200000"));
    assert!(table.contains("n/a"));
}
#[test]
fn exact_ids_family_search_provider_filter_and_vendor_aliases() {
    let payload = fixture();
    let map = map();
    let report = catalog::extract(
        &payload,
        &map,
        &Filter {
            query: Some("google/gemini-pro"),
            ..Default::default()
        },
        Sort::Input,
    )
    .unwrap();
    assert_eq!(report.models.len(), 1);
    assert_eq!(report.models[0].offers.len(), 4);
    let report = catalog::extract(
        &payload,
        &map,
        &Filter {
            query: Some("gemini"),
            vendor: Some("gemini"),
            provider: Some("OpenRouter"),
        },
        Sort::Output,
    )
    .unwrap();
    assert_eq!(report.models.len(), 2);
    assert!(
        report
            .models
            .iter()
            .all(|m| m.offers.len() == 1 && m.offers[0].provider == "openrouter")
    );
    assert!(catalog::table(&report).contains("Direct vendor offer not available"));
    let report = catalog::extract(
        &payload,
        &map,
        &Filter {
            provider: Some("reseller"),
            ..Default::default()
        },
        Sort::Output,
    )
    .unwrap();
    assert_eq!(report.models.len(), 2);
    assert!(report.models.iter().any(|m| m.vendor.is_none()));
    assert!(
        catalog::extract(
            &payload,
            &map,
            &Filter {
                vendor: Some("google"),
                provider: Some("missing"),
                ..Default::default()
            },
            Sort::Input
        )
        .is_err()
    );
    assert!(
        catalog::extract(
            &payload,
            &map,
            &Filter {
                query: Some("---"),
                ..Default::default()
            },
            Sort::Input
        )
        .is_err()
    );
}
#[test]
fn malformed_data_and_unsafe_urls_fail_without_guessing() {
    let filter = Filter {
        vendor: Some("google"),
        ..Default::default()
    };
    assert!(catalog::extract(&json!({}), &map(), &filter, Sort::Output).is_err());
    let mut p = fixture();
    p["providers"]["google"]["models"]["gemini-pro"]["canonical_model_id"] = json!("invalid");
    assert!(catalog::extract(&p, &map(), &filter, Sort::Output).is_err());
    let mut p = fixture();
    p["providers"]["google"]["models"]["gemini-pro"]["cost"]["tiers"] = json!([{"input":1}]);
    assert!(catalog::extract(&p, &map(), &filter, Sort::Output).is_err());
    for url in [
        "http://example.test/catalog",
        "https://secret@example.test/catalog",
        "https://example.test/catalog#fragment",
    ] {
        let mut m = map();
        m.url = url.into();
        assert!(m.endpoint(true).is_err());
    }
    let mut m = map();
    m.url = "http://127.0.0.1:9999/catalog".into();
    assert!(m.endpoint(false).is_err());
    assert!(m.endpoint(true).is_ok());
}
fn cli_response(binary: &str, args: &[&str], status: u16, body: String) -> std::process::Output {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(std::time::Duration::from_secs(5)))
            .unwrap();
        let mut req = Vec::new();
        let mut byte = [0];
        while !req.ends_with(b"\r\n\r\n") {
            stream.read_exact(&mut byte).unwrap();
            req.push(byte[0]);
        }
        let req = String::from_utf8(req).unwrap().to_lowercase();
        assert!(req.starts_with("get /catalog "));
        assert!(!req.contains("authorization:"));
        assert!(!req.contains("x-api-key:"));
        write!(stream,"HTTP/1.1 {status} Response\r\nLocation: http://127.0.0.1:1/must-not-follow\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).unwrap();
    });
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(root.path().join("providers/catalogs")).unwrap();
    std::fs::write(
        root.path().join("providers/catalogs/models-dev.toml"),
        include_str!("../providers/catalogs/models-dev.toml").replace(
            "https://models.dev/catalog.json",
            &format!("http://{address}/catalog"),
        ),
    )
    .unwrap();
    let result = Command::new(binary)
        .arg("--config-dir")
        .arg(root.path())
        .arg("--allow-local-http")
        .args(args)
        .output()
        .unwrap();
    server.join().unwrap();
    assert!(!root.path().join("accounts.json").exists());
    assert!(!root.path().join("cache").exists());
    result
}
#[test]
fn tm_shortcut_and_long_command_match_without_account_credentials() {
    for (bin, args) in [
        (
            env!("CARGO_BIN_EXE_tm"),
            vec!["--vendor", "google", "--json"],
        ),
        (
            env!("CARGO_BIN_EXE_token-manager"),
            vec!["prices", "--vendor", "google", "--json"],
        ),
    ] {
        let result = cli_response(bin, &args, 200, fixture().to_string());
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        let value: Value = serde_json::from_slice(&result.stdout).unwrap();
        assert_eq!(value["models"].as_array().unwrap().len(), 5);
        assert_eq!(value["currency"], "USD");
        assert!(value["observed_at"].as_u64().is_some());
    }
    let result = cli_response(
        env!("CARGO_BIN_EXE_tm"),
        &[
            "prices",
            "google/gemini-pro",
            "--provider",
            "openrouter",
            "--json",
        ],
        200,
        fixture().to_string(),
    );
    assert!(result.status.success());
    let value: Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(value["models"][0]["offers"].as_array().unwrap().len(), 1);
    assert_eq!(value["models"][0]["offers"][0]["provider"], "openrouter");
}
#[test]
fn errors_and_redirects_are_not_hidden_and_raw_bodies_are_not_printed() {
    for status in [302, 403, 429, 500] {
        let result = cli_response(
            env!("CARGO_BIN_EXE_tm"),
            &["--vendor", "google"],
            status,
            "private-raw-error".into(),
        );
        assert!(!result.status.success());
        assert!(!String::from_utf8_lossy(&result.stderr).contains("private-raw-error"));
        assert!(String::from_utf8_lossy(&result.stderr).contains(&status.to_string()));
    }
    let result = cli_response(
        env!("CARGO_BIN_EXE_tm"),
        &["--vendor", "google"],
        200,
        "not-json-secret".into(),
    );
    assert!(!result.status.success());
    assert!(!String::from_utf8_lossy(&result.stderr).contains("not-json-secret"));
}
#[test]
fn pricing_flags_are_not_silently_accepted_by_account_commands() {
    let result = Command::new(env!("CARGO_BIN_EXE_tm"))
        .args(["status", "--vendor", "google"])
        .output()
        .unwrap();
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("Pricing filters only"));
    let result = Command::new(env!("CARGO_BIN_EXE_tm"))
        .args(["add", "--help"])
        .output()
        .unwrap();
    assert!(result.status.success());
}
