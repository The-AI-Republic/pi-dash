use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Tabs};
use std::io;
use std::time::Duration;

use super::ipc_client::TuiIpc;
use super::views::{approvals, config as config_view, general, runner_status, runs};
use crate::util::paths::Paths;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    /// Daemon-level surface: cloud URL, connection state, uptime, log
    /// settings, and the start/stop service controls. First tab because
    /// "is the daemon up and connected?" is the most common question
    /// when opening the TUI.
    General,
    /// List of runners hosted by this daemon. Inline `[a]` adds a
    /// new runner (against the locally-installed machine token);
    /// `[d]` deregisters one. Selection cycles with `j`/`k`.
    RunnerStatus,
    /// Per-runner config editor. The Runners-tab selection (or the
    /// picker bar) drives which runner this tab edits.
    Config,
    Runs,
    Approvals,
}

impl Tab {
    pub fn all() -> [Tab; 5] {
        [
            Tab::General,
            Tab::RunnerStatus,
            Tab::Config,
            Tab::Runs,
            Tab::Approvals,
        ]
    }

    pub fn label(&self) -> &'static str {
        match self {
            Tab::General => "General",
            // "Runners" (plural) honours the multi-runner shape of the
            // tab — one daemon may host many runners. Keep the enum
            // variant `RunnerStatus` to minimise diff against existing
            // code paths.
            Tab::RunnerStatus => "Runners",
            Tab::Config => "Config",
            Tab::Runs => "Runs",
            Tab::Approvals => "Approvals",
        }
    }

    /// Parse `--tab` values: accepts the canonical name or a 1-based
    /// index (`1`–`5`). Old singular `runner` and `status` aliases are
    /// kept for back-compat with scripts that already use them; the
    /// canonical token is now `runners`.
    pub fn parse_cli(raw: &str) -> Option<Tab> {
        let s = raw.trim().to_ascii_lowercase();
        match s.as_str() {
            "general" | "1" => Some(Tab::General),
            "runners" | "runner" | "runner-status" | "runner_status" | "status" | "2" => {
                Some(Tab::RunnerStatus)
            }
            "config" | "3" => Some(Tab::Config),
            "runs" | "4" => Some(Tab::Runs),
            "approvals" | "5" => Some(Tab::Approvals),
            _ => None,
        }
    }
}

pub struct AppState {
    pub tab: Tab,
    pub paths: Paths,
    pub ipc: TuiIpc,
    pub status: Option<crate::ipc::protocol::StatusSnapshot>,
    pub runs: Vec<crate::history::index::RunSummary>,
    pub approvals: Vec<crate::approval::router::ApprovalRecord>,
    /// Currently-on-disk config, decoded from `config.toml`. `None` means
    /// the file is missing (first-run state) — the Config tab switches into
    /// "Register with cloud" mode when that happens.
    pub config_loaded: Option<crate::config::schema::Config>,
    /// Working copy users edit in the Config tab. Kicked off as a clone of
    /// `config_loaded` and mutated in-place as fields get toggled / edited.
    /// `w` writes this to disk; `Esc` (in browse mode) discards it back to
    /// `config_loaded`.
    pub config_working: Option<crate::config::schema::Config>,
    /// `Some(buffer)` while the user is typing into a Text/U32 field. The
    /// buffer is seeded with the field's current stringified value; Enter
    /// commits it, Esc cancels. `None` is browse mode.
    pub config_edit_buffer: Option<String>,
    /// Transient single-line error from the last Enter-commit attempt
    /// (e.g. "expected a non-negative integer"). Cleared on the next
    /// successful commit or on tab switch.
    pub config_edit_error: Option<String>,
    pub config_error: Option<String>,
    /// Last `service::reload::restart_and_verify` result after a save. The
    /// Config tab surfaces this so users can see whether their edit broke
    /// the daemon.
    pub reload_outcome: Option<crate::service::reload::ReloadOutcome>,
    pub error: Option<String>,
    pub quit: bool,
    pub selected: usize,
    pub show_help: bool,
    pub confirm_stop: bool,
    pub confirm_exit: bool,
    pub confirm_exit_yes: bool,
    pub last_approval_count: usize,
    /// Last seen service state (`active`, `inactive`, `failed`, `unknown`).
    /// Populated by `service::detect().status()` on refresh.
    pub service_state: Option<String>,
    /// Transient banner shown on the Runner tab after a start/stop action:
    /// e.g. "starting service…" or "stop failed: …". Cleared on next refresh.
    pub service_action_msg: Option<String>,
    /// Inline registration form shown in the Config tab whenever `config.toml`
    /// is missing — replaces the old standalone onboarding wizard. Cleared
    /// after a successful register call.
    pub register_form: Option<RegisterForm>,
    /// Index into `config_working.runners` for the currently-focused runner
    /// in tabs that show per-runner data (Runs / Approvals / Config). Bare
    /// `pidash tui` on a single-runner install pins this to 0; multi-runner
    /// installs cycle with `<` / `>` or jump with digit keys. Clamped to
    /// `len() - 1` whenever the config reloads.
    pub runner_picker_idx: usize,
    /// Cursor position on the General tab (log level vs log retention).
    pub tab_general_field: super::views::general::GeneralField,
    /// Cursor position on the Runners tab (which runner is highlighted).
    /// Reused for the picker too — when the user selects a runner here,
    /// the picker on Config/Runs/Approvals stays in sync.
    pub runners_list_idx: usize,
    /// Inline add-runner form, shown over the Runners tab when the user
    /// presses `[a]`. None when not in the add flow.
    pub add_runner_form: Option<AddRunnerForm>,
    /// Pending remove-runner confirmation (carries the target name);
    /// `[y]` runs the deregister, anything else cancels.
    pub remove_runner_confirm: Option<String>,
}

/// Multi-step form for adding a runner from inside the TUI. The
/// project and pod fields are pickers — the form fetches the projects
/// (with their pods embedded) on open via `cli::token::list_projects`,
/// the user cycles selections with ↑/↓ within those fields, and the
/// pod picker re-anchors to the project's default pod whenever the
/// project selection changes.
#[derive(Debug, Clone, Default)]
pub struct AddRunnerForm {
    /// Free-text fields.
    pub name: String,
    pub working_dir: String,
    /// Cloud-fetched project list. `None` while loading; `Some(empty)`
    /// when the workspace has no projects (form surfaces an error and
    /// disables Submit).
    pub projects: Option<Vec<crate::cli::token::ProjectInfo>>,
    /// Index into `projects` for the highlighted project. Clamped on
    /// load so empty / single-project workspaces don't break.
    pub project_idx: usize,
    /// Index into the picked project's `pods` list. Reset to 0
    /// (default pod, which the cloud sorts first) on every project
    /// change.
    pub pod_idx: usize,
    /// 0 = name, 1 = project picker, 2 = pod picker, 3 = working_dir,
    /// 4 = Submit.
    pub focus: u8,
    pub busy: bool,
    pub error: Option<String>,
}

impl AddRunnerForm {
    pub fn field_count() -> u8 {
        5
    }

    /// Returns the mutable text buffer when focus is on a free-text
    /// field; `None` for picker / submit fields. The picker fields use
    /// their own ↑/↓ cycle handler instead.
    pub fn current_buffer_mut(&mut self) -> Option<&mut String> {
        match self.focus {
            0 => Some(&mut self.name),
            3 => Some(&mut self.working_dir),
            _ => None,
        }
    }

    pub fn selected_project(&self) -> Option<&crate::cli::token::ProjectInfo> {
        self.projects.as_ref()?.get(self.project_idx)
    }

    pub fn selected_pod(&self) -> Option<&crate::cli::token::PodInfo> {
        self.selected_project()?.pods.get(self.pod_idx)
    }

    /// Cycle the picker on the focused picker field. Used when the
    /// form is in picker focus (project or pod) and the user presses
    /// ↑/↓ inside the field.
    pub fn cycle_picker(&mut self, delta: isize) {
        let projects_len = self
            .projects
            .as_ref()
            .map(|p| p.len())
            .unwrap_or(0);
        match self.focus {
            1 if projects_len > 0 => {
                self.project_idx = wrap_idx(self.project_idx, delta, projects_len);
                // Reset pod selection when project changes; cloud
                // sorts default pod first so 0 is the right anchor.
                self.pod_idx = 0;
            }
            2 => {
                let pods_len = self
                    .selected_project()
                    .map(|p| p.pods.len())
                    .unwrap_or(0);
                if pods_len > 0 {
                    self.pod_idx = wrap_idx(self.pod_idx, delta, pods_len);
                }
            }
            _ => {}
        }
    }
}

/// Bounded wrap-around for picker indices. `len` is assumed > 0.
fn wrap_idx(cur: usize, delta: isize, len: usize) -> usize {
    let n = len as isize;
    let next = (cur as isize + delta).rem_euclid(n);
    next as usize
}

/// Three-field form (URL / token / name) plus a Register button. Focus is
/// an index 0..=3. All text input goes to whichever field is focused;
/// Up/Down or Tab moves focus; Enter advances focus for text fields and
/// submits when focus lands on the button.
#[derive(Clone)]
pub struct RegisterForm {
    pub cloud_url: String,
    pub token: String,
    pub name: String,
    /// 0 = cloud_url, 1 = token, 2 = name, 3 = Register button.
    pub focus: u8,
    pub busy: bool,
    pub error: Option<String>,
}

// Manual Debug that masks the token so a stray `{:?}` or `tracing::debug!`
// never prints the one-time registration secret in the clear.
impl std::fmt::Debug for RegisterForm {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RegisterForm")
            .field("cloud_url", &self.cloud_url)
            .field("token", &"[REDACTED]")
            .field("name", &self.name)
            .field("focus", &self.focus)
            .field("busy", &self.busy)
            .field("error", &self.error)
            .finish()
    }
}

impl RegisterForm {
    pub fn new(default_name: String) -> Self {
        Self {
            cloud_url: "http://localhost".to_string(),
            token: String::new(),
            name: default_name,
            focus: 0,
            busy: false,
            error: None,
        }
    }

    pub fn field_count() -> u8 {
        4
    }

    pub fn current_buffer_mut(&mut self) -> Option<&mut String> {
        match self.focus {
            0 => Some(&mut self.cloud_url),
            1 => Some(&mut self.token),
            2 => Some(&mut self.name),
            _ => None,
        }
    }
}

pub async fn run(paths: Paths, initial_tab: Tab) -> Result<()> {
    let ipc = TuiIpc {
        socket: paths.ipc_socket_path(),
        // Multi-runner picker is wired by Phase D.3; until then, leave
        // the selector empty so reads return the daemon-decided default
        // (single-runner installs work transparently; multi-runner
        // returns the union for read endpoints).
        selected_runner: None,
    };
    let mut state = AppState {
        tab: initial_tab,
        paths: paths.clone(),
        ipc,
        status: None,
        runs: Vec::new(),
        approvals: Vec::new(),
        config_loaded: None,
        config_working: None,
        config_edit_buffer: None,
        config_edit_error: None,
        config_error: None,
        reload_outcome: None,
        error: None,
        quit: false,
        selected: 0,
        show_help: false,
        confirm_stop: false,
        confirm_exit: false,
        confirm_exit_yes: true,
        last_approval_count: 0,
        service_state: None,
        service_action_msg: None,
        register_form: None,
        runner_picker_idx: 0,
        tab_general_field: super::views::general::GeneralField::default(),
        runners_list_idx: 0,
        add_runner_form: None,
        remove_runner_confirm: None,
    };

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    crossterm::execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let result = loop_ui(&mut terminal, &mut state).await;

    disable_raw_mode()?;
    crossterm::execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    result
}

async fn loop_ui(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    state: &mut AppState,
) -> Result<()> {
    refresh(state).await;
    let mut ticker = tokio::time::interval(Duration::from_millis(500));
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        terminal.draw(|f| draw(f, state))?;
        tokio::select! {
            _ = ticker.tick() => refresh(state).await,
            maybe = poll_event() => {
                if let Some(ev) = maybe {
                    handle_event(ev, state).await;
                }
            }
        }
        if state.quit {
            break;
        }
    }
    Ok(())
}

async fn poll_event() -> Option<Event> {
    // Off-thread crossterm poll.
    tokio::task::spawn_blocking(|| {
        if event::poll(Duration::from_millis(200)).ok()? {
            event::read().ok()
        } else {
            None
        }
    })
    .await
    .ok()
    .flatten()
}

/// Push the picker selection through to the IPC layer so per-runner read
/// endpoints (`runs`, `approvals`) scope to the focused runner. Looks up
/// the runner's *name* from the working config — the IPC selector is by
/// name, not by index. Falls back to `None` (daemon-default) when no
/// config is loaded yet, or if the index is out of bounds for a stale
/// configuration.
fn sync_picker_to_ipc(state: &mut AppState) {
    let total = state
        .config_working
        .as_ref()
        .map(|c| c.runners.len())
        .unwrap_or(0);
    if total == 0 {
        state.ipc.selected_runner = None;
        return;
    }
    if state.runner_picker_idx >= total {
        state.runner_picker_idx = total - 1;
    }
    state.ipc.selected_runner = state
        .config_working
        .as_ref()
        .and_then(|c| c.runners.get(state.runner_picker_idx))
        .map(|r| r.name.clone());
}

/// Move the runner picker by `delta` (signed), wrapping at ends. No-op when
/// only one runner is configured. Pushes the new selection into the IPC
/// scope and triggers a refresh so per-runner views update immediately.
async fn move_picker(state: &mut AppState, delta: isize) {
    let total = state
        .config_working
        .as_ref()
        .map(|c| c.runners.len())
        .unwrap_or(0);
    if total <= 1 {
        return;
    }
    let cur = state.runner_picker_idx as isize;
    let next = ((cur + delta).rem_euclid(total as isize)) as usize;
    state.runner_picker_idx = next;
    sync_picker_to_ipc(state);
    suppress_approval_alert_once(state);
    refresh(state).await;
}

/// Jump straight to runner index `idx` (0-based). Used by the digit-key
/// shortcuts in the picker bar. No-op when `idx` is out of range.
async fn jump_picker(state: &mut AppState, idx: usize) {
    let total = state
        .config_working
        .as_ref()
        .map(|c| c.runners.len())
        .unwrap_or(0);
    if total <= 1 || idx >= total {
        return;
    }
    state.runner_picker_idx = idx;
    sync_picker_to_ipc(state);
    suppress_approval_alert_once(state);
    refresh(state).await;
}

/// Disable the "new approval arrived" bell + auto-jump for one refresh
/// tick. We call this right before refreshing after a picker change:
/// switching from a 0-approval runner to a 5-approval runner would
/// otherwise look like "5 new approvals arrived" and steal focus to
/// the Approvals tab. Setting the high-water mark to `usize::MAX`
/// makes the `v.len() > was` comparison in `refresh` impossible for
/// this tick; refresh then resets it to the real count.
fn suppress_approval_alert_once(state: &mut AppState) {
    state.last_approval_count = usize::MAX;
}

async fn refresh(state: &mut AppState) {
    // Service state independent of IPC — the daemon may be down entirely,
    // in which case we still want to show "inactive" on the Runner tab.
    state.service_state = match crate::service::detect().status().await {
        Ok(s) if !s.is_empty() => Some(s),
        Ok(_) => Some("unknown".to_string()),
        Err(e) => Some(format!("error: {e}")),
    };

    match state.ipc.status().await {
        Ok(s) => state.status = Some(s),
        Err(e) => {
            state.status = None;
            state.error = Some(format!("status: {e}"));
        }
    }
    // Approvals are polled even off-tab so we can fire the bell + focus jump.
    if let Ok(v) = state.ipc.approvals().await {
        let was = state.last_approval_count;
        state.last_approval_count = v.len();
        if v.len() > was && state.tab != Tab::Approvals {
            // Bell + auto-focus.
            print!("\x07");
            state.tab = Tab::Approvals;
            state.selected = 0;
        }
        state.approvals = v;
    }
    match state.tab {
        Tab::Runs => {
            if let Ok(v) = state.ipc.runs().await {
                state.runs = v;
            }
        }
        Tab::Config => {
            // Direct file I/O — no daemon needed. load_config_opt swallows
            // NotFound and returns None so we can route into the register
            // sub-widget without an error banner.
            match crate::config::file::load_config_opt(&state.paths) {
                Ok(Some(cfg)) => {
                    state.config_loaded = Some(cfg.clone());
                    state.config_error = None;
                    // Seed the working copy on first load. Don't clobber if
                    // the user already has unsaved edits in flight — the
                    // 500 ms refresh tick shouldn't erase their work.
                    if state.config_working.is_none() {
                        state.config_working = Some(cfg);
                    }
                    // Clamp the picker into the now-known runner count and
                    // make sure the IPC selector matches the picked name.
                    sync_picker_to_ipc(state);
                }
                Ok(None) => {
                    state.config_loaded = None;
                    state.config_working = None;
                    state.config_error = None;
                    // Fresh machine — show the inline register form.
                    if state.register_form.is_none() {
                        state.register_form = Some(RegisterForm::new(default_hostname()));
                    }
                }
                Err(e) => {
                    state.config_loaded = None;
                    state.config_error = Some(format!("{e:#}"));
                }
            }
        }
        _ => {}
    }
}

async fn handle_event(ev: Event, state: &mut AppState) {
    if let Event::Key(key) = ev {
        if key.kind != KeyEventKind::Press {
            return;
        }
        // Help overlay consumes keys until dismissed.
        if state.show_help {
            if matches!(
                key.code,
                KeyCode::Esc | KeyCode::Char('?') | KeyCode::Char('q')
            ) {
                state.show_help = false;
            }
            return;
        }
        if state.confirm_exit {
            match (key.code, key.modifiers) {
                (KeyCode::Char('c'), KeyModifiers::CONTROL) => state.quit = true,
                (KeyCode::Enter, _) => {
                    if state.confirm_exit_yes {
                        state.quit = true;
                    } else {
                        state.confirm_exit = false;
                    }
                }
                (KeyCode::Char('y') | KeyCode::Char('Y'), _) => state.quit = true,
                (KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc, _) => {
                    state.confirm_exit = false;
                }
                (KeyCode::Left | KeyCode::Right | KeyCode::Char('h') | KeyCode::Char('l'), _) => {
                    state.confirm_exit_yes = !state.confirm_exit_yes;
                }
                _ => {}
            }
            return;
        }
        if state.confirm_stop {
            match key.code {
                KeyCode::Char('y') | KeyCode::Char('Y') => {
                    // Ask daemon to disconnect; then quit the TUI.
                    let _ = state
                        .ipc
                        .decide("__stop__", crate::cloud::protocol::ApprovalDecision::Accept)
                        .await
                        .ok();
                    state.quit = true;
                }
                _ => state.confirm_stop = false,
            }
            return;
        }
        // Remove-runner confirmation modal: y / Y commits, anything
        // else cancels. Same shape as confirm_stop above.
        if state.remove_runner_confirm.is_some() {
            match key.code {
                KeyCode::Char('y') | KeyCode::Char('Y') => {
                    submit_remove_runner(state).await;
                }
                _ => state.remove_runner_confirm = None,
            }
            return;
        }
        // Add-runner form modal (4 text fields + submit). Active on the
        // Runners tab when the user pressed `[a]`. We intercept all
        // typed input here so it lands in the focused field instead of
        // triggering global hotkeys.
        if state.add_runner_form.is_some() {
            match (key.code, key.modifiers) {
                (KeyCode::Char('c'), KeyModifiers::CONTROL) => {
                    state.confirm_exit = true;
                    state.confirm_exit_yes = true;
                    return;
                }
                (KeyCode::Esc, _) => {
                    state.add_runner_form = None;
                    return;
                }
                // Up/Down on a picker field cycles the choices; on a
                // text field it falls through to "previous/next field"
                // (so the existing arrow-key muscle memory still works
                // when typing). Tab / BackTab always change field.
                (KeyCode::Up, _) => {
                    if let Some(f) = state.add_runner_form.as_mut() {
                        if matches!(f.focus, 1 | 2) {
                            f.cycle_picker(-1);
                        } else {
                            add_runner_form_advance_focus(f, false);
                        }
                    }
                    return;
                }
                (KeyCode::Down, _) => {
                    if let Some(f) = state.add_runner_form.as_mut() {
                        if matches!(f.focus, 1 | 2) {
                            f.cycle_picker(1);
                        } else {
                            add_runner_form_advance_focus(f, true);
                        }
                    }
                    return;
                }
                (KeyCode::BackTab, _) => {
                    if let Some(f) = state.add_runner_form.as_mut() {
                        add_runner_form_advance_focus(f, false);
                    }
                    return;
                }
                (KeyCode::Tab, _) => {
                    if let Some(f) = state.add_runner_form.as_mut() {
                        add_runner_form_advance_focus(f, true);
                    }
                    return;
                }
                (KeyCode::Enter, _) => {
                    let submit =
                        matches!(state.add_runner_form.as_ref().map(|f| f.focus), Some(4));
                    if submit {
                        submit_add_runner_form(state).await;
                    } else if let Some(f) = state.add_runner_form.as_mut() {
                        add_runner_form_advance_focus(f, true);
                    }
                    return;
                }
                (KeyCode::Backspace, _) => {
                    if let Some(f) = state.add_runner_form.as_mut()
                        && let Some(buf) = f.current_buffer_mut()
                    {
                        buf.pop();
                    }
                    return;
                }
                (KeyCode::Char(c), mods) if !mods.contains(KeyModifiers::CONTROL) => {
                    if let Some(f) = state.add_runner_form.as_mut()
                        && let Some(buf) = f.current_buffer_mut()
                    {
                        buf.push(c);
                    }
                    return;
                }
                _ => return,
            }
        }
        // Config tab: inline Register form when there's no config yet.
        // This form covers what the old full-screen onboarding wizard did.
        // We intercept text / nav / submit keys but leave global shortcuts
        // (Ctrl+C to quit, 1–4 / h / l to switch tabs) to the main match
        // so the user can always escape.
        if state.tab == Tab::Config
            && state.register_form.is_some()
            && state.config_working.is_none()
        {
            match (key.code, key.modifiers) {
                (KeyCode::Char('c'), KeyModifiers::CONTROL) => {
                    state.confirm_exit = true;
                    state.confirm_exit_yes = true;
                    return;
                }
                (
                    KeyCode::Char('1')
                    | KeyCode::Char('2')
                    | KeyCode::Char('3')
                    | KeyCode::Char('4')
                    | KeyCode::Char('5'),
                    _,
                )
                | (KeyCode::Char('h') | KeyCode::Char('l') | KeyCode::Left | KeyCode::Right, _) => {
                    // Fall through to the global tab switcher so the user
                    // can leave the register screen without completing it.
                }
                (KeyCode::Up, _) | (KeyCode::BackTab, _) => {
                    if let Some(f) = state.register_form.as_mut() {
                        register_form_advance_focus(f, false);
                    }
                    return;
                }
                (KeyCode::Down, _) | (KeyCode::Tab, _) => {
                    if let Some(f) = state.register_form.as_mut() {
                        register_form_advance_focus(f, true);
                    }
                    return;
                }
                (KeyCode::Enter, _) => {
                    let submit = matches!(state.register_form.as_ref().map(|f| f.focus), Some(3));
                    if submit {
                        submit_register_form(state).await;
                    } else if let Some(f) = state.register_form.as_mut() {
                        register_form_advance_focus(f, true);
                    }
                    return;
                }
                (KeyCode::Esc, _) => {
                    // Esc on the register form clears the in-flight error
                    // (so the user can re-attempt) but doesn't cancel the
                    // whole screen — there's nothing to cancel to, the
                    // machine has no config yet.
                    if let Some(f) = state.register_form.as_mut() {
                        f.error = None;
                    }
                    return;
                }
                (KeyCode::Backspace, _) => {
                    if let Some(f) = state.register_form.as_mut()
                        && let Some(buf) = f.current_buffer_mut()
                    {
                        buf.pop();
                    }
                    return;
                }
                (KeyCode::Char(c), mods) if !mods.contains(KeyModifiers::CONTROL) => {
                    if let Some(f) = state.register_form.as_mut()
                        && let Some(buf) = f.current_buffer_mut()
                    {
                        buf.push(c);
                    }
                    return;
                }
                _ => return,
            }
        }

        // Config tab edit-buffer mode consumes text input: while the user
        // is typing into a text field, letters/backspace edit that buffer.
        // Enter commits, Esc cancels. Ctrl+C remains a universal escape
        // hatch — otherwise the user can get wedged with no way to quit
        // the TUI without Esc+q.
        if state.tab == Tab::Config && state.config_edit_buffer.is_some() {
            if let (KeyCode::Char('c'), m) = (key.code, key.modifiers)
                && m.contains(KeyModifiers::CONTROL)
            {
                state.confirm_exit = true;
                state.confirm_exit_yes = true;
                return;
            }
            match key.code {
                KeyCode::Enter => commit_config_edit(state),
                KeyCode::Esc => {
                    state.config_edit_buffer = None;
                    state.config_edit_error = None;
                }
                KeyCode::Backspace => {
                    if let Some(buf) = state.config_edit_buffer.as_mut() {
                        buf.pop();
                    }
                }
                KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                    if let Some(buf) = state.config_edit_buffer.as_mut() {
                        buf.push(c);
                    }
                }
                _ => {}
            }
            return;
        }
        match (key.code, key.modifiers) {
            (KeyCode::Char('q'), _) | (KeyCode::Char('c'), KeyModifiers::CONTROL) => {
                state.confirm_exit = true;
                state.confirm_exit_yes = true;
            }
            (KeyCode::Char('Q'), _) => state.confirm_stop = true,
            (KeyCode::Char('?'), _) => state.show_help = true,
            (KeyCode::Char('1'), _) => {
                state.tab = Tab::General;
                state.selected = 0;
                refresh(state).await;
            }
            (KeyCode::Char('2'), _) => {
                state.tab = Tab::RunnerStatus;
                state.selected = 0;
                refresh(state).await;
            }
            (KeyCode::Char('3'), _) => {
                state.tab = Tab::Config;
                state.selected = 0;
                refresh(state).await;
            }
            (KeyCode::Char('4'), _) => {
                state.tab = Tab::Runs;
                state.selected = 0;
                refresh(state).await;
            }
            (KeyCode::Char('5'), _) => {
                state.tab = Tab::Approvals;
                state.selected = 0;
                refresh(state).await;
            }
            (KeyCode::Char('j') | KeyCode::Down, _) => {
                if state.tab == Tab::Config {
                    let n = super::views::config::field_count();
                    if n > 0 {
                        state.selected = (state.selected + 1) % n;
                    }
                } else if state.tab == Tab::General {
                    state.tab_general_field = state.tab_general_field.next();
                } else if state.tab == Tab::RunnerStatus {
                    let n = state
                        .status
                        .as_ref()
                        .map(|s| s.runners.len())
                        .unwrap_or(0);
                    if n > 0 {
                        state.runners_list_idx = (state.runners_list_idx + 1) % n;
                        sync_picker_to_runner_idx(state);
                    }
                } else {
                    state.selected = state.selected.saturating_add(1);
                }
            }
            (KeyCode::Char('k') | KeyCode::Up, _) => {
                if state.tab == Tab::Config {
                    let n = super::views::config::field_count();
                    if n > 0 {
                        state.selected = if state.selected == 0 {
                            n - 1
                        } else {
                            state.selected - 1
                        };
                    }
                } else if state.tab == Tab::General {
                    state.tab_general_field = state.tab_general_field.prev();
                } else if state.tab == Tab::RunnerStatus {
                    let n = state
                        .status
                        .as_ref()
                        .map(|s| s.runners.len())
                        .unwrap_or(0);
                    if n > 0 {
                        state.runners_list_idx = if state.runners_list_idx == 0 {
                            n - 1
                        } else {
                            state.runners_list_idx - 1
                        };
                        sync_picker_to_runner_idx(state);
                    }
                } else {
                    state.selected = state.selected.saturating_sub(1);
                }
            }
            (KeyCode::Char('h') | KeyCode::Left, _) => {
                state.tab = match state.tab {
                    Tab::General => Tab::Approvals,
                    Tab::RunnerStatus => Tab::General,
                    Tab::Config => Tab::RunnerStatus,
                    Tab::Runs => Tab::Config,
                    Tab::Approvals => Tab::Runs,
                };
                state.selected = 0;
                refresh(state).await;
            }
            (KeyCode::Char('l') | KeyCode::Right, _) => {
                state.tab = match state.tab {
                    Tab::General => Tab::RunnerStatus,
                    Tab::RunnerStatus => Tab::Config,
                    Tab::Config => Tab::Runs,
                    Tab::Runs => Tab::Approvals,
                    Tab::Approvals => Tab::General,
                };
                state.selected = 0;
                refresh(state).await;
            }
            (KeyCode::Char('r'), _) => refresh(state).await,
            // Service controls migrated to the General tab. Pressing
            // `s`/`x` from the Runners tab is now a no-op so the user
            // can't accidentally start/stop the daemon while focused
            // on a runner row.
            (KeyCode::Char('s'), _) if state.tab == Tab::General => {
                run_service_action(state, ServiceAction::Start).await;
            }
            (KeyCode::Char('x'), _) if state.tab == Tab::General => {
                run_service_action(state, ServiceAction::Stop).await;
            }
            // Runners tab: [a] open add-runner form, [d] confirm remove
            // of the highlighted runner.
            (KeyCode::Char('a'), _) if state.tab == Tab::RunnerStatus => {
                state.add_runner_form = Some(AddRunnerForm {
                    working_dir: default_working_dir_for_new_runner(state),
                    ..AddRunnerForm::default()
                });
                // Kick off the project fetch immediately so the picker
                // is populated by the time the user tabs into it.
                load_projects_into_form(state).await;
            }
            (KeyCode::Char('d'), _) if state.tab == Tab::RunnerStatus => {
                if let Some(s) = &state.status
                    && let Some(r) = s.runners.get(state.runners_list_idx)
                {
                    state.remove_runner_confirm = Some(r.name.clone());
                }
            }
            // General tab field nav + edit.
            (KeyCode::Enter, _) if state.tab == Tab::General => {
                start_or_apply_general_field(state);
            }
            (KeyCode::Char('w'), _) if state.tab == Tab::General => {
                save_config(state).await;
            }
            (KeyCode::Esc, _) if state.tab == Tab::General => {
                if let Some(loaded) = state.config_loaded.clone() {
                    state.config_working = Some(loaded);
                }
                state.config_edit_error = None;
            }
            (KeyCode::Enter, _) if state.tab == Tab::Config => {
                start_or_apply_config_field(state);
            }
            (KeyCode::Char('w'), _) if state.tab == Tab::Config => {
                save_config(state).await;
            }
            (KeyCode::Esc, _) if state.tab == Tab::Config => {
                // Discard pending edits — roll working copy back to the
                // last-loaded snapshot. Leaves reload_outcome alone so the
                // user can still see whether their previous save succeeded.
                if let Some(loaded) = state.config_loaded.clone() {
                    state.config_working = Some(loaded);
                }
                state.config_edit_error = None;
            }
            // Runner picker: `<` and `>` cycle, digit keys jump (1-based in
            // the UI, 0-based internally). Skip on the Config tab while a
            // text input is active — that path is handled earlier and
            // returned. The picker is global so the user can switch runners
            // from any per-runner tab.
            (KeyCode::Char('<') | KeyCode::Char(','), _)
                if matches!(state.tab, Tab::Config | Tab::Runs | Tab::Approvals) =>
            {
                move_picker(state, -1).await;
            }
            (KeyCode::Char('>') | KeyCode::Char('.'), _)
                if matches!(state.tab, Tab::Config | Tab::Runs | Tab::Approvals) =>
            {
                move_picker(state, 1).await;
            }
            (KeyCode::Char(c @ '1'..='9'), KeyModifiers::ALT)
                if matches!(state.tab, Tab::Config | Tab::Runs | Tab::Approvals) =>
            {
                if let Some(d) = c.to_digit(10) {
                    jump_picker(state, (d as usize).saturating_sub(1)).await;
                }
            }
            (KeyCode::Char('a'), _) if state.tab == Tab::Approvals => {
                accept_selected(state, crate::cloud::protocol::ApprovalDecision::Accept).await;
            }
            (KeyCode::Char('A'), KeyModifiers::SHIFT) if state.tab == Tab::Approvals => {
                accept_selected(
                    state,
                    crate::cloud::protocol::ApprovalDecision::AcceptForSession,
                )
                .await;
            }
            (KeyCode::Char('d'), _) if state.tab == Tab::Approvals => {
                accept_selected(state, crate::cloud::protocol::ApprovalDecision::Decline).await;
            }
            _ => {}
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum ServiceAction {
    Start,
    Stop,
}

/// Enter key on the Config tab in browse mode: toggle booleans, cycle enums
/// in place; open a text-input buffer for Text/U32 fields seeded with the
/// current stringified value.
fn start_or_apply_config_field(state: &mut AppState) {
    use super::views::config as cfg_view;
    let Some(cfg) = state.config_working.as_mut() else {
        return;
    };
    if cfg_view::field_count() == 0 {
        return;
    }
    let idx = state.selected.min(cfg_view::field_count() - 1);
    let spec = cfg_view::field_at(idx);
    state.config_edit_error = None;
    let runner_idx = state.runner_picker_idx;
    match spec.kind {
        cfg_view::FieldKind::Bool => cfg_view::toggle_bool(cfg, spec.id, runner_idx),
        cfg_view::FieldKind::Enum(_) => cfg_view::cycle_enum(cfg, spec.id, runner_idx),
        cfg_view::FieldKind::Text | cfg_view::FieldKind::U32 => {
            state.config_edit_buffer =
                Some(cfg_view::display_value(cfg, spec.id, runner_idx));
        }
    }
}

/// Commit the pending `config_edit_buffer` to the working config. On parse /
/// validation failure leave the buffer in place so the user can fix it
/// without retyping everything.
fn commit_config_edit(state: &mut AppState) {
    use super::views::config as cfg_view;
    let Some(buf) = state.config_edit_buffer.take() else {
        return;
    };
    let Some(cfg) = state.config_working.as_mut() else {
        return;
    };
    if cfg_view::field_count() == 0 {
        return;
    }
    let idx = state.selected.min(cfg_view::field_count() - 1);
    let spec = cfg_view::field_at(idx);
    match cfg_view::set_text_value(cfg, spec.id, &buf, state.runner_picker_idx) {
        Ok(()) => {
            state.config_edit_error = None;
        }
        Err(e) => {
            state.config_edit_error = Some(e);
            state.config_edit_buffer = Some(buf);
        }
    }
}

/// `w` on the Config tab. Write the working copy to `config.toml`, then
/// run `restart_and_verify` so the user immediately sees whether their
/// edit broke the daemon. On file-write failure, surface it and leave the
/// daemon alone (no point restarting with unchanged bytes).
async fn save_config(state: &mut AppState) {
    let Some(cfg) = state.config_working.clone() else {
        return;
    };
    if let Err(e) = crate::config::file::write_config(&state.paths, &cfg) {
        state.config_edit_error = Some(format!("save failed: {e:#}"));
        return;
    }
    state.config_loaded = Some(cfg);
    state.config_edit_error = None;
    state.reload_outcome = Some(crate::service::reload::restart_and_verify(&state.paths).await);
    // Pull a fresh view of everything else now that the daemon restarted.
    refresh(state).await;
}

fn add_runner_form_advance_focus(form: &mut AddRunnerForm, forward: bool) {
    let n = AddRunnerForm::field_count();
    form.focus = if forward {
        (form.focus + 1) % n
    } else if form.focus == 0 {
        n - 1
    } else {
        form.focus - 1
    };
}

/// Suggest a working_dir for a new runner: the parent of the primary
/// runner's working_dir, joined with the new runner's name (filled in
/// once the user types it). Falls back to `~/.pidash/<placeholder>`
/// when no config exists yet so the field isn't empty.
fn default_working_dir_for_new_runner(state: &AppState) -> String {
    if let Some(cfg) = state.config_working.as_ref()
        && let Some(primary) = cfg.runners.first()
        && let Some(parent) = primary.workspace.working_dir.parent()
    {
        return parent.join("runner-new").display().to_string();
    }
    state
        .paths
        .default_working_dir()
        .join("runner-new")
        .display()
        .to_string()
}

/// Sync the runner picker (used by Config / Runs / Approvals) to a
/// specific row index from the Runners list. Keeps the two cursors
/// coherent so the user doesn't have to re-pick on every tab switch.
fn sync_picker_to_runner_idx(state: &mut AppState) {
    let total = state
        .config_working
        .as_ref()
        .map(|c| c.runners.len())
        .unwrap_or(0);
    if total == 0 {
        return;
    }
    let idx = state.runners_list_idx.min(total - 1);
    state.runner_picker_idx = idx;
    sync_picker_to_ipc(state);
}

/// General-tab Enter handler. Cycles the log level enum or opens the
/// edit buffer for retention_days. Mirrors the Config-tab pattern at
/// `start_or_apply_config_field`, but typed for the two daemon fields.
fn start_or_apply_general_field(state: &mut AppState) {
    let Some(cfg) = state.config_working.as_mut() else {
        return;
    };
    state.config_edit_error = None;
    match state.tab_general_field {
        super::views::general::GeneralField::LogLevel => {
            const LOG_LEVELS: &[&str] = &["trace", "debug", "info", "warn", "error"];
            let cur = LOG_LEVELS
                .iter()
                .position(|s| *s == cfg.daemon.log_level)
                .unwrap_or(2);
            cfg.daemon.log_level = LOG_LEVELS[(cur + 1) % LOG_LEVELS.len()].to_string();
        }
        super::views::general::GeneralField::LogRetentionDays => {
            state.config_edit_buffer = Some(cfg.daemon.log_retention_days.to_string());
        }
    }
}

/// Fetch the project list (with pods embedded) into the open add-runner
/// form. Called once when the form opens so the picker is populated by
/// the time the user tabs over to it. On error, the form's `error`
/// field is set and the picker stays empty — the user can retry by
/// closing and reopening the form (the cloud reachability is the
/// likely issue, and re-fetching automatically would just spin).
async fn load_projects_into_form(state: &mut AppState) {
    if state.add_runner_form.is_none() {
        return;
    }
    match crate::cli::token::list_projects(&state.paths).await {
        Ok(projects) => {
            if let Some(f) = state.add_runner_form.as_mut() {
                f.project_idx = 0;
                f.pod_idx = 0;
                f.projects = Some(projects);
            }
        }
        Err(e) => {
            if let Some(f) = state.add_runner_form.as_mut() {
                f.projects = Some(Vec::new());
                f.error = Some(format!("could not fetch projects: {e:#}"));
            }
        }
    }
}

/// Commit the add-runner form. Calls into `cli::token::add_runner`
/// (the library entry point — no stdout side effects) and then runs
/// `restart_and_verify` so the daemon picks up the new
/// `[[runner]]` block immediately. On cloud / validation failure we
/// keep the form visible with the error attached so the user can
/// retry without re-typing.
async fn submit_add_runner_form(state: &mut AppState) {
    let Some(form) = state.add_runner_form.as_mut() else {
        return;
    };
    let name = form.name.trim().to_string();
    let working_dir = form.working_dir.trim().to_string();
    // Picker selections — required since the user can't type a project.
    let Some(project) = form.selected_project().cloned() else {
        form.error = Some(
            "no projects available — verify `pidash token list-projects` works."
                .into(),
        );
        return;
    };
    // Pod is required-but-defaulted: every project has at least its
    // auto-created default pod. When the user hasn't moved off the
    // default we still send it explicitly so the cloud doesn't silently
    // resolve to a different pod if one is added in the meantime.
    let pod_name = form.selected_pod().map(|p| p.name.clone());

    if name.is_empty() {
        form.error = Some("name is required".into());
        return;
    }
    if let Err(e) = crate::util::runner_name::validate(&name) {
        form.error = Some(format!("invalid name: {e}"));
        return;
    }
    if working_dir.is_empty() {
        form.error = Some("working_dir is required".into());
        return;
    }

    form.busy = true;
    form.error = None;

    let args = crate::cli::token::AddRunnerArgs {
        name,
        project: project.identifier.clone(),
        pod: pod_name,
        working_dir: std::path::PathBuf::from(working_dir),
        agent: crate::config::schema::AgentKind::Codex,
    };

    let outcome = match crate::cli::token::add_runner(args, &state.paths).await {
        Ok(o) => o,
        Err(e) => {
            if let Some(f) = state.add_runner_form.as_mut() {
                f.busy = false;
                f.error = Some(format!("{e:#}"));
            }
            return;
        }
    };
    state.add_runner_form = None;
    // Restart the daemon so the new RunnerInstance is hosted; outcome
    // surfaces in the General tab's banner.
    state.reload_outcome =
        Some(crate::service::reload::restart_and_verify(&state.paths).await);
    refresh(state).await;
    // Aim the picker at the freshly-added runner so Config / Runs land
    // on it right away.
    if let Some(cfg) = state.config_working.as_ref()
        && let Some(idx) = cfg
            .runners
            .iter()
            .position(|r| r.runner_id == outcome.runner_id)
    {
        state.runners_list_idx = idx;
        state.runner_picker_idx = idx;
        sync_picker_to_ipc(state);
    }
}

/// Commit a `remove_runner_confirm` decision. Cleared either way; on
/// cloud / fs failure we surface the error in `reload_outcome`'s
/// banner so the user sees what broke without losing the modal flow.
async fn submit_remove_runner(state: &mut AppState) {
    let Some(name) = state.remove_runner_confirm.take() else {
        return;
    };
    let args = crate::cli::token::RemoveRunnerArgs { name: name.clone() };
    match crate::cli::token::remove_runner(args, &state.paths).await {
        Ok(_) => {
            state.reload_outcome =
                Some(crate::service::reload::restart_and_verify(&state.paths).await);
        }
        Err(e) => {
            state.reload_outcome = Some(crate::service::reload::ReloadOutcome {
                ok: false,
                summary: format!("remove {name:?} failed"),
                detail: Some(format!("{e:#}")),
                service_state: state
                    .service_state
                    .clone()
                    .unwrap_or_else(|| "unknown".into()),
            });
        }
    }
    // Clamp list index so we don't point past the end of the now
    // shorter runner list.
    if let Some(cfg) = state.config_working.as_ref() {
        let n = cfg.runners.len();
        if n > 0 {
            if state.runners_list_idx >= n {
                state.runners_list_idx = n - 1;
            }
            if state.runner_picker_idx >= n {
                state.runner_picker_idx = n - 1;
            }
        }
    }
    refresh(state).await;
}

fn register_form_advance_focus(form: &mut RegisterForm, forward: bool) {
    let n = RegisterForm::field_count();
    form.focus = if forward {
        (form.focus + 1) % n
    } else if form.focus == 0 {
        n - 1
    } else {
        form.focus - 1
    };
}

/// Submit the registration form. On success, writes config + credentials,
/// installs the service unit, and kicks the daemon — same end state as
/// `pidash configure --url ... --token ...` on the CLI.
async fn submit_register_form(state: &mut AppState) {
    let Some(form) = state.register_form.as_mut() else {
        return;
    };
    // Field-level validation first so we don't waste a cloud round trip.
    let cloud_url = form.cloud_url.trim().to_string();
    let token = form.token.trim().to_string();
    let name = form.name.trim().to_string();
    if let Err(e) = crate::cli::configure::validate_cloud_url(&cloud_url) {
        form.error = Some(format!("{e}"));
        return;
    }
    if token.is_empty() {
        form.error = Some("registration token is required".into());
        return;
    }
    if name.is_empty() {
        form.error = Some("runner name is required".into());
        return;
    }
    if let Err(e) = crate::util::runner_name::validate(&name) {
        form.error = Some(format!("invalid runner name: {e}"));
        return;
    }
    form.busy = true;
    form.error = None;

    // The TUI register form is on hold pending the multi-runner UX
    // refactor — the project picker hasn't landed yet. We surface the
    // limitation by sending the request with an empty project string,
    // which the cloud rejects with a clear "project is required" 400.
    // Users should run `pidash configure --url ... --token ... --project
    // <SLUG>` from the CLI for now.
    let req = crate::cloud::register::RegisterRequest {
        runner_name: name.clone(),
        os: std::env::consts::OS.to_string(),
        arch: std::env::consts::ARCH.to_string(),
        version: crate::RUNNER_VERSION.to_string(),
        protocol_version: crate::PROTOCOL_VERSION,
        project: String::new(),
        pod: None,
    };
    let resp = match crate::cloud::register::register(&cloud_url, &token, &req).await {
        Ok(r) => r,
        Err(e) => {
            if let Some(form) = state.register_form.as_mut() {
                form.busy = false;
                form.error = Some(format!("register failed: {e:#}"));
            }
            return;
        }
    };

    // Write config + credentials (same shape as cli::configure::execute's
    // happy path, minus the `extras` apply since the TUI form doesn't
    // collect advanced fields — user edits them in the Config tab after).
    let cfg = crate::config::schema::Config {
        version: 2,
        daemon: crate::config::schema::DaemonConfig {
            cloud_url: cloud_url.clone(),
            log_level: "info".to_string(),
            log_retention_days: 14,
        },
        runners: vec![crate::config::schema::RunnerConfig {
            name: name.clone(),
            runner_id: resp.runner_id,
            workspace_slug: resp.workspace_slug.clone(),
            project_slug: resp.project_identifier.clone(),
            pod_id: resp.pod_id,
            workspace: crate::config::schema::WorkspaceSection {
                working_dir: state.paths.default_working_dir(),
            },
            agent: Default::default(),
            codex: Default::default(),
            claude_code: Default::default(),
            approval_policy: Default::default(),
        }],
    };
    if let Err(e) = crate::config::file::write_config(&state.paths, &cfg) {
        if let Some(form) = state.register_form.as_mut() {
            form.busy = false;
            form.error = Some(format!("writing config.toml: {e:#}"));
        }
        return;
    }
    let creds = crate::config::schema::Credentials {
        token: None,
        runner_id: resp.runner_id,
        runner_secret: resp.runner_secret,
        api_token: resp.api_token,
        issued_at: chrono::Utc::now(),
    };
    if let Err(e) = crate::config::file::write_credentials(&state.paths, &creds) {
        if let Some(form) = state.register_form.as_mut() {
            form.busy = false;
            form.error = Some(format!("writing credentials.toml: {e:#}"));
        }
        return;
    }

    // Install + start the service so the runner actually runs after
    // registering. If unit-write fails, surface the error in the form and
    // keep the user in register mode — files are on disk, but without the
    // service unit the daemon won't come up, so dismissing the form would
    // leave the user in a broken state with no obvious retry path.
    let svc = crate::service::detect();
    if let Err(e) = svc.write_unit(&state.paths).await {
        if let Some(form) = state.register_form.as_mut() {
            form.busy = false;
            form.error = Some(format!("writing service unit: {e:#}"));
        }
        return;
    }
    let outcome = crate::service::reload::restart_and_verify(&state.paths).await;
    let outcome_ok = outcome.ok;
    state.reload_outcome = Some(outcome);

    if !outcome_ok {
        // Files + unit are on disk but the daemon didn't come up cleanly.
        // Keep the form so the footer banner + form error together tell
        // the user to re-check credentials or fix the environment; don't
        // transition to the editable Config view as if we succeeded.
        if let Some(form) = state.register_form.as_mut() {
            form.busy = false;
            form.error = Some(
                "service did not reach cloud-connected state — check footer banner for detail"
                    .into(),
            );
        }
        return;
    }

    // Transition out of register mode: the next refresh() will pick up
    // config.toml from disk and populate config_working.
    state.register_form = None;
    state.config_loaded = Some(cfg.clone());
    state.config_working = Some(cfg);
}

fn default_hostname() -> String {
    if let Ok(h) = std::env::var("HOSTNAME")
        && !h.is_empty()
    {
        return h;
    }
    nix::unistd::gethostname()
        .ok()
        .and_then(|os| os.into_string().ok())
        .unwrap_or_else(|| "runner".to_string())
}

async fn run_service_action(state: &mut AppState, action: ServiceAction) {
    let svc = crate::service::detect();
    let (verb_present, verb_past) = match action {
        ServiceAction::Start => ("starting", "started"),
        ServiceAction::Stop => ("stopping", "stopped"),
    };
    state.service_action_msg = Some(format!("{verb_present} service…"));
    let result = match action {
        ServiceAction::Start => svc.start().await,
        ServiceAction::Stop => svc.stop().await,
    };
    state.service_action_msg = Some(match result {
        Ok(()) => format!("service {verb_past}."),
        Err(e) => format!("service {verb_present} failed: {e:#}"),
    });
    // Pull fresh service/IPC state so the banner isn't contradicted by stale
    // cells on the next redraw.
    refresh(state).await;
}

async fn accept_selected(state: &mut AppState, decision: crate::cloud::protocol::ApprovalDecision) {
    if let Some(rec) = state.approvals.get(state.selected).cloned() {
        let _ = state.ipc.decide(&rec.approval_id, decision).await;
        if let Ok(v) = state.ipc.approvals().await {
            state.approvals = v;
            state.selected = 0;
        }
    }
}

fn draw(f: &mut ratatui::Frame<'_>, state: &AppState) {
    // Picker bar is only shown when there's more than one runner AND the
    // active tab is a per-runner tab (Config / Runs / Approvals). The
    // Runner-status tab already lists all runners inline so a picker would
    // be redundant.
    let show_picker = state
        .config_working
        .as_ref()
        .map(|c| c.runners.len() > 1)
        .unwrap_or(false)
        && matches!(state.tab, Tab::Config | Tab::Runs | Tab::Approvals);

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
        .split(f.area());

    let (tabs_idx, picker_idx, body_idx, hint_idx) = if show_picker {
        (0usize, Some(1usize), 2usize, 3usize)
    } else {
        (0, None, 1, 2)
    };

    let titles: Vec<Line<'_>> = Tab::all()
        .iter()
        .map(|t| Line::from(Span::styled(t.label(), Style::default())))
        .collect();
    let idx = match state.tab {
        Tab::General => 0,
        Tab::RunnerStatus => 1,
        Tab::Config => 2,
        Tab::Runs => 3,
        Tab::Approvals => 4,
    };
    let tabs = Tabs::new(titles)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Pi Dash Runner "),
        )
        .select(idx)
        .highlight_style(Style::default().add_modifier(Modifier::BOLD | Modifier::REVERSED));
    f.render_widget(tabs, layout[tabs_idx]);

    if let Some(pi) = picker_idx {
        f.render_widget(config_view::runner_picker_bar(state), layout[pi]);
    }

    match state.tab {
        Tab::General => general::render(f, layout[body_idx], state),
        Tab::RunnerStatus => runner_status::render(f, layout[body_idx], state),
        Tab::Config => config_view::render(f, layout[body_idx], state),
        Tab::Runs => runs::render(f, layout[body_idx], state),
        Tab::Approvals => approvals::render(f, layout[body_idx], state),
    }

    let hint = Line::from(Span::styled(
        " [1]General [2]Runners [3]Config [4]Runs [5]Approvals  h/l switch  j/k move  </> runner  r refresh  ?help  q exit ",
        Style::default().add_modifier(Modifier::DIM),
    ));
    f.render_widget(Paragraph::new(hint), layout[hint_idx]);

    if state.show_help {
        render_help(f);
    } else if state.confirm_exit {
        render_confirm_exit(f, state.confirm_exit_yes);
    } else if state.confirm_stop {
        render_confirm_stop(f);
    } else if let Some(name) = &state.remove_runner_confirm {
        render_confirm_remove_runner(f, name);
    } else if let Some(form) = &state.add_runner_form {
        render_add_runner_form(f, form);
    }
}

/// Centered modal for the add-runner form. Drawn over whatever tab the
/// user pressed `[a]` from. Five fields: name (text), project (picker
/// from cloud), pod (picker cascaded by project, default pod
/// pre-selected), working_dir (text), Submit. The pickers cycle with
/// ↑/↓ when focused; Tab / BackTab walks between fields.
fn render_add_runner_form(f: &mut ratatui::Frame<'_>, form: &AddRunnerForm) {
    use ratatui::widgets::Clear;

    let area = centered_rect(72, 65, f.area());
    f.render_widget(Clear, area);

    let project_value = match &form.projects {
        None => "(loading projects…)".to_string(),
        Some(list) if list.is_empty() => "(no projects available)".to_string(),
        Some(list) => {
            let p = &list[form.project_idx.min(list.len() - 1)];
            format!(
                "{} — {}   ({}/{})",
                p.identifier,
                p.name,
                form.project_idx + 1,
                list.len(),
            )
        }
    };
    let pod_value = match form.selected_project() {
        None => "(pick a project first)".to_string(),
        Some(p) if p.pods.is_empty() => "(no pods on project)".to_string(),
        Some(p) => {
            let pod = &p.pods[form.pod_idx.min(p.pods.len() - 1)];
            let tag = if pod.is_default { "  [default]" } else { "" };
            format!(
                "{}{}   ({}/{})",
                pod.name,
                tag,
                form.pod_idx + 1,
                p.pods.len(),
            )
        }
    };

    let mut lines: Vec<Line<'_>> = vec![
        Line::from(Span::styled(
            "Add a runner to this machine",
            Style::default()
                .fg(ratatui::style::Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::styled(
            "Project + pod fetched from the cloud. ↑/↓ cycles within picker fields; Tab moves between fields.",
            Style::default().add_modifier(Modifier::DIM),
        )),
        Line::raw(""),
        modal_field_line("Name        ", &form.name, form.focus == 0),
        modal_field_line("Project     ", &project_value, form.focus == 1),
        modal_field_line("Pod         ", &pod_value, form.focus == 2),
        modal_field_line("Working dir ", &form.working_dir, form.focus == 3),
        Line::raw(""),
    ];
    let submit_label = if form.busy {
        " Adding… "
    } else {
        " Submit "
    };
    let submit_style = if form.focus == 4 {
        Style::default()
            .fg(ratatui::style::Color::Black)
            .bg(ratatui::style::Color::Green)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default()
            .fg(ratatui::style::Color::Green)
            .add_modifier(Modifier::BOLD)
    };
    lines.push(Line::from(vec![
        Span::raw("   "),
        Span::styled(submit_label.to_string(), submit_style),
        Span::raw("   "),
        Span::styled(
            "Esc cancel",
            Style::default().add_modifier(Modifier::DIM),
        ),
    ]));
    if let Some(e) = &form.error {
        lines.push(Line::raw(""));
        lines.push(Line::from(Span::styled(
            e.clone(),
            Style::default().fg(ratatui::style::Color::Red),
        )));
    }

    let p = Paragraph::new(lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Add runner "),
        )
        .wrap(ratatui::widgets::Wrap { trim: false });
    f.render_widget(p, area);
}

fn modal_field_line(label: &str, value: &str, focused: bool) -> Line<'static> {
    let marker = if focused { "▶" } else { " " };
    let cursor = if focused { "▊" } else { "" };
    let value_style = if focused {
        Style::default()
            .fg(ratatui::style::Color::Yellow)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(ratatui::style::Color::White)
    };
    Line::from(vec![
        Span::styled(
            format!(" {marker} "),
            Style::default()
                .fg(if focused {
                    ratatui::style::Color::Cyan
                } else {
                    ratatui::style::Color::DarkGray
                })
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(format!("{} ", label)),
        Span::styled(format!("{value}{cursor}"), value_style),
    ])
}

fn render_confirm_remove_runner(f: &mut ratatui::Frame<'_>, name: &str) {
    use ratatui::widgets::Clear;
    let area = centered_rect(50, 30, f.area());
    f.render_widget(Clear, area);
    let body = Paragraph::new(vec![
        Line::from(vec![
            Span::raw("Remove runner "),
            Span::styled(
                format!("{name:?}"),
                Style::default()
                    .fg(ratatui::style::Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw("?"),
        ]),
        Line::raw(""),
        Line::from("Deregisters cloud-side, strips it from config.toml,"),
        Line::from("and deletes the local data directory. The other"),
        Line::from("runners on this machine keep running."),
        Line::raw(""),
        Line::from("[y] yes     [any other key] cancel"),
    ])
    .block(
        Block::default()
            .borders(Borders::ALL)
            .title(" Confirm remove "),
    );
    f.render_widget(body, area);
}

fn render_help(f: &mut ratatui::Frame<'_>) {
    use ratatui::layout::Alignment;
    use ratatui::widgets::Clear;

    let area = centered_rect(60, 60, f.area());
    f.render_widget(Clear, area);
    let body = Paragraph::new(vec![
        Line::from("Pi Dash Runner — TUI help"),
        Line::raw(""),
        Line::from("1–4       jump to view"),
        Line::from("h/l ←/→   prev/next view"),
        Line::from("j/k ↑/↓   move selection"),
        Line::from("↵     open detail"),
        Line::from("r     force refresh"),
        Line::from("s     start runner service  (General tab)"),
        Line::from("x     stop runner service   (General tab)"),
        Line::from("↵     edit field / toggle  (Config tab)"),
        Line::from("w     save + reload daemon (Config tab)"),
        Line::from("Esc   discard edits       (Config tab)"),
        Line::from("a     accept approval        (Approvals tab)"),
        Line::from("a     add a runner           (Runners tab)"),
        Line::from("A     accept for session     (Approvals tab)"),
        Line::from("d     decline                (Approvals tab)"),
        Line::from("d     remove highlighted runner (Runners tab)"),
        Line::raw(""),
        Line::from("Multi-runner picker (Config / Runs / Approvals):"),
        Line::from("</,    previous runner"),
        Line::from(">/.    next runner"),
        Line::from("Alt+N  jump to runner N (1–9)"),
        Line::from("q / Ctrl+C  quit TUI (asks for confirmation)"),
        Line::from("Q           stop daemon (asks for confirmation)"),
        Line::from("?     toggle this help"),
    ])
    .alignment(Alignment::Left)
    .block(Block::default().borders(Borders::ALL).title(" Help "));
    f.render_widget(body, area);
}

fn render_confirm_exit(f: &mut ratatui::Frame<'_>, yes_selected: bool) {
    use ratatui::widgets::Clear;

    let area = centered_rect(40, 20, f.area());
    f.render_widget(Clear, area);
    let sel = Style::default().add_modifier(Modifier::REVERSED);
    let unsel = Style::default().add_modifier(Modifier::DIM);
    let (yes_style, no_style) = if yes_selected {
        (sel, unsel)
    } else {
        (unsel, sel)
    };
    let body = Paragraph::new(vec![
        Line::from("Are you sure to exit?"),
        Line::raw(""),
        Line::from(vec![
            Span::raw("  "),
            Span::styled(" Yes ", yes_style),
            Span::raw("    "),
            Span::styled(" No ", no_style),
        ]),
        Line::raw(""),
        Line::from("↵ confirm   ←/→ switch   y / n / Esc"),
    ])
    .block(Block::default().borders(Borders::ALL).title(" Exit "));
    f.render_widget(body, area);
}

fn render_confirm_stop(f: &mut ratatui::Frame<'_>) {
    use ratatui::widgets::Clear;

    let area = centered_rect(40, 20, f.area());
    f.render_widget(Clear, area);
    let body = Paragraph::new(vec![
        Line::from("Stop the runner daemon?"),
        Line::raw(""),
        Line::from("Any active run will be cancelled."),
        Line::raw(""),
        Line::from("[y] yes     [any other key] cancel"),
    ])
    .block(Block::default().borders(Borders::ALL).title(" Confirm "));
    f.render_widget(body, area);
}

fn centered_rect(
    percent_x: u16,
    percent_y: u16,
    r: ratatui::layout::Rect,
) -> ratatui::layout::Rect {
    use ratatui::layout::{Constraint, Direction, Layout};
    let popup = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(r)[1];
    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup)[1]
}
