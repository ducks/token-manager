use crate::provider::Provider;
use anyhow::{Context, Result, ensure};
use reqwest::{blocking::Client, redirect::Policy};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{io::Read, str::FromStr, time::Duration};

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PricingMap {
    pub path: String,
    pub source: String,
    pub rows: String,
    pub id: String,
    pub name: String,
    pub input: String,
    pub output: String,
}
impl PricingMap {
    pub fn validate(&self, provider: &Provider, local: bool) -> Result<()> {
        let mut catalog = provider.clone();
        catalog.path = self.path.clone();
        catalog.endpoint(local)?;
        for pointer in [&self.rows, &self.id, &self.name, &self.input, &self.output] {
            ensure!(
                pointer.starts_with('/'),
                "pricing mappings require JSON pointers"
            );
        }
        let source = reqwest::Url::parse(&self.source)?;
        ensure!(
            source.scheme() == "https"
                && source.username().is_empty()
                && source.password().is_none(),
            "pricing source must be HTTPS without credentials"
        );
        Ok(())
    }
}
#[derive(Clone, Copy, clap::ValueEnum, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Sort {
    Input,
    Output,
}
#[derive(Serialize)]
pub struct ModelPrice {
    pub id: String,
    pub name: String,
    pub input_per_million: Option<String>,
    pub output_per_million: Option<String>,
}
#[derive(Serialize)]
pub struct Report {
    pub provider: String,
    pub source: String,
    pub observed_at: u64,
    pub currency: &'static str,
    pub sort: Sort,
    pub models: Vec<ModelPrice>,
}
fn rate(row: &Value, pointer: &str) -> Option<String> {
    let value = row.pointer(pointer)?;
    let text = match value {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        _ => return None,
    };
    let rate = Decimal::from_str(&text).ok()?;
    if rate.is_sign_negative() {
        return None;
    }
    Some(
        rate.checked_mul(Decimal::from(1_000_000))?
            .normalize()
            .to_string(),
    )
}
fn normalized(s: &str) -> String {
    s.to_lowercase()
        .chars()
        .filter(|c| c.is_alphanumeric())
        .collect()
}
fn text(row: &Value, pointer: &str) -> Result<String> {
    let s = row
        .pointer(pointer)
        .and_then(Value::as_str)
        .context("Catalog model identity missing")?;
    ensure!(
        !s.is_empty() && s.len() <= 512 && !s.chars().any(char::is_control),
        "Invalid catalog model identity"
    );
    Ok(s.to_owned())
}
pub fn extract(
    value: &Value,
    map: &PricingMap,
    query: &str,
    sort: Sort,
) -> Result<Vec<ModelPrice>> {
    let query = normalized(query);
    ensure!(!query.is_empty(), "Enter a model or family name");
    let rows = value
        .pointer(&map.rows)
        .and_then(Value::as_array)
        .context("Catalog response does not match pricing mapping")?;
    let mut models = Vec::new();
    for row in rows {
        let id = text(row, &map.id)?;
        let name = text(row, &map.name)?;
        if normalized(&id).contains(&query) || normalized(&name).contains(&query) {
            models.push(ModelPrice {
                id,
                name,
                input_per_million: rate(row, &map.input),
                output_per_million: rate(row, &map.output),
            });
        }
    }
    // An exact model ID, short slug, or display name takes precedence over family matching.
    let exact = |m: &ModelPrice| {
        normalized(&m.id) == query
            || normalized(m.id.rsplit('/').next().unwrap_or(&m.id)) == query
            || normalized(&m.name) == query
    };
    if models.iter().any(exact) {
        models.retain(exact);
    }
    let key = |m: &ModelPrice| {
        match sort {
            Sort::Input => &m.input_per_million,
            Sort::Output => &m.output_per_million,
        }
        .as_ref()
        .and_then(|s| Decimal::from_str(s).ok())
    };
    models.sort_by(|a, b| key(b).cmp(&key(a)).then_with(|| a.id.cmp(&b.id)));
    ensure!(
        !models.is_empty(),
        "No models matched; try a family name or catalog model ID"
    );
    Ok(models)
}
pub fn fetch(provider: &Provider, query: &str, sort: Sort, local: bool) -> Result<Report> {
    ensure!(
        !normalized(query).is_empty(),
        "Enter a model or family name"
    );
    let map = provider
        .pricing
        .as_ref()
        .context("This provider has no public pricing catalog mapping")?;
    map.validate(provider, local)?;
    let mut catalog = provider.clone();
    catalog.path = map.path.clone();
    let client = Client::builder()
        .timeout(Duration::from_secs(15))
        .connect_timeout(Duration::from_secs(3))
        .redirect(Policy::none())
        .build()?;
    let response = client
        .get(catalog.endpoint(local)?)
        .send()
        .map_err(|_| anyhow::anyhow!("Pricing catalog request failed or timed out"))?;
    ensure!(
        response.status().is_success(),
        "Pricing catalog returned HTTP {}",
        response.status().as_u16()
    );
    let mut body = Vec::new();
    response
        .take(8 * 1024 * 1024 + 1)
        .read_to_end(&mut body)
        .context("Could not read pricing catalog")?;
    ensure!(
        body.len() <= 8 * 1024 * 1024,
        "Pricing catalog exceeds size limit"
    );
    let value = serde_json::from_slice(&body)
        .map_err(|_| anyhow::anyhow!("Pricing catalog did not return valid JSON"))?;
    Ok(Report {
        provider: provider.id.clone(),
        source: map.source.clone(),
        observed_at: crate::state::now(),
        currency: "USD",
        sort,
        models: extract(&value, map, query, sort)?,
    })
}
pub fn table(report: &Report) -> String {
    let sort = match report.sort {
        Sort::Input => "input",
        Sort::Output => "output",
    };
    let mut out = format!(
        "{} catalog — USD / 1M tokens; highest {sort} price first\n",
        report.provider
    );
    out.push_str(&format!(
        "{:<55} {:>16} {:>16}\n",
        "MODEL", "INPUT", "OUTPUT"
    ));
    for m in &report.models {
        out.push_str(&format!(
            "{:<55} {:>16} {:>16}\n",
            m.id,
            m.input_per_million.as_deref().unwrap_or("not available"),
            m.output_per_million.as_deref().unwrap_or("not available")
        ));
    }
    out.push_str(&format!("\nSource: {}\nBase catalog token rates; caching, context tiers and other charges may differ.\n", report.source));
    out
}
