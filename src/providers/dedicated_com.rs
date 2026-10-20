//! dedicated.com (US + Montreal): server cards with `data-*` specs and a
//! per-location "N left" counter.

use anyhow::Result;
use scraper::{ElementRef, Html, Selector};

use crate::model::{Offer, Stock};
use crate::util::{clean, first_num, get_text, parse_storage};

const URL: &str = "https://www.dedicated.com/dedicated-servers";

fn sel(s: &str) -> Selector {
    Selector::parse(s).expect("static CSS selector is valid")
}

fn place(code: &str) -> (&'static str, &'static str) {
    match code {
        "atl" => ("US", "Atlanta"),
        "chi" => ("US", "Chicago"),
        "dal" => ("US", "Dallas"),
        "la" => ("US", "Los Angeles"),
        "mtl" => ("CA", "Montreal"),
        "nyc" => ("US", "New York"),
        "sea" => ("US", "Seattle"),
        _ => ("US", "US"),
    }
}

pub async fn fetch(client: &reqwest::Client) -> Result<Vec<Offer>> {
    let html = get_text(client, URL).await?;
    let doc = Html::parse_document(&html);
    let num = sel(".loc-stock-num");
    let mut out = Vec::new();
    for card in doc.select(&sel("div.single[data-pid]")) {
        let attr = |k: &str| card.value().attr(k).unwrap_or_default();
        let Some(price) = first_num(attr("data-price")) else {
            continue;
        };
        let pid = attr("data-pid");
        // "Intel Xeon Dual Gold 6140 | 256GB - 2 x 2TB SSD"
        let spec = clean(attr("data-cpu"));
        let (cpu, rest) = spec.split_once(" | ").unwrap_or((&spec, ""));
        let disk = rest
            .split_once(" - ")
            .map_or(attr("data-storage"), |(_, d)| d)
            .to_string();
        let order = card
            .select(&sel("a[href*='cart.php']"))
            .next()
            .and_then(|a| a.value().attr("href"))
            .map_or_else(|| URL.to_string(), String::from);
        let items: Vec<ElementRef> = card.select(&sel("li.location-item")).collect();
        for li in items {
            let classes: Vec<&str> = li.value().classes().collect();
            let Some(code) = classes.iter().find(|c| **c != "location-item").copied() else {
                continue;
            };
            // "na" = not offered here
            if classes.contains(&"na") {
                continue;
            }
            let qty = li
                .select(&num)
                .next()
                .and_then(|n| first_num(&n.text().collect::<String>()))
                .map(|n| n as i64);
            let (country, city) = place(code);
            let mut o = Offer::new("dedicatedcom", format!("{pid}@{code}"));
            o.name = spec.clone();
            o.cpu = cpu.to_string();
            o.ram_gb = first_num(attr("data-ram"));
            (o.storage_gb, o.nvme) = parse_storage(&disk);
            o.storage = disk.clone();
            o.port_mbps = first_num(attr("data-port")).map(|g| (g * 1000.0) as u32);
            o.location = city.into();
            o.country = country.into();
            o.price_monthly = price;
            o.currency = "USD";
            o.stock_qty = qty;
            o.stock = match qty {
                Some(n) if n > 0 && !classes.contains(&"out") => Stock::InStock,
                _ => Stock::OutOfStock,
            };
            o.provider_cpu_mark = first_num(attr("data-benchmark")).filter(|b| *b > 0.0);
            o.spec_text = attr("data-ram").to_string();
            o.url = order.clone();
            out.push(o);
        }
    }
    anyhow::ensure!(!out.is_empty(), "dedicated.com: no servers");
    Ok(out)
}
