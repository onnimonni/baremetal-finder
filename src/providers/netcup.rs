//! netcup root servers (KVM VMs with dedicated cores, not bare metal) via
//! the Nuxt site's public content/product API. Only month-to-month variants
//! are emitted; longer terms are noted.

use std::collections::BTreeSet;

use anyhow::Result;
use scraper::Html;
use serde_json::Value;

use crate::model::{Offer, Stock};
use crate::util::{clean, first_num, get_json, parse_storage};

const API: &str = "https://www.netcup.com/api/v2";

fn decode(s: &str) -> String {
    clean(
        &Html::parse_fragment(s)
            .root_element()
            .text()
            .collect::<String>(),
    )
}

fn country(code: &str) -> &'static str {
    match code {
        "VIE" => "AT",
        "MNZ" => "US",
        "SIN" => "SG",
        _ => "DE", // NUE, EU ("No preference Europe" = Nuremberg/Vienna)
    }
}

pub async fn fetch(client: &reqwest::Client) -> Result<Vec<Offer>> {
    let page = get_json(client, &format!("{API}/pages/slug/en/server/root-server")).await?;
    let mut slugs = BTreeSet::new();
    // Page contents reference sections as `{"section": {"id": N}}`.
    let sections: BTreeSet<i64> = page["data"]["contents"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|c| c["section"]["id"].as_i64())
        .collect();
    for id in sections {
        let Ok(sec) = get_json(client, &format!("{API}/content/sections/{id}")).await else {
            continue;
        };
        collect_urls(&sec, &mut slugs);
    }
    anyhow::ensure!(!slugs.is_empty(), "netcup: no root server products found");

    let mut out = Vec::new();
    for slug in slugs {
        let v = match get_json(client, &format!("{API}/pages/slug{slug}")).await {
            Ok(v) => v,
            Err(e) => {
                eprintln!("netcup {slug}: {e:#}");
                continue;
            }
        };
        let variants = v["data"]["variants"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        for var in variants
            .iter()
            .filter(|x| x["minimum_contract_period"] == 1)
        {
            let title = var["title"].as_str().unwrap_or_default();
            let loc = title.rsplit(' ').next().unwrap_or_default();
            let longer: Vec<String> = variants
                .iter()
                .filter(|x| {
                    x["minimum_contract_period"] != 1
                        && x["title"]
                            .as_str()
                            .is_some_and(|t| t.ends_with(&format!(" {loc}")))
                })
                .map(|x| format!("{}M €{}", x["minimum_contract_period"], x["netPrice"]))
                .collect();
            let details = if var["details"].is_array() {
                &var["details"]
            } else {
                &v["data"]["details"]
            };
            let feats: Vec<(String, String)> = details
                .as_array()
                .into_iter()
                .flatten()
                .filter(|d| d["title"] == "Features")
                .flat_map(|d| d["features"].as_array().cloned().unwrap_or_default())
                .map(|f| {
                    (
                        f["label"].as_str().unwrap_or_default().to_string(),
                        decode(f["value"].as_str().unwrap_or_default()),
                    )
                })
                .collect();
            let feat = |l: &str| {
                feats
                    .iter()
                    .find(|(k, _)| k == l)
                    .map(|(_, v)| v.clone())
                    .unwrap_or_default()
            };
            let disk = feat("Hard disk");
            let sold_out = var["showSoldOut"] == true || var["availableToOrder"] == false;

            let mut o = Offer::new("netcup", var["id"].to_string());
            o.name = title.to_string();
            o.cpu = feat("Processor");
            o.dedicated_cores = first_num(&feat("CPU cores")).map(|c| c as u32);
            o.cores = o.dedicated_cores;
            o.ram_gb = first_num(&feat("Main memory"));
            (o.storage_gb, o.nvme) = parse_storage(&disk);
            o.storage = disk;
            o.port_mbps = first_num(&feat("Interface Speed")).map(|g| (g * 1000.0) as u32);
            o.traffic = Some("flat (throttled >3TB/24h)".into());
            o.location = feat("Server location");
            o.country = country(loc).into();
            o.price_monthly = var["netPrice"].as_f64().unwrap_or(0.0);
            o.setup_fee = var["netSetup_costs"].as_f64().unwrap_or(0.0);
            o.stock = if sold_out {
                Stock::OutOfStock
            } else {
                Stock::InStock
            };
            o.spec_text = feats
                .iter()
                .map(|(k, v)| format!("{k}: {v}"))
                .collect::<Vec<_>>()
                .join(" | ");
            o.url = format!("https://www.netcup.com{slug}");
            o.note = Some(format!("VM with dedicated cores; {}", longer.join(", ")));
            out.push(o);
        }
    }
    Ok(out)
}

/// Product page paths ("/en/server/root-server/<slug>") anywhere in a section.
fn collect_urls(v: &Value, out: &mut BTreeSet<String>) {
    match v {
        Value::Object(m) => {
            if let Some(u) = m.get("fullUrl").and_then(Value::as_str)
                && m.contains_key("minimumContractPeriod")
            {
                out.insert(u.split('#').next().unwrap_or(u).to_string());
            }
            m.values().for_each(|x| collect_urls(x, out));
        }
        Value::Array(a) => a.iter().for_each(|x| collect_urls(x, out)),
        _ => {}
    }
}
