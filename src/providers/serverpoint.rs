//! ServerPoint (Dallas, Las Vegas): public bare-metal sizes API with an
//! out-of-stock flag.

use anyhow::Result;
use serde_json::Value;

use crate::model::{Offer, Stock};
use crate::util::{first_num, get_json};

const API: &str = "https://api-v1.serverpoint.com/public/bm/sizes";

fn num(v: &Value) -> Option<f64> {
    v.as_str().and_then(first_num).or(v.as_f64())
}

fn place(region: &str) -> (&'static str, &'static str) {
    if region.starts_with("las") {
        ("US", "Las Vegas")
    } else {
        ("US", "Dallas")
    }
}

pub async fn fetch(client: &reqwest::Client) -> Result<Vec<Offer>> {
    let v = get_json(client, API).await?;
    let mut out = Vec::new();
    for s in v["bm_sizes"].as_array().into_iter().flatten() {
        let price = match (num(&s["price"]), num(&s["sale_price"]).filter(|p| *p > 0.0)) {
            (Some(p), Some(sale)) => p.min(sale),
            (Some(p), None) => p,
            _ => continue,
        };
        let sockets = num(&s["cpu_sockets"]).unwrap_or(1.0) as u32;
        let model = s["cpu_model"].as_str().unwrap_or_default();
        let vendor = if model.contains("EPYC") || model.contains("Ryzen") {
            "AMD"
        } else {
            "Intel"
        };
        let count = num(&s["disk_count"]).unwrap_or(1.0);
        let size = num(&s["disk_size"]).unwrap_or(0.0);
        let dtype = s["disk_type"].as_str().unwrap_or_default();
        let id = s["size"].as_str().unwrap_or_default();
        for region in s["availableregions"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
        {
            let (country, city) = place(region);
            let mut o = Offer::new("serverpoint", format!("{id}@{region}"));
            o.name = format!("{model} {}GB", s["ram"].as_str().unwrap_or_default());
            o.cpu = format!("{vendor} {model}");
            o.cpu_count = sockets;
            o.cores = num(&s["cpu_count"]).map(|c| c as u32);
            o.threads = num(&s["cpu_threads"]).map(|c| c as u32);
            o.ram_gb = num(&s["ram"]);
            o.storage_gb = Some(count * size);
            o.storage = format!("{count}x {size} GB {dtype}");
            o.nvme = Some(dtype.eq_ignore_ascii_case("nvme"));
            o.port_mbps = num(&s["nic_port"]).map(|g| (g * 1000.0) as u32);
            o.traffic = Some(format!(
                "{} {}",
                s["data_transfer_per_month"].as_str().unwrap_or_default(),
                s["data_transfer_unit"].as_str().unwrap_or_default()
            ));
            o.location = city.into();
            o.country = country.into();
            o.price_monthly = price;
            o.currency = "USD";
            o.stock = if s["out_of_stock"] == "1" {
                Stock::OutOfStock
            } else {
                Stock::InStock
            };
            o.availability = (s["ready_in_under_an_hour"] == "1").then(|| "< 1h".to_string());
            o.spec_text = s["ram_type"].as_str().unwrap_or_default().to_string();
            o.url = "https://www.serverpoint.com/en/dedicated-servers/".into();
            out.push(o);
        }
    }
    Ok(out)
}
