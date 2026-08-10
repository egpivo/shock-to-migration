//! Declared routes, per-route measurement config, and multi-dimensional route evidence.
//!
//! Documentation, executability, observation, and linkage are separate fields.
//! None of them alone establishes net migration.
//!
//! Flow unit / denominator live on each route — not on a global `[flow]` table.

use chrono::NaiveDate;
use serde::{Deserialize, Serialize};

use crate::flow::AttributionMethod;
use crate::identity::AssetKey;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RouteMechanism {
    SwapPair,
    IssuerConversion,
    Bridge,
    BurnMint,
    Unknown,
    Other(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DocStatus {
    IssuerNamed,
    DocsMentioned,
    None,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecStatus {
    Permissionless,
    Gated,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ObservedStatus {
    Yes,
    NoInWindow,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LinkageClass {
    /// Same transaction / same actor legs.
    DirectSameTx,
    /// Matched on mint/asset pair (not necessarily issuer-operated).
    MintPairMatch,
    /// Issuer burn on A and mint on B (possibly off-chain join).
    IssuerTwoLeg,
    Unlinked,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Restriction {
    Kyc,
    MinSize,
    Jurisdiction,
    Deadline,
    Whitelist,
    Other(String),
}

/// Which route endpoint the flow quantities are denominated on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MeasuredLeg {
    Source,
    Destination,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FlowUnitConfig {
    TokenNative,
    QuoteUsd,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum DenominatorPolicy {
    SupplySnapshot { asset: AssetKey, as_of: NaiveDate },
}

/// Per-route measurement contract: unit, measured leg, attribution, denominator.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RouteMeasurement {
    pub unit: FlowUnitConfig,
    pub unit_asset: AssetKey,
    pub measured_leg: MeasuredLeg,
    pub attribution: AttributionMethod,
    pub denominator: Option<DenominatorPolicy>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Route {
    pub id: String,
    pub source: AssetKey,
    pub destination: AssetKey,
    pub mechanism: RouteMechanism,
    pub measurement: RouteMeasurement,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RouteEvidence {
    pub route_id: String,
    pub documented: DocStatus,
    pub technically_executable: ExecStatus,
    pub observed_on_chain: ObservedStatus,
    pub restrictions: Vec<Restriction>,
    pub linkage_class: LinkageClass,
    pub notes: Option<String>,
}

impl Route {
    /// Asset key implied by `measured_leg`.
    pub fn measured_asset(&self) -> &AssetKey {
        match self.measurement.measured_leg {
            MeasuredLeg::Source => &self.source,
            MeasuredLeg::Destination => &self.destination,
        }
    }
}
