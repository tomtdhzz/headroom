//! Limit windows and the snapshot aggregate, plus the BindingRule that maps a
//! model class to the windows that constrain it.

use std::time::{Duration, SystemTime};

use super::model::{AccountId, ModelClass, Percent, ProviderId, Tier};

/// What a limit window constrains.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LimitScope {
    /// Constrains every model under the provider (e.g. the shared 5h pool).
    Shared,
    /// Constrains only models of this tier (e.g. the Opus/Fable weekly cap).
    Tier(Tier),
}

/// Whether a window currently permits usage.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WindowStatus {
    Ok,
    Unavailable,
}

/// A single quota window (value object).
#[derive(Clone, Debug)]
pub struct LimitWindow {
    pub id: String,
    pub label: String,
    pub scope: LimitScope,
    pub period: Option<Duration>,
    pub remaining: Percent,
    pub resets_at: Option<SystemTime>,
    pub status: WindowStatus,
}

impl LimitWindow {
    /// The BindingRule predicate: does this window constrain the given class?
    ///
    /// Shared windows constrain all classes; a tier window constrains only the
    /// matching tier.
    pub fn constrains(&self, class: &ModelClass) -> bool {
        match &self.scope {
            LimitScope::Shared => true,
            LimitScope::Tier(t) => matches!(class, ModelClass::Tier(ct) if ct == t),
        }
    }
}

/// One point-in-time reading of every window for a single account (aggregate).
#[derive(Clone, Debug)]
pub struct QuotaSnapshot {
    pub provider: ProviderId,
    pub account: AccountId,
    pub plan: Option<String>,
    pub fetched_at: SystemTime,
    pub windows: Vec<LimitWindow>,
}

impl QuotaSnapshot {
    /// BindingRule applied: the windows constraining `class`
    /// = every `Shared` window plus any window scoped to this class's tier.
    pub fn binding_windows(&self, class: &ModelClass) -> Vec<&LimitWindow> {
        self.windows
            .iter()
            .filter(|w| w.constrains(class))
            .collect()
    }

    /// The observable model classes: always `Base`, plus one class per distinct
    /// tier that has a tier-scoped window in this snapshot.
    pub fn model_classes(&self) -> Vec<ModelClass> {
        let mut classes = vec![ModelClass::Base];
        let mut seen: Vec<Tier> = Vec::new();
        for w in &self.windows {
            if let LimitScope::Tier(t) = &w.scope {
                if !seen.contains(t) {
                    seen.push(t.clone());
                    classes.push(ModelClass::Tier(t.clone()));
                }
            }
        }
        classes
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn win(id: &str, scope: LimitScope, remaining: u8, status: WindowStatus) -> LimitWindow {
        LimitWindow {
            id: id.to_string(),
            label: id.to_string(),
            scope,
            period: None,
            remaining: Percent::new(remaining),
            resets_at: None,
            status,
        }
    }

    fn snapshot() -> QuotaSnapshot {
        QuotaSnapshot {
            provider: ProviderId::new("anthropic"),
            account: AccountId::new("f6*"),
            plan: None,
            fetched_at: SystemTime::UNIX_EPOCH,
            windows: vec![
                win("anthropic:5h", LimitScope::Shared, 72, WindowStatus::Ok),
                win("anthropic:7d", LimitScope::Shared, 92, WindowStatus::Ok),
                win(
                    "anthropic:7d:fable",
                    LimitScope::Tier(Tier::new("fable")),
                    100,
                    WindowStatus::Ok,
                ),
            ],
        }
    }

    #[test]
    fn model_classes_are_base_plus_tiers_with_windows() {
        let classes = snapshot().model_classes();
        assert_eq!(
            classes,
            vec![ModelClass::Base, ModelClass::Tier(Tier::new("fable"))]
        );
    }

    #[test]
    fn binding_rule_base_sees_only_shared() {
        let snap = snapshot();
        let ids: Vec<&str> = snap
            .binding_windows(&ModelClass::Base)
            .iter()
            .map(|w| w.id.as_str())
            .collect();
        assert_eq!(ids, vec!["anthropic:5h", "anthropic:7d"]);
    }

    #[test]
    fn binding_rule_tier_sees_shared_plus_its_tier() {
        let snap = snapshot();
        let ids: Vec<&str> = snap
            .binding_windows(&ModelClass::Tier(Tier::new("fable")))
            .iter()
            .map(|w| w.id.as_str())
            .collect();
        assert_eq!(
            ids,
            vec!["anthropic:5h", "anthropic:7d", "anthropic:7d:fable"]
        );
    }
}
