//! Zomro (NL, US, SG, IN): paged dedicated server table (`?offset=N`, 25
//! rows per page). "Available: N pcs." is the stock count; rows without one
//! are built to order.

use anyhow::Result;
use scraper::{ElementRef, Html, Selector};

use crate::geo::iso_from_name;
use crate::model::{Offer, Stock};
use crate::util::{clean, first_num, get_text, parse_storage};

const URL: &str = "https://zomro.com/dedicated-servers";

fn sel(s: &str) -> Selector {
    Selector::parse(s).expect("static CSS selector is valid")
}

fn text(e: ElementRef) -> String {
    clean(&e.text().collect::<Vec<_>>().join(" "))
}

pub async fn fetch(client: &reqwest::Client) -> Result<Vec<Offer>> {
    let mut out = Vec::new();
    // Pages of 25; stop when a page adds nothing new.
    for offset in (0..500).step_by(25) {
        let html = get_text(client, &format!("{URL}?offset={offset}")).await?;
        let before = out.len();
        // Later pages are bare `<tr>` fragments, which need a table around them.
        let html = if html.contains("<table") {
            html
        } else {
            format!("<table>{html}</table>")
        };
        parse(&Html::parse_document(&html), &mut out);
        let mut seen = std::collections::HashSet::new();
        out.retain(|o| seen.insert(o.sku.clone()));
        if out.len() == before {
            break;
        }
    }
    anyhow::ensure!(!out.is_empty(), "zomro: no servers");
    Ok(out)
}

fn parse(doc: &Html, out: &mut Vec<Offer>) {
    let spec = sel(".dedic-server-item__mobile-spec");
    let label = sel(".dedic-server-item__mobile-spec-label");
    let value = sel(".dedic-server-item__mobile-spec-value");
    for row in doc.select(&sel("tr.dedic-server-item[data-server-id]")) {
        let first = |css: &str| row.select(&sel(css)).next().map(text).unwrap_or_default();
        let Some(price) = first_num(&first(".dedic-server-item__mobile-price")) else {
            continue;
        };
        let specs: Vec<(String, String)> = row
            .select(&spec)
            .filter_map(|s| {
                Some((
                    text(s.select(&label).next()?),
                    text(s.select(&value).next()?),
                ))
            })
            .collect();
        let get = |k: &str| {
            specs
                .iter()
                .find(|(l, _)| l == k)
                .map(|(_, v)| v.clone())
                .unwrap_or_default()
        };
        let summary = first(".dedic-server-item__mobile-summary");
        let qty = summary
            .split("Available:")
            .nth(1)
            .and_then(first_num)
            .map(|n| n as i64);
        let setup_free = row
            .select(&sel(".dedic-server-item__setup--free"))
            .next()
            .is_some();
        let setup = if setup_free {
            0.0
        } else {
            first_num(&first(".dedic-server-item__setup")).unwrap_or(0.0)
        };
        let disk = get("Drive");
        let port = get("Port speed");
        let loc = get("Location");
        let cpu = get("CPU");
        let (count, cpu) = match cpu.split_once(" x ").or_else(|| cpu.split_once("x ")) {
            Some((n, c)) if n.trim().parse::<u32>().is_ok() => {
                (n.trim().parse().unwrap_or(1), c.to_string())
            }
            _ => (1, cpu),
        };
        let mut o = Offer::new(
            "zomro",
            row.value()
                .attr("data-server-id")
                .unwrap_or_default()
                .to_string(),
        );
        o.name = first(".dedic-server-item__mobile-name")
            .split(" €")
            .next()
            .unwrap_or_default()
            .to_string();
        o.cpu = cpu;
        o.cpu_count = count;
        o.cores = first_num(&get("Number of cores")).map(|c| c as u32);
        o.ram_gb = first_num(&get("RAM"));
        (o.storage_gb, o.nvme) = parse_storage(&disk);
        o.storage = disk;
        o.port_mbps =
            first_num(&port).map(|n| if port.contains("Gbps") { n * 1000.0 } else { n } as u32);
        o.traffic = Some(get("Traffic/month"));
        // "India, Mumbai", "USA"
        let country = loc.split(", ").next().unwrap_or_default();
        o.country = match country {
            "USA" => "US",
            c => iso_from_name(c).unwrap_or_default(),
        }
        .to_string();
        o.location = loc;
        o.price_monthly = price;
        o.setup_fee = setup;
        o.stock_qty = qty;
        o.stock = match qty {
            Some(n) if n > 0 => Stock::InStock,
            _ => Stock::Unknown,
        };
        o.availability = qty.is_none().then(|| "build-to-order".to_string());
        o.spec_text = get("RAM");
        o.url = URL.into();
        out.push(o);
    }
}
