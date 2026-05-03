//! 120fps frame-rate clamp for the FrameScheduler.
//!
//! Ported from `codex-rs/tui/src/tui/frame_rate_limiter.rs`
//! (Apache-2.0).

use std::time::{Duration, Instant};

pub const MIN_FRAME_INTERVAL: Duration = Duration::from_micros(8_333);

#[derive(Debug, Clone)]
pub struct FrameRateLimiter {
    last_emitted: Option<Instant>,
}

impl Default for FrameRateLimiter {
    fn default() -> Self {
        Self::new()
    }
}

impl FrameRateLimiter {
    pub fn new() -> Self {
        Self { last_emitted: None }
    }

    /// Adjust `requested` so it can't fire sooner than
    /// `last_emitted + MIN_FRAME_INTERVAL`.
    pub fn clamp_deadline(&self, requested: Instant) -> Instant {
        match self.last_emitted {
            None => requested,
            Some(last) => requested.max(last + MIN_FRAME_INTERVAL),
        }
    }

    pub fn mark_emitted(&mut self, t: Instant) {
        self.last_emitted = Some(t);
    }
}
