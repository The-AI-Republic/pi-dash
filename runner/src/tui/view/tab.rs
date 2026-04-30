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
use super::KeyHandled;
use crate::util::paths::Paths;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
    fn render(&self, area: Rect, buf: &mut Buffer, data: &AppData);

    /// Hand a key to the tab. The tab is responsible for the
    /// leaf-first rule: it should hand the key to its focused
    /// textarea / list before consulting any pane-level binding.
    /// Returns `Consumed` once any layer claims the key.
    fn handle_key(&mut self, key: KeyEvent, ctx: &mut TabCtx<'_>) -> KeyHandled;

    /// Accept a paste burst (bracketed-paste path). Default: ignore.
    fn handle_paste(&mut self, _text: String, _ctx: &mut TabCtx<'_>) -> KeyHandled {
        KeyHandled::NotConsumed
    }

    /// Active keymap contexts for this tab right now. The dispatcher
    /// builds `[…active_contexts(), Tabs, Global]` and resolves keys
    /// through it (`§5.5`). When a textarea is focused inside the
    /// tab, this should return `[Context::TextInput]` *only* — no
    /// `Tabs`, no `Global` — so digit/letter keys can never be
    /// hijacked by tab switches.
    fn active_contexts(&self) -> Vec<Context>;

    /// Cursor position the terminal should park at this frame.
    fn cursor_pos(&self, _area: Rect, _data: &AppData) -> Option<(u16, u16)> {
        None
    }

    /// Called when the tab becomes active.
    fn on_focus(&mut self, _ctx: &mut TabCtx<'_>) {}
}
