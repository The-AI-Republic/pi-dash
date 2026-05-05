//! Runners tab — list of every runner this daemon hosts plus the
//! per-runner settings panel for the highlighted runner.
//!
//! Four top-level focus cards on row 0:
//!
//!   `runner_list` (interactive) | `settings` (interactive, children = each
//!   editable field) | `live_state` (read-only) | `hotkeys` (read-only)
//!
//! At Layer 1 ←/→ cycles through the cards. ↑/↓ on `runner_list` moves
//! its internal cursor (and broadcasts `SelectRunner`); ↑/↓ on the other
//! cards is a no-op. Enter on `settings` dives to Layer 2 where each
//! field is an Item; Enter on a Bool/Enum field cycles in place; Enter
//! on a Text/U32 field opens an inline edit buffer (TextInput context).

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
use super::super::view::focus::{
    border_style, is_focused, is_in_path, FocusNode, FocusPath,
};
use super::super::view::tab::{Tab, TabCtx, TabKind};
use super::super::view::KeyHandled;
use super::super::widgets::{SelectableList, TextArea};
use super::config as fields;

const CARD_RUNNER_LIST: &str = "runner_list";
const CARD_SETTINGS: &str = "settings";
const CARD_LIVE_STATE: &str = "live_state";
const CARD_HOTKEYS: &str = "hotkeys";

pub struct RunnerStatusTab {
    list: SelectableList<String>,
    /// Inline edit buffer for Text/U32 fields (Layer 2 → text-input
    /// sub-state). Presence of this buffer flips the active context to
    /// `TextInput` so the dispatcher routes typing to us.
    edit_buffer: Option<TextArea>,
    /// Which field id we're editing — needed to commit back via
    /// `set_text_value` even after the cursor moves.
    editing_field: Option<fields::FieldId>,
}

impl RunnerStatusTab {
    pub fn new() -> Self {
        Self {
            list: SelectableList::new(),
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
    }

    pub fn selected_runner_name(&self, data: &AppData) -> Option<String> {
        if let Some(id) = self.list.selected_id() {
            return Some(id.clone());
        }
        let working = data.config_working.as_ref()?;
        working
            .runners
            .get(data.runner_picker_idx)
            .map(|r| r.name.clone())
    }

    /// Index of the field currently selected at Layer 2 (for the
    /// settings card). Falls back to 0 when the focus path doesn't
    /// resolve (e.g. between renders).
    fn focused_field_idx(&self, focus: &FocusPath) -> usize {
        focus
            .segments()
            .last()
            .and_then(|leaf| {
                fields::FIELDS
                    .iter()
                    .position(|spec| spec.id.id_str() == *leaf)
            })
            .unwrap_or(0)
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

    fn focus_tree(&self, data: &AppData) -> Vec<FocusNode> {
        let runners_present = data
            .config_working
            .as_ref()
            .map(|c| !c.runners.is_empty())
            .unwrap_or(false);
        let settings_children = if runners_present {
            fields::FIELDS
                .iter()
                .enumerate()
                .map(|(i, spec)| FocusNode::Item {
                    id: spec.id.id_str(),
                    interactive: true,
                    row: i,
                })
                .collect()
        } else {
            Vec::new()
        };
        vec![
            FocusNode::Card {
                id: CARD_RUNNER_LIST,
                interactive: true,
                row: 0,
                children: Vec::new(),
            },
            FocusNode::Card {
                id: CARD_SETTINGS,
                interactive: runners_present,
                row: 0,
                children: settings_children,
            },
            FocusNode::Card {
                id: CARD_LIVE_STATE,
                interactive: false,
                row: 0,
                children: Vec::new(),
            },
            FocusNode::Card {
                id: CARD_HOTKEYS,
                interactive: false,
                row: 0,
                children: Vec::new(),
            },
        ]
    }

    fn render(&self, area: Rect, buf: &mut Buffer, data: &AppData, focus: &FocusPath) {
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
        let right = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(8), Constraint::Length(11)])
            .split(body[1]);
        self.render_runner_list(body[0], buf, data, focus);
        self.render_settings_panel(right[0], buf, data, focus);
        render_live_state_panel(
            right[1],
            buf,
            data,
            self.selected_runner_name(data).as_deref(),
            is_focused(focus, CARD_LIVE_STATE),
        );
        hotkeys_card(is_focused(focus, CARD_HOTKEYS)).render(outer[1], buf);
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

        // Edit-buffer path: typing Text/U32 fields. The dispatcher only
        // routes here when active_contexts() is `[TextInput]`, so all
        // other key handlers are inert in this mode.
        if self.edit_buffer.is_some() {
            if let Some(buf) = self.edit_buffer.as_mut() {
                let h = buf.handle_key(key);
                if matches!(h, KeyHandled::Consumed) {
                    return KeyHandled::Consumed;
                }
            }
            return match key.code {
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
                _ => KeyHandled::Consumed,
            };
        }

        // List-cursor moves on the runner_list card (no sibling row
        // exists, so the dispatcher hands ↑/↓/j/k here).
        if focus.current() == Some(CARD_RUNNER_LIST) {
            let runner_ids: Vec<String> = match &ctx.data.status {
                Some(s) => s.runners.iter().map(|r| r.name.clone()).collect(),
                None => Vec::new(),
            };
            match key.code {
                KeyCode::Char('j') | KeyCode::Down => {
                    self.list.move_down(&runner_ids);
                    if let Some(idx) = self.list.selected_index() {
                        ctx.tx.send(AppEvent::SelectRunner(idx));
                    }
                    return KeyHandled::Consumed;
                }
                KeyCode::Char('k') | KeyCode::Up => {
                    self.list.move_up(&runner_ids);
                    if let Some(idx) = self.list.selected_index() {
                        ctx.tx.send(AppEvent::SelectRunner(idx));
                    }
                    return KeyHandled::Consumed;
                }
                _ => {}
            }
        }

        // Tab-wide hotkeys (work regardless of which card is focused).
        match (key.code, key.modifiers) {
            (KeyCode::Char('a'), m) if !m.contains(KeyModifiers::CONTROL) => {
                let v = super::modals::add_runner::AddRunnerView::open(ctx.data);
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
            (KeyCode::Char('w'), _) => {
                ctx.tx.send(AppEvent::SaveConfig);
                KeyHandled::Consumed
            }
            _ => KeyHandled::NotConsumed,
        }
    }

    fn activate_item(&mut self, item_id: super::super::view::CardId, ctx: &mut TabCtx<'_>) -> KeyHandled {
        // Only settings fields are activatable items in this tab.
        let Some(field_id) = fields::FieldId::from_id_str(item_id) else {
            return KeyHandled::NotConsumed;
        };
        let Some(cfg) = ctx.data.config_working.as_mut() else {
            return KeyHandled::NotConsumed;
        };
        ctx.data.config_edit_error = None;
        let runner_idx = ctx.data.runner_picker_idx;
        let spec = fields::FIELDS
            .iter()
            .find(|s| s.id == field_id)
            .expect("field id round-trips");
        match spec.kind {
            fields::FieldKind::Bool => fields::toggle_bool(cfg, field_id, runner_idx),
            fields::FieldKind::Enum(_) => fields::cycle_enum(cfg, field_id, runner_idx),
            fields::FieldKind::Text | fields::FieldKind::U32 => {
                let value = fields::display_value(cfg, field_id, runner_idx);
                self.edit_buffer = Some(TextArea::with_text(value));
                self.editing_field = Some(field_id);
            }
        }
        KeyHandled::Consumed
    }

    fn active_contexts(&self, focus: &FocusPath) -> Vec<Context> {
        if self.edit_buffer.is_some() {
            return vec![Context::TextInput];
        }
        // Keep the RunnerPicker (`<` / `>` / Alt+1..9) keys live anywhere
        // on this tab so the user can switch runners without going
        // back to Layer 0.
        let _ = focus;
        vec![Context::List]
    }

    fn on_focus(&mut self, ctx: &mut TabCtx<'_>) {
        self.reconcile(ctx.data);
    }
}

impl RunnerStatusTab {
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

    fn render_runner_list(&self, area: Rect, buf: &mut Buffer, data: &AppData, focus: &FocusPath) {
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
        let list = List::new(items)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(border_style(is_focused(focus, CARD_RUNNER_LIST)))
                    .title(title),
            )
            .highlight_style(Style::default().add_modifier(Modifier::REVERSED));
        let mut lstate = ListState::default();
        lstate.select(Some(picked_idx));
        StatefulWidget::render(list, area, buf, &mut lstate);
    }

    fn render_settings_panel(&self, area: Rect, buf: &mut Buffer, data: &AppData, focus: &FocusPath) {
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
        let edit_buffer_str: Option<String> =
            self.edit_buffer.as_ref().map(|t| t.text().to_string());
        let p = Paragraph::new(fields::editable_lines(
            working,
            &loaded,
            self.focused_field_idx(focus),
            data.runner_picker_idx,
            edit_buffer_str.as_deref(),
        ))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(border_style(is_in_path(focus, CARD_SETTINGS)))
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

fn hotkeys_card(focused: bool) -> Paragraph<'static> {
    Paragraph::new(Line::from(vec![
        Span::styled("[a] add", Style::default().fg(Color::Green)),
        Span::raw("   "),
        Span::styled("[d] remove", Style::default().fg(Color::Red)),
        Span::raw("   ←/→ card   ↑/↓ list   ↵ edit   [w] save   [r] refresh"),
    ]))
    .block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(border_style(focused))
            .title(" Controls "),
    )
}

fn render_live_state_panel(
    area: Rect,
    buf: &mut Buffer,
    data: &AppData,
    selected_name: Option<&str>,
    focused: bool,
) {
    let snapshot = selected_name
        .and_then(|n| data.status.as_ref().and_then(|s| s.runner_by_name(n)));
    let mut lines: Vec<Line<'static>> = Vec::new();
    let Some(r) = snapshot else {
        lines.push(Line::from(Span::styled(
            "(no live state — daemon idle or runner not yet reported)",
            Style::default().fg(Color::DarkGray),
        )));
        Paragraph::new(lines)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(border_style(focused))
                    .title(" Live state "),
            )
            .wrap(Wrap { trim: true })
            .render(area, buf);
        return;
    };
    let pod = r.pod_id.map(|p| p.to_string()).unwrap_or_else(|| "(unassigned)".into());
    lines.push(field_kv("Pod", &pod));
    lines.push(field_kv("Heartbeat", &fmt_age(r.last_heartbeat)));
    let current_run = match &r.current_run {
        Some(run) => format!("{} ({}) · {} events", run.run_id, run.status, run.events),
        None => "(idle)".to_string(),
    };
    lines.push(field_kv("Current run", &current_run));
    if let Some(obs) = r.observability.as_ref() {
        let last_event = match (&obs.last_event_kind, obs.last_event_at) {
            (Some(k), Some(at)) => format!("{k}  ({})", fmt_age(Some(at))),
            _ => "(none)".into(),
        };
        lines.push(field_kv("Last event", &last_event));
        if let Some(s) = obs.last_event_summary.as_deref() {
            lines.push(Line::from(vec![
                Span::styled("            ", Style::default()),
                Span::styled(s.to_string(), Style::default().fg(Color::DarkGray)),
            ]));
        }
        let turns = obs
            .turn_count
            .map(|n| n.to_string())
            .unwrap_or_else(|| "—".into());
        let tokens = obs
            .tokens
            .as_ref()
            .map(|t| format!("in={} out={} total={}", t.input, t.output, t.total))
            .unwrap_or_else(|| "—".into());
        lines.push(field_kv("Turns", &turns));
        lines.push(field_kv("Tokens", &tokens));
        let agent = match (obs.agent_pid, obs.agent_subprocess_alive) {
            (Some(pid), Some(true)) => format!("pid {pid} · alive"),
            (Some(pid), Some(false)) => format!("pid {pid} · exited"),
            (Some(pid), None) => format!("pid {pid}"),
            _ => "—".into(),
        };
        lines.push(field_kv("Agent", &agent));
    } else {
        lines.push(Line::from(Span::styled(
            "(observability disabled — set agent_observability_v1=true to surface tokens/turns/last event)",
            Style::default().fg(Color::DarkGray),
        )));
    }
    Paragraph::new(lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(border_style(focused))
                .title(" Live state "),
        )
        .wrap(Wrap { trim: true })
        .render(area, buf);
}

fn field_kv(label: &str, value: &str) -> Line<'static> {
    Line::from(vec![
        Span::styled(
            format!("{label:<11} "),
            Style::default().fg(Color::Cyan),
        ),
        Span::raw(value.to_string()),
    ])
}

fn fmt_age(ts: Option<chrono::DateTime<chrono::Utc>>) -> String {
    match ts {
        None => "(none)".into(),
        Some(at) => {
            let secs = (chrono::Utc::now() - at).num_seconds();
            if secs < 0 {
                "in the future?".into()
            } else if secs < 60 {
                format!("{secs}s ago")
            } else if secs < 3600 {
                format!("{}m {}s ago", secs / 60, secs % 60)
            } else {
                format!("{}h{}m ago", secs / 3600, (secs % 3600) / 60)
            }
        }
    }
}
