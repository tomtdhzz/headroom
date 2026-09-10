//! Domain layer: the ubiquitous language of AI-subscription quota headroom.
//!
//! Pure, IO-free, and independently testable. Nothing here knows about `omp`,
//! JSON, files, or terminals — those live in `adapters`/`delivery`.

pub mod alert;
pub mod control;
pub mod forecast;
pub mod headroom;
pub mod model;
pub mod window;

pub use alert::{evaluate, Alert, AlertKind, AlertLevel, Suggestion, Thresholds};
pub use control::{resolve_model, ModelRef, Resolve, Role, RolePins};
pub use forecast::{BurnRate, ExhaustionForecast, Sample};
pub use headroom::Headroom;
pub use model::{AccountId, ModelClass, Percent, ProviderId, Tier};
pub use window::{LimitScope, LimitWindow, QuotaSnapshot, WindowStatus};

use std::time::Duration;

/// Compact, human-readable duration such as `1d3h`, `47m`, or `12s`.
pub fn human_duration(d: Duration) -> String {
    let secs = d.as_secs();
    if secs >= 86_400 {
        let (days, hours) = (secs / 86_400, (secs % 86_400) / 3_600);
        if hours > 0 {
            format!("{days}d{hours}h")
        } else {
            format!("{days}d")
        }
    } else if secs >= 3_600 {
        let (hours, mins) = (secs / 3_600, (secs % 3_600) / 60);
        if mins > 0 {
            format!("{hours}h{mins}m")
        } else {
            format!("{hours}h")
        }
    } else if secs >= 60 {
        format!("{}m", secs / 60)
    } else {
        format!("{secs}s")
    }
}
