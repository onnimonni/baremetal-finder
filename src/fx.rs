use std::collections::HashMap;

use anyhow::{Context, Result};

use crate::util::get_text;

const ECB_URL: &str = "https://www.ecb.europa.eu/stats/eurofxref/eurofxref-daily.xml";

/// Currency -> units per 1 EUR (ECB reference rates). EUR itself is 1.0.
pub async fn ecb_rates(client: &reqwest::Client) -> Result<HashMap<String, f64>> {
    let xml = get_text(client, ECB_URL).await?;
    let doc = roxmltree::Document::parse(&xml).context("parsing ECB XML")?;
    let mut rates: HashMap<String, f64> = doc
        .descendants()
        .filter_map(|n| {
            let cur = n.attribute("currency")?;
            let rate = n.attribute("rate")?.parse().ok()?;
            Some((cur.to_string(), rate))
        })
        .collect();
    anyhow::ensure!(rates.contains_key("USD"), "ECB feed had no USD rate");
    rates.insert("EUR".into(), 1.0);
    Ok(rates)
}
