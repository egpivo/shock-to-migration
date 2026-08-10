//! Coverage gaps and evidence boundaries.
//!
//! Missing observations stay missing. They are never silently treated as zero.

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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CoverageGap {
    pub kind: MissingKind,
    pub scope: String,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct EvidenceBoundary {
    pub missing: Vec<CoverageGap>,
    pub assumptions: Vec<String>,
}

impl EvidenceBoundary {
    pub fn merge_declared(&mut self, gaps: impl IntoIterator<Item = CoverageGap>) {
        self.missing.extend(gaps);
    }

    pub fn push_detected(&mut self, gap: CoverageGap) {
        self.missing.push(gap);
    }

    pub fn assume(&mut self, statement: impl Into<String>) {
        self.assumptions.push(statement.into());
    }
}
