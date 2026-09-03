//! System wall-clock adapter.

use std::time::SystemTime;

use crate::app::ports::Clock;

pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> SystemTime {
        SystemTime::now()
    }
}
