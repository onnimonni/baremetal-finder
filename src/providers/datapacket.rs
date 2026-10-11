//! DataPacket: public GraphQL API used by their configurator. Every
//! CPU/RAM/storage combination is its own config with per-location stock.

use std::collections::HashMap;

use anyhow::Result;
use serde_json::{Value, json};

use crate::model::{Offer, Stock};
use crate::util::parse_num;

const URL: &str = "https://app.datapacket.com/graphql";
const REGIONS: &[&str] = &[
    "europe",
    "north-america",
    "asia-pacific",
    "latin-america",
    "middle-east-and-africa",
];
const QUERY: &str = "query AvailableServerConfigurationsByRegion($region: String!, $hasGpu: Boolean, $currency: Currency, $billingPeriod: ServerBillingPeriod) { availableServerConfigurationsByRegion(region: $region, hasGpu: $hasGpu, currency: $currency, billingPeriod: $billingPeriod) { currency cpus { oid count family name cores threads passmarkScore traffic { minimum maximum } } configurations { id stockCount cid memory storageId locationDistributions { price location stockCount uplinkCapacities { bandwidth serverCount } } } storages { id drives { count size type } } locations { countryIso location locationShortname stockDeliveryTime } } }";

pub async fn fetch(client: &reqwest::Client) -> Result<Vec<Offer>> {
    let mut out = Vec::new();
    for region in REGIONS {
        let body = json!({
            "query": QUERY,
            // isPublic omitted = full inventory, not just the website's 7 CPUs.
            "variables": {"region": region, "hasGpu": false, "currency": "EUR", "billingPeriod": "MONTHLY"},
        });
        let v: Value = client
            .post(URL)
            .json(&body)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        if let Some(errs) = v.get("errors") {
            anyhow::bail!("datapacket {region}: {errs}");
        }
        out.extend(parse(&v["data"]["availableServerConfigurationsByRegion"]));
    }
    Ok(out)
}

fn parse(d: &Value) -> Vec<Offer> {
    let arr = |k: &str| d[k].as_array().cloned().unwrap_or_default();
    let cpus: HashMap<String, Value> = arr("cpus")
        .into_iter()
        .map(|c| (c["oid"].as_str().unwrap_or_default().to_string(), c))
        .collect();
    let storages: HashMap<String, Value> = arr("storages")
        .into_iter()
        .map(|s| (s["id"].as_str().unwrap_or_default().to_string(), s))
        .collect();
    let locations: HashMap<String, Value> = arr("locations")
        .into_iter()
        .map(|l| (l["location"].as_str().unwrap_or_default().to_string(), l))
        .collect();
    let currency = if d["currency"] == "USD" { "USD" } else { "EUR" };

    let mut out = Vec::new();
    for c in arr("configurations") {
        let Some(cpu) = c["cid"].as_str().and_then(|id| cpus.get(id)) else {
            continue;
        };
        let drives = c["storageId"]
            .as_str()
            .and_then(|id| storages.get(id))
            .and_then(|s| s["drives"].as_array().cloned())
            .unwrap_or_default();
        let storage = drives
            .iter()
            .map(|d| {
                format!(
                    "{}x {}GB {}",
                    d["count"],
                    d["size"],
                    d["type"].as_str().unwrap_or("")
                )
            })
            .collect::<Vec<_>>()
            .join(" + ");
        let storage_gb: f64 = drives
            .iter()
            .map(|d| d["count"].as_f64().unwrap_or(0.0) * d["size"].as_f64().unwrap_or(0.0))
            .sum();
        let count = cpu["count"].as_u64().unwrap_or(1) as u32;
        let family = cpu["family"].as_str().unwrap_or_default();
        let vendor = family.split_whitespace().next().unwrap_or_default();
        let cpu_name = format!("{vendor} {}", cpu["name"].as_str().unwrap_or_default());

        for ld in c["locationDistributions"].as_array().into_iter().flatten() {
            let Some(price) = ld["price"].as_str().and_then(parse_num) else {
                continue;
            };
            let loc_name = ld["location"].as_str().unwrap_or_default();
            let loc = locations.get(loc_name);
            let qty = ld["stockCount"].as_i64().unwrap_or(0);
            let port = ld["uplinkCapacities"].as_array().and_then(|u| {
                u.iter()
                    .filter_map(|x| x["bandwidth"].as_f64())
                    .reduce(f64::min)
            });

            let mut o = Offer::new(
                "datapacket",
                format!("{}@{loc_name}", c["id"].as_str().unwrap_or("?")),
            );
            o.name = if count > 1 {
                format!("{count}x {cpu_name} / {}GB", c["memory"])
            } else {
                format!("{cpu_name} / {}GB", c["memory"])
            };
            o.cpu = cpu_name.clone();
            o.cpu_count = count;
            o.cores = cpu["cores"].as_u64().map(|x| x as u32);
            o.threads = cpu["threads"].as_u64().map(|x| x as u32);
            o.provider_cpu_mark = cpu["passmarkScore"].as_f64();
            o.ram_gb = c["memory"].as_f64();
            o.ecc = Some(true);
            o.storage = storage.clone();
            o.storage_gb = (storage_gb > 0.0).then_some(storage_gb);
            o.nvme = Some(drives.iter().any(|d| d["type"] == "NVMe"));
            o.port_mbps = port.map(|g| (g * 1000.0) as u32);
            o.traffic = cpu["traffic"]["minimum"]
                .as_f64()
                .map(|t| format!("{t} Gbps committed"));
            o.location = loc_name.to_string();
            o.country = loc
                .and_then(|l| l["countryIso"].as_str())
                .unwrap_or_default()
                .to_string();
            o.price_monthly = price;
            o.currency = currency;
            o.stock = if qty > 0 {
                Stock::InStock
            } else {
                Stock::OutOfStock
            };
            o.stock_qty = Some(qty);
            o.availability = loc
                .and_then(|l| l["stockDeliveryTime"].as_i64())
                .filter(|_| qty > 0)
                .map(|h| format!("{h}h"));
            o.url = "https://www.datapacket.com/server-configuration".into();
            out.push(o);
        }
    }
    out
}
