//! Runners tab — list of every runner this daemon hosts plus the
//! per-runner settings panel for the highlighted runner.
//!
//! Owns: a `SelectableList<runner_name>` (bug-2 fix — selection
//! survives refresh and `[d]` reads the same source as the
//! highlight); a `Pane` enum (`Pane::List | Pane::Settings`) — the
//! single source of truth for visual focus (bug-1 fix); the
//! settings-pane field cursor + an optional inline `TextArea` edit
//! buffer.

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::buffer::Buffer;
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Block, Borders, List, ListItem, ListState, Paragraph, StatefulWidget, Widget, Wrap,
};

use super::super::app::AppData;
use super::super::event::AppEvent;
use super::super::input::keymap::Context;
use super::super::view::tab::{Tab, TabCtx, TabKind};
use super::super::view::KeyHandled;
use super::super::widgets::{SelectableList, TextArea};
use super::config as fields;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Pane {
    #[default]
    List,
    Settings,
}

pub struct RunnerStatusTab {
    list: SelectableList<String>,
    pane: Pane,
    /// Index into `FIELDS` for the settings pane field cursor.
    field_idx: usize,
    /// Inline edit buffer for Text/U32 fields.
    edit_buffer: Option<TextArea>,
    /// Which field id we're editing — needed to commit back via
    /// `set_text_value` even after the cursor moves.
    editing_field: Option<fields::FieldId>,
}

impl RunnerStatusTab {
    pub fn new() -> Self {
        Self {
            list: SelectableList::new(),
            pane: Pane::List,
            field_idx: 0,
            edit_buffer: None,
            editing_field: None,
        }
    }

    pub fn reconcile(&mut self, data: &AppData) {
        let ids: Vec<String> = match &data.status {
            Some(s) => s.runners.iter().map(|r| r.name.clone()).collect(),
            None => Vec::new(),
        };
        self.list.reconcile(&ids);
        // Keep the global picker idx in sync with the list selection
        // so `<`/`>` and Alt+N stay coherent across tab switches.
        if let Some(idx) = self.list.selected_index() {
            // Note: we don't push back to the AppData here — the
            // dispatcher does that explicitly via `SelectRunner`.
            let _ = idx;
        }
    }

    pub fn toggle_focus(&mut self) {
        self.pane = match self.pane {
            Pane::List => Pane::Settings,
            Pane::Settings => Pane::List,
        };
    }

    pub fn selected_runner_name(&self, data: &AppData) -> Option<String> {
        // Prefer the list's id; fall back to the picker index for
        // fresh-load case where the list hasn't reconciled yet.
        if let Some(id) = self.list.selected_id() {
            return Some(id.clone());
        }
        let working = data.config_working.as_ref()?;
        working.runners.get(data.runner_picker_idx).map(|r| r.name.clone())
    }
}

impl Default for RunnerStatusTab {
    fn default() -> Self {
        Self::new()
    }
}

impl Tab for RunnerStatusTab {
    fn kind(&self) -> TabKind {
        TabKind::RunnerStatus
    }

    fn render(&self, area: Rect, buf: &mut Buffer, data: &AppData) {
        if data.config_working.is_none() {
            render_unregistered_placeholder(area, buf);
            return;
        }
        let outer = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(8), Constraint::Length(3)])
            .split(area);
        let body = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Length(36), Constraint::Min(40)])
            .split(outer[0]);
        self.render_runner_list(body[0], buf, data);
        self.render_settings_panel(body[1], buf, data);
        hotkeys_card().render(outer[1], buf);
    }

    fn handle_key(&mut self, key: KeyEvent, ctx: &mut TabCtx<'_>) -> KeyHandled {
        if key.kind != KeyEventKind::Press && key.kind != KeyEventKind::Repeat {
            return KeyHandled::Consumed;
        }
        // 1) Edit-buffer path: textarea is the focused leaf.
        if self.edit_buffer.is_some() {
            return self.handle_edit_buffer_key(key, ctx);
        }
        let runner_ids: Vec<String> = match &ctx.data.status {
            Some(s) => s.runners.iter().map(|r| r.name.clone()).collect(),
            None => Vec::new(),
        };

        match (key.code, key.modifiers) {
            (KeyCode::Tab, _) | (KeyCode::BackTab, _) => {
                self.toggle_focus();
                KeyHandled::Consumed
            }
            (KeyCode::Char('j') | KeyCode::Down, _) => {
                match self.pane {
                    Pane::List => {
                        self.list.move_down(&runner_ids);
                        if let Some(idx) = self.list.selected_index() {
                            ctx.tx.send(AppEvent::SelectRunner(idx));
                        }
                    }
                    Pane::Settings => {
                        let n = fields::field_count();
                        if n > 0 {
                            self.field_idx = (self.field_idx + 1) % n;
                        }
                    }
                }
                KeyHandled::Consumed
            }
            (KeyCode::Char('k') | KeyCode::Up, _) => {
                match self.pane {
                    Pane::List => {
                        self.list.move_up(&runner_ids);
                        if let Some(idx) = self.list.selected_index() {
                            ctx.tx.send(AppEvent::SelectRunner(idx));
                        }
                    }
                    Pane::Settings => {
                        let n = fields::field_count();
                        if n > 0 {
                            self.field_idx = if self.field_idx == 0 { n - 1 } else { self.field_idx - 1 };
                        }
                    }
                }
                KeyHandled::Consumed
            }
            (KeyCode::Enter, _) if matches!(self.pane, Pane::Settings) => {
                self.start_or_apply_settings_field(ctx);
                KeyHandled::Consumed
            }
            (KeyCode::Char('w'), _) => {
                ctx.tx.send(AppEvent::SaveConfig);
                KeyHandled::Consumed
            }
            (KeyCode::Esc, _) => {
                ctx.tx.send(AppEvent::DiscardConfigEdits);
                KeyHandled::Consumed
            }
            (KeyCode::Char('a'), m) if !m.contains(KeyModifiers::CONTROL) => {
                let v = super::modals::add_runner::AddRunnerView::open(
                    ctx.data,
                    ctx.tx.clone(),
                    ctx.paths.clone(),
                );
                ctx.tx.push_view(Box::new(v));
                KeyHandled::Consumed
            }
            (KeyCode::Char('d'), m) if !m.contains(KeyModifiers::CONTROL) => {
                if let Some(name) = self.selected_runner_name(ctx.data) {
                    let v = super::modals::remove_runner::RemoveRunnerView::new(name);
                    ctx.tx.push_view(Box::new(v));
                }
                KeyHandled::Consumed
            }
            _ => KeyHandled::NotConsumed,
        }
    }

    fn active_contexts(&self) -> Vec<Context> {
        if self.edit_buffer.is_some() {
            vec![Context::TextInput]
        } else {
            match self.pane {
                Pane::List => vec![Context::List],
                Pane::Settings => vec![Context::Settings],
            }
        }
    }

    fn on_focus(&mut self, ctx: &mut TabCtx<'_>) {
        self.reconcile(ctx.data);
    }
}

impl RunnerStatusTab {
    fn handle_edit_buffer_key(&mut self, key: KeyEvent, ctx: &mut TabCtx<'_>) -> KeyHandled {
        // Leaf-first: textarea claims printable / cursor / backspace.
        if let Some(buf) = self.edit_buffer.as_mut() {
            let h = buf.handle_key(key);
            if matches!(h, KeyHandled::Consumed) {
                return KeyHandled::Consumed;
            }
        }
        match key.code {
            KeyCode::Enter => {
                self.commit_edit_buffer(ctx);
                KeyHandled::Consumed
            }
            KeyCode::Esc => {
                self.edit_buffer = None;
                self.editing_field = None;
                ctx.data.config_edit_error = None;
                KeyHandled::Consumed
            }
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                KeyHandled::NotConsumed
            }
            _ => KeyHandled::Consumed,
        }
    }

    fn start_or_apply_settings_field(&mut self, ctx: &mut TabCtx<'_>) {
        let Some(cfg) = ctx.data.config_working.as_mut() else {
            return;
        };
        if fields::field_count() == 0 {
            return;
        }
        let idx = self.field_idx.min(fields::field_count() - 1);
        let spec = fields::field_at(idx);
        ctx.data.config_edit_error = None;
        let runner_idx = ctx.data.runner_picker_idx;
        match spec.kind {
            fields::FieldKind::Bool => fields::toggle_bool(cfg, spec.id, runner_idx),
            fields::FieldKind::Enum(_) => fields::cycle_enum(cfg, spec.id, runner_idx),
            fields::FieldKind::Text | fields::FieldKind::U32 => {
                let value = fields::display_value(cfg, spec.id, runner_idx);
                self.edit_buffer = Some(TextArea::with_text(value));
                self.editing_field = Some(spec.id);
            }
        }
    }

    fn commit_edit_buffer(&mut self, ctx: &mut TabCtx<'_>) {
        let Some(buf) = self.edit_buffer.take() else {
            return;
        };
        let Some(id) = self.editing_field.take() else {
            return;
        };
        let text = buf.text().to_string();
        let Some(cfg) = ctx.data.config_working.as_mut() else {
            return;
        };
        let runner_idx = ctx.data.runner_picker_idx;
        match fields::set_text_value(cfg, id, &text, runner_idx) {
            Ok(()) => {
                ctx.data.config_edit_error = None;
            }
            Err(e) => {
                ctx.data.config_edit_error = Some(e);
                self.edit_buffer = Some(buf);
                self.editing_field = Some(id);
            }
        }
    }

    fn render_runner_list(&self, area: Rect, buf: &mut Buffer, data: &AppData) {
        let runners: Vec<&crate::ipc::protocol::RunnerStatusSnapshot> = data
            .status
            .as_ref()
            .map(|s| s.runners.iter().collect())
            .unwrap_or_default();
        if runners.is_empty() {
            let p = Paragraph::new(vec![Line::from(Span::styled(
                "Daemon up but no runners reported yet — check the General tab.",
                Style::default().fg(Color::DarkGray),
            ))])
            .block(Block::default().borders(Borders::ALL).title(" Runners "))
            .wrap(Wrap { trim: true });
            p.render(area, buf);
            return;
        }
        let picked_idx = self.list.selected_index().unwrap_or(0).min(runners.len() - 1);
        let items: Vec<ListItem<'_>> = runners
            .iter()
            .enumerate()
            .map(|(i, r)| {
                let project = r.project_slug.as_deref().unwrap_or("(no project)");
                let approvals = if r.approvals_pending > 0 {
                    format!("approvals={}", r.approvals_pending)
                } else {
                    String::new()
                };
                let prefix = if i == picked_idx { "▶ " } else { "  " };
                let line = Line::from(vec![
                    Span::styled(prefix.to_string(), Style::default().fg(Color::Cyan)),
                    Span::styled(
                        format!("{:<24} ", r.name),
                        Style::default()
                            .fg(Color::White)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        format!("project={:<14} ", project),
                        Style::default().fg(Color::Cyan),
                    ),
                    Span::styled(
                        format!("{:<10} ", format!("{:?}", r.status)),
                        match r.status {
                            crate::cloud::protocol::RunnerStatus::Idle => {
                                Style::default().fg(Color::Green)
                            }
                            crate::cloud::protocol::RunnerStatus::Busy => {
                                Style::default().fg(Color::Yellow)
                            }
                            crate::cloud::protocol::RunnerStatus::Reconnecting
                            | crate::cloud::protocol::RunnerStatus::AwaitingReauth => {
                                Style::default().fg(Color::DarkGray)
                            }
                        },
                    ),
                    Span::styled(approvals, Style::default().fg(Color::Yellow)),
                ]);
                ListItem::new(line)
            })
            .collect();
        let total = runners.len();
        let title = format!(" Runners ({}/{}) ", picked_idx + 1, total);
        let focused = matches!(self.pane, Pane::List);
        let block_style = if focused {
            Style::default().fg(Color::Yellow)
        } else {
            Style::default()
        };
        let list = List::new(items)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(block_style)
                    .title(title),
            )
            .highlight_style(Style::default().add_modifier(Modifier::REVERSED));
        let mut lstate = ListState::default();
        lstate.select(Some(picked_idx));
        StatefulWidget::render(list, area, buf, &mut lstate);
    }

    fn render_settings_panel(&self, area: Rect, buf: &mut Buffer, data: &AppData) {
        let Some(working) = data.config_working.as_ref() else {
            return;
        };
        if working.runners.is_empty() {
            let placeholder = Paragraph::new(vec![
                Line::raw(""),
                Line::from(Span::styled(
                    "No selected runner",
                    Style::default().add_modifier(Modifier::BOLD),
                )),
                Line::raw(""),
                Line::from(Span::styled(
                    "Press [a] to register a runner under this connection.",
                    Style::default().add_modifier(Modifier::DIM),
                )),
            ])
            .alignment(Alignment::Center)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(" Settings (selected runner) "),
            )
            .wrap(Wrap { trim: false });
            placeholder.render(area, buf);
            return;
        }
        let loaded = data.config_loaded.clone();
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(0), Constraint::Length(5)])
            .split(area);
        let dirty = loaded
            .as_ref()
            .map(|l| fields::differs(l, working))
            .unwrap_or(true);
        let title = if dirty {
            " Settings (selected runner) [unsaved] "
        } else {
            " Settings (selected runner) "
        };
        let focused = matches!(self.pane, Pane::Settings);
        let block_style = if focused {
            Style::default().fg(Color::Yellow)
        } else {
            Style::default()
        };
        let edit_buffer_str: Option<String> =
            self.edit_buffer.as_ref().map(|t| t.text().to_string());
        let p = Paragraph::new(fields::editable_lines(
            working,
            &loaded,
            self.field_idx,
            data.runner_picker_idx,
            edit_buffer_str.as_deref(),
        ))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(block_style)
                .title(title),
        )
        .wrap(Wrap { trim: false });
        p.render(chunks[0], buf);
        let footer = fields::footer(
            self.edit_buffer.is_some(),
            data.config_edit_error.as_deref(),
            data.reload_outcome.as_ref(),
        );
        footer.render(chunks[1], buf);
    }
}

fn render_unregistered_placeholder(area: Rect, buf: &mut Buffer) {
    let lines = vec![
        Line::from(Span::styled(
            "No runners configured yet.",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )),
        Line::raw(""),
        Line::from("Open the General tab (press [1]) to register this machine with the cloud."),
        Line::from("Once registered, runners will appear here."),
    ];
    let p = Paragraph::new(lines)
        .block(Block::default().borders(Borders::ALL).title(" Runners "))
        .wrap(Wrap { trim: true });
    p.render(area, buf);
}

fn hotkeys_card() -> Paragraph<'static> {
    Paragraph::new(Line::from(vec![
        Span::styled("[a] add", Style::default().fg(Color::Green)),
        Span::raw("   "),
        Span::styled("[d] remove", Style::default().fg(Color::Red)),
        Span::raw("   [Tab] switch card   [j/k ↑↓] move   [</>] runner   [↵] edit   [w] save   [r] refresh"),
    ]))
    .block(Block::default().borders(Borders::ALL).title(" Controls "))
}
