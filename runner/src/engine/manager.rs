//! The machine's built-in engine, as the daemon holds it (design §4.3).
//!
//! - **Engine credential** (`ec_`, 14 days): read from `config.toml` /
//!   the keychain through the daemon's [`WorkspaceTokens`] store, so a
//!   `pidash login --engine` reaches the daemon with `CredentialsReload`.
//! - **Profile**: `GET /api/v1/runner/engine/profile/` with a daemon
//!   credential, at start, on every engine-credential change and every 15
//!   minutes. An available profile is written into the engine's
//!   `CODEX_HOME/config.toml` and published for spawns
//!   ([`super::agent::set_launch`]).
//! - **Model token**: fetched with the engine credential when the auth
//!   helper asks (`pidash __engine model-token` → IPC `EngineModelToken`),
//!   held **in memory only**, and reused until fewer than 60 s remain.
//! - **State**: available / signed out / expiring / unavailable with a
//!   reason, reported in `Hello`, `pidash status` and the machine session's
//!   heartbeat (`engine_state`).
//! - **Pause**: the owner's switch and the server's automatic reason
//!   (design §8), cached from the server's machine payload (session
//!   welcome, reconcile, the pause call's answer) and reported with the
//!   state, so a client can offer Pause or Resume.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use chrono::{DateTime, Utc};
use tokio::sync::Notify;

use super::api::{EngineApi, EngineApiError, EnginePause, EngineProfile, MachinePayload, reason};
use super::codex_config::{AuthHelper, codex_home, write_engine_config};
use crate::daemon::workspace_tokens::WorkspaceTokens;
use crate::ipc::protocol::{EngineSignInState, EngineState, EngineStatusInfo};
use crate::util::paths::Paths;

/// Refresh the model token when fewer than this many seconds remain.
pub const REFRESH_SKEW_SECS: i64 = 60;
/// How often the profile is fetched again.
pub const PROFILE_INTERVAL: Duration = Duration::from_secs(15 * 60);
/// A credential this close to expiry is `expiring` (design §4.3).
pub const EXPIRING_WITHIN_DAYS: i64 = 3;

/// A model token the daemon holds in memory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelToken {
    pub token: String,
    pub expires_at: DateTime<Utc>,
}

impl ModelToken {
    fn fresh_at(&self, now: DateTime<Utc>) -> bool {
        self.expires_at - now >= chrono::Duration::seconds(REFRESH_SKEW_SECS)
    }
}

/// Why no model token could be handed out.
#[derive(Debug, Clone, thiserror::Error, PartialEq, Eq)]
#[error("{reason_code}: {message}")]
pub struct ModelTokenError {
    pub reason_code: String,
    pub message: String,
}

#[derive(Default)]
struct State {
    profile: Option<EngineProfile>,
    profile_fetched_at: Option<DateTime<Utc>>,
    /// Sign-in state the server last reported (profile / login).
    server_sign_in: Option<EngineSignInState>,
    model_token: Option<ModelToken>,
    /// Reason code of the last refused model-token request; cleared by the
    /// next success or a credential change.
    token_failure: Option<String>,
    /// The machine's pause state as the server last reported it, and when.
    pause: Option<(EnginePause, DateTime<Utc>)>,
    /// Bumped by every credential change. A model-token fetch started
    /// under an older generation (with the old credential) does not store
    /// its result.
    generation: u64,
}

pub struct EngineManager {
    api: Option<EngineApi>,
    paths: Paths,
    tokens: WorkspaceTokens,
    /// Engine binary resolved at daemon start (`None`: not installed).
    binary: Option<PathBuf>,
    /// Program the engine runs as its auth helper (this `pidash`).
    helper_program: PathBuf,
    state: Mutex<State>,
    /// Serialises model-token fetches, so concurrent helpers share one.
    fetch: tokio::sync::Mutex<()>,
    changed: Notify,
}

impl EngineManager {
    pub(crate) fn new(
        cloud_url: Option<&str>,
        paths: Paths,
        tokens: WorkspaceTokens,
        binary: Option<PathBuf>,
    ) -> Arc<Self> {
        let api = cloud_url
            .filter(|u| !u.is_empty())
            .and_then(|u| EngineApi::new(u).ok());
        let helper_program = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("pidash"));
        let server_sign_in = tokens.engine_sign_in_state();
        Arc::new(Self {
            api,
            paths,
            tokens,
            binary,
            helper_program,
            state: Mutex::new(State {
                server_sign_in,
                ..State::default()
            }),
            fetch: tokio::sync::Mutex::new(()),
            changed: Notify::new(),
        })
    }

    fn state(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().expect("engine state poisoned")
    }

    /// The account's `CODEX_HOME`.
    pub fn codex_home(&self) -> PathBuf {
        let (user_id, email) = self.tokens.account();
        codex_home(&self.paths, user_id.as_deref().or(email.as_deref()))
    }

    /// The engine credential changed (`CredentialsReload` after a login or
    /// logout): drop the model token and fetch the profile again.
    pub fn credentials_changed(&self) {
        {
            let mut st = self.state();
            st.generation += 1;
            st.model_token = None;
            st.token_failure = None;
            st.server_sign_in = self.tokens.engine_sign_in_state();
        }
        self.changed.notify_waiters();
    }

    /// Cache the machine's pause state the server reported.
    pub fn set_pause(&self, pause: EnginePause) {
        self.state().pause = Some((pause, Utc::now()));
    }

    /// Cache the pause state from a machine payload, when it carries one.
    pub fn note_machine_payload(&self, payload: &MachinePayload) {
        if let Some(pause) = payload.pause() {
            self.set_pause(pause);
        }
    }

    /// The cached pause state, if the server reported one.
    pub fn pause(&self) -> Option<EnginePause> {
        self.state().pause.as_ref().map(|(p, _)| p.clone())
    }

    /// Set the owner's pause switch on the server with `machine_token` and
    /// cache the answer.
    pub async fn set_paused(
        &self,
        machine_token: &str,
        dev_machine_id: uuid::Uuid,
        paused: bool,
    ) -> Result<EngineStatusInfo, EngineApiError> {
        let api = self
            .api
            .as_ref()
            .ok_or_else(|| EngineApiError::Other("the daemon runs offline".into()))?;
        let payload = api
            .set_paused(machine_token, dev_machine_id, paused)
            .await?;
        let mut pause = payload.pause().unwrap_or_default();
        // The server answered for the switch we set; a body without the
        // automatic reason keeps the one we knew.
        if payload.engine_auto_pause_reason.is_none()
            && let Some(known) = self.pause()
        {
            pause.auto_pause_reason = known.auto_pause_reason;
        }
        self.set_pause(pause);
        Ok(self.status())
    }

    /// Read the machine from the server with `machine_token` and cache its
    /// pause state.
    pub async fn refresh_machine(
        &self,
        machine_token: &str,
        dev_machine_id: uuid::Uuid,
    ) -> Result<MachinePayload, EngineApiError> {
        let api = self
            .api
            .as_ref()
            .ok_or_else(|| EngineApiError::Other("the daemon runs offline".into()))?;
        let payload = api.machine(machine_token, dev_machine_id).await?;
        self.note_machine_payload(&payload);
        Ok(payload)
    }

    /// The current model token for the engine's auth helper.
    pub async fn model_token(&self) -> Result<ModelToken, ModelTokenError> {
        if let Some(t) = self.cached_token(Utc::now()) {
            return Ok(t);
        }
        let _single_flight = self.fetch.lock().await;
        // Another caller may have refreshed it while we waited.
        if let Some(t) = self.cached_token(Utc::now()) {
            return Ok(t);
        }
        // Read together with the credential below: a change after this
        // point bumps it, and the fetched token is then dropped.
        let generation = self.state().generation;
        let Some(credential) = self.tokens.engine_credential() else {
            return Err(ModelTokenError {
                reason_code: reason::ENGINE_SIGN_IN_REQUIRED.into(),
                message: "the built-in engine is not signed in on this computer; run `pidash login --engine`".into(),
            });
        };
        if credential.expires_at.is_some_and(|e| e <= Utc::now()) {
            return Err(ModelTokenError {
                reason_code: reason::ENGINE_SIGN_IN_REQUIRED.into(),
                message: "the engine credential expired; run `pidash login --engine`".into(),
            });
        }
        let Some(api) = self.api.as_ref() else {
            return Err(ModelTokenError {
                reason_code: "offline".into(),
                message: "the daemon runs offline".into(),
            });
        };
        match api.model_token(&credential.token).await {
            Ok(resp) => {
                let token = ModelToken {
                    token: resp.token,
                    expires_at: resp.expires_at,
                };
                let mut st = self.state();
                if st.generation != generation {
                    // The credential changed (a sign-out or a new sign-in)
                    // while this fetch was in flight: its token was minted
                    // with the old credential.
                    return Err(ModelTokenError {
                        reason_code: reason::ENGINE_SIGN_IN_REQUIRED.into(),
                        message: "the engine credential changed while the model token was \
                                  being fetched; try again"
                            .into(),
                    });
                }
                st.model_token = Some(token.clone());
                st.token_failure = None;
                Ok(token)
            }
            Err(e) => {
                let reason_code = e
                    .reason_code()
                    .unwrap_or("model_token_unavailable")
                    .to_string();
                if matches!(e, EngineApiError::Refused { .. }) {
                    let mut st = self.state();
                    if st.generation != generation {
                        return Err(ModelTokenError {
                            reason_code,
                            message: e.to_string(),
                        });
                    }
                    st.model_token = None;
                    st.token_failure = Some(reason_code.clone());
                }
                Err(ModelTokenError {
                    reason_code,
                    message: e.to_string(),
                })
            }
        }
    }

    fn cached_token(&self, now: DateTime<Utc>) -> Option<ModelToken> {
        self.state().model_token.clone().filter(|t| t.fresh_at(now))
    }

    /// Fetch the profile with a daemon credential and (re)write the engine
    /// config from it.
    pub async fn refresh_profile(&self) {
        let Some(api) = self.api.as_ref() else {
            return;
        };
        let Some((_, machine_token)) = self.tokens.session_candidates().into_iter().next() else {
            tracing::debug!("engine profile not fetched: no workspace is signed in");
            return;
        };
        let profile = match api.profile(&machine_token).await {
            Ok(p) => p,
            Err(EngineApiError::Refused {
                reason_code,
                status,
                ..
            }) => {
                tracing::warn!(status, %reason_code, "engine profile refused");
                EngineProfile {
                    available: false,
                    lane: String::new(),
                    base_url: String::new(),
                    model: String::new(),
                    reason_code,
                    engine_sign_in_state: None,
                }
            }
            Err(e) => {
                tracing::warn!("engine profile fetch failed: {e}");
                return;
            }
        };
        self.apply_profile(profile);
    }

    fn apply_profile(&self, profile: EngineProfile) {
        let codex_home = self.codex_home();
        if profile.available {
            let helper = AuthHelper::for_paths(self.helper_program.clone(), &self.paths);
            match write_engine_config(&codex_home, &profile, &helper) {
                Ok(_) => super::agent::set_launch(Some(super::agent::EngineLaunch { codex_home })),
                Err(e) => tracing::error!("writing the engine config failed: {e:#}"),
            }
        } else {
            super::agent::set_launch(None);
        }
        let reported = profile.engine_sign_in_state;
        {
            let mut st = self.state();
            if profile.available
                && matches!(
                    st.token_failure.as_deref(),
                    Some(reason::PROFILE_UNAVAILABLE) | Some(reason::ENGINE_DISABLED)
                )
            {
                st.token_failure = None;
            }
            if reported.is_some() {
                st.server_sign_in = reported;
            }
            st.profile = Some(profile);
            st.profile_fetched_at = Some(Utc::now());
        }
        if let Some(server) = reported
            && self.tokens.engine_sign_in_state() != Some(server)
        {
            self.mirror_sign_in_state(server);
        }
    }

    /// Mirror the server's sign-in state into `config.toml` (best-effort).
    ///
    /// The in-memory copy is updated at once, so the next profile pass
    /// compares equal and does not rewrite the file (and, with the
    /// credentials in the keychain, every keychain item) again. The write
    /// itself can block on the keychain for seconds, so it runs off the
    /// async workers. A failed write is retried after the next reload
    /// from disk brings the old value back.
    fn mirror_sign_in_state(&self, server: EngineSignInState) {
        self.tokens.set_engine_sign_in_state(Some(server));
        let paths = self.paths.clone();
        let write = move || {
            let result = crate::config::file::mutate_config(&paths, |cfg| {
                cfg.cli
                    .get_or_insert_with(Default::default)
                    .engine_sign_in_state = Some(server);
                Ok(())
            });
            if let Err(e) = result {
                tracing::debug!("mirroring the engine sign-in state failed: {e:#}");
            }
        };
        match tokio::runtime::Handle::try_current() {
            Ok(rt) => {
                rt.spawn_blocking(write);
            }
            Err(_) => write(),
        }
    }

    /// Machine-level engine state at `now`.
    pub fn status_at(&self, now: DateTime<Utc>) -> EngineStatusInfo {
        // A background install can resolve the pinned binary after this
        // manager was constructed. Keep an existing startup path stable,
        // but pick up the first successful install without a daemon restart.
        let binary = self
            .binary
            .as_deref()
            .or_else(|| super::locate::resolved().map(|engine| engine.binary.as_path()));
        let credential = self.tokens.engine_credential();
        let st = self.state();
        let local = match &credential {
            Some(c) => match c.expires_at {
                Some(e) if e <= now => EngineSignInState::Expired,
                Some(e) if e - now <= chrono::Duration::days(EXPIRING_WITHIN_DAYS) => {
                    EngineSignInState::Expiring
                }
                _ => EngineSignInState::Active,
            },
            None => match st.server_sign_in {
                // No credential held: whatever the server last said, but a
                // live state without a credential is not live here.
                Some(EngineSignInState::Active) | Some(EngineSignInState::Expiring) | None => {
                    EngineSignInState::Never
                }
                Some(other) => other,
            },
        };
        // The server refusing the credential outranks its local expiry.
        let sign_in_state = if credential.is_some()
            && st.token_failure.as_deref() == Some(reason::ENGINE_SIGN_IN_REQUIRED)
        {
            match st.server_sign_in {
                Some(s @ (EngineSignInState::SignedOut | EngineSignInState::Revoked)) => s,
                _ => EngineSignInState::Expired,
            }
        } else {
            local
        };
        let signed_in = matches!(
            sign_in_state,
            EngineSignInState::Active | EngineSignInState::Expiring
        );
        let profile_reason = st.profile.as_ref().filter(|p| !p.available).map(|p| {
            if p.reason_code.is_empty() {
                reason::PROFILE_UNAVAILABLE.to_string()
            } else {
                p.reason_code.clone()
            }
        });
        let (state, reason_code) = if binary.is_none() {
            (
                EngineState::Unavailable,
                Some(reason::ENGINE_NOT_INSTALLED.to_string()),
            )
        } else if !signed_in {
            (
                EngineState::SignedOut,
                Some(reason::ENGINE_SIGN_IN_REQUIRED.to_string()),
            )
        } else if let Some(r) = profile_reason {
            (EngineState::Unavailable, Some(r))
        } else if let Some(r) = st.token_failure.clone() {
            (EngineState::Unavailable, Some(r))
        } else if st.profile.is_none() {
            (
                EngineState::Unavailable,
                Some(reason::PROFILE_PENDING.to_string()),
            )
        } else if sign_in_state == EngineSignInState::Expiring {
            (EngineState::Expiring, None)
        } else {
            (EngineState::Available, None)
        };
        EngineStatusInfo {
            state,
            reason_code,
            sign_in_state,
            credential_expires_at: credential.and_then(|c| c.expires_at),
            model: st
                .profile
                .as_ref()
                .filter(|p| p.available && !p.model.is_empty())
                .map(|p| p.model.clone()),
            binary: binary.map(|b| b.display().to_string()),
            profile_fetched_at: st.profile_fetched_at,
            paused_by_owner: st.pause.as_ref().map(|(p, _)| p.paused_by_owner),
            auto_pause_reason: st
                .pause
                .as_ref()
                .and_then(|(p, _)| p.auto_pause_reason.clone()),
            pause_checked_at: st.pause.as_ref().map(|(_, at)| *at),
        }
    }

    pub fn status(&self) -> EngineStatusInfo {
        self.status_at(Utc::now())
    }

    /// The profile last fetched, if any.
    pub fn profile(&self) -> Option<EngineProfile> {
        self.state().profile.clone()
    }

    /// Profile refresh loop: at start, on every credential change, and
    /// every [`PROFILE_INTERVAL`]. A refused model token is retried on each
    /// pass so `wallet_empty` / `gateway_session_revoked` clear on their
    /// own once the cause is fixed.
    pub async fn run(self: Arc<Self>, shutdown: Arc<Notify>) {
        loop {
            let changed = self.changed.notified();
            tokio::pin!(changed);
            changed.as_mut().enable();
            self.refresh_profile().await;
            let retry_token = {
                let st = self.state();
                st.token_failure.is_some()
                    && st.token_failure.as_deref() != Some(reason::ENGINE_SIGN_IN_REQUIRED)
            };
            if retry_token {
                let _ = self.model_token().await;
            }
            tokio::select! {
                _ = shutdown.notified() => return,
                _ = &mut changed => {}
                _ = tokio::time::sleep(PROFILE_INTERVAL) => {}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::schema::{CliSection, EngineCredential};
    use crate::engine::test_server::{self, Handler};
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn paths(root: &std::path::Path) -> Paths {
        Paths {
            config_dir: root.join("config"),
            data_dir: root.join("data"),
            runtime_dir: root.join("runtime"),
            machine_context_dir: root.join("config"),
        }
    }

    fn cli(expires_in: chrono::Duration) -> CliSection {
        CliSection {
            token: Some("mt_acme".into()),
            workspace_slug: Some("acme".into()),
            account_user_id: Some("u-1".into()),
            engine_credential: Some(EngineCredential {
                token: "ec_live".into(),
                expires_at: Some(Utc::now() + expires_in),
            }),
            ..Default::default()
        }
    }

    fn profile_json(available: bool, reason: &str) -> String {
        serde_json::json!({
            "available": available, "lane": "openhub",
            "base_url": "https://gw.example/v1", "model": "gpt-5.5",
            "reason_code": reason, "engine_sign_in_state": "active"
        })
        .to_string()
    }

    fn manager(url: &str, root: &std::path::Path, cli: CliSection) -> Arc<EngineManager> {
        EngineManager::new(
            Some(url),
            paths(root),
            WorkspaceTokens::from_cli(Some(&cli)),
            Some(PathBuf::from(
                "/lib/pidash/engine/0.153.4/pidash-agent-engine",
            )),
        )
    }

    #[tokio::test]
    async fn model_token_is_held_in_memory_and_refreshed_under_60s() {
        let calls = Arc::new(AtomicUsize::new(0));
        let c = calls.clone();
        // First token expires in 30 s (inside the skew), the next in 10 min.
        let handler: Handler = Arc::new(move |req| {
            assert_eq!(req.path, super::super::api::MODEL_TOKEN_PATH);
            assert_eq!(req.bearer(), Some("ec_live"), "engine credential only");
            let n = c.fetch_add(1, Ordering::SeqCst);
            let ttl = if n == 0 { 30 } else { 600 };
            let body = serde_json::json!({
                "token": format!("gw_{n}"),
                "expires_at": (Utc::now() + chrono::Duration::seconds(ttl)).to_rfc3339(),
            });
            (200, body.to_string())
        });
        let server = test_server::start(handler).await;
        let tmp = tempfile::tempdir().unwrap();
        let m = manager(&server.url, tmp.path(), cli(chrono::Duration::days(10)));

        let first = m.model_token().await.unwrap();
        assert_eq!(first.token, "gw_0");
        // Under 60 s left → refreshed.
        let second = m.model_token().await.unwrap();
        assert_eq!(second.token, "gw_1");
        // Fresh → served from memory, no request.
        let third = m.model_token().await.unwrap();
        assert_eq!(third.token, "gw_1");
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        // Nothing short-lived on disk.
        assert!(!tmp.path().join("runtime/model.token").exists());
        let found = walk(tmp.path())
            .into_iter()
            .filter(|p| std::fs::read_to_string(p).is_ok_and(|s| s.contains("gw_1")))
            .count();
        assert_eq!(found, 0, "model token written to disk");
    }

    fn walk(dir: &std::path::Path) -> Vec<PathBuf> {
        let mut out = Vec::new();
        if let Ok(rd) = std::fs::read_dir(dir) {
            for e in rd.flatten() {
                let p = e.path();
                if p.is_dir() {
                    out.extend(walk(&p));
                } else {
                    out.push(p);
                }
            }
        }
        out
    }

    #[tokio::test]
    async fn a_token_fetched_with_a_credential_that_changed_meanwhile_is_dropped() {
        // The credential changes (a sign-out) while the fetch is in flight.
        let slot: Arc<std::sync::OnceLock<Arc<EngineManager>>> = Arc::default();
        let s = slot.clone();
        let calls = Arc::new(AtomicUsize::new(0));
        let c = calls.clone();
        let handler: Handler = Arc::new(move |_req| {
            if c.fetch_add(1, Ordering::SeqCst) == 0 {
                s.get().unwrap().credentials_changed();
            }
            let body = serde_json::json!({
                "token": "gw_old",
                "expires_at": (Utc::now() + chrono::Duration::seconds(600)).to_rfc3339(),
            });
            (200, body.to_string())
        });
        let server = test_server::start(handler).await;
        let tmp = tempfile::tempdir().unwrap();
        let m = manager(&server.url, tmp.path(), cli(chrono::Duration::days(10)));
        slot.set(m.clone()).ok().unwrap();

        let err = m.model_token().await.unwrap_err();
        assert_eq!(err.reason_code, reason::ENGINE_SIGN_IN_REQUIRED);
        assert!(
            m.cached_token(Utc::now()).is_none(),
            "stale token not cached"
        );
        // The next request fetches afresh.
        assert_eq!(m.model_token().await.unwrap().token, "gw_old");
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn the_mirrored_sign_in_state_is_written_once() {
        let tmp = tempfile::tempdir().unwrap();
        let p = paths(tmp.path());
        let cfg = crate::config::schema::Config {
            version: 2,
            daemon: crate::config::schema::DaemonConfig {
                cloud_url: "https://cloud.invalid".into(),
                ..Default::default()
            },
            runners: vec![],
            cli: Some(cli(chrono::Duration::days(10))),
            engine: Default::default(),
            auth: Default::default(),
        };
        crate::config::file::write_config(&p, &cfg).unwrap();
        let m = manager(
            "https://cloud.invalid",
            tmp.path(),
            cli(chrono::Duration::days(10)),
        );
        assert_eq!(m.tokens.engine_sign_in_state(), None);
        let profile: EngineProfile =
            serde_json::from_str(&profile_json(false, "engine_disabled")).unwrap();
        m.apply_profile(profile);
        // In memory at once, so the next pass does not rewrite.
        assert_eq!(
            m.tokens.engine_sign_in_state(),
            Some(EngineSignInState::Active)
        );
        // And on disk once the blocking write ran.
        for _ in 0..100 {
            let on_disk = crate::config::file::load_config(&p)
                .unwrap()
                .cli
                .unwrap()
                .engine_sign_in_state;
            if on_disk == Some(EngineSignInState::Active) {
                return;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        panic!("the sign-in state was not mirrored to config.toml");
    }

    #[tokio::test]
    async fn refused_model_token_marks_the_engine_unavailable_with_the_reason() {
        let wallet_empty = Arc::new(std::sync::atomic::AtomicBool::new(true));
        let w = wallet_empty.clone();
        let handler: Handler = Arc::new(move |req| {
            if req.path == super::super::api::PROFILE_PATH {
                return (200, profile_json(true, ""));
            }
            if w.load(Ordering::SeqCst) {
                (402, r#"{"reason_code":"wallet_empty"}"#.into())
            } else {
                let body = serde_json::json!({
                    "token": "gw", "expires_at": (Utc::now() + chrono::Duration::minutes(10)).to_rfc3339()
                });
                (200, body.to_string())
            }
        });
        let server = test_server::start(handler).await;
        let tmp = tempfile::tempdir().unwrap();
        let m = manager(&server.url, tmp.path(), cli(chrono::Duration::days(10)));
        m.refresh_profile().await;
        assert_eq!(m.status().state, EngineState::Available);

        let err = m.model_token().await.unwrap_err();
        assert_eq!(err.reason_code, "wallet_empty");
        let st = m.status();
        assert_eq!(st.state, EngineState::Unavailable);
        assert_eq!(st.reason_code.as_deref(), Some("wallet_empty"));

        wallet_empty.store(false, Ordering::SeqCst);
        m.model_token().await.unwrap();
        assert_eq!(m.status().state, EngineState::Available);
    }

    #[tokio::test]
    async fn profile_writes_the_engine_config_and_names_its_reason_when_unavailable() {
        let available = Arc::new(std::sync::atomic::AtomicBool::new(true));
        let a = available.clone();
        let handler: Handler = Arc::new(move |req| {
            assert_eq!(
                req.bearer(),
                Some("mt_acme"),
                "profile uses the daemon credential"
            );
            if a.load(Ordering::SeqCst) {
                (200, profile_json(true, ""))
            } else {
                (200, profile_json(false, "engine_disabled"))
            }
        });
        let server = test_server::start(handler).await;
        let tmp = tempfile::tempdir().unwrap();
        let m = manager(&server.url, tmp.path(), cli(chrono::Duration::days(10)));
        assert_eq!(
            m.status().reason_code.as_deref(),
            Some(reason::PROFILE_PENDING)
        );
        m.refresh_profile().await;
        let home = tmp.path().join("data/engine/u-1/codex-home");
        assert_eq!(m.codex_home(), home);
        let written = std::fs::read_to_string(home.join("config.toml")).unwrap();
        assert!(written.contains("https://gw.example/v1"));
        assert!(written.contains("model-token"));
        let st = m.status();
        assert_eq!(st.state, EngineState::Available);
        assert_eq!(st.model.as_deref(), Some("gpt-5.5"));

        available.store(false, Ordering::SeqCst);
        m.refresh_profile().await;
        let st = m.status();
        assert_eq!(st.state, EngineState::Unavailable);
        assert_eq!(st.reason_code.as_deref(), Some("engine_disabled"));
    }

    #[tokio::test]
    async fn sign_in_states() {
        let handler: Handler = Arc::new(|_| (200, profile_json(true, "")));
        let server = test_server::start(handler).await;
        let tmp = tempfile::tempdir().unwrap();

        // Active, then expiring within three days.
        let m = manager(&server.url, tmp.path(), cli(chrono::Duration::days(10)));
        m.refresh_profile().await;
        assert_eq!(m.status().sign_in_state, EngineSignInState::Active);
        let st = m.status_at(Utc::now() + chrono::Duration::days(8));
        assert_eq!(st.sign_in_state, EngineSignInState::Expiring);
        assert_eq!(st.state, EngineState::Expiring);
        // Expired after its 14 days: signed out, with the pause reason.
        let st = m.status_at(Utc::now() + chrono::Duration::days(11));
        assert_eq!(st.sign_in_state, EngineSignInState::Expired);
        assert_eq!(st.state, EngineState::SignedOut);
        assert_eq!(
            st.reason_code.as_deref(),
            Some(reason::ENGINE_SIGN_IN_REQUIRED)
        );

        // Never had one.
        let mut never = cli(chrono::Duration::days(1));
        never.engine_credential = None;
        let m = manager(&server.url, tmp.path(), never.clone());
        assert_eq!(m.status().sign_in_state, EngineSignInState::Never);
        assert_eq!(m.status().state, EngineState::SignedOut);
        let err = m.model_token().await.unwrap_err();
        assert_eq!(err.reason_code, reason::ENGINE_SIGN_IN_REQUIRED);

        // Explicitly signed out (mirrored): stays signed_out, not "never".
        never.engine_sign_in_state = Some(EngineSignInState::SignedOut);
        let m = manager(&server.url, tmp.path(), never);
        assert_eq!(m.status().sign_in_state, EngineSignInState::SignedOut);

        // The server refusing the credential → expired.
        let refuse: Handler = Arc::new(|_| (401, String::new()));
        let server = test_server::start(refuse).await;
        let m = manager(&server.url, tmp.path(), cli(chrono::Duration::days(10)));
        let err = m.model_token().await.unwrap_err();
        assert_eq!(err.reason_code, reason::ENGINE_SIGN_IN_REQUIRED);
        assert_eq!(m.status().sign_in_state, EngineSignInState::Expired);
    }

    #[tokio::test]
    async fn pause_state_is_cached_from_the_server_and_reported() {
        let paused = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let p = paused.clone();
        let handler: Handler = Arc::new(move |req| {
            assert_eq!(req.bearer(), Some("mt_acme"));
            if req.method == "PATCH" {
                let on = req.json()["engine_paused_by_owner"].as_bool().unwrap();
                p.store(on, Ordering::SeqCst);
                // An older server answers 204 with no body.
                return (200, String::new());
            }
            let body = serde_json::json!({
                "engine_paused_by_owner": p.load(Ordering::SeqCst),
                "engine_auto_pause_reason": "wallet_empty",
                "runners": [],
            });
            (200, body.to_string())
        });
        let server = test_server::start(handler).await;
        let tmp = tempfile::tempdir().unwrap();
        let m = manager(&server.url, tmp.path(), cli(chrono::Duration::days(10)));
        let id = uuid::Uuid::new_v4();
        assert_eq!(m.status().paused(), None, "unknown before the server says");

        m.refresh_machine("mt_acme", id).await.unwrap();
        let st = m.status();
        assert_eq!(st.paused_by_owner, Some(false));
        assert_eq!(st.auto_pause_reason.as_deref(), Some("wallet_empty"));
        assert_eq!(st.paused(), Some(true));

        let st = m.set_paused("mt_acme", id, true).await.unwrap();
        assert_eq!(st.paused_by_owner, Some(true));
        // The bodyless answer keeps the automatic reason we knew.
        assert_eq!(st.auto_pause_reason.as_deref(), Some("wallet_empty"));
        assert!(st.pause_checked_at.is_some());
        let sent = &server.requests_to(&super::super::api::dev_machine_path(id));
        assert_eq!(sent.last().unwrap().json()["engine_paused_by_owner"], true);
    }

    #[test]
    fn no_engine_binary_is_unavailable() {
        let tmp = tempfile::tempdir().unwrap();
        let m = EngineManager::new(
            None,
            paths(tmp.path()),
            WorkspaceTokens::from_cli(Some(&cli(chrono::Duration::days(10)))),
            None,
        );
        let st = m.status();
        assert_eq!(st.state, EngineState::Unavailable);
        assert_eq!(
            st.reason_code.as_deref(),
            Some(reason::ENGINE_NOT_INSTALLED)
        );
    }

    #[test]
    fn a_binary_installed_after_startup_makes_the_engine_available() {
        // Resolution is process-wide and immutable once successful. Run in
        // a child so this test does not change other tests' engine lookup.
        const CHILD: &str = "PIDASH_TEST_LATE_ENGINE_INSTALL";
        if std::env::var_os(CHILD).is_none() {
            let output = std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "engine::manager::tests::a_binary_installed_after_startup_makes_the_engine_available"])
                .env(CHILD, "1")
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stdout)
            );
            return;
        }
        let tmp = tempfile::tempdir().unwrap();
        let m = EngineManager::new(
            None,
            paths(tmp.path()),
            WorkspaceTokens::from_cli(Some(&cli(chrono::Duration::days(10)))),
            None,
        );
        m.apply_profile(serde_json::from_str(&profile_json(true, "")).unwrap());
        assert_eq!(
            m.status().reason_code.as_deref(),
            Some(reason::ENGINE_NOT_INSTALLED)
        );
        let installed = tmp.path().join("pidash-agent-engine");
        std::fs::write(&installed, "fake engine").unwrap();
        super::super::locate::init_at_daemon_start(&crate::config::schema::EngineSection {
            binary: Some(installed.clone()),
            ..Default::default()
        })
        .unwrap();
        let status = m.status();
        assert_eq!(status.state, EngineState::Available);
        assert_eq!(status.binary, Some(installed.display().to_string()));
        assert_eq!(status.reason_code, None);
    }
}
