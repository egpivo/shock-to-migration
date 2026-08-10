//! Frozen daily market-response CSV ingest (generic; not venue-specific).
//!
//! Canonical columns (aliases accepted):
//! - `asset_key` (or `instrument_id` — must match a project asset key)
//! - `day` (or `timestamp` — date or RFC3339/datetime; calendar day is used)
//! - `price` (optional; empty → missing, never zero)
//! - `volume` (optional; empty → missing, never zero)
//!
//! Duplicate `(asset_key, day)` rows are a hard ingest error.

use std::collections::HashSet;
use std::fs::File;
use std::path::Path;
use std::str::FromStr;

use chrono::{DateTime, NaiveDate, Utc};
use rust_decimal::Decimal;
use serde::Deserialize;
use thiserror::Error;

use crate::identity::AssetKey;
use crate::response::ResponseObservation;

#[derive(Debug, Error)]
pub enum ResponseIngestError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("csv error: {0}")]
    Csv(#[from] csv::Error),
    #[error("parse error in {path}: {message}")]
    Parse { path: String, message: String },
}

#[derive(Debug, Deserialize)]
struct RawRow {
    #[serde(default)]
    asset_key: Option<String>,
    #[serde(default)]
    instrument_id: Option<String>,
    #[serde(default)]
    day: Option<String>,
    #[serde(default)]
    timestamp: Option<String>,
    price: Option<String>,
    volume: Option<String>,
}

pub fn load_daily_response(path: &Path) -> Result<Vec<ResponseObservation>, ResponseIngestError> {
    let file = File::open(path)?;
    let mut reader = csv::Reader::from_reader(file);
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    let path_str = path.display().to_string();

    for (idx, row) in reader.deserialize::<RawRow>().enumerate() {
        let row = row?;
        let key_raw = row
            .asset_key
            .as_deref()
            .or(row.instrument_id.as_deref())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| ResponseIngestError::Parse {
                path: path_str.clone(),
                message: format!(
                    "row {}: missing asset_key/instrument_id (project asset key required)",
                    idx + 2
                ),
            })?;
        let day = parse_day(row.day.as_deref(), row.timestamp.as_deref(), &path_str, idx)?;
        let price = parse_opt_decimal(row.price.as_deref(), &path_str, idx, "price")?;
        let volume = parse_opt_decimal(row.volume.as_deref(), &path_str, idx, "volume")?;
        let asset_key = AssetKey::new(key_raw);
        if !seen.insert((asset_key.clone(), day)) {
            return Err(ResponseIngestError::Parse {
                path: path_str,
                message: format!(
                    "row {}: duplicate observation for asset '{}' on {day}",
                    idx + 2,
                    asset_key
                ),
            });
        }
        out.push(ResponseObservation {
            asset_key,
            day,
            price,
            volume,
        });
    }
    Ok(out)
}

fn parse_day(
    day: Option<&str>,
    timestamp: Option<&str>,
    path: &str,
    idx: usize,
) -> Result<NaiveDate, ResponseIngestError> {
    let day_raw = day.map(str::trim).filter(|s| !s.is_empty());
    let ts_raw = timestamp.map(str::trim).filter(|s| !s.is_empty());

    match (day_raw, ts_raw) {
        (None, None) => Err(ResponseIngestError::Parse {
            path: path.to_string(),
            message: format!("row {}: missing day/timestamp", idx + 2),
        }),
        (Some(d), None) => parse_day_value(d, path, idx),
        (None, Some(ts)) => parse_timestamp_value(ts, path, idx),
        (Some(d), Some(ts)) => {
            let from_day = parse_day_value(d, path, idx)?;
            let from_ts = parse_timestamp_value(ts, path, idx)?;
            if from_day != from_ts {
                return Err(ResponseIngestError::Parse {
                    path: path.to_string(),
                    message: format!(
                        "row {}: conflicting day '{d}' and timestamp '{ts}' (resolve to {from_day} vs {from_ts})",
                        idx + 2
                    ),
                });
            }
            Ok(from_day)
        }
    }
}

fn parse_day_value(raw: &str, path: &str, idx: usize) -> Result<NaiveDate, ResponseIngestError> {
    NaiveDate::from_str(raw).map_err(|e| ResponseIngestError::Parse {
        path: path.to_string(),
        message: format!("row {}: invalid day '{raw}': {e}", idx + 2),
    })
}

fn parse_timestamp_value(
    raw: &str,
    path: &str,
    idx: usize,
) -> Result<NaiveDate, ResponseIngestError> {
    if let Ok(d) = NaiveDate::from_str(raw) {
        return Ok(d);
    }
    if let Ok(dt) = DateTime::parse_from_rfc3339(raw) {
        return Ok(dt.with_timezone(&Utc).date_naive());
    }
    if let Ok(dt) = DateTime::parse_from_str(raw, "%Y-%m-%d %H:%M:%S%z") {
        return Ok(dt.with_timezone(&Utc).date_naive());
    }
    if let Ok(naive) = chrono::NaiveDateTime::parse_from_str(raw, "%Y-%m-%dT%H:%M:%S") {
        return Ok(naive.date());
    }
    if let Ok(naive) = chrono::NaiveDateTime::parse_from_str(raw, "%Y-%m-%d %H:%M:%S") {
        return Ok(naive.date());
    }
    Err(ResponseIngestError::Parse {
        path: path.to_string(),
        message: format!("row {}: invalid day/timestamp '{raw}'", idx + 2),
    })
}

fn parse_opt_decimal(
    raw: Option<&str>,
    path: &str,
    idx: usize,
    field: &str,
) -> Result<Option<Decimal>, ResponseIngestError> {
    let Some(s) = raw.map(str::trim).filter(|s| !s.is_empty()) else {
        return Ok(None);
    };
    Decimal::from_str(s)
        .map(Some)
        .map_err(|e| ResponseIngestError::Parse {
            path: path.to_string(),
            message: format!("row {}: invalid {field} '{s}': {e}", idx + 2),
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn write_csv(body: &str) -> std::path::PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        static N: AtomicU64 = AtomicU64::new(0);
        let n = N.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "shocktrace-response-ingest-{}-{:?}-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            std::thread::current().id(),
            n
        ));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("response.csv");
        fs::write(&path, body).unwrap();
        path
    }

    #[test]
    fn accepts_timestamp_alias_and_rejects_duplicate_day() {
        let path = write_csv(
            "instrument_id,timestamp,price,volume\nGC,2026-06-12T14:00:00Z,100,1\nGC,2026-06-12,101,2\n",
        );
        let err = load_daily_response(&path).unwrap_err();
        assert!(err.to_string().contains("duplicate"));
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn empty_price_stays_none() {
        let path = write_csv("asset_key,day,price,volume\nGC,2026-06-12,,10\n");
        let rows = load_daily_response(&path).unwrap();
        assert!(rows[0].price.is_none());
        assert_eq!(rows[0].volume.unwrap().to_string(), "10");
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn rejects_conflicting_day_and_timestamp() {
        let path = write_csv(
            "asset_key,day,timestamp,price,volume\nGC,2026-06-12,2026-06-13T00:00:00Z,100,1\n",
        );
        let err = load_daily_response(&path).unwrap_err();
        assert!(err.to_string().contains("conflicting"), "got: {err}");
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn accepts_agreeing_day_and_timestamp() {
        let path = write_csv(
            "asset_key,day,timestamp,price,volume\nGC,2026-06-12,2026-06-12T14:00:00Z,100,1\n",
        );
        let rows = load_daily_response(&path).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].day.to_string(), "2026-06-12");
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }
}
