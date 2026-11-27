//! Cross-provider pricing from explicit canonical model identities, never fuzzy joins.
use crate::prices::Sort;
use anyhow::{Context, Result, ensure};
use reqwest::{Url, blocking::Client, redirect::Policy};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::Read,
    path::Path,
    str::FromStr,
    time::Duration,
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CatalogMap {
    pub url: String,
    pub source: String,
    pub providers: String,
    pub models: String,
    pub provider_models: String,
    pub canonical_id: String,
    pub cost: String,
    #[serde(default)]
    pub direct_providers: BTreeMap<String, String>,
    #[serde(default)]
    pub vendor_aliases: BTreeMap<String, String>,
}
impl CatalogMap {
    pub fn load(root: &Path) -> Result<Self> {
        let path = root.join("providers/catalogs/models-dev.toml");
        let text = if path.exists() {
            ensure!(
                path.is_file() && !path.is_symlink(),
                "Catalog definition must be a regular file"
            );
            fs::read_to_string(path).context("Could not read catalog definition")?
        } else {
            include_str!("../providers/catalogs/models-dev.toml").to_owned()
        };
        toml::from_str(&text).context("Invalid catalog definition")
    }
    pub fn endpoint(&self, local: bool) -> Result<Url> {
        let url = Url::parse(&self.url).context("Invalid pricing catalog URL")?;
        ensure!(
            url.username().is_empty() && url.password().is_none() && url.fragment().is_none(),
            "Catalog URL must not contain credentials or a fragment"
        );
        ensure!(
            url.scheme() == "https"
                || (local
                    && url.scheme() == "http"
                    && matches!(url.host_str(), Some("127.0.0.1" | "[::1]"))),
            "Catalog requires HTTPS; loopback fixtures need --allow-local-http"
        );
        for pointer in [
            &self.providers,
            &self.models,
            &self.provider_models,
            &self.canonical_id,
            &self.cost,
        ] {
            ensure!(
                pointer.starts_with('/'),
                "Catalog mapping requires JSON pointers"
            );
        }
        for (k, v) in self
            .direct_providers
            .iter()
            .chain(self.vendor_aliases.iter())
        {
            ensure!(
                namespace(k) && namespace(v),
                "Invalid catalog vendor mapping"
            );
        }
        ensure!(
            safe_url(&self.source).is_some(),
            "Catalog source must be an HTTPS URL without credentials"
        );
        Ok(url)
    }
}
#[derive(Default)]
pub struct Filter<'a> {
    pub query: Option<&'a str>,
    pub vendor: Option<&'a str>,
    pub provider: Option<&'a str>,
}
#[derive(Serialize)]
pub struct Rates {
    pub input_per_million: Option<String>,
    pub output_per_million: Option<String>,
    pub cache_read_per_million: Option<String>,
    pub cache_write_per_million: Option<String>,
}
#[derive(Serialize)]
pub struct Tier {
    pub context_over: u64,
    pub rates: Rates,
}
#[derive(Serialize)]
pub struct Offer {
    pub provider: String,
    pub provider_name: String,
    pub model_id: String,
    pub direct: bool,
    pub rates: Rates,
    pub tiers: Vec<Tier>,
    pub status: Option<String>,
    pub note: Option<String>,
    pub provider_url: Option<String>,
    pub catalog_model_updated: Option<String>,
}
#[derive(Serialize)]
pub struct Model {
    pub id: String,
    pub name: String,
    pub vendor: Option<String>,
    pub family: Option<String>,
    pub offers: Vec<Offer>,
}
#[derive(Serialize)]
pub struct Report {
    pub source: String,
    pub observed_at: u64,
    pub currency: &'static str,
    pub sort: Sort,
    pub unlinked_offers: usize,
    pub models: Vec<Model>,
}
fn norm(s: &str) -> String {
    s.to_lowercase()
        .chars()
        .filter(|c| c.is_alphanumeric())
        .collect()
}
fn clean(s: &str) -> bool {
    !s.is_empty() && s.len() <= 512 && !s.chars().any(char::is_control)
}
fn label(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .filter(|s| clean(s))
        .map(str::to_owned)
}
fn safe_url(s: &str) -> Option<String> {
    let url = Url::parse(s).ok()?;
    (clean(s) && url.scheme() == "https" && url.username().is_empty() && url.password().is_none())
        .then(|| s.to_owned())
}
fn rate(value: &Value, key: &str) -> Option<String> {
    let raw = value.get(key)?;
    let text = match raw {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        _ => return None,
    };
    let number = Decimal::from_str(&text)
        .or_else(|_| Decimal::from_scientific(&text))
        .ok()?;
    (!number.is_sign_negative()).then(|| number.normalize().to_string())
}
fn rates(cost: &Value, text_tokens: bool) -> Rates {
    let get = |k| if text_tokens { rate(cost, k) } else { None };
    Rates {
        input_per_million: get("input"),
        output_per_million: get("output"),
        cache_read_per_million: get("cache_read"),
        cache_write_per_million: get("cache_write"),
    }
}
fn price(rates: &Rates, sort: Sort) -> Option<Decimal> {
    match sort {
        Sort::Input => &rates.input_per_million,
        Sort::Output => &rates.output_per_million,
    }
    .as_deref()
    .and_then(|s| Decimal::from_str(s).ok())
}
fn namespace(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
}
fn canonical(value: &str) -> bool {
    value
        .split_once('/')
        .is_some_and(|(vendor, model)| namespace(vendor) && clean(model))
}
pub fn extract(value: &Value, map: &CatalogMap, filter: &Filter<'_>, sort: Sort) -> Result<Report> {
    for s in [filter.query, filter.vendor, filter.provider]
        .into_iter()
        .flatten()
    {
        ensure!(
            !norm(s).is_empty(),
            "Model, vendor and provider filters cannot be empty"
        );
    }
    ensure!(
        filter.query.is_some() || filter.vendor.is_some() || filter.provider.is_some(),
        "Specify a model/family, --vendor, or --provider"
    );
    let providers = value
        .pointer(&map.providers)
        .and_then(Value::as_object)
        .context("Pricing catalog provider map missing")?;
    let metadata = value
        .pointer(&map.models)
        .and_then(Value::as_object)
        .context("Pricing catalog model map missing")?;
    let vendors: BTreeSet<&str> = metadata
        .keys()
        .filter_map(|id| id.split_once('/').map(|(v, _)| v))
        .collect();
    let requested_vendor = filter.vendor.map(|v| {
        map.vendor_aliases
            .iter()
            .find(|(k, _)| norm(k) == norm(v))
            .map(|(_, v)| v.clone())
            .unwrap_or_else(|| v.to_lowercase())
    });
    let mut groups: BTreeMap<String, Model> = BTreeMap::new();
    let mut unlinked_offers = 0;
    for (provider_id, provider) in providers {
        ensure!(namespace(provider_id), "Invalid pricing provider identity");
        let provider_name = label(provider, "name").unwrap_or_else(|| provider_id.clone());
        if filter
            .provider
            .is_some_and(|p| norm(p) != norm(provider_id) && norm(p) != norm(&provider_name))
        {
            continue;
        }
        let models = provider
            .pointer(&map.provider_models)
            .and_then(Value::as_object)
            .context("Pricing catalog provider models missing")?;
        let direct_vendor = map
            .direct_providers
            .get(provider_id)
            .map(String::as_str)
            .or_else(|| {
                vendors
                    .contains(provider_id.as_str())
                    .then_some(provider_id.as_str())
            });
        for (model_id, offering) in models {
            ensure!(clean(model_id), "Invalid pricing model identity");
            let linked = match offering.pointer(&map.canonical_id) {
                Some(Value::String(id)) => {
                    ensure!(canonical(id), "Invalid canonical model identity");
                    Some(id.clone())
                }
                None | Some(Value::Null) => None,
                _ => anyhow::bail!("Invalid canonical model identity"),
            };
            // Only exact catalog identities or a known first-party namespace are joined.
            let linked = linked
                .or_else(|| metadata.contains_key(model_id).then(|| model_id.clone()))
                .or_else(|| {
                    vendors.contains(provider_id.as_str()).then(|| {
                        if model_id.starts_with(&format!("{provider_id}/")) {
                            model_id.clone()
                        } else {
                            format!("{provider_id}/{model_id}")
                        }
                    })
                });
            let vendor = linked
                .as_deref()
                .and_then(|id| id.split_once('/').map(|(v, _)| v.to_owned()));
            if vendor.is_none() {
                unlinked_offers += 1;
            }
            if requested_vendor
                .as_ref()
                .is_some_and(|v| vendor.as_ref().is_none_or(|actual| norm(v) != norm(actual)))
            {
                continue;
            }
            let id = linked
                .clone()
                .unwrap_or_else(|| format!("unlinked:{provider_id}/{model_id}"));
            let meta = linked.as_ref().and_then(|id| metadata.get(id));
            let name = meta
                .and_then(|m| label(m, "name"))
                .or_else(|| label(offering, "name"))
                .unwrap_or_else(|| model_id.clone());
            let family = meta
                .and_then(|m| label(m, "family"))
                .or_else(|| label(offering, "family"));
            let outputs = offering
                .pointer("/modalities/output")
                .and_then(Value::as_array);
            let text_tokens = outputs
                .is_some_and(|a| !a.is_empty() && a.iter().all(|s| s.as_str() == Some("text")));
            let cost = offering.pointer(&map.cost).unwrap_or(&Value::Null);
            let mut note = (!text_tokens).then(|| {
                "Non-text or unspecified output: token rates unavailable; see provider pricing"
                    .to_owned()
            });
            let mut tiers = Vec::new();
            if let Some(raw) = cost.get("tiers") {
                let raw = raw.as_array().context("Invalid pricing tiers")?;
                for tier in raw {
                    let threshold = tier
                        .pointer("/tier/size")
                        .and_then(Value::as_u64)
                        .context("Context tier threshold missing")?;
                    ensure!(
                        tier.pointer("/tier/type")
                            .and_then(Value::as_str)
                            .is_none_or(|v| v == "context"),
                        "Unsupported pricing tier type"
                    );
                    ensure!(
                        !tiers.iter().any(|t: &Tier| t.context_over == threshold),
                        "Duplicate context tier"
                    );
                    tiers.push(Tier {
                        context_over: threshold,
                        rates: rates(tier, text_tokens),
                    });
                }
            } else if let Some(tier) = cost.get("context_over_200k") {
                tiers.push(Tier {
                    context_over: 200_000,
                    rates: rates(tier, text_tokens),
                });
            }
            tiers.sort_by_key(|t| t.context_over);
            if offering.pointer("/experimental/modes").is_some() {
                let extra = "Mode-specific prices may differ; see provider pricing";
                note = Some(
                    note.map(|n| format!("{n}; {extra}"))
                        .unwrap_or_else(|| extra.into()),
                );
            }
            let direct = vendor
                .as_deref()
                .is_some_and(|v| v == provider_id || direct_vendor == Some(v));
            let group = groups.entry(id.clone()).or_insert_with(|| Model {
                id,
                name,
                vendor,
                family,
                offers: Vec::new(),
            });
            group.offers.push(Offer {
                provider: provider_id.clone(),
                provider_name: provider_name.clone(),
                model_id: model_id.clone(),
                direct,
                rates: rates(cost, text_tokens),
                tiers,
                status: label(offering, "status"),
                note,
                provider_url: provider
                    .get("doc")
                    .and_then(Value::as_str)
                    .and_then(safe_url),
                catalog_model_updated: label(offering, "last_updated"),
            });
        }
    }
    let mut models: Vec<Model> = groups.into_values().collect();
    if let Some(query) = filter.query {
        let q = norm(query);
        let exact = |m: &Model| {
            norm(&m.id) == q
                || norm(m.id.split_once('/').map(|(_, s)| s).unwrap_or(&m.id)) == q
                || norm(&m.name) == q
                || m.offers.iter().any(|o| norm(&o.model_id) == q)
        };
        if models.iter().any(exact) {
            models.retain(exact);
        } else {
            models.retain(|m| {
                norm(&m.id).contains(&q)
                    || norm(&m.name).contains(&q)
                    || m.family.as_ref().is_some_and(|f| norm(f).contains(&q))
                    || m.offers.iter().any(|o| norm(&o.model_id).contains(&q))
            });
        }
    }
    for m in &mut models {
        let vendor = m.vendor.as_deref().unwrap_or("");
        m.offers.sort_by(|a, b| {
            (b.provider == vendor)
                .cmp(&(a.provider == vendor))
                .then_with(|| b.direct.cmp(&a.direct))
                .then_with(|| price(&b.rates, sort).cmp(&price(&a.rates, sort)))
                .then_with(|| a.provider.cmp(&b.provider))
                .then_with(|| a.model_id.cmp(&b.model_id))
        });
    }
    let key = |m: &Model| m.offers.iter().find_map(|o| price(&o.rates, sort));
    models.sort_by(|a, b| key(b).cmp(&key(a)).then_with(|| a.id.cmp(&b.id)));
    ensure!(
        !models.is_empty(),
        "No models matched the model/vendor/provider filters"
    );
    Ok(Report {
        source: map.source.clone(),
        observed_at: crate::state::now(),
        currency: "USD",
        sort,
        unlinked_offers,
        models,
    })
}
pub fn fetch(map: &CatalogMap, filter: &Filter<'_>, sort: Sort, local: bool) -> Result<Report> {
    let endpoint = map.endpoint(local)?;
    let client = Client::builder()
        .timeout(Duration::from_secs(30))
        .connect_timeout(Duration::from_secs(5))
        .redirect(Policy::none())
        .build()?;
    let response = client
        .get(endpoint)
        .send()
        .map_err(|_| anyhow::anyhow!("Pricing catalog request failed or timed out"))?;
    ensure!(
        response.status().is_success(),
        "Pricing catalog returned HTTP {}",
        response.status().as_u16()
    );
    let mut body = Vec::new();
    response
        .take(32 * 1024 * 1024 + 1)
        .read_to_end(&mut body)
        .context("Could not read pricing catalog")?;
    ensure!(
        body.len() <= 32 * 1024 * 1024,
        "Pricing catalog exceeds size limit"
    );
    let value = serde_json::from_slice(&body)
        .map_err(|_| anyhow::anyhow!("Pricing catalog did not return valid JSON"))?;
    extract(&value, map, filter, sort)
}
fn row(label: &str, rates: &Rates) -> String {
    format!(
        "  {label:<44} {:>12} {:>12} {:>12}\n",
        rates.input_per_million.as_deref().unwrap_or("n/a"),
        rates.output_per_million.as_deref().unwrap_or("n/a"),
        rates.cache_read_per_million.as_deref().unwrap_or("n/a")
    )
}
pub fn table(report: &Report) -> String {
    let mut out = format!(
        "USD / 1M text tokens — direct vendor offers first\nCatalog: {} (fetched at Unix {})\n",
        report.source, report.observed_at
    );
    out.push_str("Listed catalog offers only; rates may lag vendor changes. Subscription fees, non-token charges and discounts are excluded.\n");
    for model in &report.models {
        out.push_str(&format!("\n{} — {}\n", model.id, model.name));
        out.push_str(&format!(
            "  {:<44} {:>12} {:>12} {:>12}\n",
            "PROVIDER / CONTEXT", "INPUT", "OUTPUT", "CACHE READ"
        ));
        if !model.offers.iter().any(|o| o.direct) {
            out.push_str("  Direct vendor offer not available in these results.\n");
        }
        for offer in &model.offers {
            let label = format!(
                "{}{} [{}]",
                offer.provider_name,
                if offer.direct { " (direct)" } else { "" },
                offer.model_id
            );
            out.push_str(&row(&label, &offer.rates));
            if let Some(first) = offer.tiers.first() {
                out.push_str(&format!(
                    "    Base rates above: context <= {} tokens\n",
                    first.context_over
                ));
            }
            for (i, tier) in offer.tiers.iter().enumerate() {
                let upper = offer
                    .tiers
                    .get(i + 1)
                    .map(|t| format!(" to {}", t.context_over))
                    .unwrap_or_default();
                out.push_str(&row(
                    &format!("  context > {}{upper}", tier.context_over),
                    &tier.rates,
                ));
            }
            if let Some(status) = &offer.status {
                out.push_str(&format!("    Status: {status}\n"));
            }
            if let Some(note) = &offer.note {
                out.push_str(&format!("    {note}\n"));
            }
            if let Some(url) = &offer.provider_url {
                out.push_str(&format!("    Provider details: {url}\n"));
            }
        }
    }
    if report.unlinked_offers > 0 {
        out.push_str(&format!("\n{} catalog offers lack a canonical vendor identity; they remain separate and cannot match --vendor.\n",report.unlinked_offers));
    }
    out
}
