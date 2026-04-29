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
use super::views::{approvals, config as config_view, runner_status, runs};
use crate::util::paths::Paths;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    /// Daemon health + start/stop controls. First tab so `pidash tui` lands
    /// here and the user immediately sees whether the runner is up.
    RunnerStatus,
    /// Config editor, now the primary reason a user opens the TUI. Moved to
    /// position 2 because config editing is the most common flow once the
    /// service is running.
    Config,
    Runs,
    Approvals,
}

impl Tab {
    pub fn all() -> [Tab; 4] {
        [Tab::RunnerStatus, Tab::Config, Tab::Runs, Tab::Approvals]
    }

    pub fn label(&self) -> &'static str {
        match self {
            Tab::RunnerStatus => "Runner",
            Tab::Config => "Config",
            Tab::Runs => "Runs",
            Tab::Approvals => "Approvals",
        }
    }

    /// Parse `--tab` values: accepts the canonical name (`runner`, `config`,
    /// `runs`, `approvals`) or a 1-based index (`1`–`4`). Unknown input
    /// yields `None` so the caller can surface a clap-style error.
    pub fn parse_cli(raw: &str) -> Option<Tab> {
        let s = raw.trim().to_ascii_lowercase();
        match s.as_str() {
            "runner" | "runner-status" | "runner_status" | "status" | "1" => {
                Some(Tab::RunnerStatus)
            }
            "config" | "2" => Some(Tab::Config),
            "runs" | "3" => Some(Tab::Runs),
            "approvals" | "4" => Some(Tab::Approvals),
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
    refresh(state).await;
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
                    | KeyCode::Char('4'),
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
                state.tab = Tab::RunnerStatus;
                state.selected = 0;
                refresh(state).await;
            }
            (KeyCode::Char('2'), _) => {
                state.tab = Tab::Config;
                state.selected = 0;
                refresh(state).await;
            }
            (KeyCode::Char('3'), _) => {
                state.tab = Tab::Runs;
                state.selected = 0;
                refresh(state).await;
            }
            (KeyCode::Char('4'), _) => {
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
                } else {
                    state.selected = state.selected.saturating_sub(1);
                }
            }
            (KeyCode::Char('h') | KeyCode::Left, _) => {
                state.tab = match state.tab {
                    Tab::RunnerStatus => Tab::Approvals,
                    Tab::Config => Tab::RunnerStatus,
                    Tab::Runs => Tab::Config,
                    Tab::Approvals => Tab::Runs,
                };
                state.selected = 0;
                refresh(state).await;
            }
            (KeyCode::Char('l') | KeyCode::Right, _) => {
                state.tab = match state.tab {
                    Tab::RunnerStatus => Tab::Config,
                    Tab::Config => Tab::Runs,
                    Tab::Runs => Tab::Approvals,
                    Tab::Approvals => Tab::RunnerStatus,
                };
                state.selected = 0;
                refresh(state).await;
            }
            (KeyCode::Char('r'), _) => refresh(state).await,
            (KeyCode::Char('s'), _) if state.tab == Tab::RunnerStatus => {
                run_service_action(state, ServiceAction::Start).await;
            }
            (KeyCode::Char('x'), _) if state.tab == Tab::RunnerStatus => {
                run_service_action(state, ServiceAction::Stop).await;
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
        Tab::RunnerStatus => 0,
        Tab::Config => 1,
        Tab::Runs => 2,
        Tab::Approvals => 3,
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
        Tab::RunnerStatus => runner_status::render(f, layout[body_idx], state),
        Tab::Config => config_view::render(f, layout[body_idx], state),
        Tab::Runs => runs::render(f, layout[body_idx], state),
        Tab::Approvals => approvals::render(f, layout[body_idx], state),
    }

    let hint = Line::from(Span::styled(
        " [1]Runner [2]Config [3]Runs [4]Approvals  h/l switch  j/k move  </> runner  r refresh  ?help  q exit ",
        Style::default().add_modifier(Modifier::DIM),
    ));
    f.render_widget(Paragraph::new(hint), layout[hint_idx]);

    if state.show_help {
        render_help(f);
    } else if state.confirm_exit {
        render_confirm_exit(f, state.confirm_exit_yes);
    } else if state.confirm_stop {
        render_confirm_stop(f);
    }
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
        Line::from("s     start runner service  (Runner tab)"),
        Line::from("x     stop runner service   (Runner tab)"),
        Line::from("↵     edit field / toggle  (Config tab)"),
        Line::from("w     save + reload daemon (Config tab)"),
        Line::from("Esc   discard edits       (Config tab)"),
        Line::from("a     accept approval (once)"),
        Line::from("A     accept for session"),
        Line::from("d     decline"),
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
