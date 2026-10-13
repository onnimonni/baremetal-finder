//! Clouvider: HostBill order-system category pages. No stock signal exists,
//! so offers are `unknown`; a product that is not listed is not orderable.

use std::collections::HashSet;

use anyhow::Result;
use scraper::{ElementRef, Html, Selector};

use crate::model::{Offer, Stock};
use crate::util::{clean, first_num, get_text, parse_storage};

const BASE: &str = "https://console.clouvider.co.uk/cart";
const CATEGORIES: &[&str] = &[
    "ams1-dedicated-servers-",
    "ashburn-dedicated-servers-",
    "atl-dedicated-servers-",
    "chicago-dedicated-servers-",
    "dallas-dedicated-servers-",
    "fr6-dedicated-servers-",
    "la-dedicated-servers-",
    "manchester-dedicated-servers-",
    "nyc-dedicated-servers-",
    "phoenix-dedicated-servers-",
    "thn2-dedicated-servers",
    "dedicated-server-sale",
    "dedicated-10gbps-servers-",
    "storage-dedicated-servers",
    "unmetered-dedicated-servers",
];

fn sel(s: &str) -> Selector {
    Selector::parse(s).expect("static CSS selector is valid")
}

fn text(e: ElementRef) -> String {
    clean(&e.text().collect::<String>())
}

pub async fn fetch(client: &reqwest::Client) -> Result<Vec<Offer>> {
    let card = sel("div.card.cart-product[data-value]");
    let name_s = sel("h4");
    let price_s = sel("span.product-price.cycle-m");
    let row_s = sel(".row.mx-3");
    let label_s = sel(".text-muted");
    let value_s = sel(".font-weight-bold");
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for cat in CATEGORIES {
        let html = match get_text(client, &format!("{BASE}/{cat}/")).await {
            Ok(h) => h,
            Err(e) => {
                eprintln!("clouvider {cat}: {e:#}");
                continue;
            }
        };
        let doc = Html::parse_document(&html);
        for c in doc.select(&card) {
            let pid = c.value().attr("data-value").unwrap_or_default();
            if !seen.insert(pid.to_string()) {
                continue;
            }
            let Some(price) = c
                .select(&price_s)
                .next()
                .map(text)
                .and_then(|t| first_num(&t))
            else {
                continue;
            };
            let specs: Vec<(String, String)> = c
                .select(&row_s)
                .filter_map(|r| {
                    Some((
                        r.select(&label_s).next().map(text)?,
                        r.select(&value_s).next().map(text)?,
                    ))
                })
                .collect();
            let spec = |l: &str| {
                specs
                    .iter()
                    .find(|(k, _)| k == l)
                    .map(|(_, v)| v.clone())
                    .unwrap_or_default()
            };
            let cpu_specs = spec("CPU Specs");
            let storage = spec("Storage");
            let location = spec("Location");
            let cc = location
                .rsplit(", ")
                .next()
                .unwrap_or_default()
                .to_uppercase();

            let mut o = Offer::new("clouvider", pid);
            o.name = c.select(&name_s).next().map(text).unwrap_or_default();
            let cpu = spec("CPU");
            // "2x Xeon …" prefixes are handled when storing.
            o.cpu = cpu;
            o.cores = first_num(&cpu_specs).map(|c| c as u32);
            o.threads = cpu_specs
                .split(',')
                .nth(1)
                .and_then(first_num)
                .map(|t| t as u32);
            o.ram_gb = first_num(&spec("RAM"));
            o.ecc = Some(spec("RAM").contains("ECC"));
            // "960GB NVMe upto 3 NVMe": only the first part is included.
            let base = storage.split(" up").next().unwrap_or(&storage).to_string();
            (o.storage_gb, o.nvme) = parse_storage(&base);
            o.storage = storage;
            let port = spec("Port Speed");
            o.port_mbps =
                first_num(&port).map(|g| if port.contains("Gbit") { g * 1000.0 } else { g } as u32);
            o.traffic = Some(spec("Premium Bandwidth")).filter(|t| !t.is_empty());
            o.location = location.clone();
            o.country = if cc == "UK" { "GB".into() } else { cc };
            o.price_monthly = price;
            o.stock = Stock::Unknown;
            o.spec_text = specs
                .iter()
                .map(|(k, v)| format!("{k}: {v}"))
                .collect::<Vec<_>>()
                .join(" | ");
            o.url = format!("https://console.clouvider.co.uk/?cmd=cart&action=add&id={pid}");
            o.note = Some("no stock signal; VAT treatment unclear".into());
            out.push(o);
        }
    }
    Ok(out)
}
