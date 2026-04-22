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

use ratatui::layout::{Constraint, Direction, Layout, Rect};
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
    FieldSpec {
        id: FieldId::LogLevel,
        label: "level",
        section: "Logging",
        kind: FieldKind::Enum(LOG_LEVELS),
    },
    FieldSpec {
        id: FieldId::LogRetentionDays,
        label: "retention_days",
        section: "Logging",
        kind: FieldKind::U32,
    },
];

pub fn field_count() -> usize {
    FIELDS.len()
}

pub fn field_at(idx: usize) -> &'static FieldSpec {
    &FIELDS[idx.min(FIELDS.len() - 1)]
}

pub fn display_value(cfg: &Config, id: FieldId) -> String {
    match id {
        FieldId::RunnerName => cfg.runner.name.clone(),
        FieldId::WorkspaceWorkingDir => cfg.workspace.working_dir.display().to_string(),
        FieldId::AgentKind => match cfg.agent.kind {
            AgentKind::Codex => "codex".into(),
            AgentKind::ClaudeCode => "claude-code".into(),
        },
        FieldId::CodexBinary => cfg.codex.binary.clone(),
        FieldId::CodexModelDefault => cfg.codex.model_default.clone().unwrap_or_default(),
        FieldId::ClaudeBinary => cfg.claude_code.binary.clone(),
        FieldId::ClaudeModelDefault => cfg.claude_code.model_default.clone().unwrap_or_default(),
        FieldId::ApprovalAutoReadonly => {
            cfg.approval_policy.auto_approve_readonly_shell.to_string()
        }
        FieldId::ApprovalAutoWrites => {
            cfg.approval_policy.auto_approve_workspace_writes.to_string()
        }
        FieldId::ApprovalAutoNetwork => cfg.approval_policy.auto_approve_network.to_string(),
        FieldId::LogLevel => cfg.logging.level.clone(),
        FieldId::LogRetentionDays => cfg.logging.retention_days.to_string(),
    }
}

/// Commit an edited buffer to the config. Only valid for Text/U32 fields;
/// Bool/Enum fields use `toggle_bool` / `cycle_enum` instead. Returns a
/// user-facing error message on parse/validation failure.
pub fn set_text_value(cfg: &mut Config, id: FieldId, s: &str) -> Result<(), String> {
    match id {
        FieldId::RunnerName => {
            crate::util::runner_name::validate(s).map_err(|e| e.to_string())?;
            cfg.runner.name = s.to_string();
        }
        FieldId::WorkspaceWorkingDir => {
            if s.trim().is_empty() {
                return Err("working_dir cannot be empty".into());
            }
            cfg.workspace.working_dir = std::path::PathBuf::from(s);
        }
        FieldId::CodexBinary => {
            if s.trim().is_empty() {
                return Err("binary cannot be empty".into());
            }
            cfg.codex.binary = s.to_string();
        }
        FieldId::CodexModelDefault => {
            cfg.codex.model_default = if s.is_empty() {
                None
            } else {
                Some(s.to_string())
            };
        }
        FieldId::ClaudeBinary => {
            if s.trim().is_empty() {
                return Err("binary cannot be empty".into());
            }
            cfg.claude_code.binary = s.to_string();
        }
        FieldId::ClaudeModelDefault => {
            cfg.claude_code.model_default = if s.is_empty() {
                None
            } else {
                Some(s.to_string())
            };
        }
        FieldId::LogRetentionDays => {
            let n: u32 = s
                .parse()
                .map_err(|_| format!("expected a non-negative integer, got {s:?}"))?;
            cfg.logging.retention_days = n;
        }
        FieldId::AgentKind
        | FieldId::ApprovalAutoReadonly
        | FieldId::ApprovalAutoWrites
        | FieldId::ApprovalAutoNetwork
        | FieldId::LogLevel => {
            return Err("field is not a text input; use Enter to toggle/cycle instead".into());
        }
    }
    Ok(())
}

pub fn toggle_bool(cfg: &mut Config, id: FieldId) {
    match id {
        FieldId::ApprovalAutoReadonly => {
            let v = &mut cfg.approval_policy.auto_approve_readonly_shell;
            *v = !*v;
        }
        FieldId::ApprovalAutoWrites => {
            let v = &mut cfg.approval_policy.auto_approve_workspace_writes;
            *v = !*v;
        }
        FieldId::ApprovalAutoNetwork => {
            let v = &mut cfg.approval_policy.auto_approve_network;
            *v = !*v;
        }
        _ => {}
    }
}

pub fn cycle_enum(cfg: &mut Config, id: FieldId) {
    match id {
        FieldId::AgentKind => {
            cfg.agent.kind = match cfg.agent.kind {
                AgentKind::Codex => AgentKind::ClaudeCode,
                AgentKind::ClaudeCode => AgentKind::Codex,
            };
        }
        FieldId::LogLevel => {
            let cur = LOG_LEVELS
                .iter()
                .position(|s| *s == cfg.logging.level)
                .unwrap_or(2);
            cfg.logging.level = LOG_LEVELS[(cur + 1) % LOG_LEVELS.len()].to_string();
        }
        _ => {}
    }
}

// --- rendering ---------------------------------------------------------------

pub fn render(f: &mut ratatui::Frame<'_>, area: Rect, state: &AppState) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(0), Constraint::Length(6)])
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

    match (&state.config_working, &state.config_loaded) {
        (Some(working), loaded) => {
            let dirty = loaded.as_ref().map(|l| differs(l, working)).unwrap_or(true);
            let title = if dirty {
                " Configuration [unsaved changes] "
            } else {
                " Configuration "
            };
            let p = Paragraph::new(editable_lines(working, loaded, state))
                .block(Block::default().borders(Borders::ALL).title(title))
                .wrap(Wrap { trim: false });
            f.render_widget(p, chunks[0]);
        }
        (None, _) => {
            let p = Paragraph::new(register_form_lines(state))
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title(" Register with cloud "),
                )
                .wrap(Wrap { trim: false });
            f.render_widget(p, chunks[0]);
        }
    }

    f.render_widget(footer(state), chunks[1]);
}

fn register_form_lines(state: &AppState) -> Vec<Line<'static>> {
    let Some(form) = state.register_form.as_ref() else {
        // No form yet — refresh() will seed one next tick; show a hint.
        return vec![
            Line::from(Span::styled(
                "Loading…",
                Style::default().add_modifier(Modifier::DIM),
            )),
        ];
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

    lines.push(form_field_line("Cloud URL", &form.cloud_url, form.focus == 0, false));
    lines.push(form_field_line("Token", &mask_token(&form.token), form.focus == 1, true));
    lines.push(form_field_line("Runner name", &form.name, form.focus == 2, false));
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

fn form_field_line(label: &str, value: &str, focused: bool, _masked: bool) -> Line<'static> {
    let marker = if focused { "▶" } else { " " };
    let value_style = if focused {
        Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::White)
    };
    let cursor = if focused { "▊" } else { "" };
    Line::from(vec![
        Span::styled(
            format!(" {marker} "),
            Style::default()
                .fg(if focused { Color::Cyan } else { Color::DarkGray })
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(format!("{label:<14} ")),
        Span::styled(format!("{value}{cursor}"), value_style),
    ])
}

fn form_button_line(focused: bool, busy: bool) -> Line<'static> {
    let label = if busy { " Registering… " } else { " Register " };
    let style = if focused {
        Style::default()
            .fg(Color::Black)
            .bg(Color::Green)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)
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

fn editable_lines(
    working: &Config,
    loaded: &Option<Config>,
    state: &AppState,
) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    let selected_idx = state.selected.min(field_count().saturating_sub(1));

    // Runner section: name is editable, cloud_url + workspace_slug read-only.
    // cloud_url gets an extra hint line — since it's bound to the runner's
    // credentials (minted by that cloud), changing it locally would leave a
    // broken setup. Point the user at the register flow explicitly.
    lines.push(section_header("Runner"));
    lines.extend(render_editable_row(
        working,
        loaded,
        state,
        selected_idx,
        index_of(FieldId::RunnerName),
    ));
    lines.push(readonly_row("cloud_url", &working.runner.cloud_url));
    lines.push(readonly_hint(
        "to change, generate a new token in the cloud UI and re-run `pidash configure`",
    ));
    lines.push(readonly_row(
        "workspace_slug",
        working.runner.workspace_slug.as_deref().unwrap_or("-"),
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

    lines.push(section_header("Codex"));
    for id in [FieldId::CodexBinary, FieldId::CodexModelDefault] {
        lines.extend(render_editable_row(
            working,
            loaded,
            state,
            selected_idx,
            index_of(id),
        ));
    }
    lines.push(Line::raw(""));

    lines.push(section_header("Claude Code"));
    for id in [FieldId::ClaudeBinary, FieldId::ClaudeModelDefault] {
        lines.extend(render_editable_row(
            working,
            loaded,
            state,
            selected_idx,
            index_of(id),
        ));
    }
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
            working.approval_policy.allowlist_commands.len()
        ),
    ));
    lines.push(readonly_row(
        "denylist_commands",
        &format!(
            "{} entries (edit via CLI / hand-edit)",
            working.approval_policy.denylist_commands.len()
        ),
    ));
    lines.push(Line::raw(""));

    lines.push(section_header("Logging"));
    for id in [FieldId::LogLevel, FieldId::LogRetentionDays] {
        lines.extend(render_editable_row(
            working,
            loaded,
            state,
            selected_idx,
            index_of(id),
        ));
    }

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
    let displayed = if editing_here {
        format!("{}▊", state.config_edit_buffer.as_deref().unwrap_or(""))
    } else {
        display_value(working, spec.id)
    };

    let modified = loaded
        .as_ref()
        .map(|l| display_value(l, spec.id) != display_value(working, spec.id))
        .unwrap_or(true);

    let marker = if is_selected { "▶" } else { " " };
    let mod_marker = if modified { "●" } else { " " };

    let value_style = if editing_here {
        Style::default().fg(Color::Yellow)
    } else if is_selected {
        Style::default().fg(Color::White).add_modifier(Modifier::BOLD)
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
        Span::styled(
            "   ".to_string(),
            Style::default().fg(Color::DarkGray),
        ),
        Span::raw(format!("{label:<30} ")),
        Span::styled(
            value.to_string(),
            Style::default().fg(Color::DarkGray),
        ),
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

fn footer(state: &AppState) -> Paragraph<'_> {
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
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Status "),
        )
        .wrap(Wrap { trim: true })
}

fn index_of(id: FieldId) -> usize {
    FIELDS.iter().position(|f| f.id == id).expect("FieldId missing from FIELDS table")
}

fn differs(a: &Config, b: &Config) -> bool {
    FIELDS
        .iter()
        .any(|f| display_value(a, f.id) != display_value(b, f.id))
}
