//! Reusable TUI widgets composed from raw ratatui primitives.
//!
//! Ratatui itself ships only render-only widgets; input handling, focus,
//! scroll state, and validation are the application's responsibility.
//! Codex's TUI takes the same approach (see `codex-rs/tui/src/bottom_pane`).
//! This module lifts the patterns we need into a small reusable layer so
//! forms / overlays don't keep reinventing them.

pub mod picker;
pub mod scroll_state;
pub mod selectable_list;
pub mod textarea;

pub use scroll_state::ScrollState;
pub use selectable_list::SelectableList;
pub use textarea::TextArea;
