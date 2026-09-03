//! Headroom: the effective remaining quota for a model class under all the
//! windows that bind it.

use super::model::{ModelClass, Percent};
use super::window::{LimitWindow, QuotaSnapshot, WindowStatus};

/// The computed headroom for one model class.
#[derive(Clone, Debug)]
pub struct Headroom {
    pub class: ModelClass,
    /// The binding minimum: you can only use a model as much as its scarcest
    /// constraining window allows.
    pub effective_remaining: Percent,
    /// The window that produced the minimum — the thing that will stop you.
    pub bottleneck: LimitWindow,
    /// True when every binding window is `Ok` and some quota remains.
    pub available: bool,
}

impl Headroom {
    /// Compute headroom for `class` in `snapshot`, or `None` when no window
    /// binds the class.
    pub fn for_class(snapshot: &QuotaSnapshot, class: &ModelClass) -> Option<Headroom> {
        let binding = snapshot.binding_windows(class);
        if binding.is_empty() {
            return None;
        }
        // `min_by_key` returns the first minimum on ties → deterministic.
        let bottleneck = *binding
            .iter()
            .min_by_key(|w| w.remaining)
            .expect("binding is non-empty");
        let effective = bottleneck.remaining;
        let available =
            binding.iter().all(|w| w.status == WindowStatus::Ok) && !effective.is_zero();
        Some(Headroom {
            class: class.clone(),
            effective_remaining: effective,
            bottleneck: bottleneck.clone(),
            available,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::model::{AccountId, ProviderId, Tier};
    use crate::domain::window::{LimitScope, QuotaSnapshot};
    use std::time::SystemTime;

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

    fn snap(fable_remaining: u8, fable_status: WindowStatus) -> QuotaSnapshot {
        QuotaSnapshot {
            provider: ProviderId::new("anthropic"),
            account: AccountId::new("f6*"),
            plan: None,
            fetched_at: SystemTime::UNIX_EPOCH,
            windows: vec![
                win("5h", LimitScope::Shared, 72, WindowStatus::Ok),
                win("7d", LimitScope::Shared, 92, WindowStatus::Ok),
                win(
                    "7d:fable",
                    LimitScope::Tier(Tier::new("fable")),
                    fable_remaining,
                    fable_status,
                ),
            ],
        }
    }

    #[test]
    fn shared_pool_is_the_bottleneck_when_tier_is_full() {
        let s = snap(100, WindowStatus::Ok);
        let hr = Headroom::for_class(&s, &ModelClass::Tier(Tier::new("fable"))).unwrap();
        assert_eq!(hr.effective_remaining, Percent::new(72));
        assert_eq!(hr.bottleneck.id, "5h");
        assert!(hr.available);
    }

    #[test]
    fn tier_cap_becomes_bottleneck_but_base_is_unaffected() {
        let s = snap(4, WindowStatus::Ok);
        let fable = Headroom::for_class(&s, &ModelClass::Tier(Tier::new("fable"))).unwrap();
        assert_eq!(fable.effective_remaining, Percent::new(4));
        assert_eq!(fable.bottleneck.id, "7d:fable");

        let base = Headroom::for_class(&s, &ModelClass::Base).unwrap();
        assert_eq!(base.effective_remaining, Percent::new(72));
    }

    #[test]
    fn unavailable_binding_window_makes_class_unavailable() {
        let s = snap(50, WindowStatus::Unavailable);
        let fable = Headroom::for_class(&s, &ModelClass::Tier(Tier::new("fable"))).unwrap();
        assert!(!fable.available);
    }
}
