//! `View` trait + completion model.
//!
//! A `View` is anything pushed onto the modal stack: a confirm dialog,
//! the add-runner form, the help overlay. Every view owns its own
//! state and key handling. The base tabs ("General", "Runners", …)
//! are *not* `View`s — they are `Tab`s in `view::tab` — because they
//! are not modal and have richer pane structure.
//!
//! Shape borrowed from codex `bottom_pane/bottom_pane_view.rs`,
//! adapted for our flat single-stack model (no full-screen overlay
//! today).

use crossterm::event::KeyEvent;

use super::event_sender::AppEventSender;
use super::input::keymap::KeymapRegistry;
use super::render::Renderable;
use crate::util::paths::Paths;

pub mod tab;

pub use tab::{Tab as TabView, TabKind};

/// Did the view consume the key, or should we fall through to the
/// underlying handler? Borrowed from claudy's `Consumed | NotConsumed`
/// (`useKeybinding.ts:113-121`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyHandled {
    Consumed,
    NotConsumed,
}

/// What happened when a view auto-popped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewCompletion {
    Accepted,
    Cancelled,
}

/// Did `on_ctrl_c` consume the Ctrl+C, or should the global handler
/// fire? Mirrors codex's `CancellationEvent`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cancellation {
    Handled,
    NotHandled,
}

/// Context handed to a view's `handle_key`. Carries the bus sender,
/// the current keymap (for chord state in future), and a few config
/// paths views might need to spawn their own tasks.
pub struct ViewCtx<'a> {
    pub tx: &'a AppEventSender,
    pub keymap: &'a KeymapRegistry,
    pub paths: &'a Paths,
}

pub trait View: Renderable {
    fn handle_key(&mut self, key: KeyEvent, ctx: &mut ViewCtx<'_>) -> KeyHandled;

    fn handle_paste(&mut self, _text: String, _ctx: &mut ViewCtx<'_>) -> KeyHandled {
        KeyHandled::NotConsumed
    }

    /// Once true, the dispatcher pops this view (and runs the
    /// dismiss-after-child-accept chain).
    fn is_complete(&self) -> bool {
        false
    }

    fn completion(&self) -> Option<ViewCompletion> {
        None
    }

    fn dismiss_after_child_accept(&self) -> bool {
        false
    }

    /// True for transient modals — global hotkeys (`q`, digit keys)
    /// are not consulted while a modal is on top. False for "base
    /// surfaces" pushed onto the stack (we don't have any today, but
    /// the hook is here for the future).
    fn is_modal(&self) -> bool {
        true
    }

    fn on_ctrl_c(&mut self, _ctx: &mut ViewCtx<'_>) -> Cancellation {
        Cancellation::NotHandled
    }
}
