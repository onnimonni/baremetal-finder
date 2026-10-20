//! GTHost (US, CA, EU): instant-delivery machines from the public "for sale"
//! API, one record per physical server. Identical machines at the same
//! price and location are grouped and counted.

use std::collections::{BTreeMap, HashSet};

use anyhow::Result;
use serde_json::Value;

use crate::model::{Offer, Stock};
use crate::util::{first_num, get_json, parse_storage};

const API: &str = "https://cp.gthost.com/api/forsale2?page_size=500&locale=en&currency=usd";

fn country(region: &str) -> String {
    // "Zurich (CH)", "London (UK)"
    let cc = region
        .rsplit_once('(')
        .map_or("", |(_, c)| c.trim_end_matches(')'));
    if cc == "UK" {
        "GB".into()
    } else {
        cc.to_string()
    }
}

pub async fn fetch(client: &reqwest::Client) -> Result<Vec<Offer>> {
    let mut seen = HashSet::new();
    let mut machines: Vec<Value> = Vec::new();
    for page in 0..20 {
        let v = get_json(client, &format!("{API}&page={page}")).await?;
        let total = v["total"].as_u64().unwrap_or(0) as usize;
        let servers = v["servers"].as_array().cloned().unwrap_or_default();
        let before = machines.len();
        machines.extend(
            servers
                .into_iter()
                .filter(|s| s["id"].as_i64().is_some_and(|id| seen.insert(id))),
        );
        if machines.len() >= total || machines.len() == before {
            break;
        }
    }
    let mut groups: BTreeMap<String, (i64, Value)> = BTreeMap::new();
    for m in machines {
        if m["gpu"].as_array().is_some_and(|g| !g.is_empty()) {
            continue;
        }
        let key = format!(
            "{}|{}|{}|{}|{}|{}",
            m["cpu"][0]["full_name"],
            m["ram"],
            m["hdd"],
            m["port_speed"]["name"],
            m["region"]["name"],
            m["price"]
        );
        groups.entry(key).or_insert((0, m.clone())).0 += 1;
    }
    let mut out = Vec::new();
    for (qty, m) in groups.into_values() {
        let Some(price) = m["price"].as_f64() else {
            continue;
        };
        let cpu = &m["cpu"][0];
        // "Intel Dual Xeon Gold 6152 44c/88t 2.1-3.7GHz" -> "Intel Dual Xeon Gold 6152"
        let full = cpu["full_name"].as_str().unwrap_or_default();
        let model: Vec<&str> = full
            .split_whitespace()
            .take_while(|w| !w.contains("c/"))
            .collect();
        let threads = full
            .split_whitespace()
            .find_map(|w| w.split_once("c/"))
            .and_then(|(_, t)| first_num(t))
            .map(|t| t as u32);
        let disk = m["hdd"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|d| d["name"].as_str())
            .collect::<Vec<_>>()
            .join(" + ");
        let ram = m["ram"][0]["name"].as_str().unwrap_or_default();
        let port = m["port_speed"]["name"].as_str().unwrap_or_default();
        let region = m["region"]["name"].as_str().unwrap_or_default();
        let mut o = Offer::new("gthost", m["id"].to_string());
        o.name = m["name"].as_str().unwrap_or_default().to_string();
        o.cpu = model.join(" ").replace(" Single ", " ");
        o.cpu_count = cpu["cpu_count"].as_u64().unwrap_or(1) as u32;
        o.cores = cpu["core_count"].as_u64().map(|c| c as u32);
        o.threads = threads;
        o.ram_gb = first_num(ram);
        // "2x960GB SSD 2x7.68TB NVMe"
        (o.storage_gb, o.nvme) = parse_storage(&disk);
        o.storage = disk;
        o.port_mbps =
            first_num(port).map(|n| if port.contains("Gbit") { n * 1000.0 } else { n } as u32);
        o.traffic = Some("unmetered".into());
        o.location = region.split(" (").next().unwrap_or_default().to_string();
        o.country = country(region);
        o.price_monthly = price;
        o.currency = "USD";
        o.stock = Stock::InStock;
        o.stock_qty = Some(qty);
        o.availability = Some("instant".into());
        o.spec_text = m["ram"][0]["full_name"].as_str().unwrap_or(ram).to_string();
        o.url = m["order_link"]
            .as_str()
            .unwrap_or("https://gthost.com/instant-servers")
            .to_string();
        out.push(o);
    }
    Ok(out)
}
