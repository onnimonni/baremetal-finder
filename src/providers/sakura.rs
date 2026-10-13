//! Sakura Internet dedicated "PHY" (JP, Ishikari): public plan API with a
//! stock flag. Prices include 10% consumption tax and are converted to net.

use anyhow::Result;

use crate::model::{Offer, Stock};
use crate::util::get_json;

const URL: &str = "https://secure.sakura.ad.jp/cloud/api/dedicated-phy/1.0/server_plans/";
const TAX: f64 = 1.10;

pub async fn fetch(client: &reqwest::Client) -> Result<Vec<Offer>> {
    let v = get_json(client, URL).await?;
    let mut out = Vec::new();
    for p in v["server_plans"].as_array().into_iter().flatten() {
        let spec = &p["standard_spec"];
        let model = spec["cpu_model_name"].as_str().unwrap_or_default();
        // GPU plans (H100/H200/B200) are out of scope.
        if spec["gpu_model_name"].is_string()
            || p["name"].as_str().is_some_and(|n| n.contains("GPU"))
        {
            continue;
        }
        let Some(monthly) = p["prices"]["subscription"]["monthly_amount_with_tax"].as_f64() else {
            continue;
        };
        let vendor = if model.contains("EPYC") || model.contains("Ryzen") {
            "AMD"
        } else {
            "Intel Xeon"
        };
        let storage: Vec<String> = spec["storages"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|s| {
                format!(
                    "{}x {} GB {} {}",
                    s["device_count"],
                    s["size"],
                    s["bus_type"].as_str().unwrap_or_default(),
                    s["media_type"].as_str().unwrap_or_default()
                )
            })
            .collect();
        let storage_gb: f64 = spec["storages"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|s| s["device_count"].as_f64().unwrap_or(1.0) * s["size"].as_f64().unwrap_or(0.0))
            .sum();
        let storage = storage.join(" + ");
        let id = p["plan_id"].as_str().unwrap_or_default();
        let mut o = Offer::new("sakura", id.to_string());
        o.name = id.to_string();
        o.cpu = format!("{vendor} {model}");
        o.cpu_count = spec["cpu_count"].as_u64().unwrap_or(1) as u32;
        o.cores = spec["cpu_core_count"].as_u64().map(|c| c as u32);
        o.ram_gb = spec["memory_size"].as_f64();
        o.storage_gb = Some(storage_gb);
        o.nvme = Some(storage.to_lowercase().contains("nvme"));
        o.storage = storage;
        o.port_mbps = spec["global_bandwidth"].as_u64().map(|b| b as u32);
        o.location = "Ishikari".into();
        o.country = "JP".into();
        o.price_monthly = monthly / TAX;
        o.setup_fee = p["prices"]["registration"]["amount_with_tax"]
            .as_f64()
            .unwrap_or(0.0)
            / TAX;
        o.currency = "JPY";
        o.stock = if p["stock_available"] == true {
            Stock::InStock
        } else {
            Stock::OutOfStock
        };
        o.url = "https://server.sakura.ad.jp/".into();
        o.note = p["min_usage_months"]
            .as_u64()
            .map(|m| format!("min {m} months"));
        out.push(o);
    }
    Ok(out)
}
