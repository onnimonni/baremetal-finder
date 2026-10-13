//! Interserver (US): "buy now" marketplace of physical in-stock servers.
//! Every listed row is one available unit.

use anyhow::Result;
use scraper::{ElementRef, Html, Selector};

use crate::model::{Offer, Stock};
use crate::util::{clean, first_num, get_text, parse_storage};

const URL: &str = "https://www.interserver.net/dedicated/buy-now-servers.html";

fn sel(s: &str) -> Selector {
    Selector::parse(s).expect("static CSS selector is valid")
}

fn text(e: ElementRef) -> String {
    clean(&e.text().collect::<String>())
}

pub async fn fetch(client: &reqwest::Client) -> Result<Vec<Offer>> {
    let html = get_text(client, URL).await?;
    let doc = Html::parse_document(&html);
    let row = sel("tr[data-price]");
    let cpu_s = sel(".cpu-details h6");
    let spec_s = sel(".cpu-specs");
    let storage_s = sel(".storage-info");
    let net_s = sel(".network-info");
    let link_s = sel("a[href*='server=']");
    let mut out = Vec::new();
    for (i, r) in doc.select(&row).enumerate() {
        let a = |k: &str| r.value().attr(k).unwrap_or_default();
        let get = |s: &Selector| r.select(s).next().map(text).unwrap_or_default();
        let Some(price) = first_num(a("data-price")) else {
            continue;
        };
        let href = r
            .select(&link_s)
            .next()
            .and_then(|l| l.value().attr("href"));
        let id = href.and_then(|h| h.split("server=").nth(1)).map_or_else(
            || i.to_string(),
            |s| s.split('&').next().unwrap_or(s).to_string(),
        );
        // "2 CPU, 28 cores, 2.7 GHz"
        let spec = get(&spec_s);
        let count = spec.split(',').next().and_then(first_num).unwrap_or(1.0) as u32;
        let storage = get(&storage_s);
        let net = get(&net_s);

        let mut o = Offer::new("interserver", id);
        o.cpu = get(&cpu_s);
        o.name = format!("{} / {}", o.cpu, a("data-ram"));
        o.cpu_count = count;
        o.cores = first_num(a("data-cores")).map(|c| c as u32);
        o.ram_gb = first_num(a("data-ram"));
        (o.storage_gb, o.nvme) = parse_storage(&storage);
        o.storage = storage;
        o.port_mbps =
            first_num(&net).map(|g| if net.contains("Gbps") { g * 1000.0 } else { g } as u32);
        o.traffic = net.contains("Unmetered").then(|| "unmetered".into());
        o.location = a("data-location").to_string();
        o.country = "US".into();
        o.price_monthly = price;
        o.currency = "USD";
        o.stock = Stock::InStock;
        o.stock_qty = Some(1);
        o.spec_text = text(r);
        o.url = href.map_or_else(
            || URL.to_string(),
            |h| {
                if h.starts_with("http") {
                    h.to_string()
                } else {
                    format!("https://www.interserver.net{h}")
                }
            },
        );
        out.push(o);
    }
    Ok(out)
}
