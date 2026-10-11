//! servers.com "Scalable Bare Metal": inline `const flavor_list = {...}` on
//! the product page. Hourly prices only (monthly = 730 h) and no stock signal.

use std::sync::LazyLock;

use anyhow::{Context, Result};
use regex::Regex;
use serde_json::Value;

use crate::model::{Offer, Stock};
use crate::util::{currency, get_text, parse_num, parse_storage};

const URL: &str = "https://www.servers.com/products/scalable-bare-metal";
const HOURS_PER_MONTH: f64 = 730.0;

pub async fn fetch(client: &reqwest::Client) -> Result<Vec<Offer>> {
    static LIST: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"(?s)const flavor_list = (\{.*?\});").unwrap());
    let html = get_text(client, URL).await?;
    let json = LIST
        .captures(&html)
        .context("servers.com: flavor_list not found")?;
    let v: Value = serde_json::from_str(&json[1])?;
    let mut out = Vec::new();
    for loc in v["locations"].as_array().into_iter().flatten() {
        let name = loc["location_name"].as_str().unwrap_or_default().trim();
        let code = loc["location_code"].as_str().unwrap_or_default();
        let Some(cur) = loc["currency"].as_str().and_then(currency) else {
            continue;
        };
        let country = if name.contains("Netherlands") {
            "NL"
        } else if name.contains("USA") {
            "US"
        } else {
            ""
        };
        for f in loc["flavors"].as_array().into_iter().flatten() {
            let Some(hourly) = f["price_per_hour"].as_str().and_then(parse_num) else {
                continue;
            };
            let drives = f["drives_configuration"].as_str().unwrap_or_default();
            let count = f["cpu_count"].as_u64().unwrap_or(1) as u32;
            let mut o = Offer::new("serverscom", format!("{}@{code}", f["id"]));
            o.name = f["name"].as_str().unwrap_or_default().to_string();
            o.cpu = f["cpu_name"].as_str().unwrap_or_default().to_string();
            o.cpu_count = count;
            o.cores = f["cpu_cores_count"].as_u64().map(|c| c as u32 * count);
            o.threads = f["cpu_threads"].as_u64().map(|t| t as u32 * count);
            o.ram_gb = f["ram_size"].as_f64();
            (o.storage_gb, o.nvme) = parse_storage(drives);
            o.storage = drives.to_string();
            o.location = name.to_string();
            o.country = country.into();
            o.price_monthly = hourly * HOURS_PER_MONTH;
            o.currency = cur;
            o.stock = Stock::Unknown;
            o.url = URL.into();
            o.note = Some(format!("hourly {hourly} {cur} x 730h; no stock signal"));
            out.push(o);
        }
    }
    Ok(out)
}
