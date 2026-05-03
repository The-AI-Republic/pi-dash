//! Add-runner modal.
//!
//! Five fields: name (text), project (picker fetched from cloud), pod
//! (picker cascaded by project), working_dir (text), Submit. Text
//! fields are real `TextArea`s so digits / `h` / `l` cannot escape
//! into tab switches while editing (Bug-3 invariant carries to the
//! modal too).

use std::sync::{Arc, Mutex};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Widget, Wrap};

use crate::tui::app::AppData;
use crate::tui::event::AppEvent;
use crate::tui::event_sender::AppEventSender;
use crate::tui::render::Renderable;
use crate::tui::view::{Cancellation, KeyHandled, View, ViewCompletion, ViewCtx};
use crate::tui::widgets::picker::{Picker, PickerOutcome, PickerRow};
use crate::tui::widgets::TextArea;
use crate::util::paths::Paths;

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

#[derive(Debug, Clone)]
enum ProjectsState {
    Loading,
    Loaded(Vec<crate::cloud::projects::ProjectInfo>),
    Failed(String),
}

pub struct AddRunnerView {
    name: TextArea,
    working_dir: TextArea,
    /// Shared with the project-fetch task spawned in `open()`. Read by
    /// render and key handlers; written exactly once by the task. The
    /// `Mutex` is held only briefly — there's no contention concern.
    projects: Arc<Mutex<ProjectsState>>,
    project_idx: usize,
    pod_idx: usize,
    focus: Focus,
    busy: bool,
    error: Option<String>,
    active_picker: Option<(PickerKind, Picker)>,
    complete: bool,
    completion: Option<ViewCompletion>,
}

impl AddRunnerView {
    pub fn open(data: &AppData, tx: AppEventSender, paths: Paths) -> Self {
        let projects = Arc::new(Mutex::new(ProjectsState::Loading));
        let projects_writer = projects.clone();
        // Fire-and-forget cloud fetch. On completion, write the result
        // through the shared Mutex and ping the bus so the modal
        // re-renders with the loaded list.
        tokio::spawn(async move {
            let result = crate::cloud::projects::list_projects(&paths).await;
            let next = match result {
                Ok(p) => ProjectsState::Loaded(p),
                Err(e) => ProjectsState::Failed(format!("{e:#}")),
            };
            *projects_writer.lock().expect("projects mutex poisoned") = next;
            tx.send(AppEvent::Refresh);
        });
        Self {
            name: TextArea::new().placeholder("runner name"),
            working_dir: TextArea::with_text(default_working_dir(data)),
            projects,
            project_idx: 0,
            pod_idx: 0,
            focus: Focus::Name,
            busy: false,
            error: None,
            active_picker: None,
            complete: false,
            completion: None,
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
        let projects_guard = self.projects.lock().expect("projects mutex poisoned");
        let projects = match &*projects_guard {
            ProjectsState::Loaded(p) if !p.is_empty() => p.clone(),
            _ => return,
        };
        drop(projects_guard);

        match self.focus {
            Focus::Project => {
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

        let projects_guard = self.projects.lock().expect("projects mutex poisoned");
        let project = match &*projects_guard {
            ProjectsState::Loading => {
                self.error = Some("projects still loading…".into());
                return;
            }
            ProjectsState::Failed(msg) => {
                self.error = Some(format!("project list unavailable: {msg}"));
                return;
            }
            ProjectsState::Loaded(list) => match list.get(self.project_idx).cloned() {
                Some(p) => p,
                None => {
                    self.error = Some(
                        "no projects available — verify cloud reachable and connection enrolled."
                            .into(),
                    );
                    return;
                }
            },
        };
        drop(projects_guard);
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
}

impl Renderable for AddRunnerView {
    fn render(&self, area: Rect, buf: &mut Buffer) {
        let modal = centered_rect(72, 65, area);
        Clear.render(modal, buf);

        let projects_guard = self.projects.lock().expect("projects mutex poisoned");
        let project_value = match &*projects_guard {
            ProjectsState::Loading => "(loading projects…)".to_string(),
            ProjectsState::Failed(msg) => format!("(load failed: {msg})"),
            ProjectsState::Loaded(list) if list.is_empty() => "(no projects available)".to_string(),
            ProjectsState::Loaded(list) => {
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
        let pod_value = match &*projects_guard {
            ProjectsState::Loaded(list) => match list.get(self.project_idx) {
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
            },
            _ => "(pick a project first)".to_string(),
        };
        drop(projects_guard);

        let mut lines: Vec<Line<'_>> = vec![
            Line::from(Span::styled(
                "Add a runner to this machine",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )),
            Line::from(Span::styled(
                "Project + pod fetched from the cloud. Enter / → opens picker; type to filter; Tab moves between fields.",
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
        // The actual picker submodal renders in `render_overlay` below
        // because `Picker::render` requires `&mut Self` + `&mut Frame`,
        // which the buffer-only Renderable path doesn't provide.
    }
}

impl View for AddRunnerView {
    fn handle_key(&mut self, key: KeyEvent, ctx: &mut ViewCtx<'_>) -> KeyHandled {
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

    fn render_overlay(&mut self, frame: &mut ratatui::Frame<'_>, area: Rect) {
        if let Some((_, picker)) = self.active_picker.as_mut() {
            picker.render(frame, area);
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
