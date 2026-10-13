//! UpCloud (Helsinki, Frankfurt, Amsterdam, Stockholm, …): plans, zones and
//! per-zone prices embedded in the price calculator's Next.js data. No
//! availability signal. Prices are cents per hour, capped at 672 h/month.

use anyhow::{Context, Result};

use crate::model::{Offer, Stock};
use crate::util::{get_text, json_after, next_flight};

const URL: &str = "https://calc.upcloud.com/cloud-servers?currency=EUR";
const HOURS_CAP: f64 = 672.0;

fn zone_country(zone: &str) -> String {
    match zone.get(..2).unwrap_or_default() {
        "uk" => "GB".into(),
        cc => cc.to_uppercase(),
    }
}

pub async fn fetch(client: &reqwest::Client) -> Result<Vec<Offer>> {
    let html = get_text(client, URL).await?;
    let flight = next_flight(&html);
    let plans = json_after(&flight, "plans").context("upcloud: no plans")?;
    let prices = json_after(&flight, "eurPrices").context("upcloud: no prices")?;
    let zones = json_after(&flight, "zones").unwrap_or_default();
    let zone_name = |id: &str| {
        zones
            .as_array()
            .and_then(|z| z.iter().find(|z| z["id"] == id))
            .and_then(|z| z["description"].as_str())
            .unwrap_or(id)
            .to_string()
    };
    // One price object per zone (sometimes wrapped in an extra array).
    let zone_prices: Vec<serde_json::Value> = match prices.as_array() {
        Some(a) if a.first().is_some_and(|x| x.is_array()) => {
            a[0].as_array().cloned().unwrap_or_default()
        }
        Some(a) => a.clone(),
        None => Vec::new(),
    };

    let mut out = Vec::new();
    for p in plans.as_array().into_iter().flatten() {
        let name = p["name"].as_str().unwrap_or_default();
        if p["category"] == "gpu" {
            continue;
        }
        for z in &zone_prices {
            let zone = z["name"].as_str().unwrap_or_default();
            let Some(cents) = z[format!("server_plan_{name}")]["price"].as_f64() else {
                continue;
            };
            let vcpus = p["coreNumber"].as_u64().unwrap_or(0) as u32;
            let mut o = Offer::new("upcloud", format!("{name}@{zone}"));
            o.kind = "vps";
            o.name = name.to_string();
            o.cpu = format!(
                "AMD EPYC (UpCloud {})",
                p["category"].as_str().unwrap_or("plan")
            );
            o.vcpus = Some(vcpus);
            o.threads = Some(vcpus);
            o.ram_gb = p["memoryAmount"].as_f64().map(|mb| mb / 1024.0);
            o.storage_gb = p["storageSize"].as_f64();
            o.storage = format!(
                "{} GB {}",
                p["storageSize"],
                p["storageTier"].as_str().unwrap_or_default()
            );
            o.nvme = Some(true);
            o.traffic = p["publicTrafficOut"]
                .as_f64()
                .map(|gb| format!("{} TB", gb / 1000.0));
            o.location = zone_name(zone);
            o.country = zone_country(zone);
            o.price_monthly = cents * HOURS_CAP / 100.0;
            o.stock = Stock::Unknown;
            o.url = "https://upcloud.com/pricing/".into();
            o.note = Some("no availability signal".into());
            out.push(o);
        }
    }
    Ok(out)
}
