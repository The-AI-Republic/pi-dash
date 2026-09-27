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
//! 4. Write the returned `APIToken` to `[cli].token` in `config.toml`.
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
    #[arg(long, hide = true, value_name = "CODE")]
    pub device_code: Option<String>,
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

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .context("building HTTP client")?;

    let start = match args.device_code.as_deref() {
        // Pre-approved by the desktop app: nothing to show and nobody to ask,
        // just the exchange. `expires_in` bounds the polling; the server is
        // authoritative and ends the poll with a terminal error if the grant
        // is already gone.
        Some(code) => StartResponse {
            device_code: code.to_string(),
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

    // Persist the short-lived bridge APIToken just long enough to seed the
    // local config and workspace binding. It is replaced below by a shared
    // dev-machine MachineToken, which is the credential used by both the CLI
    // and all runners hosted by this install.
    runner_ops::write_cli_token(paths, &cloud_url, &token.access_token)
        .context("writing temporary [cli].token to config.toml")?;

    println!();
    if let Some(email) = token.user_email.as_deref() {
        println!("✓ Logged in as {email}.");
    } else {
        println!("✓ Logged in.");
    }

    // v1: a CLI install is bound to one workspace. Resolve which one
    // (auto-pick if there's only one membership; prompt otherwise),
    // persist the choice, and use it for any subsequent runner-add.
    let workspace_slug = resolve_workspace_binding(
        paths,
        &cloud_url,
        &token.access_token,
        args.workspace.as_deref(),
    )
    .await?;
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
    )
    .await?;
    runner_ops::write_cli_token(paths, &cloud_url, &machine_token.machine_token)
        .context("writing dev-machine token to config.toml")?;
    if machine_token.workspace_slug != workspace_slug {
        runner_ops::write_cli_workspace(paths, &machine_token.workspace_slug)
            .context("writing returned workspace binding to config.toml")?;
    }
    println!("  Workspace: {}", machine_token.workspace_slug);

    Ok(LoginOutcome { cloud_url })
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

/// Pick which workspace this CLI install is bound to and persist it
/// to `[cli].workspace_slug`. Always reaches out to the cloud (even if
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

    runner_ops::write_cli_workspace(paths, &chosen_slug)
        .context("writing [cli].workspace_slug to config.toml")?;
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
        }
    }

    fn args_with_url(url: Option<&str>) -> Args {
        Args {
            url: url.map(str::to_string),
            no_browser: true,
            workspace: None,
            device_code: None,
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
        runner_ops::write_cli_token(&paths, "https://self.example.com", "tok").unwrap();
        let (url, source) = resolve_cloud_url(&args_with_url(None), &paths).unwrap();
        assert_eq!(url, "https://self.example.com");
        assert_eq!(source, CloudUrlSource::Config);
    }

    #[test]
    fn explicit_url_flag_wins_over_existing_config() {
        let tmp = tempdir().unwrap();
        let paths = paths_for(tmp.path());
        runner_ops::write_cli_token(&paths, "https://self.example.com", "tok").unwrap();
        let args = args_with_url(Some("https://other.example.com"));
        let (url, source) = resolve_cloud_url(&args, &paths).unwrap();
        assert_eq!(url, "https://other.example.com");
        assert_eq!(source, CloudUrlSource::Flag);
    }
}
