//! Notifier adapter: structured stderr lines (localized) plus an optional macOS
//! desktop notification for the most severe alert.

use anyhow::Result;

use crate::app::ports::Notifier;
use crate::delivery::display_name;
use crate::delivery::i18n::Locale;
use crate::domain::Alert;

pub struct CliNotifier {
    desktop: bool,
    locale: Locale,
}

impl CliNotifier {
    pub fn new(desktop: bool, locale: Locale) -> Self {
        CliNotifier { desktop, locale }
    }
}

impl Notifier for CliNotifier {
    fn notify(&self, alerts: &[Alert]) -> Result<()> {
        for a in alerts {
            let suggestion = self
                .locale
                .alert_suggestion(a)
                .map(|s| format!(" · {s}"))
                .unwrap_or_default();
            eprintln!(
                "[{}] {}/{} {} — {}{}",
                self.locale.level_tag(a.level),
                display_name(a.provider.as_str()),
                a.account,
                a.subject,
                self.locale.alert_reason(a),
                suggestion
            );
        }

        if self.desktop {
            if let Some(top) = alerts.iter().max_by_key(|a| a.level) {
                desktop_notify(top, self.locale);
            }
        }
        Ok(())
    }
}

#[cfg(target_os = "macos")]
fn desktop_notify(alert: &Alert, locale: Locale) {
    use std::process::Command;
    let title = format!("headroom: {}", locale.level_tag(alert.level));
    let body = format!(
        "{}/{} {} — {}",
        display_name(alert.provider.as_str()),
        alert.account,
        alert.subject,
        locale.alert_reason(alert)
    );
    let script = format!(
        "display notification {} with title {}",
        applescript_quote(&body),
        applescript_quote(&title)
    );
    let _ = Command::new("osascript").arg("-e").arg(script).output();
}

#[cfg(not(target_os = "macos"))]
fn desktop_notify(_alert: &Alert, _locale: Locale) {}

#[cfg(target_os = "macos")]
fn applescript_quote(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}
