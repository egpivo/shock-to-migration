//! Daily directional flow CSV ingest.
//!
//! Rows are route-agnostic quantities. Unit and attribution are stamped from
//! each route's `measurement` config during analysis.

use std::fs::File;
use std::path::Path;
use std::str::FromStr;

use chrono::NaiveDate;
use rust_decimal::Decimal;
use serde::Deserialize;
use thiserror::Error;

use crate::flow::Quantity;

#[derive(Debug, Error)]
pub enum IngestError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("csv error: {0}")]
    Csv(#[from] csv::Error),
    #[error("parse error in {path}: {message}")]
    Parse { path: String, message: String },
}

#[derive(Debug, Deserialize)]
pub struct DailyFlowRow {
    pub route_id: String,
    pub day: String,
    pub gross_a_to_b: String,
    pub gross_b_to_a: String,
}

/// Parsed flow row before unit/attribution are applied from route measurement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawDailyFlow {
    pub route_id: String,
    pub day: NaiveDate,
    pub gross_a_to_b: Quantity,
    pub gross_b_to_a: Quantity,
}

pub fn load_daily_flow_rows(path: &Path) -> Result<Vec<RawDailyFlow>, IngestError> {
    let file = File::open(path)?;
    let mut reader = csv::Reader::from_reader(file);
    let mut out = Vec::new();
    let path_str = path.display().to_string();

    for (idx, row) in reader.deserialize::<DailyFlowRow>().enumerate() {
        let row = row?;
        let day = NaiveDate::from_str(&row.day).map_err(|e| IngestError::Parse {
            path: path_str.clone(),
            message: format!("row {}: invalid day '{}': {e}", idx + 2, row.day),
        })?;
        let a_to_b = parse_qty(&row.gross_a_to_b, &path_str, idx)?;
        let b_to_a = parse_qty(&row.gross_b_to_a, &path_str, idx)?;
        out.push(RawDailyFlow {
            route_id: row.route_id,
            day,
            gross_a_to_b: a_to_b,
            gross_b_to_a: b_to_a,
        });
    }

    Ok(out)
}

fn parse_qty(raw: &str, path: &str, idx: usize) -> Result<Quantity, IngestError> {
    let value = Decimal::from_str(raw.trim()).map_err(|e| IngestError::Parse {
        path: path.to_string(),
        message: format!("row {}: invalid quantity '{raw}': {e}", idx + 2),
    })?;
    Quantity::new(value).map_err(|e| IngestError::Parse {
        path: path.to_string(),
        message: format!("row {}: {e}", idx + 2),
    })
}
