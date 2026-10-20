//! Servers Australia: in-stock dedicated servers API (AUD excl. GST).

use anyhow::Result;

use crate::model::{Offer, Stock};
use crate::util::{first_num, get_json, parse_storage};

const API: &str = "https://api.mysau.com.au/v2/ordering/dedicatedserver/available";

pub async fn fetch(client: &reqwest::Client) -> Result<Vec<Offer>> {
    let v = get_json(client, API).await?;
    let mut out = Vec::new();
    for s in v.as_array().into_iter().flatten() {
        let Some(price) = s["price"].as_f64() else {
            continue;
        };
        // "2 x E5-2690 V4 @2.6GHz"
        let raw = s["cpu"].as_str().unwrap_or_default();
        let raw = raw.split(" @").next().unwrap_or(raw);
        let (count, model) = match raw.split_once(" x ") {
            Some((n, m)) if n.trim().parse::<u32>().is_ok() => (n.trim().parse().unwrap_or(1), m),
            _ => (1, raw),
        };
        let cpu = if model.starts_with('E') {
            format!("Intel Xeon {model}")
        } else {
            model.to_string()
        };
        let disk = s["storage"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|d| d.as_str())
            .collect::<Vec<_>>()
            .join(" + ");
        let mut o = Offer::new(
            "serversaustralia",
            s["hash"].as_str().unwrap_or_default().to_string(),
        );
        o.name = format!("{raw} {}", s["ram"].as_str().unwrap_or_default());
        o.cpu = cpu;
        o.cpu_count = count;
        o.cores = s["cpu_cores"].as_u64().map(|c| c as u32);
        o.ram_gb = s["ram"].as_str().and_then(first_num);
        (o.storage_gb, o.nvme) = parse_storage(&disk);
        o.storage = disk;
        o.location = s["location"].as_str().unwrap_or_default().to_string();
        o.country = "AU".into();
        o.price_monthly = price;
        o.currency = "AUD";
        o.stock = Stock::InStock;
        o.url = "https://www.serversaustralia.com.au/dedicated-servers".into();
        out.push(o);
    }
    Ok(out)
}
