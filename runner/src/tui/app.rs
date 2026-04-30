//! `App` — runner TUI orchestrator.
//!
//! Implements the architecture described in `.ai_design/tui_refactor/design.md`:
//!
//! - The terminal is owned by `Tui` (in `tui_runtime`); App never touches stdout.
//! - Two event types: physical `TuiEvent` (Key/Paste/Resize/Draw) and
//!   logical `AppEvent` (a bus enum). `App::run` is a three-source
//!   `select!` over `app_event_rx`, `tui_events.next()`, and a 500ms
//!   ticker.
//! - Mutation goes through the bus: widgets hold an `AppEventSender`,
//!   never a back-reference to `App`. The dispatcher (`dispatch_app_event`)
//!   is one flat match.
//! - Modality lives in `view_stack: Vec<Box<dyn View>>` — modals open by
//!   posting `AppEvent::PushView`; auto-pop on `is_complete()`.
//! - Focus *within* the base view is per-tab and derived (Pane enums on
//!   each Tab). No global focus pointer.
//! - Keybindings are declarative (`input::keymap`): a small registry of
//!   `(KeyEvent, Context) → Action`. Resolution is pure; active contexts
//!   are computed per dispatch. When a textarea is the focused child,
//!   the active-contexts list is `[TextInput]` only — digit / letter
//!   keys cannot escape into tab switches (Bug 3 fix).
//! - Render is a pure function of widget state: `Renderable::render`.
//!   Frames coalesce through `FrameRequester`; per-loop `draw()` is gone.
//! - IPC runs on spawned tasks per `Tick`, gated by per-concern
//!   in-flight flags. The 500ms App ticker is the sole cadence owner.

use std::time::Duration;

use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Tabs};
use tokio::sync::mpsc;

use super::event::AppEvent;
use super::event_sender::AppEventSender;
use super::input::keymap::{self, Action, Context, KeymapRegistry, Resolution};
use super::ipc_client::TuiIpc;
use super::tui_runtime::{FrameRequester, Tui, TuiEvent};
use super::view::tab::TabCtx;
use super::view::{KeyHandled, View, ViewCompletion, ViewCtx};
use super::views::{ApprovalsTab, GeneralTab, RunnerStatusTab, RunsTab};
use super::view::tab::{Tab as TabTrait, TabKind};
use crate::util::paths::Paths;

/// Public re-export so `cli/tui.rs` keeps using `tui::app::Tab`.
pub type Tab = TabKind;

/// Cross-cutting application data — the IPC snapshots, the loaded /
/// working config, transient banners. Per-tab cursors, picker indices,
/// and form text live on the corresponding tab struct, not here.
pub struct AppData {
    pub paths: Paths,
    pub ipc: TuiIpc,
    pub status: Option<crate::ipc::protocol::StatusSnapshot>,
    pub runs: Vec<crate::history::index::RunSummary>,
    pub approvals: Vec<crate::approval::router::ApprovalRecord>,
    pub config_loaded: Option<crate::config::schema::Config>,
    pub config_working: Option<crate::config::schema::Config>,
    /// Set whenever the most recent `set_text_value` call returned an
    /// error so the user can see what's wrong without losing their
    /// input.
    pub config_edit_error: Option<String>,
    pub config_error: Option<String>,
    pub reload_outcome: Option<crate::service::reload::ReloadOutcome>,
    pub error: Option<String>,
    pub last_approval_count: usize,
    pub service_state: Option<String>,
    pub service_action_msg: Option<String>,
    /// Index into `config_working.runners` chosen by the runner picker.
    pub runner_picker_idx: usize,
    /// In-flight gates so a slow IPC call doesn't pile up across ticks.
    pub ipc_status_in_flight: bool,
    pub ipc_approvals_in_flight: bool,
    pub ipc_runs_in_flight: bool,
    pub ipc_service_in_flight: bool,
    pub ipc_config_in_flight: bool,
    /// True for one tick after a manual picker change, suppressing the
    /// "new approvals" bell + auto-jump while the count rebases.
    pub suppress_approval_alert: bool,
}

impl AppData {
    pub fn new(paths: Paths) -> Self {
        let ipc = TuiIpc {
            socket: paths.ipc_socket_path(),
            selected_runner: None,
        };
        Self {
            paths,
            ipc,
            status: None,
            runs: Vec::new(),
            approvals: Vec::new(),
            config_loaded: None,
            config_working: None,
            config_edit_error: None,
            config_error: None,
            reload_outcome: None,
            error: None,
            last_approval_count: 0,
            service_state: None,
            service_action_msg: None,
            runner_picker_idx: 0,
            ipc_status_in_flight: false,
            ipc_approvals_in_flight: false,
            ipc_runs_in_flight: false,
            ipc_service_in_flight: false,
            ipc_config_in_flight: false,
            suppress_approval_alert: false,
        }
    }

    /// Resolve the current picker index to a runner *name* (the IPC
    /// scope key). Returns `None` for "use the daemon's default."
    pub fn picker_runner_name(&self) -> Option<String> {
        self.config_working
            .as_ref()
            .and_then(|c| c.runners.get(self.runner_picker_idx))
            .map(|r| r.name.clone())
    }

    pub fn sync_picker_to_ipc(&mut self) {
        let total = self
            .config_working
            .as_ref()
            .map(|c| c.runners.len())
            .unwrap_or(0);
        if total == 0 {
            self.ipc.selected_runner = None;
            return;
        }
        if self.runner_picker_idx >= total {
            self.runner_picker_idx = total - 1;
        }
        self.ipc.selected_runner = self.picker_runner_name();
    }
}

pub struct App {
    pub data: AppData,
    pub tab: TabKind,
    pub general: GeneralTab,
    pub runner_status: RunnerStatusTab,
    pub runs_tab: RunsTab,
    pub approvals_tab: ApprovalsTab,
    pub view_stack: Vec<Box<dyn View + Send>>,
    pub keymap: KeymapRegistry,
    pub event_tx: AppEventSender,
    pub frame: FrameRequester,
    pub quit: bool,
}

impl App {
    pub fn new(
        paths: Paths,
        initial_tab: TabKind,
        event_tx: AppEventSender,
        frame: FrameRequester,
    ) -> Self {
        let data = AppData::new(paths.clone());
        Self {
            data,
            tab: initial_tab,
            general: GeneralTab::new(),
            runner_status: RunnerStatusTab::new(),
            runs_tab: RunsTab::new(),
            approvals_tab: ApprovalsTab::new(),
            view_stack: Vec::new(),
            keymap: keymap::default_bindings::defaults(),
            event_tx,
            frame,
            quit: false,
        }
    }

    fn with_tab_ctx<R>(&mut self, f: impl FnOnce(&mut dyn TabTrait, &mut TabCtx<'_>) -> R) -> R {
        let paths = self.data.paths.clone();
        let mut ctx = TabCtx {
            tx: &self.event_tx,
            data: &mut self.data,
            keymap: &self.keymap,
            paths: &paths,
            frame: &self.frame,
        };
        match self.tab {
            TabKind::General => f(&mut self.general, &mut ctx),
            TabKind::RunnerStatus => f(&mut self.runner_status, &mut ctx),
            TabKind::Runs => f(&mut self.runs_tab, &mut ctx),
            TabKind::Approvals => f(&mut self.approvals_tab, &mut ctx),
        }
    }

    fn active_tab(&self) -> &dyn TabTrait {
        match self.tab {
            TabKind::General => &self.general,
            TabKind::RunnerStatus => &self.runner_status,
            TabKind::Runs => &self.runs_tab,
            TabKind::Approvals => &self.approvals_tab,
        }
    }

    /// Render the whole frame: tabs row + body + hint footer + any
    /// modal on top.
    pub fn render(&self, frame: &mut ratatui::Frame<'_>) {
        let area = frame.area();
        let buf = frame.buffer_mut();

        let show_picker = self
            .data
            .config_working
            .as_ref()
            .map(|c| c.runners.len() > 1)
            .unwrap_or(false)
            && matches!(self.tab, TabKind::Runs | TabKind::Approvals);

        let constraints: Vec<Constraint> = if show_picker {
            vec![
                Constraint::Length(3),
                Constraint::Length(3),
                Constraint::Min(0),
                Constraint::Length(1),
            ]
        } else {
            vec![
                Constraint::Length(3),
                Constraint::Min(0),
                Constraint::Length(1),
            ]
        };
        let layout = Layout::default()
            .direction(Direction::Vertical)
            .constraints(constraints)
            .split(area);

        let (tabs_idx, picker_idx, body_idx, hint_idx) = if show_picker {
            (0usize, Some(1usize), 2usize, 3usize)
        } else {
            (0, None, 1, 2)
        };

        let titles: Vec<Line<'_>> = TabKind::all()
            .iter()
            .map(|t| Line::from(Span::styled(t.label(), Style::default())))
            .collect();
        let tabs = Tabs::new(titles)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(" Pi Dash Runner "),
            )
            .select(self.tab.idx())
            .highlight_style(Style::default().add_modifier(Modifier::BOLD | Modifier::REVERSED));
        ratatui::widgets::Widget::render(tabs, layout[tabs_idx], buf);

        if let Some(pi) = picker_idx {
            ratatui::widgets::Widget::render(
                super::views::config::runner_picker_bar(&self.data),
                layout[pi],
                buf,
            );
        }

        // Body — delegate to the active tab.
        self.active_tab().render(layout[body_idx], buf, &self.data);

        let hint = Line::from(Span::styled(
            " [1]General [2]Runners [3]Runs [4]Approvals  h/l switch  j/k move  </> runner  r refresh  ?help  q exit ",
            Style::default().add_modifier(Modifier::DIM),
        ));
        ratatui::widgets::Widget::render(Paragraph::new(hint), layout[hint_idx], buf);

        // Modals on top, in stack order.
        for view in &self.view_stack {
            view.render(area, buf);
        }

        // Cursor placement: top-of-stack first, else active tab.
        let cursor = self
            .view_stack
            .last()
            .and_then(|v| v.cursor_pos(area))
            .or_else(|| self.active_tab().cursor_pos(layout[body_idx], &self.data));
        if let Some((x, y)) = cursor {
            frame.set_cursor_position((x, y));
        }
    }

    /// Logical event dispatcher — one flat match.
    pub async fn dispatch_app_event(&mut self, ev: AppEvent) -> Result<()> {
        match ev {
            AppEvent::Quit => self.quit = true,

            AppEvent::Tick => {
                self.spawn_status_poll();
                self.spawn_approvals_poll();
                self.spawn_service_poll();
                self.spawn_config_poll();
                if matches!(self.tab, TabKind::Runs) {
                    self.spawn_runs_poll();
                }
            }

            AppEvent::Bell => {
                // Output the BEL after the current frame so it doesn't
                // corrupt mid-render (the old code did `print!("\x07")`
                // inside the draw closure — that bug stays gone).
                use std::io::Write;
                let _ = write!(std::io::stdout(), "\x07");
                let _ = std::io::stdout().flush();
            }

            AppEvent::Refresh => {
                // Force-poll all four concerns regardless of in-flight
                // flags (user explicitly asked).
                self.data.ipc_status_in_flight = false;
                self.data.ipc_approvals_in_flight = false;
                self.data.ipc_runs_in_flight = false;
                self.data.ipc_service_in_flight = false;
                self.data.ipc_config_in_flight = false;
                self.spawn_status_poll();
                self.spawn_approvals_poll();
                self.spawn_service_poll();
                self.spawn_config_poll();
                if matches!(self.tab, TabKind::Runs) {
                    self.spawn_runs_poll();
                }
            }

            AppEvent::StatusUpdated(result) => {
                self.data.ipc_status_in_flight = false;
                match result {
                    Ok(s) => {
                        self.data.status = Some(s);
                        self.data.error = None;
                    }
                    Err(e) => {
                        self.data.status = None;
                        self.data.error = Some(format!("status: {e}"));
                    }
                }
                self.runner_status.reconcile(&self.data);
                self.frame.schedule_frame();
            }

            AppEvent::ApprovalsUpdated(result) => {
                self.data.ipc_approvals_in_flight = false;
                if let Ok(v) = result {
                    let was = self.data.last_approval_count;
                    let count = v.len();
                    self.data.last_approval_count = count;
                    if !self.data.suppress_approval_alert
                        && count > was
                        && self.tab != TabKind::Approvals
                    {
                        self.event_tx.send(AppEvent::Bell);
                        self.tab = TabKind::Approvals;
                        self.with_tab_ctx(|tab, ctx| tab.on_focus(ctx));
                    }
                    self.data.suppress_approval_alert = false;
                    self.data.approvals = v;
                    self.approvals_tab.reconcile(&self.data);
                }
                self.frame.schedule_frame();
            }

            AppEvent::RunsUpdated(result) => {
                self.data.ipc_runs_in_flight = false;
                if let Ok(v) = result {
                    self.data.runs = v;
                    self.runs_tab.reconcile(&self.data);
                }
                self.frame.schedule_frame();
            }

            AppEvent::ServiceStateUpdated(result) => {
                self.data.ipc_service_in_flight = false;
                self.data.service_state = match result {
                    Ok(s) if !s.is_empty() => Some(s),
                    Ok(_) => Some("unknown".to_string()),
                    Err(e) => Some(format!("error: {e}")),
                };
                self.frame.schedule_frame();
            }

            AppEvent::ConfigUpdated(result) => {
                self.data.ipc_config_in_flight = false;
                match result {
                    Ok(Some(cfg)) => {
                        self.data.config_loaded = Some(cfg.clone());
                        self.data.config_error = None;
                        if self.data.config_working.is_none() {
                            self.data.config_working = Some(cfg);
                        }
                        self.data.sync_picker_to_ipc();
                        // Disarm the registration form when config exists.
                        self.general.on_config_present(&self.data);
                    }
                    Ok(None) => {
                        self.data.config_loaded = None;
                        self.data.config_working = None;
                        self.data.config_error = None;
                        self.general.on_config_missing(&self.data);
                    }
                    Err(e) => {
                        self.data.config_loaded = None;
                        self.data.config_error = Some(format!("{e:#}"));
                    }
                }
                self.runner_status.reconcile(&self.data);
                self.frame.schedule_frame();
            }

            AppEvent::PushView(v) => {
                self.view_stack.push(v);
                self.frame.schedule_frame();
            }
            AppEvent::PopView => {
                self.view_stack.pop();
                self.frame.schedule_frame();
            }

            AppEvent::Approval { approval_id, decision } => {
                let ipc = self.data.ipc.clone();
                let tx = self.event_tx.clone();
                tokio::spawn(async move {
                    let _ = ipc.decide(&approval_id, decision).await;
                    match ipc.approvals().await {
                        Ok(v) => tx.send(AppEvent::ApprovalsUpdated(Ok(v))),
                        Err(e) => tx.send(AppEvent::ApprovalsUpdated(Err(format!("{e:#}")))),
                    }
                });
            }

            AppEvent::ServiceStart => self.spawn_service_action(true),
            AppEvent::ServiceStop => self.spawn_service_action(false),
            AppEvent::ServiceActionResult(msg) => {
                self.data.service_action_msg = Some(msg);
                self.event_tx.send(AppEvent::Refresh);
                self.frame.schedule_frame();
            }

            AppEvent::SubmitRegister => {
                self.spawn_register_submit();
            }
            AppEvent::SubmitAddRunner => {
                // The AddRunnerView posts this with its own state via
                // a separate event path. We never reach here today.
            }
            AppEvent::SubmitRemoveRunner(name) => {
                self.spawn_remove_runner(name);
            }
            AppEvent::SaveConfig => {
                self.spawn_save_config();
            }
            AppEvent::DiscardConfigEdits => {
                if let Some(loaded) = self.data.config_loaded.clone() {
                    self.data.config_working = Some(loaded);
                }
                self.data.config_edit_error = None;
                self.frame.schedule_frame();
            }
            AppEvent::ReloadOutcomeUpdated(o) => {
                self.data.reload_outcome = Some(o);
                self.event_tx.send(AppEvent::Refresh);
                self.frame.schedule_frame();
            }
            AppEvent::SelectRunner(idx) => {
                let total = self
                    .data
                    .config_working
                    .as_ref()
                    .map(|c| c.runners.len())
                    .unwrap_or(0);
                if total > 0 && idx < total {
                    self.data.runner_picker_idx = idx;
                    self.data.sync_picker_to_ipc();
                    self.data.suppress_approval_alert = true;
                    self.event_tx.send(AppEvent::Refresh);
                    self.frame.schedule_frame();
                }
            }
        }
        Ok(())
    }

    fn spawn_status_poll(&mut self) {
        if self.data.ipc_status_in_flight {
            return;
        }
        self.data.ipc_status_in_flight = true;
        let ipc = self.data.ipc.clone();
        let tx = self.event_tx.clone();
        tokio::spawn(async move {
            let result = ipc.status().await.map_err(|e| format!("{e:#}"));
            tx.send(AppEvent::StatusUpdated(result));
        });
    }

    fn spawn_approvals_poll(&mut self) {
        if self.data.ipc_approvals_in_flight {
            return;
        }
        self.data.ipc_approvals_in_flight = true;
        let ipc = self.data.ipc.clone();
        let tx = self.event_tx.clone();
        tokio::spawn(async move {
            let result = ipc.approvals().await.map_err(|e| format!("{e:#}"));
            tx.send(AppEvent::ApprovalsUpdated(result));
        });
    }

    fn spawn_runs_poll(&mut self) {
        if self.data.ipc_runs_in_flight {
            return;
        }
        self.data.ipc_runs_in_flight = true;
        let ipc = self.data.ipc.clone();
        let tx = self.event_tx.clone();
        tokio::spawn(async move {
            let result = ipc.runs().await.map_err(|e| format!("{e:#}"));
            tx.send(AppEvent::RunsUpdated(result));
        });
    }

    fn spawn_service_poll(&mut self) {
        if self.data.ipc_service_in_flight {
            return;
        }
        self.data.ipc_service_in_flight = true;
        let tx = self.event_tx.clone();
        tokio::spawn(async move {
            let result = crate::service::detect()
                .status()
                .await
                .map_err(|e| format!("{e:#}"));
            tx.send(AppEvent::ServiceStateUpdated(result));
        });
    }

    fn spawn_config_poll(&mut self) {
        if self.data.ipc_config_in_flight {
            return;
        }
        self.data.ipc_config_in_flight = true;
        let paths = self.data.paths.clone();
        let tx = self.event_tx.clone();
        tokio::spawn(async move {
            let result = crate::config::file::load_config_opt(&paths).map_err(|e| format!("{e:#}"));
            tx.send(AppEvent::ConfigUpdated(result));
        });
    }

    fn spawn_service_action(&mut self, start: bool) {
        let (verb_present, verb_past) = if start {
            ("starting", "started")
        } else {
            ("stopping", "stopped")
        };
        self.data.service_action_msg = Some(format!("{verb_present} service…"));
        let tx = self.event_tx.clone();
        tokio::spawn(async move {
            let svc = crate::service::detect();
            let result = if start {
                svc.start().await
            } else {
                svc.stop().await
            };
            let msg = match result {
                Ok(()) => format!("service {verb_past}."),
                Err(e) => format!("service {verb_present} failed: {e:#}"),
            };
            tx.send(AppEvent::ServiceActionResult(msg));
        });
    }

    fn spawn_register_submit(&mut self) {
        let Some(form) = self.general.register_form_snapshot() else {
            return;
        };
        let paths = self.data.paths.clone();
        let tx = self.event_tx.clone();
        self.general.set_register_busy(true, None);
        tokio::spawn(async move {
            match super::views::general::submit_register(&paths, form).await {
                Ok((cfg, outcome)) => {
                    tx.send(AppEvent::ConfigUpdated(Ok(Some(cfg))));
                    tx.send(AppEvent::ReloadOutcomeUpdated(outcome));
                }
                Err(e) => {
                    tx.send(AppEvent::ConfigUpdated(Err(e)));
                }
            }
        });
    }

    fn spawn_remove_runner(&mut self, name: String) {
        let paths = self.data.paths.clone();
        let tx = self.event_tx.clone();
        tokio::spawn(async move {
            let args = crate::cli::runner::RemoveArgs { name: name.clone() };
            let outcome = match crate::cli::runner::remove(args, &paths).await {
                Ok(_) => crate::service::reload::restart_and_verify(&paths).await,
                Err(e) => crate::service::reload::ReloadOutcome {
                    ok: false,
                    summary: format!("remove {name:?} failed"),
                    detail: Some(format!("{e:#}")),
                    service_state: "unknown".into(),
                },
            };
            tx.send(AppEvent::ReloadOutcomeUpdated(outcome));
        });
    }

    fn spawn_save_config(&mut self) {
        let Some(cfg) = self.data.config_working.clone() else {
            return;
        };
        let paths = self.data.paths.clone();
        let tx = self.event_tx.clone();
        if let Err(e) = crate::config::file::write_config(&paths, &cfg) {
            self.data.config_edit_error = Some(format!("save failed: {e:#}"));
            return;
        }
        self.data.config_loaded = Some(cfg);
        self.data.config_edit_error = None;
        tokio::spawn(async move {
            let outcome = crate::service::reload::restart_and_verify(&paths).await;
            tx.send(AppEvent::ReloadOutcomeUpdated(outcome));
        });
    }

    /// Five-layer key routing: stack-top → focused child → pane keymap →
    /// tab keymap → global keymap.
    pub fn dispatch_key(&mut self, key: KeyEvent) {
        // Layer 1: top-of-stack modal.
        if let Some(view) = self.view_stack.last_mut() {
            let paths = self.data.paths.clone();
            let mut ctx = ViewCtx {
                tx: &self.event_tx,
                keymap: &self.keymap,
                paths: &paths,
            };
            let handled = view.handle_key(key, &mut ctx);
            if view.is_complete() {
                let comp = view.completion();
                self.pop_with_completion(comp);
            }
            // While a modal is on top, never fall through to base
            // surface — global hotkeys are inert.
            if matches!(handled, KeyHandled::Consumed) || self.view_stack.last().is_some_and(|v| v.is_modal()) {
                self.frame.schedule_frame();
                return;
            }
        }

        // Layers 2-5: hand off to the active tab. Tab is responsible for
        // leaf-first routing inside its own subtree (focused TextArea
        // before any keymap), and consumes the key if any layer claims
        // it. If not consumed, we resolve via the keymap below.
        let tab_handled = self.with_tab_ctx(|tab, ctx| tab.handle_key(key, ctx));
        if matches!(tab_handled, KeyHandled::Consumed) {
            self.frame.schedule_frame();
            return;
        }

        // Layer 5: tab + global keymap. The tab's `active_contexts()`
        // already returns `[TextInput]` only when a textarea is focused,
        // so this layer is structurally inert in that case.
        let mut active = self.active_tab().active_contexts();
        // Append fall-through contexts (Tabs, Global) only when the
        // focused-child rule allows it — i.e. when TextInput is not
        // the only active context.
        let text_input_only = active == [Context::TextInput];
        if !text_input_only {
            active.push(Context::Tabs);
            active.push(Context::Global);
        }
        let resolution = keymap::resolve(&key, &active, &self.keymap);
        if let Resolution::Match(action) = resolution {
            self.dispatch_action(action);
            self.frame.schedule_frame();
        }
    }

    fn pop_with_completion(&mut self, completion: Option<ViewCompletion>) {
        self.view_stack.pop();
        if let Some(ViewCompletion::Accepted) = completion {
            while self
                .view_stack
                .last()
                .is_some_and(|v| v.dismiss_after_child_accept())
            {
                self.view_stack.pop();
            }
        }
    }

    fn dispatch_action(&mut self, action: Action) {
        match action {
            Action::Quit => {
                let v = super::views::modals::confirm::ConfirmExitView::new();
                self.event_tx.push_view(Box::new(v));
            }
            Action::QuitForce => self.quit = true,
            Action::StopDaemon => {
                let v = super::views::modals::confirm::ConfirmStopView::new();
                self.event_tx.push_view(Box::new(v));
            }
            Action::OpenHelp => {
                let v = super::views::modals::help::HelpView::new();
                self.event_tx.push_view(Box::new(v));
            }
            Action::Refresh => self.event_tx.send(AppEvent::Refresh),

            Action::NextTab => {
                let next = (self.tab.idx() + 1) % 4;
                self.set_tab(TabKind::from_idx(next).unwrap());
            }
            Action::PrevTab => {
                let prev = (self.tab.idx() + 3) % 4;
                self.set_tab(TabKind::from_idx(prev).unwrap());
            }
            Action::GoToTab(i) => {
                if let Some(t) = TabKind::from_idx(i) {
                    self.set_tab(t);
                }
            }

            Action::ListUp => {
                self.with_tab_ctx(|tab, ctx| {
                    tab.handle_key(KeyEvent::new(KeyCode::Up, crossterm::event::KeyModifiers::NONE), ctx);
                });
            }
            Action::ListDown => {
                self.with_tab_ctx(|tab, ctx| {
                    tab.handle_key(KeyEvent::new(KeyCode::Down, crossterm::event::KeyModifiers::NONE), ctx);
                });
            }
            Action::ListAccept => {
                self.with_tab_ctx(|tab, ctx| {
                    tab.handle_key(KeyEvent::new(KeyCode::Enter, crossterm::event::KeyModifiers::NONE), ctx);
                });
            }
            Action::ListCancel => {
                self.with_tab_ctx(|tab, ctx| {
                    tab.handle_key(KeyEvent::new(KeyCode::Esc, crossterm::event::KeyModifiers::NONE), ctx);
                });
            }

            Action::ApprovalAccept => {
                if let Some(rec) = self.approvals_tab.selected_record(&self.data) {
                    self.event_tx.send(AppEvent::Approval {
                        approval_id: rec.approval_id.clone(),
                        decision: crate::cloud::protocol::ApprovalDecision::Accept,
                    });
                }
            }
            Action::ApprovalAcceptForSession => {
                if let Some(rec) = self.approvals_tab.selected_record(&self.data) {
                    self.event_tx.send(AppEvent::Approval {
                        approval_id: rec.approval_id.clone(),
                        decision: crate::cloud::protocol::ApprovalDecision::AcceptForSession,
                    });
                }
            }
            Action::ApprovalDecline => {
                if let Some(rec) = self.approvals_tab.selected_record(&self.data) {
                    self.event_tx.send(AppEvent::Approval {
                        approval_id: rec.approval_id.clone(),
                        decision: crate::cloud::protocol::ApprovalDecision::Decline,
                    });
                }
            }

            Action::SettingsToggleFocus => {
                self.runner_status.toggle_focus();
            }

            Action::ServiceStart if matches!(self.tab, TabKind::General) => {
                self.event_tx.send(AppEvent::ServiceStart);
            }
            Action::ServiceStop if matches!(self.tab, TabKind::General) => {
                self.event_tx.send(AppEvent::ServiceStop);
            }
            Action::ServiceStart | Action::ServiceStop => { /* inert outside General */ }

            Action::OpenAddRunner if matches!(self.tab, TabKind::RunnerStatus) => {
                let v = super::views::modals::add_runner::AddRunnerView::open(&self.data);
                self.event_tx.push_view(Box::new(v));
            }
            Action::OpenAddRunner => {}
            Action::RemoveSelectedRunner if matches!(self.tab, TabKind::RunnerStatus) => {
                if let Some(name) = self.runner_status.selected_runner_name(&self.data) {
                    let v = super::views::modals::remove_runner::RemoveRunnerView::new(name);
                    self.event_tx.push_view(Box::new(v));
                }
            }
            Action::RemoveSelectedRunner => {}

            Action::RunnerPickerPrev => self.move_picker(-1),
            Action::RunnerPickerNext => self.move_picker(1),
            Action::RunnerPickerJump(i) => self.event_tx.send(AppEvent::SelectRunner(i)),

            Action::SaveConfig => self.event_tx.send(AppEvent::SaveConfig),
            Action::DiscardEdits => self.event_tx.send(AppEvent::DiscardConfigEdits),

            Action::FieldNext | Action::FieldPrev | Action::SubmitForm => {
                // Field navigation is handled by the focused tab's
                // own handler — these actions are reserved for future
                // declarative use.
            }
            Action::ConfirmYes | Action::ConfirmNo => {
                // Confirm dialogs are modals; if we're here, no modal
                // claimed the key, which is a bug.
            }
        }
    }

    fn set_tab(&mut self, t: TabKind) {
        self.tab = t;
        self.with_tab_ctx(|tab, ctx| tab.on_focus(ctx));
        self.event_tx.send(AppEvent::Refresh);
        self.frame.schedule_frame();
    }

    fn move_picker(&mut self, delta: isize) {
        let total = self
            .data
            .config_working
            .as_ref()
            .map(|c| c.runners.len())
            .unwrap_or(0);
        if total <= 1 {
            return;
        }
        let n = total as isize;
        let cur = self.data.runner_picker_idx as isize;
        let next = (cur + delta).rem_euclid(n) as usize;
        self.event_tx.send(AppEvent::SelectRunner(next));
    }

    pub fn handle_tui_event(&mut self, ev: TuiEvent) -> Result<bool> {
        match ev {
            TuiEvent::Key(key) => {
                self.dispatch_key(key);
                Ok(true)
            }
            TuiEvent::Paste(text) => {
                if let Some(view) = self.view_stack.last_mut() {
                    let paths = self.data.paths.clone();
                    let mut ctx = ViewCtx {
                        tx: &self.event_tx,
                        keymap: &self.keymap,
                        paths: &paths,
                    };
                    let _ = view.handle_paste(text, &mut ctx);
                } else {
                    self.with_tab_ctx(|tab, ctx| tab.handle_paste(text, ctx));
                }
                self.frame.schedule_frame();
                Ok(true)
            }
            TuiEvent::Resize(_, _) => {
                self.frame.schedule_frame();
                Ok(true)
            }
            TuiEvent::Draw => Ok(false),
        }
    }
}

pub async fn run(paths: Paths, initial_tab: TabKind) -> Result<()> {
    let mut tui = Tui::init()?;
    let frame_requester = tui.frame_requester().clone();

    let (tx, mut rx) = mpsc::unbounded_channel::<AppEvent>();
    let sender = AppEventSender::new(tx);

    let mut app = App::new(paths, initial_tab, sender.clone(), frame_requester.clone());
    // Initial fetch + first frame.
    sender.send(AppEvent::Refresh);
    app.with_tab_ctx(|tab, ctx| tab.on_focus(ctx));
    frame_requester.schedule_frame();

    let mut tui_events = tui.event_stream();
    let mut ticker = tokio::time::interval(Duration::from_millis(500));
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

    loop {
        tokio::select! {
            Some(ev) = rx.recv() => {
                app.dispatch_app_event(ev).await?;
            }
            Some(ev) = tui_events.next() => {
                if let TuiEvent::Draw = ev {
                    tui.draw(|f| app.render(f))?;
                } else {
                    let _ = app.handle_tui_event(ev);
                }
            }
            _ = ticker.tick() => {
                sender.send(AppEvent::Tick);
            }
        }
        if app.quit {
            break;
        }
    }

    Ok(())
}
