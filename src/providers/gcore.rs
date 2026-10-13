//! Gcore dedicated servers (20 countries): pricing calculator JSON. No stock
//! signal. All current models are Intel Xeon. The endpoint drops
//! connections intermittently, so it is retried.

use std::sync::LazyLock;

use anyhow::Result;
use regex::Regex;

use crate::geo::iso_from_name;
use crate::model::{Offer, Stock};
use crate::util::{currency, first_num, get_json, parse_storage};

const URL: &str = "https://gcore.com/api/hosting-calc/dedicated_servers/?format=json";

fn country(name: &str) -> String {
    // One entry spells "Korea" with a Cyrillic "о".
    let name = name.replace('о', "o");
    match name.as_str() {
        "UK" => "GB".into(),
        "USA" => "US".into(),
        "RSA" => "ZA".into(),
        "Korea" => "KR".into(),
        "Hong Kong" => "HK".into(),
        "Turkey" => "TR".into(),
        "Ukraine" => "UA".into(),
        n => iso_from_name(n).unwrap_or_default().into(),
    }
}

pub async fn fetch(client: &reqwest::Client) -> Result<Vec<Offer>> {
    // "2x480SSD + 2x2000SATA" -> "2x480 GB SSD + 2x2000 GB SATA"
    static DISK: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"(\d)([A-Z])").expect("static regex is valid"));
    let mut v = get_json(client, URL).await;
    for _ in 0..3 {
        if v.is_ok() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        v = get_json(client, URL).await;
    }
    let v = v?;
    let mut out = Vec::new();
    for s in v.as_array().into_iter().flatten() {
        if !s["gpu"].is_null() {
            continue;
        }
        let Some(price) = s["server_price"]["monthly"].as_f64() else {
            continue;
        };
        // "2xGold-6248", "E-2388G", "E5-2630v4"
        let raw = s["cpu"].as_str().unwrap_or_default();
        let (count, model) = match raw.split_once('x') {
            Some((n, m)) if n.parse::<u32>().is_ok() => (n.parse().unwrap_or(1), m),
            _ => (1, raw),
        };
        let model = match model.split_once('-') {
            Some((tier @ ("Gold" | "Silver" | "Platinum" | "Bronze"), n)) => format!("{tier} {n}"),
            _ => model.to_string(),
        };
        let disk = DISK
            .replace_all(s["hdd"].as_str().unwrap_or_default(), "$1 GB $2")
            .into_owned();
        let mut o = Offer::new("gcore", s["id"].as_str().unwrap_or_default().to_string());
        o.name = s["name"].as_str().unwrap_or_default().to_string();
        o.cpu = format!("Intel Xeon {model}");
        o.cpu_count = count;
        o.ram_gb = s["ram"].as_str().and_then(first_num);
        (o.storage_gb, o.nvme) = parse_storage(&disk);
        o.storage = disk;
        o.port_mbps = s["connection_speed"]
            .as_str()
            .and_then(first_num)
            .map(|g| (g * 1000.0) as u32);
        o.traffic = s["traffic"].as_str().map(String::from);
        o.location = s["data_center"].as_str().unwrap_or_default().to_string();
        o.country = country(s["country"].as_str().unwrap_or_default());
        o.price_monthly = price;
        o.currency = s["server_price"]["currency"]
            .as_str()
            .and_then(currency)
            .unwrap_or("EUR");
        o.stock = Stock::Unknown;
        o.url = "https://gcore.com/hosting/dedicated".into();
        out.push(o);
    }
    Ok(out)
}
