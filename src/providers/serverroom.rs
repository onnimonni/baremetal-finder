//! ServerRoom (Amsterdam, Bucharest, US): the "instant" page loads one JSON
//! list of every ready server; each row is one physical machine.

use anyhow::Result;

use crate::model::{Offer, Stock};
use crate::util::{get_json, parse_storage};

const URL: &str = "https://serverroom.net/api/readyservers/config?detail=true";

fn country(location: &str) -> &'static str {
    match location.split(',').next().unwrap_or_default().trim() {
        "Amsterdam" => "NL",
        "Bucharest" => "RO",
        "Frankfurt" => "DE",
        "London" => "GB",
        _ if location.ends_with("US") => "US",
        _ => "",
    }
}

pub async fn fetch(client: &reqwest::Client) -> Result<Vec<Offer>> {
    let v = get_json(client, URL).await?;
    let mut out = Vec::new();
    for s in v.as_array().into_iter().flatten() {
        let Some(price) = s["price"].as_f64() else {
            continue;
        };
        let skel = &s["skel"];
        let location = s["location"].as_str().unwrap_or_default();
        let hdd = s["hdd"].as_str().unwrap_or_default();
        let mut o = Offer::new("serverroom", s["name"].as_str().unwrap_or("?"));
        o.name = format!(
            "{} {}",
            s["type"].as_str().unwrap_or_default(),
            s["name"].as_str().unwrap_or_default()
        );
        // "Intel Xeon E3-1284L v4 Quad Core 2.90 GHz"
        o.cpu = skel["CPU"]["SubItem"]
            .as_str()
            .or(s["cpu"].as_str())
            .unwrap_or_default()
            .to_string();
        o.cpu_count = skel["CPU"]["Value"].as_u64().unwrap_or(1) as u32;
        o.ram_gb = s["ram"].as_f64();
        (o.storage_gb, o.nvme) = parse_storage(hdd);
        o.storage = hdd.to_string();
        o.port_mbps = skel["PortSpeed"].as_u64().map(|p| p as u32);
        o.traffic = s["traffic"].as_str().map(String::from);
        o.location = location
            .split(',')
            .next()
            .unwrap_or(location)
            .trim()
            .to_string();
        o.country = country(location).into();
        o.price_monthly = price;
        o.currency = "USD";
        o.stock = Stock::InStock;
        o.stock_qty = Some(1);
        o.availability = Some("ready".into());
        o.spec_text = format!("{hdd} {}", s["cpu"].as_str().unwrap_or_default());
        o.url = "https://serverroom.net/instant".into();
        o.note = s["discount"]["price"]
            .as_f64()
            .map(|d| format!("promo ${d:.2} ({}% off)", s["discount"]["percent"]));
        out.push(o);
    }
    Ok(out)
}
