//! RAM generation and NVMe PCIe generation, from provider text or (RAM only)
//! inferred from the CPU platform.

use std::sync::LazyLock;

use regex::Regex;

/// "64 GB DDR5", "DDRIII", "on-die ECC" (DDR5 only), OVH "ram-64g-ecc-2400"
/// (speed: 4800+ MT/s is DDR5, 2133-3200 is DDR4).
pub fn ram_type_from_text(text: &str) -> Option<&'static str> {
    static DDR: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"(?i)\bDDR\s?(5|4|3|III|IV|V)\b").expect("static regex is valid")
    });
    static OVH_SPEED: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"ram-\d+g-[a-z-]*?(\d{4})\b").expect("static regex is valid"));
    if let Some(c) = DDR.captures(text) {
        return match c[1].to_ascii_uppercase().as_str() {
            "5" | "V" => Some("DDR5"),
            "4" | "IV" => Some("DDR4"),
            _ => Some("DDR3"),
        };
    }
    let lower = text.to_ascii_lowercase();
    if lower.contains("on-die") || lower.contains("odecc") {
        return Some("DDR5");
    }
    let speed: u32 = OVH_SPEED.captures(&lower)?[1].parse().ok()?;
    match speed {
        4400.. => Some("DDR5"),
        2133..=3600 => Some("DDR4"),
        1066..=1866 => Some("DDR3"),
        _ => None,
    }
}

/// "NVMe SSD (GEN4)", "Gen5 NVMe", "PCIe 4.0".
pub fn nvme_gen_from_text(text: &str) -> Option<u8> {
    static GEN: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"(?i)\b(?:gen\s?([345])|pcie\s?([345])(?:\.0)?)\b")
            .expect("static regex is valid")
    });
    let c = GEN.captures(text)?;
    c.get(1).or(c.get(2))?.as_str().parse().ok()
}

/// Memory generation implied by the CPU platform, for normalized PassMark
/// keys (see `passmark::cpu_key`). `None` where boards exist for both.
pub fn ram_type_from_cpu(key: &str) -> Option<&'static str> {
    static RULES: LazyLock<Vec<(Regex, &'static str)>> = LazyLock::new(|| {
        [
            // AM5 desktop / EPYC 4004+4005 / Genoa+Turin / Siena / TR 7000+9000
            (r"ryzen \d (pro )?[789]\d{3}", "DDR5"),
            (r"epyc ([489]\d{3})", "DDR5"),
            (r"threadripper (pro )?[79]\d{3}", "DDR5"),
            // Xeon 6, 4th/5th gen Scalable (x4xx/x5xx), Xeon E-24xx, Core Ultra 200
            (r"xeon 6\d{3}", "DDR5"),
            (r"xeon (bronze|silver|gold|platinum) \d[45]\d{2}", "DDR5"),
            (r"xeon e-24\d{2}", "DDR5"),
            (r"ultra [579] 2\d{2}", "DDR5"),
            // AM4 / SP3 / 1st-3rd gen Scalable / Xeon E-21xx..23xx, W-2xxx, D-15xx/21xx
            (r"ryzen \d (pro )?[1-5]\d{3}", "DDR4"),
            (r"epyc 7\d{3}", "DDR4"),
            (r"threadripper (pro )?[1-5]\d{3}", "DDR4"),
            (r"xeon (bronze|silver|gold|platinum) \d[1-3]\d{2}", "DDR4"),
            (r"xeon e-2[123]\d{2}", "DDR4"),
            (r"xeon w-[23]\d{3}", "DDR4"),
            (r"xeon d-(15|21)\d{2}", "DDR4"),
            (r"e[35]-\d{4}v[34]", "DDR4"),
            (r"e3-1\d{3}v[56]", "DDR4"),
            (r"i[3579]-[6-9]\d{3}", "DDR4"),
            // Sandy/Ivy Bridge era
            (r"e[35]-\d{4}(v2)?$", "DDR3"),
            (r"(x|e|l)5\d{3}", "DDR3"),
        ]
        .into_iter()
        .map(|(re, t)| (Regex::new(re).expect("static regex is valid"), t))
        .collect()
    });
    RULES
        .iter()
        .find(|(re, _)| re.is_match(key))
        .map(|(_, t)| *t)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::passmark::cpu_key;

    #[test]
    fn text() {
        assert_eq!(ram_type_from_text("64 GB DDR5 RAM"), Some("DDR5"));
        assert_eq!(ram_type_from_text("16GB ECC DDRIII"), Some("DDR3"));
        assert_eq!(ram_type_from_text("ram-128g-on-die-ecc-3600"), Some("DDR5"));
        assert_eq!(ram_type_from_text("ram-64g-ecc-2400"), Some("DDR4"));
        assert_eq!(ram_type_from_text("ram-192g-ecc-5600"), Some("DDR5"));
        assert_eq!(nvme_gen_from_text("2x 1.92 TB NVMe SSD (GEN4)"), Some(4));
        assert_eq!(nvme_gen_from_text("2x960GB Gen5 NVMe"), Some(5));
        assert_eq!(nvme_gen_from_text("2x 1TB NVMe"), None);
    }

    #[test]
    fn cpu() {
        let t = |n: &str| ram_type_from_cpu(&cpu_key(n));
        assert_eq!(t("AMD Ryzen 9 7950X"), Some("DDR5"));
        assert_eq!(t("AMD Ryzen 9 5950X"), Some("DDR4"));
        assert_eq!(t("AMD EPYC 9454P"), Some("DDR5"));
        assert_eq!(t("AMD EPYC 4565P"), Some("DDR5"));
        assert_eq!(t("AMD EPYC 7502P"), Some("DDR4"));
        assert_eq!(t("Intel Xeon Gold 6248R"), Some("DDR4"));
        assert_eq!(t("Intel Xeon Gold 5412U"), Some("DDR5"));
        assert_eq!(t("Intel Xeon E5-2680 v4"), Some("DDR4"));
        assert_eq!(t("Intel Xeon E3-1230 v2"), Some("DDR3"));
        assert_eq!(t("Intel Core i9-14900K"), None);
    }
}
