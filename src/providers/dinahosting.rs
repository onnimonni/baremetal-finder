//! Dinahosting (ES, Madrid): dedicated machine JSON. `disponibilidad` is
//! "< 24h" when deliverable, "Consultar" (ask) otherwise.

use std::collections::HashSet;
use std::sync::LazyLock;

use anyhow::Result;
use regex::Regex;

use crate::model::{Offer, Stock};
use crate::util::{get_json, parse_num};

/// Where the clock speed starts: " (2.2 /", " a 3.0 GHz", " 3.4 GHz".
static CLOCK: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\s+(?:\(|a\s+\d|\d+(?:\.\d+)?\s*(?:GHz|Ghz|/))").expect("static regex is valid")
});

const URL: &str = "https://dinahosting.com/dedicados/maquinas";

pub async fn fetch(client: &reqwest::Client) -> Result<Vec<Offer>> {
    let v = get_json(client, URL).await?;
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for m in v
        .as_object()
        .into_iter()
        .flat_map(|o| o.values())
        .filter_map(|g| g.as_array())
        .flatten()
    {
        let Some(id) = m["id"].as_i64() else { continue };
        if !seen.insert(id) {
            continue;
        }
        let Some(price) = m["precioMes"].as_str().and_then(parse_num) else {
            continue;
        };
        // "Xeon E-2236 3.4 GHz", "AMD EPYC 9124 a 3.0 GHz 16c/32t",
        // "Xeon Silver 4114 (2.2 / 3 GHz)", "E5-2603 1.6 GHz"
        let spec = m["cpu_spec"].as_str().unwrap_or_default();
        let cpu = CLOCK.split(spec).next().unwrap_or(spec).trim();
        let cpu = if cpu.starts_with("AMD") || cpu.starts_with("Intel") {
            cpu.to_string()
        } else if cpu.starts_with("Xeon") {
            format!("Intel {cpu}")
        } else {
            format!("Intel Xeon {cpu}")
        };
        let n = m["discos_incluidos"].as_u64().unwrap_or(0);
        let size = m["discos_tamano"].as_f64().unwrap_or(0.0);
        let kind = m["discos_tipo"].as_str().unwrap_or_default();
        let avail = m["disponibilidad"].as_str().unwrap_or_default();
        let mut o = Offer::new("dinahosting", id.to_string());
        o.name = m["comercial"]["tagline"]
            .as_str()
            .unwrap_or_default()
            .to_string();
        o.cpu = cpu;
        o.cpu_count = m["cpu_num"].as_u64().unwrap_or(1) as u32;
        o.cores = m["cpu_nucleos_fisicos"]
            .as_u64()
            .map(|c| c as u32 * o.cpu_count);
        o.threads = m["cpu_subprocesos"]
            .as_u64()
            .map(|c| c as u32 * o.cpu_count);
        o.ram_gb = m["ram_incluida"].as_f64();
        o.storage = format!("{n}x {size} GB {kind}");
        o.storage_gb = Some(n as f64 * size);
        o.nvme = Some(kind.eq_ignore_ascii_case("nvme"));
        o.port_mbps = Some(1000);
        o.traffic = Some("unmetered".into());
        o.location = "Madrid".into();
        o.country = "ES".into();
        o.price_monthly = price;
        o.stock = if avail.contains("24h") {
            Stock::InStock
        } else {
            Stock::OutOfStock
        };
        o.availability = Some(avail.to_string());
        o.url = format!("https://dinahosting.com/dedicados/configurar/maquina/{id}");
        if let Some(p) = m["precioPromo"].as_str() {
            o.note = Some(format!("promo €{p}/mo"));
        }
        out.push(o);
    }
    Ok(out)
}
