//! DigitalOcean Droplets: sizes and prices from the pricing page's Next.js
//! data; which plan classes each region offers from the docs' availability
//! table (per class, not per size).

use std::collections::HashMap;

use anyhow::{Context, Result};
use scraper::{ElementRef, Html, Selector};
use serde_json::Value;

use crate::model::{Offer, Stock};
use crate::util::{clean, get_text, json_after, next_flight};

const PRICING: &str = "https://www.digitalocean.com/pricing/droplets";
const REGIONS: &str = "https://docs.digitalocean.com/platform/regional-availability/";

/// Pricing data keys -> availability table row labels.
const CLASSES: &[(&str, &str, bool)] = &[
    ("basic", "Basic", true),
    ("generalPurpose", "General Purpose", false),
    ("cpuOptimized", "CPU-Optimized", false),
    ("memoryOptimized", "Memory-Optimized", false),
    ("storageOptimized", "Storage-Optimized", false),
];

fn sel(s: &str) -> Selector {
    Selector::parse(s).expect("static CSS selector is valid")
}

fn region_country(code: &str) -> &'static str {
    match code.get(..3).unwrap_or_default() {
        "AMS" => "NL",
        "LON" => "GB",
        "FRA" => "DE",
        "SGP" => "SG",
        "TOR" => "CA",
        "BLR" => "IN",
        "SYD" => "AU",
        _ => "US",
    }
}

/// class label -> regions where it is (at least partly) offered.
fn class_regions(html: &str) -> HashMap<String, Vec<String>> {
    let doc = Html::parse_document(html);
    let mut out = HashMap::new();
    for table in doc.select(&sel("table")) {
        let header: Vec<String> = table
            .select(&sel("thead th"))
            .map(|h| clean(&h.text().collect::<String>()))
            .collect();
        if !header.iter().any(|h| h == "AMS3") {
            continue;
        }
        for row in table.select(&sel("tbody tr")) {
            let cells: Vec<ElementRef> = row.select(&sel("td")).collect();
            let Some(label) = cells.first().map(|c| clean(&c.text().collect::<String>())) else {
                continue;
            };
            if out.contains_key(&label) {
                continue;
            }
            let regions = cells
                .iter()
                .enumerate()
                .skip(1)
                .filter(|(_, c)| c.select(&sel("i[class*='fa-circle']")).next().is_some())
                .filter_map(|(i, _)| header.get(i).cloned())
                .collect();
            out.insert(label, regions);
        }
    }
    out
}

pub async fn fetch(client: &reqwest::Client) -> Result<Vec<Offer>> {
    let html = get_text(client, PRICING).await?;
    let data = json_after(&next_flight(&html), "data").context("digitalocean: no pricing data")?;
    let availability = class_regions(&get_text(client, REGIONS).await?);
    let mut out = Vec::new();
    for (key, label, shared) in CLASSES {
        let regions = availability.get(*label).cloned().unwrap_or_default();
        let tiers = data[*key].as_object().cloned().unwrap_or_default();
        for (tier, sizes) in tiers {
            for s in sizes.as_array().into_iter().flatten() {
                let Some(price) = s["price"]["monthly"].as_f64() else {
                    continue;
                };
                let slug = s["slug"].as_str().unwrap_or_default();
                for region in &regions {
                    out.push(offer(s, slug, &tier, *shared, price, region));
                }
            }
        }
    }
    Ok(out)
}

fn offer(s: &Value, slug: &str, tier: &str, shared: bool, price: f64, region: &str) -> Offer {
    let vcpus = s["cpus"].as_u64().unwrap_or(0) as u32;
    let mut o = Offer::new("digitalocean", format!("{slug}@{region}"));
    o.kind = "vps";
    o.name = slug.to_string();
    o.cpu = match tier {
        "premiumIntel" => "Intel Xeon (DigitalOcean Premium)".into(),
        "premiumAMD" => "AMD EPYC (DigitalOcean Premium)".into(),
        _ => "Intel/AMD (DigitalOcean)".into(),
    };
    o.vcpus = Some(vcpus);
    o.threads = Some(vcpus);
    o.shared_cpu = Some(shared);
    o.ram_gb = s["memory"].as_f64();
    o.storage_gb = s["disk"]["boot"].as_f64();
    o.storage = format!("{} GB SSD", o.storage_gb.unwrap_or(0.0));
    o.nvme = Some(true);
    o.port_mbps = s["network"]["throughput"]
        .as_f64()
        .map(|g| (g * 1000.0) as u32);
    o.traffic = s["price"]["transferQuota"]
        .as_f64()
        .map(|gb| format!("{} TB", gb / 1000.0));
    o.location = region.to_string();
    o.country = region_country(region).into();
    o.price_monthly = price;
    o.currency = "USD";
    o.stock = Stock::Unknown;
    o.availability = Some("class offered in region".into());
    o.url = PRICING.into();
    o
}
