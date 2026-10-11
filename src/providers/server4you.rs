//! server4you: plans are in the Next.js flight data of the listing page;
//! each variant's order page returns HTTP 500 "Derzeit ausverkauft" when
//! sold out.

use std::sync::LazyLock;

use anyhow::{Context, Result};
use regex::Regex;
use serde_json::Value;

use crate::model::{Offer, Stock};
use crate::util::{first_num, get_text, parse_storage};

const URL: &str = "https://server4you.com/de/dedicated-server/";

pub async fn fetch(client: &reqwest::Client) -> Result<Vec<Offer>> {
    static PUSH: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r#"self\.__next_f\.push\(\[1,("(?:[^"\\]|\\.)*")\]\)"#)
            .expect("static regex is valid")
    });
    let html = get_text(client, URL).await?;
    let flight: String = PUSH
        .captures_iter(&html)
        .filter_map(|c| serde_json::from_str::<String>(&c[1]).ok())
        .collect();
    let start = flight
        .find(r#""servers":["#)
        .context("server4you: no servers array")?
        + r#""servers":"#.len();
    let servers: Value =
        serde_json::from_str(balanced(&flight[start..]).context("server4you: unbalanced JSON")?)?;

    let mut out = Vec::new();
    for s in servers.as_array().into_iter().flatten() {
        let cpu = crate::util::clean(s["Processor"].as_str().unwrap_or_default());
        let details = s["ProcessorDetails"].as_str().unwrap_or_default();
        let dcs: Vec<&str> = s["DataCenter"]
            .as_str()
            .unwrap_or_default()
            .split(',')
            .map(str::trim)
            .collect();
        for v in s["Variants"].as_array().into_iter().flatten() {
            let Some(price) = v["price"]
                .as_array()
                .and_then(|a| a.iter().find_map(|p| p["eur"].as_str()))
                .and_then(first_num)
            else {
                continue;
            };
            let order = v["url"][0]["de"].as_str().unwrap_or_default();
            // Sold-out variants answer 500 with "Derzeit ausverkauft!".
            let stock = match client.get(order).send().await {
                Ok(r) if r.status().is_success() => Stock::InStock,
                Ok(r) if r.status().as_u16() == 500 => Stock::OutOfStock,
                _ => Stock::Unknown,
            };
            let storage = v["storage"].as_str().unwrap_or_default();
            for dc in &dcs {
                let (country, city) = match *dc {
                    "us" => ("US", "St. Louis"),
                    _ => ("FR", "Strasbourg"),
                };
                let mut o = Offer::new(
                    "server4you",
                    format!("{}@{dc}", order.rsplit('/').next().unwrap_or(order)),
                );
                o.name = s["Title"].as_str().unwrap_or_default().to_string();
                o.cpu = cpu.clone();
                o.cores = first_num(details).map(|c| c as u32);
                o.threads = details
                    .split(',')
                    .nth(1)
                    .and_then(first_num)
                    .map(|t| t as u32);
                o.ram_gb = v["ram"].as_str().and_then(first_num);
                (o.storage_gb, o.nvme) = parse_storage(storage);
                o.storage = storage.to_string();
                o.port_mbps = s["Uplink"]
                    .as_str()
                    .and_then(first_num)
                    .map(|g| (g * 1000.0) as u32);
                o.location = city.into();
                o.country = country.into();
                o.price_monthly = price;
                o.stock = stock;
                o.url = order.to_string();
                o.note = Some("price as listed (may include VAT)".into());
                out.push(o);
            }
        }
    }
    Ok(out)
}

/// The JSON value at the start of `s` (array/object), by bracket matching
/// that skips string contents.
fn balanced(s: &str) -> Option<&str> {
    let (mut depth, mut in_str, mut esc) = (0i32, false, false);
    for (i, ch) in s.char_indices() {
        if in_str {
            match (esc, ch) {
                (true, _) => esc = false,
                (false, '\\') => esc = true,
                (false, '"') => in_str = false,
                _ => {}
            }
            continue;
        }
        match ch {
            '"' => in_str = true,
            '[' | '{' => depth += 1,
            ']' | '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(&s[..=i]);
                }
            }
            _ => {}
        }
    }
    None
}
