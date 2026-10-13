//! Contabo: Astro site backed by a JSON catalog. Stock is per product
//! (`soldOut`); location addons carry surcharges and occasionally an
//! "out-of-stock" marker in their slug.

use anyhow::Result;
use regex::Regex;
use serde_json::Value;

use crate::model::{Offer, Stock};
use crate::util::{first_num, get_json, get_text, parse_storage};

const BASE: &str = "https://contabo.com/en";

fn country(region: &str) -> &'static str {
    match region {
        "EU" => "DE",
        "UK" => "GB",
        "JPN" => "JP",
        "IND" => "IN",
        "SIN" => "SG",
        "AUS" => "AU",
        r if r.starts_with("US") => "US",
        _ => "",
    }
}

pub async fn fetch(client: &reqwest::Client) -> Result<Vec<Offer>> {
    let html = get_text(client, &format!("{BASE}/dedicated-servers/")).await?;
    let re = Regex::new(r#"data-product-id="([A-Za-z0-9]+)""#).expect("static regex is valid");
    let mut ids: Vec<String> = re.captures_iter(&html).map(|c| c[1].to_string()).collect();
    ids.sort();
    ids.dedup();
    anyhow::ensure!(!ids.is_empty(), "contabo: no product ids on listing page");
    let stock = get_json(
        client,
        &format!(
            "{BASE}/catalog/priceStock?preview=false&productIds={}",
            ids.join(",")
        ),
    )
    .await?;

    let mut out = Vec::new();
    for id in &ids {
        let p = get_json(
            client,
            &format!("{BASE}/catalog/product?preview=false&productId={id}"),
        )
        .await?;
        let addons = get_json(
            client,
            &format!("{BASE}/catalog/productAddons?preview=false&productId={id}"),
        )
        .await?;
        let st = stock
            .as_array()
            .and_then(|a| a.iter().find(|s| s["id"] == id.as_str()));
        let sold_out = st.is_some_and(|s| s["soldOut"] == true) || p["outOfStock"] == true;
        let price = st
            .and_then(|s| s["price"]["EUR"].as_f64())
            .or(p["price"]["EUR"].as_f64())
            .unwrap_or(0.0);
        let spec = |t: &str| {
            p["specsCollection"]["items"]
                .as_array()
                .and_then(|a| a.iter().find(|i| i["type"] == t))
                .cloned()
                .unwrap_or(Value::Null)
        };
        let cpu = spec("cpu");
        let storage = spec("storage")["title"]
            .as_str()
            .unwrap_or_default()
            .to_string();
        let setup = p["periods"]
            .as_array()
            .and_then(|a| a.iter().find(|x| x["length"] == 1))
            .and_then(|x| x["setup"]["EUR"].as_f64())
            .unwrap_or(0.0);

        let locations: Vec<&Value> = addons
            .as_array()
            .map(|a| a.iter().filter(|x| x["category"] == "location").collect())
            .unwrap_or_default();
        for loc in locations {
            let region = loc["value"].as_str().unwrap_or_default();
            let slug = loc["slug"].as_str().unwrap_or_default();
            let mut o = Offer::new(
                "contabo",
                format!("{}@{slug}", p["slug"].as_str().unwrap_or(id)),
            );
            o.name = p["title"].as_str().unwrap_or_default().to_string();
            o.cpu = cpu["subtitle"].as_str().unwrap_or_default().to_string();
            o.cores = cpu["title"].as_str().and_then(first_num).map(|c| c as u32);
            o.ram_gb = spec("ram")["value"]
                .as_f64()
                .or_else(|| spec("ram")["title"].as_str().and_then(first_num));
            (o.storage_gb, o.nvme) = parse_storage(&storage);
            o.storage = storage.clone();
            o.port_mbps = spec("port")["title"]
                .as_str()
                .and_then(first_num)
                .map(|g| (g * 1000.0) as u32);
            o.traffic = spec("traffic")["title"].as_str().map(String::from);
            o.location = loc["title"].as_str().unwrap_or(region).to_string();
            o.country = country(region).into();
            o.price_monthly = price + loc["price"]["EUR"].as_f64().unwrap_or(0.0);
            o.setup_fee = setup;
            o.stock = if sold_out || slug.contains("out-of-stock") {
                Stock::OutOfStock
            } else {
                Stock::InStock
            };
            o.availability = Some(
                if slug.contains("out-of-stock") {
                    "location out of stock"
                } else if sold_out {
                    "sold out"
                } else {
                    "available"
                }
                .into(),
            );
            o.spec_text = format!(
                "{} {} {}",
                spec("ram")["title"].as_str().unwrap_or_default(),
                spec("ram")["subtitle"].as_str().unwrap_or_default(),
                o.storage
            );
            o.url = format!(
                "{BASE}/dedicated-servers/{}",
                p["slug"].as_str().unwrap_or_default()
            );
            o.note = Some("stock flag is per product; monthly term price".into());
            out.push(o);
        }
    }
    Ok(out)
}
