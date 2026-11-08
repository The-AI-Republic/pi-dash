// Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

mod dist_guard;

use dist_guard::{check_dist_bundle, check_login_handoff, scan_text_assets};

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
    let dist_valid = check_dist_bundle(std::path::Path::new("dist")).is_ok();
    println!(
        "cargo:rustc-env=PIDASH_DIST_VALID={}",
        if dist_valid { "1" } else { "0" }
    );

    // Hand main.rs the API base dev-prep.sh recorded for this dist/, so it
    // can warn at startup when the binary resolves a different one. Absent
    // for a hand-managed dist/ without the sidecar.
    if let Ok(info) = std::fs::read_to_string("dist/bake-info.txt") {
        println!("cargo:rerun-if-changed=dist/bake-info.txt");
        if let Some(api_base) = info.lines().find_map(|l| l.strip_prefix("api_base=")) {
            println!("cargo:rustc-env=PIDASH_BAKED_API_BASE={api_base}");
        }
    }

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
        if let Err(problem) = check_dist_bundle(&dist) {
            panic!(
                "Release build but {problem}. Run desktop/scripts/dev-prep.sh to populate \
                dist/ (release pipelines may pre-populate it and set PIDASH_SKIP_DEV_PREP=1)."
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
    if let Err(problem) = check_login_handoff(dist) {
        panic!("Release build with PIDASH_DESKTOP_EXTERNAL_SIGNIN=1 but {problem}.");
    }
}
