//! IP-Projects (Frankfurt): catalog product cards (built to order, no stock
//! signal) + the "Server Depot" of physically available units. Prices on
//! the site include German VAT; `window.session.tax_percent` is stripped.

use std::sync::LazyLock;

use anyhow::Result;
use regex::Regex;
use scraper::{ElementRef, Html, Selector};

use crate::model::{Offer, Stock};
use crate::util::{clean, first_num, get_text, parse_storage};

const BASE: &str = "https://www.ip-projects.de";
const CATALOG: &[&str] = &[
    "performance/amd",
    "performance/intel",
    "enterprise/amd",
    "enterprise/intel",
    "enterprise/arm",
    "bare-metal",
];

fn sel(s: &str) -> Selector {
    Selector::parse(s).expect("static CSS selector is valid")
}

fn text(e: ElementRef) -> String {
    clean(&e.text().collect::<String>())
}

fn tax_factor(html: &str) -> f64 {
    static TAX: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"tax_percent\s*:\s*([\d.]+)").expect("static regex is valid"));
    TAX.captures(html)
        .and_then(|c| c[1].parse::<f64>().ok())
        .map_or(1.19, |p| 1.0 + p / 100.0)
}

pub async fn fetch(client: &reqwest::Client) -> Result<Vec<Offer>> {
    let mut out = Vec::new();
    let depot = get_text(client, &format!("{BASE}/en/dedicated-server/serverdepot")).await?;
    out.extend(parse_depot(&depot));
    for path in CATALOG {
        match get_text(client, &format!("{BASE}/en/dedicated-server/{path}")).await {
            Ok(html) => out.extend(parse_catalog(&html)),
            Err(e) => eprintln!("ipprojects {path}: {e:#}"),
        }
    }
    Ok(out)
}

fn parse_depot(html: &str) -> Vec<Offer> {
    static CT: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"Cores:\s*(\d+),\s*Threads:\s*(\d+)").expect("static regex is valid")
    });
    static SCORE: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"Score:\s*(\d+)").expect("static regex is valid"));
    let tax = tax_factor(html);
    let doc = Html::parse_document(html);
    let entry = sel("details.product-entry");
    let col = |c: &str| sel(&format!(".product-table-column.{c}"));
    let (cpu_s, ram_s, hdd_s, loc_s, ava_s, price_s) = (
        col("cpu"),
        col("ram"),
        col("hdd"),
        col("loc"),
        col("ava"),
        col("pricing"),
    );
    let info_s = sel(".cpuinfo");
    let avail_s = sel(".availableCircle");
    let order_s = sel("a.order-link");
    let mut out = Vec::new();
    for e in doc.select(&entry) {
        let get = |s: &Selector| e.select(s).next().map(text).unwrap_or_default();
        let Some(price) = first_num(&get(&price_s)) else {
            continue;
        };
        let info = get(&info_s);
        let hdd = get(&hdd_s);
        let now = e
            .select(&avail_s)
            .next()
            .is_some_and(|a| a.value().classes().any(|c| c == "availableNow"));

        let mut o = Offer::new("ipprojects", e.value().id().unwrap_or("?"));
        o.name = format!("Server Depot {}", e.value().id().unwrap_or_default());
        o.cpu = get(&cpu_s);
        if let Some(c) = CT.captures(&info) {
            o.cores = c[1].parse().ok();
            o.threads = c[2].parse().ok();
        }
        o.provider_cpu_mark = SCORE.captures(&info).and_then(|c| c[1].parse().ok());
        o.ram_gb = first_num(&get(&ram_s));
        (o.storage_gb, o.nvme) = parse_storage(&hdd);
        o.storage = hdd;
        o.location = get(&loc_s);
        o.country = "DE".into();
        o.price_monthly = price / tax;
        o.stock = if now {
            Stock::InStock
        } else {
            Stock::OutOfStock
        };
        o.stock_qty = Some(1);
        o.availability = Some(get(&ava_s));
        o.url = e
            .select(&order_s)
            .next()
            .and_then(|a| a.value().attr("href"))
            .map_or_else(
                || format!("{BASE}/en/dedicated-server/serverdepot"),
                |h| format!("{BASE}{h}"),
            );
        o.note = Some("used unit; VAT stripped".into());
        out.push(o);
    }
    out
}

fn parse_catalog(html: &str) -> Vec<Offer> {
    static MARKS: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"(\d+)\(MT\)").expect("static regex is valid"));
    let tax = tax_factor(html);
    let doc = Html::parse_document(html);
    let card = sel(".product-card");
    let title_s = sel(".title");
    let model_s = sel(".cpu .model");
    let cpu_divs = sel(".cpu > div");
    let marks_s = sel(".cpumarks");
    let metric_s = sel(".metric-property");
    let price_s = sel(".price");
    let order_s = sel("a.order-button");
    let mut out = Vec::new();
    for c in doc.select(&card) {
        let Some(title) = c.select(&title_s).next().map(text) else {
            continue;
        };
        let Some(price) = c
            .select(&price_s)
            .next()
            .map(text)
            .and_then(|t| first_num(&t))
        else {
            continue;
        };
        // divs after the "Cores / Threads" label: [cores, threads, ...]
        let divs: Vec<String> = c.select(&cpu_divs).map(text).collect();
        let ct_at = divs.iter().position(|d| d.starts_with("Cores"));
        let metrics: Vec<String> = c.select(&metric_s).map(text).collect();
        let disk = metrics
            .iter()
            .find(|m| m.contains("Storage"))
            .cloned()
            .unwrap_or_default();
        let href = c
            .select(&order_s)
            .next()
            .and_then(|a| a.value().attr("href"));

        let mut o = Offer::new("ipprojects", href.unwrap_or(&title).to_string());
        o.name = title;
        o.cpu = c.select(&model_s).next().map(text).unwrap_or_default();
        o.cores = ct_at
            .and_then(|i| divs.get(i + 1))
            .and_then(|d| first_num(d))
            .map(|n| n as u32);
        o.threads = ct_at
            .and_then(|i| divs.get(i + 2))
            .and_then(|d| first_num(d))
            .map(|n| n as u32);
        o.provider_cpu_mark = c
            .select(&marks_s)
            .next()
            .map(text)
            .and_then(|t| MARKS.captures(&t).and_then(|m| m[1].parse().ok()));
        o.ram_gb = metrics
            .iter()
            .find(|m| m.contains("RAM"))
            .and_then(|m| first_num(m));
        (o.storage_gb, o.nvme) = parse_storage(&disk);
        o.storage = disk;
        o.location = "Frankfurt".into();
        o.country = "DE".into();
        o.price_monthly = price / tax;
        // 1-month minimum term setup (net); 0 with 12-month term.
        o.setup_fee = 108.40;
        o.stock = Stock::Unknown;
        o.availability = Some("build-to-order".into());
        o.url = href.map_or_else(|| BASE.to_string(), |h| format!("{BASE}{h}"));
        o.note = Some("VAT stripped; setup €0 with 12-month term".into());
        out.push(o);
    }
    out
}
