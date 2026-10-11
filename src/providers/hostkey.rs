//! Hostkey: the instant-server page calls a tokenless multipart API that
//! returns presets with `limit_order` (= units in stock).

use anyhow::Result;
use reqwest::multipart::Form;
use serde_json::Value;

use crate::model::{Offer, Stock};
use crate::util::{first_num, parse_storage};

const URL: &str = "https://hostkey.com/apiadmin/action";

pub async fn fetch(client: &reqwest::Client) -> Result<Vec<Offer>> {
    let fields = [
        ("entity", "cart"),
        ("action_name", "get_presets_list"),
        ("tag", "bm"),
        ("netag", "web_nocom,web_nosite"),
        ("currency", "eur"),
        ("pricerate", "1"),
        ("currencycon", "cb"),
        ("servertype", "1"),
        ("filter", "yes"),
        ("language", "eng"),
        ("invapi", "yes"),
        ("site", "com"),
    ];
    let form = fields.iter().fold(Form::new(), |f, (k, v)| f.text(*k, *v));
    let v: Value = client
        .post(URL)
        .multipart(form)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    let items = v["data"]["response"].as_array().ok_or_else(|| {
        anyhow::anyhow!(
            "hostkey: unexpected response {}",
            v.to_string().chars().take(200).collect::<String>()
        )
    })?;
    Ok(items.iter().filter_map(offer).collect())
}

fn offer(p: &Value) -> Option<Offer> {
    let c = &p["common"];
    let hw = &p["hardware"];
    let price = c["conditions"]["items"]
        .as_array()?
        .iter()
        .find(|i| i["currency"] == "eur")?["prices"]["current"]
        .as_f64()?;
    let cpus: Vec<&Value> = hw["cpu"]["items"]
        .as_array()
        .map(|a| a.iter().collect())
        .unwrap_or_default();
    let cpu = cpus
        .first()
        .and_then(|c| c["line"].as_str())
        .or(hw["cpu"]["description"].as_str())
        .unwrap_or_default();
    let count = (hw["cpu"]["number"]
        .as_u64()
        .unwrap_or(cpus.len().max(1) as u64)) as u32;
    let disks = hw["hard_drive"]["description"]
        .as_str()
        .unwrap_or_default()
        .to_string();
    let net = &hw["network"];
    let qty = c["conditions"]["limit_order"].as_i64().unwrap_or(0);
    let loc = c["location"].as_str().unwrap_or_default();

    let mut o = Offer::new("hostkey", c["id"].to_string());
    o.name = format!(
        "{} ({})",
        c["name"].as_str().unwrap_or_default(),
        c["description"].as_str().unwrap_or_default()
    );
    o.cpu = cpu.to_string();
    o.cpu_count = count;
    o.cores = hw["cpu"]["number_cores"]
        .as_u64()
        .or_else(|| {
            cpus.first()
                .and_then(|c| c["number_cores"].as_u64())
                .map(|n| n * count as u64)
        })
        .map(|n| n as u32);
    o.ram_gb = hw["ram"]["volume"].as_f64();
    (o.storage_gb, o.nvme) = parse_storage(&disks);
    o.storage = disks;
    o.port_mbps = net["portSpeed"]
        .as_str()
        .and_then(first_num)
        .map(|g| (g * 1000.0) as u32);
    o.traffic = net["traffic"]
        .as_str()
        .and_then(|t| t.split('/').nth(1))
        .map(|t| t.trim().to_string());
    o.location = loc.to_string();
    o.country = if loc == "UK" {
        "GB".into()
    } else {
        loc.to_string()
    };
    o.price_monthly = price;
    o.stock = if qty > 0 {
        Stock::InStock
    } else {
        Stock::OutOfStock
    };
    o.stock_qty = Some(qty);
    o.availability = p["extended"]["param1"]
        .as_str()
        .map(|s| s.trim().to_string());
    o.url = "https://hostkey.com/dedicated-servers/instant/".into();
    Some(o)
}
