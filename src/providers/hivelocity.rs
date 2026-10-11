//! Hivelocity: tokenless store inventory JSON (keyed by datacenter) with
//! `stock` available/limited/unavailable and a count when limited.

use std::sync::LazyLock;

use anyhow::Result;
use regex::Regex;
use serde_json::Value;

use crate::model::{Offer, Stock};
use crate::util::{first_num, get_json};

const URL: &str = "https://core.hivelocity.net/api/v2/inventory/store";

fn dc_country(dc: &str) -> Option<&'static str> {
    let code = dc.trim_start_matches("EDGE-");
    Some(match &code[..code.len().saturating_sub(1).min(3)] {
        "AMS" => "NL",
        "FRA" => "DE",
        "LON" => "GB",
        "ARN" => "SE",
        "CDG" => "FR",
        "LIN" => "IT",
        "TOJ" => "ES",
        "YYZ" | "YXX" => "CA",
        "BOM" => "IN",
        "HKG" => "HK",
        "NRT" => "JP",
        "SAO" => "BR",
        "SIN" => "SG",
        "SYD" => "AU",
        "ALB" | "ATL" | "DAL" | "IAD" | "LAX" | "MIA" | "OGB" | "ORD" | "SEA" | "SNV" | "TPA" => {
            "US"
        }
        _ => return None,
    })
}

pub async fn fetch(client: &reqwest::Client) -> Result<Vec<Offer>> {
    static COUNT: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"^(\d+)\s*x\s*").expect("static regex is valid"));
    static GHZ: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"\s+[\d.]+\s*GHz.*$").expect("static regex is valid"));
    let v = get_json(client, URL).await?;
    let mut out = Vec::new();
    for (dc, products) in v.as_object().into_iter().flatten() {
        if dc.starts_with("DEV-") {
            continue;
        }
        for p in products.as_array().into_iter().flatten() {
            if !matches!(p["compute_type"].as_str(), Some("dedicated" | "outlet")) {
                continue;
            }
            let Some(base) = p["product_monthly_price"].as_f64() else {
                continue;
            };
            let raw_cpu = p["product_cpu"].as_str().unwrap_or_default();
            // "2 x 4310 2.1GHz Xeon Scalable" -> "Xeon 4310"
            let mut cpu = GHZ.replace(&COUNT.replace(raw_cpu, ""), "").to_string();
            if raw_cpu.contains("Xeon") && !cpu.contains("Xeon") {
                cpu = format!("Xeon {cpu}");
            }
            let info = &p["processor_info"];
            let sockets = info["sockets"].as_u64().unwrap_or(1) as u32;
            let disks: Vec<Value> = p["product_drive_info"]["disks"]
                .as_array()
                .cloned()
                .unwrap_or_default();
            let total_gb = p["product_drive_info"]["total_size"]["size"]
                .as_f64()
                .map(|s| {
                    if p["product_drive_info"]["total_size"]["units"] == "tb" {
                        s * 1000.0
                    } else {
                        s
                    }
                });
            let bw = p["product_bandwidth"].as_str().unwrap_or_default();
            let qty = p["limited_stock"].as_i64();
            let stock = p["stock"].as_str().unwrap_or("unavailable");

            let mut o = Offer::new("hivelocity", format!("{}@{dc}", p["product_id"]));
            o.name = p["product_name"].as_str().unwrap_or(raw_cpu).to_string();
            o.cpu = cpu;
            o.cpu_count = sockets;
            o.cores = info["cores"].as_u64().map(|c| c as u32 * sockets);
            o.threads = info["threads"].as_u64().map(|t| t as u32 * sockets);
            o.ram_gb = p["product_memory"].as_str().and_then(first_num);
            o.storage = p["product_drive"].as_str().unwrap_or_default().to_string();
            o.storage_gb = total_gb;
            o.nvme = Some(
                disks
                    .iter()
                    .any(|d| d["nvme"] == true || d["type"] == "nvme"),
            );
            // "100TB / 1Gbps"
            o.port_mbps = bw
                .split('/')
                .nth(1)
                .and_then(first_num)
                .map(|g| (g * 1000.0) as u32);
            o.traffic = bw.split('/').next().map(|t| t.trim().to_string());
            o.location = dc.clone();
            o.country = dc_country(dc).unwrap_or_default().to_string();
            // The store UI adds the location premium to the base price.
            o.price_monthly = base + p["monthly_location_premium"].as_f64().unwrap_or(0.0);
            o.currency = "USD";
            o.stock = match stock {
                "available" | "limited" => Stock::InStock,
                _ => Stock::OutOfStock,
            };
            o.stock_qty = if stock == "limited" { qty } else { None };
            o.availability = Some(stock.to_string());
            o.url = format!("https://store.hivelocity.net/product/{}", p["product_id"]);
            if p["compute_type"] == "outlet" {
                o.note = Some("outlet (used)".into());
            }
            out.push(o);
        }
    }
    Ok(out)
}
