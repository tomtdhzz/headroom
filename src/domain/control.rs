//! Control plane: the ubiquitous language of *switching* which model backs each
//! omp role. Pure and IO-free — the `omp config`/`omp models` calls live in the
//! adapter; this layer only models roles, model references, the current pins,
//! and how a free-text query resolves to exactly one model.
//!
//! A "role" is omp's assignment slot (`default`, `plan`, `slow`, `smol`,
//! `advisor`) — the Clash "策略组" analog. A pin binds a role to a concrete
//! model selector; clearing a pin hands the choice back to omp (the "auto"/
//! URLTest analog).

use std::collections::BTreeMap;
use std::fmt;

use super::model::ProviderId;

/// An omp model-assignment slot. These are the switchable "policy groups".
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Role {
    /// The primary interactive model.
    Default,
    /// Architectural planning model (`--plan`).
    Plan,
    /// Thorough/reasoning model (`--slow`).
    Slow,
    /// Fast/cheap model for lightweight tasks (`--smol`).
    Smol,
    /// Passive second-opinion reviewer (`advisor.enabled`).
    Advisor,
}

impl Role {
    /// Display/iteration order: most consequential first.
    pub const ALL: [Role; 5] = [
        Role::Default,
        Role::Plan,
        Role::Slow,
        Role::Smol,
        Role::Advisor,
    ];

    /// The omp `modelRoles` record key.
    pub fn key(self) -> &'static str {
        match self {
            Role::Default => "default",
            Role::Plan => "plan",
            Role::Slow => "slow",
            Role::Smol => "smol",
            Role::Advisor => "advisor",
        }
    }

    /// Parse a role from a CLI token (case-insensitive; accepts the key).
    pub fn parse(s: &str) -> Option<Role> {
        match s.trim().to_ascii_lowercase().as_str() {
            "default" | "def" | "main" => Some(Role::Default),
            "plan" => Some(Role::Plan),
            "slow" => Some(Role::Slow),
            "smol" | "fast" => Some(Role::Smol),
            "advisor" | "adv" => Some(Role::Advisor),
            _ => None,
        }
    }
}

impl fmt::Display for Role {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.key())
    }
}

/// Real capability + price metadata for a model, sourced from `omp models`.
/// Prices are USD per 1M tokens (omp's list price — a *relative* cost signal;
/// subscription quota is tracked separately by the usage windows).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ModelCaps {
    pub cost_in: Option<f64>,
    pub cost_out: Option<f64>,
    /// Context window in tokens.
    pub context: Option<u32>,
    /// Accepts image input (vision) — good for screenshot/diagram-reading tasks.
    pub vision: bool,
    /// Exposes a reasoning/thinking mode — good for planning/hard problems.
    pub reasoning: bool,
}

impl ModelCaps {
    /// The price signal used for ranking: output price (dominant), else input.
    /// `None` when the model has no price at all.
    pub fn price_key(&self) -> Option<f64> {
        self.cost_out.or(self.cost_in)
    }
}

/// A concrete model that can back a role — the Clash "节点" analog.
#[derive(Clone, Debug, PartialEq)]
pub struct ModelRef {
    pub provider: ProviderId,
    /// The fully-qualified selector omp accepts, e.g. `anthropic/claude-opus-4`.
    pub selector: String,
    /// Human-friendly name, e.g. `Claude Opus 4`.
    pub name: String,
    /// Price + capability metadata (default/empty when omp omits it).
    pub caps: ModelCaps,
}

impl ModelRef {
    pub fn new(provider: ProviderId, selector: impl Into<String>, name: impl Into<String>) -> Self {
        ModelRef {
            provider,
            selector: selector.into(),
            name: name.into(),
            caps: ModelCaps::default(),
        }
    }

    pub fn with_caps(mut self, caps: ModelCaps) -> Self {
        self.caps = caps;
        self
    }
}

/// The current role→selector assignments (omp's `modelRoles` record). Unset
/// roles are absent (omp picks). Ordered for deterministic serialization.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RolePins {
    map: BTreeMap<String, String>,
}

impl RolePins {
    pub fn from_pairs(pairs: impl IntoIterator<Item = (String, String)>) -> Self {
        RolePins {
            map: pairs.into_iter().collect(),
        }
    }

    /// The selector pinned to `role`, if any.
    pub fn get(&self, role: Role) -> Option<&str> {
        self.map.get(role.key()).map(String::as_str)
    }

    /// Pin `role` to `selector`.
    pub fn set(&mut self, role: Role, selector: impl Into<String>) {
        self.map.insert(role.key().to_string(), selector.into());
    }

    /// Clear `role`'s pin (hand the choice back to omp). Returns whether a pin
    /// was removed.
    pub fn clear(&mut self, role: Role) -> bool {
        self.map.remove(role.key()).is_some()
    }

    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    /// Every (role-key, selector) pair, ordered — including keys for roles this
    /// enum doesn't model (preserved verbatim so a write never drops them).
    pub fn pairs(&self) -> impl Iterator<Item = (&str, &str)> {
        self.map.iter().map(|(k, v)| (k.as_str(), v.as_str()))
    }
}

/// The outcome of resolving a free-text query against the model catalog.
#[derive(Clone, Debug, PartialEq)]
pub enum Resolve {
    /// Exactly one model matched.
    Unique(ModelRef),
    /// No model matched.
    None,
    /// Several matched — the caller must disambiguate.
    Ambiguous(Vec<ModelRef>),
}

/// Resolve a query to one model, cc-switch/omp `--model`-style fuzzy matching.
///
/// Precedence (first non-empty tier wins): exact selector → exact name (ci) →
/// selector substring → name substring. Within a tier, an unambiguous single
/// hit resolves; ties are reported as `Ambiguous`. A query that names a
/// provider prefix (`anthropic/opus`) is matched against the selector.
pub fn resolve_model(models: &[ModelRef], query: &str) -> Resolve {
    let q = query.trim().to_ascii_lowercase();
    if q.is_empty() {
        return Resolve::None;
    }

    let exact_selector: Vec<&ModelRef> = models
        .iter()
        .filter(|m| m.selector.to_ascii_lowercase() == q)
        .collect();
    let exact_name: Vec<&ModelRef> = models
        .iter()
        .filter(|m| m.name.to_ascii_lowercase() == q)
        .collect();
    let sub_selector: Vec<&ModelRef> = models
        .iter()
        .filter(|m| m.selector.to_ascii_lowercase().contains(&q))
        .collect();
    let sub_name: Vec<&ModelRef> = models
        .iter()
        .filter(|m| m.name.to_ascii_lowercase().contains(&q))
        .collect();

    for tier in [exact_selector, exact_name, sub_selector, sub_name] {
        match tier.as_slice() {
            [] => continue,
            [one] => return Resolve::Unique((*one).clone()),
            many => return Resolve::Ambiguous(many.iter().map(|m| (*m).clone()).collect()),
        }
    }
    Resolve::None
}

/// A task-shaped recommendation axis, backed only by real `omp models` fields.
/// (No "drawing/generation" axis — these are coding models; `vision` means they
/// can *read* images, not create them.)
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fit {
    /// Cheapest by list price — the quota-alert escape hatch.
    Cheap,
    /// Accepts image input (screenshots/diagrams).
    Vision,
    /// Exposes reasoning/thinking (planning, hard problems).
    Reasoning,
    /// Largest context window (big files, long chats).
    LongContext,
}

/// The best model in `models` for `fit`, or `None` if nothing qualifies.
/// Ties break toward the cheaper model, then the shorter selector (stable).
pub fn top_for(models: &[ModelRef], fit: Fit) -> Option<&ModelRef> {
    let cheapest = |a: &&ModelRef, b: &&ModelRef| {
        let pa = a.caps.price_key().unwrap_or(f64::INFINITY);
        let pb = b.caps.price_key().unwrap_or(f64::INFINITY);
        pa.total_cmp(&pb)
            .then(a.selector.len().cmp(&b.selector.len()))
    };
    match fit {
        Fit::Cheap => models
            .iter()
            .filter(|m| m.caps.price_key().is_some())
            .min_by(cheapest),
        Fit::Vision => models.iter().filter(|m| m.caps.vision).min_by(cheapest),
        Fit::Reasoning => models.iter().filter(|m| m.caps.reasoning).min_by(cheapest),
        Fit::LongContext => models
            .iter()
            .filter(|m| m.caps.context.is_some())
            .max_by_key(|m| m.caps.context.unwrap_or(0)),
    }
}

/// Relative price bucket of a model within a catalog, by output-price terciles.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PriceTier {
    Cheap,
    Mid,
    Pricey,
    Unknown,
}

/// Bucket `model`'s price against the `models` catalog (lower third = cheap,
/// upper third = pricey). `Unknown` when the model has no price.
pub fn price_tier(models: &[ModelRef], model: &ModelRef) -> PriceTier {
    let Some(p) = model.caps.price_key() else {
        return PriceTier::Unknown;
    };
    let mut prices: Vec<f64> = models.iter().filter_map(|m| m.caps.price_key()).collect();
    if prices.len() < 3 {
        return PriceTier::Mid;
    }
    prices.sort_by(f64::total_cmp);
    let lo = prices[prices.len() / 3];
    let hi = prices[prices.len() * 2 / 3];
    if p <= lo {
        PriceTier::Cheap
    } else if p >= hi {
        PriceTier::Pricey
    } else {
        PriceTier::Mid
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(sel: &str, name: &str) -> ModelRef {
        let provider = sel.split('/').next().unwrap_or("");
        ModelRef::new(ProviderId::new(provider), sel, name)
    }

    fn catalog() -> Vec<ModelRef> {
        vec![
            m("anthropic/claude-opus-4", "Claude Opus 4"),
            m("anthropic/claude-sonnet-4", "Claude Sonnet 4"),
            m("anthropic/claude-3-haiku-20240307", "Claude Haiku 3"),
            m("openai-codex/gpt-5.3-codex", "GPT-5.3 Codex"),
        ]
    }

    #[test]
    fn role_roundtrips_through_key() {
        for r in Role::ALL {
            assert_eq!(Role::parse(r.key()), Some(r));
        }
        assert_eq!(Role::parse("FAST"), Some(Role::Smol));
        assert_eq!(Role::parse("nope"), None);
    }

    #[test]
    fn pins_set_clear_and_preserve_unknown_roles() {
        let mut pins = RolePins::from_pairs([("mystery".to_string(), "x/y".to_string())]);
        pins.set(Role::Default, "anthropic/claude-opus-4");
        assert_eq!(pins.get(Role::Default), Some("anthropic/claude-opus-4"));
        assert!(pins.clear(Role::Default));
        assert!(!pins.clear(Role::Default));
        // Unknown role key survives round-trips (never silently dropped).
        let keys: Vec<&str> = pins.pairs().map(|(k, _)| k).collect();
        assert_eq!(keys, ["mystery"]);
    }

    #[test]
    fn resolve_unique_substring() {
        assert_eq!(
            resolve_model(&catalog(), "opus"),
            Resolve::Unique(m("anthropic/claude-opus-4", "Claude Opus 4"))
        );
    }

    #[test]
    fn resolve_exact_selector_beats_substring() {
        // "claude-sonnet-4" is an exact selector suffix but not a full selector;
        // exact selector requires the full string.
        let r = resolve_model(&catalog(), "anthropic/claude-sonnet-4");
        assert_eq!(
            r,
            Resolve::Unique(m("anthropic/claude-sonnet-4", "Claude Sonnet 4"))
        );
    }

    #[test]
    fn resolve_ambiguous_lists_all() {
        match resolve_model(&catalog(), "claude") {
            Resolve::Ambiguous(v) => assert_eq!(v.len(), 3),
            other => panic!("expected ambiguous, got {other:?}"),
        }
    }

    #[test]
    fn resolve_none_when_no_match() {
        assert_eq!(resolve_model(&catalog(), "gemini"), Resolve::None);
        assert_eq!(resolve_model(&catalog(), "  "), Resolve::None);
    }

    fn caps(cost_out: f64, ctx: u32, vision: bool, reasoning: bool) -> ModelCaps {
        ModelCaps {
            cost_in: Some(cost_out / 5.0),
            cost_out: Some(cost_out),
            context: Some(ctx),
            vision,
            reasoning,
        }
    }

    fn priced_catalog() -> Vec<ModelRef> {
        vec![
            m("anthropic/claude-opus-4", "Opus").with_caps(caps(25.0, 1_000_000, true, true)),
            m("anthropic/claude-haiku", "Haiku").with_caps(caps(4.0, 200_000, true, false)),
            m("openai-codex/gpt-5", "GPT-5").with_caps(caps(10.0, 400_000, false, true)),
            m("anthropic/claude-text", "Text").with_caps(caps(1.0, 100_000, false, false)),
        ]
    }

    #[test]
    fn top_for_picks_by_axis() {
        let cat = priced_catalog();
        assert_eq!(
            top_for(&cat, Fit::Cheap).unwrap().selector,
            "anthropic/claude-text"
        );
        assert_eq!(
            top_for(&cat, Fit::LongContext).unwrap().selector,
            "anthropic/claude-opus-4"
        );
        // Vision → cheapest vision-capable (Haiku over Opus).
        assert_eq!(
            top_for(&cat, Fit::Vision).unwrap().selector,
            "anthropic/claude-haiku"
        );
        // Reasoning → cheapest reasoning-capable (GPT-5 over Opus).
        assert_eq!(
            top_for(&cat, Fit::Reasoning).unwrap().selector,
            "openai-codex/gpt-5"
        );
    }

    #[test]
    fn top_for_none_when_axis_unsatisfiable() {
        let plain = vec![m("x/y", "Y")]; // no caps, no price/vision/etc.
        assert!(top_for(&plain, Fit::Cheap).is_none());
        assert!(top_for(&plain, Fit::Vision).is_none());
    }

    #[test]
    fn price_tier_buckets_by_terciles() {
        let cat = priced_catalog();
        let cheap = m("anthropic/claude-text", "Text").with_caps(caps(1.0, 100_000, false, false));
        let dear =
            m("anthropic/claude-opus-4", "Opus").with_caps(caps(25.0, 1_000_000, true, true));
        assert_eq!(price_tier(&cat, &cheap), PriceTier::Cheap);
        assert_eq!(price_tier(&cat, &dear), PriceTier::Pricey);
        // No price → Unknown.
        assert_eq!(price_tier(&cat, &m("x/y", "Y")), PriceTier::Unknown);
    }
}
