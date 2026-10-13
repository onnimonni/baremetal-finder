//! i3D.net: instance prices from the Cloudflare-protected marketing page
//! (fetched with the Chrome-emulating client) and the tokenless FlexMetal
//! location list. Per-location stock needs a FlexMetal API key (I3D_TOKEN).

use std::collections::HashMap;

use anyhow::Result;
use scraper::{ElementRef, Html, Selector};
use serde_json::Value;

use crate::model::{Offer, Stock};
use crate::util::{browser, browser_text, clean, first_num, parse_storage};

const PAGE: &str = "https://www.i3d.net/bare-metal-servers/";
const API: &str = "https://api.i3d.net/v3/flexMetal";

fn sel(s: &str) -> Selector {
    Selector::parse(s).expect("static CSS selector is valid")
}

fn text(e: ElementRef) -> String {
    clean(&e.text().collect::<String>())
}

struct Card {
    name: String,
    eur: f64,
    specs: Vec<(String, String)>,
}

pub async fn fetch(client: &reqwest::Client) -> Result<Vec<Offer>> {
    let c = browser()?;
    let html = browser_text(&c, PAGE).await?;
    let cards = parse_cards(&html);
    anyhow::ensure!(!cards.is_empty(), "i3d: no instance cards found");
    let locations: Vec<Value> = crate::util::get_json(client, &format!("{API}/location"))
        .await?
        .as_array()
        .cloned()
        .unwrap_or_default();
    let stock = match std::env::var("I3D_TOKEN") {
        Ok(token) => plan_stock(client, &token).await.unwrap_or_else(|e| {
            eprintln!("i3d stock: {e:#}");
            HashMap::new()
        }),
        Err(_) => HashMap::new(),
    };

    let mut out = Vec::new();
    for card in &cards {
        let spec = |k: &str| {
            card.specs
                .iter()
                .find(|(l, _)| l.eq_ignore_ascii_case(k))
                .map(|(_, v)| v.clone())
                .unwrap_or_default()
        };
        let cpu_line = spec("CPU");
        let storage = spec("Storage");
        for loc in &locations {
            let short = loc["shortName"].as_str().unwrap_or_default();
            let mut o = Offer::new("i3d", format!("{}@{short}", card.name));
            o.name = card.name.clone();
            o.cpu = spec("CPU model");
            o.cores = first_num(&cpu_line).map(|c| c as u32);
            o.threads = cpu_line
                .split('/')
                .nth(1)
                .and_then(first_num)
                .map(|t| t as u32);
            o.ram_gb = first_num(&spec("RAM"));
            (o.storage_gb, o.nvme) = parse_storage(&storage);
            o.storage = storage.clone();
            o.location = loc["name"].as_str().unwrap_or(short).to_string();
            o.country = loc["countryShortName"]
                .as_str()
                .unwrap_or_default()
                .to_string();
            o.price_monthly = card.eur;
            (o.stock, o.availability) = match stock.get(&(card.name.clone(), o.location.clone())) {
                Some(a) if a == "unavailable" => (Stock::OutOfStock, Some(a.clone())),
                Some(a) => (Stock::InStock, Some(a.clone())),
                None => (Stock::Unknown, None),
            };
            o.spec_text = card
                .specs
                .iter()
                .map(|(k, v)| format!("{k}: {v}"))
                .collect::<Vec<_>>()
                .join(" | ");
            o.url = PAGE.into();
            o.note = Some("\"from\" price; stock needs I3D_TOKEN".into());
            out.push(o);
        }
    }
    Ok(out)
}

/// Each Elementor column with an `.instance-price` is one instance type.
fn parse_cards(html: &str) -> Vec<Card> {
    let doc = Html::parse_document(html);
    let col = sel(".elementor-inner-column");
    let price = sel(".instance-price");
    let title = sel(".elementor-heading-title.elementor-size-large");
    let para = sel(".elementor-widget-text-editor p");
    let label = sel("strong");
    doc.select(&col)
        .filter_map(|c| {
            let eur = c.select(&price).next()?.value().attr("data-eur").and_then(first_num)?;
            let name = c.select(&title).next().map(text)?;
            let p = c.select(&para).next()?;
            // "<strong>CPU:</strong> 8 Cores / … <br/><strong>CPU model: </strong>…"
            let specs = p
                .select(&label)
                .map(|s| {
                    let key = text(s).trim_end_matches(':').trim().to_string();
                    let value: String = s
                        .next_siblings()
                        .take_while(|n| !matches!(n.value(), scraper::Node::Element(e) if e.name() == "strong" || e.name() == "br"))
                        .filter_map(|n| n.value().as_text().map(|t| t.to_string()))
                        .collect();
                    (key, clean(&value))
                })
                .collect();
            Some(Card { name, eur, specs })
        })
        .collect()
}

/// (plan name, location name) -> availability ("high" … "unavailable").
async fn plan_stock(
    client: &reqwest::Client,
    token: &str,
) -> Result<HashMap<(String, String), String>> {
    let v: Value = client
        .get(format!("{API}/plans"))
        .header("PRIVATE-TOKEN", token)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    let mut out = HashMap::new();
    for p in v.as_array().into_iter().flatten() {
        let name = p["name"].as_str().unwrap_or_default();
        for l in p["locations"].as_array().into_iter().flatten() {
            if let (Some(loc), Some(a)) = (l["name"].as_str(), l["availability"].as_str()) {
                out.insert((name.to_string(), loc.to_string()), a.to_string());
            }
        }
    }
    Ok(out)
}
