//! Contract test: the omp usage adapter maps the real (redacted) JSON output
//! into the domain model correctly. Covers PRD AC1/AC2.

use headroom::adapters::omp_usage::parse_usage;
use headroom::domain::{LimitScope, ModelClass, Tier, WindowStatus};

const ANTHROPIC: &[u8] = include_bytes!("fixtures/anthropic_usage.json");
const CODEX: &[u8] = include_bytes!("fixtures/openai_codex_usage.json");

#[test]
fn anthropic_maps_shared_and_tier_scopes() {
    let snaps = parse_usage(ANTHROPIC).expect("fixture parses");
    assert_eq!(snaps.len(), 1);
    let snap = &snaps[0];
    assert_eq!(snap.provider.as_str(), "anthropic");
    assert_eq!(snap.account.as_str(), "f6*");

    let five_hour = snap
        .windows
        .iter()
        .find(|w| w.id == "anthropic:5h")
        .expect("5h window present");
    assert_eq!(five_hour.scope, LimitScope::Shared);
    assert_eq!(five_hour.status, WindowStatus::Ok);

    let fable = snap
        .windows
        .iter()
        .find(|w| w.id == "anthropic:7d:fable")
        .expect("fable tier window present");
    assert_eq!(fable.scope, LimitScope::Tier(Tier::new("fable")));
}

#[test]
fn anthropic_binding_rule_separates_base_from_tier() {
    let snaps = parse_usage(ANTHROPIC).expect("fixture parses");
    let snap = &snaps[0];

    let classes = snap.model_classes();
    assert_eq!(
        classes,
        vec![ModelClass::Base, ModelClass::Tier(Tier::new("fable"))]
    );

    // Base is bound only by shared windows; fable additionally by its tier cap.
    let base_binding = snap.binding_windows(&ModelClass::Base).len();
    let fable_binding = snap
        .binding_windows(&ModelClass::Tier(Tier::new("fable")))
        .len();
    assert!(fable_binding > base_binding);
    assert!(base_binding >= 2);
}

#[test]
fn codex_parses_shared_windows() {
    let snaps = parse_usage(CODEX).expect("fixture parses");
    assert_eq!(snaps.len(), 1);
    let snap = &snaps[0];
    assert_eq!(snap.provider.as_str(), "openai-codex");
    assert!(snap
        .windows
        .iter()
        .any(|w| w.id == "openai-codex:primary" && w.scope == LimitScope::Shared));
    // Every codex fixture window is a valid percentage.
    for w in &snap.windows {
        assert!(w.remaining.get() <= 100);
    }
}
