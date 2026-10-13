//! Datawagon (Buffalo NY, Chicago): products in the Next.js flight payload
//! with WHMCS stock levels. Only stock-tracked items are listed.

use anyhow::{Context, Result};

use crate::model::{Offer, Stock};
use crate::util::{first_num, get_text, json_after, next_flight, parse_storage};

const URL: &str = "https://datawagon.com/dedicated";

pub async fn fetch(client: &reqwest::Client) -> Result<Vec<Offer>> {
    let html = get_text(client, URL).await?;
    let products = json_after(&next_flight(&html), "products").context("datawagon: no products")?;
    let mut out = Vec::new();
    for p in products.as_array().into_iter().flatten() {
        let s = |k: &str| p[k].as_str().unwrap_or_default().trim().to_string();
        let Some(price) = first_num(&s("price")) else {
            continue;
        };
        let storage = s("storage");
        let qty = p["stocklevel"].as_i64();
        let mut o = Offer::new("datawagon", p["id"].to_string());
        o.name = s("name");
        o.cpu = s("cpu");
        // "12 @ 4.4Ghz" = threads @ clock
        o.threads = first_num(&s("cores")).map(|t| t as u32);
        o.ram_gb = first_num(&s("memory"));
        (o.storage_gb, o.nvme) = parse_storage(&storage);
        o.storage = storage;
        o.port_mbps = first_num(&s("bandwidth")).map(|g| (g * 1000.0) as u32);
        o.location = s("location");
        o.country = "US".into();
        o.price_monthly = price;
        o.currency = "USD";
        o.stock_qty = qty;
        o.stock = match (p["stockcontrol"].as_bool(), qty) {
            (Some(true), Some(n)) if n > 0 => Stock::InStock,
            (Some(true), _) => Stock::OutOfStock,
            _ => Stock::Unknown,
        };
        o.provider_cpu_mark = first_num(&s("cpuScore"));
        o.spec_text = s("memory");
        o.url = URL.into();
        out.push(o);
    }
    Ok(out)
}
