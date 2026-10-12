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
}
