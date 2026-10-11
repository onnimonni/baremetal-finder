//! Velia.net: Upmind product API (brand chosen by the Origin header).
//! Only an "available for sale" flag, no real stock signal.

use anyhow::Result;
use serde_json::Value;

use crate::geo::iso_from_name;
use crate::model::{Offer, Stock};
use crate::util::{first_num, parse_storage};

const URL: &str = "https://api.upmind.io/api/products?limit=100&with=prices,category";

pub async fn fetch(client: &reqwest::Client) -> Result<Vec<Offer>> {
    let v: Value = client
        .get(URL)
        .header("Origin", "https://checkout.velia.net")
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    let mut out = Vec::new();
    for p in v["data"].as_array().into_iter().flatten() {
        let Some(price) = p["prices"]
            .as_array()
            .and_then(|a| a.iter().find(|x| x["billing_cycle_months"] == 1))
        else {
            continue;
        };
        let desc = p["description"].as_str().unwrap_or_default();
        let line = |k: &str| {
            desc.lines()
                .find_map(|l| l.trim_start_matches(['-', ' ']).strip_prefix(k))
                .map(|s| s.trim().to_string())
                .unwrap_or_default()
        };
        let location = p["location"]
            .as_str()
            .or(p["category"]["name"].as_str())
            .unwrap_or_default()
            .to_string();
        let country = location
            .rsplit(", ")
            .next()
            .and_then(iso_from_name)
            .unwrap_or_default();
        let storage = line("Storage:");
        let sellable = p["available_for_sales"] == 1 && p["clients_can_order"] == 1;

        let mut o = Offer::new("velia", p["id"].as_str().unwrap_or_default());
        o.name = p["name"].as_str().unwrap_or_default().to_string();
        o.cpu = Some(line("CPU:"))
            .filter(|c| !c.is_empty())
            .unwrap_or_else(|| o.name.trim().to_string());
        // "2x 32x 3.55 GHz (Dual 32 Core)" or "16x 4.20 GHz"
        let cores = line("Cores:");
        let nums: Vec<f64> = cores
            .split('x')
            .map_while(|p| p.trim().parse::<f64>().ok())
            .collect();
        o.cores = match nums.as_slice() {
            [sockets, per] => Some((sockets * per) as u32),
            _ => first_num(&cores).map(|c| c as u32),
        };
        if nums.len() == 2 {
            o.cpu_count = nums[0] as u32;
        }
        o.ram_gb = first_num(&line("RAM:"));
        (o.storage_gb, o.nvme) = parse_storage(&storage);
        o.storage = storage;
        o.location = location;
        o.country = country.to_string();
        o.price_monthly = price["price"].as_f64().unwrap_or(0.0);
        o.currency = crate::util::currency(price["currency_code"].as_str().unwrap_or("EUR"))
            .unwrap_or("EUR");
        o.stock = if sellable {
            Stock::Unknown
        } else {
            Stock::OutOfStock
        };
        o.availability = p["short_description"].as_str().map(String::from);
        o.url = "https://www.velia.net/dedicated-server/".into();
        o.note = Some("no stock signal".into());
        out.push(o);
    }
    Ok(out)
}
