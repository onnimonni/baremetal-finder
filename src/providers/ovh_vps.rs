//! OVHcloud VPS: public VPS order catalog (current VPS models only) plus the
//! per-plan datacenter availability rule (available / out-of-stock).

use std::collections::HashMap;

use anyhow::Result;
use serde_json::Value;

use crate::model::{Offer, Stock};
use crate::util::get_json;

const CATALOG: &str = "https://eu.api.ovh.com/v1/order/catalog/public/vps?ovhSubsidiary=IE";
const RULE: &str = "https://eu.api.ovh.com/v1/vps/order/rule/datacenter?ovhSubsidiary=IE&planCode=";

fn dc_country(code: &str) -> &'static str {
    match code {
        "BHS" => "CA",
        "SGP" => "SG",
        "SYD" => "AU",
        "YNM" => "IN",
        "UK" => "GB",
        "DE" => "DE",
        "WAW" => "PL",
        c if c.contains("MIL") => "IT",
        _ => "FR", // GRA, SBG, RBX
    }
}

pub async fn fetch(client: &reqwest::Client) -> Result<Vec<Offer>> {
    let cat = get_json(client, CATALOG).await?;
    let products: HashMap<String, Value> = cat["products"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|p| Some((p["name"].as_str()?.to_string(), p.clone())))
        .collect();
    let mut out = Vec::new();
    for plan in cat["plans"].as_array().into_iter().flatten() {
        let code = plan["planCode"].as_str().unwrap_or_default();
        // Current lines only (vps-2025-model*, vps-2027-model*), not Local Zones.
        if !(code.starts_with("vps-2025-model") || code.starts_with("vps-2027-model"))
            || code.contains(".LZ")
        {
            continue;
        }
        let Some(price) =
            plan["pricings"].as_array().into_iter().flatten().find(|p| {
                p["mode"] == "default" && p["intervalUnit"] == "month" && p["interval"] == 1
            })
        else {
            continue;
        };
        let monthly = price["price"].as_f64().unwrap_or(0.0) / 1e8;
        let tech = plan["product"]
            .as_str()
            .and_then(|p| products.get(p))
            .map(|p| p["blobs"]["technical"].clone())
            .unwrap_or_default();
        let rule = get_json(client, &format!("{RULE}{code}"))
            .await
            .unwrap_or_default();
        let vcpus = tech["cpu"]["cores"].as_u64().unwrap_or(0) as u32;
        let disk: f64 = tech["storage"]["disks"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|d| d["capacity"].as_f64().unwrap_or(0.0))
            .sum();
        for dc in rule["datacenters"].as_array().into_iter().flatten() {
            let name = dc["datacenter"].as_str().unwrap_or_default();
            let status = dc["linuxStatus"]
                .as_str()
                .or(dc["status"].as_str())
                .unwrap_or("unknown");
            let mut o = Offer::new("ovh_vps", format!("{code}@{name}"));
            o.kind = "vps";
            o.name = plan["invoiceName"].as_str().unwrap_or(code).to_string();
            o.cpu = "OVH vCore".into();
            o.vcpus = Some(vcpus);
            o.threads = Some(vcpus);
            o.shared_cpu = Some(true);
            o.ram_gb = tech["memory"]["size"].as_f64();
            o.storage_gb = Some(disk);
            o.storage = format!("{disk} GB NVMe");
            o.nvme = Some(true);
            o.port_mbps = tech["bandwidth"]["level"].as_u64().map(|b| b as u32);
            o.traffic = Some("unlimited".into());
            o.location = name.to_string();
            o.country = dc_country(name).into();
            o.price_monthly = monthly;
            o.stock = match status {
                "available" => Stock::InStock,
                "out-of-stock" => Stock::OutOfStock,
                _ => Stock::Unknown,
            };
            o.availability = Some(status.to_string());
            o.url = "https://www.ovhcloud.com/en-ie/vps/".into();
            out.push(o);
        }
    }
    Ok(out)
}
