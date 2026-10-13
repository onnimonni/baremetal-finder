//! Exoscale (CH/AT/DE/BG/HR): tokenless instance-type and pricing APIs.
//! Disk is billed separately; no stock signal.

use anyhow::Result;
use serde_json::Value;

use crate::model::{Offer, Stock};
use crate::util::{get_json, parse_num};

const TYPES: &str = "https://api-ch-gva-2.exoscale.com/v2/instance-type";
const PRICES: &str = "https://portal.exoscale.com/api/pricing/opencompute";

fn zone_country(zone: &str) -> String {
    zone.get(..2).unwrap_or_default().to_uppercase()
}

pub async fn fetch(client: &reqwest::Client) -> Result<Vec<Offer>> {
    let types = get_json(client, TYPES).await?;
    let prices = get_json(client, PRICES).await?;
    let mut out = Vec::new();
    for t in types["instance-types"].as_array().into_iter().flatten() {
        if t["authorized"] == false {
            continue; // needs account approval (mega+, GPU)
        }
        let family = t["family"].as_str().unwrap_or_default();
        let size = t["size"].as_str().unwrap_or_default();
        let key = if family == "standard" {
            format!("running_{size}")
        } else {
            format!("running_{family}_{size}")
        };
        let Some(hourly) = prices["eur"][&key].as_str().and_then(parse_num) else {
            continue;
        };
        for zone in t["zones"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
        {
            let vcpus = t["cpus"].as_u64().unwrap_or(0) as u32;
            let mut o = Offer::new("exoscale", format!("{family}.{size}@{zone}"));
            o.kind = "vps";
            o.name = format!("{family}.{size}");
            o.cpu = format!("Exoscale {family}");
            o.vcpus = Some(vcpus);
            o.threads = Some(vcpus);
            o.ram_gb = t["memory"].as_f64().map(|b| b / 1024f64.powi(3));
            o.storage = "volume billed separately".into();
            o.location = zone.to_string();
            o.country = zone_country(zone);
            o.price_monthly = hourly * 730.0;
            o.stock = Stock::Unknown;
            o.url = "https://www.exoscale.com/pricing/".into();
            out.push(o);
        }
    }
    Ok(out)
}
