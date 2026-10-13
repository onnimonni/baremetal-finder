//! Shock Hosting (US, EU, APAC, BR, ZA): plans from the dedicated page, stock
//! quantity per plan from the WHMCS product feed (`0` = sold out).

use anyhow::Result;
use futures::future::join_all;
use scraper::{ElementRef, Html, Selector};

use crate::model::{Offer, Stock};
use crate::util::{clean, first_num, get_text, parse_storage};

const PAGE: &str = "https://www.shockhosting.net/dedicated";
const FEED: &str = "https://www.shockhosting.com/portal/feeds/productsinfo2.php?get=stock&pid=";

fn sel(s: &str) -> Selector {
    Selector::parse(s).expect("static CSS selector is valid")
}

fn text(e: ElementRef) -> String {
    clean(&e.text().collect::<Vec<_>>().join(" "))
}

/// Tab id prefix ("ny-servers") -> (country, location)
fn place(tab: &str) -> (&'static str, &'static str) {
    match tab.trim_end_matches("-servers") {
        "ny" => ("US", "New York"),
        "fl" => ("US", "Miami"),
        "tx" => ("US", "Dallas"),
        "la" => ("US", "Los Angeles"),
        "wa" => ("US", "Seattle"),
        "co" => ("US", "Denver"),
        "de" => ("DE", "Germany"),
        "nl" => ("NL", "Netherlands"),
        "uk" => ("GB", "United Kingdom"),
        "fr" => ("FR", "France"),
        "ro" => ("RO", "Romania"),
        "il" => ("IL", "Israel"),
        "za" => ("ZA", "South Africa"),
        "jp" => ("JP", "Japan"),
        "au" => ("AU", "Australia"),
        "sg" => ("SG", "Singapore"),
        "br" => ("BR", "Brazil"),
        _ => ("", ""),
    }
}

pub async fn fetch(client: &reqwest::Client) -> Result<Vec<Offer>> {
    let html = get_text(client, PAGE).await?;
    let mut plans = Vec::new();
    {
        let doc = Html::parse_document(&html);
        for p in doc.select(&sel("div.server-plan")) {
            let Some(pid) = p
                .value()
                .classes()
                .find_map(|c| c.strip_prefix("pid"))
                .map(String::from)
            else {
                continue;
            };
            let tab = p
                .ancestors()
                .filter_map(ElementRef::wrap)
                .find_map(|a| a.value().id().filter(|i| i.ends_with("-servers")))
                .unwrap_or_default()
                .to_string();
            let spec = |n: u8| {
                p.select(&sel(&format!("#spec-{n}")))
                    .next()
                    .map(text)
                    .unwrap_or_default()
            };
            let strong = |n: u8| {
                p.select(&sel(&format!("#spec-{n} strong")))
                    .next()
                    .map(text)
                    .unwrap_or_default()
            };
            let name = p
                .select(&sel("#server-model h3"))
                .next()
                .map(text)
                .unwrap_or_default();
            plans.push((
                pid,
                tab,
                name,
                strong(1),
                spec(1),
                spec(2),
                spec(3),
                spec(4),
                strong(5),
            ));
        }
    }
    let urls: Vec<String> = plans.iter().map(|p| format!("{FEED}{}", p.0)).collect();
    let stock = join_all(urls.iter().map(|u| get_text(client, u))).await;
    let mut out = Vec::new();
    for ((pid, tab, name, cpu, cpu_spec, ram, disk, net, price), qty) in
        plans.into_iter().zip(stock)
    {
        let Some(price) = first_num(&price) else {
            continue;
        };
        let qty = qty.ok().and_then(|t| t.trim().parse::<i64>().ok());
        let (country, location) = place(&tab);
        let disk = disk.trim_end_matches(" Storage").replace('×', "x");
        let mut o = Offer::new("shockhosting", pid.clone());
        o.name = name;
        o.cpu = cpu;
        // "Intel Xeon E3-1270 v3 3.5 GHz, 4 cores"
        o.cores = cpu_spec
            .split(", ")
            .nth(1)
            .and_then(first_num)
            .map(|c| c as u32);
        o.ram_gb = first_num(&ram);
        (o.storage_gb, o.nvme) = parse_storage(&disk);
        o.storage = disk;
        o.port_mbps = net
            .split("Bandwidth")
            .nth(1)
            .and_then(first_num)
            .map(|g| if net.contains("Gbps") { g * 1000.0 } else { g } as u32);
        o.traffic = net.split(" Bandwidth").next().map(String::from);
        o.location = location.to_string();
        o.country = country.to_string();
        o.price_monthly = price;
        o.currency = "USD";
        o.stock_qty = qty;
        o.stock = match qty {
            Some(n) if n > 0 => Stock::InStock,
            Some(_) => Stock::OutOfStock,
            None => Stock::Unknown,
        };
        o.spec_text = ram;
        o.url = format!("https://shockhosting.com/portal/cart.php?a=add&pid={pid}");
        out.push(o);
    }
    Ok(out)
}
