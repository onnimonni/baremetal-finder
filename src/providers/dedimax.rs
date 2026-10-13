//! Dedimax (France): broker JSON feed of ~1,000 servers worldwide. Only an
//! "available in minutes" flag; others ship in 24-72 h.

use anyhow::Result;

use crate::geo::iso_from_name;
use crate::model::{Offer, Stock};
use crate::util::{first_num, get_json, parse_storage};

const URL: &str = "https://www.dedimax.com/sdoffers";

pub async fn fetch(client: &reqwest::Client) -> Result<Vec<Offer>> {
    let v = get_json(client, URL).await?;
    let mut out = Vec::new();
    for s in v["data"].as_array().into_iter().flatten() {
        let Some(price) = s["price"].as_f64() else {
            continue;
        };
        let disk = s["disk"].as_str().unwrap_or_default();
        let bw = s["bandepassante"].as_str().unwrap_or_default();
        let country = s["location"].as_str().unwrap_or_default();
        let instant = s["available"] == true;
        let mut o = Offer::new("dedimax", s["id"].to_string());
        let cpu = s["cpu"].as_str().unwrap_or_default();
        o.name = format!("{cpu} / {}GB", s["ram"].as_str().unwrap_or("?"));
        o.cpu = cpu.split(" @").next().unwrap_or(cpu).to_string();
        o.ram_gb = s["ram"].as_str().and_then(first_num);
        (o.storage_gb, o.nvme) = parse_storage(disk);
        o.storage = disk.to_string();
        o.port_mbps = bw
            .split('@')
            .next_back()
            .and_then(first_num)
            .map(|n| if bw.contains("Gbps") { n * 1000.0 } else { n } as u32);
        o.traffic = bw
            .split('@')
            .next()
            .map(|t| t.trim().to_string())
            .filter(|t| t.contains("TB"));
        o.location = s["city"].as_str().unwrap_or(country).to_string();
        o.country = iso_from_name(country).unwrap_or_default().to_string();
        o.price_monthly = price;
        (o.stock, o.availability) = if instant {
            (Stock::InStock, Some("minutes".into()))
        } else {
            (Stock::Unknown, Some("24-72h".into()))
        };
        o.url = format!("https://panel.dedimax.com/order/sd/{}/customize", s["id"]);
        o.note = Some("broker".into());
        out.push(o);
    }
    Ok(out)
}
