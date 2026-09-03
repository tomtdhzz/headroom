//! `HistoryStore` persisting quota samples as JSON under the XDG cache dir.
//!
//! Privacy: only `provider|account|window_id` keys plus `(timestamp, remaining%)`
//! points are stored — never credentials, emails, or org identity.

use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::app::ports::HistoryStore;
use crate::domain::{AccountId, Percent, ProviderId, QuotaSnapshot, Sample};

const MAX_AGE: Duration = Duration::from_secs(30 * 24 * 3_600);
const MAX_SAMPLES: usize = 2_048;
const MIN_INTERVAL: Duration = Duration::from_secs(15 * 60);

/// A file-backed sample history.
pub struct FileHistoryStore {
    path: PathBuf,
}

impl FileHistoryStore {
    /// Use the default XDG cache location.
    pub fn new() -> Result<Self> {
        Ok(FileHistoryStore {
            path: default_path()?,
        })
    }

    /// Use an explicit path (for tests).
    pub fn at(path: PathBuf) -> Self {
        FileHistoryStore { path }
    }

    fn load(&self) -> Store {
        fs::read(&self.path)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default()
    }

    fn save(&self, store: &Store) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent).ok();
        }
        let bytes = serde_json::to_vec(store).context("serializing history")?;
        let tmp = self
            .path
            .with_extension(format!("tmp-{}", std::process::id()));
        fs::write(&tmp, bytes).context("writing history")?;
        fs::rename(&tmp, &self.path).context("committing history")?;
        Ok(())
    }
}

#[derive(Default, Serialize, Deserialize)]
struct Store {
    #[serde(default)]
    series: BTreeMap<String, Vec<Point>>,
}

#[derive(Clone, Copy, Serialize, Deserialize)]
struct Point {
    at: u64,
    remaining: u8,
}

fn key(provider: &ProviderId, account: &AccountId, window_id: &str) -> String {
    format!("{}|{}|{}", provider.as_str(), account.as_str(), window_id)
}

impl HistoryStore for FileHistoryStore {
    fn record(&self, snapshot: &QuotaSnapshot) -> Result<()> {
        let mut store = self.load();
        let now_ms = to_millis(snapshot.fetched_at);

        for window in &snapshot.windows {
            let entry = store
                .series
                .entry(key(&snapshot.provider, &snapshot.account, &window.id))
                .or_default();

            let should_push = match entry.last() {
                None => true,
                Some(last) => {
                    let changed = last.remaining != window.remaining.get();
                    let elapsed = now_ms.saturating_sub(last.at);
                    changed || elapsed >= MIN_INTERVAL.as_millis() as u64
                }
            };
            if should_push {
                entry.push(Point {
                    at: now_ms,
                    remaining: window.remaining.get(),
                });
            }

            let cutoff = now_ms.saturating_sub(MAX_AGE.as_millis() as u64);
            entry.retain(|p| p.at >= cutoff);
            if entry.len() > MAX_SAMPLES {
                let excess = entry.len() - MAX_SAMPLES;
                entry.drain(0..excess);
            }
        }

        self.save(&store)
    }

    fn series(
        &self,
        provider: &ProviderId,
        account: &AccountId,
        window_id: &str,
    ) -> Result<Vec<Sample>> {
        let store = self.load();
        let samples = store
            .series
            .get(&key(provider, account, window_id))
            .map(|points| {
                points
                    .iter()
                    .map(|p| Sample {
                        at: from_millis(p.at),
                        remaining: Percent::new(p.remaining),
                    })
                    .collect()
            })
            .unwrap_or_default();
        Ok(samples)
    }
}

fn default_path() -> Result<PathBuf> {
    if let Some(dir) = std::env::var_os("XDG_CACHE_HOME") {
        return Ok(PathBuf::from(dir).join("headroom/history.json"));
    }
    let home = std::env::var_os("HOME").context("HOME is not set")?;
    Ok(PathBuf::from(home).join(".cache/headroom/history.json"))
}

fn to_millis(t: SystemTime) -> u64 {
    t.duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn from_millis(ms: u64) -> SystemTime {
    UNIX_EPOCH + Duration::from_millis(ms)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{LimitScope, LimitWindow, WindowStatus};

    fn snap_at(ms: u64, remaining: u8) -> QuotaSnapshot {
        QuotaSnapshot {
            provider: ProviderId::new("anthropic"),
            account: AccountId::new("f6*"),
            plan: None,
            fetched_at: from_millis(ms),
            windows: vec![LimitWindow {
                id: "5h".into(),
                label: "5h".into(),
                scope: LimitScope::Shared,
                period: None,
                remaining: Percent::new(remaining),
                resets_at: None,
                status: WindowStatus::Ok,
            }],
        }
    }

    #[test]
    fn records_and_reads_back_changed_samples() {
        let dir = std::env::temp_dir().join(format!("headroom-test-{}", std::process::id()));
        let store = FileHistoryStore::at(dir.join("history.json"));

        store.record(&snap_at(0, 100)).unwrap();
        store.record(&snap_at(3_600_000, 90)).unwrap();

        let series = store
            .series(&ProviderId::new("anthropic"), &AccountId::new("f6*"), "5h")
            .unwrap();
        assert_eq!(series.len(), 2);
        assert_eq!(series[0].remaining, Percent::new(100));
        assert_eq!(series[1].remaining, Percent::new(90));

        let _ = fs::remove_dir_all(&dir);
    }
}
