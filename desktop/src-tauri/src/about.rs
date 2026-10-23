// Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! What the About dialog shows: the app version and the licence texts the
//! build stages into `licenses/` (see `licenses/README.md`).
//!
//! Those files are bundled as resources, and this is their only reader. The
//! web UI's About button beside the sidebar's user menu calls
//! [`desktop_about`] and renders what comes back.

use std::path::Path;

use serde::Serialize;
use tauri::{AppHandle, Manager};

/// A licence or notice file the build writes into `licenses/`.
struct NoticeFile {
    file: &'static str,
    /// What the text covers, as the About dialog titles it. Apache-2.0 §6
    /// grants no trademark rights, so the engine is not named after its
    /// upstream here; the notice file carries that attribution.
    component: &'static str,
    /// SPDX identifier.
    license: &'static str,
}

/// Every file `scripts/prepare-agent.sh` and the release pipelines write.
/// A file missing from this list ships in the bundle without ever being
/// shown — `tests::every_file_the_build_stages_is_rendered` guards that.
const NOTICE_FILES: &[NoticeFile] = &[
    NoticeFile {
        file: "pidash-LICENSE.txt",
        component: "Pi Dash",
        license: "AGPL-3.0-only",
    },
    NoticeFile {
        file: "agent-engine-LICENSE.txt",
        component: "Agent engine",
        license: "Apache-2.0",
    },
    NoticeFile {
        file: "agent-engine-NOTICE.txt",
        component: "Agent engine notice",
        license: "Apache-2.0",
    },
];

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BundledNotice {
    file: &'static str,
    component: &'static str,
    license: &'static str,
    /// `None` when this build did not stage the file (a plain `cargo build`
    /// without `prepare-agent.sh`). The dialog says so instead of hiding it.
    text: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct About {
    version: String,
    notices: Vec<BundledNotice>,
}

fn read_notices(resource_dir: &Path) -> Vec<BundledNotice> {
    let dir = resource_dir.join("licenses");
    NOTICE_FILES
        .iter()
        .map(|notice| BundledNotice {
            file: notice.file,
            component: notice.component,
            license: notice.license,
            text: match std::fs::read_to_string(dir.join(notice.file)) {
                Ok(text) => Some(text),
                Err(e) => {
                    eprintln!("about: {} is not readable: {e}", notice.file);
                    None
                }
            },
        })
        .collect()
}

/// The app version and the bundled licence texts, for the About dialog.
#[tauri::command]
pub fn desktop_about(app: AppHandle) -> Result<About, String> {
    let resource_dir = app
        .path()
        .resource_dir()
        .map_err(|e| format!("resource dir: {e}"))?;
    Ok(About {
        version: app.package_info().version.to_string(),
        notices: read_notices(&resource_dir),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    /// The file names a build script writes under `$license_dir/`.
    fn staged_files(script: &str) -> BTreeSet<String> {
        script
            .split("$license_dir/")
            .skip(1)
            .map(|rest| {
                rest.chars()
                    .take_while(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
                    .collect()
            })
            .collect()
    }

    #[test]
    fn every_file_the_build_stages_is_rendered() {
        let staged = staged_files(include_str!("../../scripts/prepare-agent.sh"));
        assert!(
            !staged.is_empty(),
            "prepare-agent.sh no longer stages licences"
        );
        let rendered: BTreeSet<String> = NOTICE_FILES.iter().map(|n| n.file.to_string()).collect();
        assert_eq!(staged, rendered);
    }

    #[test]
    fn reads_each_staged_text_from_the_resource_dir() {
        let resources = tempfile::tempdir().unwrap();
        let dir = resources.path().join("licenses");
        std::fs::create_dir(&dir).unwrap();
        std::fs::write(dir.join("pidash-LICENSE.txt"), "GNU AFFERO").unwrap();
        std::fs::write(dir.join("agent-engine-LICENSE.txt"), "Apache License").unwrap();
        std::fs::write(
            dir.join("agent-engine-NOTICE.txt"),
            "renamed to pidash-agent-engine",
        )
        .unwrap();
        // The directory also carries its README; that is not a licence.
        std::fs::write(dir.join("README.md"), "# Third-party binaries").unwrap();

        let notices = read_notices(resources.path());
        let shown: Vec<_> = notices
            .iter()
            .map(|n| (n.license, n.text.as_deref()))
            .collect();
        assert_eq!(
            shown,
            [
                ("AGPL-3.0-only", Some("GNU AFFERO")),
                ("Apache-2.0", Some("Apache License")),
                ("Apache-2.0", Some("renamed to pidash-agent-engine")),
            ]
        );
    }

    #[test]
    fn a_file_the_build_did_not_stage_is_listed_without_text() {
        let resources = tempfile::tempdir().unwrap();
        let notices = read_notices(resources.path());
        assert_eq!(notices.len(), NOTICE_FILES.len());
        assert!(notices.iter().all(|n| n.text.is_none()));
    }

    #[test]
    fn user_facing_titles_do_not_use_the_upstream_trademark() {
        for notice in NOTICE_FILES {
            assert!(!notice.component.to_lowercase().contains("codex"));
        }
    }
}
