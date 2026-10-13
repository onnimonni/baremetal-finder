//! Evoluso (NL/CH/SE "offshore" dedicated): server-rendered plan list. The
//! cart tracks stock but reports -1 (not tracked) for every plan, so stock is
//! unknown.

use anyhow::Result;
use scraper::{ElementRef, Html, Selector};

use crate::model::{Offer, Stock};
use crate::util::{clean, first_num, get_text, parse_storage};

const URL: &str = "https://evoluso.com/offshore-dedicated-servers";

fn sel(s: &str) -> Selector {
    Selector::parse(s).expect("static CSS selector is valid")
}

fn text(e: ElementRef) -> String {
    clean(&e.text().collect::<Vec<_>>().join(" "))
}

pub async fn fetch(client: &reqwest::Client) -> Result<Vec<Offer>> {
    let html = get_text(client, URL).await?;
    let doc = Html::parse_document(&html);
    // Walk headings and rows in document order to know each row's country.
    let nodes = sel("h3, li.grid");
    let label = sel("div[class*='xl:hidden']");
    let link = sel("a[href*='/cart/']");
    let mut country = "";
    let mut out = Vec::new();
    for n in doc.select(&nodes) {
        if n.value().name() == "h3" {
            country = match text(n).as_str() {
                t if t.contains("Netherlands") => "NL",
                t if t.contains("Switzerland") => "CH",
                t if t.contains("Sweden") => "SE",
                _ => country,
            };
            continue;
        }
        // Each field is a hidden label followed by its value.
        let field = |name: &str| -> Option<String> {
            let l = n.select(&label).find(|l| text(*l) == name)?;
            l.next_siblings()
                .filter_map(ElementRef::wrap)
                .next()
                .map(text)
        };
        let Some(price) = field("Price").and_then(|p| first_num(&p)) else {
            continue;
        };
        let Some(href) = n.select(&link).next().and_then(|a| a.value().attr("href")) else {
            continue;
        };
        let cpu_block = n.select(&sel("h4")).next();
        let ct = cpu_block
            .and_then(|h| h.next_siblings().filter_map(ElementRef::wrap).next())
            .map(text)
            .unwrap_or_default();
        let storage = field("Storage").unwrap_or_default().replace('×', "x");
        let memory = field("Memory").unwrap_or_default();

        let mut o = Offer::new("evoluso", href.rsplit('/').next().unwrap_or(href));
        o.name = field("Server").unwrap_or_default();
        o.cpu = cpu_block.map(text).unwrap_or_default();
        o.cores = ct
            .split("cores")
            .next()
            .and_then(first_num)
            .map(|c| c as u32);
        o.threads = ct.split('·').nth(1).and_then(first_num).map(|t| t as u32);
        o.ram_gb = first_num(&memory);
        (o.storage_gb, o.nvme) = parse_storage(&storage);
        o.storage = storage.clone();
        o.port_mbps = field("Port speed")
            .and_then(|p| first_num(&p))
            .map(|g| (g * 1000.0) as u32);
        o.location = country.to_string();
        o.country = country.to_string();
        o.price_monthly = price;
        o.stock = Stock::Unknown;
        o.spec_text = format!("{memory} {storage}");
        o.url = href.to_string();
        o.note = Some("stock not tracked by provider".into());
        out.push(o);
    }
    Ok(out)
}
