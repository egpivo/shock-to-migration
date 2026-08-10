//! Download-and-freeze helper for the gold/oil response demo.
//!
//! Fetches daily OHLCV from the public Yahoo Finance chart API and writes
//! shocktrace-compatible `response_daily.csv` files
//! (`asset_key,day,price,volume`) into `projects/gold/data/` and
//! `projects/oil/data/`.
//!
//! Instrument choice (see `.local/docs/GOLD_OIL_CASE.md`):
//! - Oil: `CL=F` (NYMEX WTI continuous). Volume is a plausible order of
//!   magnitude.
//! - Gold: `GLD` ETF, **not** `GC=F`. Yahoo's GC=F volume feed is not
//!   credible for this window (median ~430 "contracts"/day).
//!
//! This binary does not touch the shocktrace engine. Re-running
//! **overwrites** the frozen CSVs — only do that deliberately.
//!
//! ```text
//! cargo run -p fetch-gold-oil -- --start 2024-05-01 --end 2025-10-15
//! ```

use std::fs;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;

use chrono::{DateTime, NaiveDate, TimeZone, Utc};
use chrono_tz::Tz;
use clap::Parser;
use serde::Deserialize;
use thiserror::Error;

const USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 \
    (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36";

const CHART_URL: &str = "https://query2.finance.yahoo.com/v8/finance/chart";

#[derive(Debug, Parser)]
#[command(about = "Freeze Yahoo daily OHLCV into projects/{gold,oil}/data/response_daily.csv")]
struct Args {
    /// Start date YYYY-MM-DD (UTC epoch bound).
    #[arg(long, default_value = "2024-05-01")]
    start: NaiveDate,
    /// End date YYYY-MM-DD (UTC epoch bound).
    #[arg(long, default_value = "2025-10-15")]
    end: NaiveDate,
    /// Restrict to one Yahoo symbol (e.g. GLD). Repeatable. Default: both.
    #[arg(long = "symbol")]
    symbols: Vec<String>,
}

#[derive(Debug, Clone, Copy)]
struct Target {
    yahoo_symbol: &'static str,
    asset_key: &'static str,
    project_dir: &'static str,
}

const TARGETS: &[Target] = &[
    Target {
        yahoo_symbol: "GLD",
        asset_key: "GLD",
        project_dir: "projects/gold/data",
    },
    Target {
        yahoo_symbol: "CL=F",
        asset_key: "CL",
        project_dir: "projects/oil/data",
    },
];

#[derive(Debug, Error)]
enum FetchError {
    #[error("http error for {symbol}: {message}")]
    Http { symbol: String, message: String },
    #[error("json parse error for {0}: {1}")]
    Json(String, #[source] serde_json::Error),
    #[error("unexpected chart payload for {0}")]
    Payload(String),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("csv error: {0}")]
    Csv(#[from] csv::Error),
}

#[derive(Debug, Deserialize)]
struct ChartResponse {
    chart: ChartBody,
}

#[derive(Debug, Deserialize)]
struct ChartBody {
    result: Option<Vec<ChartResult>>,
}

#[derive(Debug, Deserialize)]
struct ChartResult {
    timestamp: Option<Vec<i64>>,
    meta: ChartMeta,
    indicators: ChartIndicators,
}

#[derive(Debug, Deserialize)]
struct ChartMeta {
    #[serde(rename = "exchangeTimezoneName")]
    exchange_timezone_name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ChartIndicators {
    quote: Vec<QuoteBlock>,
}

#[derive(Debug, Deserialize)]
struct QuoteBlock {
    close: Vec<Option<f64>>,
    volume: Vec<Option<f64>>,
}

#[derive(Debug)]
struct Row {
    asset_key: String,
    day: NaiveDate,
    price: String,
    volume: String,
}

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap_or_else(|_| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../.."))
}

fn fetch_chart_json(symbol: &str, period1: i64, period2: i64) -> Result<ChartResponse, FetchError> {
    let encoded = urlencoding_minimal(symbol);
    let url = format!("{CHART_URL}/{encoded}?period1={period1}&period2={period2}&interval=1d");
    let mut last_message = String::new();
    for attempt in 0..4 {
        let response = ureq::get(&url)
            .set("User-Agent", USER_AGENT)
            .timeout(Duration::from_secs(15))
            .call();
        match response {
            Ok(resp) => {
                let text = resp.into_string().map_err(|e| FetchError::Http {
                    symbol: symbol.into(),
                    message: e.to_string(),
                })?;
                return serde_json::from_str(&text).map_err(|e| FetchError::Json(symbol.into(), e));
            }
            Err(ureq::Error::Status(code, resp)) => {
                last_message = format!("HTTP {code}: {}", resp.status_text());
                let wait = Duration::from_secs(5 * (attempt + 1) as u64);
                eprintln!(
                    "  [{symbol}] {last_message} on attempt {}/4; retrying in {}s",
                    attempt + 1,
                    wait.as_secs()
                );
                thread::sleep(wait);
            }
            Err(e) => {
                last_message = e.to_string();
                let wait = Duration::from_secs(5 * (attempt + 1) as u64);
                eprintln!(
                    "  [{symbol}] network error on attempt {}/4: {last_message}; retrying in {}s",
                    attempt + 1,
                    wait.as_secs()
                );
                thread::sleep(wait);
            }
        }
    }
    Err(FetchError::Http {
        symbol: symbol.into(),
        message: last_message,
    })
}

/// Minimal query-path encoding for Yahoo symbols (`CL=F` → `CL%3DF`).
fn urlencoding_minimal(symbol: &str) -> String {
    let mut out = String::with_capacity(symbol.len());
    for b in symbol.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char);
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

fn chart_to_rows(payload: ChartResponse, asset_key: &str) -> Result<Vec<Row>, FetchError> {
    let result = payload
        .chart
        .result
        .and_then(|mut v| v.pop())
        .ok_or_else(|| FetchError::Payload(asset_key.into()))?;
    let timestamps = result
        .timestamp
        .ok_or_else(|| FetchError::Payload(asset_key.into()))?;
    let quote = result
        .indicators
        .quote
        .into_iter()
        .next()
        .ok_or_else(|| FetchError::Payload(asset_key.into()))?;

    let tz: Tz = match result.meta.exchange_timezone_name.as_deref() {
        Some(name) if name.contains("America") => chrono_tz::America::New_York,
        Some(name) => name.parse().unwrap_or(chrono_tz::UTC),
        None => chrono_tz::UTC,
    };

    let mut rows = Vec::new();
    let mut dropped = 0usize;
    for (i, ts) in timestamps.iter().enumerate() {
        let close = quote.close.get(i).copied().flatten();
        let volume = quote.volume.get(i).copied().flatten();
        let (Some(close), Some(volume)) = (close, volume) else {
            dropped += 1;
            continue;
        };
        // Yahoo daily timestamps are session-open epoch seconds; key on the
        // exchange-local calendar date to avoid UTC off-by-one.
        let day = Utc
            .timestamp_opt(*ts, 0)
            .single()
            .ok_or_else(|| FetchError::Payload(asset_key.into()))?
            .with_timezone(&tz)
            .date_naive();
        rows.push(Row {
            asset_key: asset_key.to_string(),
            day,
            price: format!("{close:.4}"),
            volume: format!("{}", volume as i64),
        });
    }
    if dropped > 0 {
        eprintln!(
            "  [{asset_key}] dropped {dropped} day(s) with missing price/volume \
             (missing stays missing, not zero-filled)"
        );
    }
    Ok(rows)
}

fn write_csv(path: &Path, rows: &[Row]) -> Result<(), FetchError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut wtr = csv::Writer::from_path(path)?;
    wtr.write_record(["asset_key", "day", "price", "volume"])?;
    for row in rows {
        wtr.write_record([
            row.asset_key.as_str(),
            &row.day.to_string(),
            row.price.as_str(),
            row.volume.as_str(),
        ])?;
    }
    wtr.flush()?;
    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    let root = repo_root();

    let start_dt: DateTime<Utc> = args.start.and_hms_opt(0, 0, 0).unwrap().and_utc();
    let end_dt: DateTime<Utc> = args.end.and_hms_opt(0, 0, 0).unwrap().and_utc();
    let period1 = start_dt.timestamp();
    let period2 = end_dt.timestamp();

    let targets: Vec<&Target> = if args.symbols.is_empty() {
        TARGETS.iter().collect()
    } else {
        let wanted: std::collections::BTreeSet<_> = args.symbols.iter().cloned().collect();
        TARGETS
            .iter()
            .filter(|t| wanted.contains(t.yahoo_symbol))
            .collect()
    };

    let mut failures = Vec::new();
    for (i, target) in targets.iter().enumerate() {
        println!(
            "Fetching {} ({} .. {}) ...",
            target.yahoo_symbol, args.start, args.end
        );
        match fetch_chart_json(target.yahoo_symbol, period1, period2)
            .and_then(|payload| chart_to_rows(payload, target.asset_key))
        {
            Ok(rows) => {
                let out_path = root.join(target.project_dir).join("response_daily.csv");
                write_csv(&out_path, &rows)?;
                let rel = out_path.strip_prefix(&root).unwrap_or(&out_path);
                println!("  wrote {} rows -> {}", rows.len(), rel.display());
            }
            Err(e) => {
                eprintln!("  FAILED: {}: {e}", target.yahoo_symbol);
                failures.push(target.yahoo_symbol);
            }
        }
        if i + 1 < targets.len() {
            // Avoid tripping Yahoo's rate limiter between symbols.
            thread::sleep(Duration::from_secs(15));
        }
    }

    if !failures.is_empty() {
        eprintln!(
            "\n{} symbol(s) failed to download: {failures:?}. \
             Existing frozen CSVs (if any) were left untouched for those assets.",
            failures.len()
        );
        std::process::exit(1);
    }

    println!(
        "\nDone. Remember: re-running this OVERWRITES the frozen CSVs with a fresh \
         download, which defeats the point of a frozen demo. Only re-run deliberately."
    );
    Ok(())
}
