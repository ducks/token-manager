use crate::{
    credentials,
    provider::{AccountData, Provider},
    state::{Account, atomic_write, now, read_private},
};
use reqwest::{blocking::Client, redirect::Policy};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    io::Read,
    path::Path,
    sync::{Arc, Mutex},
    time::Duration,
};

#[derive(Clone, Serialize, Deserialize)]
pub struct Row {
    pub account: String,
    pub provider: String,
    pub state: String,
    pub detail: Option<String>,
    pub data: Option<AccountData>,
    pub observed_at: Option<u64>,
    pub cached: bool,
    pub stale: bool,
    pub experimental: bool,
}
#[derive(Serialize, Deserialize)]
struct Cache {
    provider_fingerprint: String,
    observed_at: u64,
    data: AccountData,
}
fn fetch(
    client: &Client,
    provider: &Provider,
    secret: String,
    allow_local_http: bool,
) -> Result<AccountData, (&'static str, &'static str)> {
    if provider.costs.is_some() {
        return crate::costs::fetch(client, provider, &secret, allow_local_http);
    }
    let url = provider
        .endpoint(allow_local_http)
        .map_err(|_| ("configuration_error", "Invalid provider URL"))?;
    let mut request = client.request(provider.method.into(), url);
    for (name, value) in &provider.headers {
        request = request.header(name, value);
    }
    let response = request
        .header(
            &provider.auth.header,
            format!("{}{}", provider.auth.prefix, secret),
        )
        .send()
        .map_err(|_| ("network_error", "Account request failed or timed out"))?;
    let status = response.status().as_u16();
    match status {
        200..=299 => {}
        401 => return Err(("login_required", "Credential rejected; run login")),
        403 => {
            return Err((
                "permission_denied",
                "Credential lacks account access; see provider credential help",
            ));
        }
        429 => return Err(("rate_limited", "Account endpoint rate limited; try later")),
        300..=399 => {
            return Err((
                "provider_error",
                "Redirect refused; review provider endpoint",
            ));
        }
        _ => return Err(("provider_error", "Account endpoint returned an error")),
    }
    let mut body = Vec::new();
    response
        .take(1048577)
        .read_to_end(&mut body)
        .map_err(|_| ("network_error", "Could not read account response"))?;
    if body.len() > 1048576 {
        return Err(("response_error", "Account response exceeds size limit"));
    }
    let value = serde_json::from_slice(&body)
        .map_err(|_| ("response_error", "Account endpoint did not return JSON"))?;
    provider.extract(&value).map_err(|_| {
        (
            "response_error",
            "Account response does not match provider mapping",
        )
    })
}
pub fn collect(
    accounts: &[Account],
    providers: &BTreeMap<String, Provider>,
    cache_root: &Path,
    cached_only: bool,
    refresh: bool,
    allow_local_http: bool,
) -> anyhow::Result<Vec<Row>> {
    let client = Client::builder()
        .timeout(Duration::from_secs(8))
        .connect_timeout(Duration::from_secs(3))
        .redirect(Policy::none())
        .build()?;
    let queue = Mutex::new(accounts.iter().enumerate());
    let results = Arc::new(Mutex::new(Vec::new()));
    std::thread::scope(|scope| {
        for _ in 0..accounts.len().min(4) {
            let client = &client;
            let queue = &queue;
            let results = &results;
            scope.spawn(move || {
                loop {
                    let next = queue.lock().expect("queue lock").next();
                    let Some((index, account)) = next else {
                        break;
                    };
                    let mut row = Row {
                        account: account.name.clone(),
                        provider: account.provider.clone(),
                        state: "unknown".into(),
                        detail: None,
                        data: None,
                        observed_at: None,
                        cached: false,
                        stale: false,
                        experimental: false,
                    };
                    let Some(provider) = providers.get(&account.provider) else {
                        row.detail = Some("Provider definition unavailable".into());
                        results.lock().unwrap().push((index, row));
                        continue;
                    };
                    row.experimental = provider.experimental;
                    if let Some(reason) = &provider.unsupported_reason {
                        row.state = "unsupported".into();
                        row.detail = Some(reason.clone());
                        results.lock().unwrap().push((index, row));
                        continue;
                    }
                    let fingerprint = provider.fingerprint();
                    if fingerprint != account.provider_fingerprint {
                        row.state = "login_required".into();
                        row.detail =
                            Some("Provider definition changed; review it and run login".into());
                        results.lock().unwrap().push((index, row));
                        continue;
                    }
                    let cache_path = cache_root.join(format!("{}.json", account.name));
                    let cache = read_private(&cache_path)
                        .ok()
                        .and_then(|bytes| serde_json::from_slice::<Cache>(&bytes).ok())
                        .filter(|c| {
                            c.provider_fingerprint == fingerprint && c.observed_at <= now()
                        });
                    if let Some(cache) = &cache {
                        row.data = Some(cache.data.clone());
                        row.observed_at = Some(cache.observed_at);
                        row.cached = true;
                        row.stale = now().saturating_sub(cache.observed_at) > 60;
                    }
                    if cached_only || (!refresh && cache.is_some() && !row.stale) {
                        row.state = if cache.is_some() { "cached" } else { "unknown" }.into();
                    } else {
                        let fetched = credentials::resolve(account)
                            .map_err(|_| {
                                (
                                    "login_required",
                                    "Credential unavailable; run login or check its reference",
                                )
                            })
                            .and_then(|secret| fetch(client, provider, secret, allow_local_http));
                        match fetched {
                            Ok(data) => {
                                let observed_at = now();
                                let cache = Cache {
                                    provider_fingerprint: fingerprint,
                                    observed_at,
                                    data: data.clone(),
                                };
                                let saved = serde_json::to_vec(&cache)
                                    .ok()
                                    .is_some_and(|bytes| atomic_write(&cache_path, &bytes).is_ok());
                                row.state = "ready".into();
                                row.detail = (!saved)
                                    .then(|| "Live data received; cache could not be saved".into());
                                row.data = Some(data);
                                row.observed_at = Some(observed_at);
                                row.cached = false;
                                row.stale = false;
                            }
                            Err((state, detail)) => {
                                row.state = state.into();
                                row.detail = Some(if state == "permission_denied" {
                                    format!(
                                        "{detail}. {} Run: token-manager login {}",
                                        provider.credential_help, account.name
                                    )
                                } else {
                                    detail.into()
                                });
                                row.stale = row.cached;
                            }
                        }
                    }
                    results.lock().unwrap().push((index, row));
                }
            });
        }
    });
    let mut rows = results.lock().unwrap().clone();
    rows.sort_by_key(|(index, _)| *index);
    Ok(rows.into_iter().map(|(_, row)| row).collect())
}
pub fn table(rows: &[Row]) -> String {
    let mut output = format!(
        "{:<20} {:<16} {:<25} {}\n",
        "ACCOUNT", "PROVIDER", "STATUS", "ACCOUNT DATA"
    );
    if rows.is_empty() {
        output.push_str("No accounts yet. Run: token-manager add vercel\n");
    }
    for row in rows {
        let mut values = Vec::new();
        if let Some(data) = &row.data {
            for cost in &data.costs {
                values.push(format!(
                    "{} {} reported spend; {} to {} UTC (exclusive)",
                    cost.currency,
                    cost.amount.as_deref().unwrap_or("not available"),
                    cost.starting_at,
                    cost.ending_at
                ));
            }
            for balance in &data.balances {
                values.push(format!(
                    "{} {} remaining{}",
                    balance.currency,
                    balance.remaining,
                    balance
                        .used
                        .as_ref()
                        .map(|used| format!("; {used} used"))
                        .unwrap_or_default()
                ));
            }
            for quota in &data.quotas {
                let absolute = if quota.unit.is_some() {
                    format!(
                        "{} / {} {} used; ",
                        quota.used.as_deref().unwrap_or("?"),
                        quota.limit.as_deref().unwrap_or("?"),
                        quota.unit.as_deref().unwrap_or("units")
                    )
                } else {
                    String::new()
                };
                values.push(format!(
                    "{}: {}{}{}",
                    quota.label,
                    absolute,
                    quota
                        .used_percent
                        .as_ref()
                        .map(|p| format!("{p}% used"))
                        .unwrap_or_else(|| "not available".into()),
                    quota
                        .reset_at
                        .as_ref()
                        .map(|s| format!("; resets {s}"))
                        .unwrap_or_default()
                ));
            }
        }
        if values.is_empty() {
            values.push("not available".into());
        }
        output.push_str(&format!(
            "{:<20} {:<16} {:<25} {}\n",
            row.account,
            row.provider,
            row.state,
            values.join(" | ")
        ));
        if let Some(when) = row.observed_at {
            output.push_str(&format!(
                "  {} · observed {}s ago\n",
                if row.stale {
                    "STALE cached data"
                } else if row.cached {
                    "cached"
                } else {
                    "live"
                },
                now().saturating_sub(when)
            ));
        }
        if let Some(detail) = &row.detail {
            output.push_str(&format!("  {detail}\n"));
        }
        if row.experimental {
            output.push_str("  Experimental account endpoint; schema may change.\n");
        }
    }
    output
}
