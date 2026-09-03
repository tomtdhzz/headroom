//! The PollAndEvaluate use case: read snapshots, record history, and compute
//! per-class headroom, forecast, and alerts.

use anyhow::Result;

use crate::domain::{evaluate, Alert, ExhaustionForecast, Headroom, QuotaSnapshot, Thresholds};

use super::ports::{HistoryStore, UsageSource};

/// The assessment of one model class: its headroom and exhaustion forecast.
#[derive(Clone, Debug)]
pub struct ClassAssessment {
    pub headroom: Headroom,
    pub forecast: ExhaustionForecast,
}

/// Everything computed for one account in a single poll.
#[derive(Clone, Debug)]
pub struct AccountAssessment {
    pub snapshot: QuotaSnapshot,
    pub classes: Vec<ClassAssessment>,
    pub alerts: Vec<Alert>,
}

/// The result of one poll across all accounts.
#[derive(Clone, Debug)]
pub struct Assessment {
    pub accounts: Vec<AccountAssessment>,
}

impl Assessment {
    /// Every alert across all accounts, flattened.
    pub fn alerts(&self) -> Vec<Alert> {
        self.accounts
            .iter()
            .flat_map(|a| a.alerts.iter().cloned())
            .collect()
    }
}

/// Orchestrates one polling cycle over a usage source and history store.
pub struct Evaluator<'a> {
    source: &'a dyn UsageSource,
    history: &'a dyn HistoryStore,
    thresholds: Thresholds,
}

impl<'a> Evaluator<'a> {
    pub fn new(
        source: &'a dyn UsageSource,
        history: &'a dyn HistoryStore,
        thresholds: Thresholds,
    ) -> Self {
        Evaluator {
            source,
            history,
            thresholds,
        }
    }

    /// Read snapshots, record them, then assess every model class per account.
    pub fn poll(&self) -> Result<Assessment> {
        let snapshots = self.source.snapshots()?;
        let mut accounts = Vec::with_capacity(snapshots.len());

        for snapshot in snapshots {
            // Record first so the freshest point is included in the forecast.
            self.history.record(&snapshot)?;

            let mut classes = Vec::new();
            let mut assessed = Vec::new();

            for class in snapshot.model_classes() {
                let Some(headroom) = Headroom::for_class(&snapshot, &class) else {
                    continue;
                };
                let series = self
                    .history
                    .series(
                        &snapshot.provider,
                        &snapshot.account,
                        &headroom.bottleneck.id,
                    )
                    .unwrap_or_default();
                let forecast = ExhaustionForecast::from_series(&series);
                assessed.push((headroom.clone(), forecast));
                classes.push(ClassAssessment { headroom, forecast });
            }

            let alerts = evaluate(
                &snapshot.provider,
                &snapshot.account,
                &assessed,
                &self.thresholds,
            );

            accounts.push(AccountAssessment {
                snapshot,
                classes,
                alerts,
            });
        }

        Ok(Assessment { accounts })
    }
}
