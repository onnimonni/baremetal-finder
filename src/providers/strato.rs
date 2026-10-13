//! STRATO (DE): "Limited Hardware" leftover servers (sold-out rows disappear,
//! so every listed row is in stock) and the regular Linux dedicated table.
//! Prices include 19% VAT and are converted to net.

use anyhow::Result;
use scraper::{Html, Selector};

use crate::model::{Offer, Stock};
use crate::util::{clean, first_num, get_text, parse_storage};

const LIMITED: &str = "https://www.strato.de/server/limited-hardware/";
const REGULAR: &str = "https://www.strato.de/server/linux-dedicated-server/";
const VAT: f64 = 1.19;

fn sel(s: &str) -> Selector {
    Selector::parse(s).expect("static CSS selector is valid")
}

pub async fn fetch(client: &reqwest::Client) -> Result<Vec<Offer>> {
    let mut out = parse(&get_text(client, LIMITED).await?, LIMITED, true);
    out.extend(parse(&get_text(client, REGULAR).await?, REGULAR, false));
    anyhow::ensure!(!out.is_empty(), "strato: no rows");
    Ok(out)
}

fn parse(html: &str, url: &str, limited: bool) -> Vec<Offer> {
    let doc = Html::parse_document(html);
    let cell = sel("td[data-label]");
    let mut out = Vec::new();
    for row in doc.select(&sel("tr[data-price]")) {
        let attr = |k: &str| row.value().attr(k).unwrap_or_default();
        let cells: Vec<(String, String)> = row
            .select(&cell)
            .map(|td| {
                (
                    td.value()
                        .attr("data-label")
                        .unwrap_or_default()
                        .to_string(),
                    clean(&td.text().collect::<Vec<_>>().join(" ")),
                )
            })
            .collect();
        let get = |k: &str| {
            cells
                .iter()
                .find(|(l, _)| l == k)
                .map(|(_, v)| v.clone())
                .unwrap_or_default()
        };
        let name = get("Product Name");
        let Some(cents) = first_num(attr("data-price")) else {
            continue;
        };
        // "Intel Xeon E3-1230v6 Kaby Lake" -> drop the codename
        let cpu = get("CPU");
        let cpu = cpu
            .split_whitespace()
            .take_while(|w| {
                !matches!(
                    *w,
                    "Kaby"
                        | "Coffee"
                        | "Skylake"
                        | "Haswell"
                        | "Broadwell"
                        | "Raptor"
                        | "Alder"
                        | "Rocket"
                        | "Comet"
                        | "Ice"
                        | "Cascade"
                        | "Genoa"
                        | "Milan"
                        | "Rome"
                )
            })
            .collect::<Vec<_>>()
            .join(" ");
        let kerne = get("Kerne");
        let disk = [get("HDD"), get("SSD"), get("NVME")]
            .into_iter()
            .filter(|d| !d.is_empty())
            .collect::<Vec<_>>()
            .join(" + ");
        let ram = get("RAM");
        let preis = get("Preis");
        let mut o = Offer::new(
            "strato",
            format!("{}{name}", if limited { "ltd:" } else { "" }),
        );
        o.name = name;
        o.cpu = cpu;
        o.cores = first_num(attr("data-cores")).map(|c| c as u32);
        // "4 x 3,5 GHz (4C/8T)"
        o.threads = kerne
            .split_once("C/")
            .and_then(|(_, t)| first_num(t))
            .map(|t| t as u32);
        o.ram_gb = first_num(attr("data-ram"));
        o.ecc = Some(ram.contains("ECC"));
        (o.storage_gb, o.nvme) = parse_storage(&disk);
        o.storage = disk;
        o.location = "Berlin".into();
        o.country = "DE".into();
        o.price_monthly = cents / 100.0 / VAT;
        o.setup_fee = preis
            .split_once("Einrichtung")
            .and_then(|(_, f)| first_num(f))
            .map_or(0.0, |f| f / VAT);
        o.stock = if limited {
            Stock::InStock
        } else {
            Stock::Unknown
        };
        o.spec_text = format!("{ram} | {kerne}");
        o.url = url.to_string();
        o.note = (!limited).then(|| "price is the cheapest term (\"ab\")".to_string());
        out.push(o);
    }
    out
}
