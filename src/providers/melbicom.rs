//! Melbicom (LV; EU, US, Asia, Africa): paged JSON behind the site's
//! WordPress ajax handler, 10 servers per page. No stock signal.

use anyhow::Result;
use futures::future::join_all;
use serde_json::Value;

use crate::geo::iso_from_name;
use crate::model::{Offer, Stock};
use crate::util::{first_num, parse_storage};

const URL: &str = "https://www.melbicom.net/ajax.php";

async fn page(client: &reqwest::Client, n: u64) -> Result<Value> {
    let form = [
        ("action", "load_dedicated_servers".to_string()),
        ("type", "dedicated".to_string()),
        ("page", n.to_string()),
        ("currency", "EUR".to_string()),
    ];
    Ok(client
        .post(URL)
        .form(&form)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?)
}

pub async fn fetch(client: &reqwest::Client) -> Result<Vec<Offer>> {
    let first = page(client, 1).await?;
    let total = first["data"]["total"].as_u64().unwrap_or(0);
    let pages = total.div_ceil(10).max(1);
    let mut all = vec![first];
    // Small batches keep the load polite.
    let rest: Vec<u64> = (2..=pages).collect();
    for chunk in rest.chunks(8) {
        for r in join_all(chunk.iter().map(|n| page(client, *n))).await {
            match r {
                Ok(v) => all.push(v),
                Err(e) => eprintln!("melbicom: {e:#}"),
            }
        }
    }
    let mut out = Vec::new();
    for s in all
        .iter()
        .flat_map(|p| p["data"]["data"].as_array().cloned().unwrap_or_default())
    {
        let str_ = |k: &str| s[k].as_str().unwrap_or_default().to_string();
        let Some(price) = first_num(&str_("price")) else {
            continue;
        };
        if str_("gpu").chars().any(|c| c.is_ascii_alphanumeric()) {
            continue; // GPU servers
        }
        // "Xeon E3-1240 v6 3.70 GHz"
        let cpu = str_("cpu");
        let mut cpu = cpu.split(" GHz").next().unwrap_or_default().to_string();
        if let Some((c, _)) = cpu.rsplit_once(' ') {
            cpu = c.to_string();
        }
        // "2xXeon E5-2660 v3"
        let (count, cpu) = match cpu.split_once('x') {
            Some((n, c)) if n.trim().parse::<u32>().is_ok() => {
                (n.trim().parse().unwrap_or(1), c.trim().to_string())
            }
            _ => (1, cpu),
        };
        let vendor = if cpu.contains("EPYC") || cpu.contains("Ryzen") {
            "AMD"
        } else {
            "Intel"
        };
        let loc = str_("location");
        let (country, city) = loc.split_once(", ").unwrap_or((&loc, ""));
        let storage = str_("storage");
        let link = str_("order_link");
        let mut o = Offer::new(
            "melbicom",
            link.rsplit('/').next().unwrap_or_default().to_string(),
        );
        o.name = link.rsplit('/').next().unwrap_or_default().to_string();
        o.cpu = format!("{vendor} {cpu}");
        o.cpu_count = count;
        o.ram_gb = first_num(&str_("ram"));
        (o.storage_gb, o.nvme) = parse_storage(storage.split(" (").next().unwrap_or_default());
        o.storage = storage;
        o.port_mbps = first_num(&str_("network")).map(|g| {
            if str_("network").contains("Gbps") {
                (g * 1000.0) as u32
            } else {
                g as u32
            }
        });
        o.traffic = Some(str_("transfer"));
        o.location = city.to_string();
        o.country = iso_from_name(country).unwrap_or_default().to_string();
        o.price_monthly = price;
        o.stock = Stock::Unknown;
        o.url = link;
        out.push(o);
    }
    Ok(out)
}
