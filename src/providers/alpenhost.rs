//! Alpenhost (Vienna): WHMCS store. Group pages show the cheapest billing
//! cycle (often 3 years!) so each product's order page is opened to read the
//! real month-to-month price. Stock comes from WHMCS "N Verfügbar" counters
//! and disabled order buttons.

use std::sync::LazyLock;

use anyhow::Result;
use regex::Regex;
use scraper::{ElementRef, Html, Node, Selector};

use crate::model::{Offer, Stock};
use crate::util::{clean, first_num, get_text, parse_storage};

const BASE: &str = "https://alpenhost.at";
/// Bare-metal product groups (KVM/vServer groups are skipped).
const GROUPS: &[&str] = &[
    "dedicated-server-amd",
    "dedizierte-server-amd",
    "dedicated-server-intel",
    "eco-alpenserver",
];

fn sel(s: &str) -> Selector {
    Selector::parse(s).unwrap()
}

fn text(e: ElementRef) -> String {
    clean(&e.text().collect::<String>())
}

struct Listing {
    offer: Offer,
    order_href: Option<String>,
}

pub async fn fetch(client: &reqwest::Client) -> Result<Vec<Offer>> {
    let mut out = Vec::new();
    for g in GROUPS {
        let html = match get_text(client, &format!("{BASE}/Kundencenter/store/{g}")).await {
            Ok(h) => h,
            Err(e) => {
                eprintln!("alpenhost {g}: {e:#}");
                continue;
            }
        };
        for mut l in parse_group(&html, g) {
            if let Some(href) = &l.order_href {
                match monthly_price(client, href).await {
                    Ok(Some((m, s))) => {
                        l.offer.price_monthly = m;
                        l.offer.setup_fee = s;
                    }
                    Ok(None) => l.offer.note = Some("listed price may be a long-term cycle".into()),
                    Err(e) => eprintln!("alpenhost {href}: {e:#}"),
                }
            }
            out.push(l.offer);
        }
    }
    Ok(out)
}

fn parse_group(html: &str, group: &str) -> Vec<Listing> {
    let doc = Html::parse_document(html);
    let pkg = sel(".package");
    let title_sel = sel(".package-title");
    let price_sel = sel(".price-amount");
    let cycle_sel = sel(".price-cycle");
    let qty_sel = sel(".package-qty");
    let order_sel = sel("a.btn-order-now");
    let content_sel = sel(".package-content");

    let mut out = Vec::new();
    for p in doc.select(&pkg) {
        let Some(title) = p.select(&title_sel).next().map(text) else {
            continue;
        };
        // Spec blocks vary ("<strong>CPU:</strong> x<br>", "CPU : x<br>", lists),
        // so flatten to lines and split each on the first ':'.
        let specs: Vec<(String, String)> = p
            .select(&content_sel)
            .flat_map(|c| lines(c))
            .filter_map(|l| {
                let (k, v) = l.split_once(':')?;
                Some((clean(k), clean(v)))
            })
            .collect();
        let spec = |labels: &[&str]| {
            specs
                .iter()
                .find(|(l, _)| labels.iter().any(|x| l.eq_ignore_ascii_case(x)))
                .map(|(_, v)| v.clone())
                .unwrap_or_default()
        };
        let Some(price) = p
            .select(&price_sel)
            .next()
            .map(text)
            .and_then(|t| first_num(&t))
        else {
            continue;
        };
        let setup = p
            .select(&cycle_sel)
            .next()
            .map(text)
            .and_then(|t| t.split('+').nth(1).and_then(first_num))
            .unwrap_or(0.0);
        let qty = p
            .select(&qty_sel)
            .next()
            .map(text)
            .and_then(|t| first_num(&t))
            .map(|q| q as i64);
        let order = p.select(&order_sel).next();
        let disabled = order.is_some_and(|a| a.value().classes().any(|c| c == "disabled"));
        let order_href = order
            .and_then(|a| a.value().attr("href"))
            .map(|h| format!("{BASE}{}", h.split('?').next().unwrap_or(h)));

        let (cpu_count, cpu) = split_count(&cpu_model(&spec(&["CPU"])));
        let storage = spec(&["Festplatten", "Speicher", "Storage"]);
        let storage = storage.replace("SDD", "SSD");
        let mut o = Offer::new("alpenhost", format!("{group}/{title}"));
        o.name = title.clone();
        o.cpu = if cpu.is_empty() { title } else { cpu };
        o.cpu_count = cpu_count;
        o.cores = first_num(&spec(&[
            "CPU-Kerne",
            "CPU Kerne",
            "CPU-Cores",
            "Cores / Threads",
        ]))
        .map(|c| c as u32);
        let ram = spec(&["RAM", "Memory"]);
        // "2x 8GB DDR4" -> 16
        o.ram_gb = parse_storage(&ram).0;
        o.ecc = Some(ram.contains("ECC"));
        (o.storage_gb, o.nvme) = parse_storage(&storage);
        o.storage = storage;
        let uplink = spec(&["Uplink-Anschluss", "Uplink Anschluss", "Uplink"]);
        o.port_mbps = first_num(&uplink).map(|g| if uplink.contains("Gbit") || uplink.contains("GB") { g * 1000.0 } else { g } as u32);
        o.traffic = Some(spec(&["Traffic"])).filter(|t| !t.is_empty());
        o.location = "Vienna".into();
        o.country = "AT".into();
        o.price_monthly = price;
        o.setup_fee = setup;
        o.stock_qty = qty;
        o.stock = match (qty, disabled) {
            (Some(q), _) if q > 0 => Stock::InStock,
            (Some(_), _) | (None, true) => Stock::OutOfStock,
            (None, false) => Stock::Unknown,
        };
        o.url = order_href
            .clone()
            .unwrap_or_else(|| format!("{BASE}/Kundencenter/store/{group}"));
        out.push(Listing {
            offer: o,
            order_href: order_href.filter(|_| !disabled),
        });
    }
    out
}

/// Text lines of an element, treating <br> and block elements as breaks.
fn lines(e: ElementRef) -> Vec<String> {
    let mut buf = String::new();
    for n in e.descendants() {
        match n.value() {
            Node::Text(t) => buf.push_str(t),
            Node::Element(el) if matches!(el.name(), "br" | "p" | "li" | "h3" | "h4" | "div") => {
                buf.push('\n')
            }
            _ => {}
        }
    }
    buf.lines().map(clean).filter(|l| !l.is_empty()).collect()
}

/// "1x Intel Xeon E5-2623 v4 4-Kerne Bis zu 3.20 GHz" -> "1x Intel Xeon E5-2623 v4"
fn cpu_model(s: &str) -> String {
    static TAIL: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"(?i)\s+(\d+-Kerne|Bis zu|@).*$").unwrap());
    TAIL.replace(s, "").to_string()
}

/// "1x AMD Ryzen 9 9950X3D" -> (1, "AMD Ryzen 9 9950X3D")
fn split_count(s: &str) -> (u32, String) {
    static COUNT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^(\d+)\s*[x×]\s*(.+)$").unwrap());
    COUNT
        .captures(s)
        .map(|c| (c[1].parse().unwrap_or(1), c[2].to_string()))
        .unwrap_or((1, s.to_string()))
}

/// Open the order page and read the "monthly" billing cycle option:
/// "Monatlich €131,95 EUR/M. + €44,99 EUR Einrichtunsgebühr".
async fn monthly_price(client: &reqwest::Client, href: &str) -> Result<Option<(f64, f64)>> {
    let html = get_text(client, href).await?;
    let doc = Html::parse_document(&html);
    let input = sel("input[name='billingcycle'][value='monthly']");
    let title = sel(".check-title");
    let Some(radio) = doc.select(&input).next() else {
        return Ok(None);
    };
    let Some(label) = radio
        .parent()
        .and_then(ElementRef::wrap)
        .and_then(|p| p.select(&title).next())
        .map(text)
    else {
        return Ok(None);
    };
    let mut parts = label.split('+');
    let monthly = parts.next().and_then(first_num);
    let setup = parts.next().and_then(first_num).unwrap_or(0.0);
    Ok(monthly.map(|m| (m, setup)))
}
