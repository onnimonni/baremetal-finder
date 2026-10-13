use std::future::Future;
use std::pin::Pin;

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};

use anyhow::Result;

use crate::model::Offer;

pub mod alpenhost;
pub mod avoro;
pub mod bacloud;
pub mod cherry;
pub mod clouvider;
pub mod contabo;
pub mod datapacket;
pub mod dedibox;
pub mod dedimax;
pub mod evoluso;
pub mod fasthosts;
pub mod flokinet;
pub mod hetzner;
pub mod hivelocity;
pub mod hostkey;
pub mod i3d;
pub mod ikoula;
pub mod interserver;
pub mod ionos;
pub mod ipprojects;
pub mod latitude;
pub mod leaseweb;
pub mod mevspace;
pub mod netcup;
pub mod netrouting;
pub mod nforce;
pub mod nocix;
pub mod novoserve;
pub mod nuxoa;
pub mod oneprovider;
pub mod ovh;
pub mod pebblehost;
pub mod phoenixnap;
pub mod prepaidhoster;
pub mod psychz;
pub mod rapidswitch;
pub mod scaleway;
pub mod server4you;
pub mod servermania;
pub mod servernet;
pub mod serverroom;
pub mod serverscom;
pub mod spectraip;
pub mod speedypage;
pub mod tiernet;
pub mod velia;
pub mod vultr;
pub mod webnx;
pub mod webtropia;
pub mod whmcs_hosts;
pub mod worldstream;
pub mod zap;

type Fetch = for<'a> fn(
    &'a reqwest::Client,
) -> Pin<Box<dyn Future<Output = Result<Vec<Offer>>> + Send + 'a>>;

macro_rules! fetcher {
    ($f:path) => {
        |c| Box::pin($f(c))
    };
}

/// (name, fetcher). Name is what `--provider` filters on.
pub const ALL: &[(&str, Fetch)] = &[
    ("hetzner_auction", fetcher!(hetzner::auction)),
    ("hetzner", fetcher!(hetzner::standard)),
    ("ovh", fetcher!(ovh::eu)),
    ("ovh_us", fetcher!(ovh::us)),
    ("mevspace", fetcher!(mevspace::fetch)),
    ("cherry", fetcher!(cherry::fetch)),
    ("servernet", fetcher!(servernet::fetch)),
    ("alpenhost", fetcher!(alpenhost::fetch)),
    ("datapacket", fetcher!(datapacket::fetch)),
    ("latitude", fetcher!(latitude::fetch)),
    ("scaleway", fetcher!(scaleway::fetch)),
    ("hivelocity", fetcher!(hivelocity::fetch)),
    ("hostkey", fetcher!(hostkey::fetch)),
    ("pebblehost", fetcher!(pebblehost::fetch)),
    ("ipprojects", fetcher!(ipprojects::fetch)),
    ("interserver", fetcher!(interserver::fetch)),
    ("leaseweb", fetcher!(leaseweb::fetch)),
    ("worldstream", fetcher!(worldstream::fetch)),
    ("nuxoa", fetcher!(nuxoa::fetch)),
    ("contabo", fetcher!(contabo::fetch)),
    ("netcup", fetcher!(netcup::fetch)),
    ("ionos", fetcher!(ionos::fetch)),
    ("clouvider", fetcher!(clouvider::fetch)),
    ("serverscom", fetcher!(serverscom::fetch)),
    ("dedibox", fetcher!(dedibox::fetch)),
    ("vultr", fetcher!(vultr::fetch)),
    ("oneprovider", fetcher!(oneprovider::fetch)),
    ("psychz", fetcher!(psychz::fetch)),
    ("tiernet", fetcher!(tiernet::fetch)),
    ("phoenixnap", fetcher!(phoenixnap::fetch)),
    ("webtropia", fetcher!(webtropia::fetch)),
    ("server4you", fetcher!(server4you::fetch)),
    ("ikoula", fetcher!(ikoula::fetch)),
    ("avoro", fetcher!(avoro::fetch)),
    ("velia", fetcher!(velia::fetch)),
    ("nocix", fetcher!(nocix::fetch)),
    ("i3d", fetcher!(i3d::fetch)),
    ("serverroom", fetcher!(serverroom::fetch)),
    ("netrouting", fetcher!(netrouting::fetch)),
    ("novoserve", fetcher!(novoserve::fetch)),
    ("nforce", fetcher!(nforce::fetch)),
    ("evoluso", fetcher!(evoluso::fetch)),
    ("flokinet", fetcher!(flokinet::fetch)),
    ("spectraip", fetcher!(spectraip::fetch)),
    ("speedypage", fetcher!(speedypage::fetch)),
    ("fasthosts", fetcher!(fasthosts::fetch)),
    ("rapidswitch", fetcher!(rapidswitch::fetch)),
    ("bacloud", fetcher!(bacloud::fetch)),
    ("zap", fetcher!(zap::fetch)),
    ("prepaidhoster", fetcher!(prepaidhoster::fetch)),
    ("dedimax", fetcher!(dedimax::fetch)),
    ("webnx", fetcher!(webnx::fetch)),
    ("servermania", fetcher!(servermania::fetch)),
    ("quickpacket", fetcher!(whmcs_hosts::quickpacket)),
    ("racknerd", fetcher!(whmcs_hosts::racknerd)),
    ("greencloud", fetcher!(whmcs_hosts::greencloud)),
];

/// Pages behind Cloudflare JavaScript challenges: (provider, url, JS that is
/// true once the real content is there). With the `webview` feature these
/// are loaded through the OS web engine before the async fetch starts; the
/// provider then reads them with [`prefetched`]. Add new sites here.
#[cfg_attr(not(feature = "webview"), allow(dead_code))]
pub const WEBVIEW_PAGES: &[(&str, &str, &str)] = &[("avoro", avoro::URL, avoro::READY)];

static PREFETCHED: LazyLock<Mutex<HashMap<String, String>>> = LazyLock::new(Default::default);

/// HTML the webview loaded for `url`, if any.
pub fn prefetched(url: &str) -> Option<String> {
    PREFETCHED.lock().ok()?.get(url).cloned()
}

/// Load every challenged page of the selected providers (blocking; must run
/// on the main thread before the tokio runtime starts).
#[cfg(feature = "webview")]
pub fn prefetch_webview_pages(only: &[String]) {
    for (provider, url, ready) in WEBVIEW_PAGES {
        if !only.is_empty() && !only.iter().any(|o| o == provider) {
            continue;
        }
        // Cloudflare's interstitial is titled "Just a moment...".
        let ready = format!("!document.title.includes('Just a moment') && ({ready})");
        match crate::webview::grab(url, &ready) {
            Ok(html) => {
                if let Ok(mut m) = PREFETCHED.lock() {
                    m.insert((*url).to_string(), html);
                }
            }
            Err(e) => eprintln!("{provider} webview: {e:#}"),
        }
    }
}
