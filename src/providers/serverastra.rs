//! ServerAstra (Budapest, New York): WHMCS cart table with `data-*` specs.
//! Each row is one physical machine, so listed means orderable. The price
//! cell reads "€X / mo / y or €Y / mo"; the second figure is month-to-month.

use anyhow::Result;
use scraper::{Html, Selector};

use crate::model::{Offer, Stock};
use crate::util::{clean, first_num, get_text, parse_storage};

const URL: &str = "https://serverastra.com/billing/cart.php?gid=1";

pub async fn fetch(client: &reqwest::Client) -> Result<Vec<Offer>> {
    let html = get_text(client, URL).await?;
    let doc = Html::parse_document(&html);
    let row_sel = Selector::parse("tr[data-cpu]").expect("static CSS selector is valid");
    let link = Selector::parse("a[href*='a=add']").expect("static CSS selector is valid");
    let td = Selector::parse("td").expect("static CSS selector is valid");
    let mut out = Vec::new();
    for row in doc.select(&row_sel) {
        let attr = |k: &str| row.value().attr(k).unwrap_or_default();
        let cells: Vec<String> = row
            .select(&td)
            .map(|c| clean(&c.text().collect::<Vec<_>>().join(" ")))
            .collect();
        let all = cells.join(" | ");
        let monthly = all
            .split(" or ")
            .nth(1)
            .and_then(first_num)
            .or_else(|| all.split('€').nth(1).and_then(first_num));
        let Some(price) = monthly else { continue };
        let setup = all
            .split("Setup")
            .next()
            .and_then(|s| s.rsplit('€').next())
            .filter(|_| all.contains("Setup"))
            .and_then(first_num);
        // "2x Intel Xeon E5-2683v4 3.0GHz"
        let cpu_cell = cells.get(1).cloned().unwrap_or_default();
        let (count, cpu) = match cpu_cell.split_once("x ") {
            Some((n, c)) if n.trim().parse::<u32>().is_ok() => {
                (n.trim().parse().unwrap_or(1), c.to_string())
            }
            _ => (1, cpu_cell.clone()),
        };
        let cpu = cpu
            .split_whitespace()
            .filter(|w| !w.ends_with("GHz"))
            .collect::<Vec<_>>()
            .join(" ");
        let (cores, threads) = attr("data-cpu").split_once('/').unwrap_or_default();
        let disk = attr("data-disk");
        let loc = attr("data-location");
        let href = row
            .select(&link)
            .next()
            .and_then(|a| a.value().attr("href"))
            .unwrap_or_default();
        let mut o = Offer::new(
            "serverastra",
            href.rsplit('=').next().unwrap_or_default().to_string(),
        );
        o.name = cells.first().cloned().unwrap_or_default();
        o.cpu = cpu;
        o.cpu_count = count;
        o.cores = cores.parse().ok();
        o.threads = threads.parse().ok();
        o.ram_gb = first_num(attr("data-ram"));
        o.ecc = Some(attr("data-ram").contains("ECC"));
        (o.storage_gb, o.nvme) = parse_storage(disk);
        o.storage = disk.to_string();
        o.port_mbps = first_num(attr("data-network")).map(|n| if attr("data-network").contains("Gbps") { n * 1000.0 } else { n } as u32);
        o.country = if loc.contains("Hungary") { "HU" } else { "US" }.into();
        o.location = loc.to_string();
        o.price_monthly = price;
        o.setup_fee = setup.unwrap_or(0.0);
        o.stock = Stock::InStock;
        o.spec_text = attr("data-ram").to_string();
        o.url = format!("https://serverastra.com/billing/{href}");
        out.push(o);
    }
    anyhow::ensure!(!out.is_empty(), "serverastra: no rows");
    Ok(out)
}
