//! Nforce (NL + US): HTML server lists behind a tiny JS cookie gate
//! (`afh=` cookie). Stock and setup fee live on each product's detail page.

use std::sync::LazyLock;

use anyhow::Result;
use regex::Regex;
use scraper::{ElementRef, Html, Selector};

use crate::model::{Offer, Stock};
use crate::util::{clean, first_num, parse_storage};

const BASE: &str = "https://www.nforce.com";
const LISTS: &[(&str, &str, &str)] = &[
    ("/dedicated-servers", "NL", "Amsterdam"),
    ("/usa-servers", "US", "New York"),
];

fn sel(s: &str) -> Selector {
    Selector::parse(s).expect("static CSS selector is valid")
}

fn text(e: ElementRef) -> String {
    clean(&e.text().collect::<Vec<_>>().join(" "))
}

/// GET that answers the "set afh cookie and reload" page once.
async fn get(client: &reqwest::Client, url: &str) -> Result<String> {
    static AFH: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"afh=([0-9a-f]+)").expect("static regex is valid"));
    let body = client
        .get(url)
        .send()
        .await?
        .error_for_status()?
        .text()
        .await?;
    match AFH.captures(&body) {
        Some(c) if body.len() < 5000 => Ok(client
            .get(url)
            .header("Cookie", format!("afh={}", &c[1]))
            .send()
            .await?
            .error_for_status()?
            .text()
            .await?),
        _ => Ok(body),
    }
}

pub async fn fetch(client: &reqwest::Client) -> Result<Vec<Offer>> {
    let mut out = Vec::new();
    for (path, country, city) in LISTS {
        let html = get(client, &format!("{BASE}{path}")).await?;
        let rows: Vec<Offer> = parse_list(&html, country, city);
        for mut o in rows {
            if let Ok(detail) = get(client, &o.url).await {
                apply_detail(&mut o, &detail);
            }
            out.push(o);
        }
    }
    Ok(out)
}

fn parse_list(html: &str, country: &str, city: &str) -> Vec<Offer> {
    let doc = Html::parse_document(html);
    let item = sel("div.list-item");
    let get = |e: ElementRef, s: &str| e.select(&sel(s)).next().map(text).unwrap_or_default();
    let mut out = Vec::new();
    for it in doc.select(&item) {
        let Some(price) = it
            .select(&sel(".column.mrc b"))
            .next()
            .map(text)
            .and_then(|t| first_num(&t))
        else {
            continue;
        };
        let Some(href) = it
            .select(&sel(".configure a[href]"))
            .next()
            .and_then(|a| a.value().attr("href"))
        else {
            continue;
        };
        let cpu_col = get(it, ".column.cpu");
        let details = get(it, ".cpuDetails");
        let cpu = cpu_col.replace(&details, "").trim().to_string();
        let storage = get(it, ".column.storage");
        let bw = get(it, ".column.bandwidth");

        let mut o = Offer::new("nforce", href.rsplit('/').next().unwrap_or(href));
        o.name = get(it, ".chassis span.tag");
        o.cpu = cpu.trim_start_matches("Dual ").to_string();
        if cpu.starts_with("Dual ") {
            o.cpu_count = 2;
        }
        // "4C/8T @ 3.5-3.9Ghz"
        o.cores = details
            .split('C')
            .next()
            .and_then(first_num)
            .map(|c| c as u32);
        o.threads = details
            .split('/')
            .nth(1)
            .and_then(first_num)
            .map(|t| t as u32);
        o.ram_gb = first_num(&get(it, ".column.memory"));
        (o.storage_gb, o.nvme) = parse_storage(&storage);
        o.storage = storage;
        o.port_mbps = bw
            .split("Gbit")
            .next()
            .and_then(|s| s.split_whitespace().last())
            .and_then(first_num)
            .map(|g| (g * 1000.0) as u32);
        o.traffic = bw.split("Traffic").next().map(|t| t.trim().to_string());
        o.location = city.into();
        o.country = country.into();
        o.price_monthly = price;
        o.url = format!("{BASE}{href}");
        out.push(o);
    }
    out
}

fn apply_detail(o: &mut Offer, html: &str) {
    let doc = Html::parse_document(html);
    if let Some(status) = doc
        .select(&sel("img[alt^='Status:']"))
        .next()
        .and_then(|i| i.value().attr("alt"))
    {
        o.stock = if status.contains("In stock") {
            Stock::InStock
        } else {
            Stock::Unknown
        };
        o.availability = Some(status.trim_start_matches("Status:").trim().to_string());
    }
    o.setup_fee = doc
        .select(&sel("#nrc"))
        .next()
        .map(text)
        .and_then(|t| first_num(&t))
        .unwrap_or(0.0);
    o.spec_text = doc
        .select(&sel("table.properties"))
        .next()
        .map(text)
        .unwrap_or_default();
}
