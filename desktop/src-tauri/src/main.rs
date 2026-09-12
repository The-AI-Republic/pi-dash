// Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod managed_runner;
mod pidash_cli;

use std::sync::Mutex;

use tauri::{
    Manager, Url, WebviewUrl, WebviewWindowBuilder, WindowEvent,
    menu::{MenuBuilder, MenuItemBuilder, SubmenuBuilder},
    tray::{MouseButton, TrayIconBuilder, TrayIconEvent},
};
use tauri_plugin_deep_link::DeepLinkExt;
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};
use tauri_plugin_opener::OpenerExt;
use tauri_plugin_updater::UpdaterExt;

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
}

/// Where Tauri serves `WebviewUrl::App` from. Windows is plain `http`
/// because `app.windows[].useHttpsScheme` defaults to false and nothing
/// here opts in — the API server's CORS/CSRF allowlist must list this
/// exact origin (scheme included) or every credentialed XHR is refused.
#[cfg(windows)]
const BUNDLE_ORIGIN: &str = "http://tauri.localhost";
#[cfg(not(windows))]
const BUNDLE_ORIGIN: &str = "tauri://localhost";

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

/// Process an incoming `pidash://auth/callback?code=<>&state=<>` deep link.
///
/// The identity provider redirected the system browser directly to this
/// custom scheme — RFC 8252 native-app flow. The desktop is registered with
/// it as a separate client with `redirect_uri=pidash://auth/callback`, so we
/// receive the standard authorization `code` + `state`. We forward both to
/// the server's `/api/auth/desktop-exchange/` by navigating the webview
/// there; the server looks up the PKCE verifier by `state`, completes the
/// token exchange, establishes the desktop session, and sets cookies on the
/// webview's response. The cookie jar inside the app gets populated without
/// ever putting a session token in a URL.
fn handle_deep_link(app: &tauri::AppHandle, url: &str) {
    let Ok(parsed) = Url::parse(url) else {
        eprintln!("deep-link: invalid URL: {url}");
        return;
    };
    if parsed.scheme() != "pidash"
        || parsed.host_str() != Some("auth")
        || parsed.path() != "/callback"
    {
        eprintln!("deep-link: ignoring unrecognized URL: {url}");
        return;
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
    let Some(window) = app.get_webview_window("main") else {
        eprintln!("deep-link: main window not found");
        return;
    };
    show_main_window(app);
    let config = app.state::<AppConfig>();

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
        if let Err(e) = window.navigate(error_url) {
            eprintln!("deep-link: navigate failed: {e}");
        }
        return;
    }

    let (Some(code), Some(state_token)) = (code, state_token) else {
        eprintln!("deep-link: missing code or state param");
        return;
    };
    let mut session_url = config.target_url.clone();
    session_url.set_path("/api/auth/desktop-exchange/");
    session_url
        .query_pairs_mut()
        .clear()
        .append_pair("code", &code)
        .append_pair("state", &state_token);
    if let Err(e) = window.navigate(session_url) {
        eprintln!("deep-link: navigate failed: {e}");
    }
}

/// Check the updater endpoint at startup. If a newer version is available,
/// ask the user via a native dialog; install + restart on confirmation.
///
/// All failure modes (network down, malformed manifest, signature mismatch)
/// log to stderr and return silently — the app keeps running on the current
/// version. We don't want a flaky update server to gate launch.
///
/// Only runs when the build configures `plugins.updater` (see `main`).
async fn check_for_updates(handle: tauri::AppHandle) {
    let updater = match handle.updater() {
        Ok(u) => u,
        Err(e) => {
            eprintln!("updater: construction failed: {e}");
            return;
        }
    };
    let update = match updater.check().await {
        Ok(Some(u)) => u,
        Ok(None) => return,
        Err(e) => {
            eprintln!("updater: check failed: {e}");
            return;
        }
    };
    let current = handle.package_info().version.to_string();
    let install = handle
        .dialog()
        .message(format!(
            "Pi Dash {new} is available (you have {current}).\n\n\
             Install now? The app will restart automatically.",
            new = update.version,
            current = current,
        ))
        .title("Update available")
        .kind(MessageDialogKind::Info)
        .buttons(MessageDialogButtons::OkCancelCustom(
            "Install".into(),
            "Later".into(),
        ))
        .blocking_show();
    if !install {
        return;
    }
    if let Err(e) = update
        .download_and_install(|_chunk, _total| {}, || {})
        .await
    {
        eprintln!("updater: download/install failed: {e}");
        // The user explicitly opted in to the install — staying silent on
        // failure leaves them wondering whether anything happened. Surface
        // the error in a dialog so they know they're still on the old
        // version and can try again next launch.
        handle
            .dialog()
            .message(format!(
                "The update couldn't be installed.\n\n{e}\n\nYou can try again next launch."
            ))
            .title("Pi Dash update failed")
            .kind(MessageDialogKind::Error)
            .show(|_| {});
        return;
    }
    handle.restart();
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
    let initial_webview = if hot_reload {
        WebviewUrl::External(target_url.clone())
    } else {
        WebviewUrl::App("index.html".into())
    };
    let bundle_root: Option<Url> = if hot_reload {
        None
    } else {
        Some(Url::parse(BUNDLE_ORIGIN)?)
    };

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
        .manage(AppConfig { target_url, bundle_root })
        .manage(managed_runner::DaemonState::default())
        .invoke_handler(tauri::generate_handler![
            open_in_browser,
            pidash_cli::detect_pidash_cli,
            pidash_cli::install_pidash_cli,
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
            managed_runner::managed_doctor,
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
            let mut window = WebviewWindowBuilder::new(app, "main", initial_webview)
                .title("Pi Dash")
                .inner_size(1400.0, 900.0)
                .min_inner_size(800.0, 600.0)
                .resizable(true);
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
                        if let Some(w) = app.get_webview_window("main") {
                            if let Err(e) = w.navigate(target) {
                                eprintln!("nav: bounce failed: {e}");
                            }
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
            #[cfg(any(target_os = "linux", target_os = "windows"))]
            {
                if let Err(e) = app.deep_link().register("pidash") {
                    eprintln!("deep-link: register failed: {e}");
                }
            }
            let handle = app.handle().clone();
            app.deep_link().on_open_url(move |event| {
                for url in event.urls() {
                    handle_deep_link(&handle, url.as_str());
                }
            });

            // Background updater check. Spawned (not awaited) so launch isn't
            // blocked by network — if a new version is available the dialog
            // appears moments after the window opens.
            if updater_enabled {
                let update_handle = app.handle().clone();
                tauri::async_runtime::spawn(async move {
                    check_for_updates(update_handle).await;
                });
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

    fn server() -> Url {
        Url::parse("https://pidash.example.com").unwrap()
    }
    fn root() -> Url {
        Url::parse("tauri://localhost").unwrap()
    }

    #[test]
    fn api_paths_on_server_host_pass_through() {
        let url = Url::parse("https://pidash.example.com/api/auth/desktop-exchange/?code=x&state=y").unwrap();
        assert_eq!(bundle_redirect_for(&url, &server(), &root()), None);
    }

    #[test]
    fn bundle_and_foreign_origins_pass_through() {
        for u in ["tauri://localhost/", "https://login.example.com/x", "http://pidash.example.com/"] {
            let url = Url::parse(u).unwrap();
            assert_eq!(bundle_redirect_for(&url, &server(), &root()), None, "{u}");
        }
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
