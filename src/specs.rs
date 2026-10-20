//! Best-effort hardware specs from free-text listing lines such as
//! "2x 12-Core Intel Xeon E5-2680v3", "48-Threads", "4x 16GB ECC REG RAM",
//! "2x 480GB SATA3 SSD", "1x 1Gbit Connection".

use std::sync::LazyLock;

use regex::Regex;

use crate::util::{first_num, parse_storage};

#[derive(Debug, Default)]
pub struct Specs {
    pub cpu: String,
    pub cpu_count: u32,
    pub cores: Option<u32>,
    pub threads: Option<u32>,
    pub ram_gb: Option<f64>,
    pub storage: String,
    pub port_mbps: Option<u32>,
}

pub fn from_lines(lines: &[String]) -> Specs {
    static CPU: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"(?i)\b(xeon|epyc|ryzen|opteron|threadripper|atom|core\s+i[3579]|core\s+ultra|i[3579]-\d{4}|\bE[357]?-\d{4})").expect("static regex is valid")
    });
    static COUNT: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"(?i)^(\d+)\s*x\s*").expect("static regex is valid"));
    static PER_CPU_CORES: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"(?i)\b(\d+)[- ]?cores?\b|\((\d+)\s*c\s*/\s*(\d+)\s*t")
            .expect("static regex is valid")
    });
    static CORES: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"(?i)\b(\d+)\s*(?:cores?|c)\s*/\s*(\d+)\s*(?:threads?|t)\b|\b(\d+)\s*cores?\b")
            .expect("static regex is valid")
    });
    static THREADS: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"(?i)\b(\d+)[- ]?threads?\b").expect("static regex is valid"));
    // "Intel Xeon E5-2650 v2 128gb RAM"
    static RAM_TAIL: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"(?i)\s+\d+\s*gb\b").expect("static regex is valid"));
    static PORT: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"(?i)(\d+(?:\.\d+)?)\s*(g|m)(?:bit|bps|b/s|bit/s)")
            .expect("static regex is valid")
    });

    let mut s = Specs {
        cpu_count: 1,
        ..Specs::default()
    };
    let mut disks = Vec::new();
    for line in lines {
        let lower = line.to_lowercase();
        if s.cpu.is_empty() && CPU.is_match(line) {
            let mut cpu = line.as_str();
            if let Some(c) = COUNT.captures(cpu) {
                s.cpu_count = c[1].parse().unwrap_or(1);
                cpu = &cpu[c[0].len()..];
            }
            // "12-Core Intel Xeon …" / "AMD Ryzen 9 7900 (12c/24t …)"
            if let Some(c) = PER_CPU_CORES.captures(cpu) {
                let per = c
                    .get(1)
                    .or(c.get(2))
                    .and_then(|m| m.as_str().parse::<u32>().ok());
                s.cores = per.map(|p| p * s.cpu_count);
                s.threads = c
                    .get(3)
                    .and_then(|m| m.as_str().parse::<u32>().ok())
                    .map(|t| t * s.cpu_count);
            }
            let cpu = cpu.trim_start_matches("CPU Model:").trim();
            let cpu = RAM_TAIL.split(cpu).next().unwrap_or(cpu);
            if s.threads.is_none()
                && let Some(t) = THREADS.captures(cpu)
            {
                s.threads = t[1].parse::<u32>().ok().map(|t| t * s.cpu_count);
            }
            let cleaned = PER_CPU_CORES.replace_all(cpu, "");
            s.cpu = cleaned
                .split(['(', ',', '|'])
                .next()
                .unwrap_or_default()
                .trim_end_matches([' ', '-'])
                .replace(" CPU", "")
                .trim()
                .to_string();
            // "E5-2660v2 (10 cores 20 threads)" names only the model.
            if s.cpu.starts_with('E') {
                s.cpu = format!("Intel Xeon {}", s.cpu);
            }
            continue;
        }
        if s.cores.is_none()
            && let Some(c) = CORES.captures(line)
        {
            s.cores = c.get(1).or(c.get(3)).and_then(|m| m.as_str().parse().ok());
            s.threads = c.get(2).and_then(|m| m.as_str().parse().ok());
        }
        if s.threads.is_none()
            && let Some(c) = THREADS.captures(line)
        {
            s.threads = c[1].parse().ok();
        }
        if s.ram_gb.is_none()
            && (lower.contains("ram") || lower.contains("memory") || lower.contains("ddr"))
        {
            s.ram_gb = parse_storage(line).0.or_else(|| first_num(line));
            continue;
        }
        if ["nvme", "ssd", "hdd", "sata", "sas"]
            .iter()
            .any(|k| lower.contains(k))
            && !lower.contains("ram")
        {
            disks.push(line.clone());
            continue;
        }
        if s.port_mbps.is_none()
            && let Some(c) = PORT.captures(line)
        {
            let n: f64 = c[1].parse().unwrap_or(0.0);
            s.port_mbps = Some(if c[2].eq_ignore_ascii_case("g") {
                n * 1000.0
            } else {
                n
            } as u32);
        }
    }
    s.storage = disks.join(" + ");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flokinet_style() {
        let lines: Vec<String> = [
            "2x 12-Core Intel Xeon E5-2680v3",
            "48-Threads 60MB Cache 2.5/3.3GHz",
            "4x 16GB ECC REG RAM",
            "2x 480GB SATA3 SSD",
            "1x 1Gbit Connection Single port Up-link",
        ]
        .map(String::from)
        .to_vec();
        let s = from_lines(&lines);
        assert_eq!(s.cpu, "Intel Xeon E5-2680v3");
        assert_eq!(s.cpu_count, 2);
        assert_eq!(s.cores, Some(24));
        assert_eq!(s.threads, Some(48));
        assert_eq!(s.ram_gb, Some(64.0));
        assert_eq!(s.storage, "2x 480GB SATA3 SSD");
        assert_eq!(s.port_mbps, Some(1000));
    }

    #[test]
    fn bare_xeon_model() {
        let lines: Vec<String> = ["E5-2660v2 (10 cores 20 threads)", "RAM 128GB", "500GB SSD"]
            .map(String::from)
            .to_vec();
        let s = from_lines(&lines);
        assert_eq!(s.cpu, "Intel Xeon E5-2660v2");
        assert_eq!(s.cores, Some(10));
        assert_eq!(s.ram_gb, Some(128.0));
    }

    #[test]
    fn speedypage_style() {
        let lines: Vec<String> = [
            "AMD Ryzen 7900",
            "128GB DDR5 ECC Memory",
            "2x 2TB NVMe Gen4 Disk Space",
            "100TB bandwidth @ 2x 10Gbps",
        ]
        .map(String::from)
        .to_vec();
        let s = from_lines(&lines);
        assert_eq!(s.cpu, "AMD Ryzen 7900");
        assert_eq!(s.ram_gb, Some(128.0));
        assert_eq!(s.port_mbps, Some(10000));
    }
}
