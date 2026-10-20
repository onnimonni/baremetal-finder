//! Alwyzon (Vienna, AT): VPS and dedicated offers in the Next.js page data,
//! with an `outOfStock` flag. Prices are net EUR.

use anyhow::{Context, Result};
use scraper::{Html, Selector};
use serde_json::Value;

use crate::model::{Offer, Stock};
use crate::util::{clean, get_text};

const URL: &str = "https://www.alwyzon.com/en/dedicated-servers";

pub async fn fetch(client: &reqwest::Client) -> Result<Vec<Offer>> {
    let html = get_text(client, URL).await?;
    let data: Value = {
        let doc = Html::parse_document(&html);
        let next = Selector::parse("script#__NEXT_DATA__").expect("static CSS selector is valid");
        let json = doc
            .select(&next)
            .next()
            .context("alwyzon: no __NEXT_DATA__")?
            .inner_html();
        serde_json::from_str(&json)?
    };
    let mut out = Vec::new();
    for s in data["props"]["pageProps"]["offers"]
        .as_array()
        .into_iter()
        .flatten()
    {
        let Some(price) = s["price"]["amount"].as_f64() else {
            continue;
        };
        let kind = s["type"].as_str().unwrap_or_default();
        let cpu = &s["cpu"];
        let brand = match cpu["brand"].as_str() {
            Some("amd") => "AMD",
            _ => "Intel",
        };
        let disk = &s["disk"];
        let size = disk["sizeInGB"].as_f64().unwrap_or(0.0);
        let count = disk["count"].as_f64().unwrap_or(1.0);
        let dtype = format!(
            "{} {}",
            disk["fs"].as_str().unwrap_or_default(),
            disk["type"].as_str().unwrap_or_default()
        );
        let mut o = Offer::new(
            "alwyzon",
            s["slug"].as_str().unwrap_or_default().to_string(),
        );
        o.name = s["name"].as_str().unwrap_or_default().to_string();
        o.cpu = clean(&format!(
            "{brand} {}",
            cpu["model"]
                .as_str()
                .unwrap_or_default()
                .replace("<sup>", "")
                .replace("</sup>", "")
        ));
        if kind == "dedicated" {
            o.cores = cpu["cores"].as_u64().map(|c| c as u32);
            o.threads = cpu["threads"].as_u64().map(|c| c as u32);
        } else {
            o.kind = "vps";
            o.vcpus = cpu["vCores"].as_u64().map(|c| c as u32);
            o.threads = o.vcpus;
            o.shared_cpu = Some(true);
        }
        o.ram_gb = s["ram"]["amountInMB"].as_f64().map(|m| m / 1024.0);
        o.ecc = s["ram"]["ecc"].as_bool();
        o.storage_gb = Some(size * count);
        o.storage = format!("{count}x {size} GB {dtype}");
        o.nvme = Some(dtype.contains("NVMe"));
        o.port_mbps = s["network"]["uplinkInMbps"].as_u64().map(|m| m as u32);
        o.traffic = s["network"]["bandwidthFormatted"]
            .as_str()
            .map(String::from);
        o.location = s["location"]["city"]
            .as_str()
            .unwrap_or("Vienna")
            .to_string();
        o.country = s["location"]["country"]
            .as_str()
            .unwrap_or("AT")
            .to_string();
        o.price_monthly = price;
        o.setup_fee = s["setup"]["amount"].as_f64().unwrap_or(0.0);
        o.stock = if s["outOfStock"] == true {
            Stock::OutOfStock
        } else {
            Stock::InStock
        };
        o.spec_text = s["ram"]["type"].as_str().unwrap_or_default().to_string();
        o.url = URL.into();
        out.push(o);
    }
    anyhow::ensure!(!out.is_empty(), "alwyzon: no offers");
    Ok(out)
}
