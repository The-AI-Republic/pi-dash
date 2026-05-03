//! Per-runner editable-field machinery used by the Runners tab.
//!
//! Splits cleanly into a *data layer* (FieldId / FieldKind / FIELDS,
//! `display_value`, `set_text_value`, `toggle_bool`, `cycle_enum`,
//! `differs`) and a small *render layer* used by the Runners tab to
//! draw the settings panel (`editable_lines`, `footer`,
//! `runner_picker_bar`).
//!
//! The data layer is purely-functional — it takes a `Config` plus a
//! runner index. The render layer takes the same data plus the
//! tab's local pane state (selected field index + optional in-flight
//! edit buffer). No `AppState`-or-`AppData`-coupled functions
//! survive: the Runners tab passes its own state explicitly.

use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};

use crate::config::schema::{AgentKind, Config};
use crate::service::reload::ReloadOutcome;

use super::super::app::AppData;

pub const AGENT_KINDS: &[&str] = &["codex", "claude-code"];
pub const LOG_LEVELS: &[&str] = &["trace", "debug", "info", "warn", "error"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldId {
    RunnerName,
    WorkspaceWorkingDir,
    AgentKind,
    CodexBinary,
    CodexModelDefault,
    ClaudeBinary,
    ClaudeModelDefault,
    ApprovalAutoReadonly,
    ApprovalAutoWrites,
    ApprovalAutoNetwork,
    LogLevel,
    LogRetentionDays,
}

#[derive(Debug, Clone, Copy)]
pub enum FieldKind {
    Text,
    Bool,
    Enum(&'static [&'static str]),
    U32,
}

pub struct FieldSpec {
    pub id: FieldId,
    pub label: &'static str,
    pub section: &'static str,
    pub kind: FieldKind,
}

pub const FIELDS: &[FieldSpec] = &[
    FieldSpec {
        id: FieldId::RunnerName,
        label: "name",
        section: "Runner",
        kind: FieldKind::Text,
    },
    FieldSpec {
        id: FieldId::WorkspaceWorkingDir,
        label: "working_dir",
        section: "Workspace",
        kind: FieldKind::Text,
    },
    FieldSpec {
        id: FieldId::AgentKind,
        label: "kind",
        section: "Agent",
        kind: FieldKind::Enum(AGENT_KINDS),
    },
    FieldSpec {
        id: FieldId::CodexBinary,
        label: "binary",
        section: "Codex",
        kind: FieldKind::Text,
    },
    FieldSpec {
        id: FieldId::CodexModelDefault,
        label: "model_default",
        section: "Codex",
        kind: FieldKind::Text,
    },
    FieldSpec {
        id: FieldId::ClaudeBinary,
        label: "binary",
        section: "Claude Code",
        kind: FieldKind::Text,
    },
    FieldSpec {
        id: FieldId::ClaudeModelDefault,
        label: "model_default",
        section: "Claude Code",
        kind: FieldKind::Text,
    },
    FieldSpec {
        id: FieldId::ApprovalAutoReadonly,
        label: "auto_approve_readonly_shell",
        section: "Approval policy",
        kind: FieldKind::Bool,
    },
    FieldSpec {
        id: FieldId::ApprovalAutoWrites,
        label: "auto_approve_workspace_writes",
        section: "Approval policy",
        kind: FieldKind::Bool,
    },
    FieldSpec {
        id: FieldId::ApprovalAutoNetwork,
        label: "auto_approve_network",
        section: "Approval policy",
        kind: FieldKind::Bool,
    },
];

pub fn field_count() -> usize {
    FIELDS.len()
}

pub fn field_at(idx: usize) -> &'static FieldSpec {
    &FIELDS[idx.min(FIELDS.len() - 1)]
}

fn runner_at(cfg: &Config, idx: usize) -> Option<&crate::config::schema::RunnerConfig> {
    if cfg.runners.is_empty() {
        return None;
    }
    let i = idx.min(cfg.runners.len() - 1);
    cfg.runners.get(i)
}

fn runner_at_mut(
    cfg: &mut Config,
    idx: usize,
) -> Option<&mut crate::config::schema::RunnerConfig> {
    if cfg.runners.is_empty() {
        return None;
    }
    let i = idx.min(cfg.runners.len() - 1);
    cfg.runners.get_mut(i)
}

pub fn display_value(cfg: &Config, id: FieldId, runner_idx: usize) -> String {
    if let FieldId::LogLevel = id {
        return cfg.daemon.log_level.clone();
    }
    if let FieldId::LogRetentionDays = id {
        return cfg.daemon.log_retention_days.to_string();
    }
    let Some(runner) = runner_at(cfg, runner_idx) else {
        return String::new();
    };
    match id {
        FieldId::RunnerName => runner.name.clone(),
        FieldId::WorkspaceWorkingDir => runner.workspace.working_dir.display().to_string(),
        FieldId::AgentKind => match runner.agent.kind {
            AgentKind::Codex => "codex".into(),
            AgentKind::ClaudeCode => "claude-code".into(),
        },
        FieldId::CodexBinary => runner.codex.binary.clone(),
        FieldId::CodexModelDefault => runner.codex.model_default.clone().unwrap_or_default(),
        FieldId::ClaudeBinary => runner.claude_code.binary.clone(),
        FieldId::ClaudeModelDefault => runner
            .claude_code
            .model_default
            .clone()
            .unwrap_or_default(),
        FieldId::ApprovalAutoReadonly => runner
            .approval_policy
            .auto_approve_readonly_shell
            .to_string(),
        FieldId::ApprovalAutoWrites => runner
            .approval_policy
            .auto_approve_workspace_writes
            .to_string(),
        FieldId::ApprovalAutoNetwork => runner.approval_policy.auto_approve_network.to_string(),
        FieldId::LogLevel | FieldId::LogRetentionDays => unreachable!(),
    }
}

pub fn set_text_value(
    cfg: &mut Config,
    id: FieldId,
    s: &str,
    runner_idx: usize,
) -> Result<(), String> {
    if matches!(id, FieldId::LogRetentionDays) {
        let n: u32 = s
            .parse()
            .map_err(|_| format!("expected a non-negative integer, got {s:?}"))?;
        cfg.daemon.log_retention_days = n;
        return Ok(());
    }
    let Some(runner) = runner_at_mut(cfg, runner_idx) else {
        return Err("no runners configured; add one with `pidash runner add` first".into());
    };
    match id {
        FieldId::RunnerName => {
            crate::util::runner_name::validate(s).map_err(|e| e.to_string())?;
            runner.name = s.to_string();
        }
        FieldId::WorkspaceWorkingDir => {
            if s.trim().is_empty() {
                return Err("working_dir cannot be empty".into());
            }
            runner.workspace.working_dir = std::path::PathBuf::from(s);
        }
        FieldId::CodexBinary => {
            if s.trim().is_empty() {
                return Err("binary cannot be empty".into());
            }
            runner.codex.binary = s.to_string();
        }
        FieldId::CodexModelDefault => {
            runner.codex.model_default = if s.is_empty() {
                None
            } else {
                Some(s.to_string())
            };
        }
        FieldId::ClaudeBinary => {
            if s.trim().is_empty() {
                return Err("binary cannot be empty".into());
            }
            runner.claude_code.binary = s.to_string();
        }
        FieldId::ClaudeModelDefault => {
            runner.claude_code.model_default = if s.is_empty() {
                None
            } else {
                Some(s.to_string())
            };
        }
        FieldId::AgentKind
        | FieldId::ApprovalAutoReadonly
        | FieldId::ApprovalAutoWrites
        | FieldId::ApprovalAutoNetwork
        | FieldId::LogLevel
        | FieldId::LogRetentionDays => {
            return Err("field is not a text input; use Enter to toggle/cycle instead".into());
        }
    }
    Ok(())
}

pub fn toggle_bool(cfg: &mut Config, id: FieldId, runner_idx: usize) {
    let Some(runner) = runner_at_mut(cfg, runner_idx) else {
        return;
    };
    match id {
        FieldId::ApprovalAutoReadonly => {
            let v = &mut runner.approval_policy.auto_approve_readonly_shell;
            *v = !*v;
        }
        FieldId::ApprovalAutoWrites => {
            let v = &mut runner.approval_policy.auto_approve_workspace_writes;
            *v = !*v;
        }
        FieldId::ApprovalAutoNetwork => {
            let v = &mut runner.approval_policy.auto_approve_network;
            *v = !*v;
        }
        _ => {}
    }
}

pub fn cycle_enum(cfg: &mut Config, id: FieldId, runner_idx: usize) {
    match id {
        FieldId::AgentKind => {
            let Some(runner) = runner_at_mut(cfg, runner_idx) else {
                return;
            };
            runner.agent.kind = match runner.agent.kind {
                AgentKind::Codex => AgentKind::ClaudeCode,
                AgentKind::ClaudeCode => AgentKind::Codex,
            };
        }
        FieldId::LogLevel => {
            let cur = LOG_LEVELS
                .iter()
                .position(|s| *s == cfg.daemon.log_level)
                .unwrap_or(2);
            cfg.daemon.log_level = LOG_LEVELS[(cur + 1) % LOG_LEVELS.len()].to_string();
        }
        _ => {}
    }
}

pub fn differs(a: &Config, b: &Config) -> bool {
    if a.daemon.log_level != b.daemon.log_level
        || a.daemon.log_retention_days != b.daemon.log_retention_days
    {
        return true;
    }
    let n = a.runners.len().max(b.runners.len()).max(1);
    for idx in 0..n {
        for f in FIELDS {
            if display_value(a, f.id, idx) != display_value(b, f.id, idx) {
                return true;
            }
        }
    }
    false
}

// --- rendering helpers — all parameter-driven, no `AppState` access ----------

pub fn runner_picker_bar(data: &AppData) -> Paragraph<'static> {
    let Some(working) = data.config_working.as_ref() else {
        return Paragraph::new(Line::raw("")).block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Runners "),
        );
    };
    let total = working.runners.len();
    let picked = data.runner_picker_idx.min(total.saturating_sub(1));
    let mut spans: Vec<Span<'static>> = Vec::new();
    for (i, r) in working.runners.iter().enumerate() {
        let label = format!(" {}. {} ", i + 1, r.name);
        let style = if i == picked {
            Style::default()
                .fg(Color::Black)
                .bg(Color::Cyan)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::Gray)
        };
        spans.push(Span::styled(label, style));
        spans.push(Span::raw("  "));
    }
    spans.push(Span::styled(
        "   [<] prev  [>] next  [Alt+1..9] jump".to_string(),
        Style::default().add_modifier(Modifier::DIM),
    ));
    Paragraph::new(Line::from(spans)).block(
        Block::default()
            .borders(Borders::ALL)
            .title(format!(" Runners ({}/{}) ", picked + 1, total)),
    )
}

pub fn editable_lines(
    working: &Config,
    loaded: &Option<Config>,
    selected_field_idx: usize,
    runner_idx: usize,
    edit_buffer: Option<&str>,
) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    let selected_idx = selected_field_idx.min(field_count().saturating_sub(1));

    lines.push(section_header("Runner"));
    lines.extend(render_editable_row(
        working,
        loaded,
        selected_idx,
        index_of(FieldId::RunnerName),
        runner_idx,
        edit_buffer,
    ));
    if let Some(picked_runner) = runner_at(working, runner_idx) {
        lines.push(readonly_row(
            "workspace_slug",
            picked_runner.workspace_slug.as_deref().unwrap_or("-"),
        ));
        if let Some(slug) = picked_runner.project_slug.as_deref() {
            lines.push(readonly_row("project_slug", slug));
        }
    } else {
        lines.push(readonly_hint(
            "(no runners configured — add one from the Runners tab)",
        ));
    }
    lines.push(readonly_hint(
        "workspace + project are bound at registration. Re-register or use \
         `pidash token add-runner` to change them.",
    ));
    lines.push(Line::raw(""));

    lines.push(section_header("Workspace"));
    lines.extend(render_editable_row(
        working,
        loaded,
        selected_idx,
        index_of(FieldId::WorkspaceWorkingDir),
        runner_idx,
        edit_buffer,
    ));
    lines.push(Line::raw(""));

    lines.push(section_header("Agent"));
    lines.extend(render_editable_row(
        working,
        loaded,
        selected_idx,
        index_of(FieldId::AgentKind),
        runner_idx,
        edit_buffer,
    ));
    lines.push(Line::raw(""));

    lines.push(section_header("Approval policy"));
    for id in [
        FieldId::ApprovalAutoReadonly,
        FieldId::ApprovalAutoWrites,
        FieldId::ApprovalAutoNetwork,
    ] {
        lines.extend(render_editable_row(
            working,
            loaded,
            selected_idx,
            index_of(id),
            runner_idx,
            edit_buffer,
        ));
    }
    if let Some(picked_runner) = runner_at(working, runner_idx) {
        lines.push(readonly_row(
            "allowlist_commands",
            &format!(
                "{} entries (edit via CLI / hand-edit)",
                picked_runner.approval_policy.allowlist_commands.len()
            ),
        ));
        lines.push(readonly_row(
            "denylist_commands",
            &format!(
                "{} entries (edit via CLI / hand-edit)",
                picked_runner.approval_policy.denylist_commands.len()
            ),
        ));
    }
    lines.push(Line::raw(""));
    lines.push(Line::from(Span::styled(
        "Daemon-level fields (log level, log retention) live in the General tab.",
        Style::default().add_modifier(Modifier::DIM),
    )));

    lines
}

#[allow(clippy::too_many_arguments)]
fn render_editable_row(
    working: &Config,
    loaded: &Option<Config>,
    selected_idx: usize,
    field_idx: usize,
    runner_idx: usize,
    edit_buffer: Option<&str>,
) -> Vec<Line<'static>> {
    let spec = &FIELDS[field_idx];
    let is_selected = selected_idx == field_idx;
    let editing_here = is_selected && edit_buffer.is_some();
    let displayed = if editing_here {
        format!("{}▊", edit_buffer.unwrap_or(""))
    } else {
        display_value(working, spec.id, runner_idx)
    };

    let modified = loaded
        .as_ref()
        .map(|l| {
            display_value(l, spec.id, runner_idx) != display_value(working, spec.id, runner_idx)
        })
        .unwrap_or(true);

    let marker = if is_selected { "▶" } else { " " };
    let mod_marker = if modified { "●" } else { " " };

    let value_style = if editing_here {
        Style::default().fg(Color::Yellow)
    } else if is_selected {
        Style::default()
            .fg(Color::White)
            .add_modifier(Modifier::BOLD)
    } else {
        match spec.kind {
            FieldKind::Bool => match displayed.as_str() {
                "true" => Style::default().fg(Color::Green),
                _ => Style::default().fg(Color::DarkGray),
            },
            _ => Style::default().fg(Color::White),
        }
    };

    let kind_hint = match spec.kind {
        FieldKind::Bool => "[bool — Enter to toggle]",
        FieldKind::Enum(_) => "[enum — Enter to cycle]",
        FieldKind::Text => "[text — Enter to edit]",
        FieldKind::U32 => "[number — Enter to edit]",
    };

    let row = Line::from(vec![
        Span::styled(
            format!(" {marker} "),
            Style::default()
                .fg(if is_selected {
                    Color::Cyan
                } else {
                    Color::DarkGray
                })
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!("{mod_marker} "),
            Style::default().fg(if modified {
                Color::Yellow
            } else {
                Color::DarkGray
            }),
        ),
        Span::raw(format!("{:<30} ", spec.label)),
        Span::styled(displayed, value_style),
        if is_selected && !editing_here {
            Span::styled(
                format!("   {kind_hint}"),
                Style::default().add_modifier(Modifier::DIM),
            )
        } else {
            Span::raw("")
        },
    ]);
    vec![row]
}

fn readonly_row(label: &str, value: &str) -> Line<'static> {
    Line::from(vec![
        Span::styled("   ".to_string(), Style::default().fg(Color::DarkGray)),
        Span::raw(format!("{label:<30} ")),
        Span::styled(value.to_string(), Style::default().fg(Color::DarkGray)),
        Span::styled(
            "   [read-only]".to_string(),
            Style::default().add_modifier(Modifier::DIM),
        ),
    ])
}

fn readonly_hint(msg: &str) -> Line<'static> {
    Line::from(vec![
        Span::raw(" ".repeat(3 + 30 + 1)),
        Span::styled(
            format!("↳ {msg}"),
            Style::default()
                .fg(Color::DarkGray)
                .add_modifier(Modifier::DIM),
        ),
    ])
}

fn section_header(name: &str) -> Line<'static> {
    Line::from(Span::styled(
        name.to_string(),
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
    ))
}

pub fn footer(
    edit_in_progress: bool,
    edit_error: Option<&str>,
    reload_outcome: Option<&ReloadOutcome>,
) -> Paragraph<'static> {
    let mut lines = Vec::new();
    let hint_line = if edit_in_progress {
        Line::from(Span::styled(
            "EDIT: [Enter] commit   [Esc] cancel   [Backspace] delete",
            Style::default().fg(Color::Yellow),
        ))
    } else {
        Line::from(Span::styled(
            "[j/k ↑/↓] move   [Enter] edit   [w] save+reload   [Esc] discard edits",
            Style::default().add_modifier(Modifier::DIM),
        ))
    };
    lines.push(hint_line);

    if let Some(e) = edit_error {
        lines.push(Line::from(Span::styled(
            e.to_string(),
            Style::default().fg(Color::Red),
        )));
    }

    if let Some(out) = reload_outcome {
        if out.ok {
            lines.push(Line::from(Span::styled(
                format!("✓ {}", out.summary),
                Style::default().fg(Color::Green),
            )));
        } else {
            lines.push(Line::from(Span::styled(
                format!("✗ {}", out.summary),
                Style::default().fg(Color::Red),
            )));
            if let Some(detail) = &out.detail {
                let first = detail.lines().next().unwrap_or("").to_string();
                lines.push(Line::from(Span::styled(
                    first,
                    Style::default().fg(Color::Red),
                )));
            }
        }
    }

    Paragraph::new(lines)
        .block(Block::default().borders(Borders::ALL).title(" Status "))
        .wrap(Wrap { trim: true })
}

fn index_of(id: FieldId) -> usize {
    FIELDS
        .iter()
        .position(|f| f.id == id)
        .expect("FieldId missing from FIELDS table")
}
