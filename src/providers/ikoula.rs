//! Ikoula (France): Drupal range pages + per-product shop page, which says
//! "momentanément indisponible" when sold out. Prices excl. VAT ("HT").

use anyhow::Result;
use scraper::{ElementRef, Html, Selector};

use crate::model::{Offer, Stock};
use crate::util::{clean, first_num, get_text, parse_storage};

const RANGES: &[&str] = &["agile", "classic", "power", "xtreme", "regen"];

fn sel(s: &str) -> Selector {
    Selector::parse(s).unwrap()
}

fn text(e: ElementRef) -> String {
    clean(&e.text().collect::<String>())
}

pub async fn fetch(client: &reqwest::Client) -> Result<Vec<Offer>> {
    let art = sel("article.iknewsd");
    let title_s = sel(".iknewsdtitre strong");
    let price_s = sel(".ikprice strong.iksdmensuel");
    let li_s = sel("ul.iknewsdcara > li");
    let icon_s = sel("i[title]");
    let strong_s = sel("strong");
    let tip_s = sel("a.iktooltip");
    let link_s = sel(".ikcommande a[href]");
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for range in RANGES {
        let html = match get_text(
            client,
            &format!("https://www.ikoula.com/fr/serveur-dedie/{range}"),
        )
        .await
        {
            Ok(h) => h,
            Err(e) => {
                eprintln!("ikoula {range}: {e:#}");
                continue;
            }
        };
        let products: Vec<Offer> = {
            let doc = Html::parse_document(&html);
            doc.select(&art)
                .filter_map(|a| {
                    let href = a.select(&link_s).next()?.value().attr("href")?.to_string();
                    let price = a
                        .select(&price_s)
                        .next()
                        .map(text)
                        .and_then(|t| first_num(&t))?;
                    let field = |title: &str| {
                        a.select(&li_s).find(|li| {
                            li.select(&icon_s)
                                .any(|i| i.value().attr("title") == Some(title))
                        })
                    };
                    let strong = |title: &str| {
                        field(title)
                            .and_then(|li| li.select(&strong_s).next().map(text))
                            .unwrap_or_default()
                    };
                    let full = |title: &str| field(title).map(text).unwrap_or_default();
                    let tip = a
                        .select(&tip_s)
                        .next()
                        .and_then(|t| t.value().attr("data-bs-original-title"))
                        .map(|t| {
                            clean(
                                &Html::parse_fragment(t)
                                    .root_element()
                                    .text()
                                    .collect::<Vec<_>>()
                                    .join(" "),
                            )
                        })
                        .unwrap_or_default();
                    let disk = full("Stockage").replace(" To", " TB").replace(" Go", " GB");
                    let bw = strong("Bande passante");

                    let mut o = Offer::new("ikoula", href.clone());
                    o.name = a.select(&title_s).next().map(text).unwrap_or_default();
                    o.cpu = a.select(&tip_s).next().map(text).unwrap_or_default();
                    o.cores = tip
                        .split("coeurs :")
                        .nth(1)
                        .and_then(first_num)
                        .map(|c| c as u32);
                    o.threads = tip
                        .split("threads :")
                        .nth(1)
                        .and_then(first_num)
                        .map(|t| t as u32);
                    o.ram_gb = first_num(&strong("Mémoire vive"));
                    (o.storage_gb, o.nvme) = parse_storage(&disk);
                    o.storage = disk;
                    o.port_mbps = first_num(&bw)
                        .map(|g| if bw.contains("Gbps") { g * 1000.0 } else { g } as u32);
                    o.location = "France".into();
                    o.country = "FR".into();
                    o.price_monthly = price;
                    o.url = href;
                    Some(o)
                })
                .collect()
        };
        for mut o in products {
            if !seen.insert(o.url.clone()) {
                continue;
            }
            o.stock = match get_text(client, &o.url).await {
                Ok(page) if page.contains("momentanément indisponible") => Stock::OutOfStock,
                Ok(page) if page.contains("outOfLimit") => Stock::InStock,
                _ => Stock::Unknown,
            };
            out.push(o);
        }
    }
    Ok(out)
}
