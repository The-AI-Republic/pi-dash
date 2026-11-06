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
//! 4. Exchange the returned `APIToken` for the workspace's dev-machine
//!    token and store it in `config.toml`: as the default workspace
//!    (`[cli].token`), or, with `--workspace`, next to the workspaces
//!    already signed in (`[[cli.workspaces]]`).
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
use std::io::{IsTerminal, Write};
use std::time::Duration;

use crate::cli::runner_ops;
use crate::config::file;
use crate::engine::api::{EngineApi, EngineSignInState, IssuedEngineCredential};
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

    /// Sign in this workspace, next to the ones this computer already
    /// serves. One daemon hosts runners of several workspaces, each with
    /// its own token; without this flag, login refreshes the default
    /// workspace.
    #[arg(long)]
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

    /// Sign in only the built-in engine (design §4.3): renew its 14-day
    /// credential without touching the daemon's, or add the engine to an
    /// install that signed in before it existed. Needs this computer to be
    /// signed in already (the credential is bound to its machine token).
    /// With `--device-code-stdin`, redeems an engine grant the desktop app
    /// approved.
    #[arg(long)]
    pub engine: bool,
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
    /// Issued with the daemon credential when the instance can serve the
    /// built-in engine (design §4.3).
    #[serde(default)]
    engine_credential: Option<IssuedEngineCredential>,
    #[serde(
        default,
        deserialize_with = "crate::engine::api::lenient_sign_in_state"
    )]
    engine_sign_in_state: Option<EngineSignInState>,
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
    /// The engine credential, when the server issues it with the machine
    /// token rather than with the device token.
    #[serde(default)]
    engine_credential: Option<IssuedEngineCredential>,
    #[serde(
        default,
        deserialize_with = "crate::engine::api::lenient_sign_in_state"
    )]
    engine_sign_in_state: Option<EngineSignInState>,
}

#[derive(Debug, Deserialize)]
struct TokenError {
    error: String,
}

pub async fn run(args: Args, paths: &Paths) -> Result<()> {
    // Signing in replaces keychain items the keychain can never return:
    // the ones this sign-in stores a new value for. Other credentials'
    // broken items are kept as they are.
    use crate::config::secrets::{ForgetScope, ITEM_ENGINE_CREDENTIAL, forget_unreadable};
    let scope = if args.engine {
        vec![ITEM_ENGINE_CREDENTIAL.to_string()]
    } else {
        Vec::new()
    };
    let _forget = forget_unreadable(paths, ForgetScope::Items(scope));
    if args.engine {
        return engine_login(&args, paths).await;
    }
    let outcome = login_and_bind_workspace(&args, paths).await?;

    println!();
    println!("Start using Pi Dash at {}", outcome.cloud_url);
    Ok(())
}

pub async fn run_auth_only(args: Args, paths: &Paths) -> Result<()> {
    let _forget = crate::config::secrets::forget_unreadable(
        paths,
        crate::config::secrets::ForgetScope::Items(Vec::new()),
    );
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

    // Refuse a cloud mismatch, and make sure `config.toml` exists so the
    // dev-machine id can be persisted. Only after approval, so an aborted
    // login on a fresh host leaves no config behind (bare `pidash` then
    // still drops into login).
    runner_ops::bind_cloud(paths, &cloud_url, args.force)
        .context("binding config.toml to the cloud")?;

    println!();
    if let Some(email) = token.user_email.as_deref() {
        println!("✓ Browser approved sign-in as {email}.");
    } else {
        println!("✓ Browser approved sign-in.");
    }

    // One daemon serves several workspaces (design §4.9). `--workspace`
    // adds that workspace next to the ones already signed in; without it
    // the login refreshes the default workspace (auto-picked when the
    // account has one membership, the stored one when still valid, else
    // a prompt). The short-lived bridge APIToken is never persisted: it is
    // exchanged for the workspace's dev-machine token below.
    let explicit_workspace = args
        .workspace
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let workspace_slug =
        choose_workspace(paths, &cloud_url, &token.access_token, explicit_workspace).await?;
    // A login that mints the machine id creates a new machine on the
    // server: the case where it may replace an old one (design §4.10).
    let minted_machine_id = file::load_config(paths)
        .map(|c| c.daemon.dev_machine_id.is_none())
        .unwrap_or(true);
    let dev_machine_id =
        runner_ops::ensure_dev_machine_id(paths).context("ensuring local dev-machine identity")?;
    let host_label = crate::util::hostname::default_hostname();
    let machine_token = exchange_for_machine_token(
        &client,
        &cloud_url,
        &token.access_token,
        &workspace_slug,
        &dev_machine_id,
        &host_label,
        paths,
    )
    .await?;
    // This workspace's own item is replaced (or, when it becomes the
    // default, superseded) by this write, so a broken copy of it may go.
    let _forget_own = crate::config::secrets::forget_unreadable(
        paths,
        crate::config::secrets::ForgetScope::Items(crate::config::secrets::ForgetScope::workspace(
            &machine_token.workspace_slug,
            false,
        )),
    );
    let cfg = runner_ops::store_workspace_token(
        paths,
        &cloud_url,
        &machine_token.workspace_slug,
        &machine_token.machine_token,
        explicit_workspace.is_none(),
        args.force,
    )
    .context("writing dev-machine token to config.toml")?;
    // The engine credential arrives with this sign-in when the instance
    // can serve the engine (design §4.3): on the device token or the
    // machine-token exchange.
    let engine_credential = machine_token
        .engine_credential
        .clone()
        .or(token.engine_credential.clone());
    let engine_state = machine_token
        .engine_sign_in_state
        .or(token.engine_sign_in_state);
    if let Some(credential) = engine_credential {
        runner_ops::store_engine_credential(
            paths,
            Some(&credential),
            engine_state.unwrap_or(EngineSignInState::Active),
        )
        .context("storing the engine credential")?;
        println!(
            "  Built-in engine: signed in{}",
            until(credential.expires_at)
        );
    } else {
        println!("  Built-in engine: not signed in by this server (`pidash login --engine` later)");
    }
    let user_id = fetch_user_id(&client, &cloud_url, &machine_token.machine_token).await;
    if let Err(e) =
        runner_ops::store_account(paths, token.user_email.as_deref(), user_id.as_deref())
    {
        tracing::warn!("recording the signed-in account failed: {e:#}");
    }
    println!("  Workspace: {}", machine_token.workspace_slug);
    let signed_in = cfg
        .cli
        .as_ref()
        .map(|c| c.workspace_slugs())
        .unwrap_or_default();
    if signed_in.len() > 1 {
        println!("  Workspaces on this computer: {}", signed_in.join(", "));
    }
    report_credentials_store(paths);

    // A running daemon picks the new or refreshed token up without a
    // restart. Best-effort: no daemon, or an older one, just means the
    // token is used from the next start.
    crate::service::reload::notify_credentials_changed(paths).await;

    // "Is this <label>, last seen <date>? Replace it" (design §4.10). Only
    // for a new machine and only with someone at the terminal: the desktop
    // app's pre-approved sign-in (`--device-code-stdin`) asks in its own
    // window.
    if minted_machine_id && !args.device_code_stdin && std::io::stdin().is_terminal() {
        super::replace::offer_replace_after_login(
            &client,
            &cloud_url,
            &machine_token.machine_token,
            &dev_machine_id,
        )
        .await;
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

/// ` (until <date>)`, or nothing when the expiry is unknown.
fn until(expires_at: Option<chrono::DateTime<chrono::Utc>>) -> String {
    expires_at
        .map(|e| format!(" (until {})", e.format("%Y-%m-%d")))
        .unwrap_or_default()
}

/// A sign-in moves plaintext credentials into the OS keychain under
/// `credentials_store = "auto"` (design §4.3), and says where they are.
fn report_credentials_store(paths: &Paths) {
    match file::migrate_credentials_store(paths) {
        Ok((_, "keychain")) => println!("  Credentials: stored in the OS keychain"),
        Ok(_) => println!(
            "  Credentials: stored in {} (0600)",
            paths.config_path().display()
        ),
        Err(e) => println!("  (Credentials stay where they were: {e:#})"),
    }
}

/// `pidash login --engine`: an engine-only device grant, redeemed with this
/// computer's machine token so the credential is bound to this machine.
async fn engine_login(args: &Args, paths: &Paths) -> Result<()> {
    let cfg = file::load_config_opt(paths)?.ok_or_else(|| {
        anyhow::anyhow!("this computer is not signed in; run `pidash auth login` first")
    })?;
    let cloud_url = match args.url.as_deref() {
        Some(u) if u.trim_end_matches('/') != cfg.daemon.cloud_url => anyhow::bail!(
            "this computer is signed in to {}; `--url {u}` does not match",
            cfg.daemon.cloud_url
        ),
        _ => cfg.daemon.cloud_url.clone(),
    };
    let machine_token = cfg
        .cli
        .as_ref()
        .and_then(|c| {
            c.workspace_tokens()
                .into_iter()
                .map(|(_, t)| t)
                .find(|t| t.starts_with("mt_"))
        })
        .ok_or_else(|| {
            anyhow::anyhow!(
                "this computer holds no daemon credential; run `pidash auth login` first (it signs in the engine too)"
            )
        })?;
    let api = EngineApi::new(&cloud_url)?;
    let (device_code, interval, expires_in) = if args.device_code_stdin {
        (read_device_code(std::io::stdin().lock())?, 1, 300)
    } else {
        let raw = api
            .start_engine_grant()
            .await
            .map_err(|e| anyhow::anyhow!("starting the engine sign-in failed: {e}"))?;
        let start: StartResponse =
            serde_json::from_value(raw).context("parsing the device-code start response")?;
        print_user_code_block(&start);
        if !args.no_browser {
            let with_code = format!("{}?code={}", start.verification_uri, start.user_code);
            let _ = crate::util::browser::open_url(&with_code);
        }
        (start.device_code, start.interval, start.expires_in)
    };
    let granted = poll_engine_grant(
        &api,
        &device_code,
        &machine_token,
        Duration::from_secs(interval.max(1)),
        Duration::from_secs(expires_in + 5),
    )
    .await?;
    let credential = granted.engine_credential;
    runner_ops::store_engine_credential(
        paths,
        Some(&credential),
        granted
            .engine_sign_in_state
            .unwrap_or(EngineSignInState::Active),
    )
    .context("storing the engine credential")?;
    println!();
    println!(
        "✓ Built-in engine signed in{}.",
        until(credential.expires_at)
    );
    report_credentials_store(paths);
    crate::service::reload::notify_credentials_changed(paths).await;
    Ok(())
}

/// Redeem an engine-only grant until the user approves it, with the
/// machine token that binds the credential to this computer.
pub(crate) async fn poll_engine_grant(
    api: &EngineApi,
    device_code: &str,
    machine_token: &str,
    interval: Duration,
    within: Duration,
) -> Result<crate::engine::api::EngineGrantResponse> {
    use crate::engine::api::GrantPoll;
    let deadline = std::time::Instant::now() + within;
    let mut interval = interval;
    loop {
        match api.redeem_engine_grant(device_code, machine_token).await {
            Ok(GrantPoll::Granted(granted)) => return Ok(granted),
            Ok(GrantPoll::Pending) => {}
            // RFC 8628 §3.5: add 5 seconds to the interval.
            Ok(GrantPoll::SlowDown) => {
                interval = (interval + Duration::from_secs(5)).min(Duration::from_secs(30));
            }
            Err(e) => match e.reason_code() {
                Some("expired_token") => {
                    anyhow::bail!(
                        "the engine sign-in code expired — run `pidash login --engine` again"
                    )
                }
                Some("access_denied") => {
                    anyhow::bail!("the engine sign-in was denied in the browser")
                }
                _ => anyhow::bail!("the engine sign-in failed: {e}"),
            },
        }
        if std::time::Instant::now() >= deadline {
            anyhow::bail!(
                "the engine sign-in code expired before approval — run `pidash login --engine` again"
            );
        }
        print!(".");
        let _ = std::io::stdout().flush();
        tokio::time::sleep(interval).await;
    }
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
            // Not quoting the body: it holds the access token.
            return serde_json::from_str::<TokenSuccess>(&body_text).map_err(|e| {
                anyhow::anyhow!(
                    "parsing device-code token response: {}",
                    crate::engine::api::unparsable(&e)
                )
            });
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

/// Best-effort read of the signed-in user's id, for the IPC `Hello`.
async fn fetch_user_id(client: &reqwest::Client, cloud_url: &str, token: &str) -> Option<String> {
    #[derive(Deserialize)]
    struct Me {
        #[serde(default)]
        id: Option<serde_json::Value>,
    }
    let url = format!("{cloud_url}/api/v1/users/me/");
    let resp = client
        .get(&url)
        .header("X-Api-Key", token)
        .send()
        .await
        .ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let me: Me = resp.json().await.ok()?;
    match me.id? {
        serde_json::Value::String(s) => Some(s),
        other => Some(other.to_string()),
    }
}

/// Pick which workspace this login signs in. Always reaches out to the
/// cloud (even if a stored binding exists) so a stale slug from a
/// removed membership gets corrected. Writes nothing: the caller stores
/// the workspace together with its token.
///
/// Rules:
/// - 0 memberships → bail; the user must be invited first.
/// - 1 membership → silently use it (no prompt).
/// - explicit slug → validate membership and use it without prompting.
/// - ≥2 memberships without explicit slug → if the stored default is
///   still valid, keep it; else prompt the user to pick.
async fn choose_workspace(
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
    paths: &Paths,
) -> Result<MachineTokenResponse> {
    let url = format!("{cloud_url}/api/v1/auth/machine-token/");
    let resp = client
        .post(&url)
        .header("X-Api-Key", api_token)
        .json(&serde_json::json!({
            "workspace_slug": workspace_slug,
            "dev_machine_id": dev_machine_id,
            "host_label": host_label,
        }))
        .send()
        .await
        .with_context(|| format!("POST {url}"))?;
    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(machine_token_exchange_error(
            status,
            &body,
            &paths.config_path(),
        ));
    }
    // Read as text and parse here: serde's own message can quote a value,
    // and this response carries the `mt_` and `ec_` credentials.
    let body = resp
        .text()
        .await
        .context("reading machine-token exchange response")?;
    parse_machine_token_response(&body)
}

fn parse_machine_token_response(body: &str) -> Result<MachineTokenResponse> {
    serde_json::from_str::<MachineTokenResponse>(body).map_err(|e| {
        anyhow::anyhow!(
            "parsing machine-token exchange response: {}",
            crate::engine::api::unparsable(&e)
        )
    })
}

fn machine_token_exchange_error(
    status: reqwest::StatusCode,
    body: &str,
    config_path: &std::path::Path,
) -> anyhow::Error {
    // Only this structured server error indicates a possible ownership conflict.
    // Other 404s and malformed responses should keep their diagnostic details.
    if status == reqwest::StatusCode::NOT_FOUND
        && serde_json::from_str::<TokenError>(body)
            .is_ok_and(|error| error.error == "dev_machine_not_found")
    {
        return anyhow::anyhow!(
            "Browser approval succeeded, but Pi Dash could not finish setting up this computer.\n\n\
             The saved computer ID may already belong to a different Pi Dash account.\n\
             Try signing in with the account previously used on this computer.\n\n\
             To use a different account, stop Pi Dash and back up this settings file:\n  {}\n\
             Then remove only `dev_machine_id` from its [daemon] section and run `pidash auth login` again.\n\
             Existing runners may need to be registered again.\n\n\
             Details: HTTP {status}; server error: dev_machine_not_found",
            config_path.display()
        );
    }

    anyhow::anyhow!("machine-token exchange failed: HTTP {status}: {body}")
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

    #[test]
    fn a_machine_token_parse_error_never_prints_the_credentials() {
        let body = r#"{"machine_token":"mt_secret","workspace_slug":"acme","engine_credential":"ec_secret"}"#;
        let err = parse_machine_token_response(body).unwrap_err();
        let msg = format!("{err:#}");
        assert!(!msg.contains("ec_secret"), "{msg}");
        assert!(!msg.contains("mt_secret"), "{msg}");
        assert!(
            msg.contains("parsing machine-token exchange response"),
            "{msg}"
        );
    }
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
            engine: false,
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

    #[test]
    fn sign_in_responses_may_carry_the_engine_credential() {
        let t: TokenSuccess = serde_json::from_str(
            r#"{"access_token":"a","user_email":"d@x","engine_credential":
                {"token":"ec_1","expires_at":"2026-11-18T00:00:00Z"},"engine_sign_in_state":"active"}"#,
        )
        .unwrap();
        assert_eq!(t.engine_credential.unwrap().token, "ec_1");
        assert_eq!(t.engine_sign_in_state, Some(EngineSignInState::Active));
        // Older servers: no engine fields.
        let m: MachineTokenResponse =
            serde_json::from_str(r#"{"machine_token":"mt_1","workspace_slug":"acme"}"#).unwrap();
        assert!(m.engine_credential.is_none());
    }

    #[tokio::test]
    async fn engine_login_redeems_the_grant_with_the_machine_token() {
        use crate::engine::test_server::{self, Handler};
        use std::sync::Arc;
        use std::sync::atomic::{AtomicUsize, Ordering};
        let polls = Arc::new(AtomicUsize::new(0));
        let p = polls.clone();
        let handler: Handler = Arc::new(move |req| {
            assert_eq!(req.path, crate::engine::api::DEVICE_TOKEN_PATH);
            // The machine token binds the credential to this machine.
            if req.bearer() != Some("mt_acme") {
                return (401, r#"{"error":"machine_token_required"}"#.into());
            }
            assert_eq!(req.json()["device_code"], "DEV-1");
            if p.fetch_add(1, Ordering::SeqCst) == 0 {
                return (400, r#"{"error":"authorization_pending"}"#.into());
            }
            (
                200,
                r#"{"engine_credential":{"token":"ec_new","expires_at":"2026-11-18T00:00:00Z"},
                    "engine_sign_in_state":"active"}"#
                    .into(),
            )
        });
        let server = test_server::start(handler).await;
        let api = EngineApi::new(&server.url).unwrap();
        let granted = poll_engine_grant(
            &api,
            "DEV-1",
            "mt_acme",
            Duration::from_millis(10),
            Duration::from_secs(5),
        )
        .await
        .unwrap();
        assert_eq!(granted.engine_credential.token, "ec_new");
        assert_eq!(polls.load(Ordering::SeqCst), 2, "pending, then approved");

        // Stored next to the daemon credentials, sign-in state recorded.
        let tmp = tempdir().unwrap();
        let paths = paths_for(tmp.path());
        runner_ops::write_cli_token(&paths, &server.url, "mt_acme", false).unwrap();
        runner_ops::store_engine_credential(
            &paths,
            Some(&granted.engine_credential),
            granted.engine_sign_in_state.unwrap(),
        )
        .unwrap();
        let cfg = file::load_config(&paths).unwrap();
        let cli = cfg.cli.unwrap();
        assert_eq!(cli.engine_credential.unwrap().token, "ec_new");
        assert_eq!(cli.engine_sign_in_state, Some(EngineSignInState::Active));
        assert_eq!(
            cli.token.as_deref(),
            Some("mt_acme"),
            "daemon credential untouched"
        );

        // A denied grant is an error, not a credential.
        let denied: Handler = Arc::new(|_| (400, r#"{"error":"access_denied"}"#.into()));
        let server = test_server::start(denied).await;
        let api = EngineApi::new(&server.url).unwrap();
        let err = poll_engine_grant(
            &api,
            "DEV-1",
            "mt_acme",
            Duration::from_millis(10),
            Duration::from_secs(5),
        )
        .await
        .unwrap_err();
        assert!(err.to_string().contains("denied"), "{err}");
    }
}
