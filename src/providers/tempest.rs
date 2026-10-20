//! Tempest (Dallas, Miami, London, Amsterdam): portal catalog JSON with an
//! `inStock` flag.

use anyhow::Result;

use crate::model::{Offer, Stock};
use crate::util::{currency, first_num, get_json, parse_storage};

const API: &str = "https://tempest.net/api/portal/catalog?line=";

pub async fn fetch(client: &reqwest::Client) -> Result<Vec<Offer>> {
    let mut out = Vec::new();
    for line in ["cost-optimized", "enterprise"] {
        let v = get_json(client, &format!("{API}{line}")).await?;
        for p in v["products"].as_array().into_iter().flatten() {
            if !p["vm"].is_null() {
                continue;
            }
            let Some(price) = p["price"]["amount"].as_f64() else {
                continue;
            };
            let s = |k: &str| p[k].as_str().unwrap_or_default().to_string();
            // "10C / 20T @ 3.7GHz / 5.3GHz"
            let details = s("cpuDetails");
            let storage = s("storage");
            let net = s("network");
            let mut o = Offer::new("tempest", s("id"));
            o.name = format!("{} {}", s("cpu"), s("ram"));
            o.cpu = if p["cpuModel"].is_string() {
                s("cpuModel")
            } else {
                s("cpu")
            };
            o.cores = first_num(&details).map(|c| c as u32);
            o.threads = details
                .split_once("/ ")
                .and_then(|(_, t)| first_num(t))
                .map(|t| t as u32);
            o.ram_gb = first_num(&s("ram"));
            (o.storage_gb, o.nvme) = parse_storage(&storage);
            o.storage = storage;
            o.port_mbps =
                first_num(&net).map(|n| if net.contains("Gbps") { n * 1000.0 } else { n } as u32);
            o.traffic = Some(net);
            o.location = p["location"]["city"]
                .as_str()
                .unwrap_or_default()
                .to_string();
            o.country = p["location"]["countryCode"]
                .as_str()
                .unwrap_or_default()
                .to_uppercase()
                .replace("UK", "GB");
            o.price_monthly = price;
            o.setup_fee = p["price"]["setupFee"].as_f64().unwrap_or(0.0);
            o.currency = p["price"]["currency"]
                .as_str()
                .and_then(currency)
                .unwrap_or("USD");
            o.stock = if p["inStock"] == true {
                Stock::InStock
            } else {
                Stock::OutOfStock
            };
            o.spec_text = s("ram");
            o.url = s("orderUrl");
            out.push(o);
        }
    }
    Ok(out)
}
