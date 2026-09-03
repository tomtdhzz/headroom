//! Delivery layer: user-facing rendering. The domain/app layers are unaware of
//! this — `cli` (one-shot / watch) and `tui` (interactive) are interchangeable
//! adapters over the same `Assessment`.

pub mod cli;
pub mod i18n;
pub mod tui;

pub use i18n::Locale;

use std::time::SystemTime;

use crate::domain::human_duration;

/// Width of the unicode quota gauge, in cells.
pub(crate) const BAR_WIDTH: usize = 20;

/// Rendering-side severity, derived from headroom.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Sev {
    Ok,
    Warn,
    Crit,
}

pub(crate) fn severity(remaining: u8, available: bool) -> Sev {
    if !available || remaining <= 5 {
        Sev::Crit
    } else if remaining <= 20 {
        Sev::Warn
    } else {
        Sev::Ok
    }
}

pub(crate) fn worst_of(a: Sev, b: Sev) -> Sev {
    match (a, b) {
        (Sev::Crit, _) | (_, Sev::Crit) => Sev::Crit,
        (Sev::Warn, _) | (_, Sev::Warn) => Sev::Warn,
        _ => Sev::Ok,
    }
}

/// The number of filled cells for a given remaining percentage.
pub(crate) fn bar_filled(remaining: u8) -> usize {
    (((remaining as usize) * BAR_WIDTH + 50) / 100).min(BAR_WIDTH)
}

/// A fixed-width unicode gauge string, e.g. `[██████░░░░]`.
pub(crate) fn bar(remaining: u8) -> String {
    let filled = bar_filled(remaining);
    let mut s = String::with_capacity(BAR_WIDTH + 2);
    s.push('[');
    for _ in 0..filled {
        s.push('█');
    }
    for _ in filled..BAR_WIDTH {
        s.push('░');
    }
    s.push(']');
    s
}

pub(crate) fn countdown(reset: SystemTime, now: SystemTime) -> String {
    match reset.duration_since(now) {
        Ok(d) if d.as_secs() > 0 => human_duration(d),
        _ => "now".to_string(),
    }
}

pub(crate) fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let mut t: String = s.chars().take(max.saturating_sub(1)).collect();
    t.push('…');
    t
}

/// A friendly vendor label for a provider id (`anthropic` → `Claude`).
pub fn display_name(provider: &str) -> &str {
    match provider {
        "anthropic" => "Claude",
        "openai-codex" => "Codex",
        "openai" => "OpenAI",
        "google" | "google-gemini-cli" => "Gemini",
        "openrouter" => "OpenRouter",
        other => other,
    }
}
