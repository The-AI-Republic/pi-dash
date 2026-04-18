use rand::Rng;
use std::time::Duration;

/// Jittered exponential backoff used for cloud WS reconnects.
#[derive(Debug, Clone)]
pub struct Backoff {
    attempts: u32,
    cap_secs: u64,
}

impl Backoff {
    pub fn new() -> Self {
        Self {
            attempts: 0,
            cap_secs: 60,
        }
    }

    pub fn with_cap(cap_secs: u64) -> Self {
        Self {
            attempts: 0,
            cap_secs,
        }
    }

    /// Returns the next sleep duration and increments the attempt counter.
    pub fn next_delay(&mut self) -> Duration {
        self.attempts = self.attempts.saturating_add(1);
        let base = 1u64
            .checked_shl(self.attempts.min(6))
            .unwrap_or(self.cap_secs);
        let base = base.min(self.cap_secs);
        let jitter = rand::thread_rng().gen_range(0..=base);
        Duration::from_secs((base + jitter).min(self.cap_secs))
    }

    pub fn reset(&mut self) {
        self.attempts = 0;
    }

    pub fn attempts(&self) -> u32 {
        self.attempts
    }
}

impl Default for Backoff {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn caps_at_configured_maximum() {
        let mut b = Backoff::with_cap(10);
        for _ in 0..20 {
            let d = b.next_delay();
            assert!(d.as_secs() <= 10, "delay {:?} exceeded cap", d);
        }
    }

    #[test]
    fn reset_restarts_attempts() {
        let mut b = Backoff::with_cap(30);
        b.next_delay();
        b.next_delay();
        assert_eq!(b.attempts(), 2);
        b.reset();
        assert_eq!(b.attempts(), 0);
    }
}
