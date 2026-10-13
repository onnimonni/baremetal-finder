//! RapidSwitch (formerly Redstation, Maidenhead UK): chassis objects with an
//! `outOfStock` flag in the shop's Next.js flight data. Prices excl. VAT.

use std::sync::LazyLock;

use anyhow::Result;
use regex::Regex;
use serde_json::Value;

use crate::model::{Offer, Stock};
use crate::specs;
use crate::util::{balanced_json, get_text, parse_storage};

const URL: &str = "https://www.rapidswitch.com/shop/fast";

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
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    // Each listing is an object `{"chassis":{…},"outOfStock":bool,…}`.
    for (i, _) in flight.match_indices(r#"{"chassis":{"#) {
        let Some(obj) = balanced_json(&flight[i..]) else {
            continue;
        };
        let Ok(v) = serde_json::from_str::<Value>(obj) else {
            continue;
        };
        let c = &v["chassis"];
        let Some(guid) = c["guid"].as_str() else {
            continue;
        };
        if !seen.insert(guid.to_string()) || c["family"] != "bare-metal" {
            continue;
        }
        let cpu_text = c["cpu"].as_str().unwrap_or_default();
        let s = specs::from_lines(&[cpu_text.to_string()]);
        let hdd = c["hdd"].as_str().unwrap_or_default();
        let location = c["location"].as_str().unwrap_or_default();

        let mut o = Offer::new("rapidswitch", guid);
        o.name = c["name"].as_str().unwrap_or_default().to_string();
        o.cpu = s.cpu;
        o.cpu_count = s.cpu_count;
        o.cores = s.cores;
        o.threads = s.threads;
        o.ram_gb = c["ram"].as_str().and_then(crate::util::first_num);
        (o.storage_gb, o.nvme) = parse_storage(hdd);
        o.storage = hdd.to_string();
        o.location = location.split(',').next().unwrap_or(location).to_string();
        o.country = if c["region"] == "uk" {
            "GB".into()
        } else {
            "US".into()
        };
        o.price_monthly = c["monthlyPence"].as_f64().unwrap_or(0.0) / 100.0;
        o.setup_fee = c["setupPence"].as_f64().unwrap_or(0.0) / 100.0;
        o.currency = "GBP";
        o.stock = if v["outOfStock"] == true {
            Stock::OutOfStock
        } else {
            Stock::InStock
        };
        o.availability = c["deliveryTime"].as_str().map(String::from);
        o.spec_text = format!("{cpu_text} {hdd}");
        o.url = URL.into();
        out.push(o);
    }
    Ok(out)
}
