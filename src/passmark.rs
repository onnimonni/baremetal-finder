//! PassMark CPU benchmark scores from the "CPU mega page" JSON feed.

use std::collections::HashMap;
use std::sync::LazyLock;

use anyhow::{Context, Result};
use regex::Regex;
use serde::Deserialize;

use crate::util::{clean, parse_num};

const MEGA_PAGE: &str = "https://www.cpubenchmark.net/CPU_mega_page.html";
const DATA_URL: &str = "https://www.cpubenchmark.net/data/";

#[derive(Debug, Clone)]
pub struct Cpu {
    pub name: String,
    pub key: String,
    pub cpumark: f64,
    pub thread: Option<f64>,
    pub cores: Option<u32>,
    pub threads: Option<u32>,
    pub cpu_count: u32,
}

#[derive(Deserialize)]
struct Feed {
    data: Vec<Row>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Row {
    name: String,
    cpumark: String,
    thread: String,
    cores: String,
    logicals: String,
    cpu_count: serde_json::Value,
    #[serde(default)]
    secondary_cores: serde_json::Value,
}

/// The feed mixes numbers and numeric strings for the same field.
fn as_u32(v: &serde_json::Value) -> Option<u32> {
    v.as_u64()
        .map(|n| n as u32)
        .or_else(|| v.as_str()?.parse().ok())
}

/// The data endpoint returns `{"data": []}` unless the request carries a
/// fresh session cookie from the page, so use a dedicated client and retry.
pub async fn fetch() -> Result<Vec<Cpu>> {
    for attempt in 1..=3 {
        let cpus = fetch_once(&crate::util::client()?).await?;
        if !cpus.is_empty() {
            return Ok(cpus);
        }
        eprintln!("passmark: empty feed (attempt {attempt})");
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    }
    anyhow::bail!("PassMark feed stayed empty")
}

async fn fetch_once(client: &reqwest::Client) -> Result<Vec<Cpu>> {
    client.get(MEGA_PAGE).send().await?.error_for_status()?;
    let ts = chrono::Utc::now().timestamp_millis();
    let body = client
        .get(format!("{DATA_URL}?_={ts}"))
        .header("X-Requested-With", "XMLHttpRequest")
        .header("Referer", MEGA_PAGE)
        .send()
        .await?
        .error_for_status()?
        .text()
        .await?;
    let feed: Feed = serde_json::from_str(&body).context("parsing PassMark feed")?;
    Ok(feed
        .data
        .into_iter()
        .filter_map(|r| {
            let cpumark = parse_num(&r.cpumark)?;
            let cpu_count = as_u32(&r.cpu_count).unwrap_or(1).max(1);
            let secondary = as_u32(&r.secondary_cores).unwrap_or(0);
            let cores: Option<u32> = r.cores.parse().ok();
            // "logicals" is threads per core in this feed.
            let per_core: Option<u32> = r.logicals.parse().ok();
            let total_cores = cores.map(|c| (c + secondary) * cpu_count);
            Some(Cpu {
                key: cpu_key(&r.name),
                thread: parse_num(&r.thread),
                threads: cores
                    .zip(per_core)
                    .map(|(c, l)| (c * l + secondary) * cpu_count),
                cores: total_cores,
                cpu_count,
                cpumark,
                name: r.name,
            })
        })
        .collect())
}

/// Normalize a CPU name so provider spellings and PassMark spellings meet:
/// "Intel® Core™ i7-8700 @ 3.20GHz" -> "i7-8700",
/// "Intel Xeon E5-1650 v3" -> "xeon e5-1650v3", "AMD RYZEN 9 9950X" -> "ryzen 9 9950x".
pub fn cpu_key(name: &str) -> String {
    static AT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"@.*$").unwrap());
    static NOISE: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"\b(amd|intel|core|processor|cpu|with radeon graphics|\d+-core|\d+\s*cores?)\b")
            .unwrap()
    });
    static VER: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\s+(v\d)\b").unwrap());
    // OVH writes "Xeon-E3 1270 v6" for PassMark's "Xeon E3-1270 v6".
    static XEON_E: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"\b(e[3579]?)\s+(\d{4})").unwrap());
    // "i7-7700(k)" -> "i7-7700k"; "E5-2680v1" -> "E5-2680" (PassMark omits v1)
    let s = clean(name)
        .to_lowercase()
        .replace("xeon-", "xeon ")
        .replace("(k)", "k")
        .replace("v1", "");
    let s = XEON_E.replace_all(&s, "$1-$2").to_string();
    let s = AT.replace(&s, "");
    let s = NOISE.replace_all(&s, " ");
    let s = VER.replace_all(&s, "$1");
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub struct Index {
    by_key: HashMap<String, Cpu>,
    /// Measured multi-socket systems: (key, sockets) -> entry.
    multi: HashMap<(String, u32), Cpu>,
    by_token: HashMap<String, Vec<String>>,
}

#[derive(Debug, Clone)]
pub struct Match {
    pub name: String,
    pub cpumark: f64,
    pub thread: Option<f64>,
    pub cores: Option<u32>,
    pub threads: Option<u32>,
    pub method: &'static str,
}

impl Index {
    pub fn new(cpus: &[Cpu]) -> Self {
        let mut by_key = HashMap::new();
        let mut by_token: HashMap<String, Vec<String>> = HashMap::new();
        // Single-socket entries win; CPUs PassMark only lists as multi-socket
        // get a per-socket estimate (score and cores divided by socket count).
        let multi = cpus
            .iter()
            .filter(|c| c.cpu_count > 1)
            .map(|c| ((c.key.clone(), c.cpu_count), c.clone()))
            .collect();
        let mut sorted: Vec<&Cpu> = cpus.iter().collect();
        sorted.sort_by_key(|c| c.cpu_count != 1);
        for c in sorted {
            if by_key.contains_key(&c.key) {
                continue;
            }
            let n = c.cpu_count.max(1);
            let single = Cpu {
                cpumark: c.cpumark / n as f64,
                cores: c.cores.map(|x| x / n),
                threads: c.threads.map(|x| x / n),
                cpu_count: 1,
                ..c.clone()
            };
            for t in model_tokens(&c.key) {
                by_token.entry(t).or_default().push(c.key.clone());
            }
            by_key.insert(c.key.clone(), single);
        }
        Self {
            by_key,
            multi,
            by_token,
        }
    }

    /// Look up a single-socket CPU and scale for multi-socket boxes.
    pub fn lookup(&self, name: &str, cpu_count: u32) -> Option<Match> {
        let key = cpu_key(name);
        let (cpu, method) = match self.by_key.get(&key) {
            Some(c) => (c, "exact"),
            None => (self.fuzzy(&key)?, "token"),
        };
        let n = cpu_count.max(1);
        if let Some(m) = self.multi.get(&(cpu.key.clone(), n)) {
            return Some(Match {
                name: m.name.clone(),
                cpumark: m.cpumark,
                thread: m.thread.or(cpu.thread),
                cores: m.cores,
                threads: m.threads,
                method,
            });
        }
        // No measured multi-socket entry: rough scaling.
        let scale = if n == 1 { 1.0 } else { n as f64 * 0.85 };
        Some(Match {
            name: cpu.name.clone(),
            cpumark: cpu.cpumark * scale,
            thread: cpu.thread,
            cores: cpu.cores.map(|c| c * n),
            threads: cpu.threads.map(|t| t * n),
            method,
        })
    }

    /// Match on the model number token ("7950x", "e3-1240v3", "i7-8700"),
    /// preferring the candidate with the fewest extra tokens.
    fn fuzzy(&self, key: &str) -> Option<&Cpu> {
        let mut tokens = model_tokens(key);
        // Providers invent suffixes PassMark doesn't list ("EPYC 9124P").
        if !tokens.iter().any(|t| self.by_token.contains_key(t)) {
            tokens = tokens
                .iter()
                .map(|t| t.trim_end_matches('p').to_string())
                .collect();
        }
        let want: Vec<&str> = key.split(' ').collect();
        let families: Vec<&str> = want
            .iter()
            .copied()
            .filter(|w| FAMILIES.contains(w))
            .collect();
        tokens
            .iter()
            .filter_map(|t| self.by_token.get(t))
            .flatten()
            .filter_map(|k| self.by_key.get(k))
            // Never cross families: "EPYC 9645" must not become "Ryzen 5 PRO 9645".
            .filter(|c| families.iter().all(|f| c.key.split(' ').any(|t| t == *f)))
            .min_by_key(|c| {
                let have: Vec<&str> = c.key.split(' ').collect();
                let missing = want.iter().filter(|w| !have.contains(w)).count();
                let extra = have.iter().filter(|h| !want.contains(h)).count();
                (missing * 10 + extra, c.key.len())
            })
    }
}

/// Product family words that must agree between provider and PassMark names.
const FAMILIES: &[&str] = &[
    "epyc",
    "ryzen",
    "xeon",
    "threadripper",
    "ultra",
    "opteron",
    "altra",
    "ampereone",
    "atom",
    "pentium",
    "celeron",
];

/// Tokens that identify a model: contain a digit and at least 3 chars.
fn model_tokens(key: &str) -> Vec<String> {
    key.split(' ')
        .filter(|t| t.len() >= 3 && t.chars().any(|c| c.is_ascii_digit()))
        .map(str::to_string)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys() {
        assert_eq!(cpu_key("Intel® Core™ i7-8700 @ 3.20GHz"), "i7-8700");
        assert_eq!(cpu_key("Intel Xeon E5-1650 v3 @ 3.50GHz"), "xeon e5-1650v3");
        assert_eq!(cpu_key("Intel XEON E5-1650V3"), "xeon e5-1650v3");
        assert_eq!(cpu_key("AMD RYZEN 9 9950X"), "ryzen 9 9950x");
        assert_eq!(cpu_key("AMD Ryzen™ 9 7950X"), "ryzen 9 7950x");
        assert_eq!(cpu_key("Intel Core Ultra 7 265"), "ultra 7 265");
        assert_eq!(cpu_key("Intel Xeon-E3 1270 v6"), "xeon e3-1270v6");
        assert_eq!(cpu_key("E5-2680v1"), "e5-2680");
    }

    #[test]
    fn fuzzy() {
        let mk = |n: &str, m: f64| Cpu {
            name: n.into(),
            key: cpu_key(n),
            cpumark: m,
            thread: None,
            cores: None,
            threads: None,
            cpu_count: 1,
        };
        let idx = Index::new(&[
            mk("Intel Xeon E3-1240 v3 @ 3.40GHz", 1.0),
            mk("AMD Ryzen 9 7950X", 2.0),
            mk("AMD Ryzen 9 7950X3D", 3.0),
        ]);
        assert_eq!(idx.lookup("E3-1240v3", 1).unwrap().cpumark, 1.0);
        assert_eq!(idx.lookup("AMD Ryzen 9 7950X", 1).unwrap().cpumark, 2.0);
        assert_eq!(idx.lookup("Ryzen 7950X", 1).unwrap().cpumark, 2.0);
        assert_eq!(idx.lookup("AMD Ryzen 9 7950XP", 1).unwrap().cpumark, 2.0);
        assert!(idx.lookup("AMD EPYC 7950X", 1).is_none());
    }
}
