//! `ScrollState` — selection cursor + scroll offset over a finite
//! item list.
//!
//! Ported from `codex-rs/tui/src/bottom_pane/scroll_state.rs`
//! (Apache-2.0). Trimmed: the runner doesn't need codex's incremental
//! page-up/down with scrollback math beyond `ensure_visible`. Tests
//! mirror codex's verbatim.
//!
//! The pair `(selected, scroll_top)` is the only state needed to drive
//! a bounded list with arrow-key navigation. `ScrollState::reconcile`
//! is the bug-2 fix point: when items are reloaded by IPC, the
//! caller passes the same `len` and selection survives if it's still
//! in range.

#[derive(Debug, Clone, Default)]
pub struct ScrollState {
    pub selected: Option<usize>,
    pub scroll_top: usize,
}

impl ScrollState {
    /// Clamp `selected` and `scroll_top` to `len`. If the list is
    /// empty both go to 0 / `None`.
    pub fn clamp(&mut self, len: usize) {
        if len == 0 {
            self.selected = None;
            self.scroll_top = 0;
            return;
        }
        if let Some(s) = self.selected {
            if s >= len {
                self.selected = Some(len - 1);
            }
        } else {
            self.selected = Some(0);
        }
        if self.scroll_top >= len {
            self.scroll_top = len.saturating_sub(1);
        }
    }

    pub fn move_up_wrap(&mut self, len: usize) {
        if len == 0 {
            self.selected = None;
            return;
        }
        let cur = self.selected.unwrap_or(0);
        self.selected = Some(if cur == 0 { len - 1 } else { cur - 1 });
    }

    pub fn move_down_wrap(&mut self, len: usize) {
        if len == 0 {
            self.selected = None;
            return;
        }
        let cur = self.selected.unwrap_or(0);
        self.selected = Some((cur + 1) % len);
    }

    /// Adjust `scroll_top` so `selected` is visible inside a window
    /// of `rows` rows. Pulls down (selected near bottom) or up
    /// (selected scrolled past).
    pub fn ensure_visible(&mut self, len: usize, rows: usize) {
        if len == 0 || rows == 0 {
            self.scroll_top = 0;
            return;
        }
        let Some(s) = self.selected else {
            return;
        };
        if s < self.scroll_top {
            self.scroll_top = s;
        } else if s >= self.scroll_top + rows {
            self.scroll_top = s + 1 - rows;
        }
        // Don't scroll past the last possible top — keep the window full.
        let max_top = len.saturating_sub(rows);
        if self.scroll_top > max_top {
            self.scroll_top = max_top;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clamp_empty_zeros_state() {
        let mut s = ScrollState {
            selected: Some(5),
            scroll_top: 3,
        };
        s.clamp(0);
        assert_eq!(s.selected, None);
        assert_eq!(s.scroll_top, 0);
    }

    #[test]
    fn clamp_within_bounds_no_op() {
        let mut s = ScrollState {
            selected: Some(2),
            scroll_top: 1,
        };
        s.clamp(5);
        assert_eq!(s.selected, Some(2));
        assert_eq!(s.scroll_top, 1);
    }

    #[test]
    fn clamp_out_of_range_pulled_in() {
        let mut s = ScrollState {
            selected: Some(10),
            scroll_top: 8,
        };
        s.clamp(3);
        assert_eq!(s.selected, Some(2));
        assert_eq!(s.scroll_top, 2);
    }

    #[test]
    fn move_up_wrap_at_top_goes_to_end() {
        let mut s = ScrollState {
            selected: Some(0),
            scroll_top: 0,
        };
        s.move_up_wrap(4);
        assert_eq!(s.selected, Some(3));
    }

    #[test]
    fn move_down_wrap_at_end_goes_to_top() {
        let mut s = ScrollState {
            selected: Some(3),
            scroll_top: 0,
        };
        s.move_down_wrap(4);
        assert_eq!(s.selected, Some(0));
    }

    #[test]
    fn ensure_visible_pulls_window_down() {
        let mut s = ScrollState {
            selected: Some(7),
            scroll_top: 0,
        };
        s.ensure_visible(20, 5);
        // selected=7, rows=5 → scroll_top should be 3 (so 3..8 is visible)
        assert_eq!(s.scroll_top, 3);
    }

    #[test]
    fn ensure_visible_pulls_window_up() {
        let mut s = ScrollState {
            selected: Some(2),
            scroll_top: 5,
        };
        s.ensure_visible(20, 5);
        assert_eq!(s.scroll_top, 2);
    }
}
