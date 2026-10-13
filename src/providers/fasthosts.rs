//! Fasthosts (UK, DE, ES, FR, US): plans from the server page, per-datacentre
//! stock from the public stock API. API model ids map to page ids via
//! `productCodeSwapping` in the page's stock-control script.

use std::collections::HashMap;
use std::sync::LazyLock;

use anyhow::{Context, Result};
use regex::Regex;
use scraper::{ElementRef, Html, Selector};

use crate::model::{Offer, Stock};
use crate::util::{clean, first_num, get_json, get_text, parse_storage};

const SITE: &str = "https://www.fasthosts.co.uk";
const STOCK: &str = "https://api.fasthosts.co.uk/server-stock/api/v1/stock-levels?stockGroup=ds";

fn sel(s: &str) -> Selector {
    Selector::parse(s).expect("static CSS selector is valid")
}

fn text(e: ElementRef) -> String {
    clean(&e.text().collect::<Vec<_>>().join(" "))
}

pub async fn fetch(client: &reqwest::Client) -> Result<Vec<Offer>> {
    static SCRIPT: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r#"(/_astro/server-stock-control[^"']+\.js)"#).expect("static regex is valid")
    });
    static PAIR: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r#"(\w+):"(DS_\w+)""#).expect("static regex is valid"));
    let html = get_text(client, &format!("{SITE}/dedicated-servers")).await?;
    let script = SCRIPT
        .captures(&html)
        .context("fasthosts: no stock script")?[1]
        .to_string();
    let js = get_text(client, &format!("{SITE}{script}")).await?;
    // page id (lowercase DS_*) -> API model id (BM_*)
    let api_id: HashMap<String, String> = PAIR
        .captures_iter(&js)
        .map(|c| (c[2].to_lowercase(), c[1].to_string()))
        .collect();
    let stock = get_json(client, STOCK).await?;
    let dcs: HashMap<String, (String, String)> = stock["data"]["datacentres"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|d| {
            Some((
                d["publicDcCode"].as_str()?.to_string(),
                (
                    d["countryCode"].as_str()?.to_string(),
                    d["label"].as_str().unwrap_or_default().to_string(),
                ),
            ))
        })
        .collect();
    let levels: HashMap<String, Vec<(String, String)>> = stock["data"]["models"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|m| {
            let av = m["availability"]
                .as_array()?
                .iter()
                .filter_map(|a| {
                    Some((
                        a["publicDcCode"].as_str()?.to_string(),
                        a["stockLevel"].as_str()?.to_string(),
                    ))
                })
                .collect();
            Some((m["id"].as_str()?.to_string(), av))
        })
        .collect();

    let doc = Html::parse_document(&html);
    let pkg = sel("div.package-list__package[data-server-model-id]");
    let spec = sel(".package-list__package-spec");
    let price = sel(".detailed-price__main .vat-toggle-price-container");
    let mut out = Vec::new();
    for p in doc.select(&pkg) {
        let model = p.value().attr("data-server-model-id").unwrap_or_default();
        let Some(net) = p
            .select(&price)
            .next()
            .and_then(|e| e.value().attr("data-price-exc-vat"))
            .and_then(first_num)
        else {
            continue;
        };
        let specs: Vec<String> = p.select(&spec).map(text).collect();
        let s = |i: usize| specs.get(i).cloned().unwrap_or_default();
        let (cpu_spec, ram_spec, disk_spec) = (s(0), s(1), s(2));
        let name = p
            .select(&sel(".package-list__package-name"))
            .next()
            .map(text)
            .unwrap_or_default();
        // "Intel Xeon E-2356G 6 cores x 3.2 GHz" -> "Intel Xeon E-2356G"
        let cpu = cpu_spec
            .split(" cores")
            .next()
            .unwrap_or_default()
            .rsplit_once(' ')
            .map_or(cpu_spec.as_str(), |(model, _)| model)
            .trim()
            .to_string();
        // "(2x 960GB)" carries the layout.
        let disk = disk_spec
            .split_once('(')
            .map(|(_, l)| l.trim_end_matches(')').to_string())
            .unwrap_or_else(|| disk_spec.clone());
        let avail = api_id
            .get(model)
            .and_then(|id| levels.get(id))
            .cloned()
            .unwrap_or_default();
        let dc_list: Vec<(String, Option<String>)> = if avail.is_empty() {
            vec![("gb.wtr".to_string(), None)]
        } else {
            avail.into_iter().map(|(dc, lvl)| (dc, Some(lvl))).collect()
        };
        for (dc, level) in dc_list {
            let (country, label) = dcs
                .get(&dc)
                .cloned()
                .unwrap_or_else(|| ("GB".into(), dc.clone()));
            let mut o = Offer::new("fasthosts", format!("{model}@{dc}"));
            o.name = name.clone();
            o.cpu = cpu.clone();
            o.cores = cpu_spec
                .split(" cores")
                .next()
                .and_then(|c| c.split_whitespace().last())
                .and_then(first_num)
                .map(|c| c as u32);
            o.ram_gb = first_num(&ram_spec);
            (o.storage_gb, o.nvme) = parse_storage(&disk);
            o.storage = disk_spec.clone();
            o.port_mbps = Some(1000);
            o.traffic = Some("unlimited".into());
            o.location = label;
            o.country = country;
            o.price_monthly = net;
            o.currency = "GBP";
            o.stock = match level.as_deref() {
                Some("INSTOCK" | "LOWSTOCK") => Stock::InStock,
                Some(_) => Stock::Unknown,
                None => Stock::OutOfStock,
            };
            o.availability = level;
            o.spec_text = format!("{ram_spec} {disk_spec}");
            o.url = format!("{SITE}/configure/dedicated/?model={}", model.to_uppercase());
            out.push(o);
        }
    }
    Ok(out)
}
