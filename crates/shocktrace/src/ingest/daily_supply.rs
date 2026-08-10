//! Daily supply snapshot CSV ingest.

use std::fs::File;
use std::path::Path;
use std::str::FromStr;

use chrono::NaiveDate;
use rust_decimal::Decimal;
use serde::Deserialize;
use thiserror::Error;

use crate::identity::AssetKey;

#[derive(Debug, Error)]
pub enum SupplyIngestError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("csv error: {0}")]
    Csv(#[from] csv::Error),
    #[error("parse error in {path}: {message}")]
    Parse { path: String, message: String },
    #[error("no supply row for asset '{asset}' on {day}")]
    MissingSnapshot { asset: String, day: NaiveDate },
}

#[derive(Debug, Clone, Deserialize)]
pub struct DailySupplyRow {
    pub asset_key: String,
    pub day: String,
    pub supply: String,
}

pub fn load_daily_supply(
    path: &Path,
) -> Result<Vec<(AssetKey, NaiveDate, Decimal)>, SupplyIngestError> {
    let file = File::open(path)?;
    let mut reader = csv::Reader::from_reader(file);
    let mut out = Vec::new();
    let path_str = path.display().to_string();

    for (idx, row) in reader.deserialize::<DailySupplyRow>().enumerate() {
        let row = row?;
        let day = NaiveDate::from_str(&row.day).map_err(|e| SupplyIngestError::Parse {
            path: path_str.clone(),
            message: format!("row {}: invalid day '{}': {e}", idx + 2, row.day),
        })?;
        let supply =
            Decimal::from_str(row.supply.trim()).map_err(|e| SupplyIngestError::Parse {
                path: path_str.clone(),
                message: format!("row {}: invalid supply '{}': {e}", idx + 2, row.supply),
            })?;
        out.push((AssetKey::new(row.asset_key), day, supply));
    }
    Ok(out)
}

pub fn supply_on(
    rows: &[(AssetKey, NaiveDate, Decimal)],
    asset: &AssetKey,
    day: NaiveDate,
) -> Result<Decimal, SupplyIngestError> {
    rows.iter()
        .find(|(k, d, _)| k == asset && *d == day)
        .map(|(_, _, s)| *s)
        .ok_or_else(|| SupplyIngestError::MissingSnapshot {
            asset: asset.as_str().to_string(),
            day,
        })
}
