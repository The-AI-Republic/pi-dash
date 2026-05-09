//! Help overlay.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::buffer::Buffer;
use ratatui::layout::{Alignment, Rect};
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Widget};

use crate::tui::render::Renderable;
use crate::tui::view::{KeyHandled, View, ViewCompletion, ViewCtx};

use super::confirm::centered_rect;

pub struct HelpView {
    complete: bool,
}

impl HelpView {
    pub fn new() -> Self {
        Self { complete: false }
    }
}

impl Default for HelpView {
    fn default() -> Self {
        Self::new()
    }
}

impl Renderable for HelpView {
    fn render(&self, area: Rect, buf: &mut Buffer) {
        let modal = centered_rect(60, 60, area);
        Clear.render(modal, buf);
        let body = Paragraph::new(vec![
            Line::from("Pi Dash Runner — TUI help"),
            Line::raw(""),
            Line::from("1–4       jump to view"),
            Line::from("h/l ←/→   prev/next view"),
            Line::from("j/k ↑/↓   move selection"),
            Line::from("↵     open detail"),
            Line::from("r     force refresh"),
            Line::from("s     start runner service  (General tab)"),
            Line::from("x     stop runner service   (General tab)"),
            Line::from("↵     edit field / toggle  (General + Runners settings panel)"),
            Line::from("w     save + reload daemon (General + Runners)"),
            Line::from("Esc   discard pending edits (General + Runners)"),
            Line::from("a     accept approval        (Approvals tab)"),
            Line::from("a     add a runner           (Runners tab)"),
            Line::from("A     accept for session     (Approvals tab)"),
            Line::from("d     decline                (Approvals tab)"),
            Line::from("d     remove highlighted runner (Runners tab)"),
            Line::raw(""),
            Line::from("Multi-runner picker (Runners / Runs / Approvals):"),
            Line::from("</,    previous runner"),
            Line::from(">/.    next runner"),
            Line::from("Alt+N  jump to runner N (1–9)"),
            Line::from("q / Ctrl+C  quit TUI (asks for confirmation)"),
            Line::from("Q           stop daemon (asks for confirmation)"),
            Line::from("?     toggle this help"),
        ])
        .alignment(Alignment::Left)
        .block(Block::default().borders(Borders::ALL).title(" Help "));
        body.render(modal, buf);
    }
}

impl View for HelpView {
    fn handle_key(&mut self, key: KeyEvent, _ctx: &mut ViewCtx<'_>) -> KeyHandled {
        let is_close = matches!(
            key.code,
            KeyCode::Esc | KeyCode::Char('?') | KeyCode::Char('q')
        ) || (matches!(key.code, KeyCode::Char('c'))
            && key.modifiers.contains(KeyModifiers::CONTROL));
        if is_close {
            self.complete = true;
        }
        KeyHandled::Consumed
    }
    fn is_complete(&self) -> bool {
        self.complete
    }
    fn completion(&self) -> Option<ViewCompletion> {
        Some(ViewCompletion::Cancelled)
    }
}
