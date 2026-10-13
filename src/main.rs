mod db;
mod fx;
mod geo;
mod hw;
mod model;
mod passmark;
mod providers;
mod specs;
mod util;
#[cfg(feature = "webview")]
mod webview;
mod whmcs;

use anyhow::Result;
use clap::{Parser, Subcommand, ValueEnum};
use duckdb::arrow::record_batch::RecordBatch;
use duckdb::arrow::util::pretty::pretty_format_batches;

#[derive(Parser)]
#[command(about = "Find the best price/performance bare-metal servers that are in stock right now")]
struct Cli {
    /// DuckDB database file
    #[arg(long, global = true, default_value = "servers.duckdb")]
    db: String,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Fetch all providers + PassMark scores and store a snapshot
    Fetch {
        /// Only these providers (comma separated)
        #[arg(long, value_delimiter = ',')]
        provider: Vec<String>,
    },
    /// Rank offers from the latest snapshot
    Top(TopArgs),
    /// List provider names
    Providers,
}

#[derive(Parser)]
struct TopArgs {
    /// Only EU-27 locations
    #[arg(long)]
    eu: bool,
    /// Only European locations (EU + UK/NO/CH …)
    #[arg(long, conflicts_with = "eu")]
    europe: bool,
    /// Also show out-of-stock and unknown-stock offers
    #[arg(long)]
    any_stock: bool,
    /// Include offers whose stock is unknown (default: only confirmed in stock)
    #[arg(long)]
    include_unknown: bool,
    /// Only deliverable within ~1 hour (drops OVH 24H/72H and Hetzner build-to-order)
    #[arg(long)]
    now: bool,
    #[arg(long)]
    min_cores: Option<u32>,
    #[arg(long)]
    min_ram: Option<f64>,
    /// Minimum total disk in GB
    #[arg(long)]
    min_storage: Option<f64>,
    #[arg(long)]
    nvme: bool,
    /// Minimum PassMark single-thread score
    #[arg(long)]
    min_single: Option<f64>,
    /// Only this RAM generation, e.g. DDR5 (stated by provider or implied by CPU)
    #[arg(long)]
    ram_type: Option<String>,
    /// Max effective monthly EUR (setup amortized over 12 months)
    #[arg(long)]
    max_price: Option<f64>,
    #[arg(long, value_delimiter = ',')]
    provider: Vec<String>,
    #[arg(long, value_enum, default_value = "value")]
    sort: Sort,
    #[arg(long, default_value_t = 25)]
    limit: usize,
}

#[derive(Clone, Copy, ValueEnum)]
enum Sort {
    /// PassMark per EUR
    Value,
    /// Raw PassMark multi-thread
    Mark,
    /// PassMark single-thread
    Single,
    /// Cheapest first
    Price,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::Fetch { provider } => {
            // The OS web engine needs the main thread, so it runs before tokio.
            #[cfg(feature = "webview")]
            providers::prefetch_webview_pages(&provider);
            tokio::runtime::Runtime::new()?.block_on(fetch(&cli.db, &provider))
        }
        Cmd::Top(args) => top(&cli.db, &args),
        Cmd::Providers => {
            for (name, _) in providers::ALL {
                println!("{name}");
            }
            Ok(())
        }
    }
}

async fn fetch(db_path: &str, only: &[String]) -> Result<()> {
    let client = util::client()?;
    let selected: Vec<_> = providers::ALL
        .iter()
        .filter(|(n, _)| only.is_empty() || only.iter().any(|o| o == n))
        .collect();
    anyhow::ensure!(!selected.is_empty(), "no such provider; see `providers`");

    let (cpus, rates, results) = tokio::join!(
        passmark::fetch(),
        fx::ecb_rates(&client),
        futures::future::join_all(selected.iter().map(|(name, f)| {
            let client = client.clone();
            async move { (*name, f(&client).await) }
        })),
    );
    let rates = rates?;
    let conn = db::open(db_path)?;
    // Fall back to the scores cached by the previous run.
    let cpus = match cpus {
        Ok(c) => {
            db::save_cpu_scores(&conn, &c)?;
            c
        }
        Err(e) => {
            eprintln!("passmark: {e:#}; using cached scores");
            db::load_cpu_scores(&conn)?
        }
    };
    eprintln!("passmark: {} cpus, USD/EUR {:.4}", cpus.len(), rates["USD"]);
    let index = passmark::Index::new(&cpus);
    let pooled = db::pool_provider_marks(&results);
    let at = chrono::Utc::now();
    for (name, res) in &results {
        match res {
            Ok(v) => eprintln!("{name:>16}: {} offers", v.len()),
            Err(e) => eprintln!("{name:>16}: FAILED {e:#}"),
        }
        db::save_run(&conn, at, name, res, &index, &pooled, &rates)?;
    }
    let unmatched: Vec<String> = conn
        .prepare("SELECT DISTINCT provider || ': ' || cpu FROM offers WHERE fetched_at = ? AND cpu_mark IS NULL ORDER BY 1")?
        .query_map([at], |r| r.get(0))?
        .collect::<Result<_, _>>()?;
    if !unmatched.is_empty() {
        eprintln!("no benchmark score for: {}", unmatched.join("; "));
    }
    Ok(())
}

fn top(db_path: &str, a: &TopArgs) -> Result<()> {
    let conn = db::open(db_path)?;
    let mut filters = vec!["true".to_string()];
    if a.eu {
        filters.push("is_eu".into());
    }
    if a.europe {
        filters.push("is_europe".into());
    }
    if !a.any_stock {
        filters.push(
            if a.include_unknown {
                "stock <> 'out_of_stock'"
            } else {
                "stock = 'in_stock'"
            }
            .into(),
        );
    }
    if a.now {
        filters.push("coalesce(availability, '') NOT IN ('24H', '72H', '120H', '240H', '480H', 'build-to-order')".into());
    }
    if let Some(v) = a.min_cores {
        filters.push(format!("cores >= {v}"));
    }
    if let Some(v) = a.min_ram {
        filters.push(format!("ram_gb >= {v}"));
    }
    if let Some(v) = a.min_storage {
        filters.push(format!("storage_gb >= {v}"));
    }
    if a.nvme {
        filters.push("nvme".into());
    }
    if let Some(v) = &a.ram_type {
        filters.push(format!(
            "ram_type = '{}'",
            v.to_uppercase().replace('\'', "")
        ));
    }
    if let Some(v) = a.min_single {
        filters.push(format!("single_thread >= {v}"));
    }
    if let Some(v) = a.max_price {
        filters.push(format!("effective_monthly_eur <= {v}"));
    }
    if !a.provider.is_empty() {
        let list: Vec<String> = a
            .provider
            .iter()
            .map(|p| format!("'{}'", p.replace('\'', "")))
            .collect();
        filters.push(format!("provider IN ({})", list.join(",")));
    }
    let order = match a.sort {
        // Output column aliases: the query groups, so only selected columns sort.
        Sort::Value => "mark_eur DESC NULLS LAST",
        Sort::Mark => "mark DESC NULLS LAST",
        Sort::Single => "st DESC NULLS LAST, mark_eur DESC NULLS LAST",
        Sort::Price => "eur_mo, setup",
    };
    // One row per provider+model+price: OVH/Hetzner repeat offers per datacenter.
    let sql = format!(
        "SELECT provider, name, cpu, cores AS c, ram_gb::INT AS ram, ram_type AS mem, storage_gb::INT AS disk_gb,
                coalesce('gen' || nvme_gen, CASE WHEN nvme THEN 'nvme' END) AS ssd,
                round(price_monthly_eur, 2) AS eur_mo, round(setup_fee_eur, 0) AS setup,
                cpu_mark::INT AS mark, single_thread::INT AS st, mark_per_eur AS mark_eur,
                string_agg(DISTINCT country || ':' || location, ' ' ORDER BY country || ':' || location) AS where,
                any_value(coalesce(availability, stock)) AS avail
         FROM ranked WHERE {}
         GROUP BY ALL
         ORDER BY {order}
         LIMIT {}",
        filters.join(" AND "),
        a.limit
    );
    let batches: Vec<RecordBatch> = conn.prepare(&sql)?.query_arrow([])?.collect();
    println!("{}", pretty_format_batches(&batches)?);
    let snap: Option<String> = conn
        .query_row(
            "SELECT max(fetched_at)::VARCHAR FROM fetch_runs WHERE ok",
            [],
            |r| r.get(0),
        )
        .ok()
        .flatten();
    eprintln!(
        "snapshot: {}",
        snap.unwrap_or_else(|| "none — run `fetch` first".into())
    );
    Ok(())
}
