//! WebNX (US: Ogden, New York, Los Angeles, Dallas): public WHMCS products
//! JSON with structured specs and an integer stock level.

use anyhow::Result;

use crate::model::{Offer, Stock};
use crate::util::{first_num, get_json, parse_storage};

const URL: &str = "https://webnx.com/api/whmcs/products";

pub async fn fetch(client: &reqwest::Client) -> Result<Vec<Offer>> {
    let v = get_json(client, URL).await?;
    let mut out = Vec::new();
    for p in v["products"].as_array().into_iter().flatten() {
        let specs = &p["customdata"]["specs"];
        if !specs.is_object() {
            continue; // colocation / GPU entries
        }
        let Some(price) = p["pricing"]["USD"]["monthly"]
            .as_str()
            .and_then(first_num)
            .or(p["pricing"]["USD"]["monthly"].as_f64())
        else {
            continue;
        };
        let s = |k: &str| specs[k].as_str().unwrap_or_default().to_string();
        let storage = s("storage");
        let qty = p["stocklevel"].as_i64();
        let mut o = Offer::new("webnx", p["pid"].to_string());
        o.name = p["name"].as_str().unwrap_or_default().to_string();
        o.cpu = s("processor");
        o.cores = first_num(&s("cores")).map(|c| c as u32);
        o.threads = first_num(&s("threads")).map(|t| t as u32);
        o.ram_gb = first_num(&s("memory"));
        (o.storage_gb, o.nvme) = parse_storage(&storage);
        o.storage = storage.clone();
        o.port_mbps = first_num(&s("network")).map(|g| if s("network").contains('G') { g * 1000.0 } else { g } as u32);
        o.traffic = p["customdata"]["legacyNetwork"].as_str().map(String::from);
        o.location = s("location");
        o.country = "US".into();
        o.price_monthly = price;
        o.setup_fee = p["pricing"]["USD"]["msetupfee"]
            .as_str()
            .and_then(first_num)
            .unwrap_or(0.0)
            .max(0.0);
        o.currency = "USD";
        o.stock_qty = qty;
        o.stock = match qty {
            Some(n) if n > 0 => Stock::InStock,
            Some(_) => Stock::OutOfStock,
            None => Stock::Unknown,
        };
        o.availability = Some(s("provisioning")).filter(|p| !p.is_empty());
        o.spec_text = format!("{} {storage}", s("memory"));
        o.url = format!("https://webnx.com/billing/cart.php?a=add&pid={}", p["pid"]);
        out.push(o);
    }
    Ok(out)
}
