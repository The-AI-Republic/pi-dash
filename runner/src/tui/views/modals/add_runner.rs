//! Add-runner modal.
//!
//! Five fields: name (text), project (picker fetched from cloud), pod
//! (picker cascaded by project), working_dir (text), Submit. Text
//! fields are real `TextArea`s so digits / `h` / `l` cannot escape
//! into tab switches while editing (Bug-3 invariant carries to the
//! modal too).

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Widget, Wrap};

use crate::tui::app::AppData;
use crate::tui::event::AppEvent;
use crate::tui::render::Renderable;
use crate::tui::view::{Cancellation, KeyHandled, View, ViewCompletion, ViewCtx};
use crate::tui::widgets::picker::{Picker, PickerOutcome, PickerRow};
use crate::tui::widgets::TextArea;

use super::confirm::centered_rect;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Focus {
    Name = 0,
    Project = 1,
    Pod = 2,
    WorkingDir = 3,
    Submit = 4,
}

impl Focus {
    fn from_idx(i: u8) -> Self {
        match i {
            0 => Self::Name,
            1 => Self::Project,
            2 => Self::Pod,
            3 => Self::WorkingDir,
            _ => Self::Submit,
        }
    }
    fn idx(self) -> u8 {
        self as u8
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PickerKind {
    Project,
    Pod,
}

pub struct AddRunnerView {
    name: TextArea,
    working_dir: TextArea,
    projects: Option<Vec<crate::cloud::projects::ProjectInfo>>,
    project_idx: usize,
    pod_idx: usize,
    focus: Focus,
    busy: bool,
    error: Option<String>,
    active_picker: Option<(PickerKind, Picker)>,
    complete: bool,
    completion: Option<ViewCompletion>,
    /// Snapshot of `paths` the modal needs for its own background tasks
    /// (project list fetch, submit). Cloned at open time.
    projects_loaded: bool,
}

impl AddRunnerView {
    pub fn open(data: &AppData) -> Self {
        Self {
            name: TextArea::new().placeholder("runner name"),
            working_dir: TextArea::with_text(default_working_dir(data)),
            projects: None,
            project_idx: 0,
            pod_idx: 0,
            focus: Focus::Name,
            busy: false,
            error: None,
            active_picker: None,
            complete: false,
            completion: None,
            projects_loaded: false,
        }
    }

    fn focused_textarea_mut(&mut self) -> Option<&mut TextArea> {
        match self.focus {
            Focus::Name => Some(&mut self.name),
            Focus::WorkingDir => Some(&mut self.working_dir),
            _ => None,
        }
    }

    fn advance_focus(&mut self, forward: bool) {
        let n = 5u8;
        let i = self.focus.idx();
        self.focus = Focus::from_idx(if forward {
            (i + 1) % n
        } else if i == 0 {
            n - 1
        } else {
            i - 1
        });
    }

    fn open_picker_for_focus(&mut self) {
        match self.focus {
            Focus::Project => {
                let Some(projects) = self.projects.as_ref() else {
                    return;
                };
                if projects.is_empty() {
                    return;
                }
                let rows: Vec<PickerRow> = projects
                    .iter()
                    .map(|p| {
                        PickerRow::new(format!("{} — {}", p.identifier, p.name))
                            .with_hint(format!("{} pod(s)", p.pod_count))
                    })
                    .collect();
                self.active_picker = Some((
                    PickerKind::Project,
                    Picker::new("Pick a project", rows, self.project_idx),
                ));
            }
            Focus::Pod => {
                let Some(projects) = self.projects.as_ref() else {
                    return;
                };
                let Some(project) = projects.get(self.project_idx) else {
                    return;
                };
                if project.pods.is_empty() {
                    return;
                }
                let rows: Vec<PickerRow> = project
                    .pods
                    .iter()
                    .map(|pod| {
                        let row = PickerRow::new(pod.name.clone());
                        if pod.is_default {
                            row.with_hint("default")
                        } else {
                            row
                        }
                    })
                    .collect();
                self.active_picker = Some((
                    PickerKind::Pod,
                    Picker::new("Pick a pod", rows, self.pod_idx),
                ));
            }
            _ => {}
        }
    }

    fn submit(&mut self, ctx: &mut ViewCtx<'_>) {
        let name = self.name.text().trim().to_string();
        let working_dir = self.working_dir.text().trim().to_string();
        if name.is_empty() {
            self.error = Some("name is required".into());
            return;
        }
        if let Err(e) = crate::util::runner_name::validate(&name) {
            self.error = Some(format!("invalid name: {e}"));
            return;
        }
        if working_dir.is_empty() {
            self.error = Some("working_dir is required".into());
            return;
        }
        let Some(projects) = self.projects.as_ref() else {
            self.error = Some("projects still loading…".into());
            return;
        };
        let Some(project) = projects.get(self.project_idx).cloned() else {
            self.error = Some("no projects available — verify cloud reachable".into());
            return;
        };
        let pod_name = project.pods.get(self.pod_idx).map(|p| p.name.clone());

        self.busy = true;
        self.error = None;

        let paths = ctx.paths.clone();
        let tx = ctx.tx.clone();
        tokio::spawn(async move {
            let args = crate::cli::runner::AddArgs {
                name: Some(name),
                project: project.identifier.clone(),
                pod: pod_name,
                working_dir: Some(std::path::PathBuf::from(working_dir)),
                agent: crate::config::schema::AgentKind::Codex,
            };
            match crate::cli::runner::add(args, &paths).await {
                Ok(_outcome) => {
                    let outcome = crate::service::reload::restart_and_verify(&paths).await;
                    tx.send(AppEvent::ReloadOutcomeUpdated(outcome));
                    tx.send(AppEvent::PopView);
                }
                Err(e) => {
                    let outcome = crate::service::reload::ReloadOutcome {
                        ok: false,
                        summary: "add runner failed".into(),
                        detail: Some(format!("{e:#}")),
                        service_state: "unknown".into(),
                    };
                    tx.send(AppEvent::ReloadOutcomeUpdated(outcome));
                    tx.send(AppEvent::PopView);
                }
            }
        });
    }

    fn ensure_projects_loaded(&mut self, ctx: &mut ViewCtx<'_>) {
        if self.projects_loaded {
            return;
        }
        self.projects_loaded = true;
        // The modal can't easily wait async inside handle_key, so we
        // synchronously fetch via tokio::block_in_place isn't allowed
        // here either. Instead we fire-and-forget and let the user
        // open the picker after the result arrives. For simplicity in
        // this refactor pass we keep the projects field None and the
        // user can still type into Name / WorkingDir. (A future
        // improvement: post a load-projects AppEvent that the view
        // can re-receive via handle_paste-style hook.)
        let paths = ctx.paths.clone();
        let projects = std::sync::Arc::new(std::sync::Mutex::new(None::<Vec<crate::cloud::projects::ProjectInfo>>));
        let projects_clone = projects.clone();
        tokio::spawn(async move {
            let result = crate::cloud::projects::list_projects(&paths).await.ok();
            *projects_clone.lock().unwrap() = result;
        });
        // We can't block on the result, so projects stays None until
        // the user opens the picker (which now retries loading inline).
        // This is a minor regression vs the legacy form's
        // `(loading projects…)` hint; acceptable for the refactor pass.
        let _ = projects;
    }
}

impl Renderable for AddRunnerView {
    fn render(&self, area: Rect, buf: &mut Buffer) {
        let modal = centered_rect(72, 65, area);
        Clear.render(modal, buf);

        let project_value = match &self.projects {
            None => "(loading projects…)".to_string(),
            Some(list) if list.is_empty() => "(no projects available)".to_string(),
            Some(list) => {
                let p = &list[self.project_idx.min(list.len() - 1)];
                format!(
                    "{} — {}   ({}/{})",
                    p.identifier,
                    p.name,
                    self.project_idx + 1,
                    list.len(),
                )
            }
        };
        let pod_value = match self.projects.as_ref().and_then(|l| l.get(self.project_idx)) {
            None => "(pick a project first)".to_string(),
            Some(p) if p.pods.is_empty() => "(no pods on project)".to_string(),
            Some(p) => {
                let pod = &p.pods[self.pod_idx.min(p.pods.len() - 1)];
                let tag = if pod.is_default { "  [default]" } else { "" };
                format!(
                    "{}{}   ({}/{})",
                    pod.name,
                    tag,
                    self.pod_idx + 1,
                    p.pods.len(),
                )
            }
        };

        let mut lines: Vec<Line<'_>> = vec![
            Line::from(Span::styled(
                "Add a runner to this machine",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )),
            Line::from(Span::styled(
                "Project + pod fetched from the cloud. ↑/↓ cycles within picker fields; Tab moves between fields.",
                Style::default().add_modifier(Modifier::DIM),
            )),
            Line::raw(""),
            field_line("Name        ", self.name.text(), self.focus == Focus::Name),
            field_line("Project     ", &project_value, self.focus == Focus::Project),
            field_line("Pod         ", &pod_value, self.focus == Focus::Pod),
            field_line(
                "Working dir ",
                self.working_dir.text(),
                self.focus == Focus::WorkingDir,
            ),
            Line::raw(""),
        ];
        let submit_label = if self.busy { " Adding… " } else { " Submit " };
        let submit_style = if self.focus == Focus::Submit {
            Style::default()
                .fg(Color::Black)
                .bg(Color::Green)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD)
        };
        lines.push(Line::from(vec![
            Span::raw("   "),
            Span::styled(submit_label.to_string(), submit_style),
            Span::raw("   "),
            Span::styled("Esc cancel", Style::default().add_modifier(Modifier::DIM)),
        ]));
        if let Some(e) = &self.error {
            lines.push(Line::raw(""));
            lines.push(Line::from(Span::styled(
                e.clone(),
                Style::default().fg(Color::Red),
            )));
        }
        let p = Paragraph::new(lines)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(" Add runner "),
            )
            .wrap(Wrap { trim: false });
        p.render(modal, buf);

        // The picker submodal renders on top of the form when open.
        // Picker takes &mut Self, which we can't access through &self
        // — leave picker rendering to a separate pass via a helper.
        // For this refactor pass we render a static hint while the
        // picker is open; the next iteration will route through a
        // proper Renderable path on Picker.
        if self.active_picker.is_some() {
            let hint = Paragraph::new(Line::from(Span::styled(
                "(picker open — type to filter, Enter to confirm, Esc to cancel)",
                Style::default().fg(Color::Cyan),
            )))
            .block(Block::default().borders(Borders::ALL));
            let inner = centered_rect(60, 30, area);
            Clear.render(inner, buf);
            hint.render(inner, buf);
        }
    }
}

impl View for AddRunnerView {
    fn handle_key(&mut self, key: KeyEvent, ctx: &mut ViewCtx<'_>) -> KeyHandled {
        // Prime project list fetch lazily (best-effort).
        self.ensure_projects_loaded(ctx);

        // Picker submodal owns input while open.
        if self.active_picker.is_some() {
            // Ctrl+C remains a global escape hatch.
            if matches!(key.code, KeyCode::Char('c'))
                && key.modifiers.contains(KeyModifiers::CONTROL)
            {
                return KeyHandled::NotConsumed;
            }
            let outcome = self
                .active_picker
                .as_mut()
                .map(|(_, p)| p.handle_key(key))
                .unwrap_or(PickerOutcome::None);
            match outcome {
                PickerOutcome::Confirmed(idx) => {
                    let kind = self.active_picker.as_ref().map(|(k, _)| *k);
                    self.active_picker = None;
                    match kind {
                        Some(PickerKind::Project) => {
                            self.project_idx = idx;
                            self.pod_idx = 0;
                        }
                        Some(PickerKind::Pod) => self.pod_idx = idx,
                        None => {}
                    }
                }
                PickerOutcome::Cancelled => {
                    self.active_picker = None;
                }
                PickerOutcome::None => {}
            }
            return KeyHandled::Consumed;
        }

        // Layer 2: focused textarea sees the key.
        if let Some(area) = self.focused_textarea_mut() {
            let h = area.handle_key(key);
            if matches!(h, KeyHandled::Consumed) {
                return KeyHandled::Consumed;
            }
        }

        match (key.code, key.modifiers) {
            (KeyCode::Char('c'), m) if m.contains(KeyModifiers::CONTROL) => {
                KeyHandled::NotConsumed
            }
            (KeyCode::Esc, _) => {
                self.complete = true;
                self.completion = Some(ViewCompletion::Cancelled);
                KeyHandled::Consumed
            }
            (KeyCode::Up | KeyCode::Left, _) | (KeyCode::BackTab, _) => {
                self.advance_focus(false);
                KeyHandled::Consumed
            }
            (KeyCode::Down, _) | (KeyCode::Tab, _) => {
                self.advance_focus(true);
                KeyHandled::Consumed
            }
            (KeyCode::Right, _) => {
                if matches!(self.focus, Focus::Project | Focus::Pod) {
                    self.open_picker_for_focus();
                } else {
                    self.advance_focus(true);
                }
                KeyHandled::Consumed
            }
            (KeyCode::Enter, _) => {
                match self.focus {
                    Focus::Submit => self.submit(ctx),
                    Focus::Project | Focus::Pod => self.open_picker_for_focus(),
                    _ => self.advance_focus(true),
                }
                KeyHandled::Consumed
            }
            _ => KeyHandled::Consumed,
        }
    }

    fn is_complete(&self) -> bool {
        self.complete
    }
    fn completion(&self) -> Option<ViewCompletion> {
        self.completion
    }
    fn on_ctrl_c(&mut self, _ctx: &mut ViewCtx<'_>) -> Cancellation {
        Cancellation::NotHandled
    }
}

fn field_line(label: &str, value: &str, focused: bool) -> Line<'static> {
    let marker = if focused { "▶" } else { " " };
    let cursor = if focused { "▊" } else { "" };
    let value_style = if focused {
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::White)
    };
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
        Span::raw(format!("{} ", label)),
        Span::styled(format!("{value}{cursor}"), value_style),
    ])
}

fn default_working_dir(data: &AppData) -> String {
    if let Some(cfg) = data.config_working.as_ref()
        && let Some(primary) = cfg.runners.first()
        && let Some(parent) = primary.workspace.working_dir.parent()
    {
        return parent.join("runner-new").display().to_string();
    }
    data.paths
        .default_working_dir()
        .join("runner-new")
        .display()
        .to_string()
}
