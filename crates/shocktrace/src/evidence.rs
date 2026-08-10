//! Availability of an evidence section.
//!
//! Distinguishes measured empty/zero results from "not declared" / "not observable".
//! Never encode unknown as zero.

use serde::Serialize;

/// Tagged evidence container. Prefer this over bare `Vec` / `Option`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum EvidenceSection<T> {
    /// Section was requested and computed (payload may be empty if truly measured empty).
    Available { data: T },
    /// Project did not declare this evidence class (e.g. no routes).
    NotDeclared { reason: String },
    /// Declared as of interest, but observations/linkage are unavailable.
    NotObservable { reason: String },
}

impl<T> EvidenceSection<T> {
    pub fn available(data: T) -> Self {
        Self::Available { data }
    }

    pub fn not_declared(reason: impl Into<String>) -> Self {
        Self::NotDeclared {
            reason: reason.into(),
        }
    }

    pub fn not_observable(reason: impl Into<String>) -> Self {
        Self::NotObservable {
            reason: reason.into(),
        }
    }

    pub fn is_available(&self) -> bool {
        matches!(self, Self::Available { .. })
    }
}
