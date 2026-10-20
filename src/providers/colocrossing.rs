//! ColoCrossing (US + Dublin): dedicated server table. A "Customize" order
//! link means orderable, "CONTACT TO ORDER" means not in stock.

use anyhow::Result;
use scraper::{ElementRef, Html, Selector};

use crate::model::{Offer, Stock};
use crate::util::{clean, first_num, get_text, parse_storage};

const URL: &str = "https://www.colocrossing.com/server/dedicated-servers/";

fn sel(s: &str) -> Selector {
    Selector::parse(s).expect("static CSS selector is valid")
}

fn text(e: ElementRef) -> String {
    clean(&e.text().collect::<Vec<_>>().join(" "))
}

pub async fn fetch(client: &reqwest::Client) -> Result<Vec<Offer>> {
    let html = get_text(client, URL).await?;
    let doc = Html::parse_document(&html);
    let hidden = sel("span[style*='display:none']");
    let mut out = Vec::new();
    for row in doc.select(&sel("div.cstm-row.cstm-data")) {
        let cell = |c: &str| {
            row.select(&sel(&format!(".cstm-cell.{c}")))
                .next()
                .map(text)
                .unwrap_or_default()
        };
        let Some(price) = first_num(&cell("price")) else {
            continue;
        };
        let title = row.select(&sel(".cstm_plan_title")).next();
        let tier = title
            .and_then(|t| t.select(&hidden).next())
            .map(text)
            .unwrap_or_default();
        let cpu = title
            .map(text)
            .unwrap_or_default()
            .trim_start_matches(tier.as_str())
            .trim()
            .to_string();
        // "Core(s) 4 , Thread(s) 8 , Frequency …"
        let feats = row
            .select(&sel(".cpu-features"))
            .next()
            .map(text)
            .unwrap_or_default();
        let order = row
            .select(&sel("a[onclick*='order/service']"))
            .next()
            .and_then(|a| a.value().attr("onclick"))
            .and_then(|js| js.split('\'').nth(1))
            .map(String::from);
        let disk = cell("storage");
        let port = cell("port-speed");
        let id = order
            .as_deref()
            .and_then(|u| u.rsplit('/').next())
            .map_or_else(|| format!("{tier}-{cpu}"), String::from);
        let (count, cpu) = match cpu
            .strip_prefix("Dual ")
            .or_else(|| cpu.strip_prefix("2x "))
        {
            Some(c) => (2, c.to_string()),
            None => (1, cpu),
        };
        let mut o = Offer::new("colocrossing", id);
        o.name = format!("{tier} {cpu}").trim().to_string();
        o.cpu = cpu;
        o.cpu_count = count;
        o.cores = feats
            .split("Core(s)")
            .nth(1)
            .and_then(first_num)
            .map(|c| c as u32);
        o.threads = feats
            .split("Thread(s)")
            .nth(1)
            .and_then(first_num)
            .map(|c| c as u32);
        o.ram_gb = first_num(&cell("ram"));
        (o.storage_gb, o.nvme) = parse_storage(&disk);
        o.storage = disk;
        o.port_mbps =
            first_num(&port).map(|n| if port.contains("Gbps") { n * 1000.0 } else { n } as u32);
        o.traffic = Some(cell("bandwidth"));
        o.location = "US / Dublin".into();
        o.country = "US".into();
        o.price_monthly = price;
        o.currency = "USD";
        o.stock = if order.is_some() {
            Stock::InStock
        } else {
            Stock::OutOfStock
        };
        o.provider_cpu_mark = first_num(&cell("cpu-score")).filter(|m| *m > 0.0);
        o.spec_text = cell("ram");
        o.url = order.unwrap_or_else(|| URL.to_string());
        out.push(o);
    }
    anyhow::ensure!(!out.is_empty(), "colocrossing: no rows");
    Ok(out)
}
