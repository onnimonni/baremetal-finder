//! NovoServe (NL + New York): WHMCS store category pages with "N Available"
//! counters. Prices are excl. VAT.

use std::sync::LazyLock;

use anyhow::Result;
use regex::Regex;
use scraper::{ElementRef, Html, Selector};

use crate::model::{Offer, Stock};
use crate::util::{clean, first_num, get_text, parse_storage};

/// (portal, category slugs, country, city, currency)
const PORTALS: &[(&str, &[&str], &str, &str, &str)] = &[
    (
        "https://portal.novoserve.com",
        &[
            "promo",
            "budget",
            "intel-xeon-server",
            "amd-epyc-server",
            "unmetered",
            "storage",
        ],
        "NL",
        "Amsterdam",
        "EUR",
    ),
    (
        "https://portal.us.novoserve.com",
        &[
            "promo",
            "dedicated-servers",
            "amd-epyc-servers",
            "storage-servers",
            "unmetered",
        ],
        "US",
        "New York",
        "USD",
    ),
];

fn sel(s: &str) -> Selector {
    Selector::parse(s).expect("static CSS selector is valid")
}

fn text(e: ElementRef) -> String {
    clean(&e.text().collect::<Vec<_>>().join(" "))
}

pub async fn fetch(client: &reqwest::Client) -> Result<Vec<Offer>> {
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for (base, cats, country, city, currency) in PORTALS {
        for cat in *cats {
            let html = match get_text(client, &format!("{base}/store/{cat}")).await {
                Ok(h) => h,
                Err(e) => {
                    eprintln!("novoserve {cat}: {e:#}");
                    continue;
                }
            };
            for mut o in parse(&html, base) {
                if seen.insert(o.sku.clone()) {
                    o.country = (*country).into();
                    o.location = (*city).into();
                    o.currency = crate::util::currency(currency).unwrap_or("EUR");
                    out.push(o);
                }
            }
        }
    }
    Ok(out)
}

fn parse(html: &str, base: &str) -> Vec<Offer> {
    static CPU: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(
            r"(?i)((\d+)\s*x\s*)?((intel|amd)?\s*(xeon|epyc|ryzen|core)[^•,&]*?)(\s+cpu)?\s*(•|$)",
        )
        .expect("static regex is valid")
    });
    static RAM: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"(?i)(\d+)\s*GB\s*(DDR\d\s*)?RAM").expect("static regex is valid")
    });
    static DISK: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"(?i)(\d+\s*x\s*)?\d+(\.\d+)?\s*(GB|TB)\s*(NVMe|SSD|HDD|SAS|SATA)[^•]*")
            .expect("static regex is valid")
    });
    let doc = Html::parse_document(html);
    let card = sel("div.product[id^=product]");
    let name_s = sel("span[id$=-name]");
    let qty_s = sel("header span.qty");
    let desc_s = sel("p[id$=-description]");
    let price_s = sel("div.product-pricing span.price");
    let link_s = sel("a.btn-order-now");
    let mut out = Vec::new();
    for c in doc.select(&card) {
        let Some(price) = c
            .select(&price_s)
            .next()
            .map(text)
            .and_then(|t| first_num(&t))
        else {
            continue;
        };
        let desc = c.select(&desc_s).next().map(text).unwrap_or_default();
        let href = c
            .select(&link_s)
            .next()
            .and_then(|a| a.value().attr("href"))
            .unwrap_or_default();
        let qty = c
            .select(&qty_s)
            .next()
            .map(text)
            .and_then(|t| first_num(&t))
            .map(|q| q as i64);
        let cpu = CPU.captures(&desc);
        let disk = DISK
            .find(&desc)
            .map(|m| m.as_str().trim().to_string())
            .unwrap_or_default();

        let mut o = Offer::new("novoserve", format!("{base}{href}"));
        o.name = c.select(&name_s).next().map(text).unwrap_or_default();
        o.cpu = cpu
            .as_ref()
            .map(|m| m[3].trim().to_string())
            .unwrap_or_default();
        o.cpu_count = cpu
            .as_ref()
            .and_then(|m| m.get(2))
            .and_then(|n| n.as_str().parse().ok())
            .unwrap_or(1);
        o.ram_gb = RAM.captures(&desc).and_then(|m| m[1].parse().ok());
        (o.storage_gb, o.nvme) = parse_storage(&disk);
        o.storage = disk;
        o.price_monthly = price;
        o.stock_qty = qty;
        o.stock = match qty {
            Some(n) if n > 0 => Stock::InStock,
            Some(_) => Stock::OutOfStock,
            None => Stock::Unknown,
        };
        o.spec_text = desc;
        o.url = format!("{base}{href}");
        out.push(o);
    }
    out
}
