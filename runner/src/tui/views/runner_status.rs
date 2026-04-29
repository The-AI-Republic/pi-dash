//! Runners tab — list of every runner this daemon hosts.
//!
//! Multi-runner UX entry point: shows one row per `[[runner]]` block in
//! `config.toml`, the cloud connection summary at the top, and inline
//! `[a]` / `[d]` shortcuts to add or remove runners. Selection moves
//! with `j` / `k` and is reused by the picker for per-runner tabs —
//! whatever runner is highlighted here is the one Config / Runs /
//! Approvals scope to by default.
//!
//! Add and remove flows themselves live in `app.rs` (modal forms), but
//! they hang off this tab's hotkeys.

use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap};

use crate::tui::app::AppState;

pub fn render(f: &mut ratatui::Frame<'_>, area: Rect, state: &AppState) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(4),
            Constraint::Min(0),
            Constraint::Length(4),
            Constraint::Length(3),
        ])
        .split(area);
    f.render_widget(summary_card(state), chunks[0]);
    render_runners_list(f, chunks[1], state);
    f.render_widget(detail_card(state), chunks[2]);
    f.render_widget(hotkeys_card(), chunks[3]);
}

fn summary_card(state: &AppState) -> Paragraph<'_> {
    let lines = match &state.status {
        Some(s) => vec![
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
                    "{} runner{} hosted",
                    s.runners.len(),
                    if s.runners.len() == 1 { "" } else { "s" },
                )),
            ]),
            Line::from(format!("Cloud: {}", s.daemon.cloud_url)),
        ],
        None => vec![Line::from(Span::styled(
            "Daemon IPC unreachable.",
            Style::default().fg(Color::DarkGray),
        ))],
    };
    Paragraph::new(lines)
        .block(Block::default().borders(Borders::ALL).title(" Daemon "))
        .wrap(Wrap { trim: true })
}

fn render_runners_list(f: &mut ratatui::Frame<'_>, area: Rect, state: &AppState) {
    let runners: Vec<&crate::ipc::protocol::RunnerStatusSnapshot> = state
        .status
        .as_ref()
        .map(|s| s.runners.iter().collect())
        .unwrap_or_default();

    if runners.is_empty() {
        let empty = Paragraph::new(vec![
            Line::raw(""),
            Line::from(Span::styled(
                "No runners configured on this machine yet.",
                Style::default().fg(Color::DarkGray),
            )),
            Line::raw(""),
            Line::from("Press [a] to register one against the locally-installed token,"),
            Line::from(Span::styled(
                "or run `pidash configure --url ... --token <REG_CODE>` from the CLI",
                Style::default().add_modifier(Modifier::DIM),
            )),
            Line::from(Span::styled(
                "for a fresh-machine setup.",
                Style::default().add_modifier(Modifier::DIM),
            )),
        ])
        .block(Block::default().borders(Borders::ALL).title(" Runners "))
        .wrap(Wrap { trim: true });
        f.render_widget(empty, area);
        return;
    }

    let items: Vec<ListItem<'_>> = runners
        .iter()
        .map(|r| {
            let project = r.project_slug.as_deref().unwrap_or("(no project)");
            let status_label = format!("{:?}", r.status);
            let approvals = if r.approvals_pending > 0 {
                format!("approvals={}", r.approvals_pending)
            } else {
                String::new()
            };
            let line = Line::from(vec![
                Span::styled(
                    format!(" {:<24} ", r.name),
                    Style::default()
                        .fg(Color::White)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    format!("project={:<14} ", project),
                    Style::default().fg(Color::Cyan),
                ),
                Span::styled(
                    format!("{:<10} ", status_label),
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

    let list = List::new(items)
        .block(Block::default().borders(Borders::ALL).title(" Runners "))
        .highlight_style(Style::default().add_modifier(Modifier::REVERSED));
    let mut lstate = ListState::default();
    lstate.select(Some(state.runners_list_idx.min(runners.len() - 1)));
    f.render_stateful_widget(list, area, &mut lstate);
}

/// Detail-and-hint card shown below the runner list. Surfaces the
/// in-flight run for the highlighted runner (if any) plus the
/// add-runner CLI command so users discover the multi-runner workflow
/// even at N=1.
fn detail_card(state: &AppState) -> Paragraph<'_> {
    let mut lines: Vec<Line<'_>> = Vec::new();
    if let Some(s) = &state.status
        && let Some(runner) = s.runners.get(state.runners_list_idx)
    {
        if let Some(run) = &runner.current_run {
            lines.push(Line::from(format!(
                "[{}] in-flight run {} ({}); events={}",
                runner.name, run.run_id, run.status, run.events,
            )));
        } else {
            lines.push(Line::from(format!("[{}] idle", runner.name)));
        }
    } else if state.status.is_some() {
        lines.push(Line::from(Span::styled(
            "Use [a] to register a runner under the locally-installed machine token.",
            Style::default().fg(Color::Cyan),
        )));
    }
    Paragraph::new(lines)
        .block(Block::default().borders(Borders::ALL).title(" Selected "))
        .wrap(Wrap { trim: true })
}

fn hotkeys_card() -> Paragraph<'static> {
    Paragraph::new(Line::from(vec![
        Span::styled(
            "[a] add runner",
            Style::default().fg(Color::Green),
        ),
        Span::raw("   "),
        Span::styled(
            "[d] remove runner",
            Style::default().fg(Color::Red),
        ),
        Span::raw("   [j/k ↑↓] move   [r] refresh"),
    ]))
    .block(Block::default().borders(Borders::ALL).title(" Controls "))
}
