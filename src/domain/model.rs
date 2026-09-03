//! Core identifiers and value objects with enforced invariants.

use std::fmt;

/// A quota-remaining reading as a whole percentage in `0..=100`.
///
/// The invariant is enforced at construction, so every `Percent` in the domain
/// is a valid percentage — callers never need to re-clamp.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Percent(u8);

impl Percent {
    /// Construct from an integer, clamping to `0..=100`.
    pub fn new(value: u8) -> Self {
        Percent(value.min(100))
    }

    /// Construct from a floating-point reading (rounds, clamps, treats
    /// non-finite as `0`).
    pub fn from_f64(value: f64) -> Self {
        if !value.is_finite() {
            return Percent(0);
        }
        Percent(value.round().clamp(0.0, 100.0) as u8)
    }

    pub fn get(self) -> u8 {
        self.0
    }

    pub fn is_zero(self) -> bool {
        self.0 == 0
    }
}

impl fmt::Display for Percent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}%", self.0)
    }
}

/// A provider account namespace, e.g. `anthropic` or `openai-codex`.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ProviderId(String);

impl ProviderId {
    pub fn new(s: impl Into<String>) -> Self {
        ProviderId(s.into())
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ProviderId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// A single credential / workspace under a provider, kept as a redacted prefix.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct AccountId(String);

impl AccountId {
    pub fn new(s: impl Into<String>) -> Self {
        AccountId(s.into())
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for AccountId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// A model quota tier — the classification a usage window can single out,
/// e.g. Claude `fable` (the premium/Opus-class tier) or Codex `spark`.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Tier(String);

impl Tier {
    pub fn new(s: impl Into<String>) -> Self {
        Tier(s.into())
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Tier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// The observable unit of headroom.
///
/// - `Base`: models constrained only by the provider's shared windows
///   (e.g. Sonnet/Haiku under Claude's 5h + 7d shared caps).
/// - `Tier(t)`: models additionally constrained by a tier-specific window
///   (e.g. Fable under Claude's Opus weekly cap).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ModelClass {
    Base,
    Tier(Tier),
}

impl ModelClass {
    /// A short display label for the class.
    pub fn label(&self) -> String {
        match self {
            ModelClass::Base => "base".to_string(),
            ModelClass::Tier(t) => t.as_str().to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percent_clamps_and_rounds() {
        assert_eq!(Percent::new(150).get(), 100);
        assert_eq!(Percent::from_f64(72.4).get(), 72);
        assert_eq!(Percent::from_f64(72.6).get(), 73);
        assert_eq!(Percent::from_f64(-3.0).get(), 0);
        assert_eq!(Percent::from_f64(f64::NAN).get(), 0);
        assert!(Percent::new(0).is_zero());
    }

    #[test]
    fn percent_orders_by_value() {
        assert!(Percent::new(5) < Percent::new(72));
    }
}
