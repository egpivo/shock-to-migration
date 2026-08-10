//! Control-group declarations. Comparison is not causal identification.

use serde::{Deserialize, Serialize};

use crate::identity::AssetKey;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ControlRelation {
    SameIssuerDifferentUnderlying,
    SameUnderlyingDifferentIssuer,
    SameChainVenue,
    SimilarWrapperType,
    Unaffected,
    Other(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ControlAsset {
    pub key: AssetKey,
    pub relation: ControlRelation,
}
