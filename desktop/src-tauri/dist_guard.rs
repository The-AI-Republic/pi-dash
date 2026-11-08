// Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Bundle scan behind build.rs's release guards.
//!
//! Lives outside build.rs so `cargo test` can exercise it: build.rs pulls it
//! in as a module, and main.rs does the same under `cfg(test)`.

/// The desktop platform marker: only `core/platform/tauri.ts` emits it, and
/// the build-time alias excludes that module from the web bundle (F-04).
/// dev-prep.sh greps for the same string in its web_new branch.
pub const DESKTOP_MARKER: &str = "desktop_api_request";

/// Legacy placeholder guard: the repo ships a 118-byte placeholder
/// `index.html`, and the overlay-merged apps/web bundle is always larger.
const LEGACY_MIN_INDEX_BYTES: u64 = 4096;

/// True when `dist/` was baked by dev-prep.sh's web_new branch, which records
/// `frontend=web_new` in `bake-info.txt`. A hand-managed dist/ without the
/// sidecar falls through to the legacy checks.
pub fn dist_is_web_new(dist: &std::path::Path) -> bool {
    std::fs::read_to_string(dist.join("bake-info.txt"))
        .map(|info| info.lines().any(|l| l.trim() == "frontend=web_new"))
        .unwrap_or(false)
}

pub fn dist_assets_non_empty(dist: &std::path::Path) -> bool {
    std::fs::read_dir(dist.join("assets"))
        .map(|mut d| d.next().is_some())
        .unwrap_or(false)
}

/// web_new ships a tiny SPA shell (~2 KiB) that only bootstraps `assets/`,
/// so the legacy byte threshold cannot tell a good build from a broken one.
/// Instead require the shell to reference its asset bundle (dev-prep.sh's
/// web_new check).
pub fn web_new_index_references_assets(dist: &std::path::Path) -> bool {
    std::fs::read_to_string(dist.join("index.html"))
        .map(|html| html.contains("assets/"))
        .unwrap_or(false)
}

pub fn web_new_dist_has_desktop_marker(dist: &std::path::Path) -> bool {
    let mut saw_marker = false;
    scan_text_assets(dist, DESKTOP_MARKER, &mut saw_marker);
    saw_marker
}

/// Mirror of dev-prep.sh's web_new validation: the shell wires its bundle,
/// the bundle is non-empty, and it proves the desktop target.
pub fn web_new_dist_is_valid(dist: &std::path::Path) -> bool {
    web_new_index_references_assets(dist)
        && dist_assets_non_empty(dist)
        && web_new_dist_has_desktop_marker(dist)
}

/// Shared validity check used by both the release-build guard (panic) and
/// the debug-build runtime-warning flag (PIDASH_DIST_VALID). Mirrors the
/// criteria in dev-prep.sh so the three layers agree on what counts as a
/// "real" SPA bundle.
pub fn dist_index_looks_real(dist: &std::path::Path) -> bool {
    if dist_is_web_new(dist) {
        return web_new_dist_is_valid(dist);
    }
    let index = dist.join("index.html");
    let Ok(meta) = std::fs::metadata(&index) else {
        return false;
    };
    if meta.len() < LEGACY_MIN_INDEX_BYTES {
        return false;
    }
    dist_assets_non_empty(dist)
}

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

    fn write_web_new_dist(dist: &std::path::Path, index: &str, app_js: &str, bake_info: &str) {
        let assets = dist.join("assets");
        std::fs::create_dir_all(&assets).unwrap();
        std::fs::write(dist.join("index.html"), index).unwrap();
        std::fs::write(assets.join("app.js"), app_js).unwrap();
        std::fs::write(dist.join("bake-info.txt"), bake_info).unwrap();
    }

    #[test]
    fn web_new_dist_is_detected_from_bake_info() {
        let dist = tempfile::tempdir().unwrap();
        write_web_new_dist(
            dist.path(),
            "<script src=\"assets/app.js\"></script>",
            "// desktop_api_request",
            "frontend=web_new\napi_base=http://localhost:8000\n",
        );
        assert!(dist_is_web_new(dist.path()));
        assert!(dist_index_looks_real(dist.path()));
    }

    #[test]
    fn web_new_dist_without_desktop_marker_is_rejected() {
        let dist = tempfile::tempdir().unwrap();
        write_web_new_dist(
            dist.path(),
            "<script src=\"assets/app.js\"></script>",
            "// plain web bundle, no desktop code",
            "frontend=web_new\n",
        );
        assert!(dist_is_web_new(dist.path()));
        assert!(!dist_index_looks_real(dist.path()));
    }

    #[test]
    fn web_new_dist_with_unwired_shell_is_rejected() {
        let dist = tempfile::tempdir().unwrap();
        write_web_new_dist(
            dist.path(),
            "<!-- no bundle wired up -->",
            "// desktop_api_request",
            "frontend=web_new\n",
        );
        assert!(!dist_index_looks_real(dist.path()));
    }

    #[test]
    fn legacy_dist_without_bake_info_uses_size_guard() {
        let dist = tempfile::tempdir().unwrap();
        let assets = dist.path().join("assets");
        std::fs::create_dir(&assets).unwrap();
        std::fs::write(assets.join("app.js"), "// app").unwrap();
        std::fs::write(dist.path().join("index.html"), "x".repeat(8192)).unwrap();
        assert!(!dist_is_web_new(dist.path()));
        assert!(dist_index_looks_real(dist.path()));

        std::fs::write(dist.path().join("index.html"), "x".repeat(118)).unwrap();
        assert!(!dist_index_looks_real(dist.path()));
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
}
