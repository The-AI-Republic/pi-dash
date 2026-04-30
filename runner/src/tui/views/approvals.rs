//! Approvals tab — pending approvals + a detail pane.
//!
//! Two panes: a pending list (left) and a detail viewer (right).
//! Tab toggles focus between them. The list owns a stable
//! `SelectableList<approval_id>` so selection survives every 500ms
//! tick (Bug-2 fix).

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind};
use ratatui::buffer::Buffer;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph, StatefulWidget, Widget, Wrap};

use super::super::app::AppData;
use super::super::event::AppEvent;
use super::super::input::keymap::Context;
use super::super::view::tab::{Tab, TabCtx, TabKind};
use super::super::view::KeyHandled;
use super::super::widgets::SelectableList;
use crate::approval::router::ApprovalRecord;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Pane {
    #[default]
    Pending,
    Detail,
}

pub struct ApprovalsTab {
    list: SelectableList<String>,
    pane: Pane,
}

impl ApprovalsTab {
    pub fn new() -> Self {
        Self {
            list: SelectableList::new(),
            pane: Pane::Pending,
        }
    }

    pub fn reconcile(&mut self, data: &AppData) {
        let ids: Vec<String> = data.approvals.iter().map(|r| r.approval_id.clone()).collect();
        self.list.reconcile(&ids);
    }

    pub fn selected_record<'a>(&self, data: &'a AppData) -> Option<&'a ApprovalRecord> {
        let id = self.list.selected_id()?;
        data.approvals.iter().find(|r| &r.approval_id == id)
    }
}

impl Default for ApprovalsTab {
    fn default() -> Self {
        Self::new()
    }
}

impl Tab for ApprovalsTab {
    fn kind(&self) -> TabKind {
        TabKind::Approvals
    }

    fn render(&self, area: Rect, buf: &mut Buffer, data: &AppData) {
        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(40), Constraint::Percentage(60)])
            .split(area);

        let pending_focused = matches!(self.pane, Pane::Pending);
        let detail_focused = matches!(self.pane, Pane::Detail);

        let items: Vec<ListItem<'_>> = data
            .approvals
            .iter()
            .map(|r| {
                ListItem::new(format!(
                    "{}  {:?}",
                    r.requested_at.format("%H:%M:%S"),
                    r.kind
                ))
            })
            .collect();
        let list = List::new(items)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(if pending_focused {
                        Style::default().fg(Color::Yellow)
                    } else {
                        Style::default()
                    })
                    .title(" Pending "),
            )
            .highlight_style(Style::default().add_modifier(Modifier::REVERSED));
        let mut lstate = ListState::default();
        lstate.select(self.list.selected_index());
        StatefulWidget::render(list, chunks[0], buf, &mut lstate);

        let detail_text = self
            .selected_record(data)
            .map(|r| {
                let pretty = serde_json::to_string_pretty(&r.payload).unwrap_or_default();
                let reason = r.reason.clone().unwrap_or_default();
                format!(
                    "approval_id: {}\nrun_id: {}\nkind: {:?}\nreason: {}\n\npayload:\n{}",
                    r.approval_id, r.run_id, r.kind, reason, pretty
                )
            })
            .unwrap_or_else(|| "Select an approval (j/k) to see details.".to_string());

        let lines = vec![
            Line::from(detail_text),
            Line::raw(""),
            Line::from("[a] Accept   [A] Accept for session   [d] Decline"),
        ];
        let detail = Paragraph::new(lines)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(if detail_focused {
                        Style::default().fg(Color::Yellow)
                    } else {
                        Style::default()
                    })
                    .title(" Detail "),
            )
            .wrap(Wrap { trim: false });
        detail.render(chunks[1], buf);
    }

    fn handle_key(&mut self, key: KeyEvent, ctx: &mut TabCtx<'_>) -> KeyHandled {
        if key.kind != KeyEventKind::Press && key.kind != KeyEventKind::Repeat {
            return KeyHandled::Consumed;
        }
        let ids: Vec<String> = ctx
            .data
            .approvals
            .iter()
            .map(|r| r.approval_id.clone())
            .collect();
        match (key.code, key.modifiers) {
            (KeyCode::Tab, _) | (KeyCode::BackTab, _) => {
                self.pane = match self.pane {
                    Pane::Pending => Pane::Detail,
                    Pane::Detail => Pane::Pending,
                };
                KeyHandled::Consumed
            }
            (KeyCode::Char('j') | KeyCode::Down, _) => {
                self.list.move_down(&ids);
                KeyHandled::Consumed
            }
            (KeyCode::Char('k') | KeyCode::Up, _) => {
                self.list.move_up(&ids);
                KeyHandled::Consumed
            }
            (KeyCode::Char('a'), _) => {
                if let Some(rec) = self.selected_record(ctx.data) {
                    ctx.tx.send(AppEvent::Approval {
                        approval_id: rec.approval_id.clone(),
                        decision: crate::cloud::protocol::ApprovalDecision::Accept,
                    });
                }
                KeyHandled::Consumed
            }
            (KeyCode::Char('A'), _) => {
                if let Some(rec) = self.selected_record(ctx.data) {
                    ctx.tx.send(AppEvent::Approval {
                        approval_id: rec.approval_id.clone(),
                        decision: crate::cloud::protocol::ApprovalDecision::AcceptForSession,
                    });
                }
                KeyHandled::Consumed
            }
            (KeyCode::Char('d'), _) => {
                if let Some(rec) = self.selected_record(ctx.data) {
                    ctx.tx.send(AppEvent::Approval {
                        approval_id: rec.approval_id.clone(),
                        decision: crate::cloud::protocol::ApprovalDecision::Decline,
                    });
                }
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
