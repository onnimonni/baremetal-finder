//! WHMCS store pages, used by many small hosts. Two common templates:
//! "standard" (`div.product` with `#productN-name`, `span.qty`, description,
//! `.product-pricing .price`) and "lux" (`div.package` with `.package-title`,
//! `.package-qty`, `.package-content`, `.price-amount`). WHMCS shows
//! "N Available" only for stock-tracked products.

use scraper::{ElementRef, Html, Node, Selector};

use crate::model::Stock;
use crate::util::{clean, first_num};

pub struct Product {
    pub id: String,
    pub name: String,
    /// Description split into lines (`<br>` and block elements).
    pub lines: Vec<String>,
    pub price: Option<f64>,
    pub setup: f64,
    pub qty: Option<i64>,
    /// Order link, absolute path or relative to the store.
    pub order: Option<String>,
    pub disabled: bool,
}

impl Product {
    pub fn stock(&self) -> Stock {
        match (self.qty, self.disabled) {
            (Some(q), _) if q > 0 => Stock::InStock,
            (Some(_), _) | (None, true) => Stock::OutOfStock,
            (None, false) => Stock::Unknown,
        }
    }
}

fn sel(s: &str) -> Selector {
    Selector::parse(s).expect("static CSS selector is valid")
}

fn text(e: ElementRef) -> String {
    clean(&e.text().collect::<Vec<_>>().join(" "))
}

/// First element matching `css` inside `e`.
fn first<'a>(e: ElementRef<'a>, css: &str) -> Option<ElementRef<'a>> {
    e.select(&sel(css)).next()
}

/// Text lines of an element, breaking at `<br>` and block elements.
pub fn lines(e: ElementRef) -> Vec<String> {
    let mut buf = String::new();
    for n in e.descendants() {
        match n.value() {
            Node::Text(t) => buf.push_str(t),
            Node::Element(el)
                if matches!(el.name(), "br" | "p" | "li" | "div" | "h3" | "h4" | "tr") =>
            {
                buf.push('\n');
            }
            _ => {}
        }
    }
    buf.lines().map(clean).filter(|l| !l.is_empty()).collect()
}

pub fn parse_store(html: &str) -> Vec<Product> {
    let doc = Html::parse_document(html);
    let standard = sel("div.product[id^='product']");
    let lux = sel("div.package[id^='product']");
    let mut out = Vec::new();
    for p in doc.select(&standard).chain(doc.select(&lux)) {
        let id = p.value().id().unwrap_or_default().to_string();
        let name = first(p, "[id$='-name'], .package-title")
            .map(text)
            .unwrap_or_default();
        let lines = first(p, "[id$='-description'], .package-content")
            .map(lines)
            .unwrap_or_default();
        let pricing = first(p, ".product-pricing, .package-price").map(text);
        let price = first(p, ".product-pricing .price, .price-amount")
            .map(text)
            .and_then(|t| first_num(&t));
        let setup = pricing
            .as_deref()
            .and_then(|t| t.split('+').nth(1))
            .and_then(first_num)
            .unwrap_or(0.0);
        let qty = first(p, ".qty, .package-qty").map(text).and_then(|t| {
            first_num(&t.replace('-', " -")).map(|n| {
                if t.trim_start().starts_with('-') {
                    -(n as i64)
                } else {
                    n as i64
                }
            })
        });
        let order_el = first(p, "a.btn-order-now, a[id$='-order-button']");
        let order = order_el
            .and_then(|a| a.value().attr("href"))
            .map(String::from);
        let disabled = order_el.is_some_and(|a| a.value().classes().any(|c| c == "disabled"));
        out.push(Product {
            id,
            name,
            lines,
            price,
            setup,
            qty,
            order,
            disabled,
        });
    }
    out
}
