//! Nexril (Dallas): schema.org Product blocks with InStock / OutOfStock.

use anyhow::Result;
use scraper::{Html, Selector};
use serde_json::Value;

use crate::model::{Offer, Stock};
use crate::util::{first_num, get_text, parse_storage};

const URL: &str = "https://nexril.net/dedicated";

pub async fn fetch(client: &reqwest::Client) -> Result<Vec<Offer>> {
    let html = get_text(client, URL).await?;
    let doc = Html::parse_document(&html);
    let ld = Selector::parse(r#"script[type="application/ld+json"]"#)
        .expect("static CSS selector is valid");
    let mut out = Vec::new();
    for script in doc.select(&ld) {
        let Ok(p) = serde_json::from_str::<Value>(&script.inner_html()) else {
            continue;
        };
        if p["@type"] != "Product" {
            continue;
        }
        let offer = if p["offers"].is_array() {
            &p["offers"][0]
        } else {
            &p["offers"]
        };
        let Some(price) = offer["price"]
            .as_str()
            .and_then(first_num)
            .or(offer["price"].as_f64())
        else {
            continue;
        };
        // "Dual Xeon X5675 @ 3.06GHz, 96GB DDR3 ECC RAM, 250GB HDD, 30TB Bandwidth, 1 Gbps Port, …"
        let desc = p["description"].as_str().unwrap_or_default();
        let parts: Vec<&str> = desc.split(", ").collect();
        let cpu = parts
            .first()
            .and_then(|c| c.split(" @").next())
            .unwrap_or_default();
        let (count, cpu) = match cpu.strip_prefix("Dual ") {
            Some(c) => (2, c),
            None => (1, cpu),
        };
        let vendor = if cpu.contains("EPYC") || cpu.contains("Ryzen") {
            "AMD"
        } else {
            "Intel"
        };
        let disk = parts.get(2).copied().unwrap_or_default();
        let mut o = Offer::new("nexril", p["sku"].as_str().unwrap_or_default().to_string());
        o.name = p["name"].as_str().unwrap_or_default().to_string();
        o.cpu = format!("{vendor} {cpu}");
        o.cpu_count = count;
        o.ram_gb = parts.get(1).and_then(|r| first_num(r));
        (o.storage_gb, o.nvme) = parse_storage(disk);
        o.storage = disk.to_string();
        o.port_mbps = parts
            .iter()
            .find(|p| p.contains("Port"))
            .and_then(|p| first_num(p))
            .map(|g| (g * 1000.0) as u32);
        o.traffic = parts
            .iter()
            .find(|p| p.contains("Bandwidth"))
            .map(|t| t.to_string());
        o.location = "Dallas".into();
        o.country = "US".into();
        o.price_monthly = price;
        o.currency = "USD";
        o.stock = match offer["availability"].as_str() {
            Some(a) if a.ends_with("InStock") => Stock::InStock,
            Some(_) => Stock::OutOfStock,
            None => Stock::Unknown,
        };
        o.spec_text = parts.get(1).copied().unwrap_or_default().to_string();
        o.url = offer["url"].as_str().unwrap_or(URL).to_string();
        out.push(o);
    }
    anyhow::ensure!(!out.is_empty(), "nexril: no products");
    Ok(out)
}
