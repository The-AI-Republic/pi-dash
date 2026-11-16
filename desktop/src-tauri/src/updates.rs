// Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Self-update: the launch-time prompt plus a daily background check.
//!
//! At launch a new version is offered in a native dialog. While the app
//! stays open it checks again once a day, just after 00:00 UTC; an update
//! found then is not prompted for — it is announced to the web UI as
//! [`UPDATE_AVAILABLE_EVENT`], which shows a small update button beside the
//! sidebar's user menu. Clicking it calls [`desktop_install_update`]. The
//! same button shows, busy, while an update accepted at launch downloads.
//!
//! Only runs when the build configures `plugins.updater` (see `main`). All
//! check failures (network down, malformed manifest, signature mismatch) log
//! to stderr and leave the app on the current version.

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use tauri::utils::config::BundleType;
use tauri::utils::platform::bundle_type;
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

/// The versions the sidebar button shows, plus the manifest's release
/// notes. [`Update`] can only be built by the updater plugin, so the state
/// below is generic over this for tests.
trait Versions {
    fn version(&self) -> &str;
    fn current_version(&self) -> &str;
    fn body(&self) -> Option<&str>;
}

impl Versions for Update {
    fn version(&self) -> &str {
        &self.version
    }

    fn current_version(&self) -> &str {
        &self.current_version
    }

    fn body(&self) -> Option<&str> {
        self.body.as_deref()
    }
}

pub struct UpdateState<U = Update> {
    /// The update the user deferred or was never prompted for.
    pending: Mutex<Option<U>>,
    /// Set while an update downloads and installs, so a second install (or
    /// a daily check swapping `pending` underneath it) can't start.
    installing: AtomicBool,
}

impl<U> Default for UpdateState<U> {
    fn default() -> Self {
        Self {
            pending: Mutex::new(None),
            installing: AtomicBool::new(false),
        }
    }
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingUpdate {
    version: String,
    current_version: String,
    /// The manifest's release notes when the release carried any; absent
    /// otherwise so older payloads stay byte-identical.
    #[serde(skip_serializing_if = "Option::is_none")]
    notes: Option<String>,
    /// Whether this update is downloading right now. The sidebar button
    /// remounts (collapse, and routes that drop the sidebar entirely), so
    /// it can't keep that in React state — it reads it from here.
    installing: bool,
}

impl PendingUpdate {
    fn of(update: &impl Versions, installing: bool) -> Self {
        Self {
            version: update.version().to_owned(),
            current_version: update.current_version().to_owned(),
            notes: update
                .body()
                .map(str::trim)
                .filter(|notes| !notes.is_empty())
                .map(str::to_owned),
            installing,
        }
    }
}

/// The manifest arch token the updater plugin keys entries by (`x86_64`,
/// …), from the build's target arch. Mirrors the plugin's own mapping so a
/// manifest entry the binary would resolve to stays resolvable here.
fn updater_arch_for(build_arch: &str) -> Option<&'static str> {
    match build_arch {
        "x86" => Some("i686"),
        "x86_64" => Some("x86_64"),
        "arm" => Some("armv7"),
        "aarch64" => Some("aarch64"),
        "riscv64" => Some("riscv64"),
        _ => None,
    }
}

/// The install-type-specific manifest key this binary prefers
/// (`linux-x86_64-deb`, `windows-x86_64-msi`, …), or `None` when the bundle
/// type is not one of the shipped Linux/Windows ones (macOS bundles,
/// unstamped dev builds) and the manifest gate should not apply.
fn specific_platform_key(bundle: Option<BundleType>, arch: Option<&str>) -> Option<String> {
    let arch = arch?;
    match bundle? {
        BundleType::Deb => Some(format!("linux-{arch}-deb")),
        BundleType::AppImage => Some(format!("linux-{arch}-appimage")),
        BundleType::Msi => Some(format!("windows-{arch}-msi")),
        BundleType::Nsis => Some(format!("windows-{arch}-nsis")),
        _ => None,
    }
}

/// Whether the fetched manifest may serve this install type.
///
/// The plugin resolves `{os}-{arch}-{installer}` first and falls back to
/// `{os}-{arch}`. When a catalog only carries the generic key, a deb binary
/// is handed AppImage bytes (rejected by `install_deb`) and an MSI binary
/// is handed the NSIS installer (executed as NSIS, which silently converts
/// the install). In both cases the right behavior is to offer nothing: the
/// generic entry belongs to the other installer, not to this one. AppImage
/// and NSIS binaries keep falling back to the generic entry, as today.
///
/// Only static manifests (a `platforms` map) are gated: dynamic
/// (single-URL) endpoints and shapes this gate does not understand keep
/// today's behavior.
fn manifest_may_serve(
    bundle: Option<BundleType>,
    arch: Option<&str>,
    manifest: &serde_json::Value,
) -> bool {
    let Some(specific) = specific_platform_key(bundle.clone(), arch) else {
        return true;
    };
    let Some(platforms) = manifest.get("platforms").and_then(|p| p.as_object()) else {
        return true;
    };
    if platforms.contains_key(&specific) {
        return true;
    }
    // AppImage and NSIS installs resolve through the generic entry today
    // (`linux-x86_64`, `windows-x86_64`); the deb and MSI installs must not.
    // `arch` is `Some` here: without it `specific_platform_key` returned `None`.
    let arch = arch.unwrap_or_default();
    match bundle {
        Some(BundleType::AppImage) => platforms.contains_key(&format!("linux-{arch}")),
        Some(BundleType::Nsis) => platforms.contains_key(&format!("windows-{arch}")),
        _ => false,
    }
}

/// Ask the update service what is offered. `Ok(None)` means the check
/// succeeded and nothing is offered (the app is current, the release
/// was withdrawn, or the manifest has no entry for this install type);
/// `Err(())` means the check failed (network down, malformed manifest,
/// signature mismatch), in which case the caller must leave the pending
/// update alone — a failed check proves nothing.
async fn check(handle: &AppHandle) -> Result<Option<Update>, ()> {
    let updater = match handle.updater() {
        Ok(u) => u,
        Err(e) => {
            eprintln!("updater: construction failed: {e}");
            return Err(());
        }
    };
    match updater.check().await {
        Ok(Some(update)) => {
            let bundle = bundle_type();
            let arch = updater_arch_for(std::env::consts::ARCH);
            if !manifest_may_serve(bundle.clone(), arch, &update.raw_json) {
                eprintln!(
                    "updater: skipping update for {bundle:?} install; the manifest has no entry of its own type"
                );
                return Ok(None);
            }
            Ok(Some(update))
        }
        Ok(None) => Ok(None),
        Err(e) => {
            eprintln!("updater: check failed: {e}");
            Err(())
        }
    }
}

/// The update waiting for the sidebar button, if any.
fn pending_update<U: Versions>(state: &UpdateState<U>) -> Option<PendingUpdate> {
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

/// Drop the pending update and tell the web UI, which hides the sidebar
/// button: the daily check confirmed the service offers nothing (the
/// release was withdrawn). The announcement carries `null`, which the web
/// UI reads as "no update waits".
fn forget(handle: &AppHandle) {
    *handle
        .state::<UpdateState>()
        .pending
        .lock()
        .unwrap_or_else(|e| e.into_inner()) = None;
    if let Err(e) = handle.emit(UPDATE_AVAILABLE_EVENT, Option::<PendingUpdate>::None) {
        eprintln!("updater: announcing withdrawal failed: {e}");
    }
}

/// What the daily check decided about the pending update.
///
/// The caller announces `Remembered` and `Withdrawn` to the web UI;
/// `Kept` means the check failed and nothing changed.
enum DailyApply {
    Remembered,
    Withdrawn,
    Kept,
}

/// Apply one daily-check result to the pending update. A confirmed "no
/// update offered" clears the pending update — without that, a withdrawn
/// release would stay installable until the app restarts. A failed check
/// leaves the pending update alone.
fn apply_daily_result<U>(state: &UpdateState<U>, result: Result<Option<U>, ()>) -> DailyApply {
    match result {
        Ok(Some(update)) => {
            *state.pending.lock().unwrap_or_else(|e| e.into_inner()) = Some(update);
            DailyApply::Remembered
        }
        Ok(None) => {
            *state.pending.lock().unwrap_or_else(|e| e.into_inner()) = None;
            DailyApply::Withdrawn
        }
        Err(()) => DailyApply::Kept,
    }
}

/// Mark the update accepted in the launch prompt as installing. It is kept
/// as the pending update too: the sidebar button only shows for a pending
/// update, and without it the download would run with no sign of progress.
///
/// Returns `false` when an install is already running — the caller must
/// not start a second download. Like [`desktop_install_update`], the flag
/// is claimed with `swap` so two installs can never run at once.
fn begin_launch_install<U>(state: &UpdateState<U>, update: U) -> bool {
    if state.installing.swap(true, Ordering::SeqCst) {
        return false;
    }
    *state.pending.lock().unwrap_or_else(|e| e.into_inner()) = Some(update);
    true
}

/// The launch-dialog message: both versions, plus the release notes when
/// the release carried any. Notes are trimmed; empty notes read as absent.
fn launch_dialog_message(update: &impl Versions) -> String {
    let notes = update
        .body()
        .map(str::trim)
        .filter(|notes| !notes.is_empty());
    match notes {
        Some(notes) => format!(
            "Pi Dash {new} is available (you have {current}).\n\n{notes}\n\n\
             Install now? The app will restart automatically.",
            new = update.version(),
            current = update.current_version(),
        ),
        None => format!(
            "Pi Dash {new} is available (you have {current}).\n\n\
             Install now? The app will restart automatically.",
            new = update.version(),
            current = update.current_version(),
        ),
    }
}

/// Check at startup. If a newer version is available, ask the user via a
/// native dialog; install + restart on confirmation. "Later" keeps the
/// update for the sidebar button.
pub async fn prompt_at_launch(handle: AppHandle) {
    let Ok(Some(update)) = check(&handle).await else {
        return;
    };
    let message = launch_dialog_message(&update);
    let dialog = handle
        .dialog()
        .message(message)
        .title("Update available")
        .kind(MessageDialogKind::Info)
        .buttons(MessageDialogButtons::OkCancelCustom(
            "Install".into(),
            "Later".into(),
        ));
    // `blocking_show` parks the thread until the user answers. Run it on
    // the blocking pool: parking an async-runtime worker would stall every
    // other async command on a single-CPU machine.
    let install = tauri::async_runtime::spawn_blocking(move || dialog.blocking_show())
        .await
        .unwrap_or(false);
    if !install {
        remember(&handle, update);
        return;
    }
    let state = handle.state::<UpdateState>();
    if !begin_launch_install(&state, update.clone()) {
        // A sidebar install for this update is already running; its
        // completion restarts the app, so the launch "Install" must not
        // start a second download alongside it.
        return;
    }
    announce(&handle);
    if let Err(e) = update
        .download_and_install(|_chunk, _total| {}, || {})
        .await
    {
        eprintln!("updater: download/install failed: {e}");
        state.installing.store(false, Ordering::SeqCst);
        // The user explicitly opted in to the install — staying silent on
        // failure leaves them wondering whether anything happened. Surface
        // the error in a dialog so they know they're still on the old
        // version. The update is still pending, so announce it again to
        // put the sidebar button back in its clickable state for a retry.
        handle
            .dialog()
            .message(format!(
                "The update couldn't be installed.\n\n{e}\n\nYou can try again from the update button in the sidebar."
            ))
            .title("Pi Dash update failed")
            .kind(MessageDialogKind::Error)
            .show(|_| {});
        announce(&handle);
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
        match apply_daily_result(&handle.state::<UpdateState>(), check(&handle).await) {
            DailyApply::Remembered => announce(&handle),
            DailyApply::Withdrawn => forget(&handle),
            DailyApply::Kept => {}
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

    struct FakeUpdate {
        version: &'static str,
        notes: Option<&'static str>,
    }

    impl Versions for FakeUpdate {
        fn version(&self) -> &str {
            self.version
        }

        fn current_version(&self) -> &str {
            "0.3.3"
        }

        fn body(&self) -> Option<&str> {
            self.notes
        }
    }

    fn fake(version: &'static str) -> FakeUpdate {
        FakeUpdate {
            version,
            notes: None,
        }
    }

    fn fake_with_notes(version: &'static str, notes: &'static str) -> FakeUpdate {
        FakeUpdate {
            version,
            notes: Some(notes),
        }
    }

    #[test]
    fn an_update_accepted_at_launch_is_pending_and_installing() {
        let state = UpdateState::<FakeUpdate>::default();
        assert!(begin_launch_install(&state, fake("0.4.0")));
        let pending =
            pending_update(&state).expect("the sidebar button needs the update while it downloads");
        assert!(pending.installing);
        assert_eq!(pending.version, "0.4.0");
        assert_eq!(pending.current_version, "0.3.3");
    }

    #[test]
    fn a_launch_install_arriving_during_a_running_install_does_not_clobber_it() {
        let state = UpdateState::<FakeUpdate>::default();
        // The sidebar install is already running: it holds the flag and the
        // pending update (`desktop_install_update` guards the same way).
        state.installing.store(true, Ordering::SeqCst);
        *state.pending.lock().unwrap_or_else(|e| e.into_inner()) = Some(fake("0.4.0"));
        // The still-open launch dialog is answered with "Install" anyway.
        assert!(!begin_launch_install(&state, fake("0.4.1")));
        let pending = pending_update(&state).expect("the running install stays pending");
        assert_eq!(pending.version, "0.4.0");
        assert!(pending.installing);
    }

    #[test]
    fn a_confirmed_withdrawal_clears_the_pending_update() {
        let state = UpdateState::<FakeUpdate>::default();
        *state.pending.lock().unwrap_or_else(|e| e.into_inner()) = Some(fake("0.4.0"));
        assert!(matches!(
            apply_daily_result(&state, Ok(None)),
            DailyApply::Withdrawn
        ));
        assert!(pending_update(&state).is_none());
    }

    #[test]
    fn a_failed_check_keeps_the_pending_update() {
        let state = UpdateState::<FakeUpdate>::default();
        *state.pending.lock().unwrap_or_else(|e| e.into_inner()) = Some(fake("0.4.0"));
        assert!(matches!(
            apply_daily_result::<FakeUpdate>(&state, Err(())),
            DailyApply::Kept
        ));
        assert_eq!(
            pending_update(&state)
                .expect("pending survives a failed check")
                .version,
            "0.4.0"
        );
    }

    #[test]
    fn a_found_update_becomes_pending() {
        let state = UpdateState::<FakeUpdate>::default();
        assert!(matches!(
            apply_daily_result(&state, Ok(Some(fake("0.4.0")))),
            DailyApply::Remembered
        ));
        assert_eq!(
            pending_update(&state)
                .expect("the found update waits")
                .version,
            "0.4.0"
        );
    }

    #[test]
    fn the_pending_payload_carries_release_notes() {
        let state = UpdateState::<FakeUpdate>::default();
        *state.pending.lock().unwrap_or_else(|e| e.into_inner()) =
            Some(fake_with_notes("0.4.0", "Fixes the sync race."));
        let pending = pending_update(&state).expect("pending");
        assert_eq!(pending.notes.as_deref(), Some("Fixes the sync race."));
    }

    #[test]
    fn the_pending_payload_omits_empty_release_notes() {
        let state = UpdateState::<FakeUpdate>::default();
        *state.pending.lock().unwrap_or_else(|e| e.into_inner()) =
            Some(fake_with_notes("0.4.0", "   "));
        assert!(pending_update(&state).expect("pending").notes.is_none());
    }

    #[test]
    fn the_launch_dialog_names_both_versions_without_notes() {
        let message = launch_dialog_message(&fake("0.4.0"));
        assert_eq!(
            message,
            "Pi Dash 0.4.0 is available (you have 0.3.3).\n\n\
             Install now? The app will restart automatically."
        );
    }

    #[test]
    fn the_launch_dialog_includes_release_notes() {
        let message = launch_dialog_message(&fake_with_notes("0.4.0", "Fixes the sync race."));
        assert_eq!(
            message,
            "Pi Dash 0.4.0 is available (you have 0.3.3).\n\nFixes the sync race.\n\n\
             Install now? The app will restart automatically."
        );
    }

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

    /// A static-manifest body carrying exactly these platform keys.
    fn manifest_with(keys: &[&str]) -> serde_json::Value {
        let platforms: serde_json::Map<String, serde_json::Value> = keys
            .iter()
            .map(|k| {
                (
                    (*k).to_string(),
                    serde_json::json!({"url": "https://dl.example.com/x", "signature": "s"}),
                )
            })
            .collect();
        serde_json::json!({"version": "0.4.0", "platforms": platforms})
    }

    #[test]
    fn deb_install_is_not_served_by_the_generic_appimage_entry() {
        // PIDESKAPP-59: the only Linux entry is the AppImage payload.
        let manifest = manifest_with(&["linux-x86_64"]);
        assert!(!manifest_may_serve(Some(BundleType::Deb), Some("x86_64"), &manifest));
    }

    #[test]
    fn deb_install_is_served_by_its_own_entry() {
        let manifest = manifest_with(&["linux-x86_64", "linux-x86_64-deb"]);
        assert!(manifest_may_serve(Some(BundleType::Deb), Some("x86_64"), &manifest));
    }

    #[test]
    fn msi_install_is_not_served_by_the_generic_nsis_entry() {
        // PIDESKAPP-91: the only Windows entry is the NSIS payload.
        let manifest = manifest_with(&["windows-x86_64"]);
        assert!(!manifest_may_serve(Some(BundleType::Msi), Some("x86_64"), &manifest));
    }

    #[test]
    fn msi_install_is_served_by_its_own_entry() {
        let manifest = manifest_with(&["windows-x86_64", "windows-x86_64-msi"]);
        assert!(manifest_may_serve(Some(BundleType::Msi), Some("x86_64"), &manifest));
    }

    #[test]
    fn appimage_install_keeps_working_via_the_generic_entry() {
        let manifest = manifest_with(&["linux-x86_64"]);
        assert!(manifest_may_serve(
            Some(BundleType::AppImage),
            Some("x86_64"),
            &manifest
        ));
    }

    #[test]
    fn nsis_install_keeps_working_via_the_generic_entry() {
        let manifest = manifest_with(&["windows-x86_64"]);
        assert!(manifest_may_serve(Some(BundleType::Nsis), Some("x86_64"), &manifest));
    }

    #[test]
    fn a_specific_entry_alone_suffices() {
        for (bundle, key) in [
            (BundleType::Deb, "linux-x86_64-deb"),
            (BundleType::AppImage, "linux-x86_64-appimage"),
            (BundleType::Msi, "windows-x86_64-msi"),
            (BundleType::Nsis, "windows-x86_64-nsis"),
        ] {
            let manifest = manifest_with(&[key]);
            assert!(
                manifest_may_serve(Some(bundle.clone()), Some("x86_64"), &manifest),
                "{bundle:?} should be served by {key}"
            );
        }
    }

    #[test]
    fn dynamic_manifests_and_other_bundles_are_not_gated() {
        // Single-URL (dynamic) manifests have no `platforms` map.
        for manifest in [
            serde_json::json!({"version": "0.4.0"}),
            serde_json::json!({"version": "0.4.0", "url": "https://dl.example.com/x", "signature": "s"}),
        ] {
            assert!(manifest_may_serve(Some(BundleType::Deb), Some("x86_64"), &manifest));
            assert!(manifest_may_serve(Some(BundleType::Msi), Some("x86_64"), &manifest));
        }
        // macOS bundles and unstamped dev builds keep today's behavior.
        let manifest = manifest_with(&["linux-x86_64"]);
        for bundle in [None, Some(BundleType::App), Some(BundleType::Dmg)] {
            assert!(manifest_may_serve(bundle, Some("x86_64"), &manifest));
        }
    }

    #[test]
    fn arch_mapping_matches_the_plugin() {
        assert_eq!(updater_arch_for("x86"), Some("i686"));
        assert_eq!(updater_arch_for("x86_64"), Some("x86_64"));
        assert_eq!(updater_arch_for("aarch64"), Some("aarch64"));
        assert_eq!(updater_arch_for("powerpc64"), None);
    }
}
