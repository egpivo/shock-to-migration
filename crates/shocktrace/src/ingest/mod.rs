//! File-based ingest adapters. No live APIs in v0.1.

pub mod daily_flows;
pub mod daily_supply;

pub use daily_flows::{load_daily_flow_rows, DailyFlowRow, RawDailyFlow};
pub use daily_supply::{load_daily_supply, DailySupplyRow};
