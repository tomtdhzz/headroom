//! Delivery layer: user-facing rendering. The domain/app layers are unaware of
//! this — `cli` (one-shot / watch) and `tui` (interactive) are interchangeable
//! adapters over the same `Assessment`.

pub mod cli;
pub mod i18n;
pub mod switch;
pub mod tui;

pub use i18n::Locale;

use std::time::SystemTime;

use crate::domain::{human_duration, Alert};

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

/// Worst rendering severity across a provider's accounts in the assessment, or
/// `None` when the provider reports no usage (e.g. a catalog-only provider).
/// Used to give each switch "节点" a Clash-like health color from real quota.
pub(crate) fn provider_severity(
    assessment: &crate::app::evaluate::Assessment,
    provider: &str,
) -> Option<Sev> {
    let mut sev: Option<Sev> = None;
    for account in &assessment.accounts {
        if account.snapshot.provider.as_str() != provider {
            continue;
        }
        for class in &account.classes {
            let s = severity(
                class.headroom.effective_remaining.get(),
                class.headroom.available,
            );
            sev = Some(match sev {
                Some(prev) => worst_of(prev, s),
                None => s,
            });
        }
    }
    sev
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

/// A local-clock breakdown of a window's next reset, relative to `now` — the
/// "window refresh alarm" time point. `wd` is 0=Sun..6=Sat.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum ResetWhen {
    Today {
        hour: u32,
        min: u32,
    },
    Tomorrow {
        hour: u32,
        min: u32,
    },
    Weekday {
        wd: u32,
        hour: u32,
        min: u32,
    },
    Date {
        month: u32,
        day: u32,
        hour: u32,
        min: u32,
    },
}

/// When (in local wall-clock) a window resets, or `None` if `reset` is not in
/// the future. `offset_secs` is the local UTC offset in seconds.
pub(crate) fn reset_when(
    reset: SystemTime,
    now: SystemTime,
    offset_secs: i32,
) -> Option<ResetWhen> {
    // Future only: a past/equal reset is not an alarm.
    if reset
        .duration_since(now)
        .map(|d| d.as_secs() == 0)
        .unwrap_or(true)
    {
        return None;
    }
    let reset_local = local_secs(reset, offset_secs)?;
    let now_local = local_secs(now, offset_secs)?;
    let (rday, rsec) = (
        reset_local.div_euclid(86_400),
        reset_local.rem_euclid(86_400),
    );
    let nday = now_local.div_euclid(86_400);
    let hour = (rsec / 3_600) as u32;
    let min = ((rsec % 3_600) / 60) as u32;
    Some(match rday - nday {
        d if d <= 0 => ResetWhen::Today { hour, min },
        1 => ResetWhen::Tomorrow { hour, min },
        2..=6 => ResetWhen::Weekday {
            // 1970-01-01 (day 0) was a Thursday; 0=Sun..6=Sat.
            wd: (rday.rem_euclid(7) as u32 + 4) % 7,
            hour,
            min,
        },
        _ => {
            let (_, month, day) = civil_from_days(rday);
            ResetWhen::Date {
                month,
                day,
                hour,
                min,
            }
        }
    })
}

/// Local epoch-seconds (UTC epoch shifted by the local offset).
fn local_secs(t: SystemTime, offset_secs: i32) -> Option<i64> {
    let utc = t.duration_since(SystemTime::UNIX_EPOCH).ok()?.as_secs() as i64;
    Some(utc + offset_secs as i64)
}

/// Civil (year, month, day) from days since 1970-01-01 (Howard Hinnant's
/// `civil_from_days`). Valid for the full proleptic Gregorian range.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097; // [0, 146096]
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365; // [0, 399]
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32; // [1, 31]
    let m = (if mp < 10 { mp + 3 } else { mp - 9 }) as u32; // [1, 12]
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// The full, localized "window refresh alarm" clause for an alert, or `None`
/// when the bottleneck has no future reset instant. Shared by cli/tui/notifier.
pub(crate) fn reset_note(
    a: &Alert,
    now: SystemTime,
    offset_secs: i32,
    loc: Locale,
) -> Option<String> {
    let reset = a.reset_at?;
    let when = reset_when(reset, now, offset_secs)?;
    let countdown = human_duration(reset.duration_since(now).ok()?);
    Some(loc.reset_note(a, &loc.reset_when_label(&when), &countdown))
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    const DAY: u64 = 86_400;

    fn at(day: u64, hour: u64, min: u64) -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_secs(day * DAY + hour * 3_600 + min * 60)
    }

    #[test]
    fn same_local_day_is_today() {
        let now = at(20_000, 12, 0);
        assert_eq!(
            reset_when(at(20_000, 20, 30), now, 0),
            Some(ResetWhen::Today { hour: 20, min: 30 })
        );
    }

    #[test]
    fn next_local_day_is_tomorrow() {
        let now = at(20_000, 12, 0);
        assert_eq!(
            reset_when(at(20_001, 8, 0), now, 0),
            Some(ResetWhen::Tomorrow { hour: 8, min: 0 })
        );
    }

    #[test]
    fn within_the_week_is_a_weekday() {
        let now = at(20_000, 12, 0);
        // day 20003: (20003 % 7 + 4) % 7 == 1 (Monday).
        assert_eq!(
            reset_when(at(20_003, 9, 15), now, 0),
            Some(ResetWhen::Weekday {
                wd: 1,
                hour: 9,
                min: 15
            })
        );
    }

    #[test]
    fn far_out_is_a_calendar_date() {
        let now = at(20_000, 12, 0);
        match reset_when(at(20_010, 6, 0), now, 0) {
            Some(ResetWhen::Date { hour, min, .. }) => {
                assert_eq!((hour, min), (6, 0));
            }
            other => panic!("expected Date, got {other:?}"),
        }
    }

    #[test]
    fn past_or_equal_reset_has_no_alarm() {
        let now = at(20_000, 12, 0);
        assert_eq!(reset_when(at(20_000, 11, 0), now, 0), None);
        assert_eq!(reset_when(at(20_000, 12, 0), now, 0), None);
    }

    #[test]
    fn offset_shifts_the_local_civil_day() {
        // now 20:00 UTC → 04:00 next local day at +8h; reset 23:00 UTC → 07:00
        // that same local day → Today.
        let now = at(20_000, 20, 0);
        assert_eq!(
            reset_when(at(20_000, 23, 0), now, 8 * 3_600),
            Some(ResetWhen::Today { hour: 7, min: 0 })
        );
    }
}
