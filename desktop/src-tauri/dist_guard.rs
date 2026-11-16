// Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Bundle scan behind build.rs's release guards.
//!
//! Lives outside build.rs so `cargo test` can exercise it: build.rs pulls it
//! in as a module, and main.rs does the same under `cfg(test)`.

pub fn scan_text_assets(dir: &std::path::Path, expected: &str, saw_expected: &mut bool) {
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

// Same extensions dev-prep.sh and the release pipeline grep. `txt` is
// deliberately absent: dist/bake-info.txt is written by the build itself and
// carries the expected URLs verbatim, so counting it would let the guard pass
// against a bundle that was never baked with them.
fn is_text_asset(path: &std::path::Path) -> bool {
    matches!(
        path.extension().and_then(|ext| ext.to_str()),
        Some("css" | "html" | "js" | "json" | "mjs")
    )
}

/// Why `dist/` is not a real SPA bundle, or `Ok` when it is.
///
/// Shared by build.rs's release guard (panic) and its debug-build
/// `PIDASH_DIST_VALID` flag, so both agree with dev-prep.sh on what counts as
/// a real bundle rather than the committed placeholder.
pub fn check_dist_bundle(dist: &std::path::Path) -> Result<(), String> {
    let index = dist.join("index.html");
    let shell = std::fs::read_to_string(&index)
        .map_err(|e| format!("desktop/src-tauri/dist/index.html cannot be read: {e}"))?;
    if !dir_has_entries(&dist.join("assets")) {
        return Err(
            "desktop/src-tauri/dist/assets/ is missing or empty (must include hashed JS/CSS chunks)"
                .to_string(),
        );
    }
    // Legacy apps/web: its server-rendered shell is never under 4 KB.
    if shell.len() >= LEGACY_INDEX_MIN_BYTES {
        return Ok(());
    }
    // apps/web_new ships a ~2 KB shell that only bootstraps assets/, so size
    // can't tell it from the placeholder. Use dev-prep.sh's criteria instead:
    // the shell loads assets/ and the bundle carries the desktop platform
    // marker (only the PIDASH_TARGET=desktop build emits it).
    let mut has_desktop_marker = false;
    scan_text_assets(
        &dist.join("assets"),
        DESKTOP_PLATFORM_MARKER,
        &mut has_desktop_marker,
    );
    if shell.contains("assets/") && has_desktop_marker {
        return Ok(());
    }
    Err(format!(
        "desktop/src-tauri/dist/index.html is only {} bytes and is not a web_new desktop shell \
        (needs an assets/ reference and the {DESKTOP_PLATFORM_MARKER:?} marker in assets/) \
        -- looks like the placeholder",
        shell.len()
    ))
}

/// Smallest index.html the legacy apps/web build produces.
const LEGACY_INDEX_MIN_BYTES: usize = 4096;

/// Tauri command only apps/web_new's desktop platform layer invokes; the
/// same marker dev-prep.sh greps for.
const DESKTOP_PLATFORM_MARKER: &str = "desktop_api_request";

fn dir_has_entries(dir: &std::path::Path) -> bool {
    std::fs::read_dir(dir)
        .map(|mut d| d.next().is_some())
        .unwrap_or(false)
}

/// Why the bundled sign-in would not hand off to the system browser, or
/// `Ok` when it does. Checked for editions whose sign-in (an OIDC provider)
/// must not run inside the webview.
///
/// The legacy apps/web tree calls `open_in_browser` from the sign-in route's
/// own chunk. apps/web_new code-splits: the sign-in card is a lazy chunk and
/// the native call lives in the platform layer's `openSystemBrowser`, which
/// Vite puts in the entry chunk, so the two are checked separately.
pub fn check_login_handoff(dist: &std::path::Path) -> Result<(), String> {
    let mut chunks = Vec::new();
    collect_js_chunks(dist, &mut chunks);

    let signin_chunks: Vec<&String> = chunks
        .iter()
        .filter(|contents| contents.contains(SIGNIN_ROUTE_MARKER))
        .collect();
    if signin_chunks.is_empty() {
        return Err(
            "desktop/src-tauri/dist does not contain the home sign-in route \
            (the frontend overlays may not have been applied)"
                .to_string(),
        );
    }

    let legacy_handoff = signin_chunks
        .iter()
        .any(|contents| contents.contains(NATIVE_OPENER) && contents.contains("desktop"));
    let web_new_handoff = chunks.iter().any(|contents| {
        contents.contains(WEB_NEW_SIGNIN_OPENER) && contents.contains(NATIVE_OPENER)
    });
    if legacy_handoff || web_new_handoff {
        return Ok(());
    }
    Err(format!(
        "the desktop sign-in does not invoke {NATIVE_OPENER} (checked the sign-in route's chunk \
        and the platform layer's {WEB_NEW_SIGNIN_OPENER}); the sign-in would run inside the \
        Tauri webview instead of the system browser"
    ))
}

/// Heading of the sign-in screen in both frontends.
const SIGNIN_ROUTE_MARKER: &str = "Sign in to Pi Dash";

/// Native Tauri command that opens the system browser (main.rs).
const NATIVE_OPENER: &str = "open_in_browser";

/// apps/web_new platform method the cloud sign-in hands off through
/// (core/platform/tauri.ts); minification keeps object property names.
const WEB_NEW_SIGNIN_OPENER: &str = "openSystemBrowser";

fn collect_js_chunks(dir: &std::path::Path, chunks: &mut Vec<String>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_js_chunks(&path, chunks);
            continue;
        }
        if !matches!(
            path.extension().and_then(|ext| ext.to_str()),
            Some("js" | "mjs")
        ) {
            continue;
        }
        if let Ok(contents) = std::fs::read_to_string(&path) {
            chunks.push(contents);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const API_BASE: &str = "https://api.example.test";

    fn dist_contains(dist: &std::path::Path, expected: &str) -> bool {
        let mut saw_expected = false;
        scan_text_assets(dist, expected, &mut saw_expected);
        saw_expected
    }

    #[test]
    fn bake_info_sidecar_does_not_satisfy_the_scan() {
        let dist = tempfile::tempdir().unwrap();
        std::fs::write(dist.path().join("index.html"), "<html></html>").unwrap();
        std::fs::write(
            dist.path().join("bake-info.txt"),
            format!("api_base={API_BASE}\nweb_base={API_BASE}\n"),
        )
        .unwrap();

        assert!(!dist_contains(dist.path(), API_BASE));
    }

    #[test]
    fn url_baked_into_a_nested_chunk_satisfies_the_scan() {
        let dist = tempfile::tempdir().unwrap();
        let assets = dist.path().join("assets");
        std::fs::create_dir(&assets).unwrap();
        std::fs::write(dist.path().join("index.html"), "<html></html>").unwrap();
        std::fs::write(
            assets.join("constants-abc123.js"),
            format!("const a=\"{API_BASE}\";"),
        )
        .unwrap();

        assert!(dist_contains(dist.path(), API_BASE));
    }
    /// The shape `pnpm --filter web_new build:desktop` emits and dev-prep.sh
    /// accepts: a ~2 KB shell that bootstraps assets/, with the desktop
    /// platform marker in a chunk.
    fn write_web_new_dist(dist: &std::path::Path) {
        let assets = dist.join("assets");
        std::fs::create_dir(&assets).unwrap();
        std::fs::write(
            dist.join("index.html"),
            "<!doctype html><html><head>\
             <script type=\"module\" src=\"./assets/index-abc123.js\"></script>\
             </head><body><div id=\"root\"></div></body></html>",
        )
        .unwrap();
        std::fs::write(
            assets.join("index-abc123.js"),
            "core.invoke(\"desktop_api_request\",frame);",
        )
        .unwrap();
    }

    #[test]
    fn web_new_desktop_bundle_passes_the_dist_check() {
        let dist = tempfile::tempdir().unwrap();
        write_web_new_dist(dist.path());

        assert_eq!(check_dist_bundle(dist.path()), Ok(()));
    }

    #[test]
    fn small_shell_without_the_desktop_marker_fails_the_dist_check() {
        let dist = tempfile::tempdir().unwrap();
        write_web_new_dist(dist.path());
        // A web-target build: same shell, no Tauri platform code.
        std::fs::write(
            dist.path().join("assets/index-abc123.js"),
            "fetch(\"/api/\");",
        )
        .unwrap();

        assert!(check_dist_bundle(dist.path()).is_err());
    }

    #[test]
    fn small_shell_that_does_not_load_assets_fails_the_dist_check() {
        let dist = tempfile::tempdir().unwrap();
        write_web_new_dist(dist.path());
        std::fs::write(
            dist.path().join("index.html"),
            "<html><body>placeholder</body></html>",
        )
        .unwrap();

        assert!(check_dist_bundle(dist.path()).is_err());
    }

    #[test]
    fn missing_or_empty_assets_fails_the_dist_check() {
        let dist = tempfile::tempdir().unwrap();
        write_web_new_dist(dist.path());
        std::fs::remove_dir_all(dist.path().join("assets")).unwrap();
        // The marker check must not be satisfied by index.html alone.
        assert!(check_dist_bundle(dist.path()).is_err());

        std::fs::create_dir(dist.path().join("assets")).unwrap();
        assert!(check_dist_bundle(dist.path()).is_err());
    }

    #[test]
    fn placeholder_index_fails_the_dist_check() {
        let dist = tempfile::tempdir().unwrap();
        std::fs::write(dist.path().join("index.html"), "x".repeat(118)).unwrap();

        assert!(check_dist_bundle(dist.path()).is_err());
    }

    #[test]
    fn legacy_bundle_over_the_size_threshold_passes_the_dist_check() {
        let dist = tempfile::tempdir().unwrap();
        std::fs::create_dir(dist.path().join("assets")).unwrap();
        std::fs::write(dist.path().join("assets/root-abc.js"), "export{};").unwrap();
        std::fs::write(dist.path().join("index.html"), "x".repeat(4096)).unwrap();

        assert_eq!(check_dist_bundle(dist.path()), Ok(()));
    }
    fn write_chunk(dist: &std::path::Path, name: &str, contents: &str) {
        let assets = dist.join("assets");
        std::fs::create_dir_all(&assets).unwrap();
        std::fs::write(assets.join(name), contents).unwrap();
    }

    #[test]
    fn web_new_split_signin_with_native_opener_passes_the_handoff_check() {
        let dist = tempfile::tempdir().unwrap();
        write_chunk(
            dist.path(),
            "SignInCard-abc.js",
            "h(\"Sign in to Pi Dash\")",
        );
        write_chunk(
            dist.path(),
            "index-def.js",
            "openSystemBrowser:e=>t.invoke(\"open_in_browser\",{url:e})",
        );

        assert_eq!(check_login_handoff(dist.path()), Ok(()));
    }

    #[test]
    fn web_new_signin_through_the_opener_plugin_fails_the_handoff_check() {
        let dist = tempfile::tempdir().unwrap();
        write_chunk(
            dist.path(),
            "SignInCard-abc.js",
            "h(\"Sign in to Pi Dash\")",
        );
        // The pre-PIDESKAPP-183 shape: the plugin command the capabilities
        // never grant, while open_in_browser is used only for other links.
        write_chunk(
            dist.path(),
            "index-def.js",
            "openSystemBrowser:e=>t.invoke(\"plugin:opener|open_url\",{url:e})",
        );
        write_chunk(
            dist.path(),
            "links-ghi.js",
            "t.invoke(\"open_in_browser\",{url:e})",
        );

        assert!(check_login_handoff(dist.path()).is_err());
    }

    #[test]
    fn legacy_signin_chunk_with_native_opener_passes_the_handoff_check() {
        let dist = tempfile::tempdir().unwrap();
        write_chunk(
            dist.path(),
            "home-abc.js",
            "isDesktop&&invoke(\"open_in_browser\");\"Sign in to Pi Dash\";\"desktop\"",
        );

        assert_eq!(check_login_handoff(dist.path()), Ok(()));
    }

    #[test]
    fn bundle_without_the_signin_route_fails_the_handoff_check() {
        let dist = tempfile::tempdir().unwrap();
        write_chunk(
            dist.path(),
            "index-def.js",
            "openSystemBrowser:e=>t.invoke(\"open_in_browser\",{url:e})",
        );

        assert!(check_login_handoff(dist.path()).is_err());
    }
}
