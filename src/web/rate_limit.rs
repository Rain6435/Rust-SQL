//! Login rate limiting: at most 5 failed attempts per username and per source IP in a
//! sliding 15-minute window. Further attempts get 429 with a `Retry-After` hint, without
//! checking the password. State is in memory, so it resets when the server restarts.

use std::collections::{HashMap, VecDeque};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Failed attempts allowed per key within [`WINDOW`].
pub const MAX_FAILURES: usize = 5;
/// Sliding window length.
pub const WINDOW: Duration = Duration::from_secs(15 * 60);

/// Tracks recent failed logins per `u:<username>` and `ip:<address>`.
#[derive(Default)]
pub struct LoginLimiter {
    failures: Mutex<HashMap<String, VecDeque<Instant>>>,
}

fn keys(username: &str, ip: Option<&str>) -> Vec<String> {
    let mut k = vec![format!("u:{}", username.to_ascii_lowercase())];
    if let Some(ip) = ip {
        k.push(format!("ip:{ip}"));
    }
    k
}

impl LoginLimiter {
    fn prune(map: &mut HashMap<String, VecDeque<Instant>>, now: Instant) {
        map.retain(|_, q| {
            while q.front().is_some_and(|t| now.duration_since(*t) >= WINDOW) {
                q.pop_front();
            }
            !q.is_empty()
        });
    }

    /// Returns `Err(retry_after)` if the username or IP is over the limit.
    pub fn check(&self, username: &str, ip: Option<&str>) -> Result<(), Duration> {
        self.check_at(username, ip, Instant::now())
    }

    fn check_at(&self, username: &str, ip: Option<&str>, now: Instant) -> Result<(), Duration> {
        let mut map = self.failures.lock().unwrap_or_else(|e| e.into_inner());
        Self::prune(&mut map, now);
        let retry = keys(username, ip)
            .iter()
            .filter_map(|k| map.get(k))
            .filter(|q| q.len() >= MAX_FAILURES)
            .filter_map(|q| q.front())
            .map(|oldest| WINDOW.saturating_sub(now.duration_since(*oldest)))
            .max();
        match retry {
            Some(d) => Err(d.max(Duration::from_secs(1))),
            None => Ok(()),
        }
    }

    /// Records a failed attempt for the username and the IP.
    pub fn record_failure(&self, username: &str, ip: Option<&str>) {
        self.record_failure_at(username, ip, Instant::now());
    }

    fn record_failure_at(&self, username: &str, ip: Option<&str>, now: Instant) {
        let mut map = self.failures.lock().unwrap_or_else(|e| e.into_inner());
        for k in keys(username, ip) {
            map.entry(k).or_default().push_back(now);
        }
    }

    /// Clears the username's failures after a successful login. IP failures are kept.
    pub fn record_success(&self, username: &str) {
        let mut map = self.failures.lock().unwrap_or_else(|e| e.into_inner());
        map.remove(&format!("u:{}", username.to_ascii_lowercase()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn five_failures_per_username_trigger_the_limit() {
        let l = LoginLimiter::default();
        let t0 = Instant::now();
        for i in 0..MAX_FAILURES {
            assert!(l
                .check_at("Alice", Some(&format!("10.0.0.{i}")), t0)
                .is_ok());
            l.record_failure_at("alice", Some(&format!("10.0.0.{i}")), t0);
        }
        let retry = l.check_at("ALICE", Some("10.9.9.9"), t0).unwrap_err();
        assert!(retry <= WINDOW && retry >= Duration::from_secs(14 * 60));
        assert!(l.check_at("bob", Some("10.9.9.9"), t0).is_ok());
        assert!(l.check_at("alice", None, t0 + WINDOW).is_ok());
    }

    #[test]
    fn five_failures_per_ip_trigger_the_limit_across_usernames() {
        let l = LoginLimiter::default();
        let t0 = Instant::now();
        for i in 0..MAX_FAILURES {
            l.record_failure_at(&format!("user{i}"), Some("203.0.113.7"), t0);
        }
        assert!(l.check_at("someone-else", Some("203.0.113.7"), t0).is_err());
        assert!(l.check_at("someone-else", Some("203.0.113.8"), t0).is_ok());
    }

    #[test]
    fn success_clears_the_username_but_not_the_ip() {
        let l = LoginLimiter::default();
        for _ in 0..MAX_FAILURES {
            l.record_failure("alice", Some("198.51.100.1"));
        }
        l.record_success("alice");
        assert!(l.check("alice", Some("198.51.100.2")).is_ok());
        assert!(l.check("alice", Some("198.51.100.1")).is_err());
    }
}
