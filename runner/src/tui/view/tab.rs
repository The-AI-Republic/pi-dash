//! `Tab` — base-view contract.
//!
//! Each of the 4 tabs (General / Runners / Runs / Approvals) is a
//! `Tab` impl. Unlike `View`, a `Tab` is *not* modal: it lives below
//! the `view_stack`, owns its pane focus state, and routes keys
//! through the leaf-first rule (focused child → pane keymap → tab
//! keymap → global keymap).

use crossterm::event::KeyEvent;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use super::super::app::AppData;
use super::super::event_sender::AppEventSender;
use super::super::input::keymap::{Context, KeymapRegistry};
use super::super::tui_runtime::FrameRequester;
use super::focus::{CardId, FocusNode, FocusPath};
use super::KeyHandled;
use crate::util::paths::Paths;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TabKind {
    General,
    RunnerStatus,
    Runs,
    Approvals,
}

impl TabKind {
    pub fn label(self) -> &'static str {
        match self {
            TabKind::General => "General",
            TabKind::RunnerStatus => "Runners",
            TabKind::Runs => "Runs",
            TabKind::Approvals => "Approvals",
        }
    }

    pub fn idx(self) -> usize {
        match self {
            TabKind::General => 0,
            TabKind::RunnerStatus => 1,
            TabKind::Runs => 2,
            TabKind::Approvals => 3,
        }
    }

    pub fn from_idx(i: usize) -> Option<Self> {
        match i {
            0 => Some(TabKind::General),
            1 => Some(TabKind::RunnerStatus),
            2 => Some(TabKind::Runs),
            3 => Some(TabKind::Approvals),
            _ => None,
        }
    }

    pub fn all() -> [TabKind; 4] {
        [
            TabKind::General,
            TabKind::RunnerStatus,
            TabKind::Runs,
            TabKind::Approvals,
        ]
    }

    pub fn next(self) -> Self {
        let all = Self::all();
        all[(self.idx() + 1) % all.len()]
    }

    pub fn prev(self) -> Self {
        let all = Self::all();
        all[(self.idx() + all.len() - 1) % all.len()]
    }

    /// Parse `--tab` values: accepts the canonical name or a 1-based
    /// index (`1`–`4`). The old `config` alias resolves to
    /// `runners` since per-runner settings now live there.
    pub fn parse_cli(raw: &str) -> Option<TabKind> {
        let s = raw.trim().to_ascii_lowercase();
        match s.as_str() {
            "general" | "1" => Some(TabKind::General),
            "runners" | "runner" | "runner-status" | "runner_status" | "status"
            | "config" | "2" => Some(TabKind::RunnerStatus),
            "runs" | "3" => Some(TabKind::Runs),
            "approvals" | "4" => Some(TabKind::Approvals),
            _ => None,
        }
    }
}

pub struct TabCtx<'a> {
    pub tx: &'a AppEventSender,
    pub data: &'a mut AppData,
    pub keymap: &'a KeymapRegistry,
    pub paths: &'a Paths,
    pub frame: &'a FrameRequester,
}

pub trait Tab {
    fn kind(&self) -> TabKind;

    /// Render the tab body into `area`. Tabs draw their own internal
    /// layout (cards, lists, settings panels). Pure read of `data`.
    /// `focus` carries the current focus path so the tab can highlight
    /// the focused card with a yellow border.
    fn render(&self, area: Rect, buf: &mut Buffer, data: &AppData, focus: &FocusPath);

    /// Recompute the focusable card/item tree from current state.
    /// Called once per render so dynamic structure (e.g. the General
    /// tab's register form appearing on a fresh machine) is reflected
    /// immediately. Empty Vec = no focusable surface; focus stays on
    /// the tab bar (Layer 0).
    fn focus_tree(&self, _data: &AppData) -> Vec<FocusNode> {
        Vec::new()
    }

    /// Hand a key to the tab when focus is on an interactive Item AND
    /// the App-level dispatcher hasn't claimed the key (i.e. the key
    /// isn't a layer-nav key resolved by the tab's `active_contexts`).
    /// The tab dispatches based on `focus.current()`, e.g. typing into
    /// a TextArea, committing an edit buffer, etc. Returning `Consumed`
    /// suppresses any further action — including the App's auto-pop on
    /// Esc, so a tab that wants to discard an edit buffer locally can
    /// do so without exiting the layer.
    fn handle_item_key(
        &mut self,
        _key: KeyEvent,
        _ctx: &mut TabCtx<'_>,
        _focus: &FocusPath,
    ) -> KeyHandled {
        KeyHandled::NotConsumed
    }

    /// Tab-defined action when the user presses Enter on an
    /// interactive `Item`. For Cards the App handles Enter as
    /// "descend into the first child" — `activate_item` is only for
    /// leaves. Examples: cycle an enum field in place, open a buffer,
    /// trigger a Submit. Returning `KeyHandled::NotConsumed` lets the
    /// App fall through to its default (no-op).
    fn activate_item(
        &mut self,
        _item_id: CardId,
        _ctx: &mut TabCtx<'_>,
    ) -> KeyHandled {
        KeyHandled::NotConsumed
    }

    /// Accept a paste burst (bracketed-paste path). `focus` is the tab's
    /// current focus path so the tab can route the paste to whichever
    /// text field is focused. Default: ignore.
    fn handle_paste(
        &mut self,
        _text: String,
        _ctx: &mut TabCtx<'_>,
        _focus: &FocusPath,
    ) -> KeyHandled {
        KeyHandled::NotConsumed
    }

    /// Active keymap contexts for this tab given the current focus
    /// path. The dispatcher builds
    /// `[…active_contexts(), Tabs, Global]` and resolves keys through
    /// it. When a textarea is the focused leaf, this should return
    /// `[Context::TextInput]` *only* — no `Tabs`, no `Global` — so
    /// digit/letter keys can never be hijacked by tab switches.
    fn active_contexts(&self, _focus: &FocusPath) -> Vec<Context> {
        Vec::new()
    }

    /// Legacy keymap dispatch: until a tab is migrated to the
    /// focus-tree model, the App falls back to this for non-Layer-0
    /// keys. Default: ignore.
    fn handle_key(&mut self, _key: KeyEvent, _ctx: &mut TabCtx<'_>) -> KeyHandled {
        KeyHandled::NotConsumed
    }

    /// Cursor position the terminal should park at this frame.
    fn cursor_pos(
        &self,
        _area: Rect,
        _data: &AppData,
        _focus: &FocusPath,
    ) -> Option<(u16, u16)> {
        None
    }

    /// Called when the tab becomes active.
    fn on_focus(&mut self, _ctx: &mut TabCtx<'_>) {}
}
