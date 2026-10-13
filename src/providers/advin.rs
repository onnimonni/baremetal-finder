//! Advin Servers (US, Nuremberg, Johor): dedicated plans JSON with a stock
//! flag. "0.00" prices mean "contact us" and are skipped.

use anyhow::Result;

use crate::model::{Offer, Stock};
use crate::util::{first_num, get_json, parse_storage};

const URL: &str = "https://advinservers.com/api/dedicated";

fn country(loc: &str) -> &'static str {
    if loc.contains("Nuremberg") {
        "DE"
    } else if loc.contains("Johor") {
        "MY"
    } else {
        "US"
    }
}

pub async fn fetch(client: &reqwest::Client) -> Result<Vec<Offer>> {
    let v = get_json(client, URL).await?;
    let mut out = Vec::new();
    for loc in v
        .as_array()
        .into_iter()
        .flatten()
        .flat_map(|g| g["locations"].as_array().cloned().unwrap_or_default())
    {
        let place = loc["display_name"].as_str().unwrap_or_default();
        for p in loc["plans"].as_array().into_iter().flatten() {
            let Some(price) = p["price"].as_str().and_then(first_num).filter(|p| *p > 0.0) else {
                continue;
            };
            // "AMD Ryzen 9 9950X (16 cores, 32 threads)"
            let spec = p["vcpu_cores"].as_str().unwrap_or_default();
            let (cpu, counts) = spec.split_once(" (").unwrap_or((spec, ""));
            let disk = p["disk"].as_str().unwrap_or_default();
            let mem = p["memory"].as_str().unwrap_or_default();
            let mut o = Offer::new("advin", p["uid"].as_str().unwrap_or_default().to_string());
            o.name = p["name"].as_str().unwrap_or_default().to_string();
            o.cpu = cpu.to_string();
            o.cores = first_num(counts).map(|c| c as u32);
            o.threads = counts
                .split_once(", ")
                .and_then(|(_, t)| first_num(t))
                .map(|t| t as u32);
            o.ram_gb = first_num(mem);
            (o.storage_gb, o.nvme) = parse_storage(disk);
            o.storage = disk.to_string();
            o.traffic = p["bandwidth"].as_str().map(String::from);
            o.location = place.to_string();
            o.country = country(place).into();
            o.price_monthly = price;
            o.setup_fee = p["setup_fee"].as_str().and_then(first_num).unwrap_or(0.0);
            o.currency = "USD";
            o.stock = if p["stock"] == true {
                Stock::InStock
            } else {
                Stock::OutOfStock
            };
            o.availability = p["deployment_time"].as_str().map(String::from);
            o.spec_text = mem.to_string();
            o.url = p["whmcs_url"]
                .as_str()
                .unwrap_or("https://advinservers.com")
                .to_string();
            out.push(o);
        }
    }
    Ok(out)
}
