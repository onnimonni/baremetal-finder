//! OneProvider: Drupal AJAX search endpoint returning HTML rows, 50 per page,
//! with a stock tooltip (Express Delivery / In Stock / Limited / Sold Out).

use std::sync::LazyLock;

use anyhow::Result;
use regex::Regex;
use scraper::{ElementRef, Html, Selector};
use serde_json::Value;

use crate::model::{Offer, Stock};
use crate::util::{clean, first_num, get_json, parse_storage};

const SEARCH: &str = "https://oneprovider.com/search-page";

fn sel(s: &str) -> Selector {
    Selector::parse(s).expect("static CSS selector is valid")
}

fn text(e: ElementRef) -> String {
    clean(&e.text().collect::<String>().replace('\u{a0}', " "))
}

pub async fn fetch(client: &reqwest::Client) -> Result<Vec<Offer>> {
    let mut out = Vec::new();
    for page in 0..200 {
        let v = get_json(
            client,
            &format!("{SEARCH}/{page}?soldout=0&hide_cloud_instances=1"),
        )
        .await?;
        let cmds = v.as_array().cloned().unwrap_or_default();
        let html = cmds
            .iter()
            .find(|c| c["selector"] == ".results-body")
            .and_then(|c| c["data"].as_str())
            .unwrap_or_default();
        let rows = parse(html);
        let n = rows.len();
        out.extend(rows);
        let more = cmds
            .iter()
            .any(|c| c["settings"]["hasNextPage"] == Value::Bool(true));
        if n == 0 || !more {
            break;
        }
    }
    Ok(out)
}

/// EUR amount from a price field; prefers the promo price when present.
fn eur_price(field: Option<ElementRef>) -> Option<f64> {
    let eur = sel(".currency-code-eur");
    let new_amount = sel(".price-new-amount .price");
    let any_price = sel(".price");
    let amount = sel(".price-amount");
    let cent = sel(".price-cent");
    let block = field?.select(&eur).next()?;
    let price = block
        .select(&new_amount)
        .next()
        .or_else(|| block.select(&any_price).next())?;
    let whole = price
        .select(&amount)
        .next()
        .map(text)?
        .replace([',', ' '], "");
    let cents = price
        .select(&cent)
        .next()
        .map(text)
        .unwrap_or_else(|| "0".into());
    format!("{whole}.{cents}").parse().ok()
}

fn parse(html: &str) -> Vec<Offer> {
    let doc = Html::parse_fragment(html);
    let row = sel("div.results-tr.dedicated-server[data-pid]");
    doc.select(&row).filter_map(row_offer).collect()
}

/// First element matching `css` inside `r`.
fn first<'a>(r: ElementRef<'a>, css: &str) -> Option<ElementRef<'a>> {
    r.select(&sel(css)).next()
}

/// "1× 240 GB (SSD SATA) 6241 or" -> "1x 240 GB (SSD SATA)". Several drive
/// divs are alternatives ("or"); only the first is the base config.
fn base_drive(r: ElementRef) -> String {
    static DRIVE: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"(\d+)x\s*([\d.,]+)\s*(GB|TB)\s*\(([^)]*)\)").expect("static regex is valid")
    });
    let first_drive = first(r, ".field--drives > div")
        .map(text)
        .unwrap_or_default()
        .replace('×', "x");
    DRIVE
        .captures(&first_drive)
        .map(|c| format!("{}x {} {} ({})", &c[1], &c[2], &c[3], &c[4]))
        .unwrap_or(first_drive)
}

fn row_offer(r: ElementRef) -> Option<Offer> {
    static CT: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"(\d+)c/(\d+)t").expect("static regex is valid"));
    static AMOUNT: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"cpu-amount-(\d+)").expect("static regex is valid"));
    let pid = r.value().attr("data-pid").unwrap_or_default();
    let meta: Value = first(r, ".conf-data")
        .and_then(|c| c.value().attr("data-analytics"))
        .and_then(|j| serde_json::from_str(j).ok())
        .unwrap_or(Value::Null);
    let price = eur_price(first(r, ".field--price"))?;
    let count = first(r, ".res-cpu")
        .and_then(|c| c.value().attr("class"))
        .and_then(|c| AMOUNT.captures(c).and_then(|m| m[1].parse().ok()))
        .unwrap_or(1u32);
    let ct = first(r, ".field-cpu-core").map(text).unwrap_or_default();
    let disk = base_drive(r);
    let bw = first(r, ".field--bw-speed").map(text).unwrap_or_default();
    let limit = first(r, ".res-bandwidth")
        .and_then(|b| b.value().attr("data-bw-limit"))
        .and_then(first_num);
    let stock = first(r, ".res-stock a[data-tooltip]")
        .and_then(|a| a.value().attr("data-tooltip"))
        .unwrap_or_default()
        .to_string();

    let mut o = Offer::new("oneprovider", pid);
    o.cpu = meta["cpu"]["model"]
        .as_str()
        .map(|m| format!("{} {m}", meta["cpu"]["maker"].as_str().unwrap_or_default()))
        .or_else(|| first(r, ".field-cpu-name").map(text))
        .unwrap_or_default();
    let ram = first(r, ".field--base-ram-amount")
        .map(text)
        .unwrap_or_default();
    o.name = format!("{} / {ram}", o.cpu);
    o.cpu_count = count;
    if let Some(c) = CT.captures(&ct) {
        o.cores = c[1].parse::<u32>().ok().map(|x| x * count);
        o.threads = c[2].parse::<u32>().ok().map(|x| x * count);
    }
    o.ram_gb = meta["ram"].as_f64().map(|mb| mb / 1024.0);
    o.ecc = first(r, ".field--ram-type").map(|t| text(t).contains("ECC"));
    (o.storage_gb, o.nvme) = parse_storage(&disk);
    o.storage = disk;
    o.port_mbps = first_num(&bw).map(|g| if bw.contains("Gbps") { g * 1000.0 } else { g } as u32);
    o.traffic = limit.map(|l| {
        if l == 0.0 {
            "unmetered".into()
        } else {
            format!("{} TB", l / 1000.0)
        }
    });
    o.location = meta["location"]["name"]
        .as_str()
        .unwrap_or_default()
        .to_string();
    o.country = meta["location"]["code"]
        .as_str()
        .unwrap_or_default()
        .to_uppercase();
    o.price_monthly = price;
    o.setup_fee = eur_price(first(r, ".field--setup-fee")).unwrap_or(0.0);
    o.stock = match stock.as_str() {
        "Sold Out" => Stock::OutOfStock,
        "" => Stock::Unknown,
        _ => Stock::InStock,
    };
    o.availability = Some(stock).filter(|s| !s.is_empty());
    o.spec_text = text(r);
    o.url = format!("https://oneprovider.com/en/configure/dediconf/{pid}");
    Some(o)
}
