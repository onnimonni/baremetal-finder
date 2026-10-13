//! Sharktech (US + Amsterdam): `server-data.js` JS object literal with an
//! `inStock` flag per plan.

use std::sync::LazyLock;

use anyhow::{Context, Result};
use regex::Regex;
use serde_json::Value;

use crate::model::{Offer, Stock};
use crate::util::{balanced_json, first_num, get_text};

const SITE: &str = "https://sharktech.net";

/// JS literal -> JSON: quote bare keys, drop trailing commas.
fn js_to_json(js: &str) -> String {
    static KEY: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"(?m)^(\s*)([A-Za-z_]\w*):").expect("static regex is valid"));
    static TRAILING: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r",(\s*[}\]])").expect("static regex is valid"));
    TRAILING
        .replace_all(&KEY.replace_all(js, r#"$1"$2":"#), "$1")
        .into_owned()
}

fn country(city: &str) -> &'static str {
    if city.contains("Amsterdam") {
        "NL"
    } else {
        "US"
    }
}

pub async fn fetch(client: &reqwest::Client) -> Result<Vec<Offer>> {
    static SCRIPT: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r#"/server-data\.js\?v=[\w.-]+"#).expect("static regex is valid")
    });
    let page = get_text(client, &format!("{SITE}/dedicated-servers/")).await?;
    let path = SCRIPT.find(&page).map_or("/server-data.js", |m| m.as_str());
    let js = get_text(client, &format!("{SITE}{path}")).await?;
    let start = js.find('{').context("sharktech: no object")?;
    let obj = balanced_json(&js[start..]).context("sharktech: unbalanced object")?;
    let v: Value = serde_json::from_str(&js_to_json(obj)).context("sharktech: parse")?;
    let mut out = Vec::new();
    for s in v["servers"].as_array().into_iter().flatten() {
        if !s["gpu"].is_null() {
            continue;
        }
        let Some(price) = s["price"]["monthly"].as_f64() else {
            continue;
        };
        let proc = s["processor"].as_str().unwrap_or_default();
        let (count, model) = match proc.strip_prefix("Dual ") {
            Some(m) => (2, m),
            None => (1, proc),
        };
        let vendor = if model.contains("EPYC") || model.contains("Ryzen") {
            "AMD"
        } else {
            "Intel"
        };
        let model = if vendor == "AMD" || model.starts_with("Xeon") || model.starts_with("Core") {
            model.to_string()
        } else {
            format!("Xeon {model}")
        };
        let nvme = s["nvmeDrivesAvailable"]
            .as_str()
            .unwrap_or_default()
            .lines()
            .next()
            .unwrap_or_default();
        let city = s["location"].as_str().unwrap_or_default();
        let net = s["network"].as_str().unwrap_or_default();
        let mut o = Offer::new("sharktech", s["id"].to_string());
        o.name = s["name"].as_str().unwrap_or_default().to_string();
        o.cpu = format!("{vendor} {model}");
        o.cpu_count = count;
        // "36 × 2.1 GHz" is cores × clock.
        o.cores = s["processorSpeed"]
            .as_str()
            .and_then(first_num)
            .map(|t| t as u32);
        o.ram_gb = s["ram"].as_str().and_then(first_num);
        (o.storage_gb, o.nvme) = crate::util::parse_storage(nvme);
        o.storage = nvme.to_string();
        o.port_mbps =
            first_num(net).map(|g| if net.contains("Gbps") { g * 1000.0 } else { g } as u32);
        o.traffic = s["bandwidth"].as_str().map(String::from);
        o.location = city.to_string();
        o.country = country(city).into();
        o.price_monthly = price;
        o.currency = "USD";
        o.stock = if s["inStock"] == true {
            Stock::InStock
        } else {
            Stock::OutOfStock
        };
        o.url = s["link"].as_str().unwrap_or(SITE).to_string();
        out.push(o);
    }
    Ok(out)
}
