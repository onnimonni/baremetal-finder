//! Tier.Net (US): WHMCS store groups. "Out of Stock" marker or an order
//! button per product.

use anyhow::Result;
use scraper::{ElementRef, Html, Selector};

use crate::model::{Offer, Stock};
use crate::util::{clean, first_num, get_text, parse_storage};

const BASE: &str = "https://billing.tier.net/store";
const GROUPS: &[&str] = &[
    "instant-deploy-servers",
    "dedicated-servers-2026",
    "amd-specials-2024",
    "stock-sale-2023",
];

fn sel(s: &str) -> Selector {
    Selector::parse(s).expect("static CSS selector is valid")
}

fn text(e: ElementRef) -> String {
    clean(&e.text().collect::<String>())
}

pub async fn fetch(client: &reqwest::Client) -> Result<Vec<Offer>> {
    let item = sel("li[id^='product']");
    let mut out = Vec::new();
    for g in GROUPS {
        let html = match get_text(client, &format!("{BASE}/{g}")).await {
            Ok(h) => h,
            Err(e) => {
                eprintln!("tiernet {g}: {e:#}");
                continue;
            }
        };
        let doc = Html::parse_document(&html);
        for p in doc.select(&item) {
            let id = p.value().id().unwrap_or_default();
            if id.contains('-') {
                continue; // nested "productN-description" items
            }
            let by_id = |suffix: &str| sel(&format!("#{id}-{suffix}"));
            let Some(name) = p.select(&by_id("name")).next().map(text) else {
                continue;
            };
            let Some(price) = p
                .select(&by_id("price"))
                .next()
                .map(text)
                .and_then(|t| first_num(&t))
            else {
                continue;
            };
            let unavailable = p.select(&by_id("unavailable")).next().is_some();
            let order = p
                .select(&by_id("order-button"))
                .next()
                .and_then(|a| a.value().attr("href"));
            // "AMD EPYC 9274F /512GB RAM / 2x 2TB NVMe / ASHBURN, VA"
            let parts: Vec<String> = name.split('/').map(clean).collect();
            let part = |i: usize| parts.get(i).cloned().unwrap_or_default();
            let storage = parts
                .iter()
                .find(|s| {
                    s.contains("TB") || s.contains("SSD") || s.contains("NVMe") || s.contains("HDD")
                })
                .cloned()
                .unwrap_or_default();
            let location = parts.last().cloned().unwrap_or_default();

            let mut o = Offer::new("tiernet", format!("{g}/{id}"));
            o.name = name.clone();
            o.cpu = part(0);
            o.ram_gb = parts
                .iter()
                .find(|s| s.contains("RAM") || s.ends_with("GB"))
                .and_then(|s| first_num(s));
            (o.storage_gb, o.nvme) = parse_storage(&storage);
            o.storage = storage;
            o.location = location;
            o.country = "US".into();
            o.price_monthly = price;
            o.currency = "USD";
            o.stock = if unavailable {
                Stock::OutOfStock
            } else if order.is_some() {
                Stock::InStock
            } else {
                Stock::Unknown
            };
            o.spec_text = p
                .select(&by_id("description"))
                .next()
                .map(text)
                .unwrap_or_default();
            o.url = order.map_or_else(
                || format!("{BASE}/{g}"),
                |h| format!("https://billing.tier.net{h}"),
            );
            if *g == "stock-sale-2023" {
                o.note = Some("clearance".into());
            }
            out.push(o);
        }
    }
    Ok(out)
}
