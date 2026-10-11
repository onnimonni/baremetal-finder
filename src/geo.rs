/// EU-27 member states.
pub const EU: &[&str] = &[
    "AT", "BE", "BG", "HR", "CY", "CZ", "DK", "EE", "FI", "FR", "DE", "GR", "HU", "IE", "IT", "LV",
    "LT", "LU", "MT", "NL", "PL", "PT", "RO", "SK", "SI", "ES", "SE",
];

/// European but outside the EU (EEA/EFTA + UK etc.).
pub const EUROPE_NON_EU: &[&str] = &["GB", "NO", "CH", "IS", "LI", "UA", "RS", "MD"];

/// (country, is_eu, is_europe) rows for the `countries` table.
pub fn country_rows() -> Vec<(&'static str, bool, bool)> {
    EU.iter()
        .map(|c| (*c, true, true))
        .chain(EUROPE_NON_EU.iter().map(|c| (*c, false, true)))
        .collect()
}

/// English country name -> ISO code for providers that only give names.
pub fn iso_from_name(name: &str) -> Option<&'static str> {
    Some(match name.trim().to_lowercase().as_str() {
        "argentina" => "AR",
        "australia" => "AU",
        "austria" => "AT",
        "belgium" => "BE",
        "brazil" => "BR",
        "bulgaria" => "BG",
        "canada" => "CA",
        "chile" => "CL",
        "colombia" => "CO",
        "croatia" => "HR",
        "czechia" | "czech republic" => "CZ",
        "denmark" => "DK",
        "finland" => "FI",
        "france" => "FR",
        "germany" => "DE",
        "greece" => "GR",
        "hong kong" => "HK",
        "hungary" => "HU",
        "india" => "IN",
        "ireland" => "IE",
        "israel" => "IL",
        "italy" => "IT",
        "japan" => "JP",
        "latvia" => "LV",
        "lithuania" => "LT",
        "luxembourg" => "LU",
        "mexico" => "MX",
        "moldova" => "MD",
        "netherlands" | "the netherlands" => "NL",
        "nigeria" => "NG",
        "norway" => "NO",
        "peru" => "PE",
        "poland" => "PL",
        "portugal" => "PT",
        "romania" => "RO",
        "serbia" => "RS",
        "singapore" => "SG",
        "slovakia" => "SK",
        "south africa" => "ZA",
        "south korea" | "korea" => "KR",
        "spain" => "ES",
        "sweden" => "SE",
        "switzerland" => "CH",
        "turkey" | "türkiye" => "TR",
        "ukraine" => "UA",
        "united arab emirates" | "uae" => "AE",
        "united kingdom" | "uk" => "GB",
        "united states" | "usa" | "us" => "US",
        _ => return None,
    })
}
