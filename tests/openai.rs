use serde_json::json;
use token_manager::provider::Provider;
#[test]
fn api_costs_and_subscription_quotas_have_separate_contracts() {
    let api: Provider = toml::from_str(include_str!("../providers/openai.toml")).unwrap();
    let chat: Provider = toml::from_str(include_str!("../providers/chatgpt.toml")).unwrap();
    api.validate(false).unwrap();
    chat.validate(false).unwrap();
    assert_ne!(api.fingerprint(), chat.fingerprint());
    assert!(api.credential_help.contains("Admin"));
    assert!(chat.experimental);
    let (amount, _) = api.costs.unwrap().page(&json!({"data":[{"results":[{"amount":{"value":0.060001,"currency":"usd"}}]}],"has_more":false})).unwrap();
    assert_eq!(amount.unwrap().to_string(), "0.060001");
    let data = chat.extract(&json!({"rate_limit":{"primary_window":{"used_percent":42,"reset_at":1795000000},"secondary_window":null},"email":"private@example.test"})).unwrap();
    assert_eq!(data.quotas[0].used_percent.as_deref(), Some("42"));
    assert!(data.quotas[0].reset_at.as_ref().unwrap().ends_with('Z'));
    assert!(data.quotas[1].used_percent.is_none());
    assert!(
        !serde_json::to_string(&data)
            .unwrap()
            .contains("private@example.test")
    );
}
