use std::collections::HashMap;
use std::sync::LazyLock;

use anyhow::Result;
use chrono::{DateTime, Utc};
use duckdb::{Connection, params};
use regex::Regex;

use crate::geo;
use crate::hw;
use crate::model::Offer;
use crate::passmark::{Cpu, Index, Match, cpu_key};

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS offers (
    fetched_at TIMESTAMPTZ NOT NULL,
    provider VARCHAR NOT NULL,
    sku VARCHAR NOT NULL,
    name VARCHAR,
    cpu VARCHAR,
    cpu_count INTEGER,
    cores INTEGER,
    threads INTEGER,
    ram_gb DOUBLE,
    ecc BOOLEAN,
    storage VARCHAR,
    storage_gb DOUBLE,
    nvme BOOLEAN,
    port_mbps INTEGER,
    traffic VARCHAR,
    location VARCHAR,
    country VARCHAR,
    price_monthly DOUBLE,
    setup_fee DOUBLE,
    currency VARCHAR,
    price_monthly_eur DOUBLE,
    setup_fee_eur DOUBLE,
    stock VARCHAR,
    stock_qty BIGINT,
    availability VARCHAR,
    cpu_mark DOUBLE,
    single_thread DOUBLE,
    score_source VARCHAR,
    passmark_name VARCHAR,
    url VARCHAR,
    note VARCHAR
);

-- Added after the first release; appended so old databases keep working.
ALTER TABLE offers ADD COLUMN IF NOT EXISTS ram_type VARCHAR;
ALTER TABLE offers ADD COLUMN IF NOT EXISTS ram_type_source VARCHAR;
ALTER TABLE offers ADD COLUMN IF NOT EXISTS nvme_gen INTEGER;
ALTER TABLE offers ADD COLUMN IF NOT EXISTS kind VARCHAR;
ALTER TABLE offers ADD COLUMN IF NOT EXISTS vcpus INTEGER;
ALTER TABLE offers ADD COLUMN IF NOT EXISTS shared_cpu BOOLEAN;

CREATE TABLE IF NOT EXISTS fetch_runs (
    fetched_at TIMESTAMPTZ NOT NULL,
    provider VARCHAR NOT NULL,
    ok BOOLEAN NOT NULL,
    offers INTEGER,
    error VARCHAR
);

CREATE OR REPLACE TABLE countries (country VARCHAR PRIMARY KEY, is_eu BOOLEAN, is_europe BOOLEAN);

CREATE TABLE IF NOT EXISTS cpu_scores (
    name VARCHAR, key VARCHAR, cpumark DOUBLE, single_thread DOUBLE,
    cores INTEGER, threads INTEGER, cpu_count INTEGER
);

-- Newest successful snapshot of every provider.
CREATE OR REPLACE VIEW latest_offers AS
SELECT o.*
FROM offers o
JOIN (SELECT provider, max(fetched_at) AS fetched_at FROM fetch_runs WHERE ok GROUP BY provider) r
  USING (provider, fetched_at);

-- What `top` ranks. effective_monthly_eur amortizes setup over 12 months.
CREATE OR REPLACE VIEW ranked AS
SELECT
    o.*,
    coalesce(c.is_eu, false) AS is_eu,
    coalesce(c.is_europe, false) AS is_europe,
    price_monthly_eur + setup_fee_eur / 12 AS effective_monthly_eur,
    round(cpu_mark / nullif(price_monthly_eur + setup_fee_eur / 12, 0), 1) AS mark_per_eur
FROM latest_offers o
LEFT JOIN countries c USING (country);
"#;

pub fn open(path: &str) -> Result<Connection> {
    let conn = Connection::open(path)?;
    conn.execute_batch(SCHEMA)?;
    {
        let mut app = conn.appender("countries")?;
        for (c, eu, europe) in geo::country_rows() {
            app.append_row(params![c, eu, europe])?;
        }
    }
    Ok(conn)
}

pub fn save_cpu_scores(conn: &Connection, cpus: &[Cpu]) -> Result<()> {
    conn.execute("DELETE FROM cpu_scores", [])?;
    let mut app = conn.appender("cpu_scores")?;
    for c in cpus {
        app.append_row(params![
            c.name,
            c.key,
            c.cpumark,
            c.thread,
            c.cores,
            c.threads,
            c.cpu_count
        ])?;
    }
    Ok(())
}

/// "2 x Intel Xeon Gold 5120" -> (2, "Intel Xeon Gold 5120") unless the
/// provider already gave a count.
fn split_cpu_count(cpu: &str, count: u32) -> (u32, String) {
    // "Intel Dual Xeon E5-2680v3"
    if let Some((vendor, rest)) = cpu.split_once(" Dual ") {
        return (count.max(2), format!("{vendor} {rest}"));
    }
    for (word, n) in [("Single ", 1), ("Dual ", 2), ("Quad ", 4)] {
        if let Some(rest) = cpu.strip_prefix(word) {
            return (count.max(n), rest.to_string());
        }
    }
    static PREFIX: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"^(\d+)\s*[x×]\s*(.+)$").expect("static regex is valid"));
    match PREFIX.captures(cpu) {
        Some(c) => (
            if count > 1 {
                count
            } else {
                c[1].parse().unwrap_or(1)
            },
            c[2].to_string(),
        ),
        None => (count, cpu.to_string()),
    }
}

pub fn load_cpu_scores(conn: &Connection) -> Result<Vec<Cpu>> {
    let mut stmt = conn.prepare(
        "SELECT name, key, cpumark, single_thread, cores, threads, cpu_count FROM cpu_scores",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok(Cpu {
            name: r.get(0)?,
            key: r.get(1)?,
            cpumark: r.get(2)?,
            thread: r.get(3)?,
            cores: r.get(4)?,
            threads: r.get(5)?,
            cpu_count: r.get(6)?,
        })
    })?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// Median provider-published score per normalized CPU name, so a CPU one
/// provider benchmarks (DataPacket, OVH, Dedibox …) also scores elsewhere.
pub fn pool_provider_marks(results: &[(&str, Result<Vec<Offer>>)]) -> HashMap<String, f64> {
    let mut by_cpu: HashMap<String, Vec<f64>> = HashMap::new();
    for o in results
        .iter()
        .filter_map(|(_, r)| r.as_ref().ok())
        .flatten()
    {
        if let (Some(mark), None, None) = (o.provider_cpu_mark, o.dedicated_cores, o.vcpus) {
            let (count, cpu) = split_cpu_count(&o.cpu, o.cpu_count);
            if count == 1 {
                by_cpu.entry(cpu_key(&cpu)).or_default().push(mark);
            }
        }
    }
    by_cpu
        .into_iter()
        .map(|(k, mut v)| {
            v.sort_by(f64::total_cmp);
            (k, v[v.len() / 2])
        })
        .collect()
}

/// VPS plans that don't name their host CPU: the CPU generation the provider
/// documents (or that the plan is commonly reported to run on), used as a
/// rough per-vCPU reference. Scores are marked `passmark_host_guess`.
const HOST_GUESS: &[(&str, &str)] = &[
    ("Hetzner CPX", "AMD EPYC 7502P"),
    ("Hetzner CCX", "AMD EPYC 7713"),
    ("Hetzner CX", "Intel Xeon Gold 6148"),
    ("Linode", "AMD EPYC 7713"),
    ("AMD EPYC (DigitalOcean Premium)", "AMD EPYC 7543"),
    (
        "Intel Xeon (DigitalOcean Premium)",
        "Intel Xeon Platinum 8358",
    ),
    ("(DigitalOcean)", "Intel Xeon Gold 6140"),
    ("UpCloud", "AMD EPYC 7542"),
    ("AMD (Vultr", "AMD EPYC 7713"),
    ("Intel (Vultr vhp)", "Intel Xeon Platinum 8358"),
    ("Intel (Vultr", "Intel Xeon Gold 6140"),
    ("Contabo vCPU", "AMD EPYC 7282"),
];

pub fn save_run(
    conn: &Connection,
    at: DateTime<Utc>,
    provider: &str,
    result: &Result<Vec<Offer>>,
    index: &Index,
    pooled: &HashMap<String, f64>,
    fx: &HashMap<String, f64>,
) -> Result<()> {
    let (ok, n, err) = match result {
        Ok(v) => (true, v.len() as i32, None),
        Err(e) => (false, 0, Some(format!("{e:#}"))),
    };
    conn.execute(
        "INSERT INTO fetch_runs VALUES (?, ?, ?, ?, ?)",
        params![at, provider, ok, n, err],
    )?;
    let Ok(offers) = result else { return Ok(()) };

    let mut app = conn.appender("offers")?;
    for o in offers {
        let rate = fx.get(o.currency).copied().unwrap_or(1.0);
        let (cpu_count, cpu) = split_cpu_count(&o.cpu, o.cpu_count);
        let pooled_mark = pooled.get(&cpu_key(&cpu)).map(|m| {
            m * if cpu_count > 1 {
                cpu_count as f64 * 0.85
            } else {
                1.0
            }
        });
        let m = index.lookup(&cpu, cpu_count).or_else(|| {
            let (_, reference) = HOST_GUESS.iter().find(|(k, _)| cpu.contains(k))?;
            index.lookup(reference, 1).map(|m| Match {
                name: format!("~{}", m.name),
                method: "host_guess",
                ..m
            })
        });
        // Last resort: closest PassMark sibling scaled by cores.
        let estimate = (m.is_none() && o.provider_cpu_mark.is_none() && pooled_mark.is_none())
            .then(|| index.estimate(&cpu, o.cores, cpu_count))
            .flatten();
        // VM slices get their share of the whole chip's multi-thread score.
        // VPS: vCPUs are host threads; dedicated-core VMs: physical cores.
        let host = m.as_ref();
        let share = match (o.vcpus, o.dedicated_cores) {
            (Some(v), _) => host
                .and_then(|m| m.threads.or(m.cores))
                .filter(|t| *t > 0)
                .map_or(1.0, |t| (v as f64 / t as f64).min(1.0)),
            (None, Some(d)) => host
                .and_then(|m| m.cores)
                .filter(|t| *t > 0)
                .map_or(1.0, |t| (d as f64 / t as f64).min(1.0)),
            _ => 1.0,
        };
        let (mark, single, source, pm_name) = match (&m, o.provider_cpu_mark) {
            (Some(m), _) => (
                Some(m.cpumark * share),
                m.thread,
                Some(format!("passmark_{}", m.method)),
                Some(m.name.clone()),
            ),
            (None, Some(p)) => (Some(p), None, Some("provider".to_string()), None),
            (None, None) => match (pooled_mark, &estimate) {
                (Some(p), _) => (Some(p), None, Some("provider_pooled".to_string()), None),
                (None, Some(e)) => (
                    Some(e.cpumark),
                    e.thread,
                    Some("estimate_sibling".to_string()),
                    Some(e.name.clone()),
                ),
                (None, None) => (None, None, None, None),
            },
        };
        let text = format!(
            "{} {} {} {}",
            o.spec_text,
            o.name,
            o.storage,
            o.note.as_deref().unwrap_or("")
        );
        let (ram_type, ram_source) = match hw::ram_type_from_text(&text) {
            Some(t) => (Some(t), Some("provider")),
            None => {
                let key = m
                    .as_ref()
                    .map_or_else(|| cpu_key(&cpu), |m| cpu_key(&m.name));
                (hw::ram_type_from_cpu(&key), Some("cpu"))
            }
        };
        let ram_source = ram_type.and(ram_source);
        // Not the plan name: "Power-03 Gen3" is a product generation, not PCIe.
        let disk_text = format!("{} {}", o.spec_text, o.storage);
        let nvme_gen = o
            .nvme
            .filter(|n| *n)
            .and_then(|_| hw::nvme_gen_from_text(&disk_text));
        let cores = o.cores.or(m.as_ref().and_then(|m| m.cores));
        let threads = o.threads.or_else(|| {
            if o.dedicated_cores.is_some() || o.vcpus.is_some() {
                None
            } else {
                m.as_ref().and_then(|m| m.threads)
            }
        });
        app.append_row(params![
            at,
            o.provider,
            o.sku,
            o.name,
            cpu,
            cpu_count,
            cores,
            threads,
            o.ram_gb,
            o.ecc,
            o.storage,
            o.storage_gb,
            o.nvme,
            o.port_mbps,
            o.traffic,
            o.location,
            o.country,
            o.price_monthly,
            o.setup_fee,
            o.currency,
            o.price_monthly / rate,
            o.setup_fee / rate,
            o.stock.as_str(),
            o.stock_qty,
            o.availability,
            mark,
            single,
            source,
            pm_name,
            o.url,
            o.note,
            ram_type,
            ram_source,
            nvme_gen,
            o.kind,
            o.vcpus,
            o.shared_cpu,
        ])?;
    }
    Ok(())
}
