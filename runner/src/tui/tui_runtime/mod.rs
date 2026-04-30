//! Terminal runtime: `Tui` (the terminal owner) + `event_stream`
//! (crossterm → `TuiEvent`) + `frame_requester` (coalesced redraws).
//!
//! The whole module is the *L2 layer* in the design doc: nothing else
//! in the runner TUI touches `stdout`, raw mode, bracketed paste, or
//! draw timing. The `App` asks `Tui` to do these things; never reaches
//! past it.

pub mod event_stream;
pub mod frame_rate_limiter;
pub mod frame_requester;
pub mod runtime;

pub use event_stream::{TuiEvent, TuiEventStream};
pub use frame_requester::FrameRequester;
pub use runtime::Tui;
