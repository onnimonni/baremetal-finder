//! Nocix / WholeSaleInternet (same inventory, Kansas City): category tables
//! behind Cloudflare TLS fingerprinting, so fetched with the Chrome-emulating
//! client. The listing's stock button can lag; each in-stock plan's cart page
//! is the authoritative check.

use std::collections::HashSet;
use std::sync::LazyLock;

use anyhow::Result;
use regex::Regex;
use scraper::{ElementRef, Html, Selector};

use crate::model::{Offer, Stock};
use crate::util::{browser, browser_text, clean, first_num, parse_storage};

const BASE: &str = "https://www.nocix.net";
const PAGES: &[&str] = &[
    "dedicated/",
    "high-performance-dedicated-servers/",
    "enterprise-dedicated-servers/",
    "game-dedicated-servers/",
    "legacy-and-budget-dedicated-servers/",
    "storage-dedicated-servers/",
];

fn sel(s: &str) -> Selector {
    Selector::parse(s).expect("static CSS selector is valid")
}

fn text(e: ElementRef) -> String {
    clean(&e.text().collect::<String>())
}

/// "Dual Xeon E5-2670v2 64GB + 240GB SSD Preconfig" -> "Dual Xeon E5-2670v2"
fn cpu_from_name(name: &str) -> String {
    static TAIL: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"(?i)\s+(\d+\s*(GB|TB)\b.*|preconfig\w*.*|custom.*)$")
            .expect("static regex is valid")
    });
    TAIL.replace(name, "").trim().to_string()
}

pub async fn fetch(_client: &reqwest::Client) -> Result<Vec<Offer>> {
    static CART: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"cart/\?id=(\d+)").expect("static regex is valid"));
    static CT: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"(?i)(\d+)\s*Cores?\s*/\s*(\d+)\s*Threads?").expect("static regex is valid")
    });
    let c = browser()?;
    let row = sel("table.footable tr");
    let td = sel("td");
    let strong = sel("strong");
    let button = sel("button");
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for page in PAGES {
        let html = match browser_text(&c, &format!("{BASE}/{page}")).await {
            Ok(h) => h,
            Err(e) => {
                eprintln!("nocix {page}: {e:#}");
                continue;
            }
        };
        let rows: Vec<Offer> = {
            let doc = Html::parse_document(&html);
            doc.select(&row)
                .filter_map(|r| {
                    let cells: Vec<ElementRef> = r.select(&td).collect();
                    if cells.len() < 6 {
                        return None;
                    }
                    let id = CART.captures(&r.html())?[1].to_string();
                    let info = cells[1];
                    let name = info.select(&strong).next().map(text)?;
                    let info_text = text(info);
                    let btn = r.select(&button).next()?;
                    let out_of_stock = btn.value().classes().any(|c| c == "outofstock-btn");
                    let price = text(btn).split('$').nth(1).and_then(first_num);
                    let disk = text(cells[3]).replace('*', "");
                    let net = text(cells[4]);

                    let mut o = Offer::new("nocix", id.clone());
                    o.name = name.clone();
                    o.cpu = cpu_from_name(&name);
                    if let Some(m) = CT.captures(&info_text) {
                        o.cores = m[1].parse().ok();
                        o.threads = m[2].parse().ok();
                    }
                    if info_text.contains("2 processors") && !o.cpu.starts_with("Dual") {
                        o.cpu_count = 2;
                    }
                    o.ram_gb = first_num(&text(cells[2]));
                    (o.storage_gb, o.nvme) = parse_storage(&disk);
                    o.storage = disk;
                    o.port_mbps = first_num(&net).map(|n| if net.to_lowercase().contains("gbit") { n * 1000.0 } else { n } as u32);
                    o.traffic = net.to_lowercase().contains("unmetered").then(|| "unmetered".into());
                    o.location = "Kansas City".into();
                    o.country = "US".into();
                    o.price_monthly = price.unwrap_or(0.0);
                    o.currency = "USD";
                    o.stock = if out_of_stock { Stock::OutOfStock } else { Stock::InStock };
                    o.spec_text = text(r);
                    o.url = format!("{BASE}/cart/?id={id}");
                    o.note = Some("also sold as WholeSaleInternet".into());
                    Some(o)
                })
                .collect()
        };
        for mut o in rows {
            if !seen.insert(o.sku.clone()) {
                continue;
            }
            // Cart page: "Place an Order - <name> - $X/month" or "... Out Of Stock".
            if (o.stock == Stock::InStock || o.price_monthly == 0.0)
                && let Ok(cart) = browser_text(&c, &o.url).await
            {
                if cart.contains("Out Of Stock") {
                    o.stock = Stock::OutOfStock;
                }
                if o.price_monthly == 0.0
                    && let Some(p) = cart.split("MONTHLY - $").nth(1).and_then(first_num)
                {
                    o.price_monthly = p;
                }
            }
            if o.price_monthly > 0.0 {
                out.push(o);
            }
        }
    }
    Ok(out)
}
