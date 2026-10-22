//! Default anti-spam protection that works with nobody watching: rate limits, a text budget,
//! a repeat filter and automatic timeouts for repeat offenders. In-memory per server; the owner
//! will be able to tune these values in the moderation phase.

use std::collections::{HashMap, VecDeque};
use std::time::{Duration, Instant};

pub struct Limits {
    pub burst_count: usize,
    pub burst_window: Duration,
    pub chars_per_window: usize,
    pub chars_window: Duration,
    pub max_repeats: usize,
    pub repeat_window: Duration,
    pub strikes_for_timeout: usize,
    pub strike_window: Duration,
    pub timeout: Duration,
}

impl Default for Limits {
    fn default() -> Self {
        Limits {
            burst_count: 5,
            burst_window: Duration::from_secs(5),
            chars_per_window: 12_000,
            chars_window: Duration::from_secs(60),
            max_repeats: 2,
            repeat_window: Duration::from_secs(60),
            strikes_for_timeout: 5,
            strike_window: Duration::from_secs(600),
            timeout: Duration::from_secs(600),
        }
    }
}

#[derive(Debug, PartialEq)]
pub enum Verdict {
    Allow,
    /// Too fast or too much text: try again after this long.
    Slow {
        retry_after: Duration,
        reason: &'static str,
    },
    /// Automatically muted until this long from now.
    TimedOut {
        remaining: Duration,
    },
}

#[derive(Default)]
struct UserState {
    sent: VecDeque<(Instant, usize, blake3::Hash)>,
    strikes: VecDeque<Instant>,
    timed_out_until: Option<Instant>,
}

#[derive(Default)]
pub struct SpamGuard {
    limits: Limits,
    users: HashMap<i64, UserState>,
    /// Idle users are dropped when the map reaches this size, so memory doesn't grow forever.
    prune_at: usize,
}

impl Limits {
    pub fn from_settings(row: &crate::models::ServerRow) -> Limits {
        Limits {
            burst_count: row.spam_messages_per_5s.max(1) as usize,
            chars_per_window: row.spam_chars_per_minute.max(1) as usize,
            max_repeats: row.spam_max_repeats.max(1) as usize,
            timeout: Duration::from_secs(row.spam_timeout_minutes.max(0) as u64 * 60),
            ..Limits::default()
        }
    }
}

impl SpamGuard {
    pub fn set_limits(&mut self, limits: Limits) {
        self.limits = limits;
    }

    /// Clears a user's strikes and automatic timeout (when a moderator lifts it).
    pub fn forgive(&mut self, user_id: i64) {
        self.users.remove(&user_id);
    }

    fn forget_idle(&mut self, now: Instant) {
        let l = &self.limits;
        let keep = l.burst_window.max(l.chars_window).max(l.repeat_window);
        self.users.retain(|_, st| {
            st.timed_out_until.is_some_and(|t| t > now)
                || st.sent.back().is_some_and(|(t, _, _)| now.duration_since(*t) <= keep)
                || st.strikes.back().is_some_and(|t| now.duration_since(*t) <= l.strike_window)
        });
        self.prune_at = self.users.len() * 2;
    }

    /// Checks a message about to be sent; records it when allowed and a strike when not.
    pub fn check(&mut self, user_id: i64, content: &str) -> Verdict {
        self.check_at(user_id, content, Instant::now())
    }

    fn check_at(&mut self, user_id: i64, content: &str, now: Instant) -> Verdict {
        if self.users.len() >= self.prune_at.max(1000) {
            self.forget_idle(now);
        }
        let l = &self.limits;
        let st = self.users.entry(user_id).or_default();

        if let Some(until) = st.timed_out_until {
            if until > now {
                return Verdict::TimedOut { remaining: until - now };
            }
            st.timed_out_until = None;
        }

        let keep = l.burst_window.max(l.chars_window).max(l.repeat_window);
        while st.sent.front().is_some_and(|(t, _, _)| now.duration_since(*t) > keep) {
            st.sent.pop_front();
        }
        while st.strikes.front().is_some_and(|t| now.duration_since(*t) > l.strike_window) {
            st.strikes.pop_front();
        }

        let len = content.chars().count();
        let hash = blake3::hash(content.trim().to_lowercase().as_bytes());

        let in_burst: Vec<Instant> = st.sent.iter().filter(|(t, _, _)| now.duration_since(*t) < l.burst_window).map(|(t, _, _)| *t).collect();
        let chars: usize = st.sent.iter().filter(|(t, _, _)| now.duration_since(*t) < l.chars_window).map(|(_, n, _)| n).sum();
        let repeats = st.sent.iter().filter(|(t, _, h)| now.duration_since(*t) < l.repeat_window && *h == hash).count();

        let slow = if in_burst.len() >= l.burst_count {
            Some((l.burst_window.saturating_sub(now.duration_since(in_burst[0])), "Slow down: too many messages."))
        } else if chars + len > l.chars_per_window {
            Some((Duration::from_secs(10), "Slow down: too much text in a short time."))
        } else if repeats >= l.max_repeats {
            Some((Duration::from_secs(10), "Please don't repeat the same message."))
        } else {
            None
        };

        match slow {
            None => {
                st.sent.push_back((now, len, hash));
                Verdict::Allow
            }
            Some((retry_after, reason)) => {
                st.strikes.push_back(now);
                // A timeout of 0 minutes turns automatic timeouts off.
                if !l.timeout.is_zero() && st.strikes.len() >= l.strikes_for_timeout {
                    st.strikes.clear();
                    st.timed_out_until = Some(now + l.timeout);
                    Verdict::TimedOut { remaining: l.timeout }
                } else {
                    Verdict::Slow { retry_after: retry_after.max(Duration::from_millis(500)), reason }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn burst_then_timeout() {
        let mut g = SpamGuard::default();
        let t = Instant::now();
        for i in 0..5 {
            assert_eq!(g.check_at(1, &format!("msg {i}"), t), Verdict::Allow);
        }
        for i in 0..4 {
            assert!(matches!(g.check_at(1, &format!("x {i}"), t), Verdict::Slow { .. }));
        }
        assert!(matches!(g.check_at(1, "again", t), Verdict::TimedOut { .. }));
        // Other users are unaffected.
        assert_eq!(g.check_at(2, "hi", t), Verdict::Allow);
        // The timeout ends.
        assert_eq!(g.check_at(1, "back", t + Duration::from_secs(601)), Verdict::Allow);
    }

    #[test]
    fn repeats_and_text_budget() {
        let mut g = SpamGuard::default();
        let t = Instant::now();
        assert_eq!(g.check_at(1, "spam", t), Verdict::Allow);
        assert_eq!(g.check_at(1, "SPAM ", t + Duration::from_secs(6)), Verdict::Allow);
        assert!(matches!(g.check_at(1, "spam", t + Duration::from_secs(12)), Verdict::Slow { .. }));

        let big = "a".repeat(3999);
        let t2 = t + Duration::from_secs(120);
        for i in 0..3 {
            assert_eq!(g.check_at(3, &format!("{big}{i}"), t2 + Duration::from_secs(6 * i)), Verdict::Allow);
        }
        assert!(matches!(g.check_at(3, "one more", t2 + Duration::from_secs(20)), Verdict::Slow { .. }));
    }
}
