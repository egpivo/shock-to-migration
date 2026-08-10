//! Dated shocks and named analysis windows.

use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Event {
    pub id: String,
    pub name: String,
    pub timestamp: DateTime<Utc>,
}

/// Which analysis sections evaluate a named window.
///
/// Omitted `applies_to` in project.toml defaults to both [`WindowUse::Response`]
/// and [`WindowUse::Flow`] (backward compatible).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WindowUse {
    Response,
    Flow,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EventWindow {
    pub name: String,
    pub start: NaiveDate,
    pub end: NaiveDate,
    /// Non-empty after load. Default policy: response + flow.
    pub applies_to: Vec<WindowUse>,
}

impl EventWindow {
    pub fn contains(&self, day: NaiveDate) -> bool {
        day >= self.start && day <= self.end
    }

    pub fn applies_to_response(&self) -> bool {
        self.applies_to.contains(&WindowUse::Response)
    }

    pub fn applies_to_flow(&self) -> bool {
        self.applies_to.contains(&WindowUse::Flow)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.end < self.start {
            return Err(format!(
                "window '{}': end {} is before start {}",
                self.name, self.end, self.start
            ));
        }
        if self.applies_to.is_empty() {
            return Err(format!(
                "window '{}': applies_to must be non-empty (use response and/or flow)",
                self.name
            ));
        }
        Ok(())
    }
}

/// Backward-compatible default when `applies_to` is omitted from TOML.
pub fn default_window_applies_to() -> Vec<WindowUse> {
    vec![WindowUse::Response, WindowUse::Flow]
}
