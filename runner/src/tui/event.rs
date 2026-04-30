//! `AppEvent` — the application-level message bus.
//!
//! Doc shape borrowed from codex-rs (`app_event.rs`): a single enum
//! posted by every widget and every background task; the dispatcher in
//! `app::dispatch_app_event` runs a flat match on it.
//!
//! `TuiEvent` (in `tui_runtime::event_stream`) is the *physical* event
//! type — Key/Paste/Resize/Draw. `AppEvent` here is *logical*: things
//! the application does in response, plus background-task results.
//!
//! Widgets never call methods on `App`; they post `AppEvent`s through
//! an `AppEventSender` clone and the dispatcher re-enters from the
//! top. This is what makes mutation orderable and lets us hold
//! `&mut state` inside a single `select!` arm at a time.

use crate::approval::router::ApprovalRecord;
use crate::cloud::protocol::ApprovalDecision;
use crate::config::schema::Config;
use crate::history::index::RunSummary;
use crate::ipc::protocol::StatusSnapshot;
use crate::service::reload::ReloadOutcome;

use super::view::View;

/// Result of a background IPC poll. The `Option<String>` carries an
/// error description when the call failed; `None` for the success
/// path.
pub enum AppEvent {
    /// User-requested shutdown (already past the confirm modal). The
    /// run loop breaks on this.
    Quit,

    /// 500ms ticker. The dispatcher decides what to refresh based on
    /// the current tab + in-flight gates (`§5.7`).
    Tick,

    /// Periodic approval-bell side effect. Posted *after* the next
    /// draw so it doesn't corrupt the buffer mid-render.
    Bell,

    /// Background IPC results.
    StatusUpdated(Result<StatusSnapshot, String>),
    ApprovalsUpdated(Result<Vec<ApprovalRecord>, String>),
    RunsUpdated(Result<Vec<RunSummary>, String>),
    /// Service-state polled separately (`systemctl is-active …`).
    ServiceStateUpdated(Result<String, String>),
    /// Result of reading `config.toml` — `None` for the missing-file
    /// case (drives the inline register form).
    ConfigUpdated(Result<Option<Config>, String>),

    /// View-stack manipulation. Any view can request these via the
    /// `AppEventSender`; the dispatcher applies them at a clean
    /// boundary so the borrow on `&mut self.view_stack` doesn't
    /// overlap a `view.handle_key` call.
    PushView(Box<dyn View + Send>),
    PopView,

    /// Approval decision originated from the Approvals tab. The
    /// dispatcher forwards to `ipc.decide` on a spawned task and then
    /// re-polls approvals.
    Approval {
        approval_id: String,
        decision: ApprovalDecision,
    },

    /// Service start/stop, posted by the General-tab hotkeys. Runs on
    /// a spawned task; result lands in `ServiceStateUpdated` and
    /// `ServiceActionResult`.
    ServiceStart,
    ServiceStop,
    ServiceActionResult(String),

    /// User asked for an immediate IPC refresh (`r` hotkey).
    Refresh,

    /// Submit the current register form. Form contents are pulled
    /// from the GeneralTab at handle time.
    SubmitRegister,
    /// Submit the open AddRunnerView.
    SubmitAddRunner,
    /// Confirm-remove the currently-highlighted runner.
    SubmitRemoveRunner(String),
    /// Persist the working config and run `restart_and_verify`.
    SaveConfig,
    /// Clone `config_loaded` into `config_working` (Esc on the
    /// editable tabs).
    DiscardConfigEdits,
    /// Result of restart_and_verify after a save.
    ReloadOutcomeUpdated(ReloadOutcome),

    /// IPC-scoped runner-picker change. `runners[idx]` becomes the
    /// scope for read endpoints; `IpcStatusUpdated` etc. will pick it
    /// up on the next refresh.
    SelectRunner(usize),
}

impl std::fmt::Debug for AppEvent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AppEvent::Quit => f.write_str("Quit"),
            AppEvent::Tick => f.write_str("Tick"),
            AppEvent::Bell => f.write_str("Bell"),
            AppEvent::StatusUpdated(_) => f.write_str("StatusUpdated"),
            AppEvent::ApprovalsUpdated(_) => f.write_str("ApprovalsUpdated"),
            AppEvent::RunsUpdated(_) => f.write_str("RunsUpdated"),
            AppEvent::ServiceStateUpdated(_) => f.write_str("ServiceStateUpdated"),
            AppEvent::ConfigUpdated(_) => f.write_str("ConfigUpdated"),
            AppEvent::PushView(_) => f.write_str("PushView"),
            AppEvent::PopView => f.write_str("PopView"),
            AppEvent::Approval { approval_id, .. } => {
                f.debug_struct("Approval").field("id", approval_id).finish()
            }
            AppEvent::ServiceStart => f.write_str("ServiceStart"),
            AppEvent::ServiceStop => f.write_str("ServiceStop"),
            AppEvent::ServiceActionResult(s) => f.debug_tuple("ServiceActionResult").field(s).finish(),
            AppEvent::Refresh => f.write_str("Refresh"),
            AppEvent::SubmitRegister => f.write_str("SubmitRegister"),
            AppEvent::SubmitAddRunner => f.write_str("SubmitAddRunner"),
            AppEvent::SubmitRemoveRunner(n) => f.debug_tuple("SubmitRemoveRunner").field(n).finish(),
            AppEvent::SaveConfig => f.write_str("SaveConfig"),
            AppEvent::DiscardConfigEdits => f.write_str("DiscardConfigEdits"),
            AppEvent::ReloadOutcomeUpdated(_) => f.write_str("ReloadOutcomeUpdated"),
            AppEvent::SelectRunner(i) => f.debug_tuple("SelectRunner").field(i).finish(),
        }
    }
}
