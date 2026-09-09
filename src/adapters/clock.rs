//! System wall-clock adapter.

use std::time::SystemTime;

use crate::app::ports::Clock;

pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> SystemTime {
        SystemTime::now()
    }
}

/// The local UTC offset in seconds, read once from the OS via `date +%z`
/// (e.g. `+0800` → 28800). Falls back to `0` (UTC) if unavailable/unparsable.
/// Delivery uses it to render window-reset time points in local wall-clock.
pub fn local_utc_offset_seconds() -> i32 {
    use std::process::Command;
    Command::new("date")
        .arg("+%z")
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| parse_offset(String::from_utf8_lossy(&o.stdout).trim()))
        .unwrap_or(0)
}

/// Parse a `±HHMM` numeric timezone offset into seconds.
fn parse_offset(s: &str) -> Option<i32> {
    let bytes = s.as_bytes();
    if bytes.len() < 5 {
        return None;
    }
    let sign = match bytes[0] {
        b'+' => 1,
        b'-' => -1,
        _ => return None,
    };
    let hh: i32 = s.get(1..3)?.parse().ok()?;
    let mm: i32 = s.get(3..5)?.parse().ok()?;
    Some(sign * (hh * 3_600 + mm * 60))
}

#[cfg(test)]
mod tests {
    use super::parse_offset;

    #[test]
    fn parses_signed_offsets() {
        assert_eq!(parse_offset("+0800"), Some(28_800));
        assert_eq!(parse_offset("-0530"), Some(-19_800));
        assert_eq!(parse_offset("+0000"), Some(0));
        assert_eq!(parse_offset(""), None);
        assert_eq!(parse_offset("0800"), None);
    }
}
