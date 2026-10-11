//! Cherry Servers: public plans API with per-region stock counts.

use anyhow::Result;
use serde_json::Value;

use crate::model::{Offer, Stock};
use crate::util::get_json;

const PLANS: &str = "https://api.cherryservers.com/v1/plans";

pub async fn fetch(client: &reqwest::Client) -> Result<Vec<Offer>> {
    let v = get_json(client, PLANS).await?;
    let mut out = Vec::new();
    for p in v.as_array().into_iter().flatten() {
        if p["type"] != "baremetal" {
            continue;
        }
        let Some(monthly) = p["pricing"]
            .as_array()
            .and_then(|a| a.iter().find(|x| x["unit"] == "Monthly"))
        else {
            continue;
        };
        let specs = &p["specs"];
        let cpus = &specs["cpus"];
        let count = cpus["count"].as_u64().unwrap_or(1) as u32;
        let disks: Vec<&Value> = specs["storage"]
            .as_array()
            .map(|a| a.iter().collect())
            .unwrap_or_default();
        let storage_gb: f64 = disks
            .iter()
            .map(|d| {
                let size = d["size"].as_f64().unwrap_or(0.0);
                let gb = if d["unit"] == "TB" {
                    size * 1000.0
                } else {
                    size
                };
                gb * d["count"].as_f64().unwrap_or(1.0)
            })
            .sum();
        let storage = disks
            .iter()
            .map(|d| format!("{}x {}", d["count"], d["name"].as_str().unwrap_or("?")))
            .collect::<Vec<_>>()
            .join(" + ");
        let slug = p["slug"].as_str().unwrap_or_default();

        for r in p["available_regions"].as_array().into_iter().flatten() {
            let qty = r["stock_qty"].as_i64().unwrap_or(0);
            let mut o = Offer::new(
                "cherry",
                format!("{slug}-{}", r["slug"].as_str().unwrap_or("?")),
            );
            o.name = p["name"].as_str().unwrap_or(slug).to_string();
            o.cpu = cpus["name"].as_str().unwrap_or_default().to_string();
            o.cpu_count = count;
            o.cores = cpus["cores"].as_u64().map(|c| c as u32 * count);
            o.threads = cpus["threads"].as_u64().map(|t| t as u32 * count);
            o.ram_gb = specs["memory"]["total"].as_f64();
            o.ecc = specs["memory"]["name"].as_str().map(|n| n.contains("ECC"));
            o.storage = storage.clone();
            o.storage_gb = (storage_gb > 0.0).then_some(storage_gb);
            o.nvme = Some(disks.iter().any(|d| {
                d["type"] == "NVMe" || d["name"].as_str().is_some_and(|n| n.contains("NVMe"))
            }));
            o.port_mbps = specs["nics"]["name"]
                .as_str()
                .and_then(crate::util::first_num)
                .map(|g| (g * 1000.0) as u32);
            o.traffic = specs["bandwidth"]["name"].as_str().map(String::from);
            o.location = r["location"].as_str().unwrap_or_default().to_string();
            o.country = r["region_iso_2"]
                .as_str()
                .unwrap_or_default()
                .to_uppercase();
            o.price_monthly = monthly["price"].as_f64().unwrap_or(0.0);
            o.currency = if monthly["currency"] == "USD" {
                "USD"
            } else {
                "EUR"
            };
            o.stock = if qty > 0 {
                Stock::InStock
            } else {
                Stock::OutOfStock
            };
            o.stock_qty = Some(qty);
            o.url = format!("https://www.cherryservers.com/server-customizer/{slug}");
            out.push(o);
        }
    }
    Ok(out)
}
