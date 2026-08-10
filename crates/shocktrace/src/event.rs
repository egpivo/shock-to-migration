//! Dated shocks and named analysis windows.

use chrono::{DateTime, Datelike, NaiveDate, Utc};
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

/// How expected observation days are counted inside a window for coverage.
///
/// - [`SessionCalendar::Continuous`]: every calendar day in `[start, end]`
///   (appropriate for 24/7 on-chain series).
/// - [`SessionCalendar::ExchangeSessions`]: Monday–Friday only. Weekends are
///   not gaps. Exchange holidays may still appear as gaps until an explicit
///   holiday calendar is supplied.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum SessionCalendar {
    #[default]
    Continuous,
    ExchangeSessions,
}

impl SessionCalendar {
    /// Label used in coverage gap reason strings.
    pub fn coverage_label(self) -> &'static str {
        match self {
            Self::Continuous => "calendar",
            Self::ExchangeSessions => "weekday",
        }
    }
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

    /// Inclusive calendar length (always continuous days).
    pub fn calendar_day_count(&self) -> usize {
        (self.end - self.start).num_days() as usize + 1
    }

    /// Expected observation slots for coverage under `calendar`.
    pub fn expected_session_count(&self, calendar: SessionCalendar) -> usize {
        match calendar {
            SessionCalendar::Continuous => self.calendar_day_count(),
            SessionCalendar::ExchangeSessions => self.weekday_count(),
        }
    }

    /// Count of Mondays–Fridays in the inclusive range.
    pub fn weekday_count(&self) -> usize {
        let mut n = 0usize;
        let mut d = self.start;
        while d <= self.end {
            let wd = d.weekday().num_days_from_monday();
            if wd < 5 {
                n += 1;
            }
            d = d.succ_opt().expect("date range");
        }
        n
    }

    /// Days that count as expected sessions but have no observation.
    pub fn missing_sessions(
        &self,
        calendar: SessionCalendar,
        observed: &std::collections::BTreeSet<NaiveDate>,
    ) -> Vec<NaiveDate> {
        let mut missing = Vec::new();
        let mut d = self.start;
        while d <= self.end {
            let expected = match calendar {
                SessionCalendar::Continuous => true,
                SessionCalendar::ExchangeSessions => d.weekday().num_days_from_monday() < 5,
            };
            if expected && !observed.contains(&d) {
                missing.push(d);
            }
            d = d.succ_opt().expect("date range");
        }
        missing
    }
}

/// Backward-compatible default when `applies_to` is omitted from TOML.
pub fn default_window_applies_to() -> Vec<WindowUse> {
    vec![WindowUse::Response, WindowUse::Flow]
}
