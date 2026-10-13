//! FDC Servers (global): instant inventory from the configurator's public
//! proxy. Every record is one physical machine; identical machines in the
//! same city are grouped and counted.

use std::collections::BTreeMap;

use anyhow::Result;
use serde_json::Value;

use crate::model::{Offer, Stock};
use crate::util::{clean, get_json, parse_storage};

const URL: &str = "https://portal.fdcservers.net/modules/addons/fdc_bm_configurator/api/v2/servers?availability_types=instant&limit=1000&offset=0";

fn country(city: &str) -> &'static str {
    match city {
        "Amsterdam" => "NL",
        "Frankfurt" => "DE",
        "Dublin" => "IE",
        "Lisbon" => "PT",
        "London" => "GB",
        "Madrid" => "ES",
        "Paris" => "FR",
        "Prague" => "CZ",
        "Vienna" => "AT",
        "Warsaw" => "PL",
        "Hong Kong" => "HK",
        "Khabarovsk" => "RU",
        "Singapore" => "SG",
        "Tokyo" => "JP",
        "Toronto" => "CA",
        _ => "US",
    }
}

pub async fn fetch(client: &reqwest::Client) -> Result<Vec<Offer>> {
    let v = get_json(client, URL).await?;
    // cpu|count|ram|disks|city|price -> (machines, first record)
    let mut groups: BTreeMap<String, (i64, Value)> = BTreeMap::new();
    for r in v["results"].as_array().into_iter().flatten() {
        let key = format!(
            "{}|{}|{}|{}|{}|{}",
            r["cpu"]["model_title"],
            r["cpu"]["count"],
            r["ram"],
            r["storage_details"],
            r["location_details"],
            r["total_price"],
        );
        groups.entry(key).or_insert((0, r.clone())).0 += 1;
    }
    let mut out = Vec::new();
    for (qty, r) in groups.into_values() {
        let Some(price) = r["total_price"].as_f64() else {
            continue;
        };
        // "Intel(R) Xeon(R) CPU E5-2650 0 @ 2.00GHz", "AMD EPYC 7302 16-Core Processor"
        let title = clean(r["cpu"]["model_title"].as_str().unwrap_or_default());
        let cpu = title
            .split(" @ ")
            .next()
            .unwrap_or_default()
            .replace(" CPU", "")
            .split(" Processor")
            .next()
            .unwrap_or_default()
            .trim()
            .to_string();
        let cpu = cpu
            .rsplit_once(' ')
            .filter(|(_, last)| last.ends_with("-Core"))
            .map_or(cpu.clone(), |(c, _)| c.to_string());
        let disk = r["storage_details"]
            .as_array()
            .into_iter()
            .flatten()
            .flat_map(|s| s["item_details"].as_array().cloned().unwrap_or_default())
            .filter_map(|d| d.as_str().map(String::from))
            .collect::<Vec<_>>()
            .join(" + ");
        let city = r["location_details"][0].as_str().unwrap_or_default();
        let bw = r["bw_options"]
            .as_array()
            .into_iter()
            .flatten()
            .min_by(|a, b| {
                a["price"]
                    .as_f64()
                    .unwrap_or(0.0)
                    .total_cmp(&b["price"].as_f64().unwrap_or(0.0))
            });
        let mut o = Offer::new("fdc", r["hash_id"].as_str().unwrap_or_default().to_string());
        o.name = format!("{cpu} {}GB", r["ram"][0]["amount"]);
        o.cpu = cpu;
        o.cpu_count = r["cpu"]["count"].as_u64().unwrap_or(1) as u32;
        o.ram_gb = r["ram"][0]["amount"].as_f64();
        (o.storage_gb, o.nvme) = parse_storage(&disk);
        o.storage = disk;
        if let Some(bw) = bw {
            let v = bw["value"].as_f64().unwrap_or(0.0);
            o.port_mbps = Some(if bw["units"] == "Gbps" { v * 1000.0 } else { v } as u32);
            o.traffic = bw["type"].as_str().map(|t| t.to_lowercase());
        }
        o.location = city.to_string();
        o.country = country(city).into();
        o.price_monthly = price;
        o.currency = "USD";
        o.stock = Stock::InStock;
        o.stock_qty = Some(qty);
        o.url = "https://www.fdcservers.net/configurator".into();
        out.push(o);
    }
    Ok(out)
}
