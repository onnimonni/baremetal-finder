use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
#[allow(clippy::enum_variant_names)] // names mirror the stored values
pub enum Stock {
    /// Provider says it can be delivered now.
    InStock,
    /// Provider explicitly says sold out / unavailable / pre-order only.
    OutOfStock,
    /// Provider exposes no stock signal for this offer.
    Unknown,
}

impl Stock {
    pub fn as_str(self) -> &'static str {
        match self {
            Stock::InStock => "in_stock",
            Stock::OutOfStock => "out_of_stock",
            Stock::Unknown => "unknown",
        }
    }
}

/// One orderable server configuration at one location.
#[derive(Debug, Clone, Serialize)]
pub struct Offer {
    pub provider: &'static str,
    /// Provider specific id (plan code, product id, auction id …)
    pub sku: String,
    pub name: String,
    /// CPU model as the provider writes it, without the count prefix.
    pub cpu: String,
    pub cpu_count: u32,
    /// Physical cores (total over all CPUs) when the provider tells it.
    pub cores: Option<u32>,
    pub threads: Option<u32>,
    pub ram_gb: Option<f64>,
    pub ecc: Option<bool>,
    pub storage: String,
    pub storage_gb: Option<f64>,
    pub nvme: Option<bool>,
    pub port_mbps: Option<u32>,
    pub traffic: Option<String>,
    pub location: String,
    /// ISO 3166-1 alpha-2, upper case.
    pub country: String,
    /// Monthly price excl VAT, in the provider's currency.
    pub price_monthly: f64,
    pub setup_fee: f64,
    pub currency: &'static str,
    pub stock: Stock,
    pub stock_qty: Option<i64>,
    /// Raw availability text from the provider (e.g. OVH "1H-high", "72H").
    pub availability: Option<String>,
    /// Benchmark score reported by the provider itself (PassMark-ish), used as
    /// fallback when the CPU can't be matched against PassMark.
    pub provider_cpu_mark: Option<f64>,
    /// VM products: dedicated cores out of a bigger CPU; the benchmark
    /// score is scaled down to this share.
    pub dedicated_cores: Option<u32>,
    /// Free-form spec text from the listing (RAM line, drive line …), used to
    /// detect DDR generation and NVMe PCIe generation.
    pub spec_text: String,
    pub url: String,
    pub note: Option<String>,
}

impl Offer {
    pub fn new(provider: &'static str, sku: impl Into<String>) -> Self {
        Self {
            provider,
            sku: sku.into(),
            name: String::new(),
            cpu: String::new(),
            cpu_count: 1,
            cores: None,
            threads: None,
            ram_gb: None,
            ecc: None,
            storage: String::new(),
            storage_gb: None,
            nvme: None,
            port_mbps: None,
            traffic: None,
            location: String::new(),
            country: String::new(),
            price_monthly: 0.0,
            setup_fee: 0.0,
            currency: "EUR",
            stock: Stock::Unknown,
            stock_qty: None,
            availability: None,
            provider_cpu_mark: None,
            dedicated_cores: None,
            spec_text: String::new(),
            url: String::new(),
            note: None,
        }
    }
}
