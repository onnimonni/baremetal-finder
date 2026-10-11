//! Leaseweb: public SAP Commerce shop API. `quantity` = instant servers in
//! stock for "1 hour" delivery products.

use anyhow::Result;
use serde_json::Value;

use crate::geo::iso_from_name;
use crate::model::{Offer, Stock};
use crate::util::{first_num, get_json, parse_storage};

const SEARCH: &str = "https://shop-api.leaseweb.com/occ/v2/leaseweb-spa/products/search?fields=FULL&pageSize=100&curr=EUR&lang=en";

pub async fn fetch(client: &reqwest::Client) -> Result<Vec<Offer>> {
    let mut out = Vec::new();
    let mut page = 0;
    loop {
        let v = get_json(client, &format!("{SEARCH}&currentPage={page}")).await?;
        for p in v["products"].as_array().into_iter().flatten() {
            if p["productType"] == "DEDSER02"
                && let Some(o) = offer(p)
            {
                out.push(o);
            }
        }
        page += 1;
        if page >= v["pagination"]["totalPages"].as_i64().unwrap_or(0) {
            break;
        }
    }
    Ok(out)
}

/// "Netherlands Amsterdam" / "United States Washington D.C." -> (ISO, city)
fn split_location(s: &str) -> (String, String) {
    let words: Vec<&str> = s.split_whitespace().collect();
    for n in (1..=words.len().min(3)).rev() {
        if let Some(iso) = iso_from_name(&words[..n].join(" ")) {
            return (iso.to_string(), words[n..].join(" "));
        }
    }
    (String::new(), s.to_string())
}

fn offer(p: &Value) -> Option<Offer> {
    let s = |k: &str| p[k].as_str().unwrap_or_default();
    let name = s("name");
    let (model, loc) = name.split_once(" | ").unwrap_or((name, ""));
    let (country, city) = split_location(loc);
    let count = if s("defaultCPUQuantity") == "Dual" {
        2
    } else {
        1
    };
    let disks = format!(
        "{}x {}",
        s("defaultHDDIncludedAmount"),
        s("defaultHDDIncludedDiskSize")
    );
    let delivery = s("deliveryMethod");
    let qty = p["quantity"].as_i64().unwrap_or(0);

    let mut o = Offer::new("leaseweb", s("code"));
    o.name = model.to_string();
    o.cpu = s("defaultCPUType").to_string();
    o.cpu_count = count;
    o.cores = p["defaultTotalCPUCores"].as_u64().map(|c| c as u32);
    o.ram_gb = first_num(s("defaultRAM"));
    (o.storage_gb, o.nvme) = parse_storage(&disks);
    o.storage = disks;
    o.traffic = p["defaultBundledDataPack"][0].as_str().map(String::from);
    o.location = city;
    o.country = country;
    o.price_monthly = p["price"]["value"].as_f64()?;
    o.currency = crate::util::currency(p["price"]["currencyIso"].as_str()?)?;
    o.stock = match delivery {
        "1 hour" if qty > 0 => Stock::InStock,
        "1 hour" => Stock::OutOfStock,
        _ => Stock::Unknown,
    };
    o.stock_qty = (delivery == "1 hour").then_some(qty);
    o.availability = Some(
        if delivery.is_empty() {
            "unknown"
        } else {
            delivery
        }
        .to_string(),
    );
    o.url = format!(
        "https://www.leaseweb.com/en/dedicated-servers/configure/{}",
        s("code")
    );
    o.note = Some("setup fee not fetched".into());
    Some(o)
}
