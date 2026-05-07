//! `FrameRequester` (handle, `Clone`) + `FrameScheduler` (actor).
//!
//! Ported from `codex-rs/tui/src/tui/frame_requester.rs` (Apache-2.0).
//! Trimmed: the runner only has one Draw subscriber so we use a
//! `broadcast::channel(1)` purely for coalescing semantics, not
//! fan-out.
//!
//! Multiple `schedule_frame()` calls in a single window merge into
//! the *earliest* deadline. When that deadline fires, exactly one
//! Draw signal is sent.

use std::time::{Duration, Instant};

use tokio::sync::{broadcast, mpsc};
use tokio::time::sleep_until;

use super::frame_rate_limiter::FrameRateLimiter;

#[derive(Clone)]
pub struct FrameRequester {
    tx: mpsc::UnboundedSender<Instant>,
}

impl FrameRequester {
    /// Schedule a redraw as soon as possible.
    pub fn schedule_frame(&self) {
        let _ = self.tx.send(Instant::now());
    }

    /// Schedule a redraw `dur` from now.
    pub fn schedule_frame_in(&self, dur: Duration) {
        let _ = self.tx.send(Instant::now() + dur);
    }
}

pub struct FrameScheduler {
    rx: mpsc::UnboundedReceiver<Instant>,
    draw_tx: broadcast::Sender<()>,
    rate_limiter: FrameRateLimiter,
}

impl FrameScheduler {
    pub fn spawn() -> (FrameRequester, broadcast::Receiver<()>) {
        let (req_tx, req_rx) = mpsc::unbounded_channel();
        let (draw_tx, draw_rx) = broadcast::channel(1);
        let scheduler = Self {
            rx: req_rx,
            draw_tx: draw_tx.clone(),
            rate_limiter: FrameRateLimiter::new(),
        };
        tokio::spawn(scheduler.run());
        (FrameRequester { tx: req_tx }, draw_rx)
    }

    async fn run(mut self) {
        let mut next_deadline: Option<Instant> = None;
        loop {
            let now = Instant::now();
            let target = next_deadline.unwrap_or(now + Duration::from_secs(60 * 60));
            tokio::select! {
                ev = self.rx.recv() => {
                    match ev {
                        None => return,
                        Some(req) => {
                            let clamped = self.rate_limiter.clamp_deadline(req);
                            next_deadline = Some(match next_deadline {
                                Some(cur) => cur.min(clamped),
                                None => clamped,
                            });
                        }
                    }
                }
                _ = sleep_until(target.into()), if next_deadline.is_some() => {
                    next_deadline = None;
                    self.rate_limiter.mark_emitted(Instant::now());
                    let _ = self.draw_tx.send(());
                }
            }
        }
    }
}
