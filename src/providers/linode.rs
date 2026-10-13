//! Linode / Akamai Cloud: tokenless public API with per-region availability
//! for every plan type.

use std::collections::{HashMap, HashSet};

use anyhow::Result;
use serde_json::Value;

use crate::model::{Offer, Stock};
use crate::util::get_json;

const API: &str = "https://api.linode.com/v4";

pub async fn fetch(client: &reqwest::Client) -> Result<Vec<Offer>> {
    let types = get_json(client, &format!("{API}/linode/types?page_size=500")).await?;
    let regions = get_json(client, &format!("{API}/regions?page_size=500")).await?;
    let mut unavailable = HashSet::new();
    for page in 1..=20 {
        let v = get_json(
            client,
            &format!("{API}/regions/availability?page_size=500&page={page}"),
        )
        .await?;
        for a in v["data"].as_array().into_iter().flatten() {
            if a["available"] == false {
                unavailable.insert((
                    a["region"].as_str().unwrap_or_default().to_string(),
                    a["plan"].as_str().unwrap_or_default().to_string(),
                ));
            }
        }
        if page >= v["pages"].as_i64().unwrap_or(1) {
            break;
        }
    }
    let regions: Vec<Value> = regions["data"].as_array().cloned().unwrap_or_default();

    let mut out = Vec::new();
    for t in types["data"].as_array().into_iter().flatten() {
        let class = t["class"].as_str().unwrap_or_default();
        if matches!(class, "gpu" | "accelerated") {
            continue;
        }
        let id = t["id"].as_str().unwrap_or_default();
        let overrides: HashMap<&str, f64> = t["region_prices"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|r| {
                Some((
                    r["id"].as_str()?,
                    r["monthly"]
                        .as_f64()
                        .or(r["hourly"].as_f64().map(|h| h * 730.0))?,
                ))
            })
            .collect();
        let base = t["price"]["monthly"]
            .as_f64()
            .or(t["price"]["hourly"].as_f64().map(|h| h * 730.0));
        for r in &regions {
            let rid = r["id"].as_str().unwrap_or_default();
            if r["status"] != "ok"
                || !r["capabilities"]
                    .as_array()
                    .is_some_and(|c| c.iter().any(|x| x == "Linodes"))
            {
                continue;
            }
            let Some(price) = overrides.get(rid).copied().or(base) else {
                continue;
            };
            let vcpus = t["vcpus"].as_u64().unwrap_or(0) as u32;
            let mut o = Offer::new("linode", format!("{id}@{rid}"));
            o.kind = "vps";
            o.name = t["label"].as_str().unwrap_or(id).to_string();
            o.cpu = format!("AMD EPYC (Linode {class})");
            o.vcpus = Some(vcpus);
            o.threads = Some(vcpus);
            o.shared_cpu = Some(matches!(class, "nanode" | "standard"));
            o.ram_gb = t["memory"].as_f64().map(|mb| mb / 1024.0);
            o.storage_gb = t["disk"].as_f64().map(|mb| mb / 1024.0);
            o.storage = format!("{} GB SSD", o.storage_gb.unwrap_or(0.0));
            o.nvme = Some(false);
            o.port_mbps = t["network_out"].as_u64().map(|n| n as u32);
            o.traffic = t["transfer"]
                .as_f64()
                .map(|gb| format!("{} TB", gb / 1000.0));
            o.location = r["label"].as_str().unwrap_or(rid).to_string();
            o.country = r["country"].as_str().unwrap_or_default().to_uppercase();
            o.price_monthly = price;
            o.currency = "USD";
            o.stock = if unavailable.contains(&(rid.to_string(), id.to_string())) {
                Stock::OutOfStock
            } else {
                Stock::InStock
            };
            o.url = "https://www.linode.com/pricing/".into();
            out.push(o);
        }
    }
    Ok(out)
}
