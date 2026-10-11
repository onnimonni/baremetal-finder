//! Psychz: the order page returns its server table as an HTML fragment
//! when asked with `X-Dedicated-Shell-Data: 1`. Availability per row.

use std::sync::LazyLock;

use anyhow::Result;
use regex::Regex;
use scraper::{ElementRef, Html, Selector};

use crate::geo::iso_from_name;
use crate::model::{Offer, Stock};
use crate::util::{clean, first_num, parse_storage};

const URL: &str = "https://www.psychz.net/dashboard/client/web/order/dedicated-server";

fn sel(s: &str) -> Selector {
    Selector::parse(s).unwrap()
}

fn text(e: ElementRef) -> String {
    clean(&e.text().collect::<String>())
}

pub async fn fetch(client: &reqwest::Client) -> Result<Vec<Offer>> {
    static RENEW: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"Renews at ([\d.,]+)").unwrap());
    let html = client
        .get(URL)
        .header("X-Dedicated-Shell-Data", "1")
        .send()
        .await?
        .error_for_status()?
        .text()
        .await?;
    let doc = Html::parse_fragment(&html);
    let row = sel("tr[data-key]");
    let td = sel("td");
    let usd = sel("span.usdprice");
    let link = sel("a[href*='/order/plan/']");
    let mut out = Vec::new();
    for r in doc.select(&row) {
        let cells: Vec<ElementRef> = r.select(&td).collect();
        let c = |i: usize| cells.get(i).map(|e| text(*e)).unwrap_or_default();
        if cells.len() < 10 {
            continue;
        }
        let price_cell = c(9);
        let first_month = r
            .select(&usd)
            .next()
            .and_then(|s| s.value().attr("data-price"))
            .and_then(first_num);
        let Some(monthly) = RENEW
            .captures(&price_cell)
            .and_then(|m| first_num(&m[1]))
            .or(first_month)
        else {
            continue;
        };
        let loc = c(7);
        let (city, country) = loc.rsplit_once(", ").unwrap_or((&loc, ""));
        let country = match country {
            "USA" => "US",
            "NL" => "NL",
            "UK" => "GB",
            other => iso_from_name(other).unwrap_or_default(),
        };
        let href = r.select(&link).next().and_then(|a| a.value().attr("href"));
        let plan = href
            .and_then(|h| h.split("/order/plan/").nth(1))
            .and_then(|h| h.split('?').next())
            .unwrap_or_default();
        let storage = c(4);

        let mut o = Offer::new(
            "psychz",
            format!(
                "{plan}-{}-{city}",
                r.value().attr("data-key").unwrap_or_default()
            ),
        );
        o.cpu = c(0).split(" - ").next().unwrap_or_default().to_string();
        o.name = format!("{} / {}", o.cpu, c(3));
        o.cores = first_num(&c(2)).map(|x| x as u32);
        o.ram_gb = first_num(&c(3));
        (o.storage_gb, o.nvme) = parse_storage(&storage);
        o.storage = storage;
        let port = c(5);
        o.port_mbps =
            first_num(&port).map(|g| if port.contains("Gbps") { g * 1000.0 } else { g } as u32);
        o.traffic = Some(c(6));
        o.location = city.to_string();
        o.country = country.to_string();
        o.price_monthly = monthly;
        o.currency = "USD";
        let available = c(8);
        o.stock = if available.eq_ignore_ascii_case("Available") {
            Stock::InStock
        } else {
            Stock::OutOfStock
        };
        o.availability = Some(available);
        o.url = href.map_or_else(|| URL.to_string(), |h| format!("https://www.psychz.net{h}"));
        o.note = first_month.map(|f| format!("first month ${f}"));
        out.push(o);
    }
    Ok(out)
}
