// Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

fn main() {
    println!("cargo:rerun-if-env-changed=PI_DASH_URL");
    println!("cargo:rerun-if-env-changed=VITE_API_BASE_URL");
    println!("cargo:rerun-if-env-changed=VITE_WEB_BASE_URL");
    println!("cargo:rerun-if-env-changed=PI_DASH_OSS_SHA");
    println!("cargo:rerun-if-env-changed=PIDASH_DESKTOP_EXTERNAL_SIGNIN");
    // PIDASH_DESKTOP_HOT_RELOAD is read by main.rs via option_env! at
    // compile time, not by build.rs. Re-declaring it here makes Cargo
    // invalidate the crate (not just build.rs) when the env toggles, so
    // switching between bundled and hot-reload modes between `cargo tauri
    // dev` invocations forces main.rs to recompile.
    println!("cargo:rerun-if-env-changed=PIDASH_DESKTOP_HOT_RELOAD");
    // Same idea for the dist/ validity signal below — let main.rs
    // recompile when the embedded flag flips.
    println!("cargo:rerun-if-changed=dist/index.html");

    if let Ok(url) = std::env::var("PI_DASH_URL")
        && url::Url::parse(&url).is_err()
    {
        panic!("PI_DASH_URL is set but is not a valid absolute URL: {url}");
    }

    // Embed a yes/no flag for main.rs: is dist/ a real SPA bundle, or
    // still the 118-byte placeholder? In debug profile the guard below
    // doesn't panic (rust-analyzer / `cargo test` would break on fresh
    // clones), but main.rs uses this flag to print a loud startup warning
    // when bundled mode is selected against an invalid dist/. That covers
    // the `cargo build` / `cargo run` / IDE-debug-run path that bypasses
    // tauri.conf's beforeDevCommand entirely.
    let dist_valid = dist_index_looks_real();
    println!(
        "cargo:rustc-env=PIDASH_DIST_VALID={}",
        if dist_valid { "1" } else { "0" }
    );

    // Release builds load UI from desktop/src-tauri/dist/ via
    // WebviewUrl::App. The repo ships an 118-byte placeholder index.html
    // (so Tauri's frontendDist config target exists at compile time); a
    // real bundle replaces it — in CI via the download-artifact step, and
    // locally via the beforeDevCommand / beforeBuildCommand-invoked
    // scripts/dev-prep.sh.
    //
    // Hard panic only in release. Debug builds rely on the runtime
    // warning in main.rs so `cargo check`/`cargo test`/rust-analyzer
    // don't break on a fresh clone where dist/ is still the placeholder.
    if std::env::var("PROFILE").as_deref() == Ok("release") {
        let dist = std::path::PathBuf::from("dist");
        let index = dist.join("index.html");
        let meta = match std::fs::metadata(&index) {
            Ok(m) => m,
            Err(e) => panic!(
                "Release build but desktop/src-tauri/dist/index.html cannot be read: {e}. \
                Run desktop/scripts/dev-prep.sh to populate dist/ (release pipelines may \
                pre-populate it and set PIDASH_SKIP_DEV_PREP=1)."
            ),
        };
        if meta.len() < 4096 {
            panic!(
                "Release build but desktop/src-tauri/dist/index.html is only {} bytes \
                (looks like the 118-byte placeholder). Run desktop/scripts/dev-prep.sh \
                to populate dist/.",
                meta.len()
            );
        }
        let assets = dist.join("assets");
        let assets_ok = std::fs::read_dir(&assets)
            .map(|mut d| d.next().is_some())
            .unwrap_or(false);
        if !assets_ok {
            panic!(
                "Release build but desktop/src-tauri/dist/assets/ is missing or empty. \
                Run desktop/scripts/dev-prep.sh to populate it (must include hashed JS/CSS chunks)."
            );
        }
        validate_baked_api_base(&dist);
        validate_baked_web_base(&dist);
        // Editions whose sign-in hands off to the system browser (an OIDC
        // provider that must not run inside the webview) opt into checking
        // that the bundled sign-in screen actually does so.
        if std::env::var("PIDASH_DESKTOP_EXTERNAL_SIGNIN").as_deref() == Ok("1") {
            validate_desktop_login_handoff(&dist);
        }
    }

    tauri_build::build()
}

fn validate_baked_api_base(dist: &std::path::Path) {
    let expected = std::env::var("VITE_API_BASE_URL").unwrap_or_default();
    if expected.trim().is_empty() {
        panic!(
            "Release build but VITE_API_BASE_URL is unset. The bundled desktop SPA \
            would fall back to the webview origin for API calls."
        );
    }

    let mut saw_expected = false;
    scan_text_assets(dist, &expected, &mut saw_expected);

    if !saw_expected {
        panic!(
            "Release build but desktop/src-tauri/dist does not contain VITE_API_BASE_URL={expected:?}. \
            The SPA bundle may have been baked with the wrong backend URL or a stale artifact."
        );
    }
}

/// The public web origin the SPA builds shareable links from.
///
/// Bundled pages run on a Tauri-owned origin, so anything that resolves a
/// shareable URL against `window.location.origin` would hand the user a
/// `tauri://localhost` link. `desktop-overlay`'s `desktopWebUrl` resolves
/// against `VITE_WEB_BASE_URL` instead, which only works if the value was
/// actually baked into the bundle.
///
/// Falls back to `PI_DASH_URL`, which `dev-prep.sh` uses as the default and
/// which `main.rs` requires via `env!` in release -- so in a release build
/// this is never empty, and the panic below is reachable only from a
/// hand-managed `dist/`.
fn validate_baked_web_base(dist: &std::path::Path) {
    let expected = std::env::var("VITE_WEB_BASE_URL")
        .or_else(|_| std::env::var("PI_DASH_URL"))
        .unwrap_or_default();
    if expected.trim().is_empty() {
        panic!(
            "Release build but neither VITE_WEB_BASE_URL nor PI_DASH_URL is set. The bundled \
             desktop SPA could expose its Tauri origin in copied links."
        );
    }

    let mut saw_expected = false;
    scan_text_assets(dist, &expected, &mut saw_expected);

    if !saw_expected {
        panic!(
            "Release build but desktop/src-tauri/dist does not contain the configured web \
             origin {expected:?}. The SPA bundle may have a stale or missing VITE_WEB_BASE_URL."
        );
    }
}

fn validate_desktop_login_handoff(dist: &std::path::Path) {
    let mut home_signin_chunks = Vec::new();
    collect_home_signin_chunks(dist, &mut home_signin_chunks);

    if home_signin_chunks.is_empty() {
        panic!(
            "Release build with PIDASH_DESKTOP_EXTERNAL_SIGNIN=1 but desktop/src-tauri/dist does \
            not contain the home sign-in route. The frontend overlays may not have been applied."
        );
    }

    let has_browser_handoff = home_signin_chunks
        .iter()
        .any(|contents| contents.contains("open_in_browser") && contents.contains("desktop"));
    if !has_browser_handoff {
        panic!(
            "Release build but the desktop home sign-in route does not invoke open_in_browser. \
            The sign-in would run inside the Tauri webview instead of the system browser."
        );
    }
}

fn collect_home_signin_chunks(dir: &std::path::Path, chunks: &mut Vec<String>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_home_signin_chunks(&path, chunks);
            continue;
        }
        if !matches!(
            path.extension().and_then(|ext| ext.to_str()),
            Some("js" | "mjs")
        ) {
            continue;
        }
        let Ok(contents) = std::fs::read_to_string(&path) else {
            continue;
        };
        if contents.contains("Sign in to Pi Dash") {
            chunks.push(contents);
        }
    }
}

fn scan_text_assets(dir: &std::path::Path, expected: &str, saw_expected: &mut bool) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            scan_text_assets(&path, expected, saw_expected);
            continue;
        }
        if !is_text_asset(&path) {
            continue;
        }
        let Ok(contents) = std::fs::read_to_string(&path) else {
            continue;
        };
        if contents.contains(expected) {
            *saw_expected = true;
        }
    }
}

fn is_text_asset(path: &std::path::Path) -> bool {
    matches!(
        path.extension().and_then(|ext| ext.to_str()),
        Some("css" | "html" | "js" | "json" | "mjs" | "txt")
    )
}

/// Shared validity check used by both the release-build guard (panic) and
/// the debug-build runtime-warning flag (PIDASH_DIST_VALID). Mirrors the
/// criteria in dev-prep.sh so the three layers agree on what counts as a
/// "real" SPA bundle.
fn dist_index_looks_real() -> bool {
    let dist = std::path::PathBuf::from("dist");
    let index = dist.join("index.html");
    let Ok(meta) = std::fs::metadata(&index) else {
        return false;
    };
    if meta.len() < 4096 {
        return false;
    }
    let assets = dist.join("assets");
    std::fs::read_dir(&assets)
        .map(|mut d| d.next().is_some())
        .unwrap_or(false)
}
