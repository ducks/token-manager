use serde_json::json;
use token_manager::provider::Provider;
#[test]
fn analytics_cents_and_subscription_windows_are_separate() {
    let api: Provider = toml::from_str(include_str!("../providers/anthropic.toml")).unwrap();
    let chat: Provider = toml::from_str(include_str!("../providers/claude.toml")).unwrap();
    api.validate(false).unwrap();
    chat.validate(false).unwrap();
    assert_ne!(api.fingerprint(), chat.fingerprint());
    assert_eq!(api.auth.header, "x-api-key");
    assert_eq!(chat.auth.prefix, "Bearer ");
    assert_eq!(
        api.headers["anthropic-beta"],
        "analytics-usage-cost-2026-08-24"
    );
    assert_eq!(chat.headers["anthropic-beta"], "oauth-2025-04-20");
    let (amount, _) = api
        .costs
        .unwrap()
        .page(
            &json!({"data":[{"cost":{"amount":"41280.000001","currency":"USD"}}],"has_more":false}),
        )
        .unwrap();
    assert_eq!(amount.unwrap().to_string(), "412.80000001");
    let data = chat.extract(&json!({"five_hour":{"utilization":23,"resets_at":"2026-11-26T19:00:00Z"},"seven_day":null})).unwrap();
    assert_eq!(data.quotas[0].used_percent.as_deref(), Some("23"));
    assert_eq!(
        data.quotas[0].reset_at.as_deref(),
        Some("2026-11-26T19:00:00Z")
    );
    assert!(data.quotas[1].used_percent.is_none());
    assert!(data.quotas[2].used_percent.is_none());
}
