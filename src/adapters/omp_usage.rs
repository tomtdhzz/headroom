//! `UsageSource` backed by `omp usage --json --redact`.
//!
//! This is the anti-corruption layer: the omp JSON DTOs live here only and are
//! mapped into domain types. `scope.shared` / `scope.tier` drives the mapping,
//! so no per-provider or per-tier special cases are needed.

use std::process::Command;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{anyhow, bail, Context, Result};
use serde::Deserialize;

use crate::app::ports::UsageSource;
use crate::domain::{
    AccountId, LimitScope, LimitWindow, Percent, ProviderId, QuotaSnapshot, Tier, WindowStatus,
};

/// Runs the `omp` CLI to read usage.
pub struct OmpUsageSource {
    provider: Option<String>,
    program: String,
}

impl OmpUsageSource {
    pub fn new(provider: Option<String>) -> Self {
        OmpUsageSource {
            provider,
            program: "omp".to_string(),
        }
    }

    fn run(&self) -> Result<Vec<u8>> {
        let mut cmd = Command::new(&self.program);
        cmd.arg("usage");
        if let Some(p) = &self.provider {
            cmd.arg("--provider").arg(p);
        }
        cmd.arg("--json").arg("--redact");

        let output = cmd.output().map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                anyhow!(
                    "`{}` not found on PATH; install and authenticate omp first",
                    self.program
                )
            } else {
                anyhow!("failed to run `{}`: {e}", self.program)
            }
        })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            if looks_like_auth(&stderr) {
                bail!("omp usage failed: not authenticated (run `omp` and `/login`)");
            }
            let msg = stderr.trim();
            if msg.is_empty() {
                bail!("omp usage failed with status {}", output.status);
            }
            bail!("omp usage failed: {msg}");
        }
        Ok(output.stdout)
    }
}

impl UsageSource for OmpUsageSource {
    fn snapshots(&self) -> Result<Vec<QuotaSnapshot>> {
        parse_usage(&self.run()?)
    }
}

fn looks_like_auth(text: &str) -> bool {
    let t = text.to_ascii_lowercase();
    ["auth", "login", "unauthorized", "credential", "not signed"]
        .iter()
        .any(|m| t.contains(m))
}

// ---- omp usage JSON DTOs (kept private to this adapter) --------------------

#[derive(Deserialize)]
struct UsageResponse {
    #[serde(default)]
    reports: Vec<Report>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Report {
    provider: String,
    fetched_at: u64,
    #[serde(default)]
    limits: Vec<Limit>,
    #[serde(default)]
    metadata: Option<Metadata>,
}

#[derive(Deserialize)]
struct Limit {
    id: String,
    #[serde(default)]
    label: Option<String>,
    scope: Scope,
    window: WindowDto,
    amount: Amount,
    status: String,
}

#[derive(Deserialize)]
struct Scope {
    #[serde(default)]
    shared: Option<bool>,
    #[serde(default)]
    tier: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct WindowDto {
    #[serde(default)]
    duration_ms: Option<u64>,
    #[serde(default)]
    resets_at: Option<u64>,
}

#[derive(Deserialize)]
struct Amount {
    remaining: f64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Metadata {
    #[serde(default)]
    plan_type: Option<String>,
    #[serde(default)]
    account_id: Option<String>,
}

/// Parse omp usage JSON into domain snapshots. Public for contract testing.
pub fn parse_usage(raw: &[u8]) -> Result<Vec<QuotaSnapshot>> {
    let response: UsageResponse =
        serde_json::from_slice(raw).context("parsing `omp usage --json` output")?;

    let snapshots = response
        .reports
        .into_iter()
        .map(|report| {
            let account = report
                .metadata
                .as_ref()
                .and_then(|m| m.account_id.clone())
                .unwrap_or_else(|| "?".to_string());
            let plan = report.metadata.and_then(|m| m.plan_type);
            let windows = report.limits.into_iter().map(map_window).collect();
            QuotaSnapshot {
                provider: ProviderId::new(report.provider),
                account: AccountId::new(account),
                plan,
                fetched_at: from_millis(report.fetched_at),
                windows,
            }
        })
        .collect();

    Ok(snapshots)
}

fn map_window(limit: Limit) -> LimitWindow {
    let scope = match (limit.scope.shared, limit.scope.tier) {
        (Some(true), _) => LimitScope::Shared,
        (_, Some(tier)) => LimitScope::Tier(Tier::new(tier)),
        // Neither shared nor tier declared: treat conservatively as shared.
        _ => LimitScope::Shared,
    };
    let id = limit.id;
    let label = limit.label.unwrap_or_else(|| id.clone());
    LimitWindow {
        id,
        label,
        scope,
        period: limit.window.duration_ms.map(Duration::from_millis),
        remaining: Percent::from_f64(limit.amount.remaining),
        resets_at: limit.window.resets_at.map(from_millis),
        status: if limit.status.eq_ignore_ascii_case("ok") {
            WindowStatus::Ok
        } else {
            WindowStatus::Unavailable
        },
    }
}

fn from_millis(ms: u64) -> SystemTime {
    UNIX_EPOCH + Duration::from_millis(ms)
}
