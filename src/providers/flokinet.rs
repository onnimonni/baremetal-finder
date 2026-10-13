//! FlokiNET (Finland, Iceland, Netherlands, Romania): WHMCS store groups with
//! "N Available" counters.

use anyhow::Result;

use crate::model::Offer;
use crate::util::{get_text, parse_storage};
use crate::{specs, whmcs};

const BASE: &str = "https://billing.flokinet.is";
const GROUPS: &[(&str, &str, &str)] = &[
    ("dedicated-server-finland", "FI", "Finland"),
    ("dedicated-server-iceland", "IS", "Iceland"),
    ("dedicated-servers-netherlands", "NL", "Netherlands"),
    ("dedicated-servers-romania", "RO", "Romania"),
];

pub async fn fetch(client: &reqwest::Client) -> Result<Vec<Offer>> {
    let mut out = Vec::new();
    for (group, country, location) in GROUPS {
        let html = match get_text(client, &format!("{BASE}/index.php?rp=/store/{group}")).await {
            Ok(h) => h,
            Err(e) => {
                eprintln!("flokinet {group}: {e:#}");
                continue;
            }
        };
        for p in whmcs::parse_store(&html) {
            let Some(price) = p.price else { continue };
            let s = specs::from_lines(&p.lines);
            let mut o = Offer::new("flokinet", format!("{group}/{}", p.id));
            o.name = p.name.clone();
            o.cpu = s.cpu;
            o.cpu_count = s.cpu_count;
            o.cores = s.cores;
            o.threads = s.threads;
            o.ram_gb = s.ram_gb;
            (o.storage_gb, o.nvme) = parse_storage(&s.storage);
            o.storage = s.storage;
            o.port_mbps = s.port_mbps;
            o.location = (*location).into();
            o.country = (*country).into();
            o.price_monthly = price;
            o.setup_fee = p.setup;
            o.stock = p.stock();
            o.stock_qty = p.qty;
            o.spec_text = p.lines.join(" | ");
            o.url = p
                .order
                .as_deref()
                .map_or_else(|| BASE.to_string(), |h| format!("{BASE}{h}"));
            out.push(o);
        }
    }
    Ok(out)
}
