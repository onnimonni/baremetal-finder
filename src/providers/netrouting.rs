//! Netrouting (NL/SE/RO/US): shop catalog API with instant-delivery
//! quantities per location.

use anyhow::Result;

use crate::model::{Offer, Stock};
use crate::util::{first_num, get_json, parse_storage};

const URL: &str = "https://shop.netrouting.com/api/v2/portal/catalog";

pub async fn fetch(client: &reqwest::Client) -> Result<Vec<Offer>> {
    let v = get_json(client, URL).await?;
    let mut out = Vec::new();
    for p in v["data"]["products"].as_array().into_iter().flatten() {
        if p["category"] != "bare-metal" {
            continue;
        }
        let Some(price) = p["prices"]["m"]["price"].as_f64() else {
            continue;
        };
        let specs = &p["specs"];
        let desc = p["description"].as_str().unwrap_or_default();
        let disk = specs["disk"].as_str().unwrap_or_default();
        let net = specs["network"].as_str().unwrap_or_default();
        // "AMD Ryzen 9 7950X3D, 16c/32t, 64GB DDR5, …"
        let ct = desc
            .split(',')
            .find(|s| s.contains("c/"))
            .unwrap_or_default();
        for loc in p["locations"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|l| l.as_str())
        {
            let qty = p["instant_by_location"][loc].as_i64();
            let mut o = Offer::new(
                "netrouting",
                format!("{}@{loc}", p["slug"].as_str().unwrap_or("?")),
            );
            o.name = p["name"].as_str().unwrap_or_default().to_string();
            o.cpu = specs["cpu"]
                .as_str()
                .unwrap_or_default()
                .split(" @")
                .next()
                .unwrap_or_default()
                .to_string();
            o.cores = ct.split('c').next().and_then(first_num).map(|c| c as u32);
            o.threads = ct.split('/').nth(1).and_then(first_num).map(|t| t as u32);
            o.ram_gb = specs["ram"].as_str().and_then(first_num);
            (o.storage_gb, o.nvme) = parse_storage(disk);
            o.storage = disk.to_string();
            o.port_mbps = net
                .split('/')
                .next()
                .and_then(first_num)
                .map(|g| (g * 1000.0) as u32);
            o.traffic = net.split('/').nth(1).map(|t| t.trim().to_string());
            o.location = loc.split(',').next().unwrap_or(loc).trim().to_string();
            // "Miami, FL" / "New York, NY" are US states; "Hong Kong, CN" is HK.
            let code = loc.rsplit(", ").next().unwrap_or_default();
            o.country = match code {
                _ if o.location == "Hong Kong" => "HK".into(),
                "FL" | "NY" | "CA" | "TX" | "VA" | "IL" | "WA" | "GA" | "NJ" | "AZ" => "US".into(),
                c => c.to_string(),
            };
            o.price_monthly = price;
            o.setup_fee = p["prices"]["m"]["setup"].as_f64().unwrap_or(0.0);
            o.currency =
                crate::util::currency(p["currency"].as_str().unwrap_or("EUR")).unwrap_or("EUR");
            (o.stock, o.stock_qty) = match qty {
                Some(n) if n > 0 => (Stock::InStock, Some(n)),
                Some(n) => (Stock::OutOfStock, Some(n)),
                None if p["available"] == false => (Stock::OutOfStock, None),
                None => (Stock::Unknown, None),
            };
            o.availability = Some(
                if qty.is_some_and(|n| n > 0) {
                    "instant"
                } else {
                    "build-to-order"
                }
                .into(),
            );
            o.spec_text = format!("{desc} {}", specs["ram"].as_str().unwrap_or_default());
            o.url = format!(
                "https://shop.netrouting.com/product/{}",
                p["slug"].as_str().unwrap_or_default()
            );
            out.push(o);
        }
    }
    Ok(out)
}
