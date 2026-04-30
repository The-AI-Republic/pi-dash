//! Runs tab — recent runs for the picker-scoped runner.
//!
//! Owns its own `SelectableList<RunId>` so the cursor / scroll
//! position survives every 500ms IPC refresh — this is the per-tab
//! piece of the Bug-2 fix.

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, StatefulWidget};

use super::super::app::AppData;
use super::super::input::keymap::Context;
use super::super::view::tab::{Tab, TabCtx, TabKind};
use super::super::view::KeyHandled;
use super::super::widgets::SelectableList;
use uuid::Uuid;

pub struct RunsTab {
    list: SelectableList<Uuid>,
}

impl RunsTab {
    pub fn new() -> Self {
        Self {
            list: SelectableList::new(),
        }
    }

    pub fn reconcile(&mut self, data: &AppData) {
        let ids: Vec<Uuid> = data.runs.iter().map(|r| r.run_id).collect();
        self.list.reconcile(&ids);
    }
}

impl Default for RunsTab {
    fn default() -> Self {
        Self::new()
    }
}

impl Tab for RunsTab {
    fn kind(&self) -> TabKind {
        TabKind::Runs
    }

    fn render(&self, area: Rect, buf: &mut Buffer, data: &AppData) {
        let items: Vec<ListItem<'_>> = data
            .runs
            .iter()
            .map(|r| {
                let title = r.title.as_deref().unwrap_or("(no title)");
                ListItem::new(format!(
                    "{}  {:>9}  {}",
                    r.started_at.format("%m-%d %H:%M"),
                    r.status,
                    title
                ))
            })
            .collect();
        let list = List::new(items)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(ratatui::style::Color::Yellow))
                    .title(" Recent runs "),
            )
            .highlight_style(Style::default().add_modifier(Modifier::REVERSED));
        let mut lstate = ListState::default();
        lstate.select(self.list.selected_index());
        StatefulWidget::render(list, area, buf, &mut lstate);
    }

    fn handle_key(&mut self, key: KeyEvent, ctx: &mut TabCtx<'_>) -> KeyHandled {
        if key.kind != KeyEventKind::Press && key.kind != KeyEventKind::Repeat {
            return KeyHandled::Consumed;
        }
        let ids: Vec<Uuid> = ctx.data.runs.iter().map(|r| r.run_id).collect();
        match key.code {
            KeyCode::Char('j') | KeyCode::Down => {
                self.list.move_down(&ids);
                KeyHandled::Consumed
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.list.move_up(&ids);
                KeyHandled::Consumed
            }
            _ => KeyHandled::NotConsumed,
        }
    }

    fn active_contexts(&self) -> Vec<Context> {
        vec![Context::List]
    }

    fn on_focus(&mut self, ctx: &mut TabCtx<'_>) {
        self.reconcile(ctx.data);
    }
}
