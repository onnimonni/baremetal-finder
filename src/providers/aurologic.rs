//! aurologic (DE; Frankfurt, Helsinki, Amsterdam): offer lists embedded as
//! JSON in each tab's `onclick="loadDedicatedServerOffers({...})"`. Prices
//! include 19% German VAT. No stock signal (the instant-stock search needs
//! a reCAPTCHA token).

use anyhow::Result;
use scraper::{Html, Selector};
use serde_json::Value;

use crate::model::{Offer, Stock};
use crate::util::{first_num, get_text, parse_storage};

const URL: &str = "https://aurologic.com/dedicated-server/list/";
const VAT: f64 = 1.19;

pub async fn fetch(client: &reqwest::Client) -> Result<Vec<Offer>> {
    let html = get_text(client, URL).await?;
    let doc = Html::parse_document(&html);
    let tabs = Selector::parse("[onclick*='loadDedicatedServerOffers(']")
        .expect("static CSS selector is valid");
    let mut out = Vec::new();
    for t in doc.select(&tabs) {
        let js = t.value().attr("onclick").unwrap_or_default();
        let Some(json) = js
            .split_once("loadDedicatedServerOffers(")
            .and_then(|(_, r)| r.rsplit_once(')'))
            .map(|(j, _)| j)
        else {
            continue;
        };
        let Ok(v) = serde_json::from_str::<Value>(json) else {
            continue;
        };
        for (i, s) in v["offers"].as_array().into_iter().flatten().enumerate() {
            let Some(price) = s["monthly"].as_f64() else {
                continue;
            };
            let specs = s["cpu_specs"].as_str().unwrap_or_default();
            let disk = s["disk"].as_str().unwrap_or_default();
            let mem = s["memory"].as_str().unwrap_or_default();
            let mut o = Offer::new(
                "aurologic",
                s["orderUuid"]
                    .as_str()
                    .map_or_else(|| format!("{}-{i}", v["id"]), String::from),
            );
            o.name = format!("{} {}", s["cpu"].as_str().unwrap_or_default(), mem);
            o.cpu = s["cpu"].as_str().unwrap_or_default().to_string();
            // "12 Cores, 24 Threads @ 3.7 GHz"
            o.cores = first_num(specs).map(|c| c as u32);
            o.threads = specs
                .split_once(", ")
                .and_then(|(_, t)| first_num(t))
                .map(|t| t as u32);
            o.ram_gb = first_num(mem);
            (o.storage_gb, o.nvme) = parse_storage(disk);
            o.storage = disk.to_string();
            o.port_mbps = s["uplink"].as_f64().map(|g| (g * 1000.0) as u32);
            o.location = s["location"]["name"]
                .as_str()
                .unwrap_or_default()
                .to_string();
            o.country = s["location"]["iso"]
                .as_str()
                .unwrap_or_default()
                .to_uppercase();
            o.price_monthly = price / VAT;
            o.setup_fee = s["onetime"].as_f64().unwrap_or(0.0) / VAT;
            o.stock = Stock::Unknown;
            o.spec_text = mem.to_string();
            o.url = URL.into();
            out.push(o);
        }
    }
    anyhow::ensure!(!out.is_empty(), "aurologic: no offers");
    Ok(out)
}
