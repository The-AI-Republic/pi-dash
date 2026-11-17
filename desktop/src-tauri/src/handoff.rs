// Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Desktop session handoff (`.ai_design/desktop_unified_signin/design.md`
//! §5): a CLI sign-in signs in the app, with no browser.
//!
//! The app's interface is the web app, so it needs a web session, while the
//! daemon holds machine credentials, which are not web sessions. The handoff
//! turns one into the other once, through the server, without exposing
//! either credential to the webview:
//!
//! 1. This host generates a verifier (32 random bytes, held only in this
//!    process's memory) and asks the daemon over its local socket with
//!    `challenge` = base64url(SHA-256(verifier)).
//! 2. The daemon trades the challenge for a single-use code with one of its
//!    workspace credentials, bound to the user, the machine and the
//!    challenge (60 seconds).
//! 3. This host redeems the code with the verifier over the native
//!    transport (a `POST`, so no code appears in webview history or a
//!    `Referer`), which writes the response's `Set-Cookie` into the
//!    webview's cookie store. Neither the code nor the verifier ever
//!    crosses into the webview: the command answers with the account only.
//!
//! On Windows the handoff waits for PIDESKAPP-236 (the daemon pipe name is
//! predictable there); until then the Windows app uses a browser sign-in
//! for the CLI → app direction.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// One sign-in event on the app's side (desktop_unified_signin §6.2).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SyncEvent {
    pub kind: String,
    pub account: Option<SyncAccount>,
    /// Wall-clock time, RFC 3339, for the newest-wins comparison.
    pub at: String,
}

/// The account a [`SyncEvent`] names.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SyncAccount {
    pub user_id: Option<String>,
    pub email: Option<String>,
    pub cloud_url: String,
}

/// The app's reconciliation record (design §6.2): its own last sign-in or
/// sign-out, whichever kind, and the daemon `seq` it last acted on. Kept in
/// the Tauri host's store — not the webview — because sign-out wipes the
/// webview's storage exactly when this record matters.
///
/// `fresh_sign_in_at` is written by the sign-in mutations (never by a
/// session restore): its presence says the app just signed IN, with the
/// moment attached. The reconciler stamps `own_event` from it and clears
/// it, so a restored session at launch (no marker) never masquerades as a
/// fresh sign-in — which would let the app's event beat a terminal
/// sign-out it must follow (design §9 scenario 7).
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct AuthSyncRecord {
    pub own_event: Option<SyncEvent>,
    pub last_seen_seq: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fresh_sign_in_at: Option<String>,
}

/// Where the reconciliation record lives: `<app-data>/managed/sync/auth-sync.json`.
fn sync_record_path<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
) -> Result<std::path::PathBuf, String> {
    Ok(crate::app_paths::AppPaths::resolve(app)?
        .root
        .join("sync")
        .join("auth-sync.json"))
}

/// Read the reconciliation record, or `None` when it was never written.
#[tauri::command]
pub async fn auth_sync_get<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
) -> Result<Option<AuthSyncRecord>, String> {
    read_sync_record(&app)
}

/// Persist the reconciliation record, atomically (a half-written record
/// must never lose the app's newest event).
#[tauri::command]
pub async fn auth_sync_set<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    record: AuthSyncRecord,
) -> Result<(), String> {
    write_sync_record(&app, &record)
}

/// Mark a fresh browser sign-in (the sign-in mutations call this; a session
/// restore never does): stamps `fresh_sign_in_at` with the moment, so the
/// reconciler can tell a restored session apart from a sign-in that just
/// happened. Atomically read-modify-writes the record.
#[tauri::command]
pub async fn auth_sync_mark_sign_in<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
) -> Result<(), String> {
    let mut record = read_sync_record(&app)?.unwrap_or_default();
    record.fresh_sign_in_at =
        Some(chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true));
    write_sync_record(&app, &record)
}

fn read_sync_record<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
) -> Result<Option<AuthSyncRecord>, String> {
    let path = sync_record_path(app)?;
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(format!("reading the sync record: {e}")),
    };
    if text.trim().is_empty() {
        return Ok(None);
    }
    serde_json::from_str(&text)
        .map(Some)
        .map_err(|e| format!("parsing the sync record: {e}"))
}

fn write_sync_record<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    record: &AuthSyncRecord,
) -> Result<(), String> {
    let path = sync_record_path(app)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("creating {}: {e}", parent.display()))?;
    }
    let text = serde_json::to_string(record).map_err(|e| e.to_string())?;
    let staging = path.with_extension("json.tmp");
    std::fs::write(&staging, text).map_err(|e| format!("writing {}: {e}", staging.display()))?;
    std::fs::rename(&staging, &path).map_err(|e| {
        let _ = std::fs::remove_file(&staging);
        format!("writing {}: {e}", path.display())
    })?;
    Ok(())
}

/// base64url (no padding) of `bytes`. Hand-rolled so the handoff needs no
/// extra dependency; the `=` padding is omitted, as the server's check
/// accepts.
pub(crate) fn base64url(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::with_capacity(bytes.len() * 4 / 3 + 4);
    for chunk in bytes.chunks(3) {
        let n = chunk.len();
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        out.push(ALPHABET[(b[0] >> 2) as usize] as char);
        out.push(ALPHABET[(((b[0] & 0b11) << 4) | (b[1] >> 4)) as usize] as char);
        if n > 1 {
            out.push(ALPHABET[(((b[1] & 0b1111) << 2) | (b[2] >> 6)) as usize] as char);
        }
        if n > 2 {
            out.push(ALPHABET[(b[2] & 0b111111) as usize] as char);
        }
    }
    out
}

/// The 32 random bytes a handoff is built on: two UUID v4s, so no `rand`
/// dependency. Held only in this process's memory — the verifier never
/// leaves it.
pub(crate) fn new_verifier() -> [u8; 32] {
    let mut verifier = [0u8; 32];
    verifier[..16].copy_from_slice(uuid::Uuid::new_v4().as_bytes());
    verifier[16..].copy_from_slice(uuid::Uuid::new_v4().as_bytes());
    verifier
}

/// base64url(SHA-256(verifier)) — what the daemon sends as `challenge`,
/// and what the server recomputes on redeem.
pub(crate) fn challenge_for(verifier: &[u8]) -> String {
    base64url(&Sha256::digest(verifier))
}

/// What the webview may learn from a handoff: the account only. The web
/// session itself lives in the webview's cookie store, written by the
/// native redeem.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct HandoffAccount {
    pub user_id: Option<String>,
    pub email: Option<String>,
}

/// Sign the app in as the daemon's account, with no browser
/// (desktop_unified_signin §5). The `expected_user_id` is the account the
/// daemon's `Hello` named; the server refuses a code whose user is any
/// other account.
#[tauri::command]
pub async fn desktop_session_handoff(
    window: tauri::WebviewWindow,
    state: tauri::State<'_, crate::desktop_http::DesktopHttp>,
    expected_user_id: String,
) -> Result<HandoffAccount, String> {
    // PIDESKAPP-236: the Windows pipe name is predictable, so the handoff
    // waits until the socket is owned. Until then, Windows uses a browser
    // sign-in for CLI → app (design §5.1).
    if cfg!(windows) {
        return Err(
            "The handoff is not available on Windows yet; use a browser sign-in.".to_string(),
        );
    }
    let verifier = new_verifier();
    let challenge = challenge_for(&verifier);
    let answer = crate::daemon::session_handoff_at(
        &crate::daemon::DaemonPaths::standard()?.socket(),
        &challenge,
    )
    .await
    .map_err(|e| e.message())?;
    // The server's other check: the code's user must be the `expected_user_id`
    // the app took from `Hello` (design §5.1). Refused here too, before the
    // redeem uses the code up.
    let want = expected_user_id.trim();
    if !want.is_empty() && answer.user_id.as_deref().is_some_and(|got| got != want) {
        return Err("The handoff is not for this account.".to_string());
    }
    redeem(&window, &state, &answer.code, &verifier, want).await
}

/// Redeem the code with the verifier over the native transport. A native
/// `POST`, so no code appears in webview history or a `Referer`; the
/// response's `Set-Cookie` lands in the webview's cookie store through the
/// same `send()` path every API request uses.
async fn redeem(
    window: &tauri::WebviewWindow,
    state: &crate::desktop_http::DesktopHttp,
    code: &str,
    verifier: &[u8; 32],
    expected_user_id: &str,
) -> Result<HandoffAccount, String> {
    let url = state
        .api_url
        .join("auth/desktop-handoff/redeem/")
        .map_err(|_| "cannot build the redeem URL")?;
    let body = serde_json::json!({
        "code": code,
        "verifier": base64url(verifier),
        "expected_user_id": expected_user_id,
    });
    let body_bytes = serde_json::to_vec(&body).map_err(|e| e.to_string())?;
    let request = crate::desktop_http::ApiRequest::new(
        "desktop-handoff",
        url,
        "POST",
        vec![("Content-Type".to_string(), "application/json".to_string())],
        true,
    );
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
    let (response, _generation) =
        crate::desktop_http::send(window, state, request, Some(body_bytes), Some(deadline)).await?;
    if !response.status().is_success() {
        // Status only: never the code, the verifier, or a server error body
        // that might echo them.
        return Err(format!(
            "the handoff was refused (HTTP {})",
            response.status()
        ));
    }
    let payload: serde_json::Value = response
        .json()
        .await
        .map_err(|_| "cannot read the redeem response")?;
    Ok(HandoffAccount {
        user_id: payload
            .get("user_id")
            .and_then(|v| v.as_str())
            .map(str::to_string),
        email: payload
            .get("email")
            .and_then(|v| v.as_str())
            .map(str::to_string),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verifier_is_32_random_bytes_and_challenge_is_stable() {
        let a = new_verifier();
        let b = new_verifier();
        assert_eq!(a.len(), 32);
        // Two verifiers colliding is a 1-in-2^256 event; unequal also proves
        // the bytes are actually random, not a constant.
        assert_ne!(a, b);

        let challenge = challenge_for(&a);
        assert_eq!(challenge.len(), 43, "base64url(SHA-256) is 43 chars");
        assert_eq!(
            challenge_for(&a),
            challenge,
            "same verifier, same challenge"
        );
        assert_ne!(
            challenge_for(&b),
            challenge,
            "a different verifier challenges differently"
        );

        // The challenge is base64url of the digest: recomputing from the raw
        // digest gives the same string the daemon sends and the server
        // checks.
        assert_eq!(challenge, base64url(&Sha256::digest(a)));
    }

    #[test]
    fn base64url_vectors() {
        assert_eq!(base64url(b""), "");
        assert_eq!(base64url(b"f"), "Zg");
        assert_eq!(base64url(b"fo"), "Zm8");
        assert_eq!(base64url(b"foo"), "Zm9v");
        assert_eq!(base64url(b"foob"), "Zm9vYg");
        assert_eq!(base64url(b"fooba"), "Zm9vYmE");
        assert_eq!(base64url(b"foobar"), "Zm9vYmFy");
        assert_eq!(base64url(&[0xfb, 0xff, 0xfe]), "-__-");
    }
}
