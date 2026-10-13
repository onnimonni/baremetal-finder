//! webtropia (myLoc, Düsseldorf): Angular store backed by a JSON API that
//! the shop calls with a public "master token" embedded in its JS bundle.
//! Cloudflare rejects non-browser TLS, so this uses the Chrome-emulating client.

use std::sync::LazyLock;

use anyhow::{Context, Result};
use regex::Regex;
use serde_json::Value;

use crate::model::{Offer, Stock};
use crate::util::{browser, first_num, parse_storage};

const STORE: &str = "https://store.webtropia.com";

async fn get(c: &wreq::Client, url: &str, token: Option<&str>) -> Result<String> {
    let mut req = c.get(url);
    if let Some(t) = token {
        req = req
            .header("Authorization", format!("Bearer {t}"))
            .header("Accept", "application/json");
    }
    let resp = req.send().await.with_context(|| format!("GET {url}"))?;
    let status = resp.status();
    let body = resp.text().await?;
    anyhow::ensure!(status.is_success(), "GET {url} -> {status}");
    Ok(body)
}

pub async fn fetch(_client: &reqwest::Client) -> Result<Vec<Offer>> {
    static BUNDLE: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"main\.[a-f0-9]+\.js").expect("static regex is valid"));
    static TOKEN: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r#"storeId:"webtropia",masterToken:"([a-f0-9]+)""#)
            .expect("static regex is valid")
    });
    let c = browser()?;
    // The token is public (shipped to every browser); read it fresh from the
    // bundle unless WEBTROPIA_TOKEN overrides it.
    let token = match std::env::var("WEBTROPIA_TOKEN") {
        Ok(t) => t,
        Err(_) => {
            let home = get(&c, &format!("{STORE}/de/"), None)
                .await
                .context("webtropia: store page")?;
            let bundle = BUNDLE
                .find(&home)
                .context("webtropia: no main bundle")?
                .as_str();
            let js = get(&c, &format!("{STORE}/de/{bundle}"), None).await?;
            TOKEN
                .captures(&js)
                .context("webtropia: no master token in bundle")?[1]
                .to_string()
        }
    };

    let api = format!("{STORE}/api/stores/webtropia/products");
    let list: Value = serde_json::from_str(&get(&c, &api, Some(&token)).await?)?;
    let mut out = Vec::new();
    for p in list["content"].as_array().into_iter().flatten() {
        if p["type"] != "server" || p["available_for_order"] == "N" {
            continue;
        }
        let id = p["id"].as_i64().unwrap_or_default();
        let price = &p["price"][0];
        let Some(gross) = price["display_price"].as_f64() else {
            continue;
        };
        let net = if price["vat_included"] == true {
            gross / (1.0 + price["tax_rate"].as_f64().unwrap_or(19.0) / 100.0)
        } else {
            gross
        };
        let specs: Value = match get(&c, &format!("{api}/{id}/specs"), Some(&token)).await {
            Ok(s) => serde_json::from_str(&s).unwrap_or(Value::Null),
            Err(e) => {
                eprintln!("webtropia specs {id}: {e:#}");
                Value::Null
            }
        };
        let spec = |t: &str| -> Vec<String> {
            specs["content"]
                .as_array()
                .and_then(|a| a.iter().find(|s| s["type"] == t))
                .and_then(|s| s["slots"].as_array())
                .map(|slots| {
                    // Slots list alternatives per slot id; take the first of each.
                    let mut seen = std::collections::HashSet::new();
                    slots
                        .iter()
                        .filter(|x| seen.insert(x["slot_id"].to_string()))
                        .filter_map(|x| x["description"].as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default()
        };
        let cpu_text = spec("processor").join(" ");
        // "Intel Xeon E-2136 (6 x 3,3 GHz)"
        let cpu = cpu_text.split(" (").next().unwrap_or_default().to_string();
        let disks = spec("harddrive");
        let storage = disks.join(" + ").replace('.', "");
        let ram = spec("ram").join(" ");

        let mut o = Offer::new("webtropia", id.to_string());
        o.name = p["name"].as_str().unwrap_or(&cpu).to_string();
        o.cpu = cpu;
        o.cores = cpu_text
            .split(" (")
            .nth(1)
            .and_then(first_num)
            .map(|c| c as u32);
        o.ram_gb = first_num(&ram);
        o.ecc = Some(ram.contains("ECC"));
        (o.storage_gb, o.nvme) = parse_storage(&storage);
        o.storage = storage;
        o.location = "Düsseldorf".into();
        o.country = "DE".into();
        o.price_monthly = net;
        o.stock = match p["availability"].as_str() {
            Some("in_stock" | "limited") => Stock::InStock,
            Some("sold") => Stock::OutOfStock,
            _ => Stock::Unknown,
        };
        o.availability = p["availability"].as_str().map(String::from);
        o.spec_text = format!("{ram} {}", o.storage);
        o.url = format!("{STORE}/de/configurate/{id}");
        o.note = Some("VAT stripped".into());
        out.push(o);
    }
    Ok(out)
}
