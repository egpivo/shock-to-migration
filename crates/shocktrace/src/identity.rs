//! Asset and venue identity. Display tickers are never primary keys.

use serde::{Deserialize, Serialize};

/// Stable project-local handle used in config tables (`source = "A"`).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct AssetKey(pub String);

impl AssetKey {
    pub fn new(s: impl Into<String>) -> Self {
        Self(s.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for AssetKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ChainId(pub String);

impl ChainId {
    pub fn new(s: impl Into<String>) -> Self {
        Self(s.into())
    }
}

impl std::fmt::Display for ChainId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Canonical on-chain / venue locator. Ticker strings are not locators.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AssetLocator {
    Mint {
        address: String,
    },
    Erc20 {
        address: String,
    },
    CexSymbol {
        venue: String,
        symbol: String,
    },
    /// Conventional / non-chain instrument. `instrument_id` is venue-specific, not a ticker alias.
    MarketInstrument {
        venue: String,
        instrument_id: String,
    },
    Opaque {
        id: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct AssetId {
    pub chain: ChainId,
    pub locator: AssetLocator,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProductKind {
    Wrapper,
    SpotShare,
    LoanParticipation,
    Unknown,
    Other(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CanonicalAsset {
    pub key: AssetKey,
    pub id: AssetId,
    /// Informational only — never used as identity.
    pub display_symbol: String,
    pub issuer: Option<String>,
    pub product_kind: ProductKind,
    /// Economic exposure label; not identity.
    pub underlying_ref: Option<String>,
    pub role: Option<String>,
    /// How coverage denominators are counted for this asset's response series.
    #[serde(default)]
    pub session_calendar: crate::event::SessionCalendar,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct VenueId(pub String);
