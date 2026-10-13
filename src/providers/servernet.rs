//! server.net (Amsterdam/London): server list is server-rendered HTML with
//! data-soldout / data-preorder flags and per-currency prices as attributes.

use std::sync::LazyLock;

use anyhow::Result;
use regex::Regex;
use scraper::{ElementRef, Html, Selector};

use crate::model::{Offer, Stock};
use crate::util::{clean, first_num, get_text, parse_storage};

const URL: &str = "https://server.net/dedicated-servers";

fn sel(s: &str) -> Selector {
    Selector::parse(s).expect("static CSS selector is valid")
}

fn text(e: ElementRef) -> String {
    clean(&e.text().collect::<String>())
}

pub async fn fetch(client: &reqwest::Client) -> Result<Vec<Offer>> {
    let html = get_text(client, URL).await?;
    Ok(parse(&html))
}

fn parse(html: &str) -> Vec<Offer> {
    static CORES: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"(\d+)\s*Core\s*\((\d+)C HT\)").expect("static regex is valid")
    });
    static PAREN: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"\(([^)]+)\)").expect("static regex is valid"));
    let doc = Html::parse_document(html);
    let item = sel(".server-item");
    let name_sel = sel(".text-xl a");
    let cpu_sel = sel("div.font-semibold");
    let cores_sel = sel("p.text-sm");
    let bold_sel = sel("div.text-md.font-bold");
    let price_sel = sel(".price-cur");
    let multi_sel = sel("[id$='-multi']");

    let mut out = Vec::new();
    for it in doc.select(&item) {
        let attr = |k: &str| it.value().attr(k).unwrap_or_default();
        let Some(name) = it.select(&name_sel).next().map(text) else {
            continue;
        };
        let cpu_text = it.select(&cpu_sel).next().map(text).unwrap_or_default();
        // "AMD Ryzen 9950X (EPYC 4565P)": the parenthesised part is the real
        // server CPU; the first part is the marketing desktop equivalent.
        let cpu = PAREN
            .captures(&cpu_text)
            .map(|c| format!("AMD {}", &c[1]))
            .filter(|_| cpu_text.contains("Ryzen") && cpu_text.contains("EPYC"))
            .unwrap_or_else(|| cpu_text.clone());
        let bolds: Vec<String> = it.select(&bold_sel).map(text).collect();
        let prices: Vec<f64> = it
            .select(&price_sel)
            .filter_map(|p| p.value().attr("data-price-eur").and_then(first_num))
            .collect();
        let Some(&monthly) = prices.first() else {
            continue;
        };
        let location = attr("data-location");

        let mut o = Offer::new("servernet", format!("{name}-{location}"));
        o.name = name;
        o.cpu = cpu;
        if let Some(c) = it.select(&cores_sel).next().map(text).and_then(|t| {
            CORES
                .captures(&t)
                .map(|c| (c[1].to_string(), c[2].to_string()))
        }) {
            o.cores = c.0.parse().ok();
            o.threads = c.1.parse().ok();
        }
        o.provider_cpu_mark = it
            .select(&multi_sel)
            .next()
            .map(text)
            .and_then(|t| first_num(&t));
        o.ram_gb = bolds.first().and_then(|r| first_num(r));
        o.ecc = Some(text(it).contains("ECC"));
        if let Some(s) = bolds.get(1) {
            (o.storage_gb, o.nvme) = parse_storage(s);
            o.storage = s.clone();
        }
        if let Some(n) = bolds.get(2) {
            o.port_mbps = first_num(n).map(|g| (g * 1000.0) as u32);
            o.traffic = n.split_whitespace().nth(1).map(String::from);
        }
        o.location = location.to_string();
        o.country = match location {
            "london" => "GB",
            "amsterdam" => "NL",
            _ => "",
        }
        .into();
        o.price_monthly = monthly;
        o.setup_fee = prices.get(1).copied().unwrap_or(0.0);
        let sold_out = attr("data-soldout") == "1";
        o.stock = if sold_out {
            Stock::OutOfStock
        } else {
            Stock::InStock
        };
        o.availability = Some(
            if attr("data-preorder") == "1" && sold_out {
                "pre-order"
            } else if sold_out {
                "sold out"
            } else {
                "in stock"
            }
            .into(),
        );
        o.spec_text = text(it);
        o.url = URL.into();
        o.note = Some(cpu_text);
        out.push(o);
    }
    out
}
