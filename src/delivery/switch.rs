//! Text rendering for the scriptable switch commands (`roles` / `use` /
//! `clear`). Pure string building — no TTY, no color — so it stays testable and
//! pipe-friendly. The interactive Clash-style pane lives in `tui`.

use crate::app::Outcome;
use crate::domain::{ModelRef, Role, RolePins};

use super::i18n::Locale;

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
}
