//! Approvals tab — pending list + detail pane.
//!
//! Two top-level cards on the same row: `pending` (interactive) and
//! `detail` (read-only). ←/→ swaps focus between them. ↑/↓ on the
//! pending card moves the internal list cursor. The action hotkeys
//! `a`/`A`/`d` work whenever focus is anywhere on this tab.

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind};
use ratatui::buffer::Buffer;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph, StatefulWidget, Widget, Wrap};

use super::super::app::AppData;
use super::super::event::AppEvent;
use super::super::view::focus::{border_style, is_focused, FocusNode, FocusPath};
use super::super::view::tab::{Tab, TabCtx, TabKind};
use super::super::view::KeyHandled;
use super::super::widgets::SelectableList;
use crate::approval::router::ApprovalRecord;

const CARD_PENDING: &str = "pending";
const CARD_DETAIL: &str = "detail";

pub struct ApprovalsTab {
    list: SelectableList<String>,
}

impl ApprovalsTab {
    pub fn new() -> Self {
        Self {
            list: SelectableList::new(),
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

    fn focus_tree(&self, _data: &AppData) -> Vec<FocusNode> {
        vec![
            FocusNode::Card {
                id: CARD_PENDING,
                interactive: true,
                row: 0,
                children: Vec::new(),
            },
            FocusNode::Card {
                id: CARD_DETAIL,
                interactive: false,
                row: 0,
                children: Vec::new(),
            },
        ]
    }

    fn render(&self, area: Rect, buf: &mut Buffer, data: &AppData, focus: &FocusPath) {
        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(40), Constraint::Percentage(60)])
            .split(area);

        let items: Vec<ListItem<'_>> = data
            .approvals
            .iter()
            .map(|r| {
                let runner_label = data
                    .status
                    .as_ref()
                    .and_then(|s| s.runners.iter().find(|run| run.runner_id == r.runner_id))
                    .map(|run| run.name.clone())
                    .unwrap_or_else(|| "—".into());
                ListItem::new(format!(
                    "{}  [{}]  {:?}",
                    r.requested_at.format("%H:%M:%S"),
                    runner_label,
                    r.kind
                ))
            })
            .collect();
        let list = List::new(items)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(border_style(is_focused(focus, CARD_PENDING)))
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
                let runner_label = data
                    .status
                    .as_ref()
                    .and_then(|s| s.runners.iter().find(|run| run.runner_id == r.runner_id))
                    .map(|run| run.name.clone())
                    .unwrap_or_else(|| r.runner_id.to_string());
                format!(
                    "approval_id: {}\nrunner: {}\nrun_id: {}\nkind: {:?}\nreason: {}\n\npayload:\n{}",
                    r.approval_id, runner_label, r.run_id, r.kind, reason, pretty
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
                    .border_style(border_style(is_focused(focus, CARD_DETAIL)))
                    .title(" Detail "),
            )
            .wrap(Wrap { trim: false });
        detail.render(chunks[1], buf);
    }

    fn handle_item_key(
        &mut self,
        key: KeyEvent,
        ctx: &mut TabCtx<'_>,
        focus: &FocusPath,
    ) -> KeyHandled {
        if key.kind != KeyEventKind::Press && key.kind != KeyEventKind::Repeat {
            return KeyHandled::NotConsumed;
        }
        // List cursor only moves while focus is on the pending card —
        // otherwise ↑/↓ falling through here from the dispatcher means
        // "no sibling above/below" and we leave it for the legacy
        // keymap to maybe handle (e.g. RunnerPicker).
        if focus.current() == Some(CARD_PENDING) {
            let ids: Vec<String> = ctx
                .data
                .approvals
                .iter()
                .map(|r| r.approval_id.clone())
                .collect();
            match key.code {
                KeyCode::Char('j') | KeyCode::Down => {
                    self.list.move_down(&ids);
                    return KeyHandled::Consumed;
                }
                KeyCode::Char('k') | KeyCode::Up => {
                    self.list.move_up(&ids);
                    return KeyHandled::Consumed;
                }
                _ => {}
            }
        }
        // Action hotkeys are tab-wide; they fire regardless of which
        // card is focused so the user can decide an approval without
        // first ←/→ to the pending list.
        match key.code {
            KeyCode::Char('a') => {
                if let Some(rec) = self.selected_record(ctx.data) {
                    ctx.tx.send(AppEvent::Approval {
                        approval_id: rec.approval_id.clone(),
                        runner_id: rec.runner_id,
                        decision: crate::cloud::protocol::ApprovalDecision::Accept,
                    });
                }
                KeyHandled::Consumed
            }
            KeyCode::Char('A') => {
                if let Some(rec) = self.selected_record(ctx.data) {
                    ctx.tx.send(AppEvent::Approval {
                        approval_id: rec.approval_id.clone(),
                        runner_id: rec.runner_id,
                        decision: crate::cloud::protocol::ApprovalDecision::AcceptForSession,
                    });
                }
                KeyHandled::Consumed
            }
            KeyCode::Char('d') => {
                if let Some(rec) = self.selected_record(ctx.data) {
                    ctx.tx.send(AppEvent::Approval {
                        approval_id: rec.approval_id.clone(),
                        runner_id: rec.runner_id,
                        decision: crate::cloud::protocol::ApprovalDecision::Decline,
                    });
                }
                KeyHandled::Consumed
            }
            _ => KeyHandled::NotConsumed,
        }
    }

    fn on_focus(&mut self, ctx: &mut TabCtx<'_>) {
        self.reconcile(ctx.data);
    }
}

