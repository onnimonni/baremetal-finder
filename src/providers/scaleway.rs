//! Scaleway Elastic Metal: tokenless public product catalog. Stock per zone
//! needs an API key (`baremetal/v1/.../offers`), so stock is `unknown`
//! unless SCW_SECRET_KEY is set.

use std::collections::HashMap;

use anyhow::Result;
use serde_json::Value;

use crate::model::{Offer, Stock};
use crate::util::get_json;

const CATALOG: &str = "https://api.scaleway.com/product-catalog/v2alpha1/public-catalog/products?product_types=elastic_metal&page_size=100";

fn money(p: &Value) -> f64 {
    let r = &p["retail_price"];
    r["units"].as_f64().unwrap_or(0.0) + r["nanos"].as_f64().unwrap_or(0.0) / 1e9
}

pub async fn fetch(client: &reqwest::Client) -> Result<Vec<Offer>> {
    let mut products = Vec::new();
    for page in 1.. {
        let v = get_json(client, &format!("{CATALOG}&page={page}")).await?;
        let batch = v["products"].as_array().cloned().unwrap_or_default();
        let total = v["total_count"].as_u64().unwrap_or(0) as usize;
        let done = batch.is_empty();
        products.extend(batch);
        if done || products.len() >= total {
            break;
        }
    }
    let fees: HashMap<String, f64> = products
        .iter()
        .filter_map(|p| {
            Some((
                p["sku"].as_str()?.strip_suffix("/fees")?.to_string(),
                money(&p["price"]),
            ))
        })
        .collect();
    let stock = match std::env::var("SCW_SECRET_KEY") {
        Ok(key) => api_stock(client, &key).await.unwrap_or_else(|e| {
            eprintln!("scaleway stock: {e:#}");
            HashMap::new()
        }),
        Err(_) => HashMap::new(),
    };

    let mut out = Vec::new();
    for p in &products {
        let sku = p["sku"].as_str().unwrap_or_default();
        if p["status"] != "general_availability"
            || p["unit_of_measure"]["unit"] != "month"
            || sku.ends_with("/fees")
        {
            continue;
        }
        let hw = &p["properties"]["hardware"];
        let phys = &hw["cpu"]["physical"];
        let sockets = phys["sockets"].as_u64().unwrap_or(1) as u32;
        let zone = p["locality"]["zone"].as_str().unwrap_or_default();
        let offer_id = p["properties"]["elastic_metal"]["offer_id"]
            .as_str()
            .unwrap_or_default();
        let storage = hw["storage"]["description"].as_str().unwrap_or_default();
        let storage = storage.strip_prefix("Fixed local: ").unwrap_or(storage);

        let mut o = Offer::new("scaleway", sku);
        o.name = p["product"].as_str().unwrap_or(sku).to_string();
        o.cpu = hw["cpu"]["type"]
            .as_str()
            .unwrap_or_default()
            .replace(" or equivalent", "");
        o.cpu_count = sockets;
        o.cores = phys["cores_per_socket"]
            .as_u64()
            .map(|c| c as u32 * sockets);
        o.threads = hw["cpu"]["threads"].as_u64().map(|t| t as u32);
        o.ram_gb = hw["ram"]["size"]
            .as_f64()
            .map(|b| (b / 1024f64.powi(3)).round());
        o.ecc = hw["ram"]["ecc_type"].as_str().map(|e| e != "none");
        o.storage = storage.to_string();
        o.storage_gb = hw["storage"]["total"].as_f64().map(|b| (b / 1e9).round());
        o.nvme = Some(storage.to_lowercase().contains("nvme"));
        o.port_mbps = hw["network"]["public_bandwidth"]
            .as_f64()
            .map(|b| (b / 1e6) as u32);
        o.location = zone.to_string();
        o.country = zone.get(..2).unwrap_or_default().to_uppercase();
        o.price_monthly = money(&p["price"]);
        o.setup_fee = fees.get(sku).copied().unwrap_or(0.0);
        o.currency = "EUR";
        (o.stock, o.availability) = match stock.get(&(zone.to_string(), offer_id.to_string())) {
            Some(s) if s == "available" || s == "low" => (Stock::InStock, Some(s.clone())),
            Some(s) if s == "empty" => (Stock::OutOfStock, Some(s.clone())),
            Some(s) => (Stock::Unknown, Some(s.clone())),
            None => (Stock::Unknown, None),
        };
        o.spec_text = format!(
            "{} {}",
            hw["ram"]["type"].as_str().unwrap_or_default(),
            o.storage
        );
        o.url = "https://www.scaleway.com/en/pricing/elastic-metal/".into();
        out.push(o);
    }
    Ok(out)
}

/// (zone, offer_id) -> stock ("available" | "low" | "empty") via the
/// authenticated Elastic Metal API.
async fn api_stock(
    client: &reqwest::Client,
    key: &str,
) -> Result<HashMap<(String, String), String>> {
    let mut out = HashMap::new();
    for zone in [
        "fr-par-1", "fr-par-2", "nl-ams-1", "nl-ams-2", "pl-waw-2", "pl-waw-3",
    ] {
        let v: Value = client
            .get(format!(
                "https://api.scaleway.com/baremetal/v1/zones/{zone}/offers?page_size=100"
            ))
            .header("X-Auth-Token", key)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        for o in v["offers"].as_array().into_iter().flatten() {
            if let (Some(id), Some(s)) = (o["id"].as_str(), o["stock"].as_str()) {
                out.insert((zone.to_string(), id.to_string()), s.to_string());
            }
        }
    }
    Ok(out)
}
