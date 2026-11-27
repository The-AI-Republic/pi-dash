//! The daemon's workspace credentials: one dev-machine `mt_` token per
//! workspace (design §4.9, PIDESKAPP-122).
//!
//! Replaces the single `shared_machine_token` the supervisor used to hand
//! every runner and the machine client. Each runner authenticates with the
//! token of its block's `workspace_slug`; the one machine session is opened
//! with any token still live; machine-control commands use the token of the
//! workspace they name.
//!
//! The store is shared (cheap clones) between the supervisor, the machine
//! control session and the IPC server. A token the cloud answers
//! `machine_token_revoked` / `membership_revoked` for is marked revoked here,
//! so every user of the store stops offering it at once; re-reading
//! `config.toml` ([`WorkspaceTokens::reload`]) picks up tokens a later
//! `pidash auth login` wrote.

use std::collections::HashSet;
use std::sync::{Arc, RwLock};

use tokio::sync::Notify;

use crate::config::schema::{AuthEventReason, CliSection};
use crate::util::paths::Paths;

/// Only dev-machine tokens can open runner and machine sessions. A
/// user-scoped API token left in `[cli].token` by an old build is still
/// used by the CRUD subcommands, but never by the daemon.
fn is_machine_token(token: &str) -> bool {
    token.starts_with("mt_")
}

/// The `signed_out` reason a fatal transport answer stands for (design
/// §6.1): the 60 days without use ran out (`expired`), the computer was
/// removed on the web (`removed`), or the server revoked the credential.
pub(crate) fn fatal_reason(e: &crate::cloud::http::TransportError) -> AuthEventReason {
    use crate::cloud::http::TransportError;
    match e {
        TransportError::MachineTokenExpired => AuthEventReason::Expired,
        TransportError::DevMachineRevoked => AuthEventReason::Removed,
        _ => AuthEventReason::Revoked,
    }
}

/// The same, for an engine API `reason_code`.
pub(crate) fn fatal_reason_code(code: Option<&str>) -> AuthEventReason {
    match code {
        Some("machine_token_expired") => AuthEventReason::Expired,
        Some("dev_machine_revoked") => AuthEventReason::Removed,
        _ => AuthEventReason::Revoked,
    }
}

#[derive(Clone, Default)]
pub(crate) struct WorkspaceTokens {
    inner: Arc<RwLock<Inner>>,
    changed: Arc<Notify>,
    /// Where to record the daemon's own sign-out (desktop_unified_signin
    /// §6.1) when its last usable credential goes fatal. `None` in tests
    /// and in short-lived stores that never persist anything.
    persist: Option<Arc<Paths>>,
}

#[derive(Default)]
struct Inner {
    cli: CliSection,
    /// Tokens the cloud rejected as revoked. Kept until a reload no
    /// longer lists them (a new login replaces them).
    revoked: HashSet<String>,
    /// The daemon already recorded its sign-out for the current set of
    /// tokens: the transition to "no usable credential" is recorded once.
    /// Cleared when a reload brings a live token back.
    signed_out_recorded: bool,
}

impl WorkspaceTokens {
    pub(crate) fn from_cli(cli: Option<&CliSection>) -> Self {
        let store = Self::default();
        store.write().cli = cli.cloned().unwrap_or_default();
        store
    }

    /// Record the daemon's own sign-out in `paths`' `config.toml` when the
    /// last usable credential goes fatal (see [`Self::mark_fatal`]).
    pub(crate) fn with_persistence(mut self, paths: Paths) -> Self {
        self.persist = Some(Arc::new(paths));
        self
    }

    fn read(&self) -> std::sync::RwLockReadGuard<'_, Inner> {
        self.inner.read().expect("workspace token store poisoned")
    }

    fn write(&self) -> std::sync::RwLockWriteGuard<'_, Inner> {
        self.inner.write().expect("workspace token store poisoned")
    }

    /// The live token for `workspace` (a runner block's or a command's
    /// slug; `None` = the default workspace), or `None` when the workspace
    /// is signed out.
    pub(crate) fn token_for(&self, workspace: Option<&str>) -> Option<String> {
        let inner = self.read();
        inner
            .cli
            .token_for_workspace(workspace)
            .filter(|t| is_machine_token(t) && !inner.revoked.contains(*t))
            .map(str::to_string)
    }

    /// Live `(workspace, token)` pairs, default workspace first: the order
    /// the machine session tries them in.
    pub(crate) fn session_candidates(&self) -> Vec<(Option<String>, String)> {
        let inner = self.read();
        inner
            .cli
            .workspace_tokens()
            .into_iter()
            .filter(|(_, t)| is_machine_token(t) && !inner.revoked.contains(t))
            .collect()
    }

    /// The cloud rejected `token` as revoked: stop offering it. Returns the
    /// workspace it belonged to (`None` for an unbound default token).
    /// Records nothing on disk; use [`Self::mark_fatal`] for an answer
    /// that says why.
    #[cfg(test)]
    pub(crate) fn mark_revoked(&self, token: &str) -> Option<String> {
        self.mark_tokens(&[token.to_string()]).0
    }

    /// The cloud refused `token` for good (`reason`: `revoked`, `expired`,
    /// or `removed` for a computer removed on the web). Like
    /// [`Self::mark_revoked`]; and when this leaves the daemon with no
    /// usable credential, the daemon's own sign-out is recorded once
    /// (desktop_unified_signin §6.1, §12 rule 1) and the dead credentials
    /// are cleared from `config.toml`, as `pidash auth logout` clears them.
    pub(crate) fn mark_fatal(&self, token: &str, reason: AuthEventReason) -> Option<String> {
        let (workspace, went_dark) = self.mark_tokens(&[token.to_string()]);
        if went_dark {
            self.record_signed_out(reason);
        }
        workspace
    }

    /// The machine itself was removed on the web: every token it holds is
    /// dead. Records the `removed` sign-out (once).
    pub(crate) fn mark_machine_removed(&self) {
        let all: Vec<String> = self
            .read()
            .cli
            .workspace_tokens()
            .into_iter()
            .map(|(_, t)| t)
            .collect();
        let (_, went_dark) = self.mark_tokens(&all);
        if went_dark {
            self.record_signed_out(AuthEventReason::Removed);
        }
    }

    /// Mark `tokens` revoked. Returns the first one's workspace, and
    /// whether this is the first time the store has no usable credential
    /// since it last had one (decided under the lock, so concurrent
    /// callers see the transition once).
    fn mark_tokens(&self, tokens: &[String]) -> (Option<String>, bool) {
        let (workspace, went_dark) = {
            let mut inner = self.write();
            let had_live = Self::has_live(&inner);
            for t in tokens {
                inner.revoked.insert(t.clone());
            }
            let workspace = tokens.first().and_then(|token| {
                inner
                    .cli
                    .workspace_tokens()
                    .into_iter()
                    .find(|(_, t)| t == token)
                    .and_then(|(slug, _)| slug)
            });
            let went_dark = had_live && !Self::has_live(&inner) && !inner.signed_out_recorded;
            if went_dark {
                inner.signed_out_recorded = true;
            }
            (workspace, went_dark)
        };
        self.changed.notify_waiters();
        (workspace, went_dark)
    }

    fn has_live(inner: &Inner) -> bool {
        inner
            .cli
            .workspace_tokens()
            .iter()
            .any(|(_, t)| is_machine_token(t) && !inner.revoked.contains(t))
    }

    /// Persist the daemon's sign-out: one `signed_out` event (seq + 1) and
    /// the dead credentials cleared, in one locked config write. Skipped
    /// when `config.toml` no longer holds the tokens that died: a
    /// `pidash auth logout` already cleared them (and recorded its own
    /// event), or a new sign-in replaced them (the daemon picks those up
    /// on reload). Best-effort: a failure is logged, never fatal.
    fn record_signed_out(&self, reason: AuthEventReason) {
        let Some(paths) = self.persist.as_deref() else {
            return;
        };
        let revoked = self.read().revoked.clone();
        let mut recorded = false;
        let result = crate::config::file::mutate_config(paths, |cfg| {
            let Some(cli) = cfg.cli.as_mut() else {
                return Ok(());
            };
            let on_disk: Vec<String> = cli
                .workspace_tokens()
                .into_iter()
                .map(|(_, t)| t)
                .filter(|t| is_machine_token(t))
                .collect();
            if on_disk.is_empty() || on_disk.iter().any(|t| !revoked.contains(t)) {
                return Ok(());
            }
            let draft = crate::cli::runner_ops::AuthEventDraft {
                kind: crate::config::schema::AuthEventKind::SignedOut,
                reason: Some(reason),
                email: cli.account_email.clone(),
                user_id: cli.account_user_id.clone(),
                cloud_url: cfg.daemon.cloud_url.clone(),
            };
            cli.clear_all_tokens();
            cli.engine_credential = None;
            cli.engine_sign_in_state = Some(crate::config::schema::EngineSignInState::SignedOut);
            crate::cli::runner_ops::apply_auth_event(cfg, &draft);
            recorded = true;
            Ok(())
        });
        match result {
            Ok(cfg) if recorded => {
                tracing::warn!(
                    reason = reason.as_str(),
                    "this computer's daemon credential is no longer valid; recorded the sign-out \
                     (`pidash auth login` signs it in again)"
                );
                // Mirror the cleared credentials, keeping the recorded flag:
                // no live token came back.
                let mut inner = self.write();
                inner.cli = cfg.cli.unwrap_or_default();
                inner.revoked.clear();
            }
            Ok(_) => {}
            Err(e) => tracing::warn!("recording the daemon's sign-out failed: {e:#}"),
        }
    }

    #[cfg(test)]
    pub(crate) fn is_revoked(&self, token: &str) -> bool {
        self.read().revoked.contains(token)
    }

    /// Replace the held credentials with `cli` (re-read from disk).
    /// Revocation marks of tokens that are no longer listed are dropped.
    /// Returns whether anything a session would use changed.
    pub(crate) fn reload(&self, cli: Option<&CliSection>) -> bool {
        let changed = {
            let mut inner = self.write();
            let next = cli.cloned().unwrap_or_default();
            let changed = next.workspace_tokens() != inner.cli.workspace_tokens();
            let listed: HashSet<String> = next
                .workspace_tokens()
                .into_iter()
                .map(|(_, t)| t)
                .collect();
            inner.revoked.retain(|t| listed.contains(t));
            inner.cli = next;
            if Self::has_live(&inner) {
                inner.signed_out_recorded = false;
            }
            changed
        };
        if changed {
            self.changed.notify_waiters();
        }
        changed
    }

    /// Resolves the next time the set of live tokens changes (a revocation
    /// or a reload). Create the future before checking the condition you
    /// wait on, so a change in between is not missed.
    pub(crate) fn changed(&self) -> Arc<Notify> {
        self.changed.clone()
    }

    /// Workspaces with a token on disk (live or revoked), default first.
    pub(crate) fn known_workspaces(&self) -> Vec<String> {
        self.read().cli.workspace_slugs()
    }

    /// The machine's engine credential, when one with a token is held.
    pub(crate) fn engine_credential(&self) -> Option<crate::config::schema::PidashAgentCredential> {
        self.read()
            .cli
            .engine_credential
            .clone()
            .filter(|c| !c.token.is_empty())
    }

    /// The engine sign-in state recorded in `config.toml`.
    pub(crate) fn engine_sign_in_state(&self) -> Option<crate::config::schema::EngineSignInState> {
        self.read().cli.engine_sign_in_state
    }

    /// Record `state` as the engine sign-in state in memory, after it was
    /// (or while it is being) mirrored into `config.toml`.
    pub(crate) fn set_engine_sign_in_state(
        &self,
        state: Option<crate::config::schema::EngineSignInState>,
    ) {
        self.write().cli.engine_sign_in_state = state;
    }

    /// The signed-in account recorded by `pidash auth login`.
    pub(crate) fn account(&self) -> (Option<String>, Option<String>) {
        let inner = self.read();
        (
            inner.cli.account_user_id.clone(),
            inner.cli.account_email.clone(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::schema::WorkspaceCredential;

    fn two_workspaces() -> CliSection {
        CliSection {
            token: Some("mt_acme".into()),
            workspace_slug: Some("acme".into()),
            workspaces: vec![WorkspaceCredential {
                slug: "beta".into(),
                token: "mt_beta".into(),
            }],
            ..Default::default()
        }
    }

    #[test]
    fn each_runner_workspace_gets_its_own_token() {
        let store = WorkspaceTokens::from_cli(Some(&two_workspaces()));
        assert_eq!(store.token_for(Some("acme")).as_deref(), Some("mt_acme"));
        assert_eq!(store.token_for(Some("beta")).as_deref(), Some("mt_beta"));
        assert_eq!(store.token_for(None).as_deref(), Some("mt_acme"));
        assert_eq!(
            store.token_for(Some("gamma")),
            None,
            "no token → signed out"
        );
    }

    #[test]
    fn legacy_single_token_config_serves_its_one_workspace() {
        let cli = CliSection {
            token: Some("mt_only".into()),
            workspace_slug: Some("acme".into()),
            ..Default::default()
        };
        let store = WorkspaceTokens::from_cli(Some(&cli));
        assert_eq!(store.token_for(Some("acme")).as_deref(), Some("mt_only"));
        assert_eq!(store.token_for(None).as_deref(), Some("mt_only"));
        assert_eq!(
            store.session_candidates(),
            vec![(Some("acme".to_string()), "mt_only".to_string())]
        );
    }

    #[test]
    fn non_machine_tokens_are_never_used_by_the_daemon() {
        let cli = CliSection {
            token: Some("pidash_api_user_token".into()),
            workspace_slug: Some("acme".into()),
            ..Default::default()
        };
        let store = WorkspaceTokens::from_cli(Some(&cli));
        assert_eq!(store.token_for(Some("acme")), None);
        assert!(store.session_candidates().is_empty());
    }

    #[test]
    fn revoking_one_token_signs_out_only_its_workspace() {
        let store = WorkspaceTokens::from_cli(Some(&two_workspaces()));
        assert_eq!(store.mark_revoked("mt_acme").as_deref(), Some("acme"));
        assert_eq!(store.token_for(Some("acme")), None);
        assert_eq!(store.token_for(Some("beta")).as_deref(), Some("mt_beta"));
        assert_eq!(
            store.session_candidates(),
            vec![(Some("beta".to_string()), "mt_beta".to_string())]
        );
        assert_eq!(store.known_workspaces(), vec!["acme", "beta"]);
    }

    #[test]
    fn reload_with_a_fresh_token_clears_the_revocation() {
        let store = WorkspaceTokens::from_cli(Some(&two_workspaces()));
        store.mark_revoked("mt_acme");
        let mut next = two_workspaces();
        next.token = Some("mt_acme_2".into());
        assert!(store.reload(Some(&next)));
        assert_eq!(store.token_for(Some("acme")).as_deref(), Some("mt_acme_2"));
        assert!(!store.is_revoked("mt_acme_2"));
        assert!(!store.reload(Some(&next)), "same tokens → no change");
    }

    fn persisted(root: &std::path::Path, cli: CliSection, seq: Option<u64>) -> Paths {
        use crate::config::schema::{AuthEvent, AuthEventKind, Config, DaemonConfig};
        let paths = Paths {
            config_dir: root.join("config"),
            data_dir: root.join("data"),
            runtime_dir: root.join("runtime"),
            machine_context_dir: root.join("config"),
        };
        let cfg = Config {
            version: 2,
            daemon: DaemonConfig {
                cloud_url: "https://cloud.invalid".into(),
                ..DaemonConfig::default()
            },
            runners: vec![],
            cli: Some(CliSection {
                account_email: Some("dev@example.com".into()),
                account_user_id: Some("u-1".into()),
                ..cli
            }),
            engine: Default::default(),
            auth: Default::default(),
            auth_event: seq.map(|seq| AuthEvent {
                seq,
                kind: AuthEventKind::SignedIn,
                reason: None,
                account: None,
                at: chrono::Utc::now(),
            }),
        };
        crate::config::file::write_config(&paths, &cfg).unwrap();
        paths
    }

    fn on_disk(paths: &Paths) -> crate::config::schema::Config {
        crate::config::file::load_config(paths).unwrap()
    }

    #[test]
    fn the_last_fatal_token_records_one_signed_out_event_and_clears_credentials() {
        use crate::config::schema::AuthEventKind;
        let tmp = tempfile::tempdir().unwrap();
        let mut cli = two_workspaces();
        cli.engine_credential = Some(crate::config::schema::PidashAgentCredential {
            token: "ec_live".into(),
            expires_at: None,
        });
        let paths = persisted(tmp.path(), cli.clone(), Some(4));
        let store = WorkspaceTokens::from_cli(Some(&cli)).with_persistence(paths.clone());

        store.mark_fatal("mt_beta", AuthEventReason::Revoked);
        let after_one = on_disk(&paths);
        assert_eq!(
            after_one.auth_event.as_ref().unwrap().seq,
            4,
            "beta alone: no event"
        );
        assert_eq!(after_one.cli.unwrap().workspace_tokens().len(), 2);

        store.mark_fatal("mt_acme", AuthEventReason::Revoked);
        let after = on_disk(&paths);
        let event = after.auth_event.expect("the daemon's sign-out is recorded");
        assert_eq!(event.seq, 5);
        assert_eq!(event.kind, AuthEventKind::SignedOut);
        assert_eq!(event.reason, Some(AuthEventReason::Revoked));
        let account = event.account.unwrap();
        assert_eq!(account.email.as_deref(), Some("dev@example.com"));
        assert_eq!(account.cloud_url, "https://cloud.invalid");
        let cli = after.cli.unwrap();
        assert!(cli.workspace_tokens().is_empty(), "dead tokens cleared");
        assert!(
            cli.engine_credential.is_none(),
            "engine signed out as logout does"
        );
        assert!(store.session_candidates().is_empty());

        // Further fatal answers (other loops still holding the token) and a
        // machine-removed answer record nothing more.
        store.mark_fatal("mt_acme", AuthEventReason::Expired);
        store.mark_machine_removed();
        assert_eq!(on_disk(&paths).auth_event.unwrap().seq, 5);
    }

    #[test]
    fn reasons_map_from_the_server_answer() {
        use crate::cloud::http::TransportError;
        assert_eq!(
            fatal_reason(&TransportError::MachineTokenExpired),
            AuthEventReason::Expired
        );
        assert_eq!(
            fatal_reason(&TransportError::DevMachineRevoked),
            AuthEventReason::Removed
        );
        assert_eq!(
            fatal_reason(&TransportError::MachineTokenRevoked),
            AuthEventReason::Revoked
        );
        assert_eq!(
            fatal_reason(&TransportError::MembershipRevoked),
            AuthEventReason::Revoked
        );
        assert_eq!(
            fatal_reason_code(Some("machine_token_expired")),
            AuthEventReason::Expired
        );
        assert_eq!(
            fatal_reason_code(Some("machine_token_invalid")),
            AuthEventReason::Revoked
        );
    }

    #[test]
    fn a_removed_machine_records_removed() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = persisted(tmp.path(), two_workspaces(), None);
        let store =
            WorkspaceTokens::from_cli(Some(&two_workspaces())).with_persistence(paths.clone());
        store.mark_machine_removed();
        let event = on_disk(&paths).auth_event.unwrap();
        assert_eq!(event.seq, 1);
        assert_eq!(event.reason, Some(AuthEventReason::Removed));
    }

    #[test]
    fn a_newer_token_on_disk_is_not_cleared() {
        // A re-login replaced the token (the server revoked the old one)
        // before the daemon reloaded: nothing is recorded or cleared.
        let tmp = tempfile::tempdir().unwrap();
        let only_acme = CliSection {
            token: Some("mt_acme".into()),
            workspace_slug: Some("acme".into()),
            ..Default::default()
        };
        let fresh = CliSection {
            token: Some("mt_acme_2".into()),
            ..only_acme.clone()
        };
        let paths = persisted(tmp.path(), fresh, Some(7));
        let store = WorkspaceTokens::from_cli(Some(&only_acme)).with_persistence(paths.clone());
        store.mark_fatal("mt_acme", AuthEventReason::Revoked);
        let after = on_disk(&paths);
        assert_eq!(after.auth_event.unwrap().seq, 7);
        assert_eq!(after.cli.unwrap().token.as_deref(), Some("mt_acme_2"));
    }

    #[test]
    fn a_new_sign_in_rearms_the_record() {
        let tmp = tempfile::tempdir().unwrap();
        let only_acme = CliSection {
            token: Some("mt_acme".into()),
            workspace_slug: Some("acme".into()),
            ..Default::default()
        };
        let paths = persisted(tmp.path(), only_acme.clone(), None);
        let store = WorkspaceTokens::from_cli(Some(&only_acme)).with_persistence(paths.clone());
        store.mark_fatal("mt_acme", AuthEventReason::Expired);
        assert_eq!(on_disk(&paths).auth_event.unwrap().seq, 1);

        // `pidash auth login` writes a new token; the daemon reloads it.
        let relogin = CliSection {
            token: Some("mt_acme_2".into()),
            ..only_acme
        };
        crate::config::file::mutate_config(&paths, |cfg| {
            cfg.cli.as_mut().unwrap().token = relogin.token.clone();
            Ok(())
        })
        .unwrap();
        store.reload(Some(&relogin));
        store.mark_fatal("mt_acme_2", AuthEventReason::Revoked);
        let event = on_disk(&paths).auth_event.unwrap();
        assert_eq!(event.seq, 2);
        assert_eq!(event.reason, Some(AuthEventReason::Revoked));
    }
}
