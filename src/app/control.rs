//! The Switch use case: resolve a query to a model and pin it to a role, or
//! clear a role — always through the `ModelControl` port, which writes through
//! and verifies by read-back. Resolution *failures* (no/ambiguous match) are
//! returned as data (`Outcome`), not errors; only IO faults are `Err`.

use anyhow::Result;

use crate::domain::{resolve_model, ModelRef, Resolve, Role, RolePins};

use super::ports::ModelControl;

/// What a pin/clear attempt did.
#[derive(Clone, Debug)]
pub enum Outcome {
    /// A role was pinned to a model; `pins` is the verified new record.
    Pinned {
        role: Role,
        model: ModelRef,
        pins: RolePins,
    },
    /// A role's pin was removed (or was already absent); `pins` is verified.
    Cleared { role: Role, pins: RolePins },
    /// The query matched nothing — nothing was written.
    NoMatch { query: String },
    /// The query matched several models — nothing was written.
    Ambiguous {
        query: String,
        candidates: Vec<ModelRef>,
    },
}

/// Orchestrates model switching over a `ModelControl` port.
pub struct Switcher<'a> {
    control: &'a dyn ModelControl,
}

impl<'a> Switcher<'a> {
    pub fn new(control: &'a dyn ModelControl) -> Self {
        Switcher { control }
    }

    /// The full model catalog for building the policy-group view.
    pub fn models(&self) -> Result<Vec<ModelRef>> {
        self.control.available_models()
    }

    /// The current role→selector assignments.
    pub fn pins(&self) -> Result<RolePins> {
        self.control.pins()
    }

    /// Resolve `query` against the catalog and, on a unique hit, pin it to
    /// `role`. Writes through and returns the verified pins.
    pub fn pin(&self, role: Role, query: &str) -> Result<Outcome> {
        let models = self.control.available_models()?;
        match resolve_model(&models, query) {
            Resolve::None => Ok(Outcome::NoMatch {
                query: query.to_string(),
            }),
            Resolve::Ambiguous(candidates) => Ok(Outcome::Ambiguous {
                query: query.to_string(),
                candidates,
            }),
            Resolve::Unique(model) => {
                let mut pins = self.control.pins()?;
                pins.set(role, &model.selector);
                let verified = self.control.write_pins(&pins)?;
                Ok(Outcome::Pinned {
                    role,
                    model,
                    pins: verified,
                })
            }
        }
    }

    /// Pin a role directly to a known selector (no fuzzy resolution). Used by
    /// the interactive selector where the model is already chosen.
    pub fn pin_selector(&self, role: Role, selector: &str) -> Result<RolePins> {
        let mut pins = self.control.pins()?;
        pins.set(role, selector);
        self.control.write_pins(&pins)
    }

    /// Remove `role`'s pin (idempotent). Writes only when something changes.
    pub fn clear(&self, role: Role) -> Result<Outcome> {
        let mut pins = self.control.pins()?;
        let pins = if pins.clear(role) {
            self.control.write_pins(&pins)?
        } else {
            pins
        };
        Ok(Outcome::Cleared { role, pins })
    }
}
