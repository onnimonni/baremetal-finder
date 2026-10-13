//! Prepaid-Hoster (Germany): resells Hetzner auction servers; public JSON
//! API, one row per physical server. Prices include 19% German VAT.

use anyhow::Result;
use serde_json::Value;

use crate::model::{Offer, Stock};
use crate::util::get_json;

const API: &str = "https://fsn-01.api.pph.sh/public/products/dedicated";
const VAT: f64 = 1.19;

pub async fn fetch(client: &reqwest::Client) -> Result<Vec<Offer>> {
    let mut out = Vec::new();
    for page in 1..=50 {
        let v = get_json(client, &format!("{API}?page={page}")).await?;
        for s in v["data"].as_array().into_iter().flatten() {
            if let Some(o) = offer(s) {
                out.push(o);
            }
        }
        if page >= v["last_page"].as_i64().unwrap_or(1) {
            break;
        }
    }
    Ok(out)
}

fn offer(s: &Value) -> Option<Offer> {
    let price = s["price"].as_f64()?;
    let desc: Vec<&str> = s["description"]
        .as_array()?
        .iter()
        .filter_map(Value::as_str)
        .collect();
    let dc = &s["datacenter_details"];
    let count = s["diskcount"].as_f64().unwrap_or(1.0);
    let size = s["disksize"].as_f64().unwrap_or(0.0);
    let mut o = Offer::new("prepaidhoster", s["serverid"].to_string());
    o.name = format!("Server {}", s["serverid"]);
    o.cpu = s["original_cpu"]
        .as_str()
        .or(s["cpu"].as_str())
        .unwrap_or_default()
        .to_string();
    o.cores = s["cpu_info"]["cores"].as_u64().map(|c| c as u32);
    o.threads = s["cpu_info"]["threads"].as_u64().map(|t| t as u32);
    o.ram_gb = s["memory"].as_f64();
    o.storage = format!(
        "{count}x {size}GB {}",
        s["disktype"]["type"].as_str().unwrap_or_default()
    );
    o.storage_gb = Some(count * size);
    o.nvme = Some(s["disktype"]["type"] == "nvme");
    o.port_mbps = s["networkspeed"].as_u64().map(|n| n as u32);
    o.location = format!(
        "{} ({})",
        dc["city"].as_str().unwrap_or_default(),
        s["datacenter"].as_str().unwrap_or_default()
    );
    o.country = dc["country"].as_str().unwrap_or("DE").to_string();
    o.price_monthly = price / VAT;
    o.setup_fee = s["setup"].as_f64().unwrap_or(0.0) / VAT;
    o.stock = Stock::InStock;
    o.stock_qty = Some(1);
    o.spec_text = desc.join(" | ");
    o.url = "https://prepaid-hoster.de/dedicated-server".into();
    o.note = Some("Hetzner auction resale; VAT stripped".into());
    Some(o)
}
