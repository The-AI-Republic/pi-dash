//! Input pipeline: keymap + paste-burst.
//!
//! The keymap is the declarative `(KeyEvent, Context) → Action`
//! registry; the paste-burst module is a pure state machine that
//! coalesces rapid Char streams from terminals without bracketed
//! paste support. Both are pure (no I/O, no async) so they're cheap
//! to test.

pub mod keymap;
pub mod paste_burst;
