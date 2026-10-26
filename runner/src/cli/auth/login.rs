//! `pidash auth login` — RFC 8628 device-code flow client.
//!
//! Walks the user through the standard device-code dance:
//!
//! 1. `POST /api/v1/auth/device/start/` — get `user_code` + `device_code`.
//! 2. Show `user_code` + verification URI; try to open the URL in a
//!    browser as a convenience.
//! 3. Poll `POST /api/v1/auth/device/token/` at the cloud-specified
//!    interval until the user approves in the browser, hits a terminal
//!    error, or the grant expires.
//! 4. Exchange the returned `APIToken` for the shared dev-machine
//!    `MachineToken` (`POST /api/v1/auth/machine-token/`, with this
//!    computer's fingerprint) and write it, the signed-in user and the
//!    surviving `dev_machine_id` to `config.toml`. Signing in as a different
//!    user than the stored one parks the previous user's runners first (see
//!    [`super::session`]).
//!
//! The cloud to talk to comes from `--url`, else this host's existing
//! `[daemon].cloud_url`, else [`crate::DEFAULT_CLOUD_URL`] — never from a
//! prompt, so the release installer can run straight into step 1.
//!
//! After a successful login we simply confirm the account/workspace and
//! point the user at the cloud URL. Registering a runner is a separate,
//! explicit step (`pidash runner add`).

use anyhow::{Context, Result};
use clap::Args as ClapArgs;
use serde::Deserialize;
use std::io::Write;
use std::time::Duration;

use crate::cli::auth::{logout, session};
use crate::cli::runner_ops;
use crate::config::file;
use crate::util::paths::Paths;

#[derive(Debug, ClapArgs)]
pub struct Args {
    /// Pi Dash cloud base URL (e.g. `https://pidash.example.com`).
    /// Only needed for a self-hosted instance. Omitted, we reuse this
    /// host's existing `[daemon].cloud_url` if it has one, and otherwise
    /// fall back to the hosted cloud (`DEFAULT_CLOUD_URL`).
    #[arg(long)]
    pub url: Option<String>,

    /// Don't try to open the verification URL in a browser.
    #[arg(long)]
    pub no_browser: bool,

    /// Re-enroll this host against a different cloud, overwriting the
    /// existing `[daemon].cloud_url`. Without this, login refuses to
    /// rebind a host that's already enrolled with another cloud.
    #[arg(long)]
    pub force: bool,

    /// Workspace slug to bind this CLI install to after login.
    #[arg(long, hide = true)]
    pub workspace: Option<String>,

    /// Internal: finish a device-code grant that has *already* been approved,
    /// rather than starting one and asking the user to type a code.
    ///
    /// The Pi Dash desktop app uses this. It is signed in already, so it can
    /// start the grant and approve it with its own session
    /// (`POST /api/v1/auth/device/approve/`), then hand the device code here.
    /// Everything after the exchange — the machine token, the workspace
    /// binding, the config file — stays here rather than being reimplemented
    /// in the app.
    ///
    /// The code is read from **stdin**, never taken as an argument: once
    /// approved it is a bearer credential (the token endpoint asks for nothing
    /// else), and argv is visible to other local processes.
    #[arg(long, hide = true)]
    pub device_code_stdin: bool,

    /// Don't ask before switching this host to a different account. An
    /// account switch parks the current user's runners (stopping any runs in
    /// progress) until that user signs in here again.
    #[arg(long)]
    pub yes: bool,
}

#[derive(Debug, Deserialize)]
struct StartResponse {
    device_code: String,
    user_code: String,
    verification_uri: String,
    expires_in: u64,
    interval: u64,
}

#[derive(Debug, Deserialize)]
struct TokenSuccess {
    access_token: String,
    #[serde(default)]
    user_email: Option<String>,
    // The server also returns `workspace_slug` here (the workspace the
    // approve step auto-picked), but the CLI deliberately ignores it
    // and re-resolves via `GET /api/v1/auth/workspaces/` so multi-
    // workspace users can pick rather than silently inheriting the
    // server's auto-pick.
}

#[derive(Debug, Deserialize)]
struct MachineTokenResponse {
    machine_token: String,
    workspace_slug: String,
    /// The dev machine the cloud resolved this host to. May differ from the
    /// id we sent (records merged by fingerprint); always persist it.
    #[serde(default)]
    dev_machine_id: Option<uuid::Uuid>,
    #[serde(default)]
    user: Option<MachineTokenUser>,
    /// `dhk_…`, only for an interactive (device-flow) sign-in.
    #[serde(default)]
    handoff_key: Option<String>,
}

#[derive(Debug, Deserialize)]
struct MachineTokenUser {
    id: String,
    #[serde(default)]
    email: Option<String>,
}

#[derive(Debug, Deserialize)]
struct TokenError {
    error: String,
}

pub async fn run(args: Args, paths: &Paths) -> Result<()> {
    let outcome = login_and_bind_workspace(&args, paths).await?;

    println!();
    println!("Start using Pi Dash at {}", outcome.cloud_url);
    Ok(())
}

pub async fn run_auth_only(args: Args, paths: &Paths) -> Result<()> {
    login_and_bind_workspace(&args, paths).await.map(|_| ())
}

struct LoginOutcome {
    cloud_url: String,
}

async fn login_and_bind_workspace(args: &Args, paths: &Paths) -> Result<LoginOutcome> {
    let (cloud_url, source) = resolve_cloud_url(args, paths)?;
    crate::cli::connect::validate_cloud_url(&cloud_url)?;

    // Only worth saying when we chose for them. If they passed `--url` or
    // this host is already enrolled, repeating the URL back is noise.
    if source == CloudUrlSource::Default {
        println!();
        println!("Connecting to Pi Dash at {cloud_url}");
        println!("(Self-hosted? Cancel and run `pidash auth login --url <YOUR-URL>` instead.)");
    }

    // Fail before the browser dance rather than after it.
    let previous = session::current_identity(paths)?;
    if let Some(existing) = previous.cloud_url.as_deref()
        && existing != cloud_url
        && !args.force
    {
        anyhow::bail!(
            "this host is already enrolled with cloud {existing} — refusing to overwrite [cli] for a different cloud {cloud_url} (pass --force to re-enroll against the new cloud)"
        );
    }
    // Signing in rewrites the machine token (the cloud rotates it, revoking
    // the old one, even for the same user), so a running daemon reloads and
    // stops its runs. Ask *before* anything is minted: declining afterwards
    // would leave this host with no valid token at all. The desktop app
    // (`--device-code-stdin`) has already warned the user; `--yes` is the
    // scripted opt-out.
    if !(args.yes || args.device_code_stdin) {
        let active_runs = session::daemon_activity(paths).await.active_runs;
        if needs_confirmation(active_runs, false) {
            let stored_who = previous
                .user_email
                .as_deref()
                .or(previous.user_id.as_deref());
            println!();
            for line in preflight_notice(active_runs, stored_who, previous.runner_count) {
                println!("{line}");
            }
            if !crate::util::confirm::ask("Sign in anyway?", false) {
                anyhow::bail!("login cancelled — nothing was changed");
            }
        }
    }
    let previous_dev_machine_id =
        file::load_config_opt(paths)?.and_then(|c| c.daemon.dev_machine_id);

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .context("building HTTP client")?;

    let device_code = if args.device_code_stdin {
        Some(read_device_code(std::io::stdin().lock())?)
    } else {
        None
    };
    let start = match device_code {
        // Pre-approved by the desktop app: nothing to show and nobody to ask,
        // just the exchange. `expires_in` bounds the polling; the server is
        // authoritative and ends the poll with a terminal error if the grant
        // is already gone.
        Some(code) => StartResponse {
            device_code: code,
            user_code: String::new(),
            verification_uri: format!("{cloud_url}/auth/device/"),
            expires_in: 300,
            interval: 1,
        },
        None => {
            let start = start_device_code(&client, &cloud_url).await?;

            print_user_code_block(&start);

            if !args.no_browser {
                let with_code = format!("{}?code={}", start.verification_uri, start.user_code);
                let _ = crate::util::browser::open_url(&with_code);
            }
            start
        }
    };

    let token = poll_for_token(&client, &cloud_url, &start).await?;

    println!();
    if let Some(email) = token.user_email.as_deref() {
        println!("✓ Logged in as {email}.");
    } else {
        println!("✓ Logged in.");
    }

    // v1: a CLI install is bound to one workspace. Resolve which one
    // (auto-pick if there's only one membership; prompt otherwise); it is
    // persisted with the rest of the sign-in below.
    let workspace_slug = resolve_workspace_binding(
        paths,
        &cloud_url,
        &token.access_token,
        args.workspace.as_deref(),
    )
    .await?;
    // Trade the short-lived device-flow APIToken for the shared dev-machine
    // MachineToken, which is the credential used by both the CLI and all
    // runners hosted by this install. The cloud resolves (and may merge) the
    // dev-machine record by fingerprint and tells us which id survived.
    let requested_machine_id = previous_dev_machine_id.unwrap_or_else(uuid::Uuid::new_v4);
    let host_label = crate::util::hostname::default_hostname();
    let fingerprint = crate::util::machine_fingerprint::machine_fingerprint();
    let machine_token = exchange_for_machine_token(
        &client,
        &cloud_url,
        &token.access_token,
        &workspace_slug,
        &requested_machine_id,
        &host_label,
        fingerprint.as_deref(),
    )
    .await?;
    let new_user_id = machine_token.user.as_ref().map(|u| u.id.clone());
    let new_user_email = machine_token
        .user
        .as_ref()
        .and_then(|u| u.email.clone())
        .or_else(|| token.user_email.clone());

    let switching = session::is_account_switch(previous.user_id.as_deref(), new_user_id.as_deref());
    if switching {
        // Best-effort: the old user's machine token (and its handoff keys)
        // must not outlive the switch. The cloud usually revoked it already
        // when it minted ours; an unreachable cloud = warn and go on.
        if let (Some(old_url), Some(old_token)) =
            (previous.cloud_url.as_deref(), previous.token.as_deref())
        {
            match logout::revoke_token(old_url, old_token).await {
                Ok(Ok(())) => {}
                Ok(Err(detail)) => tracing::warn!(
                    "could not revoke the previous account's machine token: {detail}"
                ),
                Err(e) => {
                    tracing::warn!("could not revoke the previous account's machine token: {e:#}")
                }
            }
        }
    }

    let switched_from_who = previous
        .user_email
        .clone()
        .or_else(|| previous.user_id.clone())
        .unwrap_or_else(|| "another account".to_string());
    let switched_to_who = new_user_email
        .clone()
        .or_else(|| new_user_id.clone())
        .unwrap_or_else(|| "this account".to_string());
    let outcome = session::apply_sign_in(
        paths,
        &session::SignIn {
            cloud_url: cloud_url.clone(),
            force: args.force,
            machine_token: machine_token.machine_token.clone(),
            workspace_slug: machine_token.workspace_slug.clone(),
            dev_machine_id: machine_token.dev_machine_id.unwrap_or(requested_machine_id),
            user_id: new_user_id,
            user_email: new_user_email,
            handoff_key: machine_token.handoff_key.clone(),
        },
    )
    .context("writing dev-machine token to config.toml")?;
    println!("  Workspace: {}", machine_token.workspace_slug);
    if switching {
        println!(
            "  Switched this machine from {switched_from_who} to {switched_to_who}; {} runner(s) parked.",
            outcome.parked
        );
    }
    if outcome.restored > 0 {
        println!("  Restored {} runner(s).", outcome.restored);
    }

    // Best-effort: write the machine-level ~/.pidash/context.md now that the
    // dev-machine token and workspace binding are persisted. A failure here is
    // a note, not a login failure — the file can be refreshed later with
    // `pidash context refresh`.
    match crate::cli::context::write_machine_context_for_paths(paths).await {
        Ok(path) => println!("  Machine context: {}", path.display()),
        Err(err) => println!("  (Machine context was not written: {err})"),
    }

    Ok(LoginOutcome { cloud_url })
}

/// Whether an interactive sign-in must ask before starting: only when the
/// daemon has runs in progress that the credential change will stop.
fn needs_confirmation(active_runs: usize, skip_prompt: bool) -> bool {
    !skip_prompt && active_runs > 0
}

/// The warning shown before an interactive sign-in that will stop runs.
fn preflight_notice(
    active_runs: usize,
    stored_who: Option<&str>,
    stored_runner_count: usize,
) -> Vec<String> {
    let mut lines = vec![format!(
        "{active_runs} run(s) in progress will be stopped when you sign in."
    )];
    if let Some(who) = stored_who {
        lines.push(format!(
            "This host is signed in as {who}. Signing in as a different account parks {who}'s {stored_runner_count} runner(s) until {who} signs in here again."
        ));
    }
    lines
}

/// Where the cloud URL we're about to authenticate against came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CloudUrlSource {
    /// Explicit `--url` on this invocation.
    Flag,
    /// `[daemon].cloud_url` persisted by an earlier login or enrollment.
    Config,
    /// Nothing configured — the hosted cloud.
    Default,
}

fn resolve_cloud_url(args: &Args, paths: &Paths) -> Result<(String, CloudUrlSource)> {
    if let Some(u) = &args.url {
        return Ok((u.trim_end_matches('/').to_string(), CloudUrlSource::Flag));
    }
    if paths.config_path().exists() {
        let cfg = file::load_config(paths)?;
        if !cfg.daemon.cloud_url.is_empty() {
            return Ok((
                cfg.daemon.cloud_url.trim_end_matches('/').to_string(),
                CloudUrlSource::Config,
            ));
        }
    }
    // Fresh host, no `--url`: this is the released install one-liner's path
    // into `auth login`, and the overwhelmingly common answer is our hosted
    // cloud. Asking for it here made every hosted user type our own URL by
    // hand — and gave headless installs nothing but an error. Default, and
    // let self-hosters redirect with `--url`.
    Ok((
        crate::DEFAULT_CLOUD_URL.trim_end_matches('/').to_string(),
        CloudUrlSource::Default,
    ))
}

/// Read the pre-approved device code the desktop app pipes in for
/// `--device-code-stdin`.
fn read_device_code(mut input: impl std::io::Read) -> Result<String> {
    let mut buf = String::new();
    input
        .read_to_string(&mut buf)
        .context("reading device code from stdin")?;
    let code = buf.trim();
    if code.is_empty() {
        anyhow::bail!("--device-code-stdin was given but no device code arrived on stdin");
    }
    Ok(code.to_string())
}

async fn start_device_code(client: &reqwest::Client, cloud_url: &str) -> Result<StartResponse> {
    let url = format!("{cloud_url}/api/v1/auth/device/start/");
    let resp = client
        .post(&url)
        .json(&serde_json::json!({}))
        .send()
        .await
        .with_context(|| format!("POST {url}"))?;
    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        anyhow::bail!("device-code start failed: HTTP {status}: {body}");
    }
    resp.json::<StartResponse>()
        .await
        .context("parsing device-code start response")
}

fn print_user_code_block(start: &StartResponse) {
    let minutes = start.expires_in / 60;
    println!();
    println!("First, copy your one-time code:");
    println!();
    println!("    {}", start.user_code);
    println!();
    println!("Then open this URL in your browser and approve the login:");
    println!();
    println!("    {}", start.verification_uri);
    println!();
    println!("(Code expires in {minutes} minutes.)");
    println!();
    print!("Waiting for browser approval...");
    let _ = std::io::stdout().flush();
}

async fn poll_for_token(
    client: &reqwest::Client,
    cloud_url: &str,
    start: &StartResponse,
) -> Result<TokenSuccess> {
    let url = format!("{cloud_url}/api/v1/auth/device/token/");
    let mut interval_secs = start.interval.max(1);
    // Hard ceiling at the cloud's stated expiry, plus a small grace.
    let deadline = std::time::Instant::now() + Duration::from_secs(start.expires_in + 5);

    loop {
        if std::time::Instant::now() >= deadline {
            println!();
            anyhow::bail!("device code expired before approval — run `pidash auth login` again");
        }

        tokio::time::sleep(Duration::from_secs(interval_secs)).await;
        print!(".");
        let _ = std::io::stdout().flush();

        let resp = client
            .post(&url)
            .json(&serde_json::json!({ "device_code": start.device_code }))
            .send()
            .await
            .with_context(|| format!("POST {url}"))?;
        let status = resp.status();
        let body_text = resp.text().await.unwrap_or_default();

        if status.is_success() {
            return serde_json::from_str::<TokenSuccess>(&body_text)
                .with_context(|| format!("parsing device-code token response: {body_text}"));
        }

        // Parse the RFC 8628 error code. Anything we don't recognise is
        // surfaced as-is so the operator can decide what to do.
        let err: TokenError = serde_json::from_str(&body_text).unwrap_or(TokenError {
            error: format!("http_{}", status.as_u16()),
        });
        match err.error.as_str() {
            "authorization_pending" => {}
            "slow_down" => {
                interval_secs = (interval_secs + 5).min(30);
            }
            "expired_token" => {
                println!();
                anyhow::bail!("device code expired — run `pidash auth login` again");
            }
            "access_denied" => {
                println!();
                anyhow::bail!("login was denied in the browser");
            }
            other => {
                println!();
                anyhow::bail!("device-code poll returned {other} (HTTP {status}): {body_text}");
            }
        }
    }
}

/// Pick which workspace this CLI install is bound to (the caller persists
/// it to `[cli].workspace_slug` with the rest of the sign-in). Always reaches out to the cloud (even if
/// a stored binding exists) so a stale slug from a removed membership
/// gets corrected.
///
/// Rules:
/// - 0 memberships → bail; the user must be invited first.
/// - 1 membership → silently use it (no prompt).
/// - explicit slug → validate membership and use it without prompting.
/// - ≥2 memberships without explicit slug → if a stored slug is still valid,
///   keep it; else prompt the user to pick.
async fn resolve_workspace_binding(
    paths: &Paths,
    cloud_url: &str,
    api_token: &str,
    explicit_workspace: Option<&str>,
) -> Result<String> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()
        .context("building HTTP client for workspace list")?;
    let workspaces = fetch_workspaces(&client, cloud_url, api_token).await?;

    if workspaces.is_empty() {
        anyhow::bail!(
            "your account isn't a member of any workspace — ask an admin to invite you, then re-run `pidash auth login`"
        );
    }

    let explicit_workspace = explicit_workspace.map(str::trim).filter(|s| !s.is_empty());
    let stored = runner_ops::load_cli_workspace(paths)?;

    let chosen_slug = if let Some(slug) = explicit_workspace {
        if workspaces.iter().any(|w| w.slug == slug) {
            slug.to_string()
        } else {
            anyhow::bail!(
                "workspace {slug:?} is not available for this account — check --workspace or ask an admin to invite you"
            );
        }
    } else if workspaces.len() == 1 {
        workspaces[0].slug.clone()
    } else if let Some(slug) = stored.as_deref()
        && workspaces.iter().any(|w| w.slug == slug)
    {
        // Stable across re-logins: keep the existing binding.
        slug.to_string()
    } else {
        if stored.is_some() {
            println!();
            println!("(Previous workspace binding is no longer valid — pick a new one.)");
        }
        let picked = pick_workspace(&workspaces)?;
        picked.slug.clone()
    };

    Ok(chosen_slug)
}

#[derive(Debug, Deserialize)]
struct WorkspaceRow {
    slug: String,
    name: String,
}

#[derive(Debug, Deserialize)]
struct WorkspaceListResponse {
    workspaces: Vec<WorkspaceRow>,
}

async fn fetch_workspaces(
    client: &reqwest::Client,
    cloud_url: &str,
    api_token: &str,
) -> Result<Vec<WorkspaceRow>> {
    let url = format!("{cloud_url}/api/v1/auth/workspaces/");
    let resp = client
        .get(&url)
        .header("X-Api-Key", api_token)
        .send()
        .await
        .with_context(|| format!("GET {url}"))?;
    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        anyhow::bail!("workspace list failed: HTTP {status}: {body}");
    }
    let parsed: WorkspaceListResponse = resp.json().await.context("parsing workspace list")?;
    Ok(parsed.workspaces)
}

async fn exchange_for_machine_token(
    client: &reqwest::Client,
    cloud_url: &str,
    api_token: &str,
    workspace_slug: &str,
    dev_machine_id: &uuid::Uuid,
    host_label: &str,
    machine_fingerprint: Option<&str>,
) -> Result<MachineTokenResponse> {
    let url = format!("{cloud_url}/api/v1/auth/machine-token/");
    let mut body = serde_json::json!({
        "workspace_slug": workspace_slug,
        "dev_machine_id": dev_machine_id,
        "host_label": host_label,
    });
    if let Some(fp) = machine_fingerprint {
        body["machine_fingerprint"] = serde_json::Value::String(fp.to_string());
    }
    let resp = client
        .post(&url)
        .header("X-Api-Key", api_token)
        .json(&body)
        .send()
        .await
        .with_context(|| format!("POST {url}"))?;
    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        anyhow::bail!("machine-token exchange failed: HTTP {status}: {body}");
    }
    resp.json::<MachineTokenResponse>()
        .await
        .context("parsing machine-token exchange response")
}

fn pick_workspace(workspaces: &[WorkspaceRow]) -> Result<&WorkspaceRow> {
    use std::io::BufRead;
    println!();
    println!("Your account belongs to multiple workspaces.");
    println!("Pick the one this host should be bound to:");
    println!();
    for (i, w) in workspaces.iter().enumerate() {
        println!("  {}) {:<20} {}", i + 1, w.slug, w.name);
    }
    print!("Pick a workspace [1-{}]: ", workspaces.len());
    std::io::stdout().flush().ok();
    let stdin = std::io::stdin();
    let mut line = String::new();
    stdin.lock().read_line(&mut line).ok();
    let ans = line.trim();
    let idx: usize = ans.parse().context("expected a number")?;
    if idx == 0 || idx > workspaces.len() {
        anyhow::bail!("selection {idx} out of range");
    }
    Ok(&workspaces[idx - 1])
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn paths_for(root: &std::path::Path) -> Paths {
        Paths {
            config_dir: root.join("config"),
            data_dir: root.join("data"),
            runtime_dir: root.join("runtime"),
            machine_context_dir: root.join("config"),
        }
    }

    fn args_with_url(url: Option<&str>) -> Args {
        Args {
            url: url.map(str::to_string),
            no_browser: true,
            workspace: None,
            force: false,
            device_code_stdin: false,
            yes: false,
        }
    }

    #[test]
    fn defaults_to_hosted_cloud_when_nothing_is_configured() {
        // The install one-liner's path: no flag, no config.toml. The user
        // must not be asked to type our own production URL.
        let tmp = tempdir().unwrap();
        let paths = paths_for(tmp.path());
        let (url, source) = resolve_cloud_url(&args_with_url(None), &paths).unwrap();
        assert_eq!(url, crate::DEFAULT_CLOUD_URL);
        assert_eq!(source, CloudUrlSource::Default);
    }

    #[test]
    fn default_cloud_url_passes_transport_validation() {
        // A default nobody types by hand still has to clear the same
        // https-only bar as a URL the user supplies.
        crate::cli::connect::validate_cloud_url(crate::DEFAULT_CLOUD_URL).unwrap();
    }

    #[test]
    fn explicit_url_flag_wins_and_trailing_slash_is_trimmed() {
        let tmp = tempdir().unwrap();
        let paths = paths_for(tmp.path());
        let args = args_with_url(Some("https://self.example.com/"));
        let (url, source) = resolve_cloud_url(&args, &paths).unwrap();
        assert_eq!(url, "https://self.example.com");
        assert_eq!(source, CloudUrlSource::Flag);
    }

    #[test]
    fn existing_config_url_is_reused_over_the_default() {
        // Re-login on an enrolled self-hosted box must stay on that cloud.
        let tmp = tempdir().unwrap();
        let paths = paths_for(tmp.path());
        runner_ops::write_cli_token(&paths, "https://self.example.com", "tok", false).unwrap();
        let (url, source) = resolve_cloud_url(&args_with_url(None), &paths).unwrap();
        assert_eq!(url, "https://self.example.com");
        assert_eq!(source, CloudUrlSource::Config);
    }

    #[test]
    fn confirmation_only_with_active_runs_unless_skipped() {
        // Same-user re-login with runs in progress still asks: the token
        // rotation reloads the daemon and stops them.
        assert!(needs_confirmation(2, false));
        assert!(!needs_confirmation(0, false));
        // `--yes` / `--device-code-stdin`.
        assert!(!needs_confirmation(2, true));
    }

    #[test]
    fn preflight_notice_mentions_parking_when_a_user_is_stored() {
        let lines = preflight_notice(3, None, 0);
        assert_eq!(
            lines,
            vec!["3 run(s) in progress will be stopped when you sign in."]
        );
        let lines = preflight_notice(1, Some("a@example.com"), 2);
        assert_eq!(lines.len(), 2);
        assert!(lines[1].contains("different account parks a@example.com's 2 runner(s)"));
    }

    #[test]
    fn device_code_is_read_from_stdin_and_trimmed() {
        assert_eq!(read_device_code(&b"DEV-123\n"[..]).unwrap(), "DEV-123");
        assert_eq!(read_device_code(&b"DEV-123"[..]).unwrap(), "DEV-123");
    }

    #[test]
    fn empty_stdin_is_an_error_rather_than_an_empty_code() {
        assert!(read_device_code(&b""[..]).is_err());
        assert!(read_device_code(&b" \n"[..]).is_err());
    }

    #[test]
    fn explicit_url_flag_wins_over_existing_config() {
        let tmp = tempdir().unwrap();
        let paths = paths_for(tmp.path());
        runner_ops::write_cli_token(&paths, "https://self.example.com", "tok", false).unwrap();
        let args = args_with_url(Some("https://other.example.com"));
        let (url, source) = resolve_cloud_url(&args, &paths).unwrap();
        assert_eq!(url, "https://other.example.com");
        assert_eq!(source, CloudUrlSource::Flag);
    }
}
