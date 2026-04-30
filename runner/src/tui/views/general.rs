//! General tab — daemon-level surface.
//!
//! Two faces:
//! 1. **Configured machine**: service card, cloud-connection card,
//!    daemon settings card (log level + log retention), hotkeys
//!    footer.
//! 2. **Fresh machine**: inline register form with three TextArea
//!    fields (cloud URL, enrollment token, host label) plus a
//!    Connect button.
//!
//! The register form is the bug-3 fix point: each field is a real
//! `TextArea` widget. While a textarea is focused, the tab returns
//! `Context::TextInput` *only* from `active_contexts()`, so the
//! keymap can never resolve digit / `h` / `l` keys as tab switches.

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::buffer::Buffer;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Widget, Wrap};

use super::super::app::AppData;
use super::super::event::AppEvent;
use super::super::input::keymap::Context;
use super::super::view::tab::{Tab, TabCtx, TabKind};
use super::super::view::KeyHandled;
use super::super::widgets::TextArea;
use super::config as fields;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GeneralField {
    #[default]
    LogLevel,
    LogRetentionDays,
}

impl GeneralField {
    pub fn next(self) -> Self {
        match self {
            GeneralField::LogLevel => GeneralField::LogRetentionDays,
            GeneralField::LogRetentionDays => GeneralField::LogLevel,
        }
    }
    pub fn prev(self) -> Self {
        self.next()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegisterFocus {
    CloudUrl = 0,
    Token = 1,
    HostLabel = 2,
    Submit = 3,
}

impl RegisterFocus {
    fn from_idx(i: u8) -> Self {
        match i {
            0 => Self::CloudUrl,
            1 => Self::Token,
            2 => Self::HostLabel,
            _ => Self::Submit,
        }
    }
    fn idx(self) -> u8 {
        self as u8
    }
}

pub struct RegisterForm {
    pub cloud_url: TextArea,
    pub token: TextArea,
    pub host_label: TextArea,
    pub focus: RegisterFocus,
    pub busy: bool,
    pub error: Option<String>,
}

impl RegisterForm {
    pub fn new(default_host: String) -> Self {
        Self {
            cloud_url: TextArea::with_text("http://localhost"),
            token: TextArea::new().masked(true).placeholder("paste enrollment token"),
            host_label: TextArea::with_text(default_host),
            focus: RegisterFocus::CloudUrl,
            busy: false,
            error: None,
        }
    }

    pub fn focused_textarea_mut(&mut self) -> Option<&mut TextArea> {
        match self.focus {
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
    /// Cursor on the daemon-settings card.
    field: GeneralField,
    /// Inline edit-text for `log_retention_days` (only field that
    /// opens a buffer; LogLevel cycles in place).
    edit_buffer: Option<TextArea>,
    /// Lazily seeded the first time we observe `config` is missing.
    register: Option<RegisterForm>,
}

impl GeneralTab {
    pub fn new() -> Self {
        Self {
            field: GeneralField::default(),
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

    fn editing_text_field(&self) -> bool {
        if self.edit_buffer.is_some() {
            return true;
        }
        if let Some(reg) = &self.register {
            return matches!(
                reg.focus,
                RegisterFocus::CloudUrl | RegisterFocus::Token | RegisterFocus::HostLabel
            );
        }
        false
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

    fn render(&self, area: Rect, buf: &mut Buffer, data: &AppData) {
        if data.config_working.is_none() {
            self.render_register(area, buf);
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
        service_card(data).render(chunks[0], buf);
        connection_card(data).render(chunks[1], buf);
        self.daemon_settings_card(data).render(chunks[2], buf);
        hotkeys_card(data).render(chunks[3], buf);
    }

    fn handle_key(&mut self, key: KeyEvent, ctx: &mut TabCtx<'_>) -> KeyHandled {
        // Filter releases (defense in depth).
        if key.kind != KeyEventKind::Press && key.kind != KeyEventKind::Repeat {
            return KeyHandled::Consumed;
        }

        // 1) Register form path — leaf-first: focused textarea sees the key.
        if ctx.data.config_working.is_none() {
            return self.handle_register_key(key, ctx);
        }

        // 2) Daemon settings edit-buffer path.
        if self.edit_buffer.is_some() {
            return self.handle_edit_buffer_key(key, ctx);
        }

        // 3) Daemon settings nav — Enter cycles/edits, j/k moves.
        match (key.code, key.modifiers) {
            (KeyCode::Char('j') | KeyCode::Down, _) => {
                self.field = self.field.next();
                KeyHandled::Consumed
            }
            (KeyCode::Char('k') | KeyCode::Up, _) => {
                self.field = self.field.prev();
                KeyHandled::Consumed
            }
            (KeyCode::Enter, _) => {
                self.start_or_apply_field(ctx);
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
            (KeyCode::Char('s'), _) => {
                ctx.tx.send(AppEvent::ServiceStart);
                KeyHandled::Consumed
            }
            (KeyCode::Char('x'), _) => {
                ctx.tx.send(AppEvent::ServiceStop);
                KeyHandled::Consumed
            }
            _ => KeyHandled::NotConsumed,
        }
    }

    fn handle_paste(&mut self, text: String, _ctx: &mut TabCtx<'_>) -> KeyHandled {
        if let Some(reg) = self.register.as_mut()
            && let Some(area) = reg.focused_textarea_mut()
        {
            area.insert_str(&text);
            return KeyHandled::Consumed;
        }
        if let Some(buf) = self.edit_buffer.as_mut() {
            buf.insert_str(&text);
            return KeyHandled::Consumed;
        }
        KeyHandled::NotConsumed
    }

    fn active_contexts(&self) -> Vec<Context> {
        if self.editing_text_field() {
            // Bug-3 fix: text input is the only active context. The
            // dispatcher won't append `Tabs` / `Global`, so digit/letter
            // keys can't resolve to tab switches.
            vec![Context::TextInput]
        } else {
            vec![]
        }
    }
}

impl GeneralTab {
    fn handle_register_key(&mut self, key: KeyEvent, ctx: &mut TabCtx<'_>) -> KeyHandled {
        let Some(reg) = self.register.as_mut() else {
            return KeyHandled::NotConsumed;
        };
        // Layer 2: focused textarea gets first look.
        if let Some(area) = reg.focused_textarea_mut() {
            let h = area.handle_key(key);
            if matches!(h, KeyHandled::Consumed) {
                return KeyHandled::Consumed;
            }
        }
        // Layer 3: form-level navigation + submit.
        match (key.code, key.modifiers) {
            (KeyCode::Up, _) | (KeyCode::BackTab, _) => {
                let i = reg.focus.idx();
                let n = 4u8;
                let next = if i == 0 { n - 1 } else { i - 1 };
                reg.focus = RegisterFocus::from_idx(next);
                KeyHandled::Consumed
            }
            (KeyCode::Down, _) | (KeyCode::Tab, _) => {
                let i = reg.focus.idx();
                let next = (i + 1) % 4;
                reg.focus = RegisterFocus::from_idx(next);
                KeyHandled::Consumed
            }
            (KeyCode::Enter, _) => {
                if matches!(reg.focus, RegisterFocus::Submit) {
                    ctx.tx.send(AppEvent::SubmitRegister);
                } else {
                    let i = reg.focus.idx();
                    let next = (i + 1) % 4;
                    reg.focus = RegisterFocus::from_idx(next);
                }
                KeyHandled::Consumed
            }
            (KeyCode::Esc, _) => {
                reg.error = None;
                KeyHandled::Consumed
            }
            // Ctrl+C must escape the form so the user can quit even mid-typing.
            (KeyCode::Char('c'), m) if m.contains(KeyModifiers::CONTROL) => {
                KeyHandled::NotConsumed
            }
            _ => {
                // Anything else while a text field is focused is consumed
                // so it cannot fall through to the global keymap. The
                // textarea already consumed printable chars above; this
                // catches Function keys, etc.
                if matches!(
                    reg.focus,
                    RegisterFocus::CloudUrl | RegisterFocus::Token | RegisterFocus::HostLabel
                ) {
                    KeyHandled::Consumed
                } else {
                    KeyHandled::NotConsumed
                }
            }
        }
    }

    fn handle_edit_buffer_key(&mut self, key: KeyEvent, ctx: &mut TabCtx<'_>) -> KeyHandled {
        // Leaf-first: textarea handles printable / cursor / backspace.
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
                ctx.data.config_edit_error = None;
                KeyHandled::Consumed
            }
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                KeyHandled::NotConsumed
            }
            _ => KeyHandled::Consumed,
        }
    }

    fn start_or_apply_field(&mut self, ctx: &mut TabCtx<'_>) {
        let Some(cfg) = ctx.data.config_working.as_mut() else {
            return;
        };
        ctx.data.config_edit_error = None;
        match self.field {
            GeneralField::LogLevel => {
                use super::config::LOG_LEVELS;
                let cur = LOG_LEVELS
                    .iter()
                    .position(|s| *s == cfg.daemon.log_level)
                    .unwrap_or(2);
                cfg.daemon.log_level = LOG_LEVELS[(cur + 1) % LOG_LEVELS.len()].to_string();
            }
            GeneralField::LogRetentionDays => {
                self.edit_buffer = Some(TextArea::with_text(cfg.daemon.log_retention_days.to_string()));
            }
        }
    }

    fn commit_edit_buffer(&mut self, ctx: &mut TabCtx<'_>) {
        let Some(buf) = self.edit_buffer.take() else {
            return;
        };
        let text = buf.text().to_string();
        let Some(cfg) = ctx.data.config_working.as_mut() else {
            return;
        };
        let id = match self.field {
            GeneralField::LogRetentionDays => fields::FieldId::LogRetentionDays,
            GeneralField::LogLevel => return,
        };
        match fields::set_text_value(cfg, id, &text, ctx.data.runner_picker_idx) {
            Ok(()) => {
                ctx.data.config_edit_error = None;
            }
            Err(e) => {
                ctx.data.config_edit_error = Some(e);
                self.edit_buffer = Some(buf);
            }
        }
    }

    fn render_register(&self, area: Rect, buf: &mut Buffer) {
        let Some(reg) = self.register.as_ref() else {
            // No form yet — first refresh will seed.
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

        // Compose lines for the explanatory body, then render textareas
        // in their reserved row(s).
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
        // Reserve 3 rows per editable field (label line + cursor row).
        // We'll render text by overdrawing a Paragraph with the value
        // already serialized, since textarea owns its own cursor; the
        // simplest approach is to inline the values into the paragraph
        // content, mark the focused one bold, and skip the textarea
        // widget render call. The textarea state still drives content.
        lines.push(form_line("Cloud URL       ", reg.cloud_url.text(), reg.focus == RegisterFocus::CloudUrl, false));
        lines.push(form_line(
            "Enrollment token",
            &mask_token(reg.token.text()),
            reg.focus == RegisterFocus::Token,
            false,
        ));
        lines.push(form_line("Host label      ", reg.host_label.text(), reg.focus == RegisterFocus::HostLabel, false));
        lines.push(Line::raw(""));
        lines.push(submit_line(reg.focus == RegisterFocus::Submit, reg.busy));
        lines.push(Line::raw(""));
        lines.push(Line::from(Span::styled(
            "Tab/↑↓ move   type to edit   ↵ advance / submit",
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
                    .title(" Register with cloud "),
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

    fn daemon_settings_card<'a>(&self, data: &'a AppData) -> Paragraph<'a> {
        let Some(cfg) = data.config_working.as_ref() else {
            return Paragraph::new("(no config loaded)").block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(" Daemon settings "),
            );
        };
        let editing = self.field == GeneralField::LogRetentionDays && self.edit_buffer.is_some();
        let log_level_style = if self.field == GeneralField::LogLevel {
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
        let retention_style = if self.field == GeneralField::LogRetentionDays {
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
                format!(" {} log_level         ", marker(self.field == GeneralField::LogLevel)),
                Style::default().fg(Color::Cyan),
            ),
            Span::styled(cfg.daemon.log_level.clone(), log_level_style),
            if self.field == GeneralField::LogLevel {
                Span::styled("   [Enter cycles]".to_string(), Style::default().add_modifier(Modifier::DIM))
            } else {
                Span::raw("")
            },
        ]));
        lines.push(Line::from(vec![
            Span::styled(
                format!(" {} log_retention_days ", marker(self.field == GeneralField::LogRetentionDays)),
                Style::default().fg(Color::Cyan),
            ),
            Span::styled(retention_value, retention_style),
            if self.field == GeneralField::LogRetentionDays && !editing {
                Span::styled("   [Enter edits]".to_string(), Style::default().add_modifier(Modifier::DIM))
            } else {
                Span::raw("")
            },
        ]));
        lines.push(Line::raw(""));
        lines.push(Line::from(Span::styled(
            "[j/k ↑↓] move   [Enter] edit/cycle   [w] save+reload   [Esc] discard",
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
                    .title(" Daemon settings "),
            )
            .wrap(Wrap { trim: true })
    }
}

fn service_card(data: &AppData) -> Paragraph<'_> {
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
                .title(" Runner service "),
        )
        .wrap(Wrap { trim: true })
}

fn connection_card(data: &AppData) -> Paragraph<'_> {
    let lines = match &data.status {
        Some(s) => vec![
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
            Line::from(format!(
                "Runners:   {} configured",
                s.runners.len(),
            )),
        ],
        None => vec![Line::from(Span::styled(
            "Daemon IPC unreachable.",
            Style::default().fg(Color::DarkGray),
        ))],
    };
    Paragraph::new(lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Connection "),
        )
        .wrap(Wrap { trim: true })
}

fn hotkeys_card(data: &AppData) -> Paragraph<'_> {
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
    .block(Block::default().borders(Borders::ALL).title(" Controls "))
}

fn form_line(label: &str, value: &str, focused: bool, _: bool) -> Line<'static> {
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
    if let Ok(h) = std::env::var("HOSTNAME")
        && !h.is_empty()
    {
        return h;
    }
    nix::unistd::gethostname()
        .ok()
        .and_then(|os| os.into_string().ok())
        .unwrap_or_else(|| "runner".to_string())
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

    let req = crate::cloud::enroll::EnrollmentRequest {
        token: token.clone(),
        host_label: host_label.clone(),
        os: std::env::consts::OS.to_string(),
        arch: std::env::consts::ARCH.to_string(),
        version: crate::RUNNER_VERSION.to_string(),
    };
    let resp = crate::cloud::enroll::enroll(&cloud_url, &req)
        .await
        .map_err(|e| format!("enroll failed: {e:#}"))?;

    let cfg = crate::config::schema::Config {
        version: 2,
        daemon: crate::config::schema::DaemonConfig {
            cloud_url: cloud_url.clone(),
            log_level: "info".to_string(),
            log_retention_days: 14,
        },
        runners: Vec::new(),
    };
    crate::config::file::write_config(paths, &cfg).map_err(|e| format!("writing config.toml: {e:#}"))?;
    let creds = crate::config::schema::Credentials {
        connection_id: resp.connection_id,
        connection_secret: resp.connection_secret,
        connection_name: None,
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
