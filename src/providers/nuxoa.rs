//! NUXOA (Frankfurt): lists only physically available servers, one row per
//! machine, so each row is one unit in stock. Prices excl. VAT.

use std::sync::LazyLock;

use anyhow::Result;
use regex::Regex;
use scraper::{ElementRef, Html, Selector};

use crate::model::{Offer, Stock};
use crate::util::{clean, first_num, get_text, parse_storage};

const PAGES: &[(&str, &str)] = &[
    ("https://manager.nuxoa.de/en/dedicated", "firstcolo FRA4"),
    (
        "https://manager.nuxoa.de/en/dedicated-premium",
        "maincubes FRA01",
    ),
];

fn sel(s: &str) -> Selector {
    Selector::parse(s).expect("static CSS selector is valid")
}

fn text(e: ElementRef) -> String {
    clean(&e.text().collect::<String>().replace('\u{a0}', " "))
}

pub async fn fetch(client: &reqwest::Client) -> Result<Vec<Offer>> {
    static CT: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"(\d+)C\s*/\s*(\d+)T").expect("static regex is valid"));
    let item = sel("a.dedi-item");
    let col = |c: &str| sel(&format!("[data-col={c}]"));
    let (server_s, cpu_s, name_s, ram_s, disk_s, net_s, price_s) = (
        col("server"),
        col("cpu"),
        sel(".dedi-cpu-name"),
        col("ram"),
        col("storage"),
        col("network"),
        col("price"),
    );
    let mut out = Vec::new();
    for (url, dc) in PAGES {
        let html = get_text(client, url).await?;
        let doc = Html::parse_document(&html);
        for it in doc.select(&item) {
            let get = |s: &Selector| it.select(s).next().map(text).unwrap_or_default();
            let Some(price) = first_num(&get(&price_s)) else {
                continue;
            };
            let cpu_all = get(&cpu_s);
            let disk = get(&disk_s);
            let net = get(&net_s);
            let href = it.value().attr("href").unwrap_or(url);

            let mut o = Offer::new("nuxoa", href.rsplit('/').next().unwrap_or(href));
            o.name = get(&server_s);
            o.cpu = get(&name_s);
            if let Some(c) = CT.captures(&cpu_all) {
                o.cores = c[1].parse().ok();
                o.threads = c[2].parse().ok();
            }
            o.ram_gb = first_num(&get(&ram_s));
            (o.storage_gb, o.nvme) = parse_storage(&disk);
            o.storage = disk;
            o.port_mbps =
                first_num(&net).map(|g| if net.contains("Gbit") { g * 1000.0 } else { g } as u32);
            o.location = (*dc).to_string();
            o.country = "DE".into();
            o.price_monthly = price;
            o.stock = Stock::InStock;
            o.stock_qty = Some(1);
            o.availability = Some("immediately".into());
            o.url = href.to_string();
            o.note = Some("used unit, business customers only".into());
            out.push(o);
        }
    }
    Ok(out)
}
