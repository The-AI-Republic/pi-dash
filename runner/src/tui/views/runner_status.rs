//! Runners tab — list of every runner this daemon hosts plus the
//! per-runner settings panel for the highlighted runner.
//!
//! Replaces the old single-runner Config tab. Layout:
//!
//! - **Configured** (config.toml exists): left pane is the runner-row
//!   list ("picker"), right pane is the editable settings panel for
//!   whichever runner the picker is on. `j`/`k` moves the field
//!   cursor inside the panel; `<`/`>` and `Alt+1`–`Alt+9` move the
//!   runner picker.
//!
//! - **Fresh machine** (no config.toml): renders an empty-state
//!   placeholder pointing the user at the General tab, where the
//!   inline register form lives — registration is a daemon-level
//!   step (binds the whole daemon to a cloud URL), so it doesn't
//!   belong here.
//!
//! `[a]` opens the add-runner form (cascaded project / pod picker).
//! `[d]` confirm-removes the highlighted runner via the cloud's
//! token-authenticated deregister endpoint.

use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap};

use crate::tui::app::AppState;
use crate::tui::views::config as fields;

pub fn render(f: &mut ratatui::Frame<'_>, area: Rect, state: &AppState) {
    if state.config_working.is_none() {
        render_unregistered_placeholder(f, area);
        return;
    }

    // Outer vertical split: body row + hotkeys footer. Body is split
    // horizontally into runner list (left) and settings panel (right).
    let outer = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(8), Constraint::Length(3)])
        .split(area);
    let body = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            // 36 cols is enough for ~30-char runner names + status
            // markers; surplus width goes to the settings panel which
            // is the editing surface.
            Constraint::Length(36),
            Constraint::Min(40),
        ])
        .split(outer[0]);
    render_runner_list(f, body[0], state);
    render_settings_panel(f, body[1], state);
    f.render_widget(hotkeys_card(), outer[1]);
}

fn render_unregistered_placeholder(f: &mut ratatui::Frame<'_>, area: Rect) {
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
    f.render_widget(p, area);
}

fn render_runner_list(f: &mut ratatui::Frame<'_>, area: Rect, state: &AppState) {
    let runners: Vec<&crate::ipc::protocol::RunnerStatusSnapshot> = state
        .status
        .as_ref()
        .map(|s| s.runners.iter().collect())
        .unwrap_or_default();

    if runners.is_empty() {
        // Configured but daemon hasn't reported any RunnerStatus yet —
        // either it's still starting up, or every runner is unhealthy.
        // Show a placeholder so the layout doesn't collapse.
        let p = Paragraph::new(vec![Line::from(Span::styled(
            "Daemon up but no runners reported yet — check the General tab.",
            Style::default().fg(Color::DarkGray),
        ))])
        .block(Block::default().borders(Borders::ALL).title(" Runners "))
        .wrap(Wrap { trim: true });
        f.render_widget(p, area);
        return;
    }

    let picked = state.runner_picker_idx.min(runners.len() - 1);
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
            let prefix = if i == picked { "▶ " } else { "  " };
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
    let title = format!(" Runners ({}/{}) ", picked + 1, total);
    let focused = matches!(
        state.runner_tab_focus,
        crate::tui::app::RunnerTabFocus::RunnerList
    );
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
    lstate.select(Some(picked));
    f.render_stateful_widget(list, area, &mut lstate);
}

fn render_settings_panel(f: &mut ratatui::Frame<'_>, area: Rect, state: &AppState) {
    let Some(working) = state.config_working.as_ref() else {
        return;
    };

    // No runner configured → don't render empty form fields. Show a
    // centred placeholder pointing at the [a] hotkey instead.
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
        f.render_widget(placeholder, area);
        return;
    }

    let loaded = state.config_loaded.clone();

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
    let focused = matches!(
        state.runner_tab_focus,
        crate::tui::app::RunnerTabFocus::Settings
    );
    let block_style = if focused {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default()
    };
    let p = Paragraph::new(fields::editable_lines(working, &loaded, state))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(block_style)
                .title(title),
        )
        .wrap(Wrap { trim: false });
    f.render_widget(p, chunks[0]);
    f.render_widget(fields::footer(state), chunks[1]);
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
