//! Account switch: signing in as another account parks the previous
//! account's machine identity and restores the new account's own
//! (desktop_unified_signin §6.4).
//!
//! A machine identity belongs to one account on one cloud: the server
//! refuses a machine id owned by another user (`dev_machine_not_found`).
//! So signing in as another account first signs the old account out
//! (`pidash auth logout`), then moves its identity — `[daemon].dev_machine_id`
//! and its `[[runner]]` blocks, their per-runner data directories staying
//! where they are — to `<config_dir>/parked/<cloud_host>/<account>.toml`
//! (0600). The new account's parked identity is restored when one exists,
//! otherwise the machine id is cleared so the sign-in mints a new one.
//! Nothing is deleted. The running daemon cannot change identity in
//! process, so the CLI restarts it (`restart_and_verify`).

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::io::IsTerminal;
use std::path::{Path, PathBuf};

use crate::cli::runner_ops;
use crate::config::file;
use crate::config::schema::{Config, RunnerConfig};
use crate::util::paths::Paths;
use uuid::Uuid;

/// The machine identity of one account on one cloud: what the server binds
/// runner rows to. Parked to disk only — no credentials.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct ParkedIdentity {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    dev_machine_id: Option<Uuid>,
    #[serde(default, rename = "runner")]
    runners: Vec<RunnerConfig>,
}

/// The account a switch would move from or to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SwitchAccount {
    pub user_id: Option<String>,
    pub email: Option<String>,
}

/// The plan for one `pidash auth login`: nothing to do, or a switch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SwitchPlan {
    /// Same account (or nothing signed in yet): no parking.
    None,
    /// This computer's stored account is `previous`; the sign-in is as
    /// `incoming`. `signed_in` says whether `previous` holds live
    /// credentials (then it is signed out first; a signed-out computer just
    /// parks its identity). Either way the machine id and runner blocks
    /// move with the accounts: signing in as another account with a kept
    /// machine id fails (`dev_machine_not_found`), signed out or not.
    Switch {
        previous: SwitchAccount,
        incoming: SwitchAccount,
        signed_in: bool,
    },
}

/// The stored account, when this computer ever signed in.
fn stored_account(cfg: &Config) -> Option<SwitchAccount> {
    let cli = cfg.cli.as_ref()?;
    let email = cli.account_email.clone().filter(|s| !s.trim().is_empty());
    let user_id = cli.account_user_id.clone().filter(|s| !s.trim().is_empty());
    if email.is_none() && user_id.is_none() {
        return None;
    }
    Some(SwitchAccount { user_id, email })
}

/// Whether the stored account holds live credentials right now.
fn is_signed_in(cfg: &Config) -> bool {
    cfg.cli
        .as_ref()
        .is_some_and(|cli| !cli.workspace_tokens().is_empty())
}

/// Decide the plan from what is stored and what is signing in.
pub fn plan_switch(
    cfg: &Config,
    incoming_email: Option<&str>,
    incoming_user_id: Option<&str>,
) -> SwitchPlan {
    let Some(previous) = stored_account(cfg) else {
        return SwitchPlan::None;
    };
    let stored_email = previous.email.clone();
    let stored_id = previous.user_id.clone();
    let same =
        runner_ops::same_account(incoming_email, incoming_user_id, &stored_email, &stored_id);
    if same {
        return SwitchPlan::None;
    }
    SwitchPlan::Switch {
        signed_in: is_signed_in(cfg),
        previous,
        incoming: SwitchAccount {
            user_id: incoming_user_id.map(str::to_string),
            email: incoming_email.map(str::to_string),
        },
    }
}

/// The host of the cloud URL, as a path component (`pidash.example.com`).
fn cloud_host(cloud_url: &str) -> String {
    let trimmed = cloud_url.trim().trim_end_matches('/');
    let rest = trimmed.split("://").nth(1).unwrap_or(trimmed);
    rest.split('/').next().unwrap_or(rest).to_string()
}

/// The parking directory for one cloud: `<config_dir>/parked/<cloud_host>/`.
fn parked_dir(paths: &Paths, cloud_url: &str) -> PathBuf {
    paths.config_dir.join("parked").join(cloud_host(cloud_url))
}

/// The account key for one parked file: the user id (stable per account),
/// else the e-mail. Identifiers are sanitised to path components.
fn account_key(account: &SwitchAccount) -> Option<String> {
    let raw = account
        .user_id
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .or_else(|| account.email.as_deref().filter(|s| !s.trim().is_empty()))?;
    let safe: String = raw
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '@') {
                c
            } else {
                '_'
            }
        })
        .collect();
    let safe = safe.trim_matches('.').to_string();
    (!safe.is_empty()).then_some(safe)
}

/// `<config_dir>/parked/<cloud_host>/<account>.toml` for `account`.
fn parked_path(paths: &Paths, cloud_url: &str, account: &SwitchAccount) -> Option<PathBuf> {
    account_key(account).map(|key| parked_dir(paths, cloud_url).join(format!("{key}.toml")))
}

/// Write `config`'s machine identity (machine id + runner blocks) to
/// `path` with 0600. Runner data directories are untouched.
fn write_parked(path: &Path, cfg: &Config) -> Result<()> {
    let identity = ParkedIdentity {
        dev_machine_id: cfg.daemon.dev_machine_id,
        runners: cfg.runners.clone(),
    };
    let text = toml::to_string(&identity).context("serializing the parked machine identity")?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating {}", parent.display()))?;
    }
    std::fs::write(path, text).with_context(|| format!("writing {}", path.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
            .with_context(|| format!("chmod 0600 {}", path.display()))?;
    }
    Ok(())
}

/// Is there a parked identity for `account` on this cloud?
pub fn parked_exists(paths: &Paths, cloud_url: &str, account: &SwitchAccount) -> bool {
    parked_path(paths, cloud_url, account).is_some_and(|p| p.exists())
}

/// Park the current machine identity for `account` (call after the account
/// has been signed out, so no credentials are involved). Nothing is deleted;
/// the live config keeps its `[[runner]]` blocks until the sign-in as the
/// next account restores or replaces them.
pub fn park_identity(paths: &Paths, cloud_url: &str, account: &SwitchAccount) -> Result<()> {
    let Some(path) = parked_path(paths, cloud_url, account) else {
        return Ok(());
    };
    let cfg = file::load_config(paths)?;
    write_parked(&path, &cfg)
}

/// Restore `account`'s parked identity into the config (machine id and
/// runner blocks), or clear the machine id so the sign-in mints a new one
/// when there is nothing parked for it.
pub fn restore_or_clear_identity(
    paths: &Paths,
    cloud_url: &str,
    account: &SwitchAccount,
) -> Result<()> {
    let Some(path) = parked_path(paths, cloud_url, account) else {
        return Ok(());
    };
    if !path.exists() {
        file::mutate_config(paths, |cfg| {
            cfg.daemon.dev_machine_id = None;
            Ok(())
        })?;
        return Ok(());
    }
    let text =
        std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
    let identity: ParkedIdentity =
        toml::from_str(&text).with_context(|| format!("parsing {}", path.display()))?;
    file::mutate_config(paths, |cfg| {
        cfg.daemon.dev_machine_id = identity.dev_machine_id;
        cfg.runners = identity.runners;
        Ok(())
    })?;
    Ok(())
}

/// Runs currently in flight on this computer's runners, best-effort: the
/// running daemon's recent runs that are not queued and not terminal. A
/// daemon that does not answer means no runs can be in flight.
pub async fn count_active_runs(paths: &Paths) -> usize {
    const IN_FLIGHT: &[&str] = &[
        "assigned",
        "running",
        "awaiting_approval",
        "awaiting_reauth",
        "paused_awaiting_input",
        "cancel_requested",
        "waiting_for_worktree",
    ];
    let Ok(mut client) = pidash_ipc::Client::connect(paths.ipc_socket_path()).await else {
        return 0;
    };
    let Ok(pidash_ipc::protocol::Response::Runs(runs)) = client
        .call(pidash_ipc::protocol::Request::RunsList {
            limit: Some(200),
            runner: None,
        })
        .await
    else {
        return 0;
    };
    runs.iter()
        .filter(|r| IN_FLIGHT.contains(&r.status.as_str()))
        .count()
}

/// Ask, interactively, whether to stop `active` in-flight tasks for
/// `previous_email` and sign in as `incoming_email` instead. Non-interactive
/// sign-ins (the app's) never prompt: the app warns on its own sign-in
/// screen. Returns false when the user declines (the login aborts).
pub fn confirm_switch(
    previous_email: Option<&str>,
    incoming_email: Option<&str>,
    active: usize,
) -> bool {
    if !std::io::stdin().is_terminal() {
        return true;
    }
    let previous = previous_email.unwrap_or("another account");
    let incoming = incoming_email.unwrap_or("this account");
    if active > 0 {
        println!();
        println!("Pi Dash on this computer is running {active} task(s) for {previous};");
        println!(
            "signing in as {incoming} stops them (they end as failed and can be retried by hand)."
        );
    }
    print!("Sign in as {incoming} and switch this computer to that account? [y/N] ");
    let _ = std::io::Write::flush(&mut std::io::stdout());
    let mut answer = String::new();
    if std::io::stdin().read_line(&mut answer).is_err() {
        return false;
    }
    matches!(answer.trim().to_ascii_lowercase().as_str(), "y" | "yes")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg_with_account(email: &str, id: &str, signed_in: bool) -> Config {
        let mut cfg = Config {
            version: 2,
            daemon: Default::default(),
            runners: vec![],
            cli: Some(crate::config::schema::CliSection::default()),
            engine: Default::default(),
            auth: Default::default(),
            auth_event: None,
        };
        cfg.cli.as_mut().unwrap().account_email = Some(email.into());
        cfg.cli.as_mut().unwrap().account_user_id = Some(id.into());
        if signed_in {
            cfg.cli
                .as_mut()
                .unwrap()
                .set_workspace_token("acme", "mt_x", true);
        }
        cfg
    }

    #[test]
    fn no_switch_when_same_account_or_nothing_signed_in() {
        let signed = cfg_with_account("a@example.com", "u-a", true);
        assert_eq!(
            plan_switch(&signed, Some("a@example.com"), Some("u-a")),
            SwitchPlan::None,
            "same e-mail and user id is not a switch"
        );
        assert_eq!(
            plan_switch(&signed, Some("A@EXAMPLE.COM"), None),
            SwitchPlan::None,
            "e-mail compares case-insensitively when no user id is known"
        );
        assert_eq!(
            plan_switch(&signed, Some("b@example.com"), Some("u-b")),
            SwitchPlan::Switch {
                previous: SwitchAccount {
                    user_id: Some("u-a".into()),
                    email: Some("a@example.com".into())
                },
                incoming: SwitchAccount {
                    user_id: Some("u-b".into()),
                    email: Some("b@example.com".into())
                },
                signed_in: true,
            },
            "a different user id is a switch even on the same cloud"
        );
        let signed_out = cfg_with_account("a@example.com", "u-a", false);
        assert_eq!(
            plan_switch(&signed_out, Some("b@example.com"), Some("u-b")),
            SwitchPlan::Switch {
                previous: SwitchAccount {
                    user_id: Some("u-a".into()),
                    email: Some("a@example.com".into())
                },
                incoming: SwitchAccount {
                    user_id: Some("u-b".into()),
                    email: Some("b@example.com".into())
                },
                signed_in: false,
            },
            "a signed-out computer still parks its identity: its machine id would fail for the new account"
        );
    }

    #[test]
    fn cloud_host_and_account_key_are_path_safe() {
        assert_eq!(
            cloud_host("https://pidash.example.com/"),
            "pidash.example.com"
        );
        assert_eq!(cloud_host("http://localhost:3010/"), "localhost:3010");
        let account = SwitchAccount {
            user_id: None,
            email: Some("a/b@example.com".into()),
        };
        assert_eq!(account_key(&account).as_deref(), Some("a_b@example.com"));
        assert_eq!(
            account_key(&SwitchAccount {
                user_id: None,
                email: None
            }),
            None
        );
    }

    #[test]
    fn park_and_restore_move_identity_without_credentials() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = Paths {
            config_dir: tmp.path().join("config"),
            data_dir: tmp.path().join("data"),
            runtime_dir: tmp.path().join("runtime"),
            machine_context_dir: tmp.path().join("config"),
        };
        std::fs::create_dir_all(&paths.config_dir).unwrap();
        let machine_id = Uuid::new_v4();
        // Written as TOML so the test does not construct the (large)
        // RunnerConfig struct in code.
        let text = format!(
            "version = 2\n\n[daemon]\ncloud_url = \"https://pidash.example.com\"\ndev_machine_id = \"{machine_id}\"\n\n[[runner]]\nname = \"a\"\nrunner_id = \"{}\"\nworkspace_slug = \"acme\"\nproject_slug = \"DEF\"\n\n[runner.workspace]\nworking_dir = \"/tmp/wd\"\n",
            Uuid::new_v4()
        );
        std::fs::write(paths.config_path(), text).unwrap();
        assert_eq!(file::load_config(&paths).unwrap().runners.len(), 1);

        let account = SwitchAccount {
            user_id: Some("u-a".into()),
            email: Some("a@example.com".into()),
        };
        assert!(!parked_exists(
            &paths,
            "https://pidash.example.com",
            &account
        ));
        park_identity(&paths, "https://pidash.example.com", &account).unwrap();
        assert!(parked_exists(
            &paths,
            "https://pidash.example.com",
            &account
        ));
        let parked_text = std::fs::read_to_string(
            parked_path(&paths, "https://pidash.example.com", &account).unwrap(),
        )
        .unwrap();
        assert!(
            !parked_text.contains("mt_"),
            "the parked file holds no credentials"
        );

        // Another account restores nothing yet: its identity is cleared.
        let next = SwitchAccount {
            user_id: Some("u-b".into()),
            email: Some("b@example.com".into()),
        };
        restore_or_clear_identity(&paths, "https://pidash.example.com", &next).unwrap();
        let cleared = file::load_config(&paths).unwrap();
        assert_eq!(
            cleared.daemon.dev_machine_id, None,
            "no parked identity: mint fresh"
        );

        // The first account gets its machine id back, and only it.
        restore_or_clear_identity(&paths, "https://pidash.example.com", &account).unwrap();
        let restored = file::load_config(&paths).unwrap();
        assert_eq!(restored.daemon.dev_machine_id, Some(machine_id));
        assert_eq!(restored.runners.len(), 1);
    }
}
