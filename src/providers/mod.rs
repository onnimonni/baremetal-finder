use std::future::Future;
use std::pin::Pin;

use anyhow::Result;

use crate::model::Offer;

pub mod alpenhost;
pub mod avoro;
pub mod cherry;
pub mod clouvider;
pub mod contabo;
pub mod datapacket;
pub mod dedibox;
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
pub mod nocix;
pub mod nuxoa;
pub mod oneprovider;
pub mod ovh;
pub mod pebblehost;
pub mod phoenixnap;
pub mod psychz;
pub mod scaleway;
pub mod server4you;
pub mod servernet;
pub mod serverscom;
pub mod tiernet;
pub mod velia;
pub mod vultr;
pub mod webtropia;
pub mod worldstream;

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
];
