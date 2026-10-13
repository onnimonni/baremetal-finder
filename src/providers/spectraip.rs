//! SpectraIP (Amsterdam): specs on the marketing page, stock per plan from
//! the WHMCS add-to-cart page ("Out of Stock" vs. redirect to configure).

use anyhow::Result;
use scraper::{ElementRef, Html, Selector};

use crate::model::{Offer, Stock};
use crate::util::{first_num, get_text, parse_storage};
use crate::whmcs;

const PAGE: &str = "https://spectraip.net/dedicated-servers/";

pub async fn fetch(client: &reqwest::Client) -> Result<Vec<Offer>> {
    let html = get_text(client, PAGE).await?;
    let plans = parse(&html);
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for (pid, url, lines) in plans {
        if !seen.insert(pid.clone()) {
            continue;
        }
        // Lines look like "CPU: Intel Core i5-1135G7 (4c/8t @ 2.4-4.2 GHz)".
        let field = |label: &str| {
            lines
                .iter()
                .find_map(|l| {
                    let (k, v) = l.split_once(':')?;
                    k.trim()
                        .eq_ignore_ascii_case(label)
                        .then(|| v.trim().to_string())
                })
                .unwrap_or_default()
        };
        let Some(price) = lines
            .iter()
            .find(|l| l.contains('€') && !l.contains("setup"))
            .and_then(|l| first_num(l))
        else {
            continue;
        };
        let cpu_field = field("CPU");
        let (cpu_line, detail) = cpu_field.split_once('(').unwrap_or((&cpu_field, ""));
        let (cpu_line, detail) = (cpu_line.trim().to_string(), detail.to_string());
        let storage = field("Storage");
        let ram = field("RAM");
        let setup = lines
            .iter()
            .find(|l| l.to_lowercase().contains("setup"))
            .and_then(|l| first_num(l))
            .unwrap_or(0.0);
        // "(4c/8t @ 2.4-4.2 GHz)"
        let ct = detail.as_str();

        let mut o = Offer::new("spectraip", pid.clone());
        o.name = lines.first().cloned().unwrap_or_default();
        o.cpu = cpu_line.clone();
        o.cores = ct.split('c').next().and_then(first_num).map(|c| c as u32);
        o.threads = ct.split('/').nth(1).and_then(first_num).map(|t| t as u32);
        o.ram_gb = first_num(&ram);
        (o.storage_gb, o.nvme) = parse_storage(&storage);
        o.storage = storage.clone();
        o.port_mbps = first_num(&field("Uplink")).map(|g| (g * 1000.0) as u32);
        o.traffic = Some(field("Bandwidth")).filter(|t| !t.is_empty());
        o.location = "Amsterdam".into();
        o.country = "NL".into();
        o.price_monthly = price;
        o.setup_fee = setup;
        // WHMCS add-to-cart says "Out of Stock" or redirects to configure.
        o.stock = match get_text(client, &url).await {
            Ok(page) if page.contains("Out of Stock") => Stock::OutOfStock,
            Ok(_) => Stock::InStock,
            Err(_) => Stock::Unknown,
        };
        o.spec_text = format!("{ram} {storage}");
        o.url = url;
        out.push(o);
    }
    Ok(out)
}

/// (pid, order url, text lines of the plan's card) for each order link.
fn parse(html: &str) -> Vec<(String, String, Vec<String>)> {
    let doc = Html::parse_document(html);
    let link = Selector::parse("a[href*='pid=']").expect("static CSS selector is valid");
    doc.select(&link)
        .filter_map(|a| {
            let href = a.value().attr("href")?.to_string();
            let pid = href.split("pid=").nth(1)?.split('&').next()?.to_string();
            // The card is the closest ancestor that also holds the specs.
            let card = a
                .ancestors()
                .filter_map(ElementRef::wrap)
                .take(8)
                .find(|e| e.text().any(|t| t.trim().starts_with("CPU")))?;
            Some((pid, href, whmcs::lines(card)))
        })
        .collect()
}
