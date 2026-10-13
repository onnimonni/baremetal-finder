# baremetal-finder

Find the best price/performance dedicated (bare-metal) servers that are **in stock right now**, across 56 hosting providers, with an EU-only filter.

It fetches public catalogs, prices and stock signals, scores every CPU with [PassMark](https://www.cpubenchmark.net/) and stores snapshots in DuckDB so you can rank offers by benchmark points per euro.

```
$ baremetal-finder top --eu --now --min-cores 16 --nvme --min-ram 64 --limit 3
+----------+-------------------+-------------------+----+-----+---------+--------+-------+-------+------+----------+------------+-------------------+
| provider | name              | cpu               | c  | ram | disk_gb | eur_mo | setup | mark  | st   | mark_eur | where      | avail             |
+----------+-------------------+-------------------+----+-----+---------+--------+-------+-------+------+----------+------------+-------------------+
| mevspace | AMD Ryzen 9 7950X | AMD Ryzen 9 7950X | 16 | 96  | 2000    | 111.0  | 0.0   | 62125 | 4251 | 559.7    | PL:Wyszków | Instant available |
| ovh      | RISE-L | AMD ...  | AMD RYZEN 9 9950X | 16 | 128 | 1920    | 149.99 | 150.0 | 65707 | 4728 | 404.4    | FR:rbx     | 1H-high           |
| hetzner  | EX63              | Intel Core Ultra… | 20 | 64  | 2000    | 149.0  | 74.0  | 49682 | 4697 | 320.2    | DE:FSN1    | upfront           |
+----------+-------------------+-------------------+----+-----+---------+--------+-------+-------+------+----------+------------+-------------------+
```

## Usage

Requires Rust (or [devenv](https://devenv.sh): `devenv shell`).

```sh
cargo run --release -- fetch                      # snapshot every provider (~1 min)
cargo run --release -- fetch --provider ovh,mevspace
cargo run --release -- top --eu --now --min-cores 16 --nvme --min-ram 64
cargo run --release -- providers                  # list provider names
```

`top` options:

| Flag | Meaning |
|---|---|
| `--eu` / `--europe` | EU-27 only / EU + UK, NO, CH … |
| `--now` | only deliverable within ~1 h (drops OVH 24H/72H, build-to-order) |
| `--include-unknown` | also show offers whose provider exposes no stock signal |
| `--any-stock` | also show sold-out offers |
| `--min-cores`, `--min-ram`, `--min-storage`, `--nvme`, `--min-single` | hardware filters |
| `--ram-type DDR5` | RAM generation (stated by the provider, else implied by the CPU platform) |
| `--max-price` | max effective €/month (setup fee spread over 12 months) |
| `--sort value\|mark\|single\|price` | PassMark per €, multi-thread, single-thread, price |

Everything lands in `servers.duckdb`; query it directly:

```sh
duckdb servers.duckdb "SELECT * FROM ranked WHERE is_eu AND stock = 'in_stock' ORDER BY mark_per_eur DESC LIMIT 20"
```

| Table / view | Contents |
|---|---|
| `offers` | every snapshot (history kept) |
| `latest_offers` | newest successful snapshot per provider |
| `ranked` | `latest_offers` + `is_eu`, `effective_monthly_eur`, `mark_per_eur` |
| `fetch_runs` | per-provider run status and errors |
| `cpu_scores` | PassMark multi/single-thread scores (cached for when the feed fails) |

## Providers

| Provider | Source | Stock signal |
|---|---|---|
| Hetzner (auction) | live JSON | listed = available |
| Hetzner (standard) | price API + upfront JSON | ready per DC, else build-to-order |
| OVHcloud EU / US | public catalog + availability API | per DC (`1H-high`, `72H`, `unavailable`) |
| MEVSPACE | server finder endpoint | in stock / out of stock |
| Cherry Servers | public plans API | quantity per region |
| server.net | HTML | sold out / pre-order |
| Alpenhost | WHMCS store | quantity / disabled order button |
| DataPacket | public GraphQL | quantity per location |
| Latitude.sh | pricing page RSC payload | per country |
| Scaleway Elastic Metal | public catalog | unknown (set `SCW_SECRET_KEY` for real stock) |
| Scaleway Dedibox | pricing page data | per offer |
| Hivelocity | store inventory JSON | available / limited (qty) / unavailable |
| Hostkey | presets API | quantity |
| PebbleHost | HTML + stock JSON | per plan |
| IP-Projects | HTML (Server Depot + catalog) | depot units; catalog unknown |
| Interserver | marketplace HTML | listed = available |
| Leaseweb | shop API | quantity of 1-hour instant servers |
| Worldstream | turbo-stream data endpoint | out of stock / limited count |
| NUXOA | HTML | listed = available |
| Contabo | catalog API | per product |
| netcup (root server VMs) | content API | per variant; score scaled to dedicated cores |
| IONOS | `__NEXT_DATA__` + shop API | per datacenter |
| Clouvider | HostBill store | unknown |
| servers.com | inline flavor list | unknown |
| Vultr Bare Metal | public API | per region |
| OneProvider | search endpoint | express / in stock / limited / sold out |
| Psychz | order table fragment | available / out of stock |
| Tier.Net | WHMCS store | in / out of stock |
| PhoenixNAP | billing products API | unknown |
| webtropia | store API (browser TLS) | in stock / limited / sold |
| server4you | flight data + order page | per variant |
| Ikoula | HTML + shop page | per product |
| avoro | instant list HTML (Cloudflare JS challenge: needs `--features webview`) | listed = available |
| Velia | Upmind API | unknown |
| Nocix / WholeSaleInternet | HTML + cart page (browser TLS) | in / out of stock |
| i3D.net | marketing page (browser TLS) + FlexMetal API | unknown (set `I3D_TOKEN` for per-location stock) |
| ServerRoom | instant-server JSON | listed = available (one row per machine) |
| Netrouting | shop catalog API | instant quantity per location |
| NovoServe | WHMCS store (NL + US) | quantity |
| Nforce | HTML (cookie gate) + detail page | in stock / unknown |
| Evoluso | HTML | unknown (provider doesn't track) |
| FlokiNET (FI/IS/NL/RO) | WHMCS store | quantity |
| SpectraIP | marketing page + WHMCS cart | in / out of stock |
| SpeedyPage | WHMCS store | quantity |
| Fasthosts | server page + stock API | per datacenter |
| RapidSwitch (ex-Redstation) | shop flight data | in / out of stock |
| Bacloud | HTML location flags | per location |
| ZAP-Hosting | shop blades JSON | quantity |
| Prepaid-Hoster | public JSON API | listed = available (Hetzner auction resale) |
| Dedimax | broker JSON feed | ready-in-minutes flag |
| WebNX | WHMCS products JSON | quantity |
| ServerMania | inventory API | quantity per location |
| QuickPacket / RackNerd (browser TLS) / GreenCloud | WHMCS store | quantity where tracked |

## Browser TLS fingerprints

A few sites (webtropia, Nocix/WholeSaleInternet, i3D.net, RackNerd) sit behind Cloudflare checks that reject non-browser TLS handshakes. Those are fetched with [wreq](https://github.com/0x676e67/wreq) + `wreq-util` emulating Chrome's BoringSSL ClientHello and HTTP/2 settings, straight from your own IP (no proxies). Building it compiles BoringSSL, so `cmake` and a C compiler are needed (included in `devenv.nix`).

## Cloudflare JavaScript challenges (`webview` feature)

avoro sits behind a Cloudflare challenge that only a real browser engine passes. Build with `cargo build --release --features webview` to load it through the operating system's own web engine (WebKit on macOS/Linux, WebView2 on Windows) via [wry](https://github.com/tauri-apps/wry), in-process from a small off-screen window. When Cloudflare asks for its interactive "Verify you are human" check, the window moves on-screen and `fetch` waits up to 2 minutes for you to click it; the clearance cookie is kept by the OS web engine, so later runs pass silently until it expires. The check is never clicked automatically. Non-interactive runs (cron/CI) skip the provider with a message instead of waiting. Everything stays hidden (no window, no Dock icon) unless the click is needed; then the window comes to the front of an interactive shell. On Linux this needs WebKitGTK and a display. `BMF_WEBVIEW_DEBUG=1` shows the window from the start; `BMF_WEBVIEW_FRESH=1` ignores saved cookies to force the check.

To add another challenged site, list it in `WEBVIEW_PAGES` in `src/providers/mod.rs` (provider, URL, JS expression that is true once the real content loaded) and read the HTML in the provider with `providers::prefetched(URL)`.

## Caveats

- Prices are monthly, converted to EUR with ECB rates. Most are excl. VAT; where a site only shows gross prices VAT is stripped (noted in `note`). Latitude, Clouvider, PebbleHost and server4you may include tax.
- Several providers show promo or long-term prices on listings; the tool prefers month-to-month prices (e.g. Alpenhost order pages, Psychz renewal price). IONOS prices are first-year promos (regular price in `note`).
- CPU scores (`score_source`): `passmark_exact` / `passmark_token` (PassMark, matched by name or model number incl. spelling variants like "EPYC 9355" → "9355P", "Xeon-D 2141I" → "D-2141I"), `provider` / `provider_pooled` (a score the provider publishes, shared across providers with the same CPU), `estimate_sibling` (closest PassMark model in the same family with a similar core count, scaled per core; `passmark_name` starts with `~`). Multi-socket boxes use PassMark's measured multi-CPU score when available, otherwise ~1.7× per extra socket.
- `ram_type` (DDR3/4/5) comes from the listing text when present (`ram_type_source = 'provider'`), otherwise from the CPU platform (`'cpu'`, e.g. AM5/SP5 are DDR5-only). `nvme_gen` (PCIe 3/4/5) is only set when the provider states it.
- Scrapers break when sites change. `fetch` keeps going when one provider fails and records the error in `fetch_runs`; `top` keeps using that provider's last good snapshot.
- Be nice: run `fetch` occasionally, not in a tight loop.

## License

MIT
