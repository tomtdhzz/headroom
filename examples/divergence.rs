//! Renders the exact scenario asserted by the `depleted_tier_alerts_...` test,
//! so you can see base vs. fable converge (shared bottleneck) then diverge
//! (tier cap becomes the bottleneck). Uses fake data — no real account touched.
//!
//! Run: `cargo run --example divergence`

use std::time::SystemTime;

use anyhow::Result;

use headroom::app::ports::{HistoryStore, UsageSource};
use headroom::app::Evaluator;
use headroom::delivery::cli::{render, Palette};
use headroom::delivery::Locale;
use headroom::domain::{
    AccountId, LimitScope, LimitWindow, Percent, ProviderId, QuotaSnapshot, Sample, Thresholds,
    Tier, WindowStatus,
};

struct Fixed(QuotaSnapshot);
impl UsageSource for Fixed {
    fn snapshots(&self) -> Result<Vec<QuotaSnapshot>> {
        Ok(vec![self.0.clone()])
    }
}

struct NoHistory;
impl HistoryStore for NoHistory {
    fn record(&self, _: &QuotaSnapshot) -> Result<()> {
        Ok(())
    }
    fn series(&self, _: &ProviderId, _: &AccountId, _: &str) -> Result<Vec<Sample>> {
        Ok(Vec::new())
    }
}

fn win(id: &str, scope: LimitScope, remaining: u8) -> LimitWindow {
    LimitWindow {
        id: id.into(),
        label: id.into(),
        scope,
        period: None,
        remaining: Percent::new(remaining),
        resets_at: None,
        status: WindowStatus::Ok,
    }
}

fn snap(five_hour: u8, fable_weekly: u8) -> QuotaSnapshot {
    QuotaSnapshot {
        provider: ProviderId::new("anthropic"),
        account: AccountId::new("f6*"),
        plan: Some("max".into()),
        fetched_at: SystemTime::now(),
        windows: vec![
            win("Claude 5 Hour", LimitScope::Shared, five_hour),
            win("Claude 7 Day", LimitScope::Shared, 92),
            win(
                "Claude 7 Day (Fable)",
                LimitScope::Tier(Tier::new("fable")),
                fable_weekly,
            ),
        ],
    }
}

fn show(title: &str, snapshot: QuotaSnapshot) {
    let source = Fixed(snapshot);
    let history = NoHistory;
    let evaluator = Evaluator::new(&source, &history, Thresholds::default());
    let assessment = evaluator.poll().expect("poll");
    println!("\n========== {title} ==========");
    print!(
        "{}",
        render(&assessment, SystemTime::now(), &Palette::auto(), Locale::En)
    );
}

fn main() {
    // 5h pool (55%) is the common bottleneck → base and fable read the same.
    show(
        "A) 5h 共享池最紧 → base 与 fable 相同(=你现在看到的真实情况)",
        snap(55, 100),
    );
    // 5h refilled (72%), fable weekly nearly spent (4%) → they diverge, alert fires.
    show(
        "B) 5h 已回满、fable 周帽见底 → 分叉:降到 base 仍有 72%",
        snap(72, 4),
    );
}
