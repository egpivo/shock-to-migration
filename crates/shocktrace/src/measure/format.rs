//! Terminal-friendly text summaries for `measure` CLI output.
//!
//! Convention (matches `report.rs`): JSON keeps full `Decimal` precision;
//! text summaries round display-only values to 6 decimal places.

use std::fmt::Write as _;

use rust_decimal::Decimal;

use super::shock::AssetShockReport;
use super::stats::ZScoreUnavailable;
use crate::coverage::format_evidence_boundary;
use crate::measure::divergence::DivergenceSummary;
use crate::measure::passthrough::PassThroughSummary;

fn fmt_opt(value: Option<Decimal>) -> String {
    match value {
        Some(v) => v.round_dp(6).to_string(),
        None => "null".to_string(),
    }
}

fn fmt_reason(reason: &Option<ZScoreUnavailable>) -> String {
    match reason {
        None => String::new(),
        Some(ZScoreUnavailable::NoEventValue) => " (no event value)".into(),
        Some(ZScoreUnavailable::InsufficientBaseline { have, need }) => {
            format!(" (insufficient baseline: {have} of {need} minimum)")
        }
        Some(ZScoreUnavailable::ZeroBaselineVariance) => " (zero baseline variance)".into(),
    }
}

pub fn format_shock_report_summary(asset: &str, report: &AssetShockReport) -> String {
    let mut out = String::new();
    let shock = &report.shock;
    let _ = writeln!(out, "shock score: {asset}");
    let _ = writeln!(
        out,
        "  event window   : {} (day={})",
        shock.event_window_name,
        shock
            .event_day
            .map(|d| d.to_string())
            .unwrap_or_else(|| "none".into())
    );
    let _ = writeln!(out, "  baseline window: {}", shock.baseline_window_name);
    let _ = writeln!(out, "  event_return   : {}", fmt_opt(shock.event_return));
    let _ = writeln!(
        out,
        "  baseline mean/std/n: {} / {} / {}",
        fmt_opt(shock.baseline_mean),
        fmt_opt(shock.baseline_std),
        shock.baseline_n
    );
    let low = if shock.low_baseline {
        " (low baseline — below adequacy floor)"
    } else {
        ""
    };
    let _ = writeln!(
        out,
        "  z_score        : {}{}{}",
        fmt_opt(shock.z_score),
        fmt_reason(&shock.unavailable_reason),
        low
    );

    let _ = writeln!(
        out,
        "horizon returns (anchor={:?}):",
        report.horizons.anchor_day
    );
    for h in &report.horizons.horizons {
        let _ = writeln!(
            out,
            "  +{:<3} sessions: {} (day={})",
            h.horizon_sessions,
            fmt_opt(h.cumulative_return),
            h.day
                .map(|d| d.to_string())
                .unwrap_or_else(|| "none".into())
        );
    }

    let _ = writeln!(out, "activity anomaly:");
    let _ = writeln!(
        out,
        "  window/baseline median volume: {} / {}",
        fmt_opt(report.activity.window_median_volume),
        fmt_opt(report.activity.baseline_median_volume)
    );
    let _ = writeln!(out, "  ratio: {}", fmt_opt(report.activity.ratio));

    let _ = writeln!(out, "boundary: {}", shock.interpretation_boundary);
    out.push_str(&format_evidence_boundary(&report.boundary));
    out.push('\n');
    out
}

pub fn format_divergence_summary(summary: &DivergenceSummary) -> String {
    let mut out = String::new();
    let _ = writeln!(
        out,
        "divergence: {} vs {}",
        summary.asset_a, summary.asset_b
    );
    let _ = writeln!(
        out,
        "  event window   : {} (day={})",
        summary.event_window_name,
        summary
            .event_day
            .map(|d| d.to_string())
            .unwrap_or_else(|| "none".into())
    );
    let _ = writeln!(out, "  baseline window: {}", summary.baseline_window_name);
    let _ = writeln!(out, "  matched_days   : {}", summary.matched_days);
    let _ = writeln!(
        out,
        "  event_divergence: {}",
        fmt_opt(summary.event_divergence)
    );
    let _ = writeln!(
        out,
        "  baseline mean/std/n: {} / {} / {}",
        fmt_opt(summary.baseline_mean),
        fmt_opt(summary.baseline_std),
        summary.baseline_n
    );
    let low = if summary.low_baseline {
        " (low baseline — below adequacy floor)"
    } else {
        ""
    };
    let _ = writeln!(
        out,
        "  z_score        : {}{}{}",
        fmt_opt(summary.z_score),
        fmt_reason(&summary.unavailable_reason),
        low
    );
    let _ = writeln!(out, "boundary: {}", summary.interpretation_boundary);
    out.push_str(&format_evidence_boundary(&summary.boundary));
    out.push('\n');
    out
}

pub fn format_passthrough_summary(summary: &PassThroughSummary) -> String {
    let mut out = String::new();
    let _ = writeln!(
        out,
        "pass-through: {} vs {}",
        summary.asset_key, summary.reference_key
    );
    let _ = writeln!(out, "  event day       : {}", summary.event_day);
    let _ = writeln!(
        out,
        "  reference return: {}",
        summary.reference_return.round_dp(6)
    );
    let _ = writeln!(out, "  token return    : {}", fmt_opt(summary.token_return));
    let _ = writeln!(
        out,
        "  response gap    : {} (token - reference)",
        fmt_opt(summary.response_gap)
    );
    let direction = match summary.direction_match {
        Some(true) => "same",
        Some(false) => "opposite",
        None => "unavailable",
    };
    let _ = writeln!(out, "  direction       : {direction}");
    let _ = writeln!(out, "  reference cutoff: {}", summary.reference_cutoff);
    let _ = writeln!(out, "boundary: {}", summary.interpretation_boundary);
    out.push_str(&format_evidence_boundary(&summary.boundary));
    out.push('\n');
    out
}
