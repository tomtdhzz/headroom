//! Alert evaluation: turn headroom + forecast into ranked, *structured* signals.
//!
//! The domain emits facts only (kind, remaining, bottleneck, suggested target).
//! Human-readable, localized wording lives in the delivery layer — the domain
//! contains no display language.

use std::time::Duration;

use super::forecast::ExhaustionForecast;
use super::headroom::Headroom;
use super::model::{AccountId, Percent, ProviderId};

/// Severity, ordered so `Critical > Warn > Info`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum AlertLevel {
    Info,
    Warn,
    Critical,
}

impl AlertLevel {
    /// A stable, language-neutral tag (used for de-dup keys).
    pub fn tag(self) -> &'static str {
        match self {
            AlertLevel::Info => "INFO",
            AlertLevel::Warn => "WARN",
            AlertLevel::Critical => "CRIT",
        }
    }
}

/// Why an alert fired — the fact, not the sentence.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AlertKind {
    /// A binding window is unavailable / depleted.
    Unavailable,
    /// Effective remaining is at or below a threshold.
    LowRemaining,
    /// Projected to deplete within the ETA horizon.
    DepletingSoon,
}

/// Thresholds that drive alert levels.
#[derive(Clone, Debug)]
pub struct Thresholds {
    pub warn_pct: u8,
    pub critical_pct: u8,
    pub eta_warn: Duration,
}

impl Default for Thresholds {
    fn default() -> Self {
        Thresholds {
            warn_pct: 20,
            critical_pct: 5,
            eta_warn: Duration::from_secs(30 * 60),
        }
    }
}

/// A concrete place to switch to: a more-available class on the same account.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Suggestion {
    pub class: String,
    pub remaining: Percent,
}

/// A structured, actionable signal about one model class on one account.
#[derive(Clone, Debug)]
pub struct Alert {
    pub level: AlertLevel,
    pub provider: ProviderId,
    pub account: AccountId,
    /// The model class label in trouble.
    pub subject: String,
    pub kind: AlertKind,
    /// Effective remaining for the subject class.
    pub remaining: Percent,
    /// The bottleneck window's label.
    pub bottleneck: String,
    /// Present for `DepletingSoon`.
    pub eta: Option<Duration>,
    pub suggestion: Option<Suggestion>,
}

/// Evaluate alerts for a single account. A class raises at most one alert.
pub fn evaluate(
    provider: &ProviderId,
    account: &AccountId,
    assessed: &[(Headroom, ExhaustionForecast)],
    thresholds: &Thresholds,
) -> Vec<Alert> {
    let mut alerts = Vec::new();

    for (hr, forecast) in assessed {
        let remaining = hr.effective_remaining;
        let (level, kind, eta) = if !hr.available {
            (AlertLevel::Critical, AlertKind::Unavailable, None)
        } else if remaining.get() <= thresholds.critical_pct {
            (AlertLevel::Critical, AlertKind::LowRemaining, None)
        } else if remaining.get() <= thresholds.warn_pct {
            (AlertLevel::Warn, AlertKind::LowRemaining, None)
        } else if matches!(forecast.eta, Some(e) if e <= thresholds.eta_warn) {
            (AlertLevel::Warn, AlertKind::DepletingSoon, forecast.eta)
        } else {
            continue;
        };

        alerts.push(Alert {
            level,
            provider: provider.clone(),
            account: account.clone(),
            subject: hr.class.label(),
            kind,
            remaining,
            bottleneck: hr.bottleneck.label.clone(),
            eta,
            suggestion: suggest_switch(hr, assessed),
        });
    }

    alerts
}

/// The most-available other class with strictly more headroom.
fn suggest_switch(
    troubled: &Headroom,
    assessed: &[(Headroom, ExhaustionForecast)],
) -> Option<Suggestion> {
    assessed
        .iter()
        .map(|(hr, _)| hr)
        .filter(|hr| {
            hr.class != troubled.class
                && hr.available
                && hr.effective_remaining > troubled.effective_remaining
        })
        .max_by_key(|hr| hr.effective_remaining)
        .map(|hr| Suggestion {
            class: hr.class.label(),
            remaining: hr.effective_remaining,
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::model::{ModelClass, Percent, Tier};
    use crate::domain::window::{LimitScope, LimitWindow, WindowStatus};

    fn headroom(class: ModelClass, remaining: u8, available: bool) -> Headroom {
        Headroom {
            class,
            effective_remaining: Percent::new(remaining),
            bottleneck: LimitWindow {
                id: "b".into(),
                label: "7d:fable".into(),
                scope: LimitScope::Tier(Tier::new("fable")),
                period: None,
                remaining: Percent::new(remaining),
                resets_at: None,
                status: if available {
                    WindowStatus::Ok
                } else {
                    WindowStatus::Unavailable
                },
            },
            available,
        }
    }

    #[test]
    fn critical_low_tier_suggests_richer_base() {
        let assessed = vec![
            (
                headroom(ModelClass::Base, 72, true),
                ExhaustionForecast::default(),
            ),
            (
                headroom(ModelClass::Tier(Tier::new("fable")), 4, true),
                ExhaustionForecast::default(),
            ),
        ];
        let alerts = evaluate(
            &ProviderId::new("anthropic"),
            &AccountId::new("f6*"),
            &assessed,
            &Thresholds::default(),
        );
        assert_eq!(alerts.len(), 1);
        assert_eq!(alerts[0].level, AlertLevel::Critical);
        assert_eq!(alerts[0].kind, AlertKind::LowRemaining);
        assert_eq!(alerts[0].subject, "fable");
        let suggestion = alerts[0].suggestion.as_ref().unwrap();
        assert_eq!(suggestion.class, "base");
        assert_eq!(suggestion.remaining, Percent::new(72));
    }

    #[test]
    fn unavailable_class_is_critical() {
        let assessed = vec![(
            headroom(ModelClass::Tier(Tier::new("fable")), 50, false),
            ExhaustionForecast::default(),
        )];
        let alerts = evaluate(
            &ProviderId::new("anthropic"),
            &AccountId::new("f6*"),
            &assessed,
            &Thresholds::default(),
        );
        assert_eq!(alerts.len(), 1);
        assert_eq!(alerts[0].level, AlertLevel::Critical);
        assert_eq!(alerts[0].kind, AlertKind::Unavailable);
    }

    #[test]
    fn healthy_class_raises_nothing() {
        let assessed = vec![(
            headroom(ModelClass::Base, 72, true),
            ExhaustionForecast::default(),
        )];
        let alerts = evaluate(
            &ProviderId::new("anthropic"),
            &AccountId::new("f6*"),
            &assessed,
            &Thresholds::default(),
        );
        assert!(alerts.is_empty());
    }
}
