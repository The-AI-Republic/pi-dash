//! `pidash auth status` — show login state.

use anyhow::{Context, Result};
use clap::Args as ClapArgs;
use serde::Deserialize;
use std::time::Duration;

use crate::cli::auth::session;
use crate::cli::runner_ops;
use crate::config::file;
use crate::util::paths::Paths;

#[derive(Debug, ClapArgs)]
pub struct Args {
    /// Print machine-readable sign-in state (for the desktop app) instead of
    /// checking the token against the cloud. Works signed out and with no
    /// config at all.
    #[arg(long)]
    pub json: bool,
}

#[derive(Debug, Deserialize)]
pub(crate) struct UserMe {
    #[serde(default)]
    pub(crate) id: Option<String>,
    #[serde(default)]
    pub(crate) email: Option<String>,
    #[serde(default)]
    first_name: Option<String>,
    #[serde(default)]
    last_name: Option<String>,
}

pub async fn run(args: Args, paths: &Paths) -> Result<()> {
    if args.json {
        match status_json(paths).await {
            Ok(v) => {
                println!("{v}");
                return Ok(());
            }
            Err(e) => {
                println!(
                    "{}",
                    serde_json::json!({ "error": "status_failed", "message": format!("{e:#}") })
                );
                std::process::exit(1);
            }
        }
    }
    let token = runner_ops::load_cli_token(paths)?.ok_or_else(|| {
        anyhow::anyhow!("not logged in — run `pidash auth login` to authenticate this host")
    })?;

    let cfg = file::load_config(paths).ok();
    let cloud_url = cfg
        .as_ref()
        .map(|c| c.daemon.cloud_url.clone())
        .unwrap_or_default();
    if cloud_url.is_empty() {
        anyhow::bail!("config.toml is missing [daemon].cloud_url — re-run `pidash auth login`");
    }

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()?;
    let url = format!("{cloud_url}/api/v1/users/me/");
    let resp = client
        .get(&url)
        .header("X-Api-Key", &token)
        .send()
        .await
        .with_context(|| format!("GET {url}"))?;
    let status = resp.status();
    if status.as_u16() == 401 || status.as_u16() == 403 {
        anyhow::bail!(
            "logged in to {cloud_url} but the cloud rejected the token (HTTP {status}) — run `pidash auth login` to re-authenticate"
        );
    }
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        anyhow::bail!("/api/v1/users/me/ returned HTTP {status}: {body}");
    }
    let me: UserMe = resp.json().await.context("parsing /users/me/")?;
    remember_user(paths, &me);

    println!("Logged in to {cloud_url}");
    if let Some(email) = me.email.as_deref() {
        let name = format!(
            "{} {}",
            me.first_name.as_deref().unwrap_or(""),
            me.last_name.as_deref().unwrap_or("")
        )
        .trim()
        .to_string();
        if name.is_empty() {
            println!("  ✓ Account: {email}");
        } else {
            println!("  ✓ Account: {email} ({name})");
        }
    } else {
        println!("  ✓ Account: <unknown>");
    }

    if let Some(c) = &cfg {
        let runner_count = c.runners.len();
        if runner_count == 0 {
            println!("  - No runners registered on this host.");
            println!("    Run `pidash runner add` to register one.");
        } else {
            println!("  ✓ Runners on this host: {runner_count}");
            for r in &c.runners {
                let project = r.project_slug.as_deref().unwrap_or("?");
                println!("      - {} (project {project})", r.name);
            }
        }
    }
    Ok(())
}

/// `auth status --json`: the sign-in state the desktop app syncs against.
/// Local only, except for a one-off lookup of the user id on configs signed
/// in before user ids were recorded.
async fn status_json(paths: &Paths) -> Result<serde_json::Value> {
    let mut cfg = file::load_config_opt(paths)?;
    let signed_in = cfg
        .as_ref()
        .and_then(|c| c.cli.as_ref())
        .and_then(|cli| cli.token.as_deref())
        .is_some_and(|t| !t.is_empty());
    let missing_user = cfg
        .as_ref()
        .and_then(|c| c.cli.as_ref())
        .is_none_or(|cli| cli.user_id.is_none());
    if signed_in && missing_user && backfill_user(paths).await {
        cfg = file::load_config_opt(paths)?;
    }
    let cli = cfg.as_ref().and_then(|c| c.cli.clone()).unwrap_or_default();
    let cloud_url = cfg
        .as_ref()
        .map(|c| c.daemon.cloud_url.clone())
        .filter(|u| !u.is_empty());
    let user = match (signed_in, cli.user_id.as_deref()) {
        (true, Some(id)) => serde_json::json!({
            "id": id,
            "email": cli.user_email.clone().unwrap_or_default(),
        }),
        _ => serde_json::Value::Null,
    };
    // Signed in, but who as is not known (legacy config, lookup failed):
    // lets the desktop app tell this apart from signed out (`user` is null
    // in both cases).
    let user_unknown = signed_in && cli.user_id.is_none();
    let activity = session::daemon_activity(paths).await;
    Ok(serde_json::json!({
        "signed_in": signed_in,
        "cloud_url": cloud_url,
        "user": user,
        "user_unknown": user_unknown,
        "workspace_slug": cli.workspace_slug,
        "dev_machine_id": cfg.as_ref().and_then(|c| c.daemon.dev_machine_id).map(|id| id.to_string()),
        "changed_at": cli.changed_at().map(|t| t.to_rfc3339_opts(chrono::SecondsFormat::Secs, true)),
        "daemon_running": activity.running,
        "active_runs": activity.active_runs,
    }))
}

/// Look the signed-in user up once and record it (configs written before
/// `[cli].user_id` existed). Best-effort with a short timeout; `true` when
/// something was recorded.
pub(crate) async fn backfill_user(paths: &Paths) -> bool {
    let Ok(Some(cfg)) = file::load_config_opt(paths) else {
        return false;
    };
    let Some(token) = cfg.cli.as_ref().and_then(|c| c.token.clone()) else {
        return false;
    };
    let Some(me) = whoami(&cfg.daemon.cloud_url, &token).await else {
        return false;
    };
    remember_user(paths, &me)
}

/// `GET /api/v1/users/me/` with `token` (a machine token or a device-flow
/// API token). Best-effort with a short timeout: `None` on any failure.
pub(crate) async fn whoami(cloud_url: &str, token: &str) -> Option<UserMe> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .ok()?;
    let url = format!("{}/api/v1/users/me/", cloud_url.trim_end_matches('/'));
    let resp = client
        .get(&url)
        .header("X-Api-Key", token)
        .send()
        .await
        .ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let me = resp.json::<UserMe>().await.ok()?;
    me.id
        .as_deref()
        .is_some_and(|id| !id.is_empty())
        .then_some(me)
}

/// Record the user `/users/me/` named, unless one is already recorded.
fn remember_user(paths: &Paths, me: &UserMe) -> bool {
    let Some(id) = me.id.clone().filter(|id| !id.is_empty()) else {
        return false;
    };
    let already_known = file::load_config_opt(paths)
        .ok()
        .flatten()
        .and_then(|c| c.cli)
        .is_some_and(|cli| cli.user_id.is_some());
    if already_known {
        return false;
    }
    let mut changed = false;
    let _ = file::mutate_config(paths, |cfg| {
        let mut cli = cfg.cli.take().unwrap_or_default();
        if cli.user_id.is_none() && cli.token.is_some() {
            cli.user_id = Some(id);
            cli.user_email = me.email.clone();
            changed = true;
        }
        cfg.cli = Some(cli);
        Ok(())
    });
    changed
}
