//! Declarative, bounded read-only cost reports. Never infer a credit balance from spend.
use crate::provider::{AccountData, Provider};
use anyhow::{Context, Result, ensure};
use reqwest::blocking::Client;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet},
    io::Read,
    str::FromStr,
};

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CostMap {
    pub start_parameter: String,
    pub end_parameter: String,
    pub time_format: TimeFormat,
    #[serde(default)]
    pub query: BTreeMap<String, String>,
    pub rows: String,
    pub nested_rows: Option<String>,
    pub amount: String,
    pub currency: String,
    pub divisor: u32,
    pub has_more: String,
    pub next_page: String,
    pub page_parameter: String,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TimeFormat {
    UnixSeconds,
    Rfc3339,
}
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
pub struct Cost {
    pub currency: String,
    pub amount: Option<String>,
    pub starting_at: String,
    pub ending_at: String,
}
impl CostMap {
    pub fn validate(&self) -> Result<()> {
        ensure!(self.divisor > 0, "cost divisor must be positive");
        for pointer in [
            &self.rows,
            &self.amount,
            &self.currency,
            &self.has_more,
            &self.next_page,
        ]
        .into_iter()
        .chain(self.nested_rows.iter())
        {
            ensure!(
                pointer.starts_with('/'),
                "cost mapping requires JSON pointers"
            );
        }
        let params = [
            &self.start_parameter,
            &self.end_parameter,
            &self.page_parameter,
        ];
        ensure!(
            params.iter().collect::<BTreeSet<_>>().len() == 3,
            "cost parameters must be distinct"
        );
        for name in params.into_iter().chain(self.query.keys()) {
            ensure!(
                !name.is_empty()
                    && name.len() <= 64
                    && name
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b"_[]".contains(&b)),
                "invalid cost query parameter"
            );
        }
        ensure!(
            !self.query.keys().any(|k| params.contains(&k)),
            "fixed queries cannot override cost window or pagination"
        );
        Ok(())
    }
    pub fn page(&self, value: &Value) -> Result<(Option<Decimal>, Option<String>)> {
        let rows = value
            .pointer(&self.rows)
            .and_then(Value::as_array)
            .context("cost rows missing")?;
        let mut total: Option<Decimal> = None;
        for row in rows {
            let entries: Vec<&Value> = if let Some(pointer) = &self.nested_rows {
                row.pointer(pointer)
                    .and_then(Value::as_array)
                    .context("cost bucket missing results")?
                    .iter()
                    .collect()
            } else {
                vec![row]
            };
            for entry in entries {
                let currency = entry
                    .pointer(&self.currency)
                    .and_then(Value::as_str)
                    .context("cost currency missing")?;
                ensure!(
                    currency.eq_ignore_ascii_case("usd"),
                    "unexpected cost currency"
                );
                let raw = entry.pointer(&self.amount).context("cost amount missing")?;
                let text = match raw {
                    Value::String(s) => s.clone(),
                    Value::Number(n) => n.to_string(),
                    _ => anyhow::bail!("cost amount unavailable"),
                };
                let amount = Decimal::from_str(&text)
                    .context("invalid cost amount")?
                    .checked_div(Decimal::from(self.divisor))
                    .context("cost overflow")?;
                total = Some(
                    total
                        .unwrap_or_default()
                        .checked_add(amount)
                        .context("cost overflow")?,
                );
            }
        }
        let more = value
            .pointer(&self.has_more)
            .and_then(Value::as_bool)
            .context("cost pagination missing")?;
        let next = if more {
            let cursor = value
                .pointer(&self.next_page)
                .and_then(Value::as_str)
                .context("cost cursor missing")?;
            ensure!(
                !cursor.is_empty() && cursor.len() <= 2048 && !cursor.chars().any(char::is_control),
                "invalid cost cursor"
            );
            Some(cursor.to_owned())
        } else {
            None
        };
        Ok((total, next))
    }
}
fn timestamp(seconds: u64) -> Result<String> {
    chrono::DateTime::from_timestamp(i64::try_from(seconds)?, 0)
        .map(|d| d.to_rfc3339_opts(chrono::SecondsFormat::Secs, true))
        .context("invalid cost timestamp")
}
type FetchError = (&'static str, &'static str);
pub fn fetch(
    client: &Client,
    provider: &Provider,
    secret: &str,
    local: bool,
) -> Result<AccountData, FetchError> {
    let map = provider
        .costs
        .as_ref()
        .ok_or(("configuration_error", "Cost mapping missing"))?;
    // Completed UTC days give both APIs an aligned, comparable reporting window.
    let end = crate::state::now() / 86400 * 86400;
    let start = end.saturating_sub(30 * 86400);
    let starting_at =
        timestamp(start).map_err(|_| ("configuration_error", "Invalid cost window"))?;
    let ending_at = timestamp(end).map_err(|_| ("configuration_error", "Invalid cost window"))?;
    let mut url = provider
        .endpoint(local)
        .map_err(|_| ("configuration_error", "Invalid provider URL"))?;
    let (start_value, end_value) = match map.time_format {
        TimeFormat::UnixSeconds => (start.to_string(), end.to_string()),
        TimeFormat::Rfc3339 => (starting_at.clone(), ending_at.clone()),
    };
    url.query_pairs_mut()
        .append_pair(&map.start_parameter, &start_value)
        .append_pair(&map.end_parameter, &end_value)
        .extend_pairs(&map.query);
    let mut cursor: Option<String> = None;
    let mut seen = BTreeSet::new();
    let mut total: Option<Decimal> = None;
    for _ in 0..16 {
        let mut page_url = url.clone();
        if let Some(cursor) = &cursor {
            page_url
                .query_pairs_mut()
                .append_pair(&map.page_parameter, cursor);
        }
        let mut request = client.get(page_url).header(
            &provider.auth.header,
            format!("{}{}", provider.auth.prefix, secret),
        );
        for (name, value) in &provider.headers {
            request = request.header(name, value);
        }
        let response = request
            .send()
            .map_err(|_| ("network_error", "Cost request failed or timed out"))?;
        match response.status().as_u16() {
            200..=299 => {}
            401 => return Err(("login_required", "Credential rejected; run login")),
            403 => return Err(("permission_denied", "Credential lacks cost-report access")),
            429 => return Err(("rate_limited", "Cost endpoint rate limited; try later")),
            _ => {
                return Err((
                    "provider_error",
                    "Cost endpoint returned an error or redirect",
                ));
            }
        }
        let mut body = Vec::new();
        response
            .take(1048577)
            .read_to_end(&mut body)
            .map_err(|_| ("network_error", "Could not read cost response"))?;
        if body.len() > 1048576 {
            return Err(("response_error", "Cost response exceeds size limit"));
        }
        let value = serde_json::from_slice(&body)
            .map_err(|_| ("response_error", "Cost endpoint did not return JSON"))?;
        let (amount, next) = map.page(&value).map_err(|_| {
            (
                "response_error",
                "Cost response does not match provider mapping",
            )
        })?;
        if let Some(amount) = amount {
            total = Some(
                total
                    .unwrap_or_default()
                    .checked_add(amount)
                    .ok_or(("response_error", "Cost arithmetic overflow"))?,
            );
        }
        match next {
            Some(next) => {
                if !seen.insert(next.clone()) {
                    return Err(("response_error", "Cost pagination repeated a cursor"));
                }
                cursor = Some(next);
            }
            None => {
                return Ok(AccountData {
                    costs: vec![Cost {
                        currency: "USD".into(),
                        amount: total.map(|n| n.normalize().to_string()),
                        starting_at,
                        ending_at,
                    }],
                    ..Default::default()
                });
            }
        }
    }
    Err((
        "response_error",
        "Cost report exceeds pagination limit; partial totals discarded",
    ))
}
