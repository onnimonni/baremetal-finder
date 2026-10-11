//! PhoenixNAP Bare Metal Cloud: tokenless billing products API. Stock needs
//! OAuth credentials, so it is `unknown`.

use anyhow::Result;

use crate::model::{Offer, Stock};
use crate::util::{get_json, parse_storage};

const URL: &str = "https://api.phoenixnap.com/billing/v1/products?productCategory=SERVER";

fn location(code: &str) -> (&'static str, &'static str) {
    match code {
        "PHX" => ("US", "Phoenix"),
        "ASH" => ("US", "Ashburn"),
        "CHI" => ("US", "Chicago"),
        "SEA" => ("US", "Seattle"),
        "NLD" => ("NL", "Amsterdam"),
        "SGP" => ("SG", "Singapore"),
        _ => ("", "?"),
    }
}

pub async fn fetch(client: &reqwest::Client) -> Result<Vec<Offer>> {
    let v = get_json(client, URL).await?;
    let mut out = Vec::new();
    for p in v.as_array().into_iter().flatten() {
        let m = &p["metadata"];
        if m["gpuConfigurations"]
            .as_array()
            .is_some_and(|g| !g.is_empty())
        {
            continue;
        }
        let code = p["productCode"].as_str().unwrap_or_default();
        let count = m["cpuCount"].as_u64().unwrap_or(1) as u32;
        let storage = m["storage"].as_str().unwrap_or_default();
        for plan in p["plans"].as_array().into_iter().flatten() {
            if plan["pricingModel"] != "ONE_MONTH_RESERVATION" {
                continue;
            }
            let loc = plan["location"].as_str().unwrap_or_default();
            let (country, city) = location(loc);
            let mut o = Offer::new("phoenixnap", format!("{code}@{loc}"));
            o.name = code.to_string();
            o.cpu = m["cpu"].as_str().unwrap_or_default().to_string();
            o.cpu_count = count;
            o.cores = m["coresPerCpu"].as_u64().map(|c| c as u32 * count);
            o.ram_gb = m["ramInGb"].as_f64();
            (o.storage_gb, o.nvme) = parse_storage(storage);
            o.storage = storage.to_string();
            o.port_mbps = m["network"]
                .as_str()
                .and_then(|n| n.split('x').nth(1))
                .and_then(crate::util::first_num)
                .map(|g| (g * 1000.0) as u32);
            o.location = city.into();
            o.country = country.into();
            o.price_monthly = plan["price"].as_f64().unwrap_or(0.0);
            o.currency = "USD";
            o.stock = Stock::Unknown;
            o.url = "https://phoenixnap.com/bare-metal-cloud/instances".into();
            o.note = Some("1-month reservation price; stock needs API credentials".into());
            out.push(o);
        }
    }
    Ok(out)
}
