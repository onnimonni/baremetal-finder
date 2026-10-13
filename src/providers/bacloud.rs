//! Bacloud (Lithuania, UK, NL): bare-metal cards; each location flag's title
//! ends in "(Out of Stock)" when that location is empty.

use anyhow::Result;
use scraper::{ElementRef, Html, Selector};

use crate::model::{Offer, Stock};
use crate::util::{clean, first_num, get_text, parse_storage};

const BASE: &str = "https://www.bacloud.com";
const PAGES: &[&str] = &["/en/bare-metal-servers", "/en/dual-dedicated-servers"];

fn sel(s: &str) -> Selector {
    Selector::parse(s).expect("static CSS selector is valid")
}

fn text(e: ElementRef) -> String {
    clean(&e.text().collect::<Vec<_>>().join(" "))
}

fn country(title: &str) -> &'static str {
    let t = title.to_lowercase();
    if t.contains("uk") || t.contains("coventry") || t.contains("london") {
        "GB"
    } else if t.contains("netherlands") {
        "NL"
    } else if t.contains("lithuania") {
        "LT"
    } else if t.contains("usa") || t.contains("united states") {
        "US"
    } else {
        ""
    }
}

pub async fn fetch(client: &reqwest::Client) -> Result<Vec<Offer>> {
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for page in PAGES {
        let html = get_text(client, &format!("{BASE}{page}")).await?;
        let doc = Html::parse_document(&html);
        for card in doc.select(&sel("div.product")) {
            let get = |css: &str| card.select(&sel(css)).next().map(text).unwrap_or_default();
            let Some(price) = first_num(&get("span.current-price")) else {
                continue;
            };
            let name = get("h5.card-title");
            let cpu_text = get("span[name=cpu]");
            // "AMD EPYC™ 9355P 32C/64T"
            let ct = cpu_text
                .split_whitespace()
                .last()
                .unwrap_or_default()
                .to_string();
            let cpu = cpu_text.trim_end_matches(&ct).trim().to_string();
            let disk = {
                let s = get("span[name=storage]");
                if s.is_empty() {
                    get("span[name=hdd]")
                } else {
                    s
                }
            };
            let order = card
                .select(&sel("a[href*='pid=']"))
                .next()
                .and_then(|a| a.value().attr("href"))
                .map(|h| {
                    if h.starts_with("http") {
                        h.to_string()
                    } else {
                        format!("{BASE}/{}", h.trim_start_matches('/'))
                    }
                });
            let flags: Vec<String> = card
                .select(&sel("img.product-location-icon[title]"))
                .filter_map(|i| i.value().attr("title").map(String::from))
                .collect();
            for title in flags {
                let place = title.trim_end_matches(" (Out of Stock)").to_string();
                if !seen.insert(format!("{name}@{place}")) {
                    continue;
                }
                let mut o = Offer::new("bacloud", format!("{name}@{place}"));
                o.name = name.clone();
                o.cpu = cpu.clone();
                o.cores = ct.split('C').next().and_then(first_num).map(|c| c as u32);
                o.threads = ct.split('/').nth(1).and_then(first_num).map(|t| t as u32);
                o.ram_gb = first_num(&get("span[name=ram]"));
                (o.storage_gb, o.nvme) = parse_storage(&disk);
                o.storage = disk.clone();
                o.port_mbps = {
                    let p = get("span[name=speed]");
                    first_num(&p).map(|n| if p.to_lowercase().contains('g') { n * 1000.0 } else { n } as u32)
                };
                o.location = place.clone();
                o.country = country(&place).into();
                o.price_monthly = price;
                o.stock = if title.ends_with("(Out of Stock)") {
                    Stock::OutOfStock
                } else {
                    Stock::InStock
                };
                o.spec_text = format!("{} {disk}", get("span[name=ram]"));
                o.url = order.clone().unwrap_or_else(|| format!("{BASE}{page}"));
                out.push(o);
            }
        }
    }
    Ok(out)
}
