//! ZAP-Hosting (Frankfurt/Eygelshoven): dedicated "blades" as HTML-escaped
//! JSON in the shop page, with a stock count per configuration.

use std::sync::LazyLock;

use anyhow::{Context, Result};
use regex::Regex;
use scraper::Html;
use serde_json::Value;

use crate::model::{Offer, Stock};
use crate::util::get_text;

const URL: &str = "https://zap-hosting.com/en/shop/product/dedicated-server/";

pub async fn fetch(client: &reqwest::Client) -> Result<Vec<Offer>> {
    static BLADES: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r#":blades='([^']*)'"#).expect("static regex is valid"));
    let html = get_text(client, URL).await?;
    let raw = BLADES.captures(&html).context("zap: no blades attribute")?[1].to_string();
    // Attribute value is HTML-escaped JSON.
    let json: String = Html::parse_fragment(&format!("<p>{raw}</p>"))
        .root_element()
        .text()
        .collect();
    let blades: Vec<Value> = serde_json::from_str(&json).context("zap: blades JSON")?;
    let mut out = Vec::new();
    for b in &blades {
        let bl = &b["blade"];
        let Some(net) = b["price"]["net"].as_f64() else {
            continue;
        };
        let count = bl["cpuAmount"].as_u64().unwrap_or(1) as u32;
        let qty = b["stock"]["count"].as_i64();
        let disks = bl["diskCount"].as_f64().unwrap_or(1.0);
        let size = bl["diskCapacity"].as_f64().unwrap_or(0.0);
        let mut o = Offer::new("zap", bl["configurationToken"].as_str().unwrap_or("?"));
        o.name = b["description"].as_str().unwrap_or_default().to_string();
        o.cpu = bl["cpuDisplayName"]
            .as_str()
            .or(bl["cpuModel"].as_str())
            .unwrap_or_default()
            .split(" @")
            .next()
            .unwrap_or_default()
            .to_string();
        o.cpu_count = count;
        o.cores = bl["cpuDisplayCores"].as_u64().map(|c| c as u32 * count);
        o.threads = bl["cpuDisplayThreads"].as_u64().map(|t| t as u32 * count);
        o.ram_gb = bl["memory"].as_f64().map(|mb| mb / 1024.0);
        o.storage = format!("{disks}x {size}GB SSD");
        o.storage_gb = Some(disks * size);
        o.nvme = Some(false);
        o.port_mbps = bl["cpuDisplayNicSpeed"]
            .as_str()
            .and_then(crate::util::first_num)
            .map(|g| (g * 1000.0) as u32);
        o.location = bl["site"]["site_displayname"]
            .as_str()
            .unwrap_or("Frankfurt")
            .to_string();
        o.country = "DE".into();
        o.price_monthly = net;
        o.stock_qty = qty;
        o.stock = match qty {
            Some(n) if n > 0 => Stock::InStock,
            Some(_) => Stock::OutOfStock,
            None => Stock::Unknown,
        };
        o.availability = qty.filter(|n| *n == 0).map(|_| "~5 days".into());
        o.spec_text = bl["cpuDisplayMemoryType"]
            .as_str()
            .unwrap_or_default()
            .to_string();
        o.url = URL.into();
        out.push(o);
    }
    Ok(out)
}
