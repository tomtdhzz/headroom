//! headroom — per-model, rule-aware quota headroom for AI coding subscriptions.
//!
//! Layered as a lightweight hexagon:
//! - [`domain`]: pure ubiquitous language (windows, scopes, headroom, forecast, alerts).
//! - [`app`]: use cases and the ports they depend on.
//! - [`adapters`]: infrastructure implementing the ports (omp, files, clock, notify).
//! - [`delivery`]: user-facing rendering.

pub mod adapters;
pub mod app;
pub mod delivery;
pub mod domain;
