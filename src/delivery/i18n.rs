//! Presentation-layer localization. Language lives here, never in the domain.

use crate::domain::{human_duration, Alert, AlertKind, AlertLevel, Role};

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
            Locale::En => " ↑↓/jk select · s switch · r refresh · l 中/EN · q quit",
            Locale::Zh => " ↑↓/jk 选择 · s 切换 · r 刷新 · l 中/EN · q 退出",
        }
    }

    /// Localized name for an omp role (the "策略组").
    pub fn role_label(self, role: Role) -> &'static str {
        match (self, role) {
            (Locale::En, Role::Default) => "Default",
            (Locale::En, Role::Plan) => "Plan",
            (Locale::En, Role::Slow) => "Slow",
            (Locale::En, Role::Smol) => "Smol",
            (Locale::En, Role::Advisor) => "Advisor",
            (Locale::Zh, Role::Default) => "默认",
            (Locale::Zh, Role::Plan) => "规划",
            (Locale::Zh, Role::Slow) => "深思",
            (Locale::Zh, Role::Smol) => "轻快",
            (Locale::Zh, Role::Advisor) => "顾问",
        }
    }

    /// One-line hint for a role's purpose (shown in the policy-group pane).
    pub fn role_hint(self, role: Role) -> &'static str {
        match (self, role) {
            (Locale::En, Role::Default) => "primary interactive model",
            (Locale::En, Role::Plan) => "architectural planning (--plan)",
            (Locale::En, Role::Slow) => "thorough reasoning (--slow)",
            (Locale::En, Role::Smol) => "fast/cheap tasks (--smol)",
            (Locale::En, Role::Advisor) => "passive reviewer (advisor)",
            (Locale::Zh, Role::Default) => "主力交互模型",
            (Locale::Zh, Role::Plan) => "架构规划(--plan)",
            (Locale::Zh, Role::Slow) => "深度推理(--slow)",
            (Locale::Zh, Role::Smol) => "轻量快速任务(--smol)",
            (Locale::Zh, Role::Advisor) => "被动复审(顾问)",
        }
    }

    /// Pseudo-node meaning "clear the pin, let omp choose" (the URLTest analog).
    pub fn auto_node(self) -> &'static str {
        match self {
            Locale::En => "◎ Auto (let omp choose)",
            Locale::Zh => "◎ 自动(交由 omp)",
        }
    }

    /// Column heading for the policy-group (roles) pane.
    pub fn groups_heading(self) -> &'static str {
        match self {
            Locale::En => "policy groups",
            Locale::Zh => "策略组",
        }
    }

    /// Column heading for the candidate-nodes (models) pane.
    pub fn nodes_heading(self) -> &'static str {
        match self {
            Locale::En => "models",
            Locale::Zh => "模型节点",
        }
    }

    /// Label for an unset role pin.
    pub fn unset(self) -> &'static str {
        match self {
            Locale::En => "auto",
            Locale::Zh => "自动",
        }
    }

    /// Footer for the interactive switch pane.
    pub fn switch_footer(self) -> &'static str {
        match self {
            Locale::En => {
                " ↑↓/jk model · ←→/Tab role · / filter · ⏎ apply · c clear · g back · q quit"
            }
            Locale::Zh => {
                " ↑↓/jk 模型 · ←→/Tab 策略组 · / 过滤 · ⏎ 应用 · c 清除 · g 返回 · q 退出"
            }
        }
    }

    /// Status line after a successful pin.
    pub fn switched(self, role: Role, selector: &str) -> String {
        match self {
            Locale::En => format!("✔ pinned {} → {selector}", self.role_label(role)),
            Locale::Zh => format!("✔ 已把「{}」钉到 {selector}", self.role_label(role)),
        }
    }

    /// Status line after clearing a pin.
    pub fn cleared(self, role: Role) -> String {
        match self {
            Locale::En => format!("✔ cleared {} (auto)", self.role_label(role)),
            Locale::Zh => format!("✔ 已清除「{}」(自动)", self.role_label(role)),
        }
    }

    /// Status line for a query that matched nothing.
    pub fn no_match(self, query: &str) -> String {
        match self {
            Locale::En => format!("no model matches `{query}`"),
            Locale::Zh => format!("没有匹配 `{query}` 的模型"),
        }
    }

    /// Status line for an ambiguous query.
    pub fn ambiguous(self, query: &str, n: usize) -> String {
        match self {
            Locale::En => format!("`{query}` matches {n} models — be more specific"),
            Locale::Zh => format!("`{query}` 匹配到 {n} 个模型 —— 请更精确"),
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
