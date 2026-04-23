use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};

use crate::tui::app::AppState;

pub fn render(f: &mut ratatui::Frame<'_>, area: Rect, state: &AppState) {
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
    f.render_widget(identity_card(state), chunks[1]);
    f.render_widget(current_run_card(state), chunks[2]);
    f.render_widget(hotkeys_card(state), chunks[3]);
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

fn identity_card(state: &AppState) -> Paragraph<'_> {
    let lines = match &state.status {
        Some(s) => vec![
            Line::from(vec![
                Span::styled(
                    if s.connected {
                        "● Cloud connected"
                    } else {
                        "○ Cloud offline"
                    },
                    Style::default().fg(if s.connected {
                        Color::Green
                    } else {
                        Color::Red
                    }),
                ),
                Span::raw("  "),
                Span::raw(s.runner_name.clone()),
            ]),
            Line::from(format!("Cloud: {}", s.cloud_url)),
            Line::from(format!(
                "Runner ID: {}",
                s.runner_id
                    .map(|u| u.to_string())
                    .unwrap_or_else(|| "-".into())
            )),
            Line::from(format!("Uptime: {}s", s.uptime_secs)),
            Line::from(format!("Approvals pending: {}", s.approvals_pending)),
        ],
        None => vec![Line::from(Span::styled(
            "Daemon IPC unreachable.",
            Style::default().fg(Color::DarkGray),
        ))],
    };
    Paragraph::new(lines)
        .block(Block::default().borders(Borders::ALL).title(" Identity "))
        .wrap(Wrap { trim: true })
}

fn current_run_card(state: &AppState) -> Paragraph<'_> {
    let lines = match state.status.as_ref().and_then(|s| s.current_run.as_ref()) {
        Some(run) => vec![
            Line::from(format!("Run: {}", run.run_id)),
            Line::from(format!(
                "Thread: {}",
                run.thread_id.as_deref().unwrap_or("-")
            )),
            Line::from(format!("Status: {}", run.status)),
            Line::from(format!("Events seen: {}", run.events)),
        ],
        None => vec![Line::from("No active run.")],
    };
    Paragraph::new(lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Current run "),
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
