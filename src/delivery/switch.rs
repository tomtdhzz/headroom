//! Text rendering for the scriptable switch commands (`roles` / `use` /
//! `clear`). Pure string building — no TTY, no color — so it stays testable and
//! pipe-friendly. The interactive Clash-style pane lives in `tui`.

use crate::app::Outcome;
use crate::domain::{top_for, Fit, ModelRef, Role, RolePins};

use super::i18n::Locale;
use super::{caps_tags, fmt_ctx, fmt_price};

/// Render the current policy groups: each role and the model pinned to it (or
/// `auto`), followed by a short how-to line.
pub fn render_roles(pins: &RolePins, loc: Locale) -> String {
    let mut out = String::new();
    out.push_str(loc.groups_heading());
    out.push('\n');
    for role in Role::ALL {
        let pin = pins
            .get(role)
            .map(str::to_string)
            .unwrap_or_else(|| loc.unset().to_string());
        out.push_str(&format!(
            "  {:<8} {:<40} {}\n",
            role.key(),
            pin,
            loc.role_hint(role)
        ));
    }
    out.push('\n');
    match loc {
        Locale::En => out
            .push_str("switch:  headroom use <role> <model>   ·   clear: headroom clear <role>\n"),
        Locale::Zh => {
            out.push_str("切换:  headroom use <角色> <模型>   ·   清除: headroom clear <角色>\n")
        }
    }
    out
}

/// Render the result of a `use` / `clear` attempt for the CLI.
pub fn render_outcome(outcome: &Outcome, loc: Locale) -> String {
    match outcome {
        Outcome::Pinned { role, model, .. } => loc.switched(*role, &model.selector),
        Outcome::Cleared { role, .. } => loc.cleared(*role),
        Outcome::NoMatch { query } => loc.no_match(query),
        Outcome::Ambiguous { query, candidates } => {
            let mut s = loc.ambiguous(query, candidates.len());
            for m in candidates.iter().take(12) {
                s.push_str(&format!("\n    {}", m.selector));
            }
            if candidates.len() > 12 {
                s.push_str("\n    …");
            }
            s
        }
    }
}

/// Group a model catalog by provider, preserving first-seen provider order and
/// per-provider order. Used by the interactive node list.
pub fn group_by_provider(models: &[ModelRef]) -> Vec<(String, Vec<&ModelRef>)> {
    let mut groups: Vec<(String, Vec<&ModelRef>)> = Vec::new();
    for m in models {
        let p = m.provider.as_str();
        match groups.iter_mut().find(|(name, _)| name == p) {
            Some((_, v)) => v.push(m),
            None => groups.push((p.to_string(), vec![m])),
        }
    }
    groups
}

/// Render the model catalog with price + capabilities, grouped by provider,
/// preceded by task recommendations. Optionally filtered by `provider` and a
/// free-text `query` substring. Pure text (scriptable).
pub fn render_models(
    models: &[ModelRef],
    provider: Option<&str>,
    query: Option<&str>,
    loc: Locale,
) -> String {
    let q = query.map(|s| s.to_ascii_lowercase());
    let sel: Vec<ModelRef> = models
        .iter()
        .filter(|m| provider.is_none_or(|p| m.provider.as_str() == p))
        .filter(|m| {
            q.as_deref().is_none_or(|q| {
                m.selector.to_ascii_lowercase().contains(q)
                    || m.name.to_ascii_lowercase().contains(q)
            })
        })
        .cloned()
        .collect();

    let mut out = String::new();

    // Recommendation summary (real capabilities only).
    out.push_str(loc.recommend_prefix());
    out.push_str("  ");
    let short = |s: &str| s.rsplit('/').next().unwrap_or(s).to_string();
    let axes = [
        (Fit::Cheap, loc.rec_cheap()),
        (Fit::Vision, loc.rec_vision()),
        (Fit::Reasoning, loc.rec_reasoning()),
        (Fit::LongContext, loc.rec_long()),
    ];
    let mut recs = Vec::new();
    for (fit, label) in axes {
        if let Some(m) = top_for(&sel, fit) {
            recs.push(format!("{label}:{}", short(&m.selector)));
        }
    }
    out.push_str(&recs.join(" · "));
    out.push_str("\n\n");

    // Catalog grouped by provider.
    for (prov, group) in group_by_provider(&sel) {
        out.push_str(&format!("{prov}\n"));
        for m in group {
            let price = match (m.caps.cost_in, m.caps.cost_out) {
                (Some(i), Some(o)) => format!("{}/{}", fmt_price(i), fmt_price(o)),
                (None, Some(o)) => fmt_price(o),
                (Some(i), None) => fmt_price(i),
                (None, None) => "—".to_string(),
            };
            let ctx = m
                .caps
                .context
                .map(fmt_ctx)
                .unwrap_or_else(|| "—".to_string());
            let tags = caps_tags(&m.caps);
            out.push_str(&format!(
                "  {:<40} {:>11}  {:>5}  {}\n",
                m.selector, price, ctx, tags
            ));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::ProviderId;

    fn model(sel: &str) -> ModelRef {
        let p = sel.split('/').next().unwrap_or("");
        ModelRef::new(ProviderId::new(p), sel, sel)
    }

    #[test]
    fn roles_render_shows_pin_or_auto() {
        let pins = RolePins::from_pairs([("default".to_string(), "anthropic/opus".to_string())]);
        let s = render_roles(&pins, Locale::En);
        assert!(s.contains("default"));
        assert!(s.contains("anthropic/opus"));
        assert!(s.contains("auto")); // unset roles show auto
    }

    #[test]
    fn outcome_ambiguous_lists_candidates() {
        let out = Outcome::Ambiguous {
            query: "claude".to_string(),
            candidates: vec![model("anthropic/a"), model("anthropic/b")],
        };
        let s = render_outcome(&out, Locale::En);
        assert!(s.contains("anthropic/a"));
        assert!(s.contains("anthropic/b"));
    }

    #[test]
    fn grouping_preserves_provider_order() {
        let models = vec![
            model("anthropic/a"),
            model("openai-codex/x"),
            model("anthropic/b"),
        ];
        let g = group_by_provider(&models);
        assert_eq!(g.len(), 2);
        assert_eq!(g[0].0, "anthropic");
        assert_eq!(g[0].1.len(), 2);
        assert_eq!(g[1].0, "openai-codex");
    }

    #[test]
    fn models_render_shows_price_caps_and_recommendations() {
        use crate::domain::ModelCaps;
        let cheap = ModelRef::new(ProviderId::new("anthropic"), "anthropic/haiku", "Haiku")
            .with_caps(ModelCaps {
                cost_in: Some(0.8),
                cost_out: Some(4.0),
                context: Some(200_000),
                vision: true,
                reasoning: false,
            });
        let dear = ModelRef::new(ProviderId::new("anthropic"), "anthropic/opus", "Opus").with_caps(
            ModelCaps {
                cost_in: Some(5.0),
                cost_out: Some(25.0),
                context: Some(1_000_000),
                vision: true,
                reasoning: true,
            },
        );
        let s = render_models(&[dear, cheap], None, None, Locale::En);
        assert!(s.contains("$0.8/$4"), "renders price pair");
        assert!(s.contains("200k"), "renders context");
        assert!(s.contains("cheapest:haiku"), "recommends the cheaper model");
        assert!(
            s.contains("reasoning:opus"),
            "recommends the reasoning model"
        );
    }
}
