//! One-shot / watch CLI rendering: per-account bar gauges with whole-row
//! severity coloring, localized via `Locale`. Pure string building — testable
//! without a TTY.

use std::io::IsTerminal;
use std::time::SystemTime;

use crate::app::evaluate::{AccountAssessment, Assessment};
use crate::domain::human_duration;

use super::i18n::Locale;
use super::{bar, countdown, display_name, severity, truncate, worst_of, Sev};

/// ANSI palette; `plain()` disables color for pipes / `NO_COLOR`.
pub struct Palette {
    pub reset: &'static str,
    pub bold: &'static str,
    pub dim: &'static str,
    pub red: &'static str,
    pub yellow: &'static str,
    pub green: &'static str,
}

impl Palette {
    pub fn ansi() -> Self {
        Palette {
            reset: "\x1b[0m",
            bold: "\x1b[1m",
            dim: "\x1b[2m",
            red: "\x1b[1;31m",
            yellow: "\x1b[1;33m",
            green: "\x1b[1;32m",
        }
    }

    pub fn plain() -> Self {
        Palette {
            reset: "",
            bold: "",
            dim: "",
            red: "",
            yellow: "",
            green: "",
        }
    }

    /// Color when forced (`CLICOLOR_FORCE`), or when stdout is a TTY and
    /// `NO_COLOR` is unset.
    pub fn auto() -> Self {
        let forced = std::env::var_os("CLICOLOR_FORCE")
            .map(|v| !v.is_empty() && v != "0")
            .unwrap_or(false);
        if forced {
            return Palette::ansi();
        }
        if std::env::var_os("NO_COLOR").is_none() && std::io::stdout().is_terminal() {
            Palette::ansi()
        } else {
            Palette::plain()
        }
    }

    fn sev_color(&self, sev: Sev) -> &'static str {
        match sev {
            Sev::Crit => self.red,
            Sev::Warn => self.yellow,
            Sev::Ok => self.green,
        }
    }
}

/// Render the whole assessment to a printable string in the given locale.
pub fn render(assessment: &Assessment, now: SystemTime, p: &Palette, loc: Locale) -> String {
    let mut out = String::new();

    if assessment.accounts.is_empty() {
        out.push('\n');
        out.push_str(loc.no_accounts());
        out.push('\n');
        return out;
    }

    for account in &assessment.accounts {
        render_account(&mut out, account, now, p, loc);
    }

    let alerts = assessment.alerts();
    if !alerts.is_empty() {
        out.push_str(&format!(
            "\n{}{}{}\n",
            p.bold,
            loc.alerts_heading(),
            p.reset
        ));
        for a in &alerts {
            let color = p.sev_color(match a.level {
                crate::domain::AlertLevel::Critical => Sev::Crit,
                crate::domain::AlertLevel::Warn => Sev::Warn,
                crate::domain::AlertLevel::Info => Sev::Ok,
            });
            let suggestion = loc
                .alert_suggestion(a)
                .map(|s| format!(" · {s}"))
                .unwrap_or_default();
            out.push_str(&format!(
                "{}  {:<4} {}/{} — {}{}{}\n",
                color,
                loc.level_tag(a.level),
                display_name(a.provider.as_str()),
                a.account,
                loc.alert_reason(a),
                suggestion,
                p.reset,
            ));
        }
    }

    out
}

fn render_account(
    out: &mut String,
    account: &AccountAssessment,
    now: SystemTime,
    p: &Palette,
    loc: Locale,
) {
    let snap = &account.snapshot;
    let plan = snap.plan.as_deref().unwrap_or("-");

    let worst = account
        .classes
        .iter()
        .map(|c| severity(c.headroom.effective_remaining.get(), c.headroom.available))
        .fold(Sev::Ok, worst_of);

    out.push_str(&format!(
        "\n{}{}{} {}·{} {} {}·{} {}\n",
        p.sev_color(worst),
        display_name(snap.provider.as_str()),
        p.reset,
        p.dim,
        p.reset,
        snap.account,
        p.dim,
        p.reset,
        plan
    ));

    for class in &account.classes {
        let hr = &class.headroom;
        let remaining = hr.effective_remaining.get();
        let color = p.sev_color(severity(remaining, hr.available));

        let resets = hr
            .bottleneck
            .resets_at
            .map(|r| countdown(r, now))
            .unwrap_or_else(|| "—".to_string());
        let eta = class
            .forecast
            .eta
            .map(human_duration)
            .unwrap_or_else(|| "—".to_string());
        let flag = if hr.available {
            String::new()
        } else {
            format!("  ✕ {}", loc.unavailable())
        };

        // Whole row wrapped in one color: critical => the entire line is red.
        out.push_str(&format!(
            "{}  {:<7} {} {:>3}%  {:<22} ↺{:<7} ~{}{}{}\n",
            color,
            hr.class.label(),
            bar(remaining),
            remaining,
            truncate(&hr.bottleneck.label, 22),
            resets,
            eta,
            flag,
            p.reset,
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::evaluate::{AccountAssessment, ClassAssessment};
    use crate::domain::AccountId;
    use crate::domain::{
        evaluate, ExhaustionForecast, Headroom, LimitScope, LimitWindow, ModelClass, Percent,
        ProviderId, QuotaSnapshot, Thresholds, Tier, WindowStatus,
    };

    fn window(id: &str, remaining: u8) -> LimitWindow {
        LimitWindow {
            id: id.into(),
            label: id.into(),
            scope: LimitScope::Tier(Tier::new("fable")),
            period: None,
            remaining: Percent::new(remaining),
            resets_at: None,
            status: WindowStatus::Ok,
        }
    }

    fn assessment(remaining: u8) -> Assessment {
        let hr = Headroom {
            class: ModelClass::Tier(Tier::new("fable")),
            effective_remaining: Percent::new(remaining),
            bottleneck: window("7d:fable", remaining),
            available: true,
        };
        let alerts = evaluate(
            &ProviderId::new("anthropic"),
            &AccountId::new("f6*"),
            &[(hr.clone(), ExhaustionForecast::default())],
            &Thresholds::default(),
        );
        Assessment {
            accounts: vec![AccountAssessment {
                snapshot: QuotaSnapshot {
                    provider: ProviderId::new("anthropic"),
                    account: AccountId::new("f6*"),
                    plan: Some("max".into()),
                    fetched_at: SystemTime::UNIX_EPOCH,
                    windows: vec![],
                },
                classes: vec![ClassAssessment {
                    headroom: hr,
                    forecast: ExhaustionForecast::default(),
                }],
                alerts,
            }],
        }
    }

    #[test]
    fn english_render_is_plain_without_color() {
        let text = render(
            &assessment(4),
            SystemTime::UNIX_EPOCH,
            &Palette::plain(),
            Locale::En,
        );
        assert!(text.contains("Claude"));
        assert!(text.contains("fable"));
        assert!(text.contains("4%"));
        assert!(text.contains('█'), "should draw a bar gauge");
        assert!(text.contains("remaining"), "english alert wording");
        assert!(!text.contains('\x1b'), "plain palette must emit no ANSI");
    }

    #[test]
    fn chinese_render_localizes_chrome_and_alerts() {
        let text = render(
            &assessment(4),
            SystemTime::UNIX_EPOCH,
            &Palette::plain(),
            Locale::Zh,
        );
        assert!(text.contains("告警"), "localized alerts heading");
        assert!(text.contains("严重"), "localized severity tag");
        assert!(text.contains("仅剩"), "localized alert reason");
        assert!(!text.contains("remaining"), "no english leakage");
    }

    #[test]
    fn critical_row_is_wrapped_in_red() {
        let text = render(
            &assessment(4),
            SystemTime::UNIX_EPOCH,
            &Palette::ansi(),
            Locale::En,
        );
        assert!(text.contains("\x1b[1;31m"), "critical row should be red");
    }
}
