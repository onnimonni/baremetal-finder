//! Scaleway Instances: tokenless per-zone server catalog (monthly prices),
//! public catalog (CPU model, shared flag) and per-zone availability
//! (available / scarce / shortage).

use std::collections::HashMap;

use anyhow::Result;
use serde_json::Value;

use crate::model::{Offer, Stock};
use crate::util::{clean, get_json};

const ZONES: &[&str] = &[
    "fr-par-1", "fr-par-2", "fr-par-3", "nl-ams-1", "nl-ams-2", "nl-ams-3", "pl-waw-1", "pl-waw-2",
    "pl-waw-3",
];
const CATALOG: &str = "https://api.scaleway.com/product-catalog/v2alpha1/public-catalog/products?product_types=instance&page_size=500";

pub async fn fetch(client: &reqwest::Client) -> Result<Vec<Offer>> {
    // Offer name (upper case) -> (CPU model, shared)
    let mut cpu: HashMap<String, (String, bool)> = HashMap::new();
    for page in 1..=10 {
        let v = get_json(client, &format!("{CATALOG}&page={page}")).await?;
        let items = v["products"].as_array().cloned().unwrap_or_default();
        for p in &items {
            let c = &p["properties"]["hardware"]["cpu"];
            if let Some(name) = p["product"].as_str() {
                let model = clean(
                    &c["type"]
                        .as_str()
                        .unwrap_or_default()
                        .replace(['™', '®'], ""),
                );
                let model = model.split(" (").next().unwrap_or_default().to_string();
                cpu.insert(
                    name.to_uppercase(),
                    (model, c["virtual"]["shared"].as_bool().unwrap_or(true)),
                );
            }
        }
        if items.len() < 500 {
            break;
        }
    }

    let mut out = Vec::new();
    for zone in ZONES {
        let servers = get_json(
            client,
            &format!(
                "https://api.scaleway.com/instance/v1/zones/{zone}/products/servers?per_page=100"
            ),
        )
        .await?;
        let avail = get_json(
            client,
            &format!("https://api.scaleway.com/instance/v1/zones/{zone}/products/servers/availability?per_page=100"),
        )
        .await
        .unwrap_or(Value::Null);
        for (name, s) in servers["servers"].as_object().into_iter().flatten() {
            if s["end_of_service"] == true || s["gpu"].as_u64().is_some_and(|g| g > 0) {
                continue;
            }
            let Some(price) = s["monthly_price"].as_f64() else {
                continue;
            };
            let (model, shared) = cpu.get(&name.to_uppercase()).cloned().unwrap_or_default();
            let vcpus = s["ncpus"].as_u64().unwrap_or(0) as u32;
            let level = avail["servers"][name]["availability"].as_str();
            let mut o = Offer::new("scaleway_instances", format!("{name}@{zone}"));
            o.kind = "vps";
            o.name = name.clone();
            o.cpu = if model.is_empty() {
                "Scaleway vCPU".into()
            } else {
                model
            };
            o.vcpus = Some(vcpus);
            o.threads = Some(vcpus);
            o.shared_cpu = Some(shared);
            o.ram_gb = s["ram"].as_f64().map(|b| b / 1024f64.powi(3));
            let local = s["per_volume_constraint"]["l_ssd"]["max_size"]
                .as_f64()
                .unwrap_or(0.0);
            o.storage_gb = (local > 0.0).then_some(local / 1e9);
            o.storage = if local > 0.0 {
                format!("{:.0} GB local SSD", local / 1e9)
            } else {
                "block storage billed separately".into()
            };
            o.port_mbps = s["network"]["sum_internet_bandwidth"]
                .as_f64()
                .map(|b| (b / 1e6) as u32);
            o.location = zone.to_string();
            o.country = zone.get(..2).unwrap_or_default().to_uppercase();
            o.price_monthly = price;
            o.stock = match level {
                Some("available" | "scarce") => Stock::InStock,
                Some("shortage") => Stock::OutOfStock,
                _ => Stock::Unknown,
            };
            o.availability = level.map(String::from);
            o.url = "https://www.scaleway.com/en/pricing/virtual-instances/".into();
            out.push(o);
        }
    }
    Ok(out)
}
