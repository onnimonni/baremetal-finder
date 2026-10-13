//! Scaleway Dedibox: the pricing page's Next.js data has hardware, per-DC
//! prices and a (global, per-offer) `hasStock` flag.

use std::sync::LazyLock;

use anyhow::{Context, Result};
use regex::Regex;
use serde_json::Value;

use crate::model::{Offer, Stock};
use crate::util::{clean, get_json, get_text};

const SITE: &str = "https://www.scaleway.com";

fn money(m: &Value) -> f64 {
    m["units"].as_f64().unwrap_or(0.0) + m["nanos"].as_f64().unwrap_or(0.0) / 1e9
}

fn dc_location(dc: &str) -> (&'static str, &'static str) {
    match dc {
        "ams1" => ("NL", "Amsterdam (ams1)"),
        "dc2" => ("FR", "Paris (dc2)"),
        "dc3" => ("FR", "Paris (dc3)"),
        "dc5" => ("FR", "Paris (dc5)"),
        _ => ("FR", "Paris"),
    }
}

pub async fn fetch(client: &reqwest::Client) -> Result<Vec<Offer>> {
    static BUILD: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r#""buildId":"([^"]+)""#).expect("static regex is valid"));
    let html = get_text(client, &format!("{SITE}/en/pricing/")).await?;
    let build = BUILD.captures(&html).context("dedibox: no buildId")?;
    let v = get_json(
        client,
        &format!("{SITE}/_next/data/{}/en/pricing/dedibox.json", &build[1]),
    )
    .await?;
    let p = &v["pageProps"]["externalData"]["templates.pricing-page"];
    let stocks = &p["stocks"]["dedibox"];

    let mut out = Vec::new();
    for range in p["catalogProducts"]["dediboxRanges"]
        .as_array()
        .into_iter()
        .flatten()
    {
        for prod in range["products"].as_array().into_iter().flatten() {
            let hw = &prod["hardware"];
            let phys = &hw["cpu"]["physical"];
            let sockets = phys["sockets"].as_u64().unwrap_or(1) as u32;
            let cores = phys["coresPerSocket"].as_u64().map(|c| c as u32 * sockets);
            let disks: Vec<&Value> = hw["storages"]
                .as_array()
                .map(|a| a.iter().collect())
                .unwrap_or_default();
            let storage = disks
                .iter()
                .map(|d| {
                    format!(
                        "{}x {:.0}GB {}",
                        d["count"],
                        d["fixedLocal"]["size"].as_f64().unwrap_or(0.0) / 1e9,
                        d["fixedLocal"]["type"].as_str().unwrap_or_default()
                    )
                })
                .collect::<Vec<_>>()
                .join(" + ");
            let storage_gb: f64 = disks
                .iter()
                .map(|d| {
                    d["count"].as_f64().unwrap_or(1.0)
                        * d["fixedLocal"]["size"].as_f64().unwrap_or(0.0)
                        / 1e9
                })
                .sum();
            for dc in prod["dataCenters"].as_array().into_iter().flatten() {
                let price = &dc["price"];
                let sku = price["monthly"]["sku"].as_str().unwrap_or_default();
                // "/dedibox/core-10-l/monthly/nl-ams/dc/ams1" -> "core-10-l"
                let key = sku.split('/').nth(2).unwrap_or_default();
                let dc_code = dc["dataCenter"].as_str().unwrap_or_default();
                let (country, city) = dc_location(dc_code);
                let mut o = Offer::new("dedibox", sku);
                o.name = prod["name"].as_str().unwrap_or(key).to_string();
                o.cpu = clean(hw["cpu"]["type"].as_str().unwrap_or_default());
                o.cpu_count = sockets;
                o.cores = cores;
                o.threads = cores
                    .zip(phys["threadsPerCore"].as_u64())
                    .map(|(c, t)| c * t as u32);
                o.provider_cpu_mark = phys["benchmark"].as_f64();
                o.ram_gb = hw["ram"]["size"]
                    .as_f64()
                    .map(|b| (b / 1024f64.powi(3)).round());
                o.storage = storage.clone();
                o.storage_gb = (storage_gb > 0.0).then_some(storage_gb.round());
                o.nvme = Some(storage.contains("NVME"));
                o.port_mbps = hw["network"]["publicBandwidth"]
                    .as_f64()
                    .map(|b| (b / 1e6) as u32);
                o.location = city.into();
                o.country = country.into();
                o.price_monthly = money(&price["monthly"]["value"]);
                o.setup_fee = money(&price["setup"]["value"]);
                o.stock = match stocks[key]["hasStock"].as_bool() {
                    Some(true) => Stock::InStock,
                    Some(false) => Stock::OutOfStock,
                    None => Stock::Unknown,
                };
                o.spec_text = format!(
                    "{} {}",
                    hw["ram"]["type"].as_str().unwrap_or_default(),
                    o.storage
                );
                o.url = format!("{SITE}/en/dedibox/");
                o.note = Some(
                    "stock flag is per offer, not per DC; setup waived with 12M+ commitment".into(),
                );
                out.push(o);
            }
        }
    }
    Ok(out)
}
