use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};

use crate::config::schema::{AgentKind, Config};
use crate::tui::app::AppState;

pub fn render(f: &mut ratatui::Frame<'_>, area: Rect, state: &AppState) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(0), Constraint::Length(4)])
        .split(area);

    if let Some(err) = &state.config_error {
        let p = Paragraph::new(format!("Failed to read config.toml:\n\n{err}"))
            .style(Style::default().fg(Color::Red))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(" Configuration "),
            )
            .wrap(Wrap { trim: false });
        f.render_widget(p, chunks[0]);
        f.render_widget(footer(state), chunks[1]);
        return;
    }

    match &state.config_loaded {
        Some(cfg) => {
            let p = Paragraph::new(config_lines(cfg))
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title(" Configuration "),
                )
                .wrap(Wrap { trim: false });
            f.render_widget(p, chunks[0]);
        }
        None => {
            let p = Paragraph::new(vec![
                Line::from(Span::styled(
                    "No runner configured yet.",
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                )),
                Line::raw(""),
                Line::from("Register this runner with Pi Dash cloud to create a config."),
                Line::raw(""),
                Line::from("From another terminal, run:"),
                Line::from(Span::styled(
                    "  pidash configure --url <CLOUD_URL> --token <ONE_TIME_TOKEN>",
                    Style::default().fg(Color::Cyan),
                )),
                Line::raw(""),
                Line::from("Then return here — the Config tab will populate automatically."),
            ])
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(" Configuration "),
            )
            .wrap(Wrap { trim: false });
            f.render_widget(p, chunks[0]);
        }
    }

    f.render_widget(footer(state), chunks[1]);
}

fn footer(state: &AppState) -> Paragraph<'_> {
    let lines = match &state.reload_outcome {
        Some(out) if out.ok => vec![
            Line::from(Span::styled(
                "✓ Daemon reloaded successfully",
                Style::default().fg(Color::Green).add_modifier(Modifier::BOLD),
            )),
            Line::from(out.summary.clone()),
        ],
        Some(out) => vec![
            Line::from(Span::styled(
                "✗ Reload failed",
                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
            )),
            Line::from(out.summary.clone()),
            Line::from(Span::styled(
                out.detail.clone().unwrap_or_default(),
                Style::default().fg(Color::Red),
            )),
        ],
        None => vec![Line::from(Span::styled(
            "Editing comes next — for now, use `pidash configure --<flag>` from a shell.",
            Style::default().add_modifier(Modifier::DIM),
        ))],
    };
    Paragraph::new(lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Last reload "),
        )
        .wrap(Wrap { trim: true })
}

fn config_lines(cfg: &Config) -> Vec<Line<'_>> {
    let mut l = Vec::new();
    l.push(section_header("Runner"));
    l.push(kv("name", &cfg.runner.name));
    l.push(kv("cloud_url", &cfg.runner.cloud_url));
    l.push(kv(
        "workspace_slug",
        cfg.runner.workspace_slug.as_deref().unwrap_or("-"),
    ));
    l.push(Line::raw(""));

    l.push(section_header("Workspace"));
    l.push(kv("working_dir", &cfg.workspace.working_dir.display().to_string()));
    l.push(Line::raw(""));

    l.push(section_header("Agent"));
    l.push(kv(
        "kind",
        match cfg.agent.kind {
            AgentKind::Codex => "codex",
            AgentKind::ClaudeCode => "claude-code",
        },
    ));
    l.push(Line::raw(""));

    l.push(section_header("Codex"));
    l.push(kv("binary", &cfg.codex.binary));
    l.push(kv(
        "model_default",
        cfg.codex.model_default.as_deref().unwrap_or("-"),
    ));
    l.push(Line::raw(""));

    l.push(section_header("Claude Code"));
    l.push(kv("binary", &cfg.claude_code.binary));
    l.push(kv(
        "model_default",
        cfg.claude_code.model_default.as_deref().unwrap_or("-"),
    ));
    l.push(Line::raw(""));

    l.push(section_header("Approval policy"));
    l.push(kv_bool(
        "auto_approve_readonly_shell",
        cfg.approval_policy.auto_approve_readonly_shell,
    ));
    l.push(kv_bool(
        "auto_approve_workspace_writes",
        cfg.approval_policy.auto_approve_workspace_writes,
    ));
    l.push(kv_bool(
        "auto_approve_network",
        cfg.approval_policy.auto_approve_network,
    ));
    l.push(kv(
        "allowlist_commands",
        &format!("{} entries", cfg.approval_policy.allowlist_commands.len()),
    ));
    l.push(kv(
        "denylist_commands",
        &format!("{} entries", cfg.approval_policy.denylist_commands.len()),
    ));
    l.push(Line::raw(""));

    l.push(section_header("Logging"));
    l.push(kv("level", &cfg.logging.level));
    l.push(kv(
        "retention_days",
        &cfg.logging.retention_days.to_string(),
    ));

    l
}

fn section_header(name: &str) -> Line<'static> {
    Line::from(Span::styled(
        name.to_string(),
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
    ))
}

fn kv(k: &str, v: &str) -> Line<'static> {
    Line::from(vec![
        Span::raw(format!("  {k:<30} ")),
        Span::styled(v.to_string(), Style::default().fg(Color::White)),
    ])
}

fn kv_bool(k: &str, v: bool) -> Line<'static> {
    let (label, color) = if v {
        ("true", Color::Green)
    } else {
        ("false", Color::DarkGray)
    };
    Line::from(vec![
        Span::raw(format!("  {k:<30} ")),
        Span::styled(label.to_string(), Style::default().fg(color)),
    ])
}
