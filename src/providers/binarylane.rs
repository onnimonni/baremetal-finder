//! BinaryLane (AU, SG): public sizes API. `ded` sizes are whole dedicated
//! servers (CPU named), `cpu` sizes dedicated hyper-threads (CPU unnamed).
//! Ordering is possible in `regions` minus `regions_out_of_stock`. Prices are
//! AUD incl. 10% GST, converted to net.

use anyhow::Result;
use serde_json::Value;

use crate::model::{Offer, Stock};
use crate::util::{first_num, get_json};

const API: &str = "https://api.binarylane.com.au/v2";
const GST: f64 = 1.10;

fn place(region: &str) -> (&'static str, &'static str) {
    match region {
        "syd" => ("AU", "Sydney"),
        "mel" => ("AU", "Melbourne"),
        "bne" => ("AU", "Brisbane"),
        "adl" => ("AU", "Adelaide"),
        "per" => ("AU", "Perth"),
        "sin" => ("SG", "Singapore"),
        _ => ("AU", "Australia"),
    }
}

pub async fn fetch(client: &reqwest::Client) -> Result<Vec<Offer>> {
    let v = get_json(client, &format!("{API}/sizes?per_page=100")).await?;
    let mut out = Vec::new();
    for s in v["sizes"].as_array().into_iter().flatten() {
        let kind = s["size_type"]["slug"].as_str().unwrap_or_default();
        if !matches!(kind, "ded" | "cpu" | "vps") {
            continue;
        }
        let Some(price) = s["price_monthly"].as_f64() else {
            continue;
        };
        let slug = s["slug"].as_str().unwrap_or_default();
        let out_of_stock: Vec<&str> = s["regions_out_of_stock"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .collect();
        // "Intel Xeon E-2136\n6core/12thr, 3.3Ghz - 4.5GHz"
        let desc = s["cpu_description"].as_str().unwrap_or_default();
        let model = desc.lines().next().unwrap_or_default();
        let cores = desc.lines().nth(1).and_then(first_num).map(|c| c as u32);
        let vcpus = s["vcpus"].as_u64().map(|c| c as u32);
        for region in s["regions"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
        {
            let (country, city) = place(region);
            let mut o = Offer::new("binarylane", format!("{slug}@{region}"));
            o.name = slug.to_string();
            if kind == "ded" {
                o.cpu = model
                    .replace("AMD Ryzen 7900X", "AMD Ryzen 9 7900X")
                    .replace("AMD Ryzen 3900X", "AMD Ryzen 9 3900X");
                o.cores = cores;
                o.threads = vcpus;
            } else {
                o.kind = "vps";
                o.cpu = format!("BinaryLane {kind}");
                o.vcpus = vcpus;
                o.threads = vcpus;
                o.shared_cpu = Some(kind == "vps");
            }
            o.ram_gb = s["memory"].as_f64().map(|m| m / 1024.0);
            o.storage_gb = s["disk"].as_f64();
            o.storage = format!("{} GB SSD", s["disk"]);
            o.location = city.into();
            o.country = country.into();
            o.price_monthly = price / GST;
            o.currency = "AUD";
            o.stock = if out_of_stock.contains(&region) {
                Stock::OutOfStock
            } else {
                Stock::InStock
            };
            o.url = "https://www.binarylane.com.au/".into();
            out.push(o);
        }
    }
    Ok(out)
}
