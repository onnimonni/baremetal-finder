//! Hetzner Cloud (CX/CAX/CPX/CCX): plan matrix HTML (orderable locations and
//! a per-plan "not available" flag) joined with the website price API.

use anyhow::Result;
use scraper::{ElementRef, Html, Selector};

use crate::model::{Offer, Stock};
use crate::util::{clean, first_num, get_json, get_text, parse_num};

const PAGES: &[&str] = &[
    "https://www.hetzner.com/cloud/cost-optimized",
    "https://www.hetzner.com/cloud/regular-performance",
    "https://www.hetzner.com/cloud/general-purpose",
];
const PRICE_API: &str = "https://website-price-api.hetzner.com/api/v1/products/";

fn sel(s: &str) -> Selector {
    Selector::parse(s).expect("static CSS selector is valid")
}

fn text(e: ElementRef) -> String {
    clean(&e.text().collect::<Vec<_>>().join(" "))
}

/// CPU per plan family. Only CAX publishes a model (Ampere Altra).
fn family(name: &str) -> (&'static str, bool) {
    match name.get(..3).unwrap_or_default() {
        "CAX" => ("Ampere Altra Q80-30", true),
        "CCX" => ("AMD EPYC (Hetzner CCX)", false),
        "CPX" => ("AMD EPYC (Hetzner CPX)", true),
        _ => ("Intel/AMD (Hetzner CX)", true),
    }
}

/// One row of the cloud plan matrix.
struct Plan {
    name: String,
    vcpus: u32,
    ram: String,
    disk: String,
    /// Price API product key.
    key: String,
    dcs: Vec<String>,
    unavailable: bool,
}

pub async fn fetch(client: &reqwest::Client) -> Result<Vec<Offer>> {
    let mut out = Vec::new();
    for page in PAGES {
        let html = get_text(client, page).await?;
        let plans: Vec<Plan> = {
            let doc = Html::parse_document(&html);
            doc.select(&sel("div.cloud-matrix-product"))
                .filter_map(|p| {
                    let get = |css: &str| p.select(&sel(css)).next().map(text).unwrap_or_default();
                    let name = get(".name-cell");
                    let key = p
                        .select(&sel("[product-key]"))
                        .next()?
                        .value()
                        .attr("product-key")?
                        .to_string();
                    let vcpus = first_num(&get(".cpu-cell"))? as u32;
                    let dcs = p
                        .select(&sel(".location-flag .flag-text"))
                        .map(text)
                        .collect();
                    let unavailable = p.value().classes().any(|c| c == "not-available");
                    Some(Plan {
                        name,
                        vcpus,
                        ram: get(".ram-cell"),
                        disk: get(".drive-cell"),
                        key,
                        dcs,
                        unavailable,
                    })
                })
                .collect()
        };
        for Plan {
            name,
            vcpus,
            ram,
            disk,
            key,
            dcs,
            unavailable,
        } in plans
        {
            let prices =
                match get_json(client, &format!("{PRICE_API}{}", key.replace('+', "%2B"))).await {
                    Ok(v) => v,
                    Err(e) => {
                        eprintln!("hetznercloud {name}: {e:#}");
                        continue;
                    }
                };
            let (cpu, shared) = family(&name);
            for loc in prices["locations"].as_array().into_iter().flatten() {
                let dc = loc["datacenter"].as_str().unwrap_or_default();
                // The price API lists more locations than the plan is sold in.
                if !dcs.iter().any(|d| d == dc) {
                    continue;
                }
                let Some(monthly) = loc["prices"]["monthly"]["EUR"].as_str().and_then(parse_num)
                else {
                    continue;
                };
                let mut o = Offer::new("hetznercloud", format!("{name}@{dc}"));
                o.kind = "vps";
                o.name = name.clone();
                o.cpu = cpu.into();
                o.vcpus = Some(vcpus);
                o.threads = Some(vcpus);
                o.shared_cpu = Some(shared);
                o.ram_gb = first_num(&ram);
                o.storage = disk.clone();
                o.storage_gb = first_num(&disk);
                o.nvme = Some(disk.contains("NVMe"));
                o.location = dc.to_string();
                o.country = loc["countryCode"]
                    .as_str()
                    .unwrap_or_default()
                    .to_uppercase();
                o.price_monthly = monthly;
                o.stock = if unavailable {
                    Stock::OutOfStock
                } else {
                    Stock::InStock
                };
                o.availability = Some(
                    if unavailable {
                        "not available"
                    } else {
                        "available"
                    }
                    .into(),
                );
                o.url = (*page).into();
                o.note = Some("price incl. primary IPv4; stock flag is per plan".into());
                out.push(o);
            }
        }
    }
    Ok(out)
}
