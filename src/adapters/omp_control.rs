//! `ModelControl` backed by the `omp` CLI: `omp models --json` for the catalog
//! and `omp config get/set modelRoles` for the pins.
//!
//! This is the *write* adapter — the only place headroom mutates omp state. It
//! writes the full `modelRoles` record (dotted keys like `modelRoles.default`
//! are rejected by omp) and verifies the change by reading it back.

use std::process::Command;

use anyhow::{anyhow, bail, Context, Result};
use serde::Deserialize;
use serde_json::{Map, Value};

use crate::app::ports::ModelControl;
use crate::domain::{ModelCaps, ModelRef, ProviderId, Role, RolePins};

/// Drives the `omp` CLI to read the model catalog and read/write role pins.
pub struct OmpModelControl {
    program: String,
}

impl Default for OmpModelControl {
    fn default() -> Self {
        Self::new()
    }
}

impl OmpModelControl {
    pub fn new() -> Self {
        OmpModelControl {
            program: "omp".to_string(),
        }
    }

    /// Run `omp <args…>` and return stdout, mapping the usual failure modes to
    /// actionable errors.
    fn run(&self, args: &[&str]) -> Result<Vec<u8>> {
        let output = Command::new(&self.program)
            .args(args)
            .output()
            .map_err(|e| {
                if e.kind() == std::io::ErrorKind::NotFound {
                    anyhow!(
                        "`{}` not found on PATH; install and authenticate omp first",
                        self.program
                    )
                } else {
                    anyhow!("failed to run `{} {}`: {e}", self.program, args.join(" "))
                }
            })?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let msg = stderr.trim();
            if msg.is_empty() {
                bail!(
                    "`omp {}` failed with status {}",
                    args.join(" "),
                    output.status
                );
            }
            bail!("`omp {}` failed: {msg}", args.join(" "));
        }
        Ok(output.stdout)
    }
}

impl ModelControl for OmpModelControl {
    fn available_models(&self) -> Result<Vec<ModelRef>> {
        let raw = self.run(&["models", "--json"])?;
        parse_models(&raw)
    }

    fn pins(&self) -> Result<RolePins> {
        let raw = self.run(&["config", "get", "modelRoles", "--json"])?;
        parse_pins(&raw)
    }

    fn write_pins(&self, pins: &RolePins) -> Result<RolePins> {
        let mut map = Map::new();
        for (k, v) in pins.pairs() {
            map.insert(k.to_string(), Value::String(v.to_string()));
        }
        let json = Value::Object(map).to_string();
        self.run(&["config", "set", "modelRoles", &json])
            .context("writing modelRoles")?;
        // Verify by read-back: never trust the write silently.
        let verified = self.pins()?;
        if &verified != pins {
            bail!("modelRoles write did not verify (omp reports different pins after set)");
        }
        Ok(verified)
    }
}

// ---- omp JSON DTOs (kept private to this adapter) --------------------------

#[derive(Deserialize)]
struct ModelsResponse {
    models: Vec<ModelDto>,
}

#[derive(Deserialize)]
struct ModelDto {
    provider: String,
    selector: String,
    name: Option<String>,
    #[serde(rename = "contextWindow")]
    context_window: Option<u32>,
    #[serde(default)]
    reasoning: bool,
    #[serde(default)]
    input: Vec<String>,
    cost: Option<CostDto>,
}

#[derive(Deserialize)]
struct CostDto {
    input: Option<f64>,
    output: Option<f64>,
}

#[derive(Deserialize)]
struct ConfigGet {
    value: Option<Value>,
}

/// Parse `omp models --json` into domain model references. Public for testing.
pub fn parse_models(raw: &[u8]) -> Result<Vec<ModelRef>> {
    let resp: ModelsResponse =
        serde_json::from_slice(raw).context("parsing `omp models --json`")?;
    Ok(resp
        .models
        .into_iter()
        .map(|m| {
            let name = m.name.clone().unwrap_or_else(|| m.selector.clone());
            let caps = ModelCaps {
                cost_in: m.cost.as_ref().and_then(|c| c.input),
                cost_out: m.cost.as_ref().and_then(|c| c.output),
                context: m.context_window,
                vision: m.input.iter().any(|i| i == "image"),
                reasoning: m.reasoning,
            };
            ModelRef::new(ProviderId::new(m.provider), m.selector, name).with_caps(caps)
        })
        .collect())
}

/// Parse `omp config get modelRoles --json` into pins. Non-string values and
/// unexpected shapes yield an empty/partial record rather than failing — the
/// point is a best-effort read of what omp currently holds. Public for testing.
pub fn parse_pins(raw: &[u8]) -> Result<RolePins> {
    let got: ConfigGet = serde_json::from_slice(raw).context("parsing `omp config get`")?;
    let pairs = match got.value {
        Some(Value::Object(map)) => map
            .into_iter()
            .filter_map(|(k, v)| match v {
                Value::String(s) => Some((k, s)),
                _ => None,
            })
            .collect::<Vec<_>>(),
        _ => Vec::new(),
    };
    Ok(RolePins::from_pairs(pairs))
}

/// Convenience: pins restricted to the roles headroom models, in display order.
pub fn known_pins(pins: &RolePins) -> Vec<(Role, Option<String>)> {
    Role::ALL
        .iter()
        .map(|&r| (r, pins.get(r).map(str::to_string)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_model_catalog() {
        let raw = br#"{"models":[
            {"provider":"anthropic","selector":"anthropic/claude-opus-4","name":"Claude Opus 4",
             "contextWindow":1000000,"reasoning":true,"input":["text","image"],
             "cost":{"input":5,"output":25,"cacheRead":0.5,"cacheWrite":6.25}},
            {"provider":"openai-codex","selector":"openai-codex/gpt-5.3-codex"}
        ]}"#;
        let models = parse_models(raw).unwrap();
        assert_eq!(models.len(), 2);
        assert_eq!(models[0].provider.as_str(), "anthropic");
        assert_eq!(models[0].name, "Claude Opus 4");
        assert_eq!(models[0].caps.cost_in, Some(5.0));
        assert_eq!(models[0].caps.cost_out, Some(25.0));
        assert_eq!(models[0].caps.context, Some(1_000_000));
        assert!(models[0].caps.vision);
        assert!(models[0].caps.reasoning);
        // Missing name falls back to selector; absent caps default to empty.
        assert_eq!(models[1].name, "openai-codex/gpt-5.3-codex");
        assert_eq!(models[1].caps.cost_out, None);
        assert!(!models[1].caps.vision);
    }

    #[test]
    fn parses_pins_record() {
        let raw = br#"{"key":"modelRoles","value":{"default":"anthropic/claude-opus-4","smol":"anthropic/claude-3-haiku"},"type":"record"}"#;
        let pins = parse_pins(raw).unwrap();
        assert_eq!(pins.get(Role::Default), Some("anthropic/claude-opus-4"));
        assert_eq!(pins.get(Role::Smol), Some("anthropic/claude-3-haiku"));
        assert_eq!(pins.get(Role::Plan), None);
    }

    #[test]
    fn parses_empty_pins() {
        let raw = br#"{"key":"modelRoles","value":{},"type":"record"}"#;
        assert!(parse_pins(raw).unwrap().is_empty());
        // Absent value also yields empty, not an error.
        let raw2 = br#"{"key":"modelRoles","type":"record"}"#;
        assert!(parse_pins(raw2).unwrap().is_empty());
    }

    #[test]
    fn known_pins_lists_all_roles_in_order() {
        let pins = RolePins::from_pairs([("default".to_string(), "x/y".to_string())]);
        let kp = known_pins(&pins);
        assert_eq!(kp.len(), Role::ALL.len());
        assert_eq!(kp[0].0, Role::Default);
        assert_eq!(kp[0].1.as_deref(), Some("x/y"));
        assert_eq!(kp[1].1, None);
    }
}
