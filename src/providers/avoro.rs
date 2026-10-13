//! avoro (dataforest, DE): "instant dedicated" lists physical servers that
//! are ready now, one card per machine. Prices include 19% VAT. The site is
//! behind a Cloudflare JavaScript challenge; with the `webview` feature the
//! page is loaded through the OS web engine first.

use std::sync::OnceLock;

use anyhow::Result;
use scraper::{ElementRef, Html, Selector};

use crate::model::{Offer, Stock};
use crate::util::{browser, browser_text, clean, first_num, parse_storage};

const URL: &str = "https://avoro.eu/de/instant-dedicated";

/// HTML loaded by the webview before the async fetch starts.
static PREFETCHED: OnceLock<String> = OnceLock::new();

/// Load the page through the OS web engine (blocking, main thread only).
#[cfg(feature = "webview")]
pub fn prefetch_with_webview() {
    // Ready once Cloudflare has redirected to the real page.
    let ready = "!document.title.includes('Just a moment') && document.querySelector('#serverList, .server-item')";
    match crate::webview::grab(URL, ready) {
        Ok(html) => {
            let _ = PREFETCHED.set(html);
        }
        Err(e) => eprintln!("avoro webview: {e:#}"),
    }
}

fn sel(s: &str) -> Selector {
    Selector::parse(s).expect("static CSS selector is valid")
}

fn text(e: ElementRef) -> String {
    clean(&e.text().collect::<String>())
}

pub async fn fetch(_client: &reqwest::Client) -> Result<Vec<Offer>> {
    let html = match PREFETCHED.get() {
        Some(h) => h.clone(),
        None => browser_text(&browser()?, URL).await.map_err(|e| {
            e.context("avoro is behind a Cloudflare JS challenge; build with --features webview")
        })?,
    };
    let doc = Html::parse_document(&html);
    let item = sel(".server-item");
    let title_s = sel("h5.server-item-title");
    let spec_s = sel(".server-specs p");
    let price_s = sel("h3.price");
    let mut out = Vec::new();
    for it in doc.select(&item) {
        let Some(title) = it.select(&title_s).next().map(text) else {
            continue;
        };
        let Some(gross) = it
            .select(&price_s)
            .next()
            .map(text)
            .and_then(|t| first_num(&t))
        else {
            continue;
        };
        let specs: Vec<String> = it.select(&spec_s).map(text).collect();
        let find = |k: &str| {
            specs
                .iter()
                .find(|s| s.contains(k))
                .cloned()
                .unwrap_or_default()
        };
        let cpu_line = find("CPU");
        let cpu = cpu_line
            .trim_start_matches("CPU:")
            .split(" (")
            .next()
            .unwrap_or_default()
            .trim()
            .to_string();
        let ram_disk = specs
            .iter()
            .find(|s| s.contains("RAM"))
            .cloned()
            .unwrap_or_default();
        let net = specs
            .iter()
            .find(|s| s.contains("GBit") || s.contains("Gbit"))
            .cloned()
            .unwrap_or_default();

        let mut o = Offer::new("avoro", title.clone());
        o.name = title;
        o.cpu = cpu;
        o.cores = cpu_line
            .split(" (")
            .nth(1)
            .and_then(first_num)
            .map(|c| c as u32);
        o.threads = cpu_line
            .split('&')
            .nth(1)
            .and_then(first_num)
            .map(|t| t as u32);
        // "RAM: 32 GB DDR5 RAM | Speicher: 1 TB NVMe"
        o.ram_gb = ram_disk.split("RAM:").nth(1).and_then(first_num);
        let disk = ram_disk
            .split("Speicher:")
            .nth(1)
            .unwrap_or_default()
            .trim()
            .to_string();
        (o.storage_gb, o.nvme) = parse_storage(&disk);
        o.storage = disk;
        o.port_mbps = first_num(&net).map(|g| (g * 1000.0) as u32);
        o.provider_cpu_mark = it.value().attr("data-cpu-score").and_then(first_num);
        o.location = "Germany".into();
        o.country = "DE".into();
        o.price_monthly = gross / 1.19;
        o.stock = Stock::InStock;
        o.stock_qty = Some(1);
        o.availability = Some("instant".into());
        o.spec_text = specs.join(" | ");
        o.url = URL.into();
        o.note = Some("VAT stripped; one physical unit".into());
        out.push(o);
    }
    Ok(out)
}
