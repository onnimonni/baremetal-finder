//! MEVSPACE (Poland): the "Server Finder" XHR endpoint returns JSON with an
//! HTML fragment of 10 configurations per page, including availability.

use std::sync::LazyLock;

use anyhow::Result;
use regex::Regex;
use scraper::{ElementRef, Html, Selector};

use crate::model::{Offer, Stock};
use crate::util::{clean, first_num, get_json, parse_storage};

const BASE: &str = "https://mevspace.com";
const FINDER: &str = "https://mevspace.com/dedicated-servers/render-configurations?min_price=0&max_price=100000&min_cores=1&max_cores=1024&min_threads=1&max_threads=2048&min_frequency=0&max_frequency=10&min_ram=1&max_ram=100000&min_storage_capacity=0&max_storage_capacity=1000000&sort=ascending&internet=1";

fn sel(s: &str) -> Selector {
    Selector::parse(s).expect("static CSS selector is valid")
}

fn text(e: ElementRef) -> String {
    clean(&e.text().collect::<String>())
}

pub async fn fetch(client: &reqwest::Client) -> Result<Vec<Offer>> {
    let mut out = Vec::new();
    let mut offset = 0;
    loop {
        let v = get_json(client, &format!("{FINDER}&offset={offset}")).await?;
        let total = v["totalCount"].as_u64().unwrap_or(0) as usize;
        let html = v["html"].as_str().unwrap_or_default();
        let page = parse(html);
        let n = page.len();
        out.extend(page);
        offset += 10;
        if n == 0 || offset >= total {
            break;
        }
    }
    Ok(out)
}

fn parse(html: &str) -> Vec<Offer> {
    static PARAMS: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"(\d+)c/(\d+)t").expect("static regex is valid"));
    let doc = Html::parse_fragment(html);
    let item = sel(".configuration-item");
    let cpu_sel = sel(".cpu-name");
    let bench_sel = sel(".cpu-content > .font-bold");
    let avail_sel = sel(".availability span:last-child");
    let dot_sel = sel(".availability .dot");
    let spec_sel = sel(".specs-section > div");
    let price_sel = sel(".price .value");
    let link_sel = sel("a[href*='/configure/']");

    let mut out = Vec::new();
    for it in doc.select(&item) {
        let Some(cpu) = it.select(&cpu_sel).next().map(text) else {
            continue;
        };
        let specs: Vec<String> = it.select(&spec_sel).map(text).collect();
        let spec = |label: &str| {
            specs
                .chunks(2)
                .find(|kv| kv[0].starts_with(label))
                .and_then(|kv| kv.get(1).cloned())
                .unwrap_or_default()
        };
        let params = spec("CPU Params");
        let drives = spec("Drives");
        let avail = it.select(&avail_sel).next().map(text).unwrap_or_default();
        let dot = it
            .select(&dot_sel)
            .next()
            .and_then(|d| d.value().classes().find(|c| *c != "dot"))
            .unwrap_or_default();
        let href = it
            .select(&link_sel)
            .next()
            .and_then(|a| a.value().attr("href"));
        let Some(price) = it
            .select(&price_sel)
            .next()
            .map(text)
            .and_then(|p| first_num(&p))
        else {
            continue;
        };

        let id = href
            .and_then(|h| h.split("/configure/").nth(1))
            .unwrap_or("?")
            .to_string();
        let mut o = Offer::new("mevspace", format!("{id}-{cpu}-{params}-{drives}-{price}"));
        o.name = cpu.clone();
        o.cpu = cpu;
        if let Some(c) = PARAMS.captures(&params) {
            o.cores = c[1].parse().ok();
            o.threads = c[2].parse().ok();
        }
        o.provider_cpu_mark = it
            .select(&bench_sel)
            .map(text)
            .find(|t| t.contains("Benchmark"))
            .and_then(|t| first_num(&t));
        o.ram_gb = first_num(&spec("RAM"));
        (o.storage_gb, o.nvme) = parse_storage(&drives);
        o.storage = drives;
        o.port_mbps = first_num(&spec("Network")).map(|g| (g * 1000.0) as u32);
        o.traffic = Some("unmetered".into());
        o.location = "Wyszków".into();
        o.country = "PL".into();
        o.price_monthly = price;
        o.stock = match dot {
            "green" => Stock::InStock,
            "red" => Stock::OutOfStock,
            _ => Stock::Unknown,
        };
        o.availability = Some(avail);
        o.spec_text = specs.join(" | ");
        o.url = href.map_or_else(
            || format!("{BASE}/dedicated-servers/server-finder"),
            |h| format!("{BASE}{h}"),
        );
        out.push(o);
    }
    out
}
