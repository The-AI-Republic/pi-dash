// Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `pidash workpad …` subcommands.
//!
//! The workpad is the shared memory of the agent runs on one issue (In
//! Progress, review and test; `.ai_design/unified_workpad/design.md`). It used
//! to live in a dedicated `## Agent Workpad` IssueComment; with the comment
//! thread now reserved for human ↔ agent conversation, the workpad is a
//! plain markdown field on the issue itself and these commands are the
//! agent's read/write channel.

use std::path::{Path, PathBuf};

use clap::{Args, Subcommand};
use serde_json::json;

use crate::api_client::{
    ApiClient, CliEnv, CliError, EXIT_INVALID, EXIT_SERVER, EXIT_UNKNOWN, report_error,
};

use super::resolve::resolve_issue;
use super::state::ISSUE_ENV;

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
        /// Write the body to this file (default `.pidash/workpad.md`) and
        /// the hash to its `workpad.sha256` sibling instead of printing
        /// the body, so a refresh does not load the whole workpad into
        /// the agent's context. Prints `{updated_at, body_sha256, path}`.
        /// `--to-file` with no path uses the default.
        #[arg(long = "to-file", value_name = "PATH", num_args = 0..=1, default_missing_value = DEFAULT_BODY_FILE)]
        to_file: Option<PathBuf>,
    },
    /// Overwrite the agent workpad.
    ///
    /// Refuses an empty or whitespace-only body unless `--allow-empty` is
    /// passed, to prevent a failed `workpad get` (e.g. 429) that produced an
    /// empty file from silently wiping the stored workpad. Pass
    /// `--allow-empty` when a clear is intentional.
    ///
    /// Sends the `workpad.sha256` sibling as the `if_body_sha256` guard and
    /// rewrites it from the response; on a 412 the command fails and tells
    /// the agent to run `workpad get --to-file`, re-apply its edit, and
    /// retry. The body file is kept: pass `--remove` for the old delete
    /// behaviour (`--keep` is accepted as a no-op).
    Update {
        /// Work item identifier, e.g. `ENG-42`. Defaults to
        /// `PIDASH_ISSUE_IDENTIFIER`.
        identifier: Option<String>,
        /// Path to a file containing the workpad body (markdown).
        /// Defaults to `.pidash/workpad.md`.
        #[arg(long = "body-file", value_name = "PATH", default_value = DEFAULT_BODY_FILE)]
        body_file: PathBuf,
        /// Allow an empty or whitespace-only body to clear the workpad.
        /// Without this flag an empty body is rejected so an accidental
        /// empty file cannot wipe the stored workpad.
        #[arg(long = "allow-empty")]
        allow_empty: bool,
        /// Keep the body file after a successful upload instead of deleting it.
        /// Accepted for back-compat; the file is always kept now.
        #[arg(long = "keep")]
        keep: bool,
        /// Delete the body file after a successful upload (the old default).
        #[arg(long = "remove")]
        remove: bool,
    },
}

/// Default body path: the runner materialises the run's workpad snapshot
/// here before the agent launches (NEWPIDASH-320 §6.1).
pub const DEFAULT_BODY_FILE: &str = ".pidash/workpad.md";

/// Hash-file name: `workpad update` sends the sibling file's content as
/// the `if_body_sha256` guard and rewrites it from the response;
/// `workpad get --to-file` writes it next to the body.
pub const HASH_FILE_NAME: &str = "workpad.sha256";

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
        WorkpadCommand::Get {
            identifier,
            to_file,
        } => cmd_get(&client, identifier, to_file).await,
        WorkpadCommand::Update {
            identifier,
            body_file,
            allow_empty,
            keep,
            remove,
        } => cmd_update(&client, identifier, body_file, allow_empty, keep, remove).await,
    };
    match result {
        Ok(()) => 0,
        Err(e) => report_error(&e),
    }
}

/// The hash-file sibling of a body path: `<dir>/workpad.sha256`.
fn hash_path_for(body_file: &Path) -> PathBuf {
    match body_file.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent.join(HASH_FILE_NAME),
        _ => PathBuf::from(HASH_FILE_NAME),
    }
}

fn read_guard_hash(body_file: &Path) -> Option<String> {
    let text = std::fs::read_to_string(hash_path_for(body_file)).ok()?;
    let trimmed = text.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn write_guard_hash(body_file: &Path, hash: &str) -> Result<(), CliError> {
    std::fs::write(hash_path_for(body_file), format!("{hash}\n")).map_err(|e| {
        CliError::new(
            EXIT_UNKNOWN,
            format!(
                "failed writing workpad hash file {}: {e}",
                hash_path_for(body_file).display()
            ),
        )
    })
}

async fn cmd_get(
    client: &ApiClient,
    identifier: Option<String>,
    to_file: Option<PathBuf>,
) -> Result<(), CliError> {
    let current_issue = std::env::var(ISSUE_ENV).ok();
    let ident = resolve_identifier(identifier.as_deref(), current_issue.as_deref())?;
    let issue = resolve_issue(client, ident).await?;
    let path = format!(
        "workspaces/{}/projects/{}/work-items/{}/workpad/",
        client.env.workspace_slug, issue.project_id, issue.id
    );
    let resp = client.get(&path).await?;
    let Some(dest) = to_file else {
        println!(
            "{}",
            serde_json::to_string(&resp).expect("serialize JSON value")
        );
        return Ok(());
    };
    let body = resp
        .get("body")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| CliError::new(EXIT_SERVER, "workpad response missing 'body'"))?;
    let hash = resp
        .get("body_sha256")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| {
            CliError::new(
                EXIT_SERVER,
                "workpad response missing 'body_sha256' (old server: needs the new API deploy)",
            )
        })?;
    if let Some(parent) = dest.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent).map_err(|e| {
            CliError::new(
                EXIT_UNKNOWN,
                format!(
                    "failed creating workpad directory {}: {e}",
                    parent.display()
                ),
            )
        })?;
    }
    std::fs::write(&dest, body).map_err(|e| {
        CliError::new(
            EXIT_UNKNOWN,
            format!("failed writing workpad file {}: {e}", dest.display()),
        )
    })?;
    write_guard_hash(&dest, hash)?;
    let updated_at = resp
        .get("updated_at")
        .cloned()
        .unwrap_or(serde_json::Value::Null);
    println!(
        "{}",
        serde_json::to_string(&json!({
            "updated_at": updated_at,
            "body_sha256": hash,
            "path": dest.display().to_string(),
        }))
        .expect("serialize JSON value")
    );
    Ok(())
}

async fn cmd_update(
    client: &ApiClient,
    identifier: Option<String>,
    body_file: PathBuf,
    allow_empty: bool,
    keep: bool,
    remove: bool,
) -> Result<(), CliError> {
    let _ = keep;
    let body = load_workpad_body(&body_file)?;
    check_workpad_body(&body, allow_empty)?;
    let current_issue = std::env::var(ISSUE_ENV).ok();
    let ident = resolve_identifier(identifier.as_deref(), current_issue.as_deref())?;
    let issue = resolve_issue(client, ident).await?;
    let path = format!(
        "workspaces/{}/projects/{}/work-items/{}/workpad/",
        client.env.workspace_slug, issue.project_id, issue.id
    );
    let mut payload = json!({ "body": body });
    if let Some(guard) = read_guard_hash(&body_file) {
        payload["if_body_sha256"] = json!(guard);
    }
    // Any HTTP/network/validation error propagates here and leaves the body
    // file in place, so the agent can retry without losing its edits.
    let resp = match client.patch(&path, &payload).await {
        Ok(resp) => resp,
        Err(err) if err.status == Some(412) => {
            return Err(CliError::new(
                EXIT_INVALID,
                "workpad changed since you read it: run `pidash workpad get --to-file`, re-apply your edit, and retry",
            )
            .with_detail(err.detail.unwrap_or_default())
            .with_status(412));
        }
        Err(err) => return Err(err),
    };
    // Rewrite the guard from the response when the server reports one, so
    // two consecutive updates succeed without an intervening get.
    if let Some(hash) = resp.get("body_sha256").and_then(serde_json::Value::as_str) {
        write_guard_hash(&body_file, hash)?;
    }
    println!(
        "{}",
        serde_json::to_string(&resp).expect("serialize JSON value")
    );
    // Only past this point — a confirmed successful upload — does `--remove`
    // delete the local staging copy (`--keep` is a no-op: the file stays by
    // default). A cleanup failure must not turn a successful upload into a
    // failed command: warn and still exit 0.
    if let Some(warning) = warn_on_cleanup(cleanup_body_file(&body_file, !remove)) {
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

/// Refuse an empty or whitespace-only workpad body unless `--allow-empty`.
/// This prevents a failed `workpad get` (e.g. 429) that produced an empty
/// file from silently wiping the stored workpad.
fn check_workpad_body(body: &str, allow_empty: bool) -> Result<(), CliError> {
    if !allow_empty && body.trim().is_empty() {
        return Err(CliError::new(
            EXIT_INVALID,
            "workpad body is empty (empty or whitespace-only); pass --allow-empty to clear intentionally and do not upload an empty file when a get failed (e.g. 429)",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{
        Cleanup, check_workpad_body, cleanup_body_file, load_workpad_body, resolve_identifier,
        warn_on_cleanup,
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

    #[test]
    fn empty_workpad_body_is_rejected_without_allow_empty() {
        use crate::api_client::EXIT_INVALID;

        for body in ["", "   ", "\n\t \n"] {
            let err = check_workpad_body(body, false).expect_err("empty must be rejected");
            assert_eq!(err.exit_code, EXIT_INVALID);
            assert!(err.message.contains("--allow-empty"));
        }
    }

    #[test]
    fn empty_workpad_body_is_allowed_with_allow_empty() {
        for body in ["", "   ", "\n"] {
            check_workpad_body(body, true).expect("allow-empty passes");
        }
    }

    #[test]
    fn non_empty_workpad_body_passes() {
        check_workpad_body("hello", false).expect("non-empty passes");
        check_workpad_body("  hello  ", false).expect("non-empty with spaces passes");
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
        assert!(
            warn_on_cleanup(outcome).is_none(),
            "delete emits no warning"
        );
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

    /// A server predating the workpad guard answers no `body_sha256`;
    /// `--to-file` must fail loudly pointing at the server deploy (not the
    /// runner) and write nothing, so no unguarded sha file is ever stored.
    #[tokio::test]
    async fn get_to_file_blames_the_server_deploy_when_sha_is_absent() {
        let handler: crate::pidash_agent::test_server::Handler = std::sync::Arc::new(|req| {
            if req.path.ends_with("/work-items/ENG-7/") {
                return (200, r#"{"id":"iid-1","project":"pid-1"}"#.to_string());
            }
            assert!(req.path.contains("/workpad/"), "unexpected {}", req.path);
            (
                200,
                r#"{"body":"hello","updated_at":"2026-01-01T00:00:00Z"}"#.to_string(),
            )
        });
        let server = crate::pidash_agent::test_server::start(handler).await;
        let client = crate::api_client::ApiClient::new(crate::api_client::CliEnv {
            api_url: server.url.clone(),
            workspace_slug: "ws".to_string(),
            token: "tok".to_string(),
            run_id: None,
        })
        .expect("client");
        let dir = tempfile::tempdir().expect("tempdir");
        let dest = dir.path().join("wd.md");
        let err = super::cmd_get(&client, Some("ENG-7".to_string()), Some(dest.clone()))
            .await
            .expect_err("old server must fail --to-file");
        assert!(err.message.contains("new API deploy"), "{}", err.message);
        assert!(!dest.exists(), "no body file may be written on failure");
    }
}
