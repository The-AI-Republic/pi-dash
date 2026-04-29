//! Config tab — editable view over `config.toml`.
//!
//! Navigation: ↑/↓ or j/k move between editable fields. Enter edits:
//!   - Bool fields toggle in place.
//!   - Enum fields cycle to the next value.
//!   - Text / path / number fields open an inline text-input buffer;
//!     Enter commits, Esc cancels.
//!
//! `w` writes the working copy to `config.toml` and kicks the daemon via
//! `service::reload::restart_and_verify`. The outcome (cloud-connected or
//! detailed failure) shows in the footer so users immediately see whether
//! their edit broke the runner.
//!
//! `Esc` in browse-mode discards pending edits and reloads from disk.
//! Read-only fields (cloud_url, workspace_slug, list fields) are rendered
//! but skipped in navigation.

// Layout no longer drives the (deleted) full-page Tab::Config render
// dispatch — the Runners tab composes the `editable_lines` /
// `footer` / `register_form_lines` helpers itself.
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};

use crate::config::schema::{AgentKind, Config};
use crate::tui::app::AppState;

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

/// Every editable field, in render order. Navigation indices map 1:1 into
/// this slice, so adding or reordering fields here also reorders the UI.
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
    // Daemon-level log fields (log_level, log_retention_days) live in
    // the General tab — they're shared across every runner the daemon
    // hosts and don't make sense as per-runner edits.
];

pub fn field_count() -> usize {
    FIELDS.len()
}

pub fn field_at(idx: usize) -> &'static FieldSpec {
    &FIELDS[idx.min(FIELDS.len() - 1)]
}

/// Look up the runner the Config tab's per-runner fields apply to.
/// Clamps `idx` into bounds so a stale picker index from a freshly
/// loaded config can't panic. Daemon-level fields (LogLevel etc.)
/// don't go through this.
fn runner_at(cfg: &Config, idx: usize) -> &crate::config::schema::RunnerConfig {
    let n = cfg.runners.len().max(1);
    let i = idx.min(n - 1);
    cfg.runners.get(i).unwrap_or_else(|| cfg.primary_runner())
}

fn runner_at_mut(
    cfg: &mut Config,
    idx: usize,
) -> &mut crate::config::schema::RunnerConfig {
    let n = cfg.runners.len().max(1);
    let i = idx.min(n - 1);
    if cfg.runners.get(i).is_some() {
        return &mut cfg.runners[i];
    }
    cfg.primary_runner_mut()
}

pub fn display_value(cfg: &Config, id: FieldId, runner_idx: usize) -> String {
    let runner = runner_at(cfg, runner_idx);
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
        FieldId::ClaudeModelDefault => runner.claude_code.model_default.clone().unwrap_or_default(),
        FieldId::ApprovalAutoReadonly => runner
            .approval_policy
            .auto_approve_readonly_shell
            .to_string(),
        FieldId::ApprovalAutoWrites => runner
            .approval_policy
            .auto_approve_workspace_writes
            .to_string(),
        FieldId::ApprovalAutoNetwork => runner.approval_policy.auto_approve_network.to_string(),
        FieldId::LogLevel => cfg.daemon.log_level.clone(),
        FieldId::LogRetentionDays => cfg.daemon.log_retention_days.to_string(),
    }
}

/// Commit an edited buffer to the config. Only valid for Text/U32 fields;
/// Bool/Enum fields use `toggle_bool` / `cycle_enum` instead. Returns a
/// user-facing error message on parse/validation failure.
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
    let runner = runner_at_mut(cfg, runner_idx);
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
    let runner = runner_at_mut(cfg, runner_idx);
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
            let runner = runner_at_mut(cfg, runner_idx);
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

// --- rendering helpers, consumed by Runners tab and the global picker -----

/// Top-of-tab picker showing each configured runner as a chip. The
/// currently-selected runner is highlighted; the bar only renders when
/// there are 2+ runners so single-runner installs see the same UI as
/// before. Keys: `<`/`>` cycle, `Alt+1`–`Alt+9` jump.
pub fn runner_picker_bar(state: &AppState) -> Paragraph<'static> {
    let Some(working) = state.config_working.as_ref() else {
        return Paragraph::new(Line::raw("")).block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Runners "),
        );
    };
    let total = working.runners.len();
    let picked = state.runner_picker_idx.min(total.saturating_sub(1));
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

pub fn register_form_lines(state: &AppState) -> Vec<Line<'static>> {
    let Some(form) = state.register_form.as_ref() else {
        // No form yet — refresh() will seed one next tick; show a hint.
        return vec![Line::from(Span::styled(
            "Loading…",
            Style::default().add_modifier(Modifier::DIM),
        ))];
    };
    let mut lines = vec![
        Line::from(Span::styled(
            "This runner isn't registered yet.",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from("Fill in the form below and press [Register] to connect."),
        Line::raw(""),
    ];

    lines.push(form_field_line(
        "Cloud URL",
        &form.cloud_url,
        form.focus == 0,
    ));
    // Token is pre-masked here; `form_field_line` is unaware of masking.
    lines.push(form_field_line(
        "Token",
        &mask_token(&form.token),
        form.focus == 1,
    ));
    lines.push(form_field_line("Runner name", &form.name, form.focus == 2));
    lines.push(Line::raw(""));
    lines.push(form_button_line(form.focus == 3, form.busy));
    lines.push(Line::raw(""));
    lines.push(Line::from(Span::styled(
        "Tab/↑↓ move   type to edit   ↵ advance / submit",
        Style::default().add_modifier(Modifier::DIM),
    )));
    lines.push(Line::from(Span::styled(
        "Tokens are generated in the Pi Dash web UI: Workspace → Runners → Mint code",
        Style::default().add_modifier(Modifier::DIM),
    )));

    if let Some(e) = &form.error {
        lines.push(Line::raw(""));
        lines.push(Line::from(Span::styled(
            e.clone(),
            Style::default().fg(Color::Red),
        )));
    }
    if form.busy {
        lines.push(Line::raw(""));
        lines.push(Line::from(Span::styled(
            "contacting cloud…",
            Style::default().fg(Color::Yellow),
        )));
    }
    lines
}

fn form_field_line(label: &str, value: &str, focused: bool) -> Line<'static> {
    let marker = if focused { "▶" } else { " " };
    let value_style = if focused {
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::White)
    };
    let cursor = if focused { "▊" } else { "" };
    Line::from(vec![
        Span::styled(
            format!(" {marker} "),
            Style::default()
                .fg(if focused {
                    Color::Cyan
                } else {
                    Color::DarkGray
                })
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(format!("{label:<14} ")),
        Span::styled(format!("{value}{cursor}"), value_style),
    ])
}

fn form_button_line(focused: bool, busy: bool) -> Line<'static> {
    let label = if busy {
        " Registering… "
    } else {
        " Register "
    };
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
    Line::from(vec![
        Span::raw("   "),
        Span::styled(label.to_string(), style),
    ])
}

fn mask_token(raw: &str) -> String {
    if raw.len() <= 4 {
        "*".repeat(raw.len())
    } else {
        format!("{}…{}", &raw[..2], &raw[raw.len() - 2..])
    }
}

pub fn editable_lines(
    working: &Config,
    loaded: &Option<Config>,
    state: &AppState,
) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    let selected_idx = state.selected.min(field_count().saturating_sub(1));

    // Runner section: name is editable, workspace_slug + project_slug
    // are read-only because they're bound at registration. cloud_url
    // is daemon-level (one URL shared across every runner this daemon
    // hosts) and lives on the General tab; we don't repeat it here to
    // avoid the misleading impression that it's a per-runner setting.
    lines.push(section_header("Runner"));
    lines.extend(render_editable_row(
        working,
        loaded,
        state,
        selected_idx,
        index_of(FieldId::RunnerName),
    ));
    let picker_idx = state.runner_picker_idx;
    let picked_runner = runner_at(working, picker_idx);
    lines.push(readonly_row(
        "workspace_slug",
        picked_runner.workspace_slug.as_deref().unwrap_or("-"),
    ));
    if let Some(slug) = picked_runner.project_slug.as_deref() {
        lines.push(readonly_row("project_slug", slug));
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
        state,
        selected_idx,
        index_of(FieldId::WorkspaceWorkingDir),
    ));
    lines.push(Line::raw(""));

    lines.push(section_header("Agent"));
    lines.extend(render_editable_row(
        working,
        loaded,
        state,
        selected_idx,
        index_of(FieldId::AgentKind),
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
            state,
            selected_idx,
            index_of(id),
        ));
    }
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
    lines.push(Line::raw(""));
    lines.push(Line::from(Span::styled(
        "Daemon-level fields (log level, log retention) live in the General tab.",
        Style::default().add_modifier(Modifier::DIM),
    )));

    lines
}

fn render_editable_row(
    working: &Config,
    loaded: &Option<Config>,
    state: &AppState,
    selected_idx: usize,
    field_idx: usize,
) -> Vec<Line<'static>> {
    let spec = &FIELDS[field_idx];
    let is_selected = selected_idx == field_idx;
    let editing_here = is_selected && state.config_edit_buffer.is_some();
    let runner_idx = state.runner_picker_idx;
    let displayed = if editing_here {
        format!("{}▊", state.config_edit_buffer.as_deref().unwrap_or(""))
    } else {
        display_value(working, spec.id, runner_idx)
    };

    let modified = loaded
        .as_ref()
        .map(|l| {
            display_value(l, spec.id, runner_idx)
                != display_value(working, spec.id, runner_idx)
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

/// Continuation line for a read-only row that needs more explanation than
/// `[read-only]` conveys. Aligns under the value column for readability.
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

pub fn footer(state: &AppState) -> Paragraph<'_> {
    let mut lines = Vec::new();

    // Action hints — contextual.
    let hint_line = if state.config_edit_buffer.is_some() {
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

    // Edit-time error (e.g. bad parse).
    if let Some(e) = &state.config_edit_error {
        lines.push(Line::from(Span::styled(
            e.clone(),
            Style::default().fg(Color::Red),
        )));
    }

    // Last reload outcome.
    match &state.reload_outcome {
        Some(out) if out.ok => {
            lines.push(Line::from(Span::styled(
                format!("✓ {}", out.summary),
                Style::default().fg(Color::Green),
            )));
        }
        Some(out) => {
            lines.push(Line::from(Span::styled(
                format!("✗ {}", out.summary),
                Style::default().fg(Color::Red),
            )));
            if let Some(detail) = &out.detail {
                // First line of detail only, to keep footer compact.
                let first = detail.lines().next().unwrap_or("").to_string();
                lines.push(Line::from(Span::styled(
                    first,
                    Style::default().fg(Color::Red),
                )));
            }
        }
        None => {}
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

pub fn differs(a: &Config, b: &Config) -> bool {
    // Compare every editable field across every configured runner.
    // Daemon-level fields (LogLevel etc.) are also exercised via
    // ``display_value``; their value is independent of ``runner_idx``,
    // so we just inspect them at idx 0 to avoid duplicate work.
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
