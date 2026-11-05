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

use crate::config::schema::CliSection;

/// Only dev-machine tokens can open runner and machine sessions. A
/// user-scoped API token left in `[cli].token` by an old build is still
/// used by the CRUD subcommands, but never by the daemon.
fn is_machine_token(token: &str) -> bool {
    token.starts_with("mt_")
}

#[derive(Clone, Default)]
pub(crate) struct WorkspaceTokens {
    inner: Arc<RwLock<Inner>>,
    changed: Arc<Notify>,
}

#[derive(Default)]
struct Inner {
    cli: CliSection,
    /// Tokens the cloud rejected as revoked. Kept until a reload no
    /// longer lists them (a new login replaces them).
    revoked: HashSet<String>,
}

impl WorkspaceTokens {
    pub(crate) fn from_cli(cli: Option<&CliSection>) -> Self {
        let store = Self::default();
        store.write().cli = cli.cloned().unwrap_or_default();
        store
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
    pub(crate) fn mark_revoked(&self, token: &str) -> Option<String> {
        let workspace = {
            let mut inner = self.write();
            inner.revoked.insert(token.to_string());
            inner
                .cli
                .workspace_tokens()
                .into_iter()
                .find(|(_, t)| t == token)
                .and_then(|(slug, _)| slug)
        };
        self.changed.notify_waiters();
        workspace
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
    pub(crate) fn engine_credential(&self) -> Option<crate::config::schema::EngineCredential> {
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
}
