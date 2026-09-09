//! Notifier adapter: structured stderr lines (localized) plus an optional macOS
//! desktop notification for the most severe alert.

use anyhow::Result;

use std::time::SystemTime;

use crate::app::ports::Notifier;
use crate::delivery::i18n::Locale;
use crate::delivery::{display_name, reset_note};
use crate::domain::{AccountId, Alert, ProviderId};

/// A class that was critical last poll and is available again (watch mode).
pub struct Recovered {
    pub provider: ProviderId,
    pub account: AccountId,
    pub subject: String,
}

pub struct CliNotifier {
    desktop: bool,
    offset_secs: i32,
    locale: Locale,
}

impl CliNotifier {
    pub fn new(desktop: bool, offset_secs: i32, locale: Locale) -> Self {
        CliNotifier {
            desktop,
            offset_secs,
            locale,
        }
    }

    /// Emit one INFO "refreshed — available now" line per recovered class, and
    /// a desktop notification for the first.
    pub fn notify_recovery(&self, recovered: &[Recovered]) {
        for r in recovered {
            eprintln!(
                "[{}] {}/{} {} — {}",
                self.locale.level_tag(crate::domain::AlertLevel::Info),
                display_name(r.provider.as_str()),
                r.account,
                r.subject,
                self.locale.recovered_reason(&r.subject),
            );
        }
        if self.desktop {
            if let Some(first) = recovered.first() {
                recovery_desktop_notify(first, self.locale);
            }
        }
    }
}

impl Notifier for CliNotifier {
    fn notify(&self, alerts: &[Alert]) -> Result<()> {
        let now = SystemTime::now();
        for a in alerts {
            let suggestion = self
                .locale
                .alert_suggestion(a)
                .map(|s| format!(" · {s}"))
                .unwrap_or_default();
            let alarm = reset_note(a, now, self.offset_secs, self.locale)
                .map(|s| format!(" · {s}"))
                .unwrap_or_default();
            eprintln!(
                "[{}] {}/{} {} — {}{}{}",
                self.locale.level_tag(a.level),
                display_name(a.provider.as_str()),
                a.account,
                a.subject,
                self.locale.alert_reason(a),
                suggestion,
                alarm,
            );
        }

        if self.desktop {
            if let Some(top) = alerts.iter().max_by_key(|a| a.level) {
                desktop_notify(top, now, self.offset_secs, self.locale);
            }
        }
        Ok(())
    }
}

#[cfg(target_os = "macos")]
fn desktop_notify(alert: &Alert, now: SystemTime, offset_secs: i32, locale: Locale) {
    let alarm = reset_note(alert, now, offset_secs, locale)
        .map(|s| format!(" · {s}"))
        .unwrap_or_default();
    let body = format!(
        "{}/{} {} — {}{}",
        display_name(alert.provider.as_str()),
        alert.account,
        alert.subject,
        locale.alert_reason(alert),
        alarm,
    );
    osascript_notify(
        &format!("headroom: {}", locale.level_tag(alert.level)),
        &body,
    );
}

#[cfg(not(target_os = "macos"))]
fn desktop_notify(_alert: &Alert, _now: SystemTime, _offset_secs: i32, _locale: Locale) {}

#[cfg(target_os = "macos")]
fn recovery_desktop_notify(r: &Recovered, locale: Locale) {
    let body = format!(
        "{}/{} {}",
        display_name(r.provider.as_str()),
        r.account,
        locale.recovered_reason(&r.subject),
    );
    osascript_notify(
        &format!(
            "headroom: {}",
            locale.level_tag(crate::domain::AlertLevel::Info)
        ),
        &body,
    );
}

#[cfg(not(target_os = "macos"))]
fn recovery_desktop_notify(_r: &Recovered, _locale: Locale) {}

#[cfg(target_os = "macos")]
fn osascript_notify(title: &str, body: &str) {
    use std::process::Command;
    let script = format!(
        "display notification {} with title {}",
        applescript_quote(body),
        applescript_quote(title)
    );
    let _ = Command::new("osascript").arg("-e").arg(script).output();
}

#[cfg(target_os = "macos")]
fn applescript_quote(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}
