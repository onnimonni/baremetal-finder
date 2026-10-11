//! IONOS (DE site): product tables live in `__NEXT_DATA__` (specs inside
//! compiled MDX strings); per-datacenter stock comes from the public
//! shop-api `bulk-server-status` endpoint. Prices are net.

use std::collections::HashMap;
use std::sync::LazyLock;

use anyhow::{Context, Result};
use regex::Regex;
use serde_json::{Value, json};

use crate::model::{Offer, Stock};
use crate::util::{clean, first_num, get_text, parse_storage};

const SITE: &str = "https://www.ionos.de";
const PAGES: &[&str] = &[
    "amd-server",
    "intel-server",
    "value-dedicated-server",
    "storage-server",
];
const STATUS: &str =
    "https://www.ionos.de/shop-api/v1/api/shop/server/bulk-server-status?useSession=true";

fn dc_country(id: &str) -> Option<(&'static str, &'static str)> {
    Some(match id {
        "66703" => ("DE", "Berlin/Karlsruhe"),
        "77990" => ("FR", "Strasbourg"),
        "1109" => ("ES", "Logroño"),
        "22820" => ("GB", "Gloucester"),
        "16198" => ("US", "Lenexa"),
        _ => return None,
    })
}

pub async fn fetch(client: &reqwest::Client) -> Result<Vec<Offer>> {
    static NEXT: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r#"(?s)<script id="__NEXT_DATA__" type="application/json">(.*?)</script>"#)
            .expect("static regex is valid")
    });
    let mut products = Vec::new();
    for page in PAGES {
        let html = get_text(client, &format!("{SITE}/server/{page}")).await?;
        let data = NEXT.captures(&html).context("ionos: no __NEXT_DATA__")?;
        let v: Value = serde_json::from_str(&data[1])?;
        let mut tables = Vec::new();
        collect_tables(&v, &mut tables);
        for t in tables {
            products.extend(t["products"].as_array().cloned().unwrap_or_default());
        }
    }
    let ids: Vec<String> = products
        .iter()
        .filter_map(|p| p["articleId"].as_str().map(String::from))
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect();
    let status: Value = client
        .post(STATUS)
        .header("site-id", "PU.WH.DE")
        .header("x-market", "DE")
        .header("request-source", SITE)
        .header("Origin", SITE)
        .header("Referer", format!("{SITE}/server/{}", PAGES[0]))
        .json(&json!(ids))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    let stock: HashMap<String, Value> = status
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|s| {
            Some((
                s["serverCmsIdentifier"].as_str()?.to_string(),
                s["datacenterFeatures"].clone(),
            ))
        })
        .collect();

    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for p in &products {
        let Some(id) = p["articleId"].as_str() else {
            continue;
        };
        if !seen.insert(id.to_string()) {
            continue;
        }
        let price = &p["price"];
        let Some(net) = price["price"]["net"].as_f64() else {
            continue;
        };
        let cells: Vec<Vec<String>> = p["data"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .map(mdx_texts)
            .collect();
        // Column layouts differ per page (storage servers put RAM in the CPU
        // cell), so classify the flattened texts instead of using positions.
        let head = cells.first().cloned().unwrap_or_default();
        let cpu = cells.get(1).cloned().unwrap_or_default();
        let all: Vec<String> = cells.iter().skip(1).flatten().cloned().collect();
        let name = p["articles"][0]["label"]
            .as_str()
            .map(|l| l.trim_start_matches("Dedicated Server ").to_string())
            .or_else(|| head.last().cloned())
            .unwrap_or_else(|| id.to_string());
        let ct = all
            .iter()
            .find(|s| s.contains(" C/"))
            .cloned()
            .unwrap_or_default();
        let ram: Vec<String> = all
            .iter()
            .filter(|s| {
                s.ends_with(" GB") && s.split_whitespace().count() == 2 || s.contains("DDR")
            })
            .cloned()
            .collect();
        // German decimals: "(2 x 1,024 TB)" -> "2 x 1.024 TB"
        let layouts: Vec<String> = all
            .iter()
            .filter(|s| s.starts_with('('))
            .map(|s| s.trim_matches(['(', ')']).replace(',', "."))
            .collect();
        let disk_text = layouts.join(" + ");
        let disk_label = all
            .iter()
            .filter(|s| {
                s.contains("TB")
                    || s.contains("RAID")
                    || s.contains("NVMe")
                    || s.contains("SSD")
                    || s.contains("HDD")
            })
            .cloned()
            .collect::<Vec<_>>()
            .join(" ");
        let regular = price["strikePrice"]["net"].as_f64();
        let mct = format!(
            "{} {}",
            price["mct"]["duration"],
            price["mct"]["durationType"]
                .as_str()
                .unwrap_or_default()
                .to_lowercase()
        );

        let dcs = stock
            .get(id)
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default();
        for (dc, f) in &dcs {
            let Some((country, city)) = dc_country(dc) else {
                continue;
            };
            let mut o = Offer::new("ionos", format!("{id}@{dc}"));
            o.name = name.clone();
            o.cpu = cpu.first().cloned().unwrap_or_default();
            o.cores = ct.split(" C/").next().and_then(first_num).map(|c| c as u32);
            o.threads = ct.split(" C/").nth(1).and_then(first_num).map(|t| t as u32);
            o.ram_gb = ram
                .iter()
                .find(|r| r.ends_with(" GB"))
                .and_then(|r| first_num(r));
            o.ecc = Some(ram.iter().any(|r| r.contains("ECC")));
            (o.storage_gb, _) = parse_storage(&disk_text);
            o.nvme = Some(disk_label.contains("NVMe"));
            o.storage = disk_label.clone();
            o.port_mbps = head.first().and_then(|h| first_num(h)).map(|g| if head[0].contains("Gbit") { g * 1000.0 } else { g } as u32);
            o.location = city.into();
            o.country = country.into();
            o.price_monthly = net;
            o.setup_fee = price["setup"]["net"].as_f64().unwrap_or(0.0);
            o.stock = match f["available"].as_bool() {
                Some(true) => Stock::InStock,
                Some(false) => Stock::OutOfStock,
                None => Stock::Unknown,
            };
            o.url = format!("{SITE}/server/{}", PAGES[0]);
            o.note = Some(format!(
                "min term {mct}{}",
                regular
                    .filter(|r| (r - net).abs() > 0.01)
                    .map(|r| format!("; promo, regular €{r}"))
                    .unwrap_or_default()
            ));
            out.push(o);
        }
    }
    Ok(out)
}

/// Objects that hold a product table.
fn collect_tables<'a>(v: &'a Value, out: &mut Vec<&'a Value>) {
    match v {
        Value::Object(m) => {
            if m.contains_key("products") && m.contains_key("priceTemplate") {
                out.push(v);
            }
            m.values().for_each(|x| collect_tables(x, out));
        }
        Value::Array(a) => a.iter().for_each(|x| collect_tables(x, out)),
        _ => {}
    }
}

/// Visible strings from a compiled MDX component: `children: "…"` and
/// bare `"\n…"` text nodes.
fn mdx_texts(src: &str) -> Vec<String> {
    static TXT: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r#"children:\s*"((?:[^"\\]|\\.)*)"|"\\n((?:[^"\\]|\\.)+)""#)
            .expect("static regex is valid")
    });
    TXT.captures_iter(src)
        .filter_map(|c| c.get(1).or(c.get(2)))
        .map(|m| clean(&m.as_str().replace("\\n", " ")))
        .filter(|s| !s.is_empty())
        .collect()
}
