//! General tab — daemon-level surface.
//!
//! Focus tree (configured machine):
//!
//!   row 0: `service` (read-only)
//!   row 1: `connection` (read-only)
//!   row 2: `daemon_settings` (interactive, children = log_level + log_retention items)
//!   row 3: `hotkeys` (read-only)
//!
//! Fresh machine (no config):
//!
//!   row 0: `register` (interactive, children = cloud_url, token, host_label, submit)
//!
//! Tab-wide hotkeys (`[s]`/`[x]` service, `[w]` save) work whenever
//! focus is anywhere on this tab and no edit buffer is open.

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::buffer::Buffer;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Widget, Wrap};

use super::super::app::AppData;
use super::super::event::AppEvent;
use super::super::input::keymap::Context;
use super::super::view::focus::{
    border_style, dived_marker, is_focused, is_in_path, FocusNode, FocusPath,
};
use super::super::view::tab::{Tab, TabCtx, TabKind};
use super::super::view::{CardId, KeyHandled};
use super::super::widgets::TextArea;
use super::config as fields;

const CARD_SERVICE: &str = "service";
const CARD_CONNECTION: &str = "connection";
const CARD_DAEMON_SETTINGS: &str = "daemon_settings";
const CARD_HOTKEYS: &str = "hotkeys";
const ITEM_LOG_LEVEL: &str = "field:log_level";
const ITEM_LOG_RETENTION: &str = "field:log_retention_days";
const ITEM_AUTO_UPDATE: &str = "field:auto_update";

const CARD_REGISTER: &str = "register";
const ITEM_REG_CLOUD_URL: &str = "register:cloud_url";
const ITEM_REG_TOKEN: &str = "register:token";
const ITEM_REG_HOST_LABEL: &str = "register:host_label";
const ITEM_REG_SUBMIT: &str = "register:submit";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegisterFocus {
    CloudUrl = 0,
    Token = 1,
    HostLabel = 2,
    Submit = 3,
}

pub struct RegisterForm {
    pub cloud_url: TextArea,
    pub token: TextArea,
    pub host_label: TextArea,
    pub busy: bool,
    pub error: Option<String>,
}

impl RegisterForm {
    pub fn new(default_host: String) -> Self {
        Self {
            cloud_url: TextArea::with_text("http://localhost"),
            token: TextArea::new().masked(true).placeholder("paste enrollment token"),
            host_label: TextArea::with_text(default_host),
            busy: false,
            error: None,
        }
    }

    pub fn focused_textarea_mut(&mut self, focus: RegisterFocus) -> Option<&mut TextArea> {
        match focus {
            RegisterFocus::CloudUrl => Some(&mut self.cloud_url),
            RegisterFocus::Token => Some(&mut self.token),
            RegisterFocus::HostLabel => Some(&mut self.host_label),
            RegisterFocus::Submit => None,
        }
    }
}

#[derive(Clone)]
pub struct RegisterFormSnapshot {
    pub cloud_url: String,
    pub token: String,
    pub host_label: String,
}

pub struct GeneralTab {
    /// Inline edit-text for `log_retention_days` (only daemon-settings
    /// field that opens a buffer; `LogLevel` cycles in place).
    edit_buffer: Option<TextArea>,
    /// Lazily seeded the first time we observe `config` is missing.
    register: Option<RegisterForm>,
}

impl GeneralTab {
    pub fn new() -> Self {
        Self {
            edit_buffer: None,
            register: None,
        }
    }

    pub fn on_config_present(&mut self, _data: &AppData) {
        self.register = None;
    }

    pub fn on_config_missing(&mut self, _data: &AppData) {
        if self.register.is_none() {
            self.register = Some(RegisterForm::new(default_hostname()));
        }
    }

    pub fn register_form_snapshot(&self) -> Option<RegisterFormSnapshot> {
        self.register.as_ref().map(|r| RegisterFormSnapshot {
            cloud_url: r.cloud_url.text().to_string(),
            token: r.token.text().to_string(),
            host_label: r.host_label.text().to_string(),
        })
    }

    pub fn set_register_busy(&mut self, busy: bool, err: Option<String>) {
        if let Some(r) = self.register.as_mut() {
            r.busy = busy;
            if err.is_some() {
                r.error = err;
            }
        }
    }

    fn register_focus_for_item(item: CardId) -> Option<RegisterFocus> {
        Some(match item {
            ITEM_REG_CLOUD_URL => RegisterFocus::CloudUrl,
            ITEM_REG_TOKEN => RegisterFocus::Token,
            ITEM_REG_HOST_LABEL => RegisterFocus::HostLabel,
            ITEM_REG_SUBMIT => RegisterFocus::Submit,
            _ => return None,
        })
    }
}

impl Default for GeneralTab {
    fn default() -> Self {
        Self::new()
    }
}

impl Tab for GeneralTab {
    fn kind(&self) -> TabKind {
        TabKind::General
    }

    fn focus_tree(&self, data: &AppData) -> Vec<FocusNode> {
        if data.config_working.is_none() {
            return vec![FocusNode::Card {
                id: CARD_REGISTER,
                interactive: true,
                row: 0,
                children: vec![
                    FocusNode::Item {
                        id: ITEM_REG_CLOUD_URL,
                        interactive: true,
                        row: 0,
                    },
                    FocusNode::Item {
                        id: ITEM_REG_TOKEN,
                        interactive: true,
                        row: 1,
                    },
                    FocusNode::Item {
                        id: ITEM_REG_HOST_LABEL,
                        interactive: true,
                        row: 2,
                    },
                    FocusNode::Item {
                        id: ITEM_REG_SUBMIT,
                        interactive: true,
                        row: 3,
                    },
                ],
            }];
        }
        vec![
            FocusNode::Card {
                id: CARD_SERVICE,
                interactive: false,
                row: 0,
                children: Vec::new(),
            },
            FocusNode::Card {
                id: CARD_CONNECTION,
                interactive: false,
                row: 1,
                children: Vec::new(),
            },
            FocusNode::Card {
                id: CARD_DAEMON_SETTINGS,
                interactive: true,
                row: 2,
                children: vec![
                    FocusNode::Item {
                        id: ITEM_LOG_LEVEL,
                        interactive: true,
                        row: 0,
                    },
                    FocusNode::Item {
                        id: ITEM_LOG_RETENTION,
                        interactive: true,
                        row: 1,
                    },
                    FocusNode::Item {
                        id: ITEM_AUTO_UPDATE,
                        interactive: true,
                        row: 2,
                    },
                ],
            },
            FocusNode::Card {
                id: CARD_HOTKEYS,
                interactive: false,
                row: 3,
                children: Vec::new(),
            },
        ]
    }

    fn render(&self, area: Rect, buf: &mut Buffer, data: &AppData, focus: &FocusPath) {
        if data.config_working.is_none() {
            self.render_register(area, buf, focus);
            return;
        }
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(5),
                Constraint::Length(7),
                Constraint::Min(0),
                Constraint::Length(3),
            ])
            .split(area);
        service_card(data, is_focused(focus, CARD_SERVICE)).render(chunks[0], buf);
        connection_card(data, is_focused(focus, CARD_CONNECTION)).render(chunks[1], buf);
        self.daemon_settings_card(data, focus).render(chunks[2], buf);
        hotkeys_card(data, is_focused(focus, CARD_HOTKEYS)).render(chunks[3], buf);
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

        // Register-form text input: when focus is on one of the four
        // register items and the active context is `TextInput`, route
        // typing to the corresponding TextArea. Submit Enter routes
        // SubmitRegister; Esc clears the form error.
        if let Some(reg) = self.register.as_mut()
            && let Some(reg_focus) = focus
                .current()
                .and_then(Self::register_focus_for_item)
        {
            {
                if let Some(area) = reg.focused_textarea_mut(reg_focus) {
                    let h = area.handle_key(key);
                    if matches!(h, KeyHandled::Consumed) {
                        return KeyHandled::Consumed;
                    }
                }
                if key.code == KeyCode::Char('c')
                    && key.modifiers.contains(KeyModifiers::CONTROL)
                {
                    return KeyHandled::NotConsumed;
                }
                return match (key.code, reg_focus) {
                    (KeyCode::Enter, RegisterFocus::Submit) => {
                        ctx.tx.send(AppEvent::SubmitRegister);
                        KeyHandled::Consumed
                    }
                    (KeyCode::Esc, _) => {
                        reg.error = None;
                        KeyHandled::Consumed
                    }
                    _ => KeyHandled::Consumed,
                };
            }
        }

        // Daemon-settings edit-buffer path (LogRetentionDays).
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
                    ctx.data.config_edit_error = None;
                    KeyHandled::Consumed
                }
                KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    KeyHandled::NotConsumed
                }
                _ => KeyHandled::Consumed,
            };
        }

        // Tab-wide hotkeys (only when no edit buffer is open).
        match (key.code, key.modifiers) {
            (KeyCode::Char('s'), _) => {
                ctx.tx.send(AppEvent::ServiceStart);
                KeyHandled::Consumed
            }
            (KeyCode::Char('x'), _) => {
                ctx.tx.send(AppEvent::ServiceStop);
                KeyHandled::Consumed
            }
            (KeyCode::Char('w'), _) => {
                ctx.tx.send(AppEvent::SaveConfig);
                KeyHandled::Consumed
            }
            _ => KeyHandled::NotConsumed,
        }
    }

    fn activate_item(&mut self, item_id: CardId, ctx: &mut TabCtx<'_>) -> KeyHandled {
        match item_id {
            ITEM_LOG_LEVEL => {
                let Some(cfg) = ctx.data.config_working.as_mut() else {
                    return KeyHandled::NotConsumed;
                };
                let cur = fields::LOG_LEVELS
                    .iter()
                    .position(|s| *s == cfg.daemon.log_level)
                    .unwrap_or(2);
                cfg.daemon.log_level =
                    fields::LOG_LEVELS[(cur + 1) % fields::LOG_LEVELS.len()].to_string();
                KeyHandled::Consumed
            }
            ITEM_LOG_RETENTION => {
                let Some(cfg) = ctx.data.config_working.as_ref() else {
                    return KeyHandled::NotConsumed;
                };
                self.edit_buffer = Some(TextArea::with_text(
                    cfg.daemon.log_retention_days.to_string(),
                ));
                KeyHandled::Consumed
            }
            ITEM_AUTO_UPDATE => {
                let Some(cfg) = ctx.data.config_working.as_mut() else {
                    return KeyHandled::NotConsumed;
                };
                cfg.daemon.auto_update = !cfg.daemon.auto_update;
                KeyHandled::Consumed
            }
            ITEM_REG_SUBMIT => {
                ctx.tx.send(AppEvent::SubmitRegister);
                KeyHandled::Consumed
            }
            // Activating a register text-field item dives the user into
            // text-input mode; nothing to do here besides claim the
            // event so the dispatcher doesn't fall through.
            ITEM_REG_CLOUD_URL | ITEM_REG_TOKEN | ITEM_REG_HOST_LABEL => KeyHandled::Consumed,
            _ => KeyHandled::NotConsumed,
        }
    }

    fn handle_paste(&mut self, text: String, _ctx: &mut TabCtx<'_>) -> KeyHandled {
        if let Some(buf) = self.edit_buffer.as_mut() {
            buf.insert_str(&text);
            return KeyHandled::Consumed;
        }
        // Pasting into the register form is best-effort: we don't know
        // which field is focused without the FocusPath, so paste lands
        // on whichever TextArea is currently the leaf via insert. The
        // GeneralTab drops the paste otherwise.
        KeyHandled::NotConsumed
    }

    fn active_contexts(&self, focus: &FocusPath) -> Vec<Context> {
        // Edit buffer open OR focus is on a register text-field item
        // → TextInput context. Otherwise no special context (App
        // appends Tabs + Global).
        if self.edit_buffer.is_some() {
            return vec![Context::TextInput];
        }
        if matches!(
            focus.current(),
            Some(ITEM_REG_CLOUD_URL) | Some(ITEM_REG_TOKEN) | Some(ITEM_REG_HOST_LABEL)
        ) {
            return vec![Context::TextInput];
        }
        Vec::new()
    }
}

impl GeneralTab {
    fn commit_edit_buffer(&mut self, ctx: &mut TabCtx<'_>) {
        let Some(buf) = self.edit_buffer.take() else {
            return;
        };
        let text = buf.text().to_string();
        let Some(cfg) = ctx.data.config_working.as_mut() else {
            return;
        };
        match fields::set_text_value(cfg, fields::FieldId::LogRetentionDays, &text, ctx.data.runner_picker_idx) {
            Ok(()) => {
                ctx.data.config_edit_error = None;
            }
            Err(e) => {
                ctx.data.config_edit_error = Some(e);
                self.edit_buffer = Some(buf);
            }
        }
    }

    fn render_register(&self, area: Rect, buf: &mut Buffer, focus: &FocusPath) {
        let Some(reg) = self.register.as_ref() else {
            let p = Paragraph::new(Line::from(Span::styled(
                "Loading…",
                Style::default().add_modifier(Modifier::DIM),
            )))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(" Register with cloud "),
            );
            p.render(area, buf);
            return;
        };
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(0), Constraint::Length(3)])
            .split(area);

        let cur = focus.current();
        let mut lines: Vec<Line<'static>> = vec![
            Line::from(Span::styled(
                "This dev machine isn't enrolled yet.",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            )),
            Line::from("Fill in the form below and press [Connect] to enroll."),
            Line::raw(""),
        ];
        lines.push(form_line(
            "Cloud URL       ",
            reg.cloud_url.text(),
            cur == Some(ITEM_REG_CLOUD_URL),
        ));
        lines.push(form_line(
            "Enrollment token",
            &mask_token(reg.token.text()),
            cur == Some(ITEM_REG_TOKEN),
        ));
        lines.push(form_line(
            "Host label      ",
            reg.host_label.text(),
            cur == Some(ITEM_REG_HOST_LABEL),
        ));
        lines.push(Line::raw(""));
        lines.push(submit_line(cur == Some(ITEM_REG_SUBMIT), reg.busy));
        lines.push(Line::raw(""));
        lines.push(Line::from(Span::styled(
            "←/→/↑/↓ navigate   ↵ enter / submit   Esc back",
            Style::default().add_modifier(Modifier::DIM),
        )));
        lines.push(Line::from(Span::styled(
            "Get an enrollment token from the Pi Dash web UI: Workspace → Runners → Add connection",
            Style::default().add_modifier(Modifier::DIM),
        )));
        if let Some(e) = &reg.error {
            lines.push(Line::raw(""));
            lines.push(Line::from(Span::styled(
                e.clone(),
                Style::default().fg(Color::Red),
            )));
        }
        if reg.busy {
            lines.push(Line::raw(""));
            lines.push(Line::from(Span::styled(
                "contacting cloud…",
                Style::default().fg(Color::Yellow),
            )));
        }
        let body = Paragraph::new(lines)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(border_style(is_in_path(focus, CARD_REGISTER)))
                    .title(format!(
                        " Register with cloud {}",
                        dived_marker(focus, CARD_REGISTER)
                    )),
            )
            .wrap(Wrap { trim: false });
        body.render(chunks[0], buf);

        let footer = Paragraph::new(Line::from(vec![Span::styled(
            "Tab/↑↓ move field   ↵ advance / submit   Esc clears form error",
            Style::default().add_modifier(Modifier::DIM),
        )]))
        .block(Block::default().borders(Borders::ALL).title(" Controls "));
        footer.render(chunks[1], buf);
    }

    fn daemon_settings_card<'a>(&self, data: &'a AppData, focus: &FocusPath) -> Paragraph<'a> {
        let Some(cfg) = data.config_working.as_ref() else {
            return Paragraph::new("(no config loaded)").block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(" Daemon settings "),
            );
        };
        let cur = focus.current();
        let editing = cur == Some(ITEM_LOG_RETENTION) && self.edit_buffer.is_some();
        let log_level_focused = cur == Some(ITEM_LOG_LEVEL);
        let log_retention_focused = cur == Some(ITEM_LOG_RETENTION);
        let auto_update_focused = cur == Some(ITEM_AUTO_UPDATE);
        let log_level_style = if log_level_focused {
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::Gray)
        };
        let retention_value = if editing {
            format!("{}▊", self.edit_buffer.as_ref().map(|b| b.text()).unwrap_or(""))
        } else {
            cfg.daemon.log_retention_days.to_string()
        };
        let retention_style = if log_retention_focused {
            if editing {
                Style::default().fg(Color::Yellow)
            } else {
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD)
            }
        } else {
            Style::default().fg(Color::Gray)
        };
        let marker = |on: bool| if on { "▶" } else { " " };
        let mut lines: Vec<Line<'_>> = Vec::new();
        lines.push(Line::from(vec![
            Span::styled(
                format!(" {} log_level         ", marker(log_level_focused)),
                Style::default().fg(Color::Cyan),
            ),
            Span::styled(cfg.daemon.log_level.clone(), log_level_style),
            if log_level_focused {
                Span::styled("   [Enter cycles]".to_string(), Style::default().add_modifier(Modifier::DIM))
            } else {
                Span::raw("")
            },
        ]));
        lines.push(Line::from(vec![
            Span::styled(
                format!(" {} log_retention_days ", marker(log_retention_focused)),
                Style::default().fg(Color::Cyan),
            ),
            Span::styled(retention_value, retention_style),
            if log_retention_focused && !editing {
                Span::styled("   [Enter edits]".to_string(), Style::default().add_modifier(Modifier::DIM))
            } else {
                Span::raw("")
            },
        ]));
        let auto_update_value = if cfg.daemon.auto_update { "on" } else { "off" };
        let auto_update_style = if auto_update_focused {
            Style::default()
                .fg(if cfg.daemon.auto_update {
                    Color::Green
                } else {
                    Color::Yellow
                })
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(if cfg.daemon.auto_update {
                Color::Green
            } else {
                Color::Gray
            })
        };
        lines.push(Line::from(vec![
            Span::styled(
                format!(" {} auto_update         ", marker(auto_update_focused)),
                Style::default().fg(Color::Cyan),
            ),
            Span::styled(auto_update_value.to_string(), auto_update_style),
            if auto_update_focused {
                Span::styled(
                    "   [Enter toggles]".to_string(),
                    Style::default().add_modifier(Modifier::DIM),
                )
            } else {
                Span::raw("")
            },
        ]));
        lines.push(Line::raw(""));
        lines.push(Line::from(Span::styled(
            "←/→ between cards   ↑/↓ between fields   ↵ edit/cycle   [w] save",
            Style::default().add_modifier(Modifier::DIM),
        )));
        if let Some(e) = &data.config_edit_error {
            lines.push(Line::from(Span::styled(
                e.clone(),
                Style::default().fg(Color::Red),
            )));
        }
        if let Some(out) = &data.reload_outcome {
            let style = if out.ok {
                Style::default().fg(Color::Green)
            } else {
                Style::default().fg(Color::Red)
            };
            let mark = if out.ok { "✓" } else { "✗" };
            lines.push(Line::from(Span::styled(
                format!("{mark} {}", out.summary),
                style,
            )));
        }
        Paragraph::new(lines)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(border_style(is_in_path(focus, CARD_DAEMON_SETTINGS)))
                    .title(format!(
                        " Daemon settings {}",
                        dived_marker(focus, CARD_DAEMON_SETTINGS)
                    )),
            )
            .wrap(Wrap { trim: true })
    }
}

fn service_card(data: &AppData, focused: bool) -> Paragraph<'_> {
    let raw = data.service_state.as_deref().unwrap_or("unknown");
    let (label, color) = match raw {
        "active" => ("● Running".to_string(), Color::Green),
        "activating" | "reloading" => ("◐ Starting".to_string(), Color::Yellow),
        "inactive" | "dead" => ("○ Stopped".to_string(), Color::DarkGray),
        "failed" => ("✗ Failed".to_string(), Color::Red),
        other if other.starts_with("error:") => (format!("? {other}"), Color::Red),
        other => (format!("● {other}"), Color::Yellow),
    };
    let mut lines = vec![
        Line::from(Span::styled(
            label,
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        )),
        Line::from(format!("service state: {raw}")),
    ];
    if let Some(msg) = &data.service_action_msg {
        lines.push(Line::from(Span::styled(
            msg.clone(),
            Style::default().fg(Color::Yellow),
        )));
    }
    Paragraph::new(lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(border_style(focused))
                .title(" Runner service "),
        )
        .wrap(Wrap { trim: true })
}

fn connection_card(data: &AppData, focused: bool) -> Paragraph<'_> {
    let lines: Vec<Line<'_>> = match &data.status {
        Some(s) => {
            let mut v = vec![
                Line::from(vec![Span::styled(
                    if s.daemon.connected {
                        "● Cloud connected"
                    } else {
                        "○ Cloud offline"
                    },
                    Style::default()
                        .fg(if s.daemon.connected {
                            Color::Green
                        } else {
                            Color::Red
                        })
                        .add_modifier(Modifier::BOLD),
                )]),
                Line::from(format!("Cloud URL: {}", s.daemon.cloud_url)),
                Line::from(format!("Uptime:    {}s", s.daemon.uptime_secs)),
                Line::from(format!("Runners:   {} configured", s.runners.len(),)),
            ];
            if let Some(advisory) = &s.daemon.update
                && let Some(line) = update_advisory_line(advisory)
            {
                v.push(line);
            }
            v
        }
        None => vec![Line::from(Span::styled(
            "Daemon IPC unreachable.",
            Style::default().fg(Color::DarkGray),
        ))],
    };
    Paragraph::new(lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(border_style(focused))
                .title(" Connection "),
        )
        .wrap(Wrap { trim: true })
}

/// Render the auto-update advisory matrix from `runner/RELEASING.md`:
/// red banner for `min_required` violation, yellow "restart to apply"
/// when an auto-swap has already landed a newer binary on disk, yellow
/// "update available" otherwise. Returns `None` when there's nothing
/// to surface (running version already meets both fields).
fn update_advisory_line(adv: &crate::ipc::protocol::UpdateAdvisory) -> Option<Line<'static>> {
    use crate::ipc::protocol::version_lt;
    let running = adv.running_version.as_str();
    if let Some(min) = adv.min_required.as_deref()
        && version_lt(running, min)
    {
        let on_disk = adv.on_disk_version.as_deref().unwrap_or(running);
        // "restart to apply" only makes sense when the binary on disk
        // actually meets the floor. Before the swap has landed, the
        // user has nothing to restart onto.
        let msg = if !version_lt(on_disk, min) {
            format!("⛔ Update required: cloud floor v{min}; restart to apply")
        } else if adv.auto_update_enabled {
            format!("⛔ Update required: cloud floor v{min}; swap pending")
        } else {
            format!("⛔ Update required: cloud floor v{min} — run `pidash update --restart`")
        };
        return Some(Line::from(Span::styled(
            msg,
            Style::default()
                .fg(Color::Red)
                .add_modifier(Modifier::BOLD),
        )));
    }
    if let Some(latest) = adv.latest_announced.as_deref()
        && version_lt(running, latest)
    {
        let on_disk = adv.on_disk_version.as_deref().unwrap_or(running);
        let msg = if on_disk == latest {
            format!("⚠ Restart to apply v{latest} (running v{running})")
        } else if adv.auto_update_enabled {
            format!("⚠ Update v{latest} pending swap (running v{running})")
        } else {
            format!("⚠ Update v{latest} available — run `pidash update --restart`")
        };
        return Some(Line::from(Span::styled(
            msg,
            Style::default().fg(Color::Yellow),
        )));
    }
    None
}

fn hotkeys_card(data: &AppData, focused: bool) -> Paragraph<'_> {
    let active = matches!(data.service_state.as_deref(), Some("active"));
    let start_style = if active {
        Style::default().add_modifier(Modifier::DIM)
    } else {
        Style::default().fg(Color::Green)
    };
    let stop_style = if active {
        Style::default().fg(Color::Red)
    } else {
        Style::default().add_modifier(Modifier::DIM)
    };
    Paragraph::new(Line::from(vec![
        Span::styled("[s] start", start_style),
        Span::raw("   "),
        Span::styled("[x] stop", stop_style),
        Span::raw("   [r] refresh"),
    ]))
    .block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(border_style(focused))
            .title(" Controls "),
    )
}

fn form_line(label: &str, value: &str, focused: bool) -> Line<'static> {
    let marker = if focused { "▶" } else { " " };
    let cursor = if focused { "▊" } else { "" };
    let value_style = if focused {
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::White)
    };
    Line::from(vec![
        Span::styled(
            format!(" {marker} "),
            Style::default()
                .fg(if focused { Color::Cyan } else { Color::DarkGray })
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(format!("{label} ")),
        Span::styled(format!("{value}{cursor}"), value_style),
    ])
}

fn submit_line(focused: bool, busy: bool) -> Line<'static> {
    let label = if busy { " Connecting… " } else { " Connect " };
    let style = if focused {
        Style::default()
            .fg(Color::Black)
            .bg(Color::Green)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default()
            .fg(Color::Green)
            .add_modifier(Modifier::BOLD)
    };
    Line::from(vec![Span::raw("   "), Span::styled(label.to_string(), style)])
}

fn mask_token(raw: &str) -> String {
    if raw.len() <= 4 {
        "*".repeat(raw.len())
    } else {
        format!("{}…{}", &raw[..2], &raw[raw.len() - 2..])
    }
}

fn default_hostname() -> String {
    crate::util::hostname::default_hostname()
}

/// Submit the register form. Spawned by `App::spawn_register_submit`.
/// Returns either the persisted Config + reload outcome, or an error
/// string for `ConfigUpdated(Err)` to render.
pub async fn submit_register(
    paths: &crate::util::paths::Paths,
    snap: RegisterFormSnapshot,
) -> Result<(crate::config::schema::Config, crate::service::reload::ReloadOutcome), String> {
    let cloud_url = snap.cloud_url.trim().to_string();
    let token = snap.token.trim().to_string();
    let host_label = snap.host_label.trim().to_string();

    crate::cli::connect::validate_cloud_url(&cloud_url).map_err(|e| format!("{e}"))?;
    if token.is_empty() {
        return Err("enrollment token is required".into());
    }

    let transport = crate::cloud::http::SharedHttpTransport::new(cloud_url.clone())
        .map_err(|e| format!("transport: {e:#}"))?;
    let resp = crate::cloud::http::enroll_runner(&transport, &token, &host_label, None)
        .await
        .map_err(|e| format!("enroll failed: {e:#}"))?;

    let runner_paths = paths.for_runner(resp.runner_id);
    runner_paths
        .ensure()
        .map_err(|e| format!("creating runner dirs: {e:#}"))?;
    crate::cloud::http::write_runner_credentials(
        runner_paths.credentials_path(),
        crate::cloud::http::RunnerCredentials {
            runner_id: resp.runner_id,
            name: resp.runner_name.clone(),
            refresh_token: resp.refresh_token.clone(),
            refresh_token_generation: resp.refresh_token_generation,
        },
    )
    .await
    .map_err(|e| format!("writing runner credentials: {e:#}"))?;

    let working_dir = paths.runner_dir(resp.runner_id).join("workspace");
    let new_runner_block = crate::config::schema::RunnerConfig {
        name: resp.runner_name.clone(),
        runner_id: resp.runner_id,
        workspace_slug: Some(resp.workspace_slug.clone()),
        project_slug: Some(resp.project_identifier.clone()),
        pod_id: None,
        workspace: crate::config::schema::WorkspaceSection { working_dir },
        agent: Default::default(),
        codex: Default::default(),
        claude_code: Default::default(),
        approval_policy: Default::default(),
    };
    let cfg = crate::config::schema::Config {
        version: 2,
        daemon: crate::config::schema::DaemonConfig {
            cloud_url: cloud_url.clone(),
            log_level: "info".to_string(),
            log_retention_days: 14,
            agent_observability_v1: false,
            auto_update: true,
        },
        runners: vec![new_runner_block],
        cli: None,
    };
    crate::config::file::write_config(paths, &cfg).map_err(|e| format!("writing config.toml: {e:#}"))?;
    let creds = crate::config::schema::Credentials {
        connection_id: resp.runner_id,
        connection_secret: String::new(),
        connection_name: Some(resp.runner_name.clone()),
        api_token: None,
        issued_at: chrono::Utc::now(),
    };
    crate::config::file::write_credentials(paths, &creds)
        .map_err(|e| format!("writing credentials.toml: {e:#}"))?;

    let svc = crate::service::detect();
    svc.write_unit(paths)
        .await
        .map_err(|e| format!("writing service unit: {e:#}"))?;
    let outcome = crate::service::reload::restart_and_verify(paths).await;
    Ok((cfg, outcome))
}
