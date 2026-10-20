//! Small WHMCS hosts sharing the same store template: QuickPacket, RackNerd,
//! GreenCloud, Servinga, Servarica, Spartan Host, LiteServer. Location comes from the store group (or "US" when the group
//! doesn't say).

use anyhow::Result;

use crate::model::Offer;
use crate::util::{browser, browser_text, get_text, parse_storage};
use crate::{specs, whmcs};

/// (store page, country, location)
type Group = (&'static str, &'static str, &'static str);

const QUICKPACKET: &[Group] = &[
    (
        "https://quickpacket.com/billing/store/dedicated-servers-ashburn",
        "US",
        "Ashburn",
    ),
    (
        "https://quickpacket.com/billing/store/dedicated-servers-chicago",
        "US",
        "Chicago",
    ),
    (
        "https://quickpacket.com/billing/store/dedicated-servers-los-angeles",
        "US",
        "Los Angeles",
    ),
];
const RACKNERD: &[Group] = &[
    (
        "https://my.racknerd.com/index.php?rp=/store/dedicated-servers",
        "US",
        "US",
    ),
    (
        "https://my.racknerd.com/index.php?rp=/store/amd-ryzenepyc-dedicated-servers",
        "US",
        "US",
    ),
    (
        "https://my.racknerd.com/index.php?rp=/store/high-bandwidth-unmetered-dedicated-servers",
        "US",
        "US",
    ),
];
const GREENCLOUD: &[Group] = &[
    (
        "https://greenserver.io/billing/store/clearance-servers",
        "",
        "",
    ),
    (
        "https://greenserver.io/billing/store/dual-processors",
        "US",
        "US",
    ),
    (
        "https://greenserver.io/billing/store/high-end-us-servers",
        "US",
        "US",
    ),
];

const SERVINGA: &[Group] = &[(
    "https://metal.servinga.com/index.php?rp=/store/dedicated-server",
    "NL",
    "Rotterdam",
)];

const SERVARICA: &[Group] = &[
    (
        "https://clients.servarica.com/store/dedicated-servers",
        "CA",
        "Montreal",
    ),
    (
        "https://clients.servarica.com/store/bf-2025-dedicated-servers",
        "CA",
        "Montreal",
    ),
];
const SPARTANHOST: &[Group] = &[
    (
        "https://billing.spartanhost.net/store/dallas-rapid-deploy-dedicated-servers",
        "US",
        "Dallas",
    ),
    (
        "https://billing.spartanhost.net/store/ddos-protected-seattle-dedicated-servers-china-optimised",
        "US",
        "Seattle",
    ),
];
const LITESERVER: &[Group] = &[(
    "https://clients.liteserver.nl/index.php?rp=/store/virtual-dedicated-server",
    "NL",
    "Dronten",
)];

pub async fn quickpacket(client: &reqwest::Client) -> Result<Vec<Offer>> {
    scrape(Http::Plain(client), "quickpacket", QUICKPACKET, "USD").await
}

/// RackNerd's store rejects non-browser TLS.
pub async fn racknerd(_client: &reqwest::Client) -> Result<Vec<Offer>> {
    scrape(Http::Browser(&browser()?), "racknerd", RACKNERD, "USD").await
}

pub async fn greencloud(client: &reqwest::Client) -> Result<Vec<Offer>> {
    scrape(Http::Plain(client), "greencloud", GREENCLOUD, "USD").await
}

pub async fn servarica(client: &reqwest::Client) -> Result<Vec<Offer>> {
    scrape(Http::Plain(client), "servarica", SERVARICA, "USD").await
}

pub async fn spartanhost(client: &reqwest::Client) -> Result<Vec<Offer>> {
    scrape(Http::Plain(client), "spartanhost", SPARTANHOST, "USD").await
}

pub async fn liteserver(client: &reqwest::Client) -> Result<Vec<Offer>> {
    scrape(Http::Plain(client), "liteserver", LITESERVER, "EUR").await
}

/// Prices exclude VAT; sold-out products are hidden from the store.
pub async fn servinga(client: &reqwest::Client) -> Result<Vec<Offer>> {
    scrape(Http::Plain(client), "servinga", SERVINGA, "EUR").await
}

enum Http<'a> {
    Plain(&'a reqwest::Client),
    Browser(&'a wreq::Client),
}

impl Http<'_> {
    async fn text(&self, url: &str) -> Result<String> {
        match self {
            Http::Plain(c) => get_text(c, url).await,
            Http::Browser(c) => browser_text(c, url).await,
        }
    }
}

/// "Coventry, UK" / "Dallas, TX" / "Hong Kong" mentioned in a description.
fn place_from_lines(lines: &[String]) -> Option<(&'static str, &'static str)> {
    let all = lines.join(" ").to_lowercase();
    [
        ("coventry", ("GB", "Coventry")),
        ("london", ("GB", "London")),
        ("amsterdam", ("NL", "Amsterdam")),
        ("frankfurt", ("DE", "Frankfurt")),
        ("rotterdam", ("NL", "Rotterdam")),
        ("hong kong", ("HK", "Hong Kong")),
        ("singapore", ("SG", "Singapore")),
        ("tokyo", ("JP", "Tokyo")),
        ("dallas", ("US", "Dallas")),
        ("chicago", ("US", "Chicago")),
        ("los angeles", ("US", "Los Angeles")),
    ]
    .into_iter()
    .find(|(k, _)| all.contains(k))
    .map(|(_, v)| v)
}

async fn scrape(
    client: Http<'_>,
    provider: &'static str,
    groups: &[Group],
    currency: &'static str,
) -> Result<Vec<Offer>> {
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for (url, country, location) in groups {
        let html = match client.text(url).await {
            Ok(h) => h,
            Err(e) => {
                eprintln!("{provider} {url}: {e:#}");
                continue;
            }
        };
        for p in whmcs::parse_store(&html) {
            let Some(price) = p.price else { continue };
            if !seen.insert(p.id.clone()) {
                continue;
            }
            let desc: Vec<String> = p
                .lines
                .iter()
                .map(|l| l.trim_start_matches(['-', '•', ' ']).to_string())
                .collect();
            let mut s = specs::from_lines(&desc);
            // QuickPacket and Spartan Host name the plan after its CPU.
            if s.cpu.is_empty() {
                let mut lines = vec![p.name.clone()];
                lines.extend(desc.iter().cloned());
                s = specs::from_lines(&lines);
            }
            let (country, location) = place_from_lines(&p.lines).unwrap_or((country, location));
            let mut o = Offer::new(provider, p.id.clone());
            o.name = p.name.clone();
            o.cpu = s.cpu;
            o.cpu_count = s.cpu_count;
            o.cores = s.cores;
            o.threads = s.threads;
            o.ram_gb = s.ram_gb;
            (o.storage_gb, o.nvme) = parse_storage(&s.storage);
            o.storage = s.storage;
            o.port_mbps = s.port_mbps;
            o.location = location.to_string();
            o.country = country.to_string();
            o.price_monthly = price;
            o.setup_fee = p.setup;
            o.currency = currency;
            o.stock = p.stock();
            o.stock_qty = p.qty;
            o.spec_text = p.lines.join(" | ");
            o.url = (*url).to_string();
            out.push(o);
        }
    }
    Ok(out)
}
