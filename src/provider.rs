use anyhow::{Result, bail, ensure};
use reqwest::Url;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, fs, path::Path, str::FromStr};

include!(concat!(env!("OUT_DIR"), "/providers.rs"));

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Provider {
    pub id: String,
    pub name: String,
    pub base_url: String,
    #[serde(default)]
    pub path: String,
    #[serde(default, skip_serializing_if = "RequestMethod::is_get")]
    pub method: RequestMethod,
    pub unsupported_reason: Option<String>,
    #[serde(default)]
    pub auth: Auth,
    pub key_url: String,
    pub billing_url: String,
    pub credential_help: String,
    pub source: String,
    #[serde(default)]
    pub experimental: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pricing: Option<crate::prices::PricingMap>,
    pub balance: Option<BalanceMap>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub response_checks: Vec<ResponseCheck>,
    #[serde(default)]
    pub quotas: Vec<QuotaMap>,
}
#[derive(Clone, Copy, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "UPPERCASE")]
pub enum RequestMethod {
    #[default]
    Get,
    Post,
}
impl RequestMethod {
    pub fn is_get(&self) -> bool {
        *self == Self::Get
    }
}
impl From<RequestMethod> for reqwest::Method {
    fn from(method: RequestMethod) -> Self {
        match method {
            RequestMethod::Get => Self::GET,
            RequestMethod::Post => Self::POST,
        }
    }
}
#[derive(Clone, Copy, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ResetFormat {
    #[default]
    Text,
    UnixSeconds,
    UnixMilliseconds,
}
impl ResetFormat {
    pub fn is_text(&self) -> bool {
        *self == Self::Text
    }
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Auth {
    pub header: String,
    pub prefix: String,
}
impl Default for Auth {
    fn default() -> Self {
        Self {
            header: "Authorization".into(),
            prefix: "Bearer ".into(),
        }
    }
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ResponseCheck {
    pub path: String,
    pub equals: Value,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BalanceMap {
    pub rows: String,
    pub currency: Option<String>,
    pub currency_path: Option<String>,
    pub remaining: Option<String>,
    pub total: Option<String>,
    pub used: Option<String>,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct QuotaMap {
    pub label: String,
    pub percent: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub used: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unit: Option<String>,
    pub reset_at: Option<String>,
    #[serde(default, skip_serializing_if = "ResetFormat::is_text")]
    pub reset_format: ResetFormat,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unavailable_when: Option<String>,
}
#[derive(Clone, Deserialize, Serialize, Debug, PartialEq)]
pub struct Balance {
    pub currency: String,
    pub remaining: String,
    pub used: Option<String>,
}
#[derive(Clone, Deserialize, Serialize, Debug, PartialEq)]
pub struct Quota {
    pub label: String,
    pub used: Option<String>,
    pub limit: Option<String>,
    pub unit: Option<String>,
    pub used_percent: Option<String>,
    pub reset_at: Option<String>,
}
#[derive(Clone, Default, Deserialize, Serialize, Debug, PartialEq)]
pub struct AccountData {
    pub balances: Vec<Balance>,
    pub quotas: Vec<Quota>,
}
pub fn identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}
fn pointer(value: &str) -> bool {
    value.is_empty() || value.starts_with('/')
}
fn money(value: &Value, path: &str) -> Result<Decimal> {
    let item = value
        .pointer(path)
        .ok_or_else(|| anyhow::anyhow!("required response field missing"))?;
    let text = match item {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        _ => bail!("expected a decimal response field"),
    };
    Decimal::from_str(&text).map_err(|_| anyhow::anyhow!("invalid decimal response field"))
}
impl Provider {
    pub fn fingerprint(&self) -> String {
        // Public catalog changes do not invalidate account credentials or caches.
        let mut account_definition = self.clone();
        account_definition.pricing = None;
        format!(
            "{:x}",
            Sha256::digest(serde_json::to_vec(&account_definition).expect("serializable provider"))
        )
    }
    pub fn endpoint(&self, allow_local_http: bool) -> Result<Url> {
        let base = Url::parse(&self.base_url)?;
        ensure!(
            base.username().is_empty()
                && base.password().is_none()
                && base.query().is_none()
                && base.fragment().is_none(),
            "provider URL must not contain credentials, query or fragment"
        );
        let local = matches!(base.host_str(), Some("127.0.0.1" | "[::1]"));
        ensure!(
            base.scheme() == "https" || (allow_local_http && local && base.scheme() == "http"),
            "providers require HTTPS; local fixture HTTP needs --allow-local-http"
        );
        ensure!(
            self.path.starts_with('/') && !self.path.starts_with("//"),
            "provider path must be an absolute same-origin path"
        );
        let endpoint = base.join(&self.path)?;
        ensure!(
            endpoint.origin() == base.origin() && endpoint.fragment().is_none(),
            "provider request must stay on its origin"
        );
        Ok(endpoint)
    }
    pub fn validate(&self, allow_local_http: bool) -> Result<()> {
        ensure!(identifier(&self.id), "invalid provider ID");
        if let Some(map) = &self.pricing {
            map.validate(self, allow_local_http)?;
        }
        ensure!(
            reqwest::header::HeaderName::from_bytes(self.auth.header.as_bytes()).is_ok(),
            "invalid authentication header name"
        );
        ensure!(
            self.auth.prefix.len() <= 64
                && self
                    .auth
                    .prefix
                    .bytes()
                    .all(|b| b.is_ascii() && !b.is_ascii_control()),
            "invalid authentication header prefix"
        );
        ensure!(
            !self.name.is_empty()
                && self.name.len() <= 80
                && !self.name.chars().any(char::is_control),
            "invalid provider name"
        );
        if self.unsupported_reason.is_none() {
            self.endpoint(allow_local_http)?;
        } else {
            ensure!(
                self.path.is_empty()
                    && self.balance.is_none()
                    && self.quotas.is_empty()
                    && self.method.is_get(),
                "unsupported providers must not declare request mappings"
            );
        }
        for link in [&self.key_url, &self.billing_url] {
            let url = Url::parse(link)?;
            ensure!(
                url.scheme() == "https" && url.username().is_empty() && url.password().is_none(),
                "console links must be HTTPS without credentials"
            );
        }
        ensure!(
            self.unsupported_reason.is_some() || self.balance.is_some() || !self.quotas.is_empty(),
            "provider has no account-data mapping"
        );
        ensure!(self.response_checks.len() <= 8, "too many response checks");
        for check in &self.response_checks {
            ensure!(
                pointer(&check.path)
                    && matches!(
                        check.equals,
                        Value::String(_) | Value::Number(_) | Value::Bool(_)
                    ),
                "response checks require a pointer and scalar expected value"
            );
        }
        ensure!(self.quotas.len() <= 16, "too many quota mappings");
        if let Some(map) = &self.balance {
            ensure!(pointer(&map.rows), "invalid rows JSON pointer");
            ensure!(
                map.currency.is_some() ^ map.currency_path.is_some(),
                "choose currency or currency_path"
            );
            ensure!(
                map.remaining.is_some() || (map.total.is_some() && map.used.is_some()),
                "balance needs remaining or total and used"
            );
            for field in [&map.currency_path, &map.remaining, &map.total, &map.used]
                .into_iter()
                .flatten()
            {
                ensure!(pointer(field), "invalid balance JSON pointer");
            }
        }
        for quota in &self.quotas {
            ensure!(
                !quota.label.is_empty()
                    && quota.label.len() <= 40
                    && !quota.label.chars().any(char::is_control),
                "invalid quota label"
            );
            ensure!(
                quota.used.is_some() == quota.limit.is_some(),
                "quota used/limit must be paired"
            );
            ensure!(
                quota.percent.is_some() || quota.used.is_some(),
                "quota requires percent or used/limit"
            );
            ensure!(
                quota.unit.as_ref().is_none_or(|unit| !unit.is_empty()
                    && unit.len() <= 24
                    && unit.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')),
                "invalid quota unit"
            );
            ensure!(
                quota.used.is_none() || quota.unit.is_some(),
                "absolute quotas require an explicit unit"
            );
            for path in [
                &quota.percent,
                &quota.used,
                &quota.limit,
                &quota.reset_at,
                &quota.unavailable_when,
            ]
            .into_iter()
            .flatten()
            {
                ensure!(pointer(path), "invalid quota JSON pointer");
            }
        }
        Ok(())
    }
    pub fn extract(&self, value: &Value) -> Result<AccountData> {
        for check in &self.response_checks {
            ensure!(
                value.pointer(&check.path) == Some(&check.equals),
                "provider response check failed"
            );
        }
        let mut result = AccountData::default();
        if let Some(map) = &self.balance {
            let rows = value
                .pointer(&map.rows)
                .ok_or_else(|| anyhow::anyhow!("balance data missing"))?;
            let rows: Vec<&Value> = match rows {
                Value::Object(_) => vec![rows],
                Value::Array(rows) => rows.iter().collect(),
                _ => bail!("balance rows must be object or array"),
            };
            ensure!(
                !rows.is_empty() && rows.len() <= 32,
                "missing or excessive balance rows"
            );
            for row in rows {
                let currency = match &map.currency {
                    Some(c) => c.clone(),
                    None => row
                        .pointer(map.currency_path.as_ref().unwrap())
                        .and_then(Value::as_str)
                        .ok_or_else(|| anyhow::anyhow!("currency missing"))?
                        .to_owned(),
                };
                ensure!(
                    !currency.is_empty()
                        && currency.len() <= 8
                        && currency.bytes().all(|b| b.is_ascii_uppercase()),
                    "invalid currency"
                );
                let remaining = match &map.remaining {
                    Some(p) => money(row, p)?,
                    None => money(row, map.total.as_ref().unwrap())?
                        .checked_sub(money(row, map.used.as_ref().unwrap())?)
                        .ok_or_else(|| anyhow::anyhow!("balance arithmetic overflow"))?,
                };
                let used = map
                    .used
                    .as_ref()
                    .map(|p| money(row, p).map(|n| n.normalize().to_string()))
                    .transpose()?;
                result.balances.push(Balance {
                    currency,
                    remaining: remaining.normalize().to_string(),
                    used,
                });
            }
        }
        for quota in &self.quotas {
            // Missing counters stay unknown. Zero limits never mean unlimited.
            let optional_number = |path: &Option<String>| -> Result<Option<Decimal>> {
                match path {
                    Some(path) if value.pointer(path).is_some_and(|v| !v.is_null()) => {
                        let number = money(value, path)?;
                        ensure!(number >= Decimal::ZERO, "negative quota counter");
                        Ok(Some(number))
                    }
                    _ => Ok(None),
                }
            };
            let unavailable = match quota
                .unavailable_when
                .as_ref()
                .and_then(|path| value.pointer(path))
            {
                None | Some(Value::Null) => false,
                Some(Value::Bool(value)) => *value,
                _ => bail!("quota unavailable flag must be boolean"),
            };
            let used = if unavailable {
                None
            } else {
                optional_number(&quota.used)?
            };
            let limit = optional_number(&quota.limit)?;
            let percent = if unavailable {
                None
            } else if quota.percent.is_some() {
                optional_number(&quota.percent)?
            } else if let (Some(used), Some(limit)) = (used, limit) {
                if limit.is_zero() {
                    None
                } else {
                    Some(
                        used.checked_div(limit)
                            .and_then(|v| v.checked_mul(Decimal::from(100)))
                            .ok_or_else(|| anyhow::anyhow!("quota arithmetic overflow"))?,
                    )
                }
            } else {
                None
            };
            let used_percent = percent.map(|p| p.normalize().to_string());
            let raw_reset = quota.reset_at.as_ref().and_then(|path| value.pointer(path));
            let reset_at = if unavailable {
                None
            } else {
                match raw_reset {
                    None | Some(Value::Null) => None,
                    Some(value) => Some(match quota.reset_format {
                        ResetFormat::Text => value
                            .as_str()
                            .ok_or_else(|| anyhow::anyhow!("reset must be text"))?
                            .to_owned(),
                        format => {
                            let timestamp = value
                                .as_i64()
                                .or_else(|| value.as_str().and_then(|v| v.parse().ok()))
                                .ok_or_else(|| {
                                    anyhow::anyhow!("reset must be integer epoch time")
                                })?;
                            ensure!(timestamp >= 0, "reset epoch time must be nonnegative");
                            let date = if format == ResetFormat::UnixMilliseconds {
                                chrono::DateTime::from_timestamp_millis(timestamp)
                            } else {
                                chrono::DateTime::from_timestamp(timestamp, 0)
                            }
                            .ok_or_else(|| anyhow::anyhow!("reset epoch time out of range"))?;
                            date.to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
                        }
                    }),
                }
            };
            ensure!(
                reset_at
                    .as_ref()
                    .is_none_or(|s| s.len() <= 64 && !s.chars().any(char::is_control)),
                "invalid quota reset field"
            );
            result.quotas.push(Quota {
                label: quota.label.clone(),
                used: used.map(|n| n.normalize().to_string()),
                limit: limit.map(|n| n.normalize().to_string()),
                unit: quota.unit.clone(),
                used_percent,
                reset_at,
            });
        }
        ensure!(
            !result.balances.is_empty()
                || result
                    .quotas
                    .iter()
                    .any(|q| q.used_percent.is_some() || q.used.is_some() || q.limit.is_some()),
            "no supported account data in response"
        );
        Ok(result)
    }
}
pub fn providers(directory: &Path, allow_local_http: bool) -> Result<BTreeMap<String, Provider>> {
    let mut result = BTreeMap::new();
    for text in BUILTIN_PROVIDERS {
        let provider: Provider = toml::from_str(text)?;
        provider.validate(false)?;
        ensure!(
            result.insert(provider.id.clone(), provider).is_none(),
            "duplicate built-in provider ID"
        );
    }
    let mut custom_ids = std::collections::HashSet::new();
    if directory.exists() {
        for entry in fs::read_dir(directory)? {
            let path = entry?.path();
            if path.extension().is_none_or(|s| s != "toml") {
                continue;
            }
            ensure!(
                !path.is_symlink() && path.metadata()?.len() <= 65536,
                "provider file must be a bounded regular file"
            );
            let provider: Provider = toml::from_str(&fs::read_to_string(path)?)
                .map_err(|_| anyhow::anyhow!("invalid custom provider TOML"))?;
            provider.validate(allow_local_http)?;
            ensure!(
                custom_ids.insert(provider.id.clone()),
                "duplicate custom provider ID"
            );
            result.insert(provider.id.clone(), provider);
        }
    }
    Ok(result)
}
