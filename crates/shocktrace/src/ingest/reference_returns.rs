//! Frozen event-day reference returns used by `measure passthrough`.
//!
//! These are source-reported comparison points, not full price tapes. Each
//! row binds one named reference to one declared token asset and records the
//! source/cutoff needed to interpret the same-date response gap.

use std::collections::HashSet;
use std::fs::File;
use std::path::Path;
use std::str::FromStr;

use chrono::NaiveDate;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::identity::AssetKey;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReferenceReturnObservation {
    pub reference_key: String,
    pub asset_key: AssetKey,
    pub day: NaiveDate,
    pub reference_return: Decimal,
    pub source_label: String,
    pub source_url: String,
    pub cutoff: String,
}

#[derive(Debug, Error)]
pub enum ReferenceReturnIngestError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("csv error: {0}")]
    Csv(#[from] csv::Error),
    #[error("parse error in {path}: {message}")]
    Parse { path: String, message: String },
}

#[derive(Debug, Deserialize)]
struct RawReferenceReturn {
    reference_key: String,
    asset_key: String,
    day: String,
    reference_return: String,
    source_label: String,
    source_url: String,
    cutoff: String,
}

pub fn load_reference_returns(
    path: &Path,
) -> Result<Vec<ReferenceReturnObservation>, ReferenceReturnIngestError> {
    let file = File::open(path)?;
    let mut reader = csv::Reader::from_reader(file);
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    let path_str = path.display().to_string();

    for (idx, row) in reader.deserialize::<RawReferenceReturn>().enumerate() {
        let row = row?;
        let row_number = idx + 2;
        let reference_key = required(&row.reference_key, "reference_key", &path_str, row_number)?;
        let asset_key = required(&row.asset_key, "asset_key", &path_str, row_number)?;
        let source_label = required(&row.source_label, "source_label", &path_str, row_number)?;
        let source_url = required(&row.source_url, "source_url", &path_str, row_number)?;
        let cutoff = required(&row.cutoff, "cutoff", &path_str, row_number)?;
        let day =
            NaiveDate::from_str(row.day.trim()).map_err(|e| ReferenceReturnIngestError::Parse {
                path: path_str.clone(),
                message: format!("row {row_number}: invalid day '{}': {e}", row.day),
            })?;
        let reference_return = Decimal::from_str(row.reference_return.trim()).map_err(|e| {
            ReferenceReturnIngestError::Parse {
                path: path_str.clone(),
                message: format!(
                    "row {row_number}: invalid reference_return '{}': {e}",
                    row.reference_return
                ),
            }
        })?;

        let key = (reference_key.clone(), AssetKey::new(asset_key.clone()), day);
        if !seen.insert(key.clone()) {
            return Err(ReferenceReturnIngestError::Parse {
                path: path_str,
                message: format!(
                    "row {row_number}: duplicate reference '{}' for asset '{}' on {day}",
                    key.0, key.1
                ),
            });
        }

        out.push(ReferenceReturnObservation {
            reference_key,
            asset_key: AssetKey::new(asset_key),
            day,
            reference_return,
            source_label,
            source_url,
            cutoff,
        });
    }
    Ok(out)
}

fn required(
    raw: &str,
    field: &str,
    path: &str,
    row: usize,
) -> Result<String, ReferenceReturnIngestError> {
    let value = raw.trim();
    if value.is_empty() {
        return Err(ReferenceReturnIngestError::Parse {
            path: path.to_string(),
            message: format!("row {row}: empty {field}"),
        });
    }
    Ok(value.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn write_csv(body: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "shocktrace-reference-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("reference_returns.csv");
        fs::write(&path, body).unwrap();
        path
    }

    #[test]
    fn loads_frozen_reference_return() {
        let path = write_csv(
            "reference_key,asset_key,day,reference_return,source_label,source_url,cutoff\n\
GOLD_SPOT,PAXG,2026-07-08,-0.0052,Reuters,https://example.test,source-reported close\n",
        );
        let rows = load_reference_returns(&path).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].reference_key, "GOLD_SPOT");
        assert_eq!(
            rows[0].reference_return,
            Decimal::from_str("-0.0052").unwrap()
        );
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn rejects_duplicate_reference_row() {
        let row = "GOLD_SPOT,PAXG,2026-07-08,-0.0052,Reuters,https://example.test,source-reported close\n";
        let path = write_csv(&format!(
            "reference_key,asset_key,day,reference_return,source_label,source_url,cutoff\n{row}{row}"
        ));
        let err = load_reference_returns(&path).unwrap_err();
        assert!(err.to_string().contains("duplicate reference"));
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }
}
