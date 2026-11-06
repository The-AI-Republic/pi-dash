//! `pidash auth logout` — revoke the dev-machine tokens server-side and
//! clear them locally. Leaves `[[runner]]` blocks untouched; the runners of
//! a signed-out workspace show as `workspace_signed_out` until
//! `pidash auth login --workspace <slug>` signs it in again.

use anyhow::{Context, Result};
use clap::Args as ClapArgs;
use std::time::Duration;

use crate::cli::runner_ops;
use crate::config::file;
use crate::util::paths::Paths;

#[derive(Debug, ClapArgs)]
pub struct Args {
    /// Skip the server-side revoke and only clear the token locally.
    /// Useful when the cloud is unreachable.
    #[arg(long)]
    pub local_only: bool,

    /// Sign out only this workspace; the other workspaces on this computer
    /// stay signed in. Without it, every workspace is signed out.
    #[arg(long)]
    pub workspace: Option<String>,

    /// Sign out only the built-in engine: revoke this computer's engine
    /// credential. The daemon and its runners stay signed in; engine
    /// runners pause until `pidash login --engine`. The desktop app does not
    /// renew an engine signed out this way.
    #[arg(long, conflicts_with = "workspace")]
    pub engine: bool,
}

/// Revoke this machine's engine credential (unless `local_only`) and
/// forget it, recording `signed_out`. Returns whether one was held.
pub(crate) async fn sign_out_engine(paths: &Paths, local_only: bool) -> Result<bool> {
    let Some(cfg) = file::load_config_opt(paths)? else {
        return Ok(false);
    };
    let cli = cfg.cli.clone().unwrap_or_default();
    let held = cli
        .engine_credential
        .as_ref()
        .is_some_and(|c| !c.token.is_empty());
    if !local_only {
        let machine_token = cli
            .workspace_tokens()
            .into_iter()
            .map(|(_, t)| t)
            .find(|t| t.starts_with("mt_"));
        match machine_token {
            Some(mt) if !cfg.daemon.cloud_url.is_empty() => {
                let api = crate::engine::api::EngineApi::new(&cfg.daemon.cloud_url)?;
                if let Err(e) = api.revoke_credential(&mt).await {
                    // Still forget it locally, so the user is not stuck.
                    tracing::warn!("revoking the engine credential failed: {e}");
                    eprintln!("warning: the server did not confirm the engine sign-out: {e}");
                }
            }
            _ => {
                if held {
                    eprintln!(
                        "warning: no daemon credential to revoke the engine credential with; forgetting it locally"
                    );
                }
            }
        }
    }
    runner_ops::store_engine_credential(
        paths,
        None,
        crate::config::schema::EngineSignInState::SignedOut,
    )
    .context("forgetting the engine credential")?;
    Ok(held)
}

pub async fn run(args: Args, paths: &Paths) -> Result<()> {
    // Signing out forgets keychain items the keychain can never return, so
    // a damaged keychain can't keep this computer signed in. Only the items
    // of what is signed out: a workspace or engine sign-out never forgets
    // another credential's item.
    use crate::config::secrets::{ForgetScope, ITEM_ENGINE_CREDENTIAL, forget_unreadable};
    let engine_item = || vec![ITEM_ENGINE_CREDENTIAL.to_string()];
    let forget = forget_unreadable(
        paths,
        if args.engine {
            ForgetScope::Items(engine_item())
        } else if args.workspace.is_some() {
            ForgetScope::Items(Vec::new())
        } else {
            ForgetScope::All
        },
    );
    if args.engine {
        let held = sign_out_engine(paths, args.local_only).await?;
        crate::service::reload::notify_credentials_changed(paths).await;
        if held {
            println!(
                "Signed out the built-in engine. Run `pidash login --engine` to sign it in again."
            );
        } else {
            println!("The built-in engine was not signed in; recorded it as signed out.");
        }
        return Ok(());
    }
    let cfg = if paths.config_path().exists() {
        Some(file::load_config(paths)?)
    } else {
        None
    };
    let cli = cfg.as_ref().and_then(|c| c.cli.clone()).unwrap_or_default();
    let workspace = args
        .workspace
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let mut tokens: Vec<String> = match workspace {
        Some(slug) => cli
            .token_for_workspace(Some(slug))
            .filter(|_| cli.workspace_slugs().iter().any(|s| s == slug))
            .map(|t| vec![t.to_string()])
            .unwrap_or_default(),
        None => cli.workspace_tokens().into_iter().map(|(_, t)| t).collect(),
    };
    // The keychain items being signed out that it cannot return (duplicate
    // items): their tokens are still read one duplicate at a time and
    // revoked with the rest, so forgetting the items leaves no live token.
    let scope = match workspace {
        Some(slug) => ForgetScope::Items(ForgetScope::workspace(
            slug,
            cli.workspace_slug.as_deref() == Some(slug),
        )),
        None => ForgetScope::All,
    };
    let broken_in_scope = cfg.as_ref().is_some_and(|c| {
        c.auth.broken_keyring_items.iter().any(|i| match &scope {
            ForgetScope::All => true,
            ForgetScope::Items(items) => items.contains(i),
        })
    });
    if let ForgetScope::Items(items) = &scope {
        forget.add(items.clone());
    }
    if let Some(c) = cfg.as_ref() {
        for t in crate::config::secrets::recover_broken_values(paths, c, &scope) {
            if t.starts_with("mt_") && !tokens.contains(&t) {
                tokens.push(t);
            }
        }
    }
    if tokens.is_empty() && !broken_in_scope {
        match workspace {
            Some(slug) => println!("Workspace {slug:?} is not signed in — nothing to revoke."),
            None => {
                // No workspace is signed in (e.g. each was signed out with
                // `--workspace`), but an engine credential can outlive them
                // and still mint wallet-billed model tokens. A full logout
                // always signs the engine out; with no machine token left to
                // revoke it with, it is forgotten locally (with a warning).
                if cfg.is_some() && sign_out_engine(paths, args.local_only).await? {
                    crate::service::reload::notify_credentials_changed(paths).await;
                    println!("Not logged in to any workspace; signed out the built-in engine.");
                } else {
                    println!("Not logged in — nothing to revoke.");
                }
            }
        }
        return Ok(());
    }

    // Signing out the last workspace leaves no machine token to revoke
    // the engine credential with later, so it counts as a full logout.
    let last_workspace = workspace.is_some_and(|slug| {
        let signed_in = cli.workspace_tokens();
        signed_in.len() == 1 && signed_in[0].0.as_deref() == Some(slug)
    });
    if workspace.is_none() || last_workspace {
        // `pidash auth logout` signs the engine out too (design §4.3). Its
        // credential is revoked first, while a machine token still works.
        forget.add(engine_item());
        sign_out_engine(paths, args.local_only).await?;
    }

    if !args.local_only {
        let cloud_url = cfg
            .as_ref()
            .map(|c| c.daemon.cloud_url.clone())
            .unwrap_or_default();
        if cloud_url.is_empty() {
            anyhow::bail!(
                "config.toml is missing [daemon].cloud_url — pass --local-only to clear the local token without contacting the cloud"
            );
        }
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(15))
            .build()?;
        let url = format!("{cloud_url}/api/v1/auth/revoke/");
        for token in &tokens {
            let resp = client
                .post(&url)
                .header("X-Api-Key", token)
                .json(&serde_json::json!({}))
                .send()
                .await
                .with_context(|| format!("POST {url}"))?;
            // 200 or 401 are both acceptable: the cloud either revoked it
            // for us (200) or the token was already invalid (401). Anything
            // else, we surface but still clear locally so the user isn't
            // stuck with a half-state.
            let status = resp.status();
            if !status.is_success() && status.as_u16() != 401 {
                let body = resp.text().await.unwrap_or_default();
                tracing::warn!("revoke endpoint returned HTTP {status}: {body}");
            }
        }
    }

    match workspace {
        Some(slug) => {
            runner_ops::clear_workspace_token(paths, slug)
                .with_context(|| format!("clearing workspace {slug:?}'s token"))?;
            println!("Signed out workspace {slug}.");
        }
        None => {
            runner_ops::clear_cli_token(paths).context("clearing tokens from config.toml")?;
            println!("Logged out (the built-in engine too).");
        }
    }
    crate::service::reload::notify_credentials_changed(paths).await;

    let cfg = file::load_config(paths).ok();
    if let Some(c) = &cfg {
        let affected = c
            .runners
            .iter()
            .filter(|r| match workspace {
                Some(slug) => r.workspace_slug.as_deref() == Some(slug),
                None => true,
            })
            .count();
        if affected > 0 {
            println!();
            println!(
                "{affected} runner(s) of the signed-out workspace(s) are still configured on this host."
            );
            println!(
                "Run `pidash auth login{}` before reconnecting them, or `pidash runner remove <name>` to remove one.",
                workspace
                    .map(|s| format!(" --workspace {s}"))
                    .unwrap_or_default()
            );
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::schema::{
        CliSection, Config, DaemonConfig, EngineCredential, EngineSignInState,
    };

    fn paths(root: &std::path::Path) -> Paths {
        Paths {
            config_dir: root.join("config"),
            data_dir: root.join("data"),
            runtime_dir: root.join("runtime"),
            machine_context_dir: root.join("config"),
        }
    }

    #[tokio::test]
    async fn full_logout_with_no_workspace_left_still_signs_the_engine_out() {
        let tmp = tempfile::tempdir().unwrap();
        let p = paths(tmp.path());
        let cfg = Config {
            version: 2,
            daemon: DaemonConfig {
                cloud_url: "https://cloud.invalid".into(),
                ..DaemonConfig::default()
            },
            runners: vec![],
            cli: Some(CliSection {
                engine_credential: Some(EngineCredential {
                    token: "ec_left_behind".into(),
                    expires_at: Some(chrono::Utc::now() + chrono::Duration::days(10)),
                }),
                engine_sign_in_state: Some(EngineSignInState::Active),
                ..Default::default()
            }),
            engine: Default::default(),
            auth: Default::default(),
        };
        file::write_config(&p, &cfg).unwrap();

        run(
            Args {
                local_only: false,
                workspace: None,
                engine: false,
            },
            &p,
        )
        .await
        .unwrap();

        let after = file::load_config(&p).unwrap();
        let cli = after.cli.unwrap();
        assert!(cli.engine_credential.is_none(), "ec_ forgotten");
        assert_eq!(cli.engine_sign_in_state, Some(EngineSignInState::SignedOut));
        assert!(
            !std::fs::read_to_string(p.config_path())
                .unwrap()
                .contains("ec_left_behind")
        );
    }

    fn two_workspaces_and_engine(url: &str) -> Config {
        Config {
            version: 2,
            daemon: DaemonConfig {
                cloud_url: url.into(),
                ..DaemonConfig::default()
            },
            runners: vec![],
            cli: Some(CliSection {
                token: Some("mt_acme".into()),
                workspace_slug: Some("acme".into()),
                engine_credential: Some(EngineCredential {
                    token: "ec_live".into(),
                    expires_at: Some(chrono::Utc::now() + chrono::Duration::days(10)),
                }),
                engine_sign_in_state: Some(EngineSignInState::Active),
                ..Default::default()
            }),
            engine: Default::default(),
            auth: Default::default(),
        }
    }

    #[tokio::test]
    async fn signing_out_the_last_workspace_signs_the_engine_out_first() {
        use crate::engine::test_server;
        let server = test_server::start(std::sync::Arc::new(|_| (200, "{}".into()))).await;
        let tmp = tempfile::tempdir().unwrap();
        let p = paths(tmp.path());
        file::write_config(&p, &two_workspaces_and_engine(&server.url)).unwrap();

        run(
            Args {
                local_only: false,
                workspace: Some("acme".into()),
                engine: false,
            },
            &p,
        )
        .await
        .unwrap();

        let reqs = server.requests.lock().unwrap().clone();
        let engine_revoke = reqs
            .iter()
            .position(|r| r.path == crate::engine::api::CREDENTIAL_REVOKE_PATH)
            .expect("the engine credential was revoked");
        let token_revoke = reqs
            .iter()
            .position(|r| r.path == "/api/v1/auth/revoke/")
            .expect("the machine token was revoked");
        assert!(
            engine_revoke < token_revoke,
            "engine first, while mt_ works"
        );
        assert_eq!(reqs[engine_revoke].bearer(), Some("mt_acme"));
        let cli = file::load_config(&p).unwrap().cli.unwrap();
        assert!(cli.engine_credential.is_none());
        assert_eq!(cli.engine_sign_in_state, Some(EngineSignInState::SignedOut));
    }

    #[tokio::test]
    async fn signing_out_one_of_two_workspaces_leaves_the_engine_signed_in() {
        use crate::engine::test_server;
        let server = test_server::start(std::sync::Arc::new(|_| (200, "{}".into()))).await;
        let tmp = tempfile::tempdir().unwrap();
        let p = paths(tmp.path());
        let mut cfg = two_workspaces_and_engine(&server.url);
        cfg.cli
            .as_mut()
            .unwrap()
            .workspaces
            .push(crate::config::schema::WorkspaceCredential {
                slug: "beta".into(),
                token: "mt_beta".into(),
            });
        file::write_config(&p, &cfg).unwrap();

        run(
            Args {
                local_only: false,
                workspace: Some("beta".into()),
                engine: false,
            },
            &p,
        )
        .await
        .unwrap();

        assert!(
            server
                .requests_to(crate::engine::api::CREDENTIAL_REVOKE_PATH)
                .is_empty()
        );
        let cli = file::load_config(&p).unwrap().cli.unwrap();
        assert_eq!(cli.engine_credential.unwrap().token, "ec_live");
    }

    #[tokio::test]
    async fn logout_gets_past_a_keychain_item_that_can_never_be_read() {
        use crate::config::schema::CredentialsStoreMode;
        use crate::config::secrets::{self, ITEM_ENGINE_CREDENTIAL, MemoryKeyring};
        let tmp = tempfile::tempdir().unwrap();
        let p = paths(tmp.path());
        let keyring = MemoryKeyring::new();
        secrets::use_backend_for(&p.config_dir, keyring.clone());
        let mut cfg = two_workspaces_and_engine("https://cloud.invalid");
        cfg.auth.credentials_store = CredentialsStoreMode::Auto;
        file::write_config(&p, &cfg).unwrap();
        file::migrate_credentials_store(&p).unwrap();
        keyring.break_item(ITEM_ENGINE_CREDENTIAL);
        // Ordinary writes are refused and name the way out.
        let err = file::mutate_config(&p, |_| Ok(())).unwrap_err();
        assert!(format!("{err:#}").contains("pidash auth logout"), "{err:#}");

        run(
            Args {
                local_only: true,
                workspace: None,
                engine: false,
            },
            &p,
        )
        .await
        .unwrap();

        let after = file::load_config(&p).unwrap();
        assert!(after.auth.unread_keyring_items.is_empty());
        let cli = after.cli.unwrap_or_default();
        assert!(cli.workspace_tokens().is_empty());
        assert!(cli.engine_credential.is_none());
        assert!(keyring.items().is_empty(), "{:?}", keyring.items());
        // Writes work again.
        file::mutate_config(&p, |_| Ok(())).unwrap();
    }

    #[tokio::test]
    async fn a_broken_machine_token_item_is_revoked_before_it_is_forgotten() {
        use crate::config::schema::CredentialsStoreMode;
        use crate::config::secrets::{self, ITEM_WORKSPACE_PREFIX, MemoryKeyring};
        use crate::engine::test_server;
        for workspace in [Some("beta"), None] {
            let server = test_server::start(std::sync::Arc::new(|_| (200, "{}".into()))).await;
            let tmp = tempfile::tempdir().unwrap();
            let p = paths(tmp.path());
            let keyring = MemoryKeyring::new();
            secrets::use_backend_for(&p.config_dir, keyring.clone());
            let mut cfg = two_workspaces_and_engine(&server.url);
            cfg.cli
                .as_mut()
                .unwrap()
                .workspaces
                .push(crate::config::schema::WorkspaceCredential {
                    slug: "beta".into(),
                    token: "mt_beta".into(),
                });
            cfg.auth.credentials_store = CredentialsStoreMode::Auto;
            file::write_config(&p, &cfg).unwrap();
            file::migrate_credentials_store(&p).unwrap();
            let beta = format!("{ITEM_WORKSPACE_PREFIX}beta");
            keyring.break_item(&beta);

            run(
                Args {
                    local_only: false,
                    workspace: workspace.map(str::to_string),
                    engine: false,
                },
                &p,
            )
            .await
            .unwrap();

            let revoked: Vec<_> = server
                .requests_to("/api/v1/auth/revoke/")
                .iter()
                .filter_map(|r| r.headers.get("x-api-key").cloned())
                .collect();
            assert!(
                revoked.iter().any(|t| t == "mt_beta"),
                "{workspace:?}: beta's token was not revoked: {revoked:?}"
            );
            assert!(!keyring.items().contains_key(&beta), "{workspace:?}");
            let after = file::load_config(&p).unwrap();
            assert!(after.auth.broken_keyring_items.is_empty(), "{workspace:?}");
            if workspace.is_some() {
                // acme and the engine are untouched.
                let cli = after.cli.unwrap();
                assert_eq!(cli.token.as_deref(), Some("mt_acme"));
                assert_eq!(cli.engine_credential.unwrap().token, "ec_live");
                assert!(!revoked.iter().any(|t| t == "mt_acme"));
            }
        }
    }

    #[tokio::test]
    async fn an_engine_sign_out_keeps_a_workspaces_broken_item() {
        use crate::config::schema::CredentialsStoreMode;
        use crate::config::secrets::{self, ITEM_WORKSPACE_PREFIX, MemoryKeyring};
        let tmp = tempfile::tempdir().unwrap();
        let p = paths(tmp.path());
        let keyring = MemoryKeyring::new();
        secrets::use_backend_for(&p.config_dir, keyring.clone());
        let mut cfg = two_workspaces_and_engine("https://cloud.invalid");
        cfg.cli
            .as_mut()
            .unwrap()
            .workspaces
            .push(crate::config::schema::WorkspaceCredential {
                slug: "beta".into(),
                token: "mt_beta".into(),
            });
        cfg.auth.credentials_store = CredentialsStoreMode::Auto;
        file::write_config(&p, &cfg).unwrap();
        file::migrate_credentials_store(&p).unwrap();
        let beta = format!("{ITEM_WORKSPACE_PREFIX}beta");
        keyring.break_item(&beta);

        run(
            Args {
                local_only: true,
                workspace: None,
                engine: true,
            },
            &p,
        )
        .await
        .unwrap();

        assert_eq!(
            keyring.items().get(&beta).map(String::as_str),
            Some("mt_beta")
        );
        let after = file::load_config(&p).unwrap();
        assert_eq!(after.auth.broken_keyring_items, vec![beta]);
        assert!(after.cli.unwrap().engine_credential.is_none());
    }
}
