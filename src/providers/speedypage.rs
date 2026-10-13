//! SpeedyPage (London, Singapore, Sydney): WHMCS "lux" store, one plan per
//! location with "N Available".

use anyhow::Result;

use crate::model::Offer;
use crate::util::{currency, get_text, parse_storage};
use crate::{specs, whmcs};

const BASE: &str = "https://my.speedypage.com";
const GROUPS: &[(&str, &str, &str)] = &[
    ("dedicated-servers-london", "GB", "London"),
    ("dedicated-servers-singapore", "SG", "Singapore"),
    ("dedicated-servers-sydney", "AU", "Sydney"),
];

pub async fn fetch(client: &reqwest::Client) -> Result<Vec<Offer>> {
    let mut out = Vec::new();
    for (group, country, city) in GROUPS {
        let html = match get_text(client, &format!("{BASE}/store/{group}")).await {
            Ok(h) => h,
            Err(e) => {
                eprintln!("speedypage {group}: {e:#}");
                continue;
            }
        };
        let cur = if html.contains("GBP") {
            "GBP"
        } else if html.contains("USD") {
            "USD"
        } else {
            "EUR"
        };
        for p in whmcs::parse_store(&html) {
            let Some(price) = p.price else { continue };
            let s = specs::from_lines(&p.lines);
            let mut o = Offer::new("speedypage", format!("{group}/{}", p.id));
            o.name = p.name.clone();
            o.cpu = s.cpu;
            o.cpu_count = s.cpu_count;
            o.cores = s.cores;
            o.threads = s.threads;
            o.ram_gb = s.ram_gb;
            (o.storage_gb, o.nvme) = parse_storage(&s.storage);
            o.storage = s.storage;
            o.port_mbps = s.port_mbps;
            o.location = (*city).into();
            o.country = (*country).into();
            o.price_monthly = price;
            o.setup_fee = p.setup;
            o.currency = currency(cur).unwrap_or("GBP");
            o.stock = p.stock();
            o.stock_qty = p.qty;
            o.spec_text = p.lines.join(" | ");
            o.url = format!("{BASE}/store/{group}");
            out.push(o);
        }
    }
    Ok(out)
}
