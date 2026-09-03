//! End-to-end use-case test with in-memory ports (no omp, no files).
//! Covers PRD AC3/AC4/AC6.

use std::time::SystemTime;

use anyhow::Result;

use headroom::app::ports::{HistoryStore, UsageSource};
use headroom::app::Evaluator;
use headroom::domain::{
    AccountId, AlertKind, AlertLevel, LimitScope, LimitWindow, ModelClass, Percent, ProviderId,
    QuotaSnapshot, Sample, Thresholds, Tier, WindowStatus,
};

struct FakeSource(QuotaSnapshot);
impl UsageSource for FakeSource {
    fn snapshots(&self) -> Result<Vec<QuotaSnapshot>> {
        Ok(vec![self.0.clone()])
    }
}

struct NoHistory;
impl HistoryStore for NoHistory {
    fn record(&self, _snapshot: &QuotaSnapshot) -> Result<()> {
        Ok(())
    }
    fn series(&self, _p: &ProviderId, _a: &AccountId, _w: &str) -> Result<Vec<Sample>> {
        Ok(Vec::new())
    }
}

fn window(id: &str, scope: LimitScope, remaining: u8, status: WindowStatus) -> LimitWindow {
    LimitWindow {
        id: id.into(),
        label: id.into(),
        scope,
        period: None,
        remaining: Percent::new(remaining),
        resets_at: None,
        status,
    }
}

fn snapshot(fable_remaining: u8, fable_status: WindowStatus) -> QuotaSnapshot {
    QuotaSnapshot {
        provider: ProviderId::new("anthropic"),
        account: AccountId::new("f6*"),
        plan: Some("max".into()),
        fetched_at: SystemTime::UNIX_EPOCH,
        windows: vec![
            window("anthropic:5h", LimitScope::Shared, 72, WindowStatus::Ok),
            window("anthropic:7d", LimitScope::Shared, 92, WindowStatus::Ok),
            window(
                "anthropic:7d:fable",
                LimitScope::Tier(Tier::new("fable")),
                fable_remaining,
                fable_status,
            ),
        ],
    }
}

fn assess(snap: QuotaSnapshot) -> headroom::app::Assessment {
    let source = FakeSource(snap);
    let history = NoHistory;
    let evaluator = Evaluator::new(&source, &history, Thresholds::default());
    evaluator.poll().expect("poll succeeds")
}

#[test]
fn effective_remaining_is_the_binding_minimum() {
    // fable full -> shared 5h (72) is the bottleneck for both classes.
    let a = assess(snapshot(100, WindowStatus::Ok));
    let acc = &a.accounts[0];

    let fable = acc
        .classes
        .iter()
        .find(|c| c.headroom.class == ModelClass::Tier(Tier::new("fable")))
        .unwrap();
    assert_eq!(fable.headroom.effective_remaining, Percent::new(72));
    assert_eq!(fable.headroom.bottleneck.id, "anthropic:5h");
    assert!(acc.alerts.is_empty());
}

#[test]
fn depleted_tier_alerts_critical_and_suggests_base() {
    // fable weekly at 4% -> fable critical; base still 72% -> suggested target.
    let a = assess(snapshot(4, WindowStatus::Ok));
    let acc = &a.accounts[0];

    let fable = acc
        .classes
        .iter()
        .find(|c| c.headroom.class == ModelClass::Tier(Tier::new("fable")))
        .unwrap();
    assert_eq!(fable.headroom.effective_remaining, Percent::new(4));
    assert_eq!(fable.headroom.bottleneck.id, "anthropic:7d:fable");

    let base = acc
        .classes
        .iter()
        .find(|c| c.headroom.class == ModelClass::Base)
        .unwrap();
    assert_eq!(base.headroom.effective_remaining, Percent::new(72));

    let crit = acc
        .alerts
        .iter()
        .find(|al| al.subject == "fable")
        .expect("fable alert");
    assert_eq!(crit.level, AlertLevel::Critical);
    assert_eq!(crit.kind, AlertKind::LowRemaining);
    let suggestion = crit.suggestion.as_ref().expect("suggestion present");
    assert_eq!(suggestion.class, "base");
    assert_eq!(suggestion.remaining, Percent::new(72));
}

#[test]
fn unavailable_tier_window_is_reported_unavailable() {
    let a = assess(snapshot(50, WindowStatus::Unavailable));
    let acc = &a.accounts[0];
    let fable = acc
        .classes
        .iter()
        .find(|c| c.headroom.class == ModelClass::Tier(Tier::new("fable")))
        .unwrap();
    assert!(!fable.headroom.available);
    assert!(acc
        .alerts
        .iter()
        .any(|al| al.subject == "fable" && al.level == AlertLevel::Critical));
}
