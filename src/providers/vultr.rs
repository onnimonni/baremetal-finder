//! Vultr Bare Metal: tokenless public API. A plan listed in a region's
//! `availability?type=vbm` is in stock there.

use std::collections::{HashMap, HashSet};

use anyhow::Result;
use serde_json::Value;

use crate::model::{Offer, Stock};
use crate::util::get_json;

const API: &str = "https://api.vultr.com/v2";

pub async fn fetch(client: &reqwest::Client) -> Result<Vec<Offer>> {
    let plans = get_json(client, &format!("{API}/plans-metal?per_page=500")).await?;
    let regions = get_json(client, &format!("{API}/regions?per_page=500")).await?;
    let regions: HashMap<String, Value> = regions["regions"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|r| Some((r["id"].as_str()?.to_string(), r.clone())))
        .collect();

    let used: HashSet<String> = plans["plans_metal"]
        .as_array()
        .into_iter()
        .flatten()
        .flat_map(|p| p["locations"].as_array().cloned().unwrap_or_default())
        .filter_map(|l| l.as_str().map(String::from))
        .collect();
    let mut available: HashMap<String, HashSet<String>> = HashMap::new();
    for region in &used {
        match get_json(
            client,
            &format!("{API}/regions/{region}/availability?type=vbm"),
        )
        .await
        {
            Ok(v) => {
                let set = v["available_plans"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|p| p.as_str().map(String::from))
                    .collect();
                available.insert(region.clone(), set);
            }
            Err(e) => eprintln!("vultr availability {region}: {e:#}"),
        }
    }

    let mut out = Vec::new();
    for p in plans["plans_metal"].as_array().into_iter().flatten() {
        if p["gpu_type"].is_string() {
            continue;
        }
        let id = p["id"].as_str().unwrap_or_default();
        let sockets = p["physical_cpus"].as_u64().unwrap_or(1) as u32;
        let disk_count = p["disk_count"].as_f64().unwrap_or(1.0);
        let disk = p["disk"].as_f64().unwrap_or(0.0);
        for loc in p["locations"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
        {
            let r = regions.get(loc);
            let in_stock = available.get(loc).map(|s| s.contains(id));
            let mut o = Offer::new("vultr", format!("{id}@{loc}"));
            o.name = id.to_string();
            o.cpu = format!(
                "{} {}",
                p["cpu_manufacturer"].as_str().unwrap_or_default(),
                p["cpu_model"].as_str().unwrap_or_default()
            );
            o.cpu_count = sockets;
            o.cores = p["cpu_cores"].as_u64().map(|c| c as u32);
            o.threads = p["cpu_threads"].as_u64().map(|t| t as u32);
            o.ram_gb = p["ram"].as_f64().map(|mb| (mb / 1024.0).round());
            o.storage = format!(
                "{disk_count}x {disk}GB {}",
                p["type"].as_str().unwrap_or_default()
            );
            o.storage_gb = Some(disk_count * disk);
            o.nvme = Some(
                p["type"]
                    .as_str()
                    .is_some_and(|t| t.eq_ignore_ascii_case("nvme")),
            );
            o.traffic = p["bandwidth"]
                .as_f64()
                .map(|gb| format!("{} TB", gb / 1000.0));
            o.location = r
                .and_then(|r| r["city"].as_str())
                .unwrap_or(loc)
                .to_string();
            o.country = r
                .and_then(|r| r["country"].as_str())
                .unwrap_or_default()
                .to_string();
            o.price_monthly = p["monthly_cost"].as_f64().unwrap_or(0.0);
            o.currency = "USD";
            o.stock = match in_stock {
                Some(true) => Stock::InStock,
                Some(false) => Stock::OutOfStock,
                None => Stock::Unknown,
            };
            o.url = "https://www.vultr.com/products/bare-metal/".into();
            out.push(o);
        }
    }
    Ok(out)
}

/// Cloud Compute plans. vc2/vhf/vhp share vCPUs, voc (Optimized Cloud) is
/// dedicated. A plan listed for a region but missing from its availability
/// list is sold out there.
pub async fn vps(client: &reqwest::Client) -> Result<Vec<Offer>> {
    let plans = get_json(client, &format!("{API}/plans?per_page=500")).await?;
    let regions = get_json(client, &format!("{API}/regions?per_page=500")).await?;
    let regions: HashMap<String, Value> = regions["regions"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|r| Some((r["id"].as_str()?.to_string(), r.clone())))
        .collect();
    let mut available: HashMap<String, HashSet<String>> = HashMap::new();
    for region in regions.keys() {
        if let Ok(v) = get_json(client, &format!("{API}/regions/{region}/availability")).await {
            let set = v["available_plans"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|p| p.as_str().map(String::from))
                .collect();
            available.insert(region.clone(), set);
        }
    }
    let mut out = Vec::new();
    for p in plans["plans"].as_array().into_iter().flatten() {
        let kind = p["type"].as_str().unwrap_or_default();
        let shared = match kind {
            "vc2" | "vhf" | "vhp" => true,
            k if k.starts_with("voc") => false,
            _ => continue, // GPU / storage plans
        };
        let id = p["id"].as_str().unwrap_or_default();
        let vcpus = p["vcpu_count"].as_u64().unwrap_or(0) as u32;
        for loc in p["locations"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
        {
            let r = regions.get(loc);
            let price = p["location_cost"][loc]["monthly_cost"]
                .as_f64()
                .or(p["monthly_cost"].as_f64())
                .unwrap_or(0.0);
            let mut o = Offer::new("vultr_vps", format!("{id}@{loc}"));
            o.kind = "vps";
            o.name = id.to_string();
            o.cpu = format!(
                "{} (Vultr {kind})",
                p["cpu_vendor"].as_str().unwrap_or("x86")
            );
            o.vcpus = Some(vcpus);
            o.threads = Some(vcpus);
            o.shared_cpu = Some(shared);
            o.ram_gb = p["ram"].as_f64().map(|mb| mb / 1024.0);
            o.storage_gb = p["disk"].as_f64();
            o.storage = format!(
                "{} GB {}",
                p["disk"],
                p["disk_type"].as_str().unwrap_or("SSD")
            );
            o.nvme = Some(kind != "vc2");
            o.traffic = p["bandwidth"]
                .as_f64()
                .map(|gb| format!("{} TB", gb / 1000.0));
            o.location = r
                .and_then(|r| r["city"].as_str())
                .unwrap_or(loc)
                .to_string();
            o.country = r
                .and_then(|r| r["country"].as_str())
                .unwrap_or_default()
                .to_string();
            o.price_monthly = price;
            o.currency = "USD";
            o.stock = match available.get(loc) {
                Some(s) if s.contains(id) => Stock::InStock,
                Some(_) => Stock::OutOfStock,
                None => Stock::Unknown,
            };
            o.url = "https://www.vultr.com/pricing/".into();
            out.push(o);
        }
    }
    Ok(out)
}
