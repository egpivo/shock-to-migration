//! Daily market-response CSV ingest.

use std::fs::File;
use std::path::Path;
use std::str::FromStr;

use chrono::NaiveDate;
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
    asset_key: String,
    day: String,
    price: Option<String>,
    volume: Option<String>,
}

pub fn load_daily_response(path: &Path) -> Result<Vec<ResponseObservation>, ResponseIngestError> {
    let file = File::open(path)?;
    let mut reader = csv::Reader::from_reader(file);
    let mut out = Vec::new();
    let path_str = path.display().to_string();

    for (idx, row) in reader.deserialize::<RawRow>().enumerate() {
        let row = row?;
        let day = NaiveDate::from_str(&row.day).map_err(|e| ResponseIngestError::Parse {
            path: path_str.clone(),
            message: format!("row {}: invalid day '{}': {e}", idx + 2, row.day),
        })?;
        let price = parse_opt_decimal(row.price.as_deref(), &path_str, idx, "price")?;
        let volume = parse_opt_decimal(row.volume.as_deref(), &path_str, idx, "volume")?;
        out.push(ResponseObservation {
            asset_key: AssetKey::new(row.asset_key),
            day,
            price,
            volume,
        });
    }
    Ok(out)
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
