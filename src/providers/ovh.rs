//! OVHcloud: public order catalogs (eco = Kimsufi/So you Start/Rise, and
//! baremetalServers = Advance/Scale/HG) joined with the public availability API.

use std::collections::HashMap;
use std::sync::LazyLock;

use anyhow::Result;
use regex::Regex;
use serde_json::Value;

use crate::model::{Offer, Stock};
use crate::util::{client, get_json};

/// OVH runs separate EU and US companies with their own catalogs/accounts.
struct Region {
    provider: &'static str,
    catalogs: &'static [&'static str],
    availability: &'static str,
    currency: &'static str,
    /// Datacenters orderable through this region's account.
    datacenters: fn(&str) -> bool,
}

const EU: Region = Region {
    provider: "ovh",
    catalogs: &[
        "https://eu.api.ovh.com/v1/order/catalog/public/eco?ovhSubsidiary=IE",
        "https://eu.api.ovh.com/v1/order/catalog/public/baremetalServers?ovhSubsidiary=IE",
    ],
    availability: "https://eu.api.ovh.com/v1/dedicated/server/datacenter/availabilities",
    currency: "EUR",
    datacenters: |dc| !matches!(dc, "vin" | "hil"),
};

const US: Region = Region {
    provider: "ovh_us",
    catalogs: &[
        "https://api.us.ovhcloud.com/v1/order/catalog/public/eco?ovhSubsidiary=US",
        "https://api.us.ovhcloud.com/v1/order/catalog/public/baremetalServers?ovhSubsidiary=US",
    ],
    availability: "https://api.us.ovhcloud.com/v1/dedicated/server/datacenter/availabilities",
    currency: "USD",
    datacenters: |dc| matches!(dc, "vin" | "hil"),
};

fn dc_country(dc: &str) -> &'static str {
    match dc {
        "bhs" | "ca-east-tor-a" | "tor" => "CA",
        "fra" | "lim" => "DE",
        "lon" | "eri" => "GB",
        "waw" => "PL",
        "sgp" => "SG",
        "syd" => "AU",
        "ynm" | "mum" => "IN",
        "vin" | "hil" => "US",
        d if d.starts_with("eu-west-par") => "FR",
        d if d.starts_with("eu-south-mil") => "IT",
        _ => "FR", // gra, rbx, sbg
    }
}

/// Catalog prices are in 1e-8 of the currency unit.
fn price(v: &Value) -> f64 {
    v.as_f64().unwrap_or(0.0) / 1e8
}

/// (monthly, setup) from a plan's no-commitment monthly pricing.
fn pricing(plan: &Value) -> (f64, f64) {
    let mut monthly = 0.0;
    let mut setup = 0.0;
    for p in plan["pricings"].as_array().into_iter().flatten() {
        if p["mode"] != "default" {
            continue;
        }
        let caps = p["capacities"].as_array().cloned().unwrap_or_default();
        if caps.iter().any(|c| c == "installation") {
            setup = price(&p["price"]);
        } else if caps.iter().any(|c| c == "renew")
            && p["intervalUnit"] == "month"
            && p["interval"] == 1
        {
            monthly = price(&p["price"]);
        }
    }
    (monthly, setup)
}

struct Catalog {
    plans: HashMap<String, Value>,
    addons: HashMap<String, Value>,
    products: HashMap<String, Value>,
    region: &'static Region,
}

pub async fn eu(_client: &reqwest::Client) -> Result<Vec<Offer>> {
    fetch(&EU).await
}

pub async fn us(_client: &reqwest::Client) -> Result<Vec<Offer>> {
    fetch(&US).await
}

async fn fetch(region: &'static Region) -> Result<Vec<Offer>> {
    // Catalogs are 10-30 MB each; a fresh client keeps them off the shared pool.
    let c = client()?;
    let mut cat = Catalog {
        plans: HashMap::new(),
        addons: HashMap::new(),
        products: HashMap::new(),
        region,
    };
    for url in region.catalogs {
        let v = get_json(&c, url).await?;
        anyhow::ensure!(
            v["locale"]["currencyCode"] == region.currency,
            "unexpected OVH currency {}",
            v["locale"]["currencyCode"]
        );
        for p in v["plans"].as_array().into_iter().flatten() {
            cat.plans
                .insert(p["planCode"].as_str().unwrap_or_default().into(), p.clone());
        }
        for a in v["addons"].as_array().into_iter().flatten() {
            cat.addons
                .insert(a["planCode"].as_str().unwrap_or_default().into(), a.clone());
        }
        for p in v["products"].as_array().into_iter().flatten() {
            cat.products
                .insert(p["name"].as_str().unwrap_or_default().into(), p.clone());
        }
    }
    let avail = get_json(&c, region.availability).await?;
    let mut out = Vec::new();
    for a in avail.as_array().into_iter().flatten() {
        let Some(plan) = a["planCode"].as_str().and_then(|p| cat.plans.get(p)) else {
            continue;
        };
        for dc in a["datacenters"].as_array().into_iter().flatten() {
            if !dc["datacenter"].as_str().is_some_and(region.datacenters) {
                continue;
            }
            if let Some(o) = offer(&cat, plan, a, dc) {
                out.push(o);
            }
        }
    }
    Ok(out)
}

/// Addon from the plan's family whose code starts with the availability's
/// option code (e.g. memory "ram-64g-ecc-2400" -> "ram-64g-ecc-2400-24sk60").
fn addon<'a>(cat: &'a Catalog, plan: &Value, family: &str, option: &str) -> Option<&'a Value> {
    plan["addonFamilies"]
        .as_array()?
        .iter()
        .find(|f| f["name"] == family)?["addons"]
        .as_array()?
        .iter()
        .filter_map(Value::as_str)
        .find(|code| code.starts_with(option))
        .and_then(|code| cat.addons.get(code))
}

fn offer(cat: &Catalog, plan: &Value, a: &Value, dc: &Value) -> Option<Offer> {
    static RAM: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"ram-(\d+)g").expect("static regex is valid"));
    static BW: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"bandwidth-(\d+)").expect("static regex is valid"));

    let plan_code = plan["planCode"].as_str()?;
    let memory = a["memory"].as_str().unwrap_or_default();
    let storage = a["storage"].as_str().unwrap_or_default();
    let dc_code = dc["datacenter"].as_str()?;
    let availability = dc["availability"].as_str().unwrap_or("unknown");

    let (mut monthly, mut setup) = pricing(plan);
    for (family, opt) in [("memory", memory), ("storage", storage)] {
        if let Some(ad) = addon(cat, plan, family, opt) {
            let (m, s) = pricing(ad);
            monthly += m;
            setup += s;
        }
    }
    if monthly <= 0.0 {
        return None;
    }

    let product = plan["product"].as_str().and_then(|p| cat.products.get(p));
    let cpu = product.map(|p| &p["blobs"]["technical"]["server"]["cpu"]);
    let cpu_name = cpu
        .map(|c| {
            format!(
                "{} {}",
                c["brand"].as_str().unwrap_or_default(),
                c["model"].as_str().unwrap_or_default()
            )
        })
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| {
            plan["invoiceName"]
                .as_str()
                .and_then(|n| n.split('|').nth(1))
                .unwrap_or_default()
                .trim()
                .to_string()
        });
    let cpu_count = cpu.and_then(|c| c["number"].as_u64()).unwrap_or(1) as u32;

    let bandwidth = plan["addonFamilies"]
        .as_array()
        .and_then(|fs| fs.iter().find(|f| f["name"] == "bandwidth"))
        .and_then(|f| f["default"].as_str())
        .and_then(|d| BW.captures(d))
        .and_then(|c| c[1].parse().ok());

    let mut o = Offer::new(
        cat.region.provider,
        format!("{}.{}", a["fqn"].as_str()?, dc_code),
    );
    o.name = plan["invoiceName"]
        .as_str()
        .unwrap_or(plan_code)
        .to_string();
    o.cpu = cpu_name;
    o.cpu_count = cpu_count;
    o.cores = cpu
        .and_then(|c| c["cores"].as_u64())
        .map(|c| c as u32 * cpu_count);
    o.threads = cpu
        .and_then(|c| c["threads"].as_u64())
        .map(|t| t as u32 * cpu_count);
    o.provider_cpu_mark = cpu.and_then(|c| c["score"].as_f64());
    o.ram_gb = RAM.captures(memory).and_then(|c| c[1].parse().ok());
    o.ecc = Some(memory.contains("ecc"));
    o.storage = storage.to_string();
    (o.storage_gb, o.nvme) = ovh_storage(storage);
    o.port_mbps = bandwidth;
    o.location = dc_code.to_string();
    o.country = dc_country(dc_code).into();
    o.price_monthly = monthly;
    o.setup_fee = setup;
    o.currency = cat.region.currency;
    o.stock = match availability {
        "unavailable" | "comingSoon" => Stock::OutOfStock,
        "unknown" => Stock::Unknown,
        _ => Stock::InStock, // 1H-high, 1H-low, 24H, 72H …
    };
    o.availability = Some(availability.into());
    o.url = if cat.region.currency == "USD" {
        "https://us.ovhcloud.com/bare-metal/".into()
    } else {
        "https://eco.ovhcloud.com/en-ie/".into()
    };
    Some(o)
}

/// "softraid-2x960nvme" -> 1920 GB NVMe, "softraid-3x4000sa" -> 12000 GB,
/// "hybridsoftraid-2x4000sa-1x500nvme" -> 8500 GB.
fn ovh_storage(code: &str) -> (Option<f64>, Option<bool>) {
    static DISK: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"(\d+)x(\d+)(nvme|ssd|sa|hdd)").expect("static regex is valid")
    });
    let mut total = 0.0;
    let mut nvme = false;
    for c in DISK.captures_iter(code) {
        let n: f64 = c[1].parse().unwrap_or(0.0);
        let size: f64 = c[2].parse().unwrap_or(0.0);
        total += n * size;
        nvme |= &c[3] == "nvme";
    }
    ((total > 0.0).then_some(total), Some(nvme))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn storage_codes() {
        assert_eq!(
            ovh_storage("softraid-2x960nvme"),
            (Some(1920.0), Some(true))
        );
        assert_eq!(
            ovh_storage("softraid-3x4000sa"),
            (Some(12000.0), Some(false))
        );
    }
}
