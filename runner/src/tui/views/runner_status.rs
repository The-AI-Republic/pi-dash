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
        Some(s) => {
            let mut lines = vec![
                Line::from(vec![
                    Span::styled(
                        if s.daemon.connected {
                            "● Cloud connected"
                        } else {
                            "○ Cloud offline"
                        },
                        Style::default().fg(if s.daemon.connected {
                            Color::Green
                        } else {
                            Color::Red
                        }),
                    ),
                    Span::raw("  "),
                    Span::raw(format!(
                        "{} runner{}",
                        s.runners.len(),
                        if s.runners.len() == 1 { "" } else { "s" },
                    )),
                ]),
                Line::from(format!("Cloud: {}", s.daemon.cloud_url)),
                Line::from(format!("Uptime: {}s", s.daemon.uptime_secs)),
            ];
            if s.runners.is_empty() {
                lines.push(Line::from(Span::styled(
                    "  (no runners configured)",
                    Style::default().fg(Color::DarkGray),
                )));
            } else {
                for r in &s.runners {
                    let project = r.project_slug.as_deref().unwrap_or("-");
                    lines.push(Line::from(format!(
                        "  • {} (project={}, status={:?}, approvals={})",
                        r.name, project, r.status, r.approvals_pending,
                    )));
                }
            }
            lines
        }
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
    let mut lines: Vec<Line<'_>> = Vec::new();
    match &state.status {
        Some(s) => {
            let mut had_run = false;
            for r in &s.runners {
                if let Some(run) = &r.current_run {
                    had_run = true;
                    lines.push(Line::from(format!("[{}] Run: {}", r.name, run.run_id)));
                    lines.push(Line::from(format!(
                        "[{}] Thread: {}",
                        r.name,
                        run.thread_id.as_deref().unwrap_or("-")
                    )));
                    lines.push(Line::from(format!(
                        "[{}] Status: {}  Events: {}",
                        r.name, run.status, run.events,
                    )));
                }
            }
            if !had_run {
                lines.push(Line::from("No active run on any runner."));
            }
        }
        None => {
            lines.push(Line::from(Span::styled(
                "Daemon IPC unreachable.",
                Style::default().fg(Color::DarkGray),
            )));
        }
    }
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
