use std::sync::LazyLock;
use std::time::Duration;

use anyhow::{Context, Result};
use regex::Regex;

pub const UA: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/130.0 Safari/537.36";

pub fn client() -> Result<reqwest::Client> {
    Ok(reqwest::Client::builder()
        .user_agent(UA)
        .cookie_store(true)
        .timeout(Duration::from_secs(60))
        .build()?)
}

/// Client that reproduces Chrome's TLS (BoringSSL ClientHello/JA3/JA4) and
/// HTTP/2 fingerprint, for sites behind Cloudflare-style bot checks that
/// reject plain rustls clients. Egresses from the host IP (no proxies).
pub fn browser() -> Result<wreq::Client> {
    use wreq_util::{Emulation, Platform, Profile};
    let emulation = Emulation::builder()
        .profile(Profile::Chrome149)
        .platform(Platform::MacOS)
        .build();
    Ok(wreq::Client::builder()
        .emulation(emulation)
        .cookie_store(true)
        .timeout(Duration::from_secs(60))
        .build()?)
}

pub async fn browser_text(client: &wreq::Client, url: &str) -> Result<String> {
    let resp = client
        .get(url)
        .send()
        .await
        .with_context(|| format!("GET {url}"))?;
    let status = resp.status();
    let body = resp.text().await?;
    anyhow::ensure!(status.is_success(), "GET {url} -> {status}");
    Ok(body)
}

pub async fn get_text(client: &reqwest::Client, url: &str) -> Result<String> {
    let resp = client
        .get(url)
        .send()
        .await
        .with_context(|| format!("GET {url}"))?;
    let status = resp.status();
    let body = resp.text().await?;
    anyhow::ensure!(status.is_success(), "GET {url} -> {status}");
    Ok(body)
}

pub async fn get_json(client: &reqwest::Client, url: &str) -> Result<serde_json::Value> {
    let body = get_text(client, url).await?;
    serde_json::from_str(&body).with_context(|| format!("parsing JSON from {url}"))
}

/// Parse "1.92", "1,92", "65,817", "2.493,99" style numbers. When both
/// separators are present the last one is the decimal mark; a lone comma
/// followed by exactly three digits is a thousands separator.
pub fn parse_num(s: &str) -> Option<f64> {
    let s = s.trim();
    let (dot, comma) = (s.rfind('.'), s.rfind(','));
    let normalized = match (dot, comma) {
        (Some(d), Some(c)) if c > d => s.replace('.', "").replace(',', "."),
        (Some(_), Some(_)) => s.replace(',', ""),
        (None, Some(c)) if s.len() - c - 1 == 3 && s.matches(',').count() == 1 => {
            s.replace(',', "")
        }
        (None, Some(_)) => s.replace(',', "."),
        _ => s.to_string(),
    };
    normalized.parse().ok()
}

/// First number found in a string, e.g. "€147,68 EUR" -> 147.68
pub fn first_num(s: &str) -> Option<f64> {
    static NUM: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"\d[\d.,]*").expect("static regex is valid"));
    NUM.find(s)
        .and_then(|m| parse_num(m.as_str().trim_end_matches(['.', ','])))
}

/// Sum disk capacity and detect NVMe from strings like
/// "2 x 1.92TB Ent NVME", "2x 512 GB NVMe", "1 x 1TB NVME + 2 x 4TB HDD".
pub fn parse_storage(s: &str) -> (Option<f64>, Option<bool>) {
    static DISK: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"(?i)(?:(\d+)\s*[x×]\s*)?(\d+(?:[.,]\d+)?)\s*(TB|GB|T|G)\b")
            .expect("static regex is valid")
    });
    let mut total = 0.0;
    let mut found = false;
    for c in DISK.captures_iter(s) {
        let count: f64 = c
            .get(1)
            .and_then(|m| m.as_str().parse().ok())
            .unwrap_or(1.0);
        let Some(size) = parse_num(&c[2]) else {
            continue;
        };
        let unit = c[3].to_ascii_uppercase();
        let gb = if unit.starts_with('T') {
            size * 1000.0
        } else {
            size
        };
        total += count * gb;
        found = true;
    }
    let nvme = s.to_ascii_lowercase().contains("nvme");
    (found.then_some(total), Some(nvme))
}

/// ISO currency code as a static str (ECB has rates for all of these).
pub fn currency(code: &str) -> Option<&'static str> {
    const KNOWN: &[&str] = &[
        "EUR", "USD", "GBP", "JPY", "SGD", "AUD", "CAD", "HKD", "CHF", "SEK", "NOK", "DKK", "PLN",
        "CZK", "BRL", "INR",
    ];
    KNOWN.iter().find(|c| c.eq_ignore_ascii_case(code)).copied()
}

/// The JSON value at the start of `s` (array/object), by bracket matching
/// that skips string contents.
pub fn balanced_json(s: &str) -> Option<&str> {
    let (mut depth, mut in_str, mut esc) = (0i32, false, false);
    for (i, ch) in s.char_indices() {
        if in_str {
            match (esc, ch) {
                (true, _) => esc = false,
                (false, '\\') => esc = true,
                (false, '"') => in_str = false,
                _ => {}
            }
            continue;
        }
        match ch {
            '"' => in_str = true,
            '[' | '{' => depth += 1,
            ']' | '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(&s[..=i]);
                }
            }
            _ => {}
        }
    }
    None
}

/// Normalize whitespace and strip trademark noise.
pub fn clean(s: &str) -> String {
    s.replace(['™', '®'], "")
        .replace("(R)", "")
        .replace("(TM)", "")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn storage() {
        assert_eq!(parse_storage("2 x 1.92TB Ent NVME").0, Some(3840.0));
        assert_eq!(parse_storage("2x 512 GB NVMe").0, Some(1024.0));
        assert_eq!(parse_storage("1 x 1TB NVME").0, Some(1000.0));
        assert_eq!(parse_storage("2x 1,92 TB NVMe SSD (GEN4)").0, Some(3840.0));
        assert_eq!(parse_storage("2 x 500 GB NVME").1, Some(true));
    }

    #[test]
    fn nums() {
        assert_eq!(first_num("€147,68  EUR"), Some(147.68));
        assert_eq!(first_num("65,817"), Some(65817.0));
        assert_eq!(first_num("€2.493,99"), Some(2493.99));
        assert_eq!(first_num("$1,188/month"), Some(1188.0));
    }
}
