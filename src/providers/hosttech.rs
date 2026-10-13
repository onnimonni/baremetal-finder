//! hosttech (CH): "Restposten" leftover servers with per-unit stock (disks are
//! configured at order time) and the regular root servers (no stock signal).

use std::sync::LazyLock;

use anyhow::Result;
use regex::Regex;
use scraper::{ElementRef, Html, Selector};

use crate::model::{Offer, Stock};
use crate::util::{clean, first_num, get_text, parse_storage};

const URL: &str = "https://www.hosttech.ch/server/";
const SITE: &str = "https://www.hosttech.ch";

fn sel(s: &str) -> Selector {
    Selector::parse(s).expect("static CSS selector is valid")
}

fn text(e: ElementRef) -> String {
    clean(&e.text().collect::<Vec<_>>().join(" "))
}

/// "<cpu> [N ×] 3.2 GHz[, 6-Core]" -> (cpu, cores)
fn split_clock(s: &str) -> (String, Option<u32>) {
    static CLOCK: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"^(.*?)\s+(?:(\d+)\s*[×x]\s*)?\d+(?:[.,]\d+)?\s*GHz(?:,\s*(\d+)-Core)?")
            .expect("static regex is valid")
    });
    match CLOCK.captures(s) {
        Some(c) => (
            c[1].replace("CPU ", "").trim().to_string(),
            c.get(2).or(c.get(3)).and_then(|m| m.as_str().parse().ok()),
        ),
        None => (s.replace("CPU ", "").trim().to_string(), None),
    }
}

pub async fn fetch(client: &reqwest::Client) -> Result<Vec<Offer>> {
    let html = get_text(client, URL).await?;
    let doc = Html::parse_document(&html);
    let mut out = Vec::new();

    let item = sel(".serverlist-details-item");
    let title = sel(".serverlist-details-item-title");
    let content = sel(".serverlist-details-item-content");
    for s in doc.select(&sel("#ServerOverview .serverlist-item-container")) {
        let first = |css: &str| s.select(&sel(css)).next();
        let Some(price) = first("[data-size-price]")
            .and_then(|e| e.value().attr("data-size-price"))
            .and_then(first_num)
        else {
            continue;
        };
        let details: Vec<(String, String)> = s
            .select(&item)
            .filter_map(|i| {
                Some((
                    text(i.select(&title).next()?),
                    text(i.select(&content).next()?),
                ))
            })
            .collect();
        let detail = |k: &str| {
            details
                .iter()
                .find(|(t, _)| t.starts_with(k))
                .map(|(_, v)| v.clone())
                .unwrap_or_default()
        };
        let cpu_text = first("[data-size-cpu]")
            .and_then(|e| e.value().attr("data-size-cpu"))
            .unwrap_or_default();
        // "Intel Core i7-8700 3.2 GHz, 6-Core" / "… 3,4 GHz, 4-Core"
        let (cpu, rest_cores) = split_clock(cpu_text);
        let order = first("a.serverOrder");
        let qty = first(".serverlist-details-teaser-amount")
            .map(text)
            .and_then(|t| first_num(&t))
            .map(|n| n as i64);
        let mut o = Offer::new(
            "hosttech",
            order
                .and_then(|a| a.value().attr("server_id"))
                .unwrap_or_default()
                .to_string(),
        );
        o.name = first(".serverlist-details-title")
            .map(text)
            .unwrap_or_default();
        o.cpu = cpu;
        o.cores = rest_cores;
        o.ram_gb = first("[data-size-ram]")
            .and_then(|e| e.value().attr("data-size-ram"))
            .and_then(first_num);
        o.storage = "configurable (disks extra)".into();
        o.port_mbps = first_num(&detail("Netzwerkkarte").replace("Gigabit", "")).map(|n| n as u32);
        o.traffic = Some("unlimited".into());
        o.location = "Switzerland".into();
        o.country = "CH".into();
        o.price_monthly = price;
        o.currency = "CHF";
        o.stock_qty = qty;
        o.stock = match qty {
            Some(n) if n > 0 => Stock::InStock,
            Some(_) => Stock::OutOfStock,
            None => Stock::Unknown,
        };
        o.url = order
            .and_then(|a| a.value().attr("href"))
            .map_or(URL.to_string(), |h| format!("{SITE}{h}"));
        o.note = Some("leftover server; disks configured at order".into());
        out.push(o);
    }

    // Regular root servers: "ASX ab CHF 149.00 / Monat … AMD CPU Ryzen 7 5800X
    // 8 × 3.8 GHz 64 GB DDR4 RAM 2 × 1000 GB M.2 NVMe unlimitierter Traffic"
    let field = sel(".jet-listing-dynamic-field__content");
    let li = sel("li");
    for g in doc.select(&sel(".jet-listing-grid__item")) {
        let fields: Vec<String> = g.select(&field).map(text).collect();
        let lines: Vec<String> = g.select(&li).map(text).collect();
        let all = text(g);
        let Some(price) = all.split_once("CHF").and_then(|(_, p)| first_num(p)) else {
            continue;
        };
        let Some(name) = fields.first().cloned() else {
            continue;
        };
        let line = |k: &str| {
            lines
                .iter()
                .find(|l| l.contains(k))
                .cloned()
                .unwrap_or_default()
        };
        // "AMD Ryzen 9 7950X 16 × 4.5 GHz"
        let cpu_line = lines.first().cloned().unwrap_or_default();
        let (cpu, cores) = split_clock(&cpu_line);
        let disk = line("GB M.2").to_string();
        let disk = if disk.is_empty() { line("TB") } else { disk };
        let ram = line("RAM");
        let mut o = Offer::new("hosttech", format!("root:{name}"));
        o.name = name;
        o.cpu = cpu;
        o.cores = cores;
        o.ram_gb = first_num(&ram);
        (o.storage_gb, o.nvme) = parse_storage(&disk.replace('×', "x"));
        o.storage = disk;
        o.traffic = Some("unlimited".into());
        o.location = "Switzerland".into();
        o.country = "CH".into();
        o.price_monthly = price;
        o.currency = "CHF";
        o.stock = Stock::Unknown;
        o.spec_text = lines.join(" | ");
        o.url = g
            .select(&sel("a[href*='konfId']"))
            .next()
            .and_then(|a| a.value().attr("href"))
            .map_or(URL.to_string(), |h| {
                if h.starts_with('/') {
                    format!("{SITE}{h}")
                } else {
                    h.to_string()
                }
            });
        out.push(o);
    }
    anyhow::ensure!(!out.is_empty(), "hosttech: no servers");
    Ok(out)
}
