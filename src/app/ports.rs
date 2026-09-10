//! Ports: the interfaces the application depends on, implemented by adapters.
//! Following the Rust convention, the consumer defines the traits.

use std::time::SystemTime;

use anyhow::Result;

use crate::domain::{AccountId, Alert, ModelRef, ProviderId, QuotaSnapshot, RolePins, Sample};

/// Reads current quota snapshots for every authenticated account.
pub trait UsageSource {
    fn snapshots(&self) -> Result<Vec<QuotaSnapshot>>;
}

/// Persists sampled readings and serves per-window time series for forecasting.
pub trait HistoryStore {
    /// Record the snapshot's windows as samples (with the store's own
    /// dedup/retention policy).
    fn record(&self, snapshot: &QuotaSnapshot) -> Result<()>;

    /// The recorded samples for one window, oldest first.
    fn series(
        &self,
        provider: &ProviderId,
        account: &AccountId,
        window_id: &str,
    ) -> Result<Vec<Sample>>;
}

/// Delivers alerts to the user (terminal, desktop, chat, ...).
pub trait Notifier {
    fn notify(&self, alerts: &[Alert]) -> Result<()>;
}

/// The current wall-clock time (injected for testability).
pub trait Clock {
    fn now(&self) -> SystemTime;
}

/// The switch/control plane: reads the model catalog and current role pins, and
/// applies pin changes. Implementations are expected to write through to the
/// underlying tool (e.g. `omp config set modelRoles`) and verify by read-back.
pub trait ModelControl {
    /// Every model the tool can route to, for building the policy-group view.
    fn available_models(&self) -> Result<Vec<ModelRef>>;

    /// The current role→selector assignments.
    fn pins(&self) -> Result<RolePins>;

    /// Persist `pins` (full record) and return the verified read-back. Errors
    /// if the write did not take effect.
    fn write_pins(&self, pins: &RolePins) -> Result<RolePins>;
}
