//! ServerMania (US, Canada, Amsterdam, London): public inventory API with
//! available/total per location.

use anyhow::Result;
use serde_json::Value;

use crate::model::{Offer, Stock};
use crate::util::{first_num, get_json, parse_storage};

const URL: &str = "https://order.servermania.com/api/v1/order/inventory/servers";

fn country(name: &str) -> &'static str {
    let n = name.to_lowercase();
    match () {
        _ if n.contains("amsterdam") => "NL",
        _ if n.contains("london") => "GB",
        _ if n.contains("vancouver") || n.contains("montreal") || n.contains("toronto") => "CA",
        _ => "US",
    }
}

pub async fn fetch(client: &reqwest::Client) -> Result<Vec<Offer>> {
    let v = get_json(client, URL).await?;
    let mut out = Vec::new();
    for p in v["data"].as_array().into_iter().flatten() {
        if p["auto_hide"] == true {
            continue;
        }
        let Some(price) = p["price"]
            .as_f64()
            .or(p["price"].as_str().and_then(first_num))
        else {
            continue;
        };
        let cores = p["cores"].as_str().unwrap_or_default();
        let ram = p["ram"].as_str().unwrap_or_default();
        let storage = p["storage"].as_str().unwrap_or_default();
        let locs: Vec<Value> = p["locations"].as_array().cloned().unwrap_or_default();
        for l in &locs {
            let name = l["name"].as_str().unwrap_or_default();
            let avail = l["available"].as_i64().unwrap_or(0);
            let mut o = Offer::new("servermania", format!("{}@{name}", p["id"]));
            o.name = p["cpu"].as_str().unwrap_or_default().to_string();
            o.cpu = o.name.clone();
            // "20 x 2.2 GHz / 3.2 GHz"
            o.cores = cores
                .split('x')
                .next()
                .and_then(first_num)
                .map(|c| c as u32);
            o.ram_gb = first_num(ram);
            (o.storage_gb, o.nvme) = parse_storage(storage);
            o.storage = storage.to_string();
            o.traffic = p["bandwidth"].as_str().map(String::from);
            o.location = name.to_string();
            o.country = country(name).into();
            o.price_monthly = price;
            o.currency = "USD";
            o.stock_qty = Some(avail);
            o.stock = if avail > 0 {
                Stock::InStock
            } else {
                Stock::OutOfStock
            };
            o.availability = l["hours"].as_f64().map(|h| format!("{h}h"));
            o.spec_text = format!("{ram} {storage}");
            o.url = "https://www.servermania.com/dedicated-servers.htm".into();
            out.push(o);
        }
    }
    Ok(out)
}
