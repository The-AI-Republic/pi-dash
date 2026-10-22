// Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `pidash workpad …` subcommands.
//!
//! The workpad is the coding agent's durable per-issue scratchpad. It used
//! to live in a dedicated `## Agent Workpad` IssueComment; with the comment
//! thread now reserved for human ↔ agent conversation, the workpad is a
//! plain markdown field on the issue itself and these commands are the
//! agent's read/write channel.

use std::path::{Path, PathBuf};

use clap::{Args, Subcommand};
use serde_json::json;

use crate::api_client::{ApiClient, CliEnv, CliError, EXIT_INVALID, EXIT_UNKNOWN, report_error};

use super::resolve::resolve_issue;

#[derive(Debug, Args)]
pub struct WorkpadArgs {
    #[command(subcommand)]
    pub command: WorkpadCommand,
}

#[derive(Debug, Subcommand)]
pub enum WorkpadCommand {
    /// Fetch the agent workpad for a work item.
    ///
    /// Defaults the work-item identifier to `PIDASH_ISSUE_IDENTIFIER` when
    /// omitted, so the agent can call `pidash workpad get` with no args.
    Get {
        /// Work item identifier, e.g. `ENG-42`. Defaults to
        /// `PIDASH_ISSUE_IDENTIFIER`.
        identifier: Option<String>,
    },
    /// Overwrite the agent workpad. An empty file clears it.
    ///
    /// On a successful upload the `--body-file` is deleted, so the local
    /// staging copy can't go stale and clobber newer server content on a
    /// later run. Pass `--keep` to retain it.
    Update {
        /// Work item identifier, e.g. `ENG-42`. Defaults to
        /// `PIDASH_ISSUE_IDENTIFIER`.
        identifier: Option<String>,
        /// Path to a file containing the workpad body (markdown).
        #[arg(long = "body-file", value_name = "PATH")]
        body_file: PathBuf,
        /// Keep the body file after a successful upload instead of deleting it.
        #[arg(long = "keep")]
        keep: bool,
    },
}

pub async fn run(args: WorkpadArgs, paths: &crate::util::paths::Paths) -> i32 {
    let env = match CliEnv::resolve(paths) {
        Ok(e) => e,
        Err(e) => return report_error(&e),
    };
    let client = match ApiClient::new(env) {
        Ok(c) => c,
        Err(e) => return report_error(&CliError::new(EXIT_UNKNOWN, format!("{e}"))),
    };

    let result = match args.command {
        WorkpadCommand::Get { identifier } => cmd_get(&client, identifier).await,
        WorkpadCommand::Update {
            identifier,
            body_file,
            keep,
        } => cmd_update(&client, identifier, body_file, keep).await,
    };
    match result {
        Ok(()) => 0,
        Err(e) => report_error(&e),
    }
}

async fn cmd_get(client: &ApiClient, identifier: Option<String>) -> Result<(), CliError> {
    let current_issue = std::env::var("PIDASH_ISSUE_IDENTIFIER").ok();
    let ident = resolve_identifier(identifier.as_deref(), current_issue.as_deref())?;
    let issue = resolve_issue(client, ident).await?;
    let path = format!(
        "workspaces/{}/projects/{}/work-items/{}/workpad/",
        client.env.workspace_slug, issue.project_id, issue.id
    );
    let resp = client.get(&path).await?;
    println!(
        "{}",
        serde_json::to_string(&resp).expect("serialize JSON value")
    );
    Ok(())
}

async fn cmd_update(
    client: &ApiClient,
    identifier: Option<String>,
    body_file: PathBuf,
    keep: bool,
) -> Result<(), CliError> {
    let body = load_workpad_body(&body_file)?;
    let current_issue = std::env::var("PIDASH_ISSUE_IDENTIFIER").ok();
    let ident = resolve_identifier(identifier.as_deref(), current_issue.as_deref())?;
    let issue = resolve_issue(client, ident).await?;
    let path = format!(
        "workspaces/{}/projects/{}/work-items/{}/workpad/",
        client.env.workspace_slug, issue.project_id, issue.id
    );
    // Any HTTP/network/validation error propagates here and leaves the body
    // file in place, so the agent can retry without losing its edits.
    let resp = client.patch(&path, &json!({ "body": body })).await?;
    println!(
        "{}",
        serde_json::to_string(&resp).expect("serialize JSON value")
    );
    // Only past this point — a confirmed successful upload — do we remove the
    // local staging copy. A cleanup failure must not turn a successful upload
    // into a failed command: warn and still exit 0.
    if let Some(warning) = warn_on_cleanup(cleanup_body_file(&body_file, keep)) {
        eprintln!("{warning}");
    }
    Ok(())
}

/// What happened to the `--body-file` after a successful upload.
#[derive(Debug)]
enum Cleanup {
    /// Removed the regular file.
    Deleted,
    /// `--keep` was passed; left in place.
    Kept,
    /// Not a regular file we own (stdin `-`, a directory, symlink, missing,
    /// etc.); nothing to delete.
    Skipped,
    /// Removal was attempted on a regular file but failed. Non-fatal.
    Failed(String),
}

/// Decide whether to delete the body file after a successful upload, and do it.
///
/// Deletes **only** a plain regular file at `path`. Skips the stdin sentinel
/// `-`, directories, symlinks, and anything already gone. Never returns an
/// error: a removal failure is reported as [`Cleanup::Failed`] so the caller
/// can warn while still exiting 0.
fn cleanup_body_file(path: &Path, keep: bool) -> Cleanup {
    if keep {
        return Cleanup::Kept;
    }
    if is_stdin(path) {
        return Cleanup::Skipped;
    }
    match std::fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_file() => match std::fs::remove_file(path) {
            Ok(()) => Cleanup::Deleted,
            Err(e) => Cleanup::Failed(format!(
                "could not delete workpad body file {} after upload: {e}",
                path.display()
            )),
        },
        // Directory, symlink, fifo, or already removed — not ours to delete.
        Ok(_) | Err(_) => Cleanup::Skipped,
    }
}

/// The stdin sentinel some CLIs accept for `--body-file`. We never write it,
/// but guard against ever deleting a file literally named `-` by mistake.
fn is_stdin(path: &Path) -> bool {
    path.as_os_str() == "-"
}

/// Map a cleanup outcome to a stderr warning line, if any. Only
/// [`Cleanup::Failed`] warns; every other outcome is silent and successful.
fn warn_on_cleanup(outcome: Cleanup) -> Option<String> {
    match outcome {
        Cleanup::Failed(msg) => Some(format!("warning: {msg}")),
        Cleanup::Deleted | Cleanup::Kept | Cleanup::Skipped => None,
    }
}

fn resolve_identifier<'a>(
    explicit: Option<&'a str>,
    current_issue: Option<&'a str>,
) -> Result<&'a str, CliError> {
    explicit.or(current_issue).ok_or_else(|| {
        CliError::new(
            EXIT_INVALID,
            "workpad requires an issue identifier or PIDASH_ISSUE_IDENTIFIER",
        )
    })
}

fn load_workpad_body(path: &Path) -> Result<String, CliError> {
    std::fs::read_to_string(path).map_err(|e| {
        CliError::new(
            EXIT_UNKNOWN,
            format!("failed reading workpad body file {}: {e}", path.display()),
        )
    })
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{
        Cleanup, cleanup_body_file, load_workpad_body, resolve_identifier, warn_on_cleanup,
    };

    #[test]
    fn resolve_prefers_explicit_arg() {
        let target = resolve_identifier(Some("ENG-42"), Some("ENG-7")).expect("target");
        assert_eq!(target, "ENG-42");
    }

    #[test]
    fn resolve_falls_back_to_env() {
        let target = resolve_identifier(None, Some("ENG-7")).expect("target");
        assert_eq!(target, "ENG-7");
    }

    #[test]
    fn resolve_requires_context() {
        let err = resolve_identifier(None, None).expect_err("missing target");
        assert_eq!(
            err.message,
            "workpad requires an issue identifier or PIDASH_ISSUE_IDENTIFIER"
        );
    }

    #[test]
    fn load_workpad_body_reads_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("pad.md");
        std::fs::write(&path, "## Agent Workpad\n\nphase: implementing\n").expect("write file");

        let body = load_workpad_body(&path).expect("body");
        assert!(body.contains("phase: implementing"));
    }

    // --- cleanup after a successful upload ---------------------------------

    #[test]
    fn cleanup_deletes_regular_file_on_success() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("pad.md");
        std::fs::write(&path, "body").expect("write file");

        let outcome = cleanup_body_file(&path, false);

        assert!(matches!(outcome, Cleanup::Deleted));
        assert!(!path.exists(), "body file must be gone after a success");
        assert!(warn_on_cleanup(outcome).is_none(), "delete emits no warning");
    }

    #[test]
    fn cleanup_keeps_file_with_keep_flag() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("pad.md");
        std::fs::write(&path, "body").expect("write file");

        let outcome = cleanup_body_file(&path, true);

        assert!(matches!(outcome, Cleanup::Kept));
        assert!(path.exists(), "--keep must leave the body file in place");
        assert!(warn_on_cleanup(outcome).is_none());
    }

    #[test]
    fn cleanup_skips_stdin_sentinel() {
        // A file literally named `-` must never be deleted (stdin sentinel).
        let outcome = cleanup_body_file(Path::new("-"), false);
        assert!(matches!(outcome, Cleanup::Skipped));
    }

    #[test]
    fn cleanup_skips_directory() {
        let dir = tempfile::tempdir().expect("tempdir");
        let outcome = cleanup_body_file(dir.path(), false);
        assert!(matches!(outcome, Cleanup::Skipped));
        assert!(dir.path().exists(), "a directory must not be removed");
    }

    #[test]
    fn cleanup_skips_missing_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let missing = dir.path().join("gone.md");
        let outcome = cleanup_body_file(&missing, false);
        assert!(matches!(outcome, Cleanup::Skipped));
    }

    /// A removal failure must surface as `Failed` (→ a stderr warning) while
    /// leaving the file in place, so a successful upload still exits 0.
    #[cfg(unix)]
    #[test]
    fn cleanup_failure_is_non_fatal_and_keeps_file() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("pad.md");
        std::fs::write(&path, "body").expect("write file");

        // Drop write on the containing directory so `remove_file` is denied.
        let ro = std::fs::Permissions::from_mode(0o555);
        std::fs::set_permissions(dir.path(), ro).expect("chmod ro");

        // Under a euid that bypasses permission checks (e.g. root in CI) the
        // removal would still succeed; probe for that and skip the assertion.
        let probe = std::fs::File::create(dir.path().join(".probe"));
        let enforced = probe.is_err();

        let outcome = cleanup_body_file(&path, false);

        // Restore perms so the tempdir can be cleaned up.
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o755))
            .expect("chmod rw");

        if enforced {
            assert!(
                matches!(outcome, Cleanup::Failed(_)),
                "a denied removal must report Failed, got {outcome:?}",
            );
            assert!(path.exists(), "file must survive a failed delete");
            let warning = warn_on_cleanup(outcome).expect("failure warns");
            assert!(warning.starts_with("warning:"));
        }
    }
}
