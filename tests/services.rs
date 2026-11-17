//! Synthetic examples matching cited provider schemas, never real account data.
use serde_json::json;
use token_manager::provider::{Provider, providers};

fn catalog() -> std::collections::BTreeMap<String, Provider> {
    let tmp = tempfile::tempdir().unwrap();
    providers(tmp.path(), false).unwrap()
}
#[test]
fn all_toml_files_are_embedded_without_a_manual_rust_registry() {
    let entries = catalog();
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("providers");
    let expected: std::collections::BTreeSet<_> = std::fs::read_dir(root)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "toml"))
        .map(|path| {
            toml::from_str::<Provider>(&std::fs::read_to_string(path).unwrap())
                .unwrap()
                .id
        })
        .collect();
    assert_eq!(
        entries
            .keys()
            .cloned()
            .collect::<std::collections::BTreeSet<_>>(),
        expected
    );
    for name in ["moonshot", "moonshot-cn", "featherless", "synthetic"] {
        assert!(entries.contains_key(name), "{name} not embedded");
        entries[name].validate(false).unwrap();
    }
}
#[test]
fn moonshot_regions_remain_separate_and_use_the_available_balance_field() {
    let entries = catalog();
    let payload = json!({"code":0,"status":true,"data":{"available_balance":"7.250001","voucher_balance":"7.250001","cash_balance":"-2.5"}});
    let international = entries["moonshot"].extract(&payload).unwrap();
    let china = entries["moonshot-cn"].extract(&payload).unwrap();
    assert_eq!(international.balances[0].remaining, "7.250001");
    assert_eq!(international.balances[0].currency, "USD");
    assert_eq!(china.balances[0].currency, "CNY");
    assert_eq!(
        entries["moonshot"].endpoint(false).unwrap().host_str(),
        Some("api.moonshot.ai")
    );
    assert_eq!(
        entries["moonshot-cn"].endpoint(false).unwrap().host_str(),
        Some("api.moonshot.cn")
    );
    assert_ne!(
        entries["moonshot"].fingerprint(),
        entries["moonshot-cn"].fingerprint()
    );
    assert!(
        entries["moonshot"]
            .extract(&json!({"error":{"message":"not authorized"}}))
            .is_err()
    );
}
#[test]
fn featherless_reports_spendable_credit_after_reservations() {
    let entries = catalog();
    let data = entries["featherless"]
        .extract(&json!({
            "balance_nano_usd":"10000000000", "balance_usd":"10.000000000",
            "reserved_nano_usd":"1250000000", "reserved_usd":"1.250000000",
            "available_nano_usd":"8750000000", "available_usd":"8.750000000", "currency":"usd"
        }))
        .unwrap();
    assert_eq!(data.balances[0].remaining, "8.75");
    assert_eq!(data.balances[0].currency, "USD");
    assert!(entries["featherless"].credential_help.contains("Admin"));
    // An account subtotal is not a substitute for missing available credit.
    assert!(
        entries["featherless"]
            .extract(&json!({"balance_usd":"10"}))
            .is_err()
    );
}
#[test]
fn synthetic_shows_absolute_request_limits_and_derived_percentage() {
    let entries = catalog();
    let provider = &entries["synthetic"];
    let result = provider
        .extract(
            &json!({"subscription":{"limit":135,"requests":27,"renewsAt":"2026-12-01T12:00:00Z"}}),
        )
        .unwrap();
    let quota = &result.quotas[0];
    assert_eq!(quota.used.as_deref(), Some("27"));
    assert_eq!(quota.limit.as_deref(), Some("135"));
    assert_eq!(quota.used_percent.as_deref(), Some("20"));
    assert_eq!(quota.unit.as_deref(), Some("requests"));
    assert_eq!(quota.reset_at.as_deref(), Some("2026-12-01T12:00:00Z"));
    assert!(result.balances.is_empty());
    assert!(provider.experimental);
}
#[test]
fn derived_quota_handles_zero_unknown_over_limit_and_bad_data_honestly() {
    let entries = catalog();
    let provider = &entries["synthetic"];
    let empty = provider
        .extract(&json!({"subscription":{"limit":0,"requests":0}}))
        .unwrap();
    assert_eq!(empty.quotas[0].limit.as_deref(), Some("0"));
    assert_eq!(empty.quotas[0].used_percent, None);
    let partial = provider
        .extract(&json!({"subscription":{"limit":null,"requests":7}}))
        .unwrap();
    assert_eq!(partial.quotas[0].limit, None);
    assert_eq!(partial.quotas[0].used_percent, None);
    let over = provider
        .extract(&json!({"subscription":{"limit":"10","requests":"15"}}))
        .unwrap();
    assert_eq!(over.quotas[0].used_percent.as_deref(), Some("150"));
    assert!(
        provider
            .extract(&json!({"subscription":{"limit":null,"requests":null}}))
            .is_err()
    );
    for value in [json!(-1), json!(true), json!("NaN"), json!("infinity")] {
        assert!(
            provider
                .extract(&json!({"subscription":{"limit":10,"requests":value}}))
                .is_err()
        );
    }
}
#[test]
fn invalid_quota_definitions_are_rejected_before_account_requests() {
    let entries = catalog();
    let mut provider = entries["synthetic"].clone();
    provider.quotas[0].limit = None;
    assert!(provider.validate(false).is_err());
    let mut provider = entries["synthetic"].clone();
    provider.quotas[0].unit = None;
    assert!(provider.validate(false).is_err());
    let mut provider = entries["synthetic"].clone();
    provider.quotas[0].unit = Some("requests\nspoofed".into());
    assert!(provider.validate(false).is_err());
}

#[test]
fn in_band_errors_are_not_accepted_as_fresh_balance_data() {
    let entries = catalog();
    for payload in [
        json!({"code":42,"status":false,"data":{"available_balance":"100"}}),
        json!({"code":0,"status":false,"data":{"available_balance":"100"}}),
        json!({"code":0,"data":{"available_balance":"100"}}),
    ] {
        assert!(entries["moonshot"].extract(&payload).is_err());
        assert!(entries["moonshot-cn"].extract(&payload).is_err());
    }
}
#[test]
fn additive_quota_support_preserves_existing_provider_fingerprints() {
    let entries = catalog();
    for name in [
        "vercel",
        "opencode-go",
        "opencode-zen",
        "openrouter",
        "deepseek",
    ] {
        let serialized = serde_json::to_value(&entries[name]).unwrap();
        // Check the bytes' actual field shape, not a tautological roundtrip.
        assert!(serialized.get("response_checks").is_none());
        assert_eq!(serialized.as_object().unwrap().len(), 13);
        for quota in serialized["quotas"].as_array().unwrap() {
            assert_eq!(quota.as_object().unwrap().len(), 3);
            for field in ["used", "limit", "unit"] {
                assert!(
                    quota.get(field).is_none(),
                    "{name}: additive field changed the legacy binding"
                );
            }
        }
    }
}
