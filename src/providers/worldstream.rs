//! Worldstream (NL): React Router "single fetch" turbo-stream data endpoint
//! with `outOfStock` + `limitedAvailabilityCount` per server.

use std::collections::HashMap;

use anyhow::{Context, Result};
use serde_json::{Map, Value};

use crate::model::{Offer, Stock};
use crate::util::get_text;

const BASE: &str = "https://www.worldstream.com/en/dedicated-servers";

pub async fn fetch(client: &reqwest::Client) -> Result<Vec<Offer>> {
    let raw = get_text(client, &format!("{BASE}.data")).await?;
    let root = decode_turbo(&raw)?;
    let servers = find_key(&root, "servers")
        .and_then(Value::as_array)
        .cloned()
        .context("worldstream: no servers in payload")?;
    let mut out = Vec::new();
    for s in &servers {
        let Some(mut o) = offer(s) else { continue };
        if o.ram_gb.is_none() {
            // Base RAM only lives in the configurator payload.
            let url = format!(
                "{BASE}/configure/{}/{}.data",
                s["id"],
                s["configurationSelectionHash"].as_str().unwrap_or_default()
            );
            if let Ok(cfg) = get_text(client, &url).await.and_then(|t| decode_turbo(&t)) {
                o.ram_gb = find_key(&cfg, "response2")
                    .and_then(|r| r["memory"]["defaultSelected"].as_f64());
            }
        }
        out.push(o);
    }
    Ok(out)
}

fn offer(s: &Value) -> Option<Offer> {
    let cpu = &s["cpu"];
    let count = cpu["amount"].as_u64().unwrap_or(1) as u32;
    let disks: Vec<&Value> = s["storage"]
        .as_array()
        .map(|a| a.iter().collect())
        .unwrap_or_default();
    let storage = disks
        .iter()
        .map(|d| format!("{}x {}", d["amount"], d["name"].as_str().unwrap_or("?")))
        .collect::<Vec<_>>()
        .join(" + ");
    let storage_gb: f64 = disks
        .iter()
        .map(|d| d["amount"].as_f64().unwrap_or(1.0) * d["size"].as_f64().unwrap_or(0.0))
        .sum();
    let out_of_stock = s["outOfStock"].as_bool().unwrap_or(false);
    let limited = s["limitedAvailabilityCount"].as_i64();
    let loc = s["location"].as_str().unwrap_or_default();

    let mut o = Offer::new("worldstream", s["id"].to_string());
    o.name = s["chassis"]["name"]
        .as_str()
        .unwrap_or_default()
        .to_string();
    o.cpu = cpu["name"].as_str()?.to_string();
    o.cpu_count = count;
    o.cores = cpu["cores"].as_u64().map(|c| c as u32 * count);
    o.threads = cpu["threads"].as_u64().map(|t| t as u32 * count);
    o.ram_gb = s["memory"]["initial"].as_f64();
    o.ecc = s["memory"]["suffix"].as_str().map(|x| x.contains("ECC"));
    o.storage = storage;
    o.storage_gb = (storage_gb > 0.0).then_some(storage_gb);
    o.nvme = Some(disks.iter().any(|d| {
        d["type"] == "pcie-ssd" || d["name"].as_str().is_some_and(|n| n.contains("NVMe"))
    }));
    o.port_mbps = s["uplink"]["guaranteed"].as_u64().map(|u| u as u32);
    o.traffic = s["uplink"]["monthly"].as_f64().map(|t| format!("{t} TB"));
    o.location = loc.to_string();
    o.country = loc.to_uppercase();
    o.price_monthly = s["price"]["netPrice"].as_f64()? / 100.0;
    o.setup_fee = s["setupFee"].as_f64().unwrap_or(0.0) / 100.0;
    o.currency = if s["price"]["currency"]["currencyIsoCode"] == "USD" {
        "USD"
    } else {
        "EUR"
    };
    o.stock = if out_of_stock {
        Stock::OutOfStock
    } else {
        Stock::InStock
    };
    o.stock_qty = limited;
    o.availability = s["ttd"].as_i64().map(|m| format!("{}h", m / 60));
    o.spec_text = format!(
        "{} {}",
        s["memory"]["suffix"].as_str().unwrap_or_default(),
        o.storage
    );
    o.url = format!(
        "{BASE}/configure/{}/{}",
        s["id"],
        s["configurationSelectionHash"].as_str().unwrap_or_default()
    );
    Some(o)
}

/// Decode React Router's turbo-stream: a flat array where objects are
/// `{"_<keyIdx>": valueIdx}` and negative indices are undefined/null.
fn decode_turbo(raw: &str) -> Result<Value> {
    let first = raw.lines().next().unwrap_or_default();
    let arr: Vec<Value> =
        serde_json::from_str(first).context("turbo-stream: first line not a JSON array")?;
    let mut memo = HashMap::new();
    Ok(decode_at(&arr, 0, &mut memo, 0))
}

fn decode_at(arr: &[Value], i: i64, memo: &mut HashMap<i64, Value>, depth: usize) -> Value {
    if i < 0 || depth > 200 {
        return Value::Null;
    }
    if let Some(v) = memo.get(&i) {
        return v.clone();
    }
    let Some(v) = arr.get(i as usize) else {
        return Value::Null;
    };
    let out = match v {
        Value::Array(items) => {
            // Tagged special values like ["D", ...] (dates) are kept as-is.
            if matches!(items.first(), Some(Value::String(s)) if s.len() == 1)
                && items.len() <= 3
                && !items.get(1).is_some_and(Value::is_number)
            {
                v.clone()
            } else {
                Value::Array(
                    items
                        .iter()
                        .map(|x| {
                            x.as_i64()
                                .map_or(Value::Null, |j| decode_at(arr, j, memo, depth + 1))
                        })
                        .collect(),
                )
            }
        }
        Value::Object(m) => {
            let mut obj = Map::new();
            for (k, x) in m {
                let key = k
                    .strip_prefix('_')
                    .and_then(|n| n.parse::<usize>().ok())
                    .and_then(|n| arr.get(n))
                    .and_then(Value::as_str)
                    .unwrap_or(k)
                    .to_string();
                obj.insert(
                    key,
                    x.as_i64()
                        .map_or(Value::Null, |j| decode_at(arr, j, memo, depth + 1)),
                );
            }
            Value::Object(obj)
        }
        other => other.clone(),
    };
    memo.insert(i, out.clone());
    out
}

/// Depth-first search for the first value under `key`.
fn find_key<'a>(v: &'a Value, key: &str) -> Option<&'a Value> {
    match v {
        Value::Object(m) => m
            .get(key)
            .or_else(|| m.values().find_map(|x| find_key(x, key))),
        Value::Array(a) => a.iter().find_map(|x| find_key(x, key)),
        _ => None,
    }
}
