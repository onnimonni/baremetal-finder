//! Primcast (Amsterdam, Bucharest, Miami, New York, San Francisco): ready
//! servers API, one row per physical in-stock machine. The API returns at
//! most 40 rows sorted by price, so each location is paged by raising
//! `minprice` to the last price seen. Rows carry PassMark-style benchmarks.

use std::collections::{BTreeMap, HashSet};
use std::sync::LazyLock;

use anyhow::Result;
use regex::Regex;
use serde_json::Value;

use crate::model::{Offer, Stock};
use crate::util::{first_num, get_json, parse_storage};

const API: &str = "https://primcast.com/api/llm/readyservers/location";
/// (API slug, country, city)
const LOCATIONS: &[(&str, &str, &str)] = &[
    ("amsterdam", "NL", "Amsterdam"),
    ("bucharest", "RO", "Bucharest"),
    ("miami", "US", "Miami"),
    ("nyc", "US", "New York"),
    ("sf", "US", "San Francisco"),
];

pub async fn fetch(client: &reqwest::Client) -> Result<Vec<Offer>> {
    // "… E3-1284L v4 Quad Core 2.90 GHz", "… Dodeca Core …" -> model only
    static CORE_WORD: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"(?i)\s+\S+[- ]cores?\b.*$").expect("static regex is valid"));
    let mut out = Vec::new();
    for (slug, country, city) in LOCATIONS {
        let mut seen = HashSet::new();
        let mut rows: Vec<Value> = Vec::new();
        let mut min = 0.0;
        for _ in 0..60 {
            let page = get_json(
                client,
                &format!("{API}/{slug}/minprice/{min}/limit/40/format/json"),
            )
            .await?;
            let page = page.as_array().cloned().unwrap_or_default();
            let n = page.len();
            let fresh: Vec<Value> = page
                .into_iter()
                .filter(|r| r["serverid"].as_i64().is_some_and(|id| seen.insert(id)))
                .collect();
            if fresh.is_empty() {
                break;
            }
            min = fresh
                .iter()
                .filter_map(|r| r["price"].as_f64())
                .fold(min, f64::max);
            rows.extend(fresh);
            if n < 40 {
                break;
            }
        }
        // Identical machines at the same price -> one offer with a count.
        let mut groups: BTreeMap<String, (i64, Value)> = BTreeMap::new();
        for r in rows {
            if r["gpu"]
                .as_str()
                .is_some_and(|g| g.contains("NVIDIA") || g.contains("Radeon"))
            {
                continue;
            }
            let key = format!(
                "{}|{}|{}|{}|{}",
                r["cpu"], r["ram"], r["hdd"], r["traffic"], r["price"]
            );
            groups.entry(key).or_insert((0, r.clone())).0 += 1;
        }
        for (qty, r) in groups.into_values() {
            let Some(price) = r["price"].as_f64() else {
                continue;
            };
            let cpu_raw = r["cpu"].as_str().unwrap_or_default();
            let (count, cpu) = match cpu_raw
                .strip_prefix("2x ")
                .or_else(|| cpu_raw.strip_prefix("Dual "))
            {
                Some(c) => (2, c),
                None => (1, cpu_raw),
            };
            let cpu = CORE_WORD.replace(cpu, "").trim().to_string();
            let hdd = r["hdd"].as_str().unwrap_or_default();
            let traffic = r["traffic"].as_str().unwrap_or_default();
            let mut o = Offer::new("primcast", r["serverid"].to_string());
            o.name = r["name"].as_str().unwrap_or_default().to_string();
            o.cpu = cpu;
            o.cpu_count = count;
            o.ram_gb = r["ram"].as_f64();
            (o.storage_gb, o.nvme) = parse_storage(hdd);
            o.storage = hdd.to_string();
            o.port_mbps = first_num(traffic).map(|n| if traffic.contains("Gbps") { n * 1000.0 } else { n } as u32);
            o.traffic = Some(traffic.to_string());
            o.location = city.to_string();
            o.country = country.to_string();
            o.price_monthly = price;
            o.currency = "USD";
            o.stock = Stock::InStock;
            o.stock_qty = Some(qty);
            o.provider_cpu_mark = r["benchmark"]["multi"].as_f64().filter(|m| *m > 0.0);
            o.url = r["order_url"]
                .as_str()
                .unwrap_or("https://primcast.com/")
                .to_string();
            if let Some(list) = r["listprice"].as_f64().filter(|l| *l > price) {
                o.note = Some(format!("discounted from ${list}"));
            }
            out.push(o);
        }
    }
    Ok(out)
}
