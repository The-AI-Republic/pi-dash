//! General tab — daemon-level surface.
//!
//! Owns the daemon state that's shared across every runner the daemon
//! hosts: cloud URL, connection status, uptime, log level, log
//! retention, plus the OS service controls. Per-runner editing lives
//! on the Runners tab; this tab is for "is the daemon up and behaving."
//!
//! On a fresh machine (no config.toml) this tab takes over with the
//! inline register form — it's the cloud-binding step, which is a
//! daemon-level concern, so it belongs here rather than on the Runners
//! tab.
//!
//! Keys: `[s]` start service, `[x]` stop service, `[r]` refresh.
//! Daemon-level config editing (log_level, log_retention_days) reuses
//! the same edit-buffer flow as the Runners tab — the host `app.rs`
//! routes `Enter` / typed keys through the same handlers.

use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};

use crate::tui::app::AppState;
use crate::tui::views::config as fields;

pub fn render(f: &mut ratatui::Frame<'_>, area: Rect, state: &AppState) {
    if state.config_working.is_none() {
        render_register_view(f, area, state);
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
    f.render_widget(service_card(state), chunks[0]);
    f.render_widget(connection_card(state), chunks[1]);
    f.render_widget(daemon_settings_card(state), chunks[2]);
    f.render_widget(hotkeys_card(state), chunks[3]);
}

fn render_register_view(f: &mut ratatui::Frame<'_>, area: Rect, state: &AppState) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(0), Constraint::Length(3)])
        .split(area);
    let p = Paragraph::new(fields::register_form_lines(state))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Register with cloud "),
        )
        .wrap(Wrap { trim: false });
    f.render_widget(p, chunks[0]);
    f.render_widget(hotkeys_card_register(), chunks[1]);
}

fn hotkeys_card_register() -> Paragraph<'static> {
    Paragraph::new(Line::from(vec![Span::styled(
        "Tab/↑↓ move field   ↵ advance / submit   Esc clears form error",
        Style::default().add_modifier(Modifier::DIM),
    )]))
    .block(Block::default().borders(Borders::ALL).title(" Controls "))
}

fn service_card(state: &AppState) -> Paragraph<'_> {
    let raw = state.service_state.as_deref().unwrap_or("unknown");
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
    if let Some(msg) = &state.service_action_msg {
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

fn connection_card(state: &AppState) -> Paragraph<'_> {
    let lines = match &state.status {
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

/// Daemon-level editable settings: log level (cycles) and log retention
/// days (text). The Runners tab covers the per-runner side; here we own
/// the values that are not tied to any single runner.
///
/// `render()` short-circuits to the register view when `config_working`
/// is None, so this card is only invoked with config loaded.
fn daemon_settings_card(state: &AppState) -> Paragraph<'_> {
    let mut lines: Vec<Line<'_>> = Vec::new();
    let cfg = state
        .config_working
        .as_ref()
        .expect("daemon_settings_card called without a working config");
    let editing = state.tab_general_field == GeneralField::LogRetentionDays
        && state.config_edit_buffer.is_some();
    let log_level_style = if state.tab_general_field == GeneralField::LogLevel {
        Style::default()
            .fg(Color::White)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::Gray)
    };
    let retention_value = if editing {
        format!(
            "{}▊",
            state.config_edit_buffer.as_deref().unwrap_or("")
        )
    } else {
        cfg.daemon.log_retention_days.to_string()
    };
    let retention_style = if state.tab_general_field == GeneralField::LogRetentionDays {
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
    lines.push(Line::from(vec![
        Span::styled(
            format!(" {} log_level         ", marker(state.tab_general_field == GeneralField::LogLevel)),
            Style::default().fg(Color::Cyan),
        ),
        Span::styled(cfg.daemon.log_level.clone(), log_level_style),
        if state.tab_general_field == GeneralField::LogLevel {
            Span::styled(
                "   [Enter cycles]".to_string(),
                Style::default().add_modifier(Modifier::DIM),
            )
        } else {
            Span::raw("")
        },
    ]));
    lines.push(Line::from(vec![
        Span::styled(
            format!(" {} log_retention_days ", marker(state.tab_general_field == GeneralField::LogRetentionDays)),
            Style::default().fg(Color::Cyan),
        ),
        Span::styled(retention_value, retention_style),
        if state.tab_general_field == GeneralField::LogRetentionDays && !editing {
            Span::styled(
                "   [Enter edits]".to_string(),
                Style::default().add_modifier(Modifier::DIM),
            )
        } else {
            Span::raw("")
        },
    ]));
    lines.push(Line::raw(""));
    lines.push(Line::from(Span::styled(
        "[j/k ↑↓] move   [Enter] edit/cycle   [w] save+reload   [Esc] discard",
        Style::default().add_modifier(Modifier::DIM),
    )));
    if let Some(e) = &state.config_edit_error {
        lines.push(Line::from(Span::styled(
            e.clone(),
            Style::default().fg(Color::Red),
        )));
    }
    if let Some(out) = &state.reload_outcome {
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

fn hotkeys_card(state: &AppState) -> Paragraph<'_> {
    let active = matches!(state.service_state.as_deref(), Some("active"));
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

/// Which daemon-level field the General tab's selection cursor is on.
/// Mirrors the Config tab's `selected` index but typed since there are
/// only two daemon fields and they don't share the per-runner FIELDS
/// table.
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
        // Two-field cycle, so prev == next.
        self.next()
    }
}
