//! Snowflake IDs: milliseconds since 2026-01-01 UTC in the high bits, a sequence in the low 22.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

const EPOCH_MS: u64 = 1_767_225_600_000; // 2026-01-01T00:00:00Z
static LAST: AtomicU64 = AtomicU64::new(0);

/// A new unique, time-sortable ID (shared by every server on this host).
pub fn next_id() -> i64 {
    let now = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0);
    let candidate = now.saturating_sub(EPOCH_MS) << 22;
    let mut last = LAST.load(Ordering::Relaxed);
    loop {
        let next = if candidate > last { candidate } else { last + 1 };
        match LAST.compare_exchange_weak(last, next, Ordering::SeqCst, Ordering::Relaxed) {
            Ok(_) => return next as i64,
            Err(seen) => last = seen,
        }
    }
}

/// Parses an ID sent by a client as a decimal string.
pub fn parse(s: &str) -> Option<i64> {
    s.parse::<i64>().ok().filter(|v| *v > 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_increase() {
        let a = next_id();
        let b = next_id();
        assert!(b > a);
        assert_eq!(parse(&a.to_string()), Some(a));
        assert_eq!(parse("abc"), None);
        assert_eq!(parse("-5"), None);
    }
}
