//! Paste-burst state machine.
//!
//! Some terminals (Windows console, certain SSH paths, embedded VT
//! emulators) do *not* support bracketed paste — they deliver pasted
//! text as a stream of `KeyCode::Char` events that look exactly like
//! rapid typing. This module collapses those bursts back into a
//! single chunk so:
//!
//! - A pasted Enter doesn't submit mid-paste.
//! - A pasted `?` / `q` / `1` doesn't fire its bound action.
//! - Non-ASCII chars (IME) bypass the buffer entirely.
//!
//! Vendored from `codex-rs/tui/src/bottom_pane/paste_burst.rs`
//! (Apache-2.0). Trimmed: single-line only, no kill-buffer rules.
//!
//! The state machine is **pure** — the caller applies decisions to
//! its own textarea. We never own the textarea.

use std::time::{Duration, Instant};

const BURST_WINDOW: Duration = Duration::from_millis(8);
/// If the buffered text reached this many chars, treat the next plain
/// char as part of a paste regardless of timing.
const BURST_BUFFER_THRESHOLD: usize = 16;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BurstDecision {
    /// Insert this char as a normal keystroke. The caller should run
    /// any usual side effects (e.g. its on_change callback).
    InsertChar(char),
    /// Buffer this char inside the burst — the caller should *not*
    /// run side effects (no submit, no shortcuts).
    Buffer(char),
    /// Flush is due. Returned text was the buffered chunk; caller
    /// should insert it as a single paste.
    Flush(String),
    /// A non-ASCII (IME) char arrived mid-burst. The caller should
    /// *first* insert the buffered chunk as a paste, *then* insert the
    /// trigger char as a normal keystroke. Atomic so the IME char
    /// can't be silently swallowed by the flush.
    FlushAndInsert(String, char),
    /// Caller should ignore this event.
    Ignore,
}

#[derive(Debug, Clone, Default)]
pub struct PasteBurst {
    buffer: String,
    last_at: Option<Instant>,
}

impl PasteBurst {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn buffer(&self) -> &str {
        &self.buffer
    }

    pub fn is_empty(&self) -> bool {
        self.buffer.is_empty()
    }

    /// Drain the buffer (e.g. before a key that is not part of the
    /// burst, or when a frame deadline expires).
    pub fn flush(&mut self) -> Option<String> {
        if self.buffer.is_empty() {
            return None;
        }
        self.last_at = None;
        Some(std::mem::take(&mut self.buffer))
    }

    /// Plain char arrived (no Ctrl/Alt). Decide whether it's part of a
    /// burst or a normal keystroke. Non-ASCII chars (IME) bypass
    /// buffering — they're never paste-stream artifacts.
    pub fn on_plain_char(&mut self, c: char, now: Instant) -> BurstDecision {
        if !c.is_ascii() {
            // Bypass: flush whatever's pending first as a chunk, then
            // surface the trigger char in the same decision so the
            // caller can apply both atomically. The buffer is left
            // empty — non-ASCII chars must never linger as buffered
            // state since they're not paste-stream artifacts.
            if let Some(s) = self.flush() {
                return BurstDecision::FlushAndInsert(s, c);
            }
            return BurstDecision::InsertChar(c);
        }
        match self.last_at {
            None => {
                self.buffer.push(c);
                self.last_at = Some(now);
                BurstDecision::Buffer(c)
            }
            Some(prev) => {
                let dt = now.duration_since(prev);
                if dt <= BURST_WINDOW || self.buffer.len() >= BURST_BUFFER_THRESHOLD {
                    self.buffer.push(c);
                    self.last_at = Some(now);
                    BurstDecision::Buffer(c)
                } else {
                    // Gap exceeded — flush the prior burst, start a new
                    // pending char as a normal keystroke.
                    let flushed = self.flush();
                    self.buffer.push(c);
                    self.last_at = Some(now);
                    match flushed {
                        Some(s) if !s.is_empty() => BurstDecision::Flush(s),
                        _ => BurstDecision::Buffer(c),
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rapid_chars_buffer_into_single_flush() {
        let mut p = PasteBurst::new();
        let t0 = Instant::now();
        let _ = p.on_plain_char('a', t0);
        let _ = p.on_plain_char('b', t0 + Duration::from_micros(100));
        let _ = p.on_plain_char('c', t0 + Duration::from_micros(200));
        assert_eq!(p.buffer(), "abc");
        let flushed = p.flush();
        assert_eq!(flushed.as_deref(), Some("abc"));
    }

    #[test]
    fn slow_typing_flushes_per_char() {
        let mut p = PasteBurst::new();
        let mut t = Instant::now();
        let d1 = p.on_plain_char('a', t);
        assert_eq!(d1, BurstDecision::Buffer('a'));
        t += Duration::from_millis(50);
        let d2 = p.on_plain_char('b', t);
        // Gap > 8ms: prior 'a' flushes; 'b' is the new pending.
        match d2 {
            BurstDecision::Flush(s) => assert_eq!(s, "a"),
            other => panic!("expected Flush, got {:?}", other),
        }
    }

    #[test]
    fn non_ascii_bypasses_buffer() {
        let mut p = PasteBurst::new();
        let t = Instant::now();
        let d = p.on_plain_char('é', t);
        assert!(matches!(d, BurstDecision::InsertChar('é')));
        assert!(p.is_empty());
    }

    #[test]
    fn non_ascii_mid_burst_flushes_and_inserts_atomically() {
        // Regression: previously this branch swallowed the IME char by
        // stuffing it into the buffer with `last_at` set, then only
        // returning `Flush` for the prior text. The trigger char would
        // be invisible until the next keypress.
        let mut p = PasteBurst::new();
        let t0 = Instant::now();
        let _ = p.on_plain_char('a', t0);
        let _ = p.on_plain_char('b', t0 + Duration::from_micros(100));
        assert_eq!(p.buffer(), "ab");

        let d = p.on_plain_char('é', t0 + Duration::from_micros(200));
        match d {
            BurstDecision::FlushAndInsert(s, c) => {
                assert_eq!(s, "ab");
                assert_eq!(c, 'é');
            }
            other => panic!("expected FlushAndInsert, got {:?}", other),
        }
        // Buffer must be empty afterwards — the IME char is not a
        // paste-stream artifact and must never linger as buffered state.
        assert!(p.is_empty());
        assert!(p.last_at.is_none());
    }
}
