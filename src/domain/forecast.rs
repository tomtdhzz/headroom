//! Burn-rate estimation and exhaustion forecasting from local sample history.

use std::time::{Duration, SystemTime};

use super::model::Percent;

/// One historical reading of a window's remaining quota.
#[derive(Clone, Copy, Debug)]
pub struct Sample {
    pub at: SystemTime,
    pub remaining: Percent,
}

/// How fast quota is being consumed, in percentage points per hour.
#[derive(Clone, Copy, Debug)]
pub struct BurnRate {
    pub percent_per_hour: f64,
}

/// A projection of when a window will hit zero.
#[derive(Clone, Copy, Debug, Default)]
pub struct ExhaustionForecast {
    pub burn: Option<BurnRate>,
    pub eta: Option<Duration>,
}

impl ExhaustionForecast {
    /// Estimate burn rate and ETA from the oldest→newest points in a series.
    ///
    /// Deliberately conservative — returns no ETA when there is nothing sound to
    /// project from:
    /// - fewer than two samples,
    /// - non-positive elapsed time,
    /// - non-decreasing remaining (flat or replenished), or
    /// - an ETA that is not a finite, representable duration.
    pub fn from_series(series: &[Sample]) -> ExhaustionForecast {
        let (Some(first), Some(last)) = (series.first(), series.last()) else {
            return ExhaustionForecast::default();
        };
        if series.len() < 2 {
            return ExhaustionForecast::default();
        }
        let Ok(elapsed) = last.at.duration_since(first.at) else {
            return ExhaustionForecast::default();
        };
        let hours = elapsed.as_secs_f64() / 3_600.0;
        if hours <= 0.0 {
            return ExhaustionForecast::default();
        }
        let drop = first.remaining.get() as f64 - last.remaining.get() as f64;
        if drop <= 0.0 {
            // Not burning down — report a zero rate but no exhaustion.
            return ExhaustionForecast {
                burn: Some(BurnRate {
                    percent_per_hour: 0.0,
                }),
                eta: None,
            };
        }
        let rate = drop / hours;
        let eta_seconds = (last.remaining.get() as f64 / rate) * 3_600.0;
        let eta = if eta_seconds.is_finite() && (0.0..1e15).contains(&eta_seconds) {
            Some(Duration::from_secs_f64(eta_seconds))
        } else {
            None
        };
        ExhaustionForecast {
            burn: Some(BurnRate {
                percent_per_hour: rate,
            }),
            eta,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(secs: u64) -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_secs(secs)
    }

    #[test]
    fn single_sample_yields_no_forecast() {
        let s = [Sample {
            at: at(0),
            remaining: Percent::new(50),
        }];
        let f = ExhaustionForecast::from_series(&s);
        assert!(f.eta.is_none() && f.burn.is_none());
    }

    #[test]
    fn decreasing_series_projects_positive_rate_and_finite_eta() {
        // 100% -> 90% over 1 hour => 10 %/h; 90% left => 9h to zero.
        let s = [
            Sample {
                at: at(0),
                remaining: Percent::new(100),
            },
            Sample {
                at: at(3_600),
                remaining: Percent::new(90),
            },
        ];
        let f = ExhaustionForecast::from_series(&s);
        let rate = f.burn.unwrap().percent_per_hour;
        assert!((rate - 10.0).abs() < 1e-6);
        let eta_hours = f.eta.unwrap().as_secs_f64() / 3_600.0;
        assert!((eta_hours - 9.0).abs() < 1e-3);
    }

    #[test]
    fn non_decreasing_series_reports_no_eta() {
        let s = [
            Sample {
                at: at(0),
                remaining: Percent::new(50),
            },
            Sample {
                at: at(3_600),
                remaining: Percent::new(60),
            },
        ];
        let f = ExhaustionForecast::from_series(&s);
        assert_eq!(f.burn.unwrap().percent_per_hour, 0.0);
        assert!(f.eta.is_none());
    }
}
