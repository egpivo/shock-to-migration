//! Observations distinct from flow accounting.
//!
//! Volume, supply, depth, and trades are different measurements.
//! v0.1 wires daily flow CSV; these types exist so later response series
//! do not overload `DirectionalFlowObservation`.

use chrono::{DateTime, NaiveDate, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use crate::flow::Quantity;
use crate::identity::{AssetId, AssetKey, VenueId};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Trade {
    pub asset: AssetId,
    pub venue: Option<VenueId>,
    pub timestamp: DateTime<Utc>,
    pub notional_usd: Option<Decimal>,
    pub base_quantity: Option<Quantity>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SupplySnapshot {
    pub asset_key: AssetKey,
    pub day: NaiveDate,
    pub supply: Decimal,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DepthSnapshot {
    pub asset_key: AssetKey,
    pub venue: Option<VenueId>,
    pub timestamp: DateTime<Utc>,
    /// Executable size at a stated notional; `None` means unavailable (not zero).
    pub size_at_notional: Option<Decimal>,
    pub notional_usd: Option<Decimal>,
}
