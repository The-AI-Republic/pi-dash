// Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod about;
mod account_settings;
mod chat;
mod chat_history;
mod desktop_http;
// build.rs owns this module; it is compiled here only so its tests run.
#[cfg(test)]
#[path = "../dist_guard.rs"]
mod dist_guard;
mod ipc;
mod managed_runner;
mod pidash_cli;
mod updates;

use std::sync::Mutex;
use std::time::{Duration, Instant};

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use sha2::{Digest, Sha256};
use tauri::{
    Manager, Url, WebviewUrl, WebviewWindowBuilder, WindowEvent,
    webview::NewWindowResponse,
    menu::{MenuBuilder, MenuItemBuilder, SubmenuBuilder},
    tray::{MouseButton, TrayIconBuilder, TrayIconEvent},
};
use tauri_plugin_deep_link::DeepLinkExt;
use tauri_plugin_opener::OpenerExt;

const ZOOM_STEP: f64 = 0.1;
const ZOOM_MIN: f64 = 0.3;
const ZOOM_MAX: f64 = 3.0;

struct ZoomState(Mutex<f64>);

/// Compile-time target URL. Lives in app state so the deep-link handler
/// can derive the `/api/auth/desktop-exchange/` URL on the same origin.
struct AppConfig {
    target_url: Url,
    /// Origin the bundled SPA is served from (`WebviewUrl::App`). `None` in
    /// hot-reload mode, where the webview *is* the server host and there is
    /// no bundle to bounce back to.
    bundle_root: Option<Url>,
    /// Public origin of the hosted web app — what an in-app path becomes
    /// when it has to leave the window (see `browser_url_for_new_window`).
    web_base: Url,
}

/// Where Tauri serves `WebviewUrl::App` from. Windows is plain `http`
/// because `app.windows[].useHttpsScheme` defaults to false and nothing
/// here opts in — the API server's CORS/CSRF allowlist must list this
/// exact origin (scheme included) or every credentialed XHR is refused.
#[cfg(windows)]
const BUNDLE_ORIGIN: &str = "http://tauri.localhost";
#[cfg(not(windows))]
const BUNDLE_ORIGIN: &str = "tauri://localhost";

/// API base `dev-prep.sh` bakes into the SPA when `VITE_API_BASE_URL` is
/// unset. Must stay equal to the script's `API_BASE` default.
const DEV_API_BASE: &str = "http://localhost:8000";

/// API origin the native transport allowlists and advertises to the SPA as
/// `__PIDASH_NATIVE_HTTP__`. It has to be the origin the SPA was built
/// against: a request to any other origin leaves the native transport for
/// the webview's own, which sends no API cookies.
///
/// `vite_api_base` is the build-time `VITE_API_BASE_URL`. Unset, a
/// bundled-dev build follows `dev-prep.sh` and assumes a local API whatever
/// `PI_DASH_URL` (the sign-in target) says; hot-reload has no baked SPA and
/// keeps following the target. Release builds always have it set (build.rs
/// refuses otherwise).
fn api_base_for<'a>(vite_api_base: Option<&'a str>, target: &'a str, bundled_dev: bool) -> &'a str {
    match vite_api_base {
        Some(v) if !v.is_empty() => v,
        _ if bundled_dev => DEV_API_BASE,
        _ => target,
    }
}

/// The API base recorded in `dist/bake-info.txt`, when the binary resolved a
/// different origin than the bundled SPA was built against.
fn baked_api_base_mismatch<'a>(baked: Option<&'a str>, api_url: &Url) -> Option<&'a str> {
    let baked = baked?;
    let baked_url = Url::parse(baked).ok()?;
    (baked_url.origin() != api_url.origin()).then_some(baked)
}

/// Origin the bundled SPA is actually served from. `cargo tauri dev` has no
/// `tauri://` bundle: the CLI serves `dist/` from its own dev server and
/// bakes that address in as `build.devUrl`, which Tauri then resolves
/// `WebviewUrl::App` against. Everywhere else it is `BUNDLE_ORIGIN`.
fn bundle_root_for(dev_url: Option<&Url>) -> Url {
    dev_url
        .cloned()
        .unwrap_or_else(|| Url::parse(BUNDLE_ORIGIN).expect("BUNDLE_ORIGIN is a valid URL"))
}

/// Bundle URL for an in-app path: same path and query, bundle origin.
/// Tauri's asset resolver falls back to `index.html` for any path that
/// isn't a file in `dist/`, so React Router boots the right route from a
/// hard load of e.g. `/acme/projects/` — no `/`-funnel needed.
fn bundle_url(bundle_root: &Url, path_and_query: &str) -> Url {
    bundle_root
        .join(path_and_query)
        .unwrap_or_else(|_| bundle_root.clone())
}

/// The desktop window never shows the server's hosted web app — only the
/// bundled SPA. The one legitimate trip to the server host is the sign-in
/// hand-off (`/api/auth/desktop-exchange/`, navigated by `handle_deep_link`
/// so the session cookies land in the webview's jar). That view answers
/// with a relative redirect to an app route (or `/sign-in?error=…` when
/// the exchange fails), which would otherwise load `https://<server>/…` —
/// whatever web UI the server hosts — inside the app. Map any non-API
/// navigation on the server host to the same path on the bundle.
fn bundle_redirect_for(url: &Url, server: &Url, bundle_root: &Url) -> Option<Url> {
    let same_host = url.scheme() == server.scheme()
        && url.host_str() == server.host_str()
        && url.port_or_known_default() == server.port_or_known_default();
    if !same_host || url.path().starts_with("/api/") {
        return None;
    }
    let path_and_query = match url.query() {
        Some(q) => format!("{}?{}", url.path(), q),
        None => url.path().to_string(),
    };
    Some(bundle_url(bundle_root, &path_and_query))
}

/// Where a new-window request (`window.open(…, "_blank")`, a
/// `target="_blank"` link) goes in the user's default browser.
///
/// The desktop app is a single window: the webview has no tab or popup
/// surface, and without a handler wry drops these requests on every
/// platform, so "Open in new tab" silently did nothing. An in-app path
/// resolves against the bundle origin, which nothing outside the app can
/// load — map it to the same path on the hosted web app, as
/// `desktop-overlay`'s `desktopWebUrl` does for copied links. Any other
/// http(s) URL opens unchanged. `None` means refuse: the same http/https
/// restriction as `open_in_browser`.
fn browser_url_for_new_window(url: &Url, bundle_root: Option<&Url>, web_base: &Url) -> Option<Url> {
    // Checked first: on Windows the bundle origin is itself an http URL.
    let on_bundle = bundle_root.is_some_and(|root| {
        url.scheme() == root.scheme()
            && url.host_str() == root.host_str()
            && url.port_or_known_default() == root.port_or_known_default()
    });
    let target = if on_bundle {
        let mut target = web_base.join(url.path()).ok()?;
        target.set_query(url.query());
        target.set_fragment(url.fragment());
        target
    } else {
        url.clone()
    };
    matches!(target.scheme(), "http" | "https").then_some(target)
}

/// Whether this executable runs from an installed MSIX package; Windows
/// installs package files under `...\WindowsApps\<package>\`.
#[cfg(any(test, target_os = "linux", target_os = "windows"))]
fn is_msix_install(exe: &std::path::Path) -> bool {
    exe.components()
        .any(|c| c.as_os_str().eq_ignore_ascii_case("WindowsApps"))
}

fn show_main_window(app: &tauri::AppHandle) {
    let Some(window) = app.get_webview_window("main") else {
        eprintln!("window: main window not found");
        return;
    };
    if let Err(e) = window.show() {
        eprintln!("window: show failed: {e}");
    }
    if let Err(e) = window.unminimize() {
        eprintln!("window: unminimize failed: {e}");
    }
    if let Err(e) = window.set_focus() {
        eprintln!("window: focus failed: {e}");
    }
}

/// Open a URL in the user's default browser. Invoked from the web UI when a
/// sign-in provider must run in the real browser rather than the embedded
/// webview (an OIDC flow, for instance).
///
/// `withGlobalTauri` exposes this command to any JS running in the webview.
/// We trust the loaded origin, but a future XSS there would otherwise be
/// able to coerce the OS into opening `file://` or `javascript:` URLs.
/// Restrict to http/https.
#[tauri::command]
async fn open_in_browser(app: tauri::AppHandle, url: String) -> Result<(), String> {
    let parsed = Url::parse(&url).map_err(|e| format!("invalid URL: {e}"))?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err(format!("scheme not allowed: {}", parsed.scheme()));
    }
    app.opener()
        .open_url(url, None::<&str>)
        .map_err(|e| e.to_string())
}

/// The sign-in URL with this app's challenge on it, replacing any the caller
/// supplied.
fn sign_in_url_with_challenge(url: &Url, challenge: &str) -> Url {
    let kept: Vec<(String, String)> = url
        .query_pairs()
        .filter(|(k, _)| k != "desktop_challenge")
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect();
    let mut bound = url.clone();
    bound
        .query_pairs_mut()
        .clear()
        .extend_pairs(kept)
        .append_pair("desktop_challenge", challenge);
    bound
}

/// Start a desktop sign-in: open the server's sign-in URL in the default
/// browser and remember that this app instance is waiting for the callback.
///
/// `pidash://auth/callback` links can be opened by anything on the machine —
/// including a link built from someone else's sign-in, which would otherwise
/// sign this app into their account. So the app keeps a secret per sign-in,
/// sends only its hash here (`desktop_challenge`), and `handle_deep_link`
/// honours a callback only while one is pending, handing the secret to the
/// server to check against the flow.
#[tauri::command]
async fn desktop_begin_sign_in(app: tauri::AppHandle, url: String) -> Result<(), String> {
    let parsed = Url::parse(&url).map_err(|e| format!("invalid URL: {e}"))?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err(format!("scheme not allowed: {}", parsed.scheme()));
    }
    let challenge = app.state::<SignInState>().begin(Instant::now())?;
    app.opener()
        .open_url(sign_in_url_with_challenge(&parsed, &challenge).as_str(), None::<&str>)
        .map_err(|e| e.to_string())
}

/// How long a started sign-in waits for its callback. The server keeps the
/// flow for 10 minutes by default; a callback that arrives later has no flow
/// left to complete.
const SIGN_IN_TTL: Duration = Duration::from_secs(10 * 60);

struct PendingSignIn {
    verifier: String,
    started: Instant,
}

/// The sign-in this app instance started and is waiting on, if any.
#[derive(Default)]
struct SignInState(Mutex<Option<PendingSignIn>>);

impl SignInState {
    /// Record that a sign-in is starting and return the challenge the server
    /// stores on the flow. A second start while one is still pending keeps the
    /// same verifier, so whichever browser tab finishes first completes.
    fn begin(&self, now: Instant) -> Result<String, String> {
        let mut pending = self.0.lock().unwrap_or_else(|e| e.into_inner());
        let verifier = match pending.take() {
            Some(p) if now.duration_since(p.started) < SIGN_IN_TTL => p.verifier,
            _ => new_sign_in_verifier()?,
        };
        let challenge = sign_in_challenge(&verifier);
        *pending = Some(PendingSignIn { verifier, started: now });
        Ok(challenge)
    }

    /// The pending sign-in's verifier, once: a callback spends it whether or
    /// not the server goes on to accept the exchange.
    fn take(&self, now: Instant) -> Option<String> {
        let pending = self.0.lock().unwrap_or_else(|e| e.into_inner()).take()?;
        (now.duration_since(pending.started) < SIGN_IN_TTL).then_some(pending.verifier)
    }
}

fn new_sign_in_verifier() -> Result<String, String> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).map_err(|e| format!("no random source: {e}"))?;
    Ok(URL_SAFE_NO_PAD.encode(bytes))
}

/// S256, as in PKCE: the server sees only this hash until the callback.
fn sign_in_challenge(verifier: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

/// Process an incoming `pidash://auth/callback?code=<>&state=<>` deep link.
///
/// The identity provider redirected the system browser directly to this
/// custom scheme — RFC 8252 native-app flow. The desktop is registered with
/// it as a separate client with `redirect_uri=pidash://auth/callback`, so we
/// receive the standard authorization `code` + `state`. We forward both to
/// the server's `/api/auth/desktop-exchange/` by navigating the webview
/// there, together with the secret of the sign-in this app started
/// (`desktop_begin_sign_in`); the server checks the secret against the flow,
/// looks up the PKCE verifier by `state`, completes the token exchange,
/// establishes the desktop session, and sets cookies on the webview's
/// response. The cookie jar inside the app gets populated without ever
/// putting a session token in a URL.
///
/// A callback that arrives while no sign-in is pending is dropped: this app
/// did not ask for it.
fn handle_deep_link(app: &tauri::AppHandle, url: &str) {
    let config = app.state::<AppConfig>();
    let sign_in = app.state::<SignInState>();
    let target = match deep_link_target(url, &config, &sign_in, Instant::now()) {
        Ok(target) => target,
        Err(reason) => {
            // Not the URL: an auth callback carries a live code.
            eprintln!("deep-link: ignored ({reason})");
            return;
        }
    };
    let Some(window) = app.get_webview_window("main") else {
        eprintln!("deep-link: main window not found");
        return;
    };
    show_main_window(app);
    if let Err(e) = window.navigate(target) {
        eprintln!("deep-link: navigate failed: {e}");
    }
}

/// Where a deep link sends the webview, or why it is ignored.
fn deep_link_target(
    url: &str,
    config: &AppConfig,
    sign_in: &SignInState,
    now: Instant,
) -> Result<Url, &'static str> {
    let Ok(parsed) = Url::parse(url) else {
        return Err("invalid URL");
    };
    if parsed.scheme() != "pidash"
        || parsed.host_str() != Some("auth")
        || parsed.path() != "/callback"
    {
        return Err("unrecognized URL");
    }
    let mut code: Option<String> = None;
    let mut state_token: Option<String> = None;
    let mut oidc_error: Option<String> = None;
    let mut oidc_error_description: Option<String> = None;
    for (k, v) in parsed.query_pairs() {
        match k.as_ref() {
            "code" => code = Some(v.into_owned()),
            "state" => state_token = Some(v.into_owned()),
            "error" => oidc_error = Some(v.into_owned()),
            "error_description" => oidc_error_description = Some(v.into_owned()),
            _ => {}
        }
    }
    // Spent by any callback, including one the provider reports as failed.
    let Some(verifier) = sign_in.take(now) else {
        return Err("no sign-in was started from this app");
    };

    // The provider returns `?error=...&error_description=...` on user-aborted
    // or denied auth flows. Surface that in the webview's sign-in page
    // rather than navigating to /desktop-exchange/ with empty code/state
    // (which would just produce a generic missing_params redirect).
    if let Some(err) = oidc_error {
        let mut error_url = match &config.bundle_root {
            Some(root) => bundle_url(root, "/sign-in"),
            None => {
                let mut url = config.target_url.clone();
                url.set_path("/sign-in");
                url
            }
        };
        {
            let mut pairs = error_url.query_pairs_mut();
            pairs.clear().append_pair("error", &err);
            if let Some(desc) = oidc_error_description {
                pairs.append_pair("error_description", &desc);
            }
        }
        return Ok(error_url);
    }

    let (Some(code), Some(state_token)) = (code, state_token) else {
        return Err("missing code or state param");
    };
    let mut session_url = config.target_url.clone();
    session_url.set_path("/api/auth/desktop-exchange/");
    session_url
        .query_pairs_mut()
        .clear()
        .append_pair("code", &code)
        .append_pair("state", &state_token)
        .append_pair("desktop_verifier", &verifier);
    Ok(session_url)
}

/// Wipe the webview's own session state — cookies first of all.
///
/// Sign-out posts to the server and relies on its `Set-Cookie` deletions
/// reaching this webview. They do not: the page origin is `tauri://localhost`
/// while the API is a different origin, so the deletion is dropped and the
/// session survives a "sign out" — the app lands on the sign-in page, the
/// route guard sees a live session, and bounces straight back in. The app owns
/// this cookie jar, so it clears it itself rather than trusting a cross-origin
/// response to do it.
///
/// Called by the web layer *after* the sign-out request (which needs the CSRF
/// cookie) and before it navigates.
#[tauri::command]
async fn desktop_clear_web_data<R: tauri::Runtime>(app: tauri::AppHandle<R>) -> Result<(), String> {
    let http = app.state::<desktop_http::DesktopHttp>();
    let mut generation = http.generation.lock().await;
    *generation += 1;
    let window = app
        .get_webview_window("main")
        .ok_or_else(|| "no main webview window".to_string())?;
    window
        .clear_all_browsing_data()
        .map_err(|e| format!("clearing webview data: {e}"))
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // `target_url` is the server origin that the deep-link sign-in handler
    // navigates the webview to during sign-in (see handle_deep_link). It is
    // *not* always the URL the main webview opens at startup — see HOT_RELOAD
    // below.
    //
    // In debug builds, PIDASH_DESKTOP_HOT_RELOAD=1 (build-time) selects the
    // URL-pointing dev mode: the main webview opens at PI_DASH_URL directly,
    // so a running apps/web dev server with HMR is reflected without
    // rebuilding dist/.
    // Default (unset) uses the bundled SPA from dist/ via WebviewUrl::App —
    // the same code path as release. This gives dev/prod parity by default
    // and reserves the URL-pointing mode for explicit opt-in.
    //
    // Release builds ignore PIDASH_DESKTOP_HOT_RELOAD and still require
    // PI_DASH_URL via env! — silently baking a localhost default into a
    // shipping binary would route the /api/auth/desktop-exchange call to a
    // developer's machine.
    let hot_reload: bool =
        cfg!(debug_assertions) && matches!(option_env!("PIDASH_DESKTOP_HOT_RELOAD"), Some("1"));

    #[cfg(debug_assertions)]
    let target_url_str: &str = if let Some(v) = option_env!("PI_DASH_URL") {
        v
    } else if hot_reload {
        "http://localhost:3000"
    } else {
        // Bundled-dev default: a local API server (apps/api on :8000), so
        // the bundled SPA has a backend without forcing every dev to export
        // PI_DASH_URL. Override via PI_DASH_URL at build time for any other
        // server.
        "http://localhost:8000"
    };
    #[cfg(not(debug_assertions))]
    let target_url_str: &str = env!(
        "PI_DASH_URL",
        "PI_DASH_URL must be set when building in release mode \
         (the workflow sets it; for local release builds, export it manually)"
    );

    // OSS_SHA is the OSS pi-dash commit the bundled frontend was built
    // against, embedded for audit-trail mapping (workflow run logs fall
    // out of retention). Not load-bearing for runtime behavior; surfaced
    // in the startup eprintln below for support-debugging.
    let oss_sha = option_env!("PI_DASH_OSS_SHA").unwrap_or("<unset>");
    let mode = if hot_reload { "hot-reload" } else { "bundled" };
    eprintln!("Pi Dash: mode={mode} server target {target_url_str} (oss-sha {oss_sha})");
    let target_url = Url::parse(target_url_str)?;
    let api_url = Url::parse(api_base_for(
        option_env!("VITE_API_BASE_URL"),
        target_url_str,
        cfg!(debug_assertions) && !hot_reload,
    ))?;
    // PIDASH_BAKED_API_BASE is emitted by build.rs from dist/bake-info.txt.
    // A mismatch (a reused dist/, or VITE_API_BASE_URL changed after the SPA
    // was built) otherwise shows up only as unexplained 401s.
    if !hot_reload
        && let Some(baked) = baked_api_base_mismatch(option_env!("PIDASH_BAKED_API_BASE"), &api_url)
    {
        eprintln!(
            "Pi Dash: WARNING the bundled SPA was built against API base {baked} but this \
             binary's native transport serves {} — API requests will bypass it and go out \
             without session cookies (401s). Rebuild both with the same VITE_API_BASE_URL \
             (PI_DASH_URL only sets the sign-in target).",
            api_url.origin().ascii_serialization()
        );
    }
    let desktop_http = desktop_http::DesktopHttp::new(api_url)?;
    let initial_webview = if hot_reload {
        WebviewUrl::External(target_url.clone())
    } else {
        WebviewUrl::App("index.html".into())
    };
    // Same fallback as `dev-prep.sh` and build.rs's `validate_baked_web_base`:
    // the web app lives on the sign-in origin unless the build says otherwise.
    let web_base = option_env!("VITE_WEB_BASE_URL")
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .and_then(|v| Url::parse(v).ok())
        .unwrap_or_else(|| target_url.clone());

    // Loud-warning safety net for the cargo-not-tauri-cli debug path. A
    // dev who runs `cargo build` / `cargo run` / IDE Run-binary on a fresh
    // clone bypasses tauri.conf's beforeDevCommand entirely; build.rs only
    // panics in release profile (rust-analyzer would crash otherwise);
    // without this warning, the window would just hang on the 118-byte
    // placeholder with no signal. PIDASH_DIST_VALID is emitted by build.rs
    // from the same size/assets check dev-prep.sh and the release guard use.
    if !hot_reload && option_env!("PIDASH_DIST_VALID") == Some("0") {
        eprintln!(
            "Pi Dash: WARNING dist/ looks like the 118-byte placeholder — the window will \
             render blank/Loading forever. Run `cargo tauri dev` (which invokes \
             desktop/scripts/dev-prep.sh) instead of plain `cargo build`/`cargo run`, \
             or set PIDASH_DESKTOP_HOT_RELOAD=1 to load from a dev server."
        );
    }

    let context = tauri::generate_context!();
    let bundle_root: Option<Url> = if hot_reload {
        None
    } else {
        // Same condition Tauri uses to pick the dev server over `tauri://`.
        let dev_url = context.config().build.dev_url.as_ref().filter(|_| tauri::is_dev());
        Some(bundle_root_for(dev_url))
    };
    // Auto-update is opt-in per build: a distributor that publishes signed
    // updates supplies `plugins.updater` (pubkey + endpoints) through a
    // `--config` overlay. Without that section the plugin cannot initialise
    // (it has no key to verify against), so it is neither registered nor
    // polled and the app simply stays on the installed version.
    let updater_enabled = context.config().plugins.0.contains_key("updater");

    let mut builder = tauri::Builder::default()
        // Must be the first plugin: a second app launch (carrying a deep
        // link on its argv on Linux/Windows) hands its URLs to the running
        // instance and exits — `single_instance` + the `deep-link` feature
        // forward those URLs into `tauri_plugin_deep_link::on_open_url`.
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            show_main_window(app);
        }))
        .plugin(tauri_plugin_deep_link::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init());
    if updater_enabled {
        builder = builder.plugin(tauri_plugin_updater::Builder::new().build());
    }

    builder
        .on_window_event(|window, event| {
            if window.label() != "main" {
                return;
            }
            if let WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                if let Err(e) = window.hide() {
                    eprintln!("window: hide-to-tray failed: {e}");
                }
            }
        })
        .manage(ZoomState(Mutex::new(1.0)))
        .manage(desktop_http)
        .manage(AppConfig { target_url, bundle_root, web_base })
        .manage(SignInState::default())
        .manage(managed_runner::DaemonState::default())
        .manage(chat::ChatState::default())
        .manage(<updates::UpdateState>::default())
        .invoke_handler(tauri::generate_handler![
            open_in_browser,
            desktop_begin_sign_in,
            desktop_http::desktop_api_request,
            desktop_http::desktop_api_stream,
            desktop_http::desktop_api_cancel,
            pidash_cli::detect_pidash_cli,
            pidash_cli::install_pidash_cli,
            pidash_cli::pidash_cli_login,
            // Direct local chat with the built-in engine (PDASHOSS01-159):
            // these reach the daemon over its IPC socket and stream Chat*
            // frames back to the webview as `chat://frame` events — never via
            // the Pi Dash cloud chat relay.
            chat::chat_warm,
            chat::chat_send,
            chat::chat_cancel,
            chat::chat_close,
            chat::chat_decide,
            // Built-in agent engine: the overlay JS holds the session and
            // makes the authenticated calls, then hands the results to these
            // commands, which own the local files and the daemon process.
            managed_runner::managed_paths,
            managed_runner::managed_bootstrap,
            managed_runner::managed_enroll,
            managed_runner::managed_write_engine_config,
            managed_runner::managed_write_model_token,
            managed_runner::managed_start_daemon,
            managed_runner::managed_stop_daemon,
            managed_runner::managed_sign_out,
            desktop_clear_web_data,
            updates::desktop_pending_update,
            updates::desktop_install_update,
            about::desktop_about,
            managed_runner::managed_doctor,
            // Direct local chat history: stored on this machine only, per
            // account, never relayed to or stored by the Pi Dash server.
            chat_history::chat_create_session,
            chat_history::chat_list_sessions,
            chat_history::chat_get_session,
            chat_history::chat_list_events,
            chat_history::chat_append_event,
            chat_history::chat_set_thread_id,
            chat_history::chat_rename_session,
            chat_history::chat_delete_session,
            chat_history::chat_clear_history,
            chat_history::chat_working_dir,
            // Per-account settings kept outside the webview's storage, which
            // sign-out wipes.
            account_settings::chat_approval_modes_save,
            account_settings::chat_approval_modes_load,
        ])
        // Fallback for the navigation policy below: if a server-host page
        // does get through (e.g. a redirect the policy hook didn't see),
        // bounce as soon as it finishes loading.
        .on_page_load(move |webview, payload| {
            if payload.event() != tauri::webview::PageLoadEvent::Finished {
                return;
            }
            let config = webview.state::<AppConfig>();
            let Some(root) = &config.bundle_root else { return };
            if let Some(target) = bundle_redirect_for(payload.url(), &config.target_url, root) {
                eprintln!("nav: server host page loaded ({}), bouncing to bundle", payload.url());
                if let Err(e) = webview.navigate(target) {
                    eprintln!("nav: bounce failed: {e}");
                }
            }
        })
        .setup(move |app| {
            let api_origin = app.state::<desktop_http::DesktopHttp>()
                .api_url.origin().ascii_serialization();
            let mut window = WebviewWindowBuilder::new(app, "main", initial_webview)
                .initialization_script(format!(
                    "Object.defineProperty(window, '__PIDASH_NATIVE_HTTP__', {{ value: {} }});",
                    serde_json::to_string(&api_origin)?
                ))
                .title("Pi Dash")
                .inner_size(1400.0, 900.0)
                .min_inner_size(800.0, 600.0)
                .resizable(true);
            let open_app = app.handle().clone();
            window = window.on_new_window(move |url, _features| {
                let config = open_app.state::<AppConfig>();
                match browser_url_for_new_window(&url, config.bundle_root.as_ref(), &config.web_base) {
                    Some(target) => {
                        if let Err(e) = open_app.opener().open_url(target.as_str(), None::<&str>) {
                            eprintln!("new-window: opening {target} in the browser failed: {e}");
                        }
                    }
                    None => eprintln!("new-window: refused {url}"),
                }
                // Never a second in-app window: the request is either handed
                // to the browser or dropped.
                NewWindowResponse::Deny
            });
            if !hot_reload {
                let bounce_app = app.handle().clone();
                window = window.on_navigation(move |url| {
                    let config = bounce_app.state::<AppConfig>();
                    let Some(root) = &config.bundle_root else { return true };
                    let Some(target) = bundle_redirect_for(url, &config.target_url, root) else {
                        return true;
                    };
                    eprintln!("nav: blocked server host navigation to {url}, bouncing to bundle");
                    let app = bounce_app.clone();
                    // Deferred: navigating from inside the policy callback
                    // would re-enter the webview mid-decision.
                    tauri::async_runtime::spawn(async move {
                        if let Some(w) = app.get_webview_window("main")
                            && let Err(e) = w.navigate(target)
                        {
                            eprintln!("nav: bounce failed: {e}");
                        }
                    });
                    false
                });
            }
            window.build()?;

            // View menu: browser-style zoom shortcuts. The Tauri webview
            // doesn't bind these by default, so we register them as menu
            // accelerators. muda's accelerator parser has no `Plus` token
            // (it would silently bind the `P` key); the numpad `+` is
            // spelled `NumpadPlus`. We register three variants routed to
            // the same `zoom_in` id; alternates are Linux/Windows only —
            // on macOS, `Cmd+=` is the conventional shortcut and the
            // duplicates would visibly clutter the menubar.
            let zoom_in = MenuItemBuilder::with_id("zoom_in", "Zoom In")
                .accelerator("CmdOrCtrl+=")
                .build(app)?;
            let zoom_out = MenuItemBuilder::with_id("zoom_out", "Zoom Out")
                .accelerator("CmdOrCtrl+-")
                .build(app)?;
            let zoom_reset = MenuItemBuilder::with_id("zoom_reset", "Actual Size")
                .accelerator("CmdOrCtrl+0")
                .build(app)?;
            #[cfg(not(target_os = "macos"))]
            let zoom_in_shift = MenuItemBuilder::with_id("zoom_in", "Zoom In")
                .accelerator("CmdOrCtrl+Shift+=")
                .build(app)?;
            #[cfg(not(target_os = "macos"))]
            let zoom_in_numpad = MenuItemBuilder::with_id("zoom_in", "Zoom In")
                .accelerator("CmdOrCtrl+NumpadPlus")
                .build(app)?;

            let view = {
                let b = SubmenuBuilder::new(app, "View")
                    .items(&[&zoom_in, &zoom_out, &zoom_reset]);
                #[cfg(not(target_os = "macos"))]
                let b = b.items(&[&zoom_in_shift, &zoom_in_numpad]);
                b.build()?
            };
            let menu = MenuBuilder::new(app).items(&[&view]).build()?;
            app.set_menu(menu)?;

            let tray_show = MenuItemBuilder::with_id("tray_show", "Show Pi Dash").build(app)?;
            let tray_quit = MenuItemBuilder::with_id("tray_quit", "Quit Pi Dash").build(app)?;
            let tray_menu = MenuBuilder::new(app)
                .items(&[&tray_show])
                .separator()
                .items(&[&tray_quit])
                .build()?;
            let mut tray_builder = TrayIconBuilder::with_id("main")
                .tooltip("Pi Dash")
                .menu(&tray_menu)
                .show_menu_on_left_click(false)
                .on_menu_event(|app, event| match event.id().as_ref() {
                    "tray_show" => show_main_window(app),
                    "tray_quit" => app.exit(0),
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        ..
                    } = event
                    {
                        show_main_window(tray.app_handle());
                    }
                });
            if let Some(icon) = app.default_window_icon().cloned() {
                tray_builder = tray_builder.icon(icon);
            }
            tray_builder.build(app)?;

            app.on_menu_event(|app, event| {
                let Some(window) = app.get_webview_window("main") else {
                    return;
                };
                let state = app.state::<ZoomState>();
                // Recover from a poisoned mutex rather than crashing every
                // future menu click — the inner f64 is independently valid.
                let mut zoom = state.0.lock().unwrap_or_else(|e| e.into_inner());
                *zoom = match event.id().as_ref() {
                    "zoom_in" => (*zoom + ZOOM_STEP).min(ZOOM_MAX),
                    "zoom_out" => (*zoom - ZOOM_STEP).max(ZOOM_MIN),
                    "zoom_reset" => 1.0,
                    _ => return,
                };
                if let Err(e) = window.set_zoom(*zoom) {
                    eprintln!("set_zoom failed: {e}");
                }
            });

            // On Linux/Windows the OS routes `pidash://` by launching a
            // new app instance; the bundler registers the scheme at install
            // time, but for dev builds and re-installs we also register at
            // runtime. macOS reads the scheme from `Info.plist` and routes
            // without a re-launch.
            //
            // A Microsoft Store (MSIX) install declares the scheme in its
            // package manifest instead. Registering at runtime there would
            // write this version's WindowsApps path into HKCU, which goes
            // stale on the next Store update and outlives an uninstall.
            #[cfg(any(target_os = "linux", target_os = "windows"))]
            if !std::env::current_exe().is_ok_and(|exe| is_msix_install(&exe))
                && let Err(e) = app.deep_link().register("pidash")
            {
                eprintln!("deep-link: register failed: {e}");
            }
            let handle = app.handle().clone();
            app.deep_link().on_open_url(move |event| {
                for url in event.urls() {
                    handle_deep_link(&handle, url.as_str());
                }
            });

            // Background updater checks. Spawned (not awaited) so launch isn't
            // blocked by network — if a new version is available the dialog
            // appears moments after the window opens. The daily check only
            // surfaces the sidebar update button (see `updates`).
            if updater_enabled {
                tauri::async_runtime::spawn(updates::prompt_at_launch(app.handle().clone()));
                tauri::async_runtime::spawn(updates::run_daily_checks(app.handle().clone()));
            }

            // The agent uses the bundled CLI. Launching the desktop must not
            // install a second CLI into the user's PATH or personal config.

            Ok(())
        })
        .build(context)?
        .run(|app, event| {
            // Stop the bundled daemon when the app exits. Without this it
            // outlives the window: the server would keep the runner ONLINE and
            // dispatch work to a machine whose user has closed Pi Dash.
            if let tauri::RunEvent::Exit = event {
                managed_runner::shutdown(app);
            }
        });

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn msix_installs_are_detected_by_their_package_directory() {
        use std::path::Path;
        // Windows paths split on `\` only on Windows; `/` is a separator on
        // every platform, so these cases run in CI on any host.
        assert!(is_msix_install(Path::new(
            "C:/Program Files/WindowsApps/AIRepublic.PiDash_0.3.1.0_x64__acjt9zndhe44g/pi-dash-desktop.exe"
        )));
        assert!(is_msix_install(Path::new(
            "C:/Program Files/windowsapps/AIRepublic.PiDash_0.3.1.0_x64__acjt9zndhe44g/pi-dash-desktop.exe"
        )));
        assert!(!is_msix_install(Path::new(
            "C:/Users/me/AppData/Local/Pi Dash/pi-dash-desktop.exe"
        )));
        assert!(!is_msix_install(Path::new(
            "/opt/WindowsAppsBackup/pi-dash-desktop"
        )));
    }

    #[test]
    fn bundled_dev_api_base_matches_the_origin_dev_prep_bakes() {
        // dev-prep.sh bakes this default into the SPA when VITE_API_BASE_URL
        // is unset, whatever PI_DASH_URL says; the two must not drift apart.
        assert!(
            include_str!("../../scripts/dev-prep.sh")
                .contains(&format!("API_BASE=\"${{VITE_API_BASE_URL:-{DEV_API_BASE}}}\"")),
            "dev-prep.sh no longer defaults API_BASE to {DEV_API_BASE}"
        );
        let target = "https://pidash.example.com";
        assert_eq!(api_base_for(None, target, true), DEV_API_BASE);
        assert_eq!(api_base_for(Some(""), target, true), DEV_API_BASE);
        assert_eq!(api_base_for(Some("https://api.example.com"), target, true), "https://api.example.com");
        // Hot-reload and release keep following the server target.
        assert_eq!(api_base_for(None, target, false), target);
    }

    #[test]
    fn baked_api_base_mismatch_compares_origins() {
        let api = Url::parse("http://localhost:8000").unwrap();
        assert_eq!(baked_api_base_mismatch(None, &api), None);
        assert_eq!(baked_api_base_mismatch(Some("http://localhost:8000/"), &api), None);
        assert_eq!(
            baked_api_base_mismatch(Some("https://pidash.example.com"), &api),
            Some("https://pidash.example.com")
        );
    }

    fn server() -> Url {
        Url::parse("https://pidash.example.com").unwrap()
    }
    fn root() -> Url {
        Url::parse("tauri://localhost").unwrap()
    }

    fn web() -> Url {
        Url::parse("https://pidash.airepublic.com").unwrap()
    }
    fn new_window(url: &str, bundle_root: &str) -> Option<String> {
        let root = Url::parse(bundle_root).unwrap();
        browser_url_for_new_window(&Url::parse(url).unwrap(), Some(&root), &web()).map(String::from)
    }

    #[test]
    fn open_in_new_tab_on_an_in_app_path_goes_to_the_hosted_web_app() {
        // `window.open("/acme/browse/PROJ-12/", "_blank")` as the webview
        // resolves it on macOS/Linux and on Windows.
        for (url, bundle_root) in [
            ("tauri://localhost/acme/browse/PROJ-12/", "tauri://localhost"),
            ("http://tauri.localhost/acme/browse/PROJ-12/", "http://tauri.localhost"),
        ] {
            assert_eq!(
                new_window(url, bundle_root).as_deref(),
                Some("https://pidash.airepublic.com/acme/browse/PROJ-12/"),
                "{url}"
            );
        }
    }

    #[test]
    fn new_window_in_app_path_keeps_query_and_fragment() {
        assert_eq!(
            new_window("tauri://localhost/acme/projects/p1/cycles/c1?peek=1#comment", "tauri://localhost").as_deref(),
            Some("https://pidash.airepublic.com/acme/projects/p1/cycles/c1?peek=1#comment")
        );
    }

    #[test]
    fn new_window_external_links_open_unchanged() {
        for u in ["https://example.com/docs?a=1#b", "http://intranet.example/wiki"] {
            assert_eq!(new_window(u, "tauri://localhost").as_deref(), Some(u), "{u}");
            assert_eq!(new_window(u, "http://tauri.localhost").as_deref(), Some(u), "{u}");
        }
    }

    #[test]
    fn new_window_refuses_non_web_schemes() {
        for u in ["file:///etc/passwd", "javascript:alert(1)", "mailto:sales@example.com", "tauri://elsewhere/x"] {
            assert_eq!(new_window(u, "tauri://localhost"), None, "{u}");
        }
    }

    #[test]
    fn new_window_in_hot_reload_mode_opens_the_server_url_as_is() {
        // No bundle: the webview is already on the server host.
        let url = Url::parse("http://localhost:3000/acme/browse/PROJ-12/").unwrap();
        assert_eq!(browser_url_for_new_window(&url, None, &web()), Some(url));
    }

    #[test]
    fn new_window_in_app_path_is_refused_when_the_web_base_is_not_http() {
        let root = Url::parse("tauri://localhost").unwrap();
        let url = Url::parse("tauri://localhost/acme/").unwrap();
        assert_eq!(browser_url_for_new_window(&url, Some(&root), &root), None);
    }

    #[test]
    fn api_paths_on_server_host_pass_through() {
        let url = Url::parse("https://pidash.example.com/api/auth/desktop-exchange/?code=x&state=y").unwrap();
        assert_eq!(bundle_redirect_for(&url, &server(), &root()), None);
    }

    // The session sign-out view lives outside `/api/`, so a form POST to it
    // is bounced like any other server-host page and never reaches the
    // server. The web layer signs out over the native API transport instead.
    #[test]
    fn sign_out_form_navigation_bounces_to_bundle() {
        let url = Url::parse("https://pidash.example.com/auth/sign-out/").unwrap();
        let target = bundle_redirect_for(&url, &server(), &root()).unwrap();
        assert_eq!(target.as_str(), "tauri://localhost/auth/sign-out/");
    }

    #[test]
    fn bundle_and_foreign_origins_pass_through() {
        for u in ["tauri://localhost/", "https://login.example.com/x", "http://pidash.example.com/"] {
            let url = Url::parse(u).unwrap();
            assert_eq!(bundle_redirect_for(&url, &server(), &root()), None, "{u}");
        }
    }

    #[test]
    fn bundle_root_is_the_dev_server_only_when_tauri_serves_from_one() {
        let dev_url = Url::parse("http://127.0.0.1:1430/").unwrap();
        assert_eq!(bundle_root_for(Some(&dev_url)), dev_url);
        assert_eq!(bundle_root_for(None).as_str().trim_end_matches('/'), BUNDLE_ORIGIN);
    }

    #[test]
    fn server_root_bounces_to_bundle_root() {
        let url = Url::parse("https://pidash.example.com/").unwrap();
        let target = bundle_redirect_for(&url, &server(), &root()).unwrap();
        assert_eq!(target.as_str(), "tauri://localhost/");
    }

    #[test]
    fn server_app_route_keeps_path_and_query() {
        let url = Url::parse("https://pidash.example.com/acme/projects/?x=1").unwrap();
        let target = bundle_redirect_for(&url, &server(), &root()).unwrap();
        assert_eq!(target.as_str(), "tauri://localhost/acme/projects/?x=1");
    }

    #[test]
    fn exchange_failure_redirect_lands_on_bundle_sign_in() {
        let url = Url::parse("https://pidash.example.com/sign-in?error=state_flow_mismatch").unwrap();
        let target = bundle_redirect_for(&url, &server(), &root()).unwrap();
        assert_eq!(target.as_str(), "tauri://localhost/sign-in?error=state_flow_mismatch");
    }

    #[test]
    fn next_path_from_session_expiry_passes_through_unchanged() {
        let url = Url::parse("https://pidash.example.com/?next_path=%2Facme%2F").unwrap();
        let target = bundle_redirect_for(&url, &server(), &root()).unwrap();
        assert_eq!(target.as_str(), "tauri://localhost/?next_path=%2Facme%2F");
    }

    fn config() -> AppConfig {
        AppConfig { target_url: server(), bundle_root: Some(root()), web_base: web() }
    }

    const CALLBACK: &str = "pidash://auth/callback?code=C&state=S";

    #[test]
    fn auth_callback_is_ignored_when_this_app_started_no_sign_in() {
        // A link minted from someone else's sign-in, opened on this machine.
        let sign_in = SignInState::default();
        assert_eq!(
            deep_link_target(CALLBACK, &config(), &sign_in, Instant::now()),
            Err("no sign-in was started from this app")
        );
        assert_eq!(
            deep_link_target("pidash://auth/callback?error=access_denied", &config(), &sign_in, Instant::now()),
            Err("no sign-in was started from this app")
        );
    }

    #[test]
    fn auth_callback_carries_the_verifier_of_the_sign_in_this_app_started() {
        let sign_in = SignInState::default();
        let now = Instant::now();
        let challenge = sign_in.begin(now).unwrap();
        let target = deep_link_target(CALLBACK, &config(), &sign_in, now).unwrap();
        assert_eq!(target.origin(), server().origin());
        assert_eq!(target.path(), "/api/auth/desktop-exchange/");
        let query: std::collections::HashMap<_, _> = target.query_pairs().into_owned().collect();
        assert_eq!(query["code"], "C");
        assert_eq!(query["state"], "S");
        assert_eq!(sign_in_challenge(&query["desktop_verifier"]), challenge);
        assert_eq!(query.len(), 3);
    }

    #[test]
    fn a_started_sign_in_honours_one_callback_only() {
        let sign_in = SignInState::default();
        let now = Instant::now();
        sign_in.begin(now).unwrap();
        assert!(deep_link_target(CALLBACK, &config(), &sign_in, now).is_ok());
        assert_eq!(
            deep_link_target(CALLBACK, &config(), &sign_in, now),
            Err("no sign-in was started from this app")
        );
    }

    #[test]
    fn a_started_sign_in_expires() {
        let sign_in = SignInState::default();
        let now = Instant::now();
        sign_in.begin(now).unwrap();
        assert_eq!(
            deep_link_target(CALLBACK, &config(), &sign_in, now + SIGN_IN_TTL),
            Err("no sign-in was started from this app")
        );
    }

    #[test]
    fn unrecognized_deep_links_do_not_spend_the_pending_sign_in() {
        let sign_in = SignInState::default();
        let now = Instant::now();
        sign_in.begin(now).unwrap();
        for url in ["not a url", "pidash://other/callback?code=C&state=S", "pidash://auth/elsewhere"] {
            assert!(deep_link_target(url, &config(), &sign_in, now).is_err(), "{url}");
        }
        assert!(deep_link_target(CALLBACK, &config(), &sign_in, now).is_ok());
    }

    #[test]
    fn provider_error_callback_lands_on_bundle_sign_in_and_ends_the_sign_in() {
        let sign_in = SignInState::default();
        let now = Instant::now();
        sign_in.begin(now).unwrap();
        let target = deep_link_target(
            "pidash://auth/callback?error=access_denied&error_description=User%20cancelled",
            &config(),
            &sign_in,
            now,
        )
        .unwrap();
        assert_eq!(
            target.as_str(),
            "tauri://localhost/sign-in?error=access_denied&error_description=User+cancelled"
        );
        assert!(deep_link_target(CALLBACK, &config(), &sign_in, now).is_err());
    }

    #[test]
    fn restarting_a_pending_sign_in_keeps_its_challenge() {
        let sign_in = SignInState::default();
        let now = Instant::now();
        let first = sign_in.begin(now).unwrap();
        assert_eq!(sign_in.begin(now + Duration::from_secs(60)).unwrap(), first);
        // The restart also restarts the clock.
        assert!(sign_in.take(now + SIGN_IN_TTL).is_some());
        // Once spent or expired, the next sign-in gets a fresh secret.
        assert_ne!(sign_in.begin(now).unwrap(), first);
        assert_ne!(sign_in.begin(now + SIGN_IN_TTL).unwrap(), sign_in.begin(now + SIGN_IN_TTL * 3).unwrap());
    }

    #[test]
    fn sign_in_challenge_is_the_s256_of_the_verifier() {
        // RFC 7636 appendix B.
        assert_eq!(
            sign_in_challenge("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
    }

    #[test]
    fn sign_in_url_gets_the_challenge() {
        let url = Url::parse("https://pidash.example.com/api/auth/sign-in/?return_to=%2Facme%2F&desktop=1&desktop_challenge=stale").unwrap();
        let bound = sign_in_url_with_challenge(&url, "abc");
        let query: Vec<_> = bound.query_pairs().into_owned().collect();
        assert_eq!(
            query,
            [
                ("return_to".to_string(), "/acme/".to_string()),
                ("desktop".to_string(), "1".to_string()),
                ("desktop_challenge".to_string(), "abc".to_string()),
            ]
        );
    }

    #[test]
    fn deep_link_error_url_is_bundle_sign_in() {
        assert_eq!(bundle_url(&root(), "/sign-in").as_str(), "tauri://localhost/sign-in");
    }

    #[test]
    fn marketing_routes_bounce_too() {
        for p in ["/home", "/docs", "/pricing"] {
            let url = Url::parse(&format!("https://pidash.example.com{p}")).unwrap();
            assert!(bundle_redirect_for(&url, &server(), &root()).is_some(), "{p}");
        }
    }
}
