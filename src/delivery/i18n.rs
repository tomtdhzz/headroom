//! Presentation-layer localization. Language lives here, never in the domain.

use crate::domain::{human_duration, Alert, AlertKind, AlertLevel};

use super::ResetWhen;

/// Supported display languages.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Locale {
    En,
    Zh,
}

impl Locale {
    /// Detect from `LC_ALL` / `LC_MESSAGES` / `LANG`; Chinese locales → `Zh`.
    pub fn detect() -> Locale {
        for key in ["LC_ALL", "LC_MESSAGES", "LANG"] {
            if let Some(v) = std::env::var_os(key) {
                let v = v.to_string_lossy().to_ascii_lowercase();
                if !v.is_empty() {
                    return if v.contains("zh") {
                        Locale::Zh
                    } else {
                        Locale::En
                    };
                }
            }
        }
        Locale::En
    }

    pub fn toggle(self) -> Locale {
        match self {
            Locale::En => Locale::Zh,
            Locale::Zh => Locale::En,
        }
    }

    pub fn no_accounts(self) -> &'static str {
        match self {
            Locale::En => "no authenticated accounts with usage reported.",
            Locale::Zh => "没有已认证且返回用量的账号。",
        }
    }

    pub fn alerts_heading(self) -> &'static str {
        match self {
            Locale::En => "alerts",
            Locale::Zh => "告警",
        }
    }

    pub fn unavailable(self) -> &'static str {
        match self {
            Locale::En => "UNAVAILABLE",
            Locale::Zh => "不可用",
        }
    }

    /// Localized severity tag shown in rows/notifications.
    pub fn level_tag(self, level: AlertLevel) -> &'static str {
        match (self, level) {
            (Locale::En, AlertLevel::Critical) => "CRIT",
            (Locale::En, AlertLevel::Warn) => "WARN",
            (Locale::En, AlertLevel::Info) => "INFO",
            (Locale::Zh, AlertLevel::Critical) => "严重",
            (Locale::Zh, AlertLevel::Warn) => "警告",
            (Locale::Zh, AlertLevel::Info) => "提示",
        }
    }

    /// TUI header title, e.g. ` headroom — 2 account(s) · updated 5s ago `.
    pub fn tui_title(self, accounts: usize, secs: u64) -> String {
        match self {
            Locale::En => format!(" headroom — {accounts} account(s) · updated {secs}s ago "),
            Locale::Zh => format!(" headroom — {accounts} 个账号 · {secs}s 前刷新 "),
        }
    }

    pub fn tui_footer(self) -> &'static str {
        match self {
            Locale::En => " ↑↓/jk select · r refresh · l 中/EN · q quit",
            Locale::Zh => " ↑↓/jk 选择 · r 刷新 · l 中/EN · q 退出",
        }
    }

    /// Human-readable reason for an alert.
    pub fn alert_reason(self, a: &Alert) -> String {
        let subject = &a.subject;
        let bottleneck = &a.bottleneck;
        let remaining = a.remaining.get();
        let eta = a.eta.map(human_duration).unwrap_or_default();
        match (self, a.kind) {
            (Locale::En, AlertKind::Unavailable) => {
                format!("{subject} unavailable (bottleneck: {bottleneck})")
            }
            (Locale::En, AlertKind::LowRemaining) => {
                format!("{subject} at {remaining}% remaining (bottleneck: {bottleneck})")
            }
            (Locale::En, AlertKind::DepletingSoon) => {
                format!("{subject} projected to deplete in {eta} (bottleneck: {bottleneck})")
            }
            (Locale::Zh, AlertKind::Unavailable) => {
                format!("{subject} 不可用(瓶颈:{bottleneck})")
            }
            (Locale::Zh, AlertKind::LowRemaining) => {
                format!("{subject} 仅剩 {remaining}%(瓶颈:{bottleneck})")
            }
            (Locale::Zh, AlertKind::DepletingSoon) => {
                format!("{subject} 预计 {eta} 后耗尽(瓶颈:{bottleneck})")
            }
        }
    }

    /// Localized switch suggestion, if any.
    pub fn alert_suggestion(self, a: &Alert) -> Option<String> {
        a.suggestion.as_ref().map(|s| {
            let (class, left) = (&s.class, s.remaining.get());
            match self {
                Locale::En => format!("switch to {class} ({left}% left)"),
                Locale::Zh => format!("可切到 {class}(还剩 {left}%)"),
            }
        })
    }

    /// Localize a window-reset time point, e.g. `明天 08:00` / `tomorrow 08:00`.
    pub(crate) fn reset_when_label(self, w: &ResetWhen) -> String {
        let hm = |h: u32, m: u32| format!("{h:02}:{m:02}");
        let weekday = |wd: u32| -> &'static str {
            let en = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
            let zh = ["周日", "周一", "周二", "周三", "周四", "周五", "周六"];
            let i = (wd % 7) as usize;
            match self {
                Locale::En => en[i],
                Locale::Zh => zh[i],
            }
        };
        match (self, *w) {
            (_, ResetWhen::Today { hour, min }) => hm(hour, min),
            (Locale::En, ResetWhen::Tomorrow { hour, min }) => {
                format!("tomorrow {}", hm(hour, min))
            }
            (Locale::Zh, ResetWhen::Tomorrow { hour, min }) => format!("明天 {}", hm(hour, min)),
            (_, ResetWhen::Weekday { wd, hour, min }) => {
                format!("{} {}", weekday(wd), hm(hour, min))
            }
            (
                _,
                ResetWhen::Date {
                    month,
                    day,
                    hour,
                    min,
                },
            ) => {
                format!("{month:02}-{day:02} {}", hm(hour, min))
            }
        }
    }

    /// Proactive "window refresh alarm" clause appended to an alert, given the
    /// already-localized `when` label and a countdown string.
    pub(crate) fn reset_note(self, a: &Alert, when: &str, countdown: &str) -> String {
        let resumes = matches!(a.kind, AlertKind::Unavailable);
        match (self, resumes) {
            (Locale::En, true) => format!("resume {when} (↺{countdown})"),
            (Locale::En, false) => format!("resets {when} (↺{countdown})"),
            (Locale::Zh, true) => format!("{when} 恢复(↺{countdown})"),
            (Locale::Zh, false) => format!("{when} 刷新(↺{countdown})"),
        }
    }

    /// Watch-mode recovery ping: a previously-critical class is available again.
    pub(crate) fn recovered_reason(self, subject: &str) -> String {
        match self {
            Locale::En => format!("{subject} refreshed — available now"),
            Locale::Zh => format!("{subject} 已刷新，现在可用"),
        }
    }
}
