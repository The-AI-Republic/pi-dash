// Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Self-update: the launch-time prompt plus a daily background check.
//!
//! At launch a new version is offered in a native dialog. While the app
//! stays open it checks again once a day, just after 00:00 UTC; an update
//! found then is not prompted for — it is announced to the web UI as
//! [`UPDATE_AVAILABLE_EVENT`], which shows a small update button beside the
//! sidebar's user menu. Clicking it calls [`desktop_install_update`].
//!
//! Only runs when the build configures `plugins.updater` (see `main`). All
//! check failures (network down, malformed manifest, signature mismatch) log
//! to stderr and leave the app on the current version.

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};
use tauri_plugin_updater::{Update, UpdaterExt};

/// Emitted with a [`PendingUpdate`] payload when an update is found and the
/// user has not been prompted for it.
pub const UPDATE_AVAILABLE_EVENT: &str = "updater://available";

const DAY_SECS: u64 = 24 * 60 * 60;
/// Every open app checks "at midnight UTC"; spread them over this window so
/// the update endpoint doesn't take the whole install base in one second.
const JITTER_SECS: u64 = 15 * 60;
/// Longest single sleep while waiting for the daily check. The tokio clock
/// is monotonic and, on macOS and Linux, stops while the machine is
/// suspended — one long sleep until midnight would fire hours late after a
/// laptop wakes. Short naps re-read the wall clock instead.
const MAX_NAP: Duration = Duration::from_secs(10 * 60);

#[derive(Default)]
pub struct UpdateState {
    /// The update the user deferred or was never prompted for.
    pending: Mutex<Option<Update>>,
    /// Set while an update downloads and installs, so a second install (or
    /// a daily check swapping `pending` underneath it) can't start.
    installing: AtomicBool,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingUpdate {
    version: String,
    current_version: String,
    /// Whether this update is downloading right now. The sidebar button
    /// remounts (collapse, and routes that drop the sidebar entirely), so
    /// it can't keep that in React state — it reads it from here.
    installing: bool,
}

impl PendingUpdate {
    fn of(update: &Update, installing: bool) -> Self {
        Self {
            version: update.version.clone(),
            current_version: update.current_version.clone(),
            installing,
        }
    }
}

async fn check(handle: &AppHandle) -> Option<Update> {
    let updater = match handle.updater() {
        Ok(u) => u,
        Err(e) => {
            eprintln!("updater: construction failed: {e}");
            return None;
        }
    };
    match updater.check().await {
        Ok(update) => update,
        Err(e) => {
            eprintln!("updater: check failed: {e}");
            None
        }
    }
}

/// The update waiting for the sidebar button, if any.
fn pending_update(state: &UpdateState) -> Option<PendingUpdate> {
    let installing = state.installing.load(Ordering::SeqCst);
    state
        .pending
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .as_ref()
        .map(|update| PendingUpdate::of(update, installing))
}

/// Tell the web UI what is waiting. Also sent after a failed install, so a
/// button that remounted mid-download drops out of its installing state
/// instead of staying disabled until the next launch.
fn announce(handle: &AppHandle) {
    let Some(payload) = pending_update(&handle.state::<UpdateState>()) else {
        return;
    };
    if let Err(e) = handle.emit(UPDATE_AVAILABLE_EVENT, payload) {
        eprintln!("updater: announcing update failed: {e}");
    }
}

/// Keep `update` for the sidebar button and tell the web UI about it.
fn remember(handle: &AppHandle, update: Update) {
    *handle
        .state::<UpdateState>()
        .pending
        .lock()
        .unwrap_or_else(|e| e.into_inner()) = Some(update);
    announce(handle);
}

/// Check at startup. If a newer version is available, ask the user via a
/// native dialog; install + restart on confirmation. "Later" keeps the
/// update for the sidebar button.
pub async fn prompt_at_launch(handle: AppHandle) {
    let Some(update) = check(&handle).await else {
        return;
    };
    let install = handle
        .dialog()
        .message(format!(
            "Pi Dash {new} is available (you have {current}).\n\n\
             Install now? The app will restart automatically.",
            new = update.version,
            current = update.current_version,
        ))
        .title("Update available")
        .kind(MessageDialogKind::Info)
        .buttons(MessageDialogButtons::OkCancelCustom(
            "Install".into(),
            "Later".into(),
        ))
        .blocking_show();
    if !install {
        remember(&handle, update);
        return;
    }
    let state = handle.state::<UpdateState>();
    state.installing.store(true, Ordering::SeqCst);
    if let Err(e) = update
        .download_and_install(|_chunk, _total| {}, || {})
        .await
    {
        eprintln!("updater: download/install failed: {e}");
        state.installing.store(false, Ordering::SeqCst);
        // The user explicitly opted in to the install — staying silent on
        // failure leaves them wondering whether anything happened. Surface
        // the error in a dialog so they know they're still on the old
        // version, and keep the update so the sidebar button can retry.
        handle
            .dialog()
            .message(format!(
                "The update couldn't be installed.\n\n{e}\n\nYou can try again from the update button in the sidebar."
            ))
            .title("Pi Dash update failed")
            .kind(MessageDialogKind::Error)
            .show(|_| {});
        remember(&handle, update);
        return;
    }
    handle.restart();
}

fn unix_secs(t: SystemTime) -> u64 {
    t.duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

/// When the daily check after `last_check_secs` is due: the next 00:00 UTC,
/// plus this app's jitter.
fn next_check_secs(last_check_secs: u64, jitter_secs: u64) -> u64 {
    (last_check_secs / DAY_SECS + 1) * DAY_SECS + jitter_secs
}

/// Check once a day, just after 00:00 UTC, for as long as the app runs. The
/// launch check covers the day the app starts on, so the first daily check
/// is the next midnight. A found update is announced, never prompted for.
pub async fn run_daily_checks(handle: AppHandle) {
    let jitter_secs = (uuid::Uuid::new_v4().as_u128() % u128::from(JITTER_SECS)) as u64;
    let mut due = next_check_secs(unix_secs(SystemTime::now()), jitter_secs);
    loop {
        let now = unix_secs(SystemTime::now());
        if now < due {
            tokio::time::sleep(Duration::from_secs(due - now).min(MAX_NAP)).await;
            continue;
        }
        // A machine asleep across several midnights checks once on waking.
        due = next_check_secs(now, jitter_secs);
        if handle
            .state::<UpdateState>()
            .installing
            .load(Ordering::SeqCst)
        {
            continue;
        }
        if let Some(update) = check(&handle).await {
            remember(&handle, update);
        }
    }
}

/// The update waiting for the sidebar button, if any. The web UI asks on
/// mount because [`UPDATE_AVAILABLE_EVENT`] may fire before it listens.
#[tauri::command]
pub fn desktop_pending_update(state: tauri::State<'_, UpdateState>) -> Option<PendingUpdate> {
    pending_update(&state)
}

/// Download and install the pending update, then restart into it.
#[tauri::command]
pub async fn desktop_install_update(app: AppHandle) -> Result<(), String> {
    let state = app.state::<UpdateState>();
    let update = state
        .pending
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone()
        .ok_or_else(|| "no update is pending".to_string())?;
    if state.installing.swap(true, Ordering::SeqCst) {
        return Err("an update is already installing".into());
    }
    if let Err(e) = update
        .download_and_install(|_chunk, _total| {}, || {})
        .await
    {
        eprintln!("updater: download/install failed: {e}");
        state.installing.store(false, Ordering::SeqCst);
        // The caller learns about this from the rejected promise, but a
        // button that remounted mid-download is a different component and
        // would stay disabled. Announce the update again so it re-enables.
        announce(&app);
        return Err(e.to_string());
    }
    app.restart();
}

#[cfg(test)]
mod tests {
    use super::*;

    // 2026-09-20T13:45:00Z
    const MIDDAY: u64 = 1_789_911_900;
    // 2026-09-21T00:00:00Z
    const NEXT_MIDNIGHT: u64 = 1_789_948_800;

    #[test]
    fn daily_check_is_due_at_the_next_utc_midnight_plus_jitter() {
        assert_eq!(next_check_secs(MIDDAY, 0), NEXT_MIDNIGHT);
        assert_eq!(next_check_secs(MIDDAY, 300), NEXT_MIDNIGHT + 300);
    }

    #[test]
    fn a_check_just_after_midnight_schedules_the_following_one() {
        assert_eq!(
            next_check_secs(NEXT_MIDNIGHT + 300, 300),
            NEXT_MIDNIGHT + DAY_SECS + 300
        );
    }

    #[test]
    fn waking_days_later_schedules_the_next_midnight_after_waking() {
        let woke = NEXT_MIDNIGHT + 3 * DAY_SECS + 60;
        assert_eq!(next_check_secs(woke, 0), NEXT_MIDNIGHT + 4 * DAY_SECS);
    }
}
