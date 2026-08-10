//! Dated shocks and named analysis windows.

use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Event {
    pub id: String,
    pub name: String,
    pub timestamp: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EventWindow {
    pub name: String,
    pub start: NaiveDate,
    pub end: NaiveDate,
}

impl EventWindow {
    pub fn contains(&self, day: NaiveDate) -> bool {
        day >= self.start && day <= self.end
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.end < self.start {
            return Err(format!(
                "window '{}': end {} is before start {}",
                self.name, self.end, self.start
            ));
        }
        Ok(())
    }
}
