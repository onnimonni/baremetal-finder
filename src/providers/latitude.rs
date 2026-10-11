//! Latitude.sh: plans with per-country stock live in the pricing page's
//! React Server Components payload (requested with the `RSC: 1` header).

use std::collections::HashMap;

use anyhow::{Context, Result};
use serde_json::Value;

use crate::geo::iso_from_name;
use crate::model::{Offer, Stock};
use crate::util::parse_storage;

const URL: &str = "https://www.latitude.sh/pricing";

pub async fn fetch(client: &reqwest::Client) -> Result<Vec<Offer>> {
    let body = client
        .get(URL)
        .header("RSC", "1")
        .send()
        .await?
        .error_for_status()?
        .text()
        .await?;
    // RSC lines look like `6:{json}`; the data line contains "stockLevel".
    let line = body
        .lines()
        .find(|l| l.contains("\"stockLevel\""))
        .context("latitude: no plan data in RSC payload")?;
    let json = line.split_once(':').map(|(_, j)| j).unwrap_or(line);
    let v: Value = serde_json::from_str(json).context("latitude: RSC line is not JSON")?;
    let mut nodes = Vec::new();
    collect(&v, &mut nodes);

    let site_names: HashMap<String, String> = nodes
        .iter()
        .filter(|n| n["type"] == "regions")
        .filter_map(|n| {
            let a = &n["attributes"];
            Some((
                a["slug"].as_str()?.to_string(),
                a["name"].as_str()?.to_string(),
            ))
        })
        .collect();

    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for p in nodes.iter().filter(|n| n["type"] == "plans") {
        let a = &p["attributes"];
        let slug = a["slug"].as_str().unwrap_or_default();
        if !seen.insert(slug.to_string())
            || !a["specs"]["gpu"].as_object().is_none_or(|g| g.is_empty())
        {
            continue;
        }
        let specs = &a["specs"];
        let cpu = &specs["cpu"];
        let count = cpu["count"].as_u64().unwrap_or(1) as u32;
        let drives = specs["drives"]
            .as_array()
            .map(|d| {
                d.iter()
                    .map(|x| {
                        format!(
                            "{}x {} {}",
                            x["count"],
                            x["size"].as_str().unwrap_or("?"),
                            x["type"].as_str().unwrap_or("")
                        )
                    })
                    .collect::<Vec<_>>()
                    .join(" + ")
            })
            .unwrap_or_default();
        let nic = specs["nics"][0]["type"]
            .as_str()
            .unwrap_or_default()
            .to_string();

        for r in a["regions"].as_array().into_iter().flatten() {
            let Some(month) = r["pricing"]["USD"]["month"].as_f64() else {
                continue;
            };
            let country = r["name"].as_str().unwrap_or_default();
            let in_stock: Vec<String> = r["locations"]["inStock"]
                .as_array()
                .map(|s| {
                    s.iter()
                        .filter_map(|x| x.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default();
            let level = r["stockLevel"].as_str().unwrap_or("unknown");

            let mut o = Offer::new("latitude", format!("{slug}@{country}"));
            o.name = a["name"].as_str().unwrap_or(slug).to_string();
            o.cpu = cpu["type"].as_str().unwrap_or_default().to_string();
            o.cpu_count = count;
            o.cores = cpu["cores"].as_u64().map(|c| c as u32 * count);
            o.ram_gb = specs["memory"]["total"].as_f64();
            o.ecc = Some(true);
            (o.storage_gb, o.nvme) = parse_storage(&drives);
            o.storage = drives.clone();
            // "2 x 10Gbps" -> 10 Gbps per port
            o.port_mbps = nic
                .rsplit(' ')
                .next()
                .and_then(crate::util::first_num)
                .map(|g| (g * 1000.0) as u32);
            o.traffic = Some("20 TB egress".into());
            o.location = if in_stock.is_empty() {
                country.to_string()
            } else {
                in_stock
                    .iter()
                    .map(|s| {
                        site_names
                            .get(s)
                            .map_or(s.clone(), |n| format!("{n} ({s})"))
                    })
                    .collect::<Vec<_>>()
                    .join(", ")
            };
            o.country = iso_from_name(country).unwrap_or_default().to_string();
            o.price_monthly = month;
            o.currency = "USD";
            o.stock = match level {
                "unavailable" => Stock::OutOfStock,
                "high" | "medium" | "low" if !in_stock.is_empty() => Stock::InStock,
                _ => Stock::Unknown,
            };
            o.availability = Some(level.to_string());
            o.url = URL.into();
            o.note = Some("price incl. taxes per Latitude FAQ".into());
            out.push(o);
        }
    }
    Ok(out)
}

/// Every JSON object that has a "type" + "attributes", anywhere in the tree.
fn collect<'a>(v: &'a Value, out: &mut Vec<&'a Value>) {
    match v {
        Value::Object(m) => {
            if m.contains_key("type") && m.contains_key("attributes") {
                out.push(v);
            }
            m.values().for_each(|x| collect(x, out));
        }
        Value::Array(a) => a.iter().for_each(|x| collect(x, out)),
        _ => {}
    }
}
