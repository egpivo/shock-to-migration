//! Coverage gaps and evidence boundaries.
//!
//! Missing observations stay missing. They are never silently treated as zero.

use std::fmt;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MissingKind {
    OffChainVenue,
    HistoricalDepth,
    RouteAttribution,
    Supply,
    TraderIntent,
    ActorLinkage,
    Price,
    Other(String),
}

impl fmt::Display for MissingKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::OffChainVenue => write!(f, "off_chain_venue"),
            Self::HistoricalDepth => write!(f, "historical_depth"),
            Self::RouteAttribution => write!(f, "route_attribution"),
            Self::Supply => write!(f, "supply"),
            Self::TraderIntent => write!(f, "trader_intent"),
            Self::ActorLinkage => write!(f, "actor_linkage"),
            Self::Price => write!(f, "price"),
            Self::Other(s) => write!(f, "{s}"),
        }
    }
}

/// Whether a gap was authored in config or detected at runtime.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum GapSource {
    /// From `[[coverage_declared]]`. Always shown on section outputs.
    Declared,
    /// Produced by response/flow accounting. Filtered by section relevance.
    #[default]
    Detected,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CoverageGap {
    pub kind: MissingKind,
    pub scope: String,
    pub reason: String,
    #[serde(default)]
    pub source: GapSource,
}

impl CoverageGap {
    pub fn new(kind: MissingKind, scope: impl Into<String>, reason: impl Into<String>) -> Self {
        Self {
            kind,
            scope: scope.into(),
            reason: reason.into(),
            source: GapSource::Detected,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct EvidenceBoundary {
    pub missing: Vec<CoverageGap>,
    pub assumptions: Vec<String>,
}

impl EvidenceBoundary {
    pub fn merge_declared(&mut self, gaps: impl IntoIterator<Item = CoverageGap>) {
        self.missing.extend(gaps.into_iter().map(|mut gap| {
            gap.source = GapSource::Declared;
            gap
        }));
    }

    pub fn push_detected(&mut self, mut gap: CoverageGap) {
        gap.source = GapSource::Detected;
        self.missing.push(gap);
    }

    pub fn assume(&mut self, statement: impl Into<String>) {
        self.assumptions.push(statement.into());
    }
}
