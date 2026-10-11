//! Hetzner: server auction (all listed = in stock) + standard AX/EX models.

use std::collections::HashMap;

use anyhow::Result;
use serde_json::Value;

use crate::model::{Offer, Stock};
use crate::util::{get_json, parse_num};

const AUCTION_URL: &str = "https://www.hetzner.com/_resources/app/data/app/live_data_sb.json";
const PRICE_API: &str = "https://website-price-api.hetzner.com/api/v1/products/";
/// Product id -> datacenters where the base config ships immediately.
const UPFRONT_URL: &str = "https://www.hetzner.com/_resources/app/data/app/live_data_upfront.json";
/// Hetzner product key for the mandatory primary IPv4 address.
const IPV4_KEY: &str = "ROBOT_1266";

fn dc_country(dc: &str) -> &'static str {
    match dc.get(..3).unwrap_or_default() {
        "HEL" => "FI",
        "ASH" | "HIL" => "US",
        "SIN" => "SG",
        _ => "DE", // FSN, NBG
    }
}

pub async fn auction(client: &reqwest::Client) -> Result<Vec<Offer>> {
    let v = get_json(client, AUCTION_URL).await?;
    let servers = v["server"].as_array().cloned().unwrap_or_default();
    Ok(servers.iter().filter_map(auction_offer).collect())
}

fn auction_offer(s: &Value) -> Option<Offer> {
    let id = s["Id"].as_i64()?;
    let hw = &s["Hardware"];
    let dc = s["Details"]["Datacenter"]["Name"].as_str().unwrap_or("?");
    let disks: Vec<&str> = hw["Storage"]["Disks"]
        .as_array()
        .map(|a| a.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    let det = &hw["Storage"]["Details"];
    let sum = |k: &str| -> f64 {
        det[k]
            .as_array()
            .map(|a| a.iter().filter_map(Value::as_f64).sum())
            .unwrap_or(0.0)
    };
    let price = s["Prices"]["monthly"]["EUR"].as_f64()?;
    let ipv4 = s["IPPrices"]["monthly"]["EUR"].as_f64().unwrap_or(0.0);

    let mut o = Offer::new("hetzner_auction", id.to_string());
    o.name = format!("Auction #{id}");
    o.cpu = hw["CPU"]["Name"].as_str()?.to_string();
    // CoreCount in this feed is always 1, so cores come from PassMark.
    o.ram_gb = hw["RAM"]["Size"].as_f64();
    o.ecc = hw["RAM"]["ecc"].as_bool();
    o.storage = disks.join(", ");
    o.storage_gb = Some(sum("nvme") + sum("sata") + sum("hdd"));
    o.nvme = Some(sum("nvme") > 0.0);
    o.port_mbps = s["Details"]["Bandwidth"].as_u64().map(|b| b as u32);
    o.traffic = s["Details"]["Traffic"].as_str().map(String::from);
    o.location = dc.to_string();
    o.country = dc_country(dc).into();
    o.price_monthly = price + ipv4;
    o.setup_fee = s["Prices"]["setup"]["EUR"].as_f64().unwrap_or(0.0);
    o.stock = Stock::InStock;
    o.stock_qty = Some(1);
    o.url = format!("https://www.hetzner.com/sb/#search={id}");
    o.note = Some("used hardware; price incl. €1.70 IPv4".into());
    Some(o)
}

/// Base configurations of the current standard lineup. Specs are static
/// (from hetzner.com/dedicated-rootserver/matrix-*), prices and stock are live.
struct Model {
    name: &'static str,
    product: &'static str,
    cpu: &'static str,
    ram_gb: f64,
    ecc: bool,
    storage: &'static str,
    storage_gb: f64,
}

const MODELS: &[Model] = &[
    Model {
        name: "AX41",
        product: "1718",
        cpu: "AMD Ryzen 5 3600",
        ram_gb: 64.0,
        ecc: false,
        storage: "2x 512 GB NVMe",
        storage_gb: 1024.0,
    },
    Model {
        name: "AX42",
        product: "1719",
        cpu: "AMD Ryzen 7 PRO 8700GE",
        ram_gb: 64.0,
        ecc: true,
        storage: "2x 512 GB NVMe",
        storage_gb: 1024.0,
    },
    Model {
        name: "AX102",
        product: "1721",
        cpu: "AMD Ryzen 9 7950X3D",
        ram_gb: 128.0,
        ecc: true,
        storage: "2x 1.92 TB NVMe DC",
        storage_gb: 3840.0,
    },
    Model {
        name: "AX162",
        product: "1724",
        cpu: "AMD EPYC 9454P",
        ram_gb: 128.0,
        ecc: true,
        storage: "2x 1.92 TB NVMe DC",
        storage_gb: 3840.0,
    },
    Model {
        name: "EX44",
        product: "1728",
        cpu: "Intel Core i5-13500",
        ram_gb: 64.0,
        ecc: false,
        storage: "2x 512 GB NVMe",
        storage_gb: 1024.0,
    },
    Model {
        name: "EX63",
        product: "1729",
        cpu: "Intel Core Ultra 7 265",
        ram_gb: 64.0,
        ecc: true,
        storage: "2x 1 TB NVMe DC",
        storage_gb: 2000.0,
    },
    Model {
        name: "EX131",
        product: "1733",
        cpu: "Intel Xeon Gold 6731P",
        ram_gb: 128.0,
        ecc: true,
        storage: "2x 1.92 TB NVMe DC",
        storage_gb: 3840.0,
    },
];

pub async fn standard(client: &reqwest::Client) -> Result<Vec<Offer>> {
    let upfront = get_json(client, UPFRONT_URL).await?;
    // keys look like "1721" or "1565-1722"; take every id in the key.
    let mut ready: HashMap<String, Vec<String>> = HashMap::new();
    if let Some(obj) = upfront.as_object() {
        for (k, dcs) in obj {
            let dcs: Vec<String> = dcs
                .as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|d| d.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default();
            for id in k.split('-') {
                ready.entry(id.to_string()).or_default().extend(dcs.clone());
            }
        }
    }

    let mut out = Vec::new();
    for m in MODELS {
        let url = format!("{PRICE_API}ROBOT_{}%2B{IPV4_KEY}", m.product);
        let v = match get_json(client, &url).await {
            Ok(v) => v,
            Err(e) => {
                eprintln!("hetzner {}: {e:#}", m.name);
                continue;
            }
        };
        for loc in v["locations"].as_array().into_iter().flatten() {
            if loc["active"].as_bool() != Some(true) {
                continue;
            }
            let dc = loc["datacenter"].as_str().unwrap_or("?");
            let num = |p: &str| {
                loc["prices"][p]["EUR"]
                    .as_str()
                    .and_then(parse_num)
                    .unwrap_or(0.0)
            };
            let in_stock = ready
                .get(m.product)
                .is_some_and(|d| d.iter().any(|x| x == dc));
            let mut o = Offer::new("hetzner", format!("{}-{dc}", m.name));
            o.name = m.name.into();
            o.cpu = m.cpu.into();
            o.ram_gb = Some(m.ram_gb);
            o.ecc = Some(m.ecc);
            o.storage = m.storage.into();
            o.storage_gb = Some(m.storage_gb);
            o.nvme = Some(true);
            o.port_mbps = Some(1000);
            o.traffic = Some("unlimited".into());
            o.location = dc.into();
            o.country = dc_country(dc).into();
            o.price_monthly = num("monthly");
            o.setup_fee = num("setup");
            // Not in the upfront list = built to order (days), not sold out.
            o.stock = if in_stock {
                Stock::InStock
            } else {
                Stock::Unknown
            };
            o.availability = Some(
                if in_stock {
                    "upfront"
                } else {
                    "build-to-order"
                }
                .into(),
            );
            o.url = format!(
                "https://www.hetzner.com/dedicated-rootserver/{}/",
                m.name.to_lowercase()
            );
            o.note = Some("price incl. €1.70 IPv4".into());
            out.push(o);
        }
    }
    Ok(out)
}
