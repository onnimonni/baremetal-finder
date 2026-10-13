//! HostZealot (EE; servers in EU and US): server list embedded in the page's
//! Alpine.js state as `servers: [...]`. No stock signal.

use anyhow::{Context, Result};
use serde_json::Value;

use crate::geo::iso_from_name;
use crate::model::{Offer, Stock};
use crate::util::{balanced_json, get_text};

const URL: &str = "https://www.hostzealot.com/servers";

pub async fn fetch(client: &reqwest::Client) -> Result<Vec<Offer>> {
    let html = get_text(client, URL).await?;
    let start = html
        .find("servers: [")
        .context("hostzealot: no servers state")?
        + 9;
    let list: Vec<Value> = serde_json::from_str(
        balanced_json(&html[start..]).context("hostzealot: unbalanced servers state")?,
    )?;
    let mut out = Vec::new();
    for s in &list {
        let Some(price) = s["price"]["monthly"]["price"].as_f64() else {
            continue;
        };
        let spec = &s["specifications"];
        let Some(cpu) = spec["cpu"].as_array().and_then(|c| c.first()) else {
            continue;
        };
        let count = cpu["cpu_num"].as_u64().unwrap_or(1).max(1) as u32;
        let cores = cpu["cores"].as_u64().map(|c| c as u32 * count);
        let model = cpu["model"].as_str().unwrap_or_default();
        let vendor =
            if model.contains("EPYC") || model.contains("Ryzen") || model.contains("Opteron") {
                "AMD"
            } else {
                "Intel"
            };
        let disks: Vec<String> = spec["hdd"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|d| {
                format!(
                    "{}x {} GB {}",
                    d["count"],
                    d["size"],
                    d["type"].as_str().unwrap_or_default()
                )
            })
            .collect();
        let storage_gb: f64 = spec["hdd"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|d| d["count"].as_f64().unwrap_or(1.0) * d["size"].as_f64().unwrap_or(0.0))
            .sum();
        let ram: Vec<&Value> = spec["ram"].as_array().into_iter().flatten().collect();
        let ram_gb: f64 = ram
            .iter()
            .map(|r| r["count"].as_f64().unwrap_or(1.0) * r["size"].as_f64().unwrap_or(0.0))
            .sum();
        let ram_type = ram
            .first()
            .and_then(|r| r["type"].as_str())
            .unwrap_or_default();
        let storage = disks.join(" + ");
        let mut o = Offer::new("hostzealot", s["pid"].to_string());
        o.name = format!("{model} {}", s["dc"].as_str().unwrap_or_default());
        o.cpu = format!("{vendor} {model}");
        o.cpu_count = count;
        o.cores = cores;
        o.threads = cores.map(|c| if cpu["ht"] == 1 { c * 2 } else { c });
        o.ram_gb = Some(ram_gb);
        o.storage_gb = Some(storage_gb);
        o.nvme = Some(storage.to_lowercase().contains("nvme"));
        o.storage = storage;
        o.port_mbps = s["network_port"]
            .as_str()
            .and_then(|p| p.trim_end_matches("Gbps").parse::<f64>().ok())
            .map(|g| (g * 1000.0) as u32);
        o.traffic = s["network_volume"].as_str().map(String::from);
        o.location = s["city"].as_str().unwrap_or_default().to_string();
        o.country = s["country"]
            .as_str()
            .and_then(iso_from_name)
            .unwrap_or_default()
            .to_string();
        o.price_monthly = price;
        o.stock = Stock::Unknown;
        o.provider_cpu_mark = cpu["bench"]
            .as_f64()
            .filter(|b| *b > 0.0)
            .map(|b| b * count as f64);
        o.spec_text = format!("RAM {ram_type}");
        o.url = URL.into();
        out.push(o);
    }
    Ok(out)
}
