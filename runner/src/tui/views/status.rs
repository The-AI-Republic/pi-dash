use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};

use crate::tui::app::AppState;

pub fn render(f: &mut ratatui::Frame<'_>, area: Rect, state: &AppState) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(6), Constraint::Min(0)])
        .split(area);

    let lines = match &state.status {
        Some(s) => vec![
            Line::from(vec![
                Span::styled(
                    if s.connected {
                        "● Connected"
                    } else {
                        "○ Offline"
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
        None => vec![Line::from("Daemon not reachable. Is it running?")],
    };
    let header = Paragraph::new(lines)
        .block(Block::default().borders(Borders::ALL).title(" Identity "))
        .wrap(Wrap { trim: true });
    f.render_widget(header, chunks[0]);

    let body_lines = match state.status.as_ref().and_then(|s| s.current_run.as_ref()) {
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
    let body = Paragraph::new(body_lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Current run "),
        )
        .wrap(Wrap { trim: true });
    f.render_widget(body, chunks[1]);

    if state.onboarding_needed {
        let msg = Paragraph::new("No configuration found. Run `apple-pi-dash-runner configure --url ... --token ...` first.")
            .style(Style::default().fg(Color::Yellow));
        f.render_widget(msg, chunks[0]);
    }
}
