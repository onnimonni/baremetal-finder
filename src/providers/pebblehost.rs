//! PebbleHost: HTML listing (data-* attributes) + per-plan stock JSON
//! `/ajax/dedicated-stock?pid=N` -> {"stock": bool}.

use std::sync::LazyLock;

use anyhow::Result;
use regex::Regex;
use scraper::{ElementRef, Html, Node, Selector};

use crate::model::{Offer, Stock};
use crate::util::{clean, first_num, get_json, get_text, parse_storage};

const BASE: &str = "https://pebblehost.com";

fn sel(s: &str) -> Selector {
    Selector::parse(s).unwrap()
}

fn text(e: ElementRef) -> String {
    clean(&e.text().collect::<String>())
}

/// Text directly inside an element, ignoring nested tooltips/upgrades.
fn own_text(e: ElementRef) -> String {
    clean(
        &e.children()
            .filter_map(|n| match n.value() {
                Node::Text(t) => Some(t.to_string()),
                _ => None,
            })
            .collect::<String>(),
    )
}

pub async fn fetch(client: &reqwest::Client) -> Result<Vec<Offer>> {
    let html = get_text(client, &format!("{BASE}/dedicated/")).await?;
    let mut offers = parse(&html);
    for o in &mut offers {
        match get_json(
            client,
            &format!("{BASE}/ajax/dedicated-stock?pid={}", o.sku),
        )
        .await
        {
            Ok(v) => {
                o.stock = match v["stock"].as_bool() {
                    Some(true) => Stock::InStock,
                    Some(false) => Stock::OutOfStock,
                    None => Stock::Unknown,
                }
            }
            Err(e) => eprintln!("pebblehost stock {}: {e:#}", o.sku),
        }
    }
    Ok(offers)
}

fn parse(html: &str) -> Vec<Offer> {
    static CT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\((\d+)c/(\d+)t\)").unwrap());
    let doc = Html::parse_document(html);
    let row = sel("tr[data-stockurl]");
    let td = sel("td");
    let name_sel = sel("p.dedicated-smalltext");
    let cpu_sel = sel("b");
    let mut out = Vec::new();
    for r in doc.select(&row) {
        let a = |k: &str| r.value().attr(k).unwrap_or_default();
        let Some(pid) = a("data-stockurl").split("server=").nth(1) else {
            continue;
        };
        let Some(cents) = first_num(a("data-priceamount")) else {
            continue;
        };
        let cells: Vec<ElementRef> = r.select(&td).collect();
        let Some(first) = cells.first() else { continue };
        let cpu = first.select(&cpu_sel).next().map(text).unwrap_or_default();
        let detail = first
            .select(&name_sel)
            .map(text)
            .collect::<Vec<_>>()
            .join(" ");
        let disk = cells.get(3).map(|c| own_text(*c)).unwrap_or_default();
        let loc = r
            .value()
            .classes()
            .find_map(|c| c.strip_prefix("loc-"))
            .unwrap_or_default()
            .to_uppercase();

        let mut o = Offer::new("pebblehost", pid);
        o.name = first.select(&name_sel).next().map(text).unwrap_or_default();
        o.cpu = cpu;
        if let Some(c) = CT.captures(&detail) {
            o.cores = c[1].parse().ok();
            o.threads = c[2].parse().ok();
        }
        o.provider_cpu_mark = first_num(a("data-cpuscore"));
        o.ram_gb = first_num(a("data-ramamount"));
        (o.storage_gb, o.nvme) = parse_storage(&disk);
        o.storage = disk;
        o.location = if loc == "UK" {
            "Coventry".into()
        } else {
            loc.clone()
        };
        o.country = if loc == "UK" { "GB".into() } else { loc };
        o.price_monthly = cents / 100.0;
        o.currency = "USD";
        o.url = format!("{BASE}{}", a("data-stockurl"));
        out.push(o);
    }
    out
}
