//! D-37 data-repair commands: CLI (stage 7, PIDASHCONV-808).
//!
//! Clap wiring plus I/O orchestration for the five repair commands
//! (`apps/api/pi_dash/db/management/commands/copy_issue_comment_to_description.py`,
//! `fix_duplicate_sequences.py`, `sync_issue_version.py`,
//! `sync_issue_description_version.py`, `update_deleted_workspace_slug.py`;
//! drift baseline `01a93e17216faea7bfc156b0f864cbbe420d1c52`). Prompts,
//! stdout/stderr bytes and exit codes match Django 4.2 exactly as observed
//! on a pipe (plain, no ANSI): success lines go to stdout with exit 0,
//! `CommandError` paths print `CommandError: {message}` on stderr with
//! exit 1 (Django's `run_from_argv` rendering).
//!
//! # Ported bugs and quirks (translate, don't redesign)
//!
//! * `batch_size` is published as the raw `input()` string.
//! * The slug success line prints the already-mutated slug twice.
//! * The countdown keeps CPython `int()` conversion.
//!
//! # Known divergences (untestable across implementations)
//!
//! * Missing CLI args render clap's usage error, not argparse's.
//! * Paths where Django prints a traceback (bad countdown, `input()`
//!   EOF, unreachable database or broker) exit 1 on stderr here with a
//!   one-line `CommandError:` message instead; `input()` EOF otherwise
//!   reads as the empty string.
//! * DB-driver error text differs (sqlx vs psycopg), e.g. the slug
//!   save-failure line's `{error}` tail.

use std::fmt::Display;
use std::io::{BufRead, Write};

use chrono::Utc;
use clap::Args;
use uuid::Uuid;

/// `input("Workspace slug: ")` (`fix_duplicate_sequences.py:28`).
pub const WORKSPACE_SLUG_PROMPT: &str = "Workspace slug: ";
/// `input("Enter the batch size: ")` (`sync_*.py`).
pub const BATCH_SIZE_PROMPT: &str = "Enter the batch size: ";
/// `input("Enter the batch countdown: ")` (`sync_*.py`).
pub const BATCH_COUNTDOWN_PROMPT: &str = "Enter the batch countdown: ";

/// `fix_duplicate_sequences` CLI (`:18-20`).
#[derive(Debug, Args)]
pub struct FixArgs {
    /// Issue Identifier
    pub issue_identifier: String,
}

/// `update_deleted_workspace_slug` CLI (`:13-23`).
#[derive(Debug, Args)]
pub struct SlugArgs {
    /// The slug of the workspace to update
    pub slug: String,
    /// Run the command without making any changes
    #[arg(long)]
    pub dry_run: bool,
}

/// Print a prompt (no newline, flushed) and read one `input()` line:
/// the trailing newline stripped, EOF reading as the empty string.
fn read_input(prompt: &str) -> String {
    print!("{prompt}");
    std::io::stdout()
        .flush()
        .expect("stdout flushes for prompts");
    let mut line = String::new();
    let read = std::io::stdin()
        .lock()
        .read_line(&mut line)
        .expect("stdin reads for prompts");
    if read == 0 {
        return String::new();
    }
    if line.ends_with('\n') {
        line.pop();
        if line.ends_with('\r') {
            line.pop();
        }
    }
    line
}

/// Django's `CommandError` rendering: `CommandError: {message}` on stderr,
/// exit 1. Diverges only where Django would traceback (see module docs).
fn fail(message: impl Display) -> ! {
    eprint!("CommandError: {message}");
    eprintln!();
    std::process::exit(1);
}

/// Connect the primary pool, failing fast like `serve` (a missing or
/// unreachable database is a boot error, never per-command output).
async fn connect_primary() -> pidash_db::Pools {
    let db = pidash_db::DbConfig::from_env().unwrap_or_else(|e| fail(e));
    pidash_db::Pools::connect(&db, None)
        .await
        .unwrap_or_else(|e| fail(e))
}

/// `copy_issue_comment_to_description`: batch loop until empty, then the
/// success line (`:17-53`).
pub async fn run_copy() {
    use pidash_db::ops::repair as db;
    use pidash_services::ops::repair as decisions;

    let pools = connect_primary().await;
    let pool = pools.primary();
    loop {
        let batch = db::fetch_comment_batch(pool, db::COPY_BATCH_SIZE)
            .await
            .unwrap_or_else(|e| fail(e));
        if batch.is_empty() {
            break;
        }
        let now = Utc::now();
        let ids: Vec<Uuid> = (0..batch.len()).map(|_| Uuid::new_v4()).collect();
        let (descriptions, links) = decisions::plan_copy_batch(batch, now, ids);
        db::apply_copy_batch(pool, &descriptions, &links)
            .await
            .unwrap_or_else(|e| fail(e));
    }
    println!("{}", decisions::COPY_DONE_LINE);
}

/// `fix_duplicate_sequences` (`:27-95`).
pub async fn run_fix(args: FixArgs) {
    use pidash_db::ops::repair as db;
    use pidash_services::ops::repair as decisions;

    let slug = read_input(WORKSPACE_SLUG_PROMPT);
    let parsed = decisions::parse_fix_request(&slug, &args.issue_identifier)
        .unwrap_or_else(|message| fail(message));
    let pools = connect_primary().await;
    let pool = pools.primary();
    let candidates =
        db::find_project_rows(pool, &parsed.project_identifier, &parsed.workspace_slug)
            .await
            .unwrap_or_else(|e| fail(e));
    let project_id =
        decisions::resolve_project_id(&candidates).unwrap_or_else(|message| fail(message));
    let issues = db::fetch_duplicate_issues(pool, &project_id, parsed.sequence)
        .await
        .unwrap_or_else(|e| fail(e));
    decisions::check_duplicate_count(issues.len()).unwrap_or_else(|message| fail(message));
    // Continues the prompt line, as Django's `stdout.write` does.
    println!(
        "{}",
        decisions::fix_found_line(issues.len(), &args.issue_identifier)
    );
    let tx = db::begin_fix_tx(pool, &project_id)
        .await
        .unwrap_or_else(|e| fail(e));
    let duplicate_ids: Vec<Uuid> = issues[1..].iter().map(|issue| issue.id).collect();
    // `None` (no sequences) drops `tx` uncommitted: rollback, no writes.
    let issue_updates = db::renumber_plan(tx.max_sequence(), &duplicate_ids)
        .unwrap_or_else(|message| fail(message));
    let map = db::build_sequence_map(tx.sequences());
    let sequence_updates = db::plan_sequence_updates(&map, &issue_updates);
    tx.commit_plan(&issue_updates, &sequence_updates)
        .await
        .unwrap_or_else(|e| fail(e));
    println!("{}", decisions::FIX_DONE_LINE);
}

async fn run_sync(kind: pidash_services::ops::repair::SyncKind) {
    use pidash_jobs::celery::CeleryTaskMessage;
    use pidash_services::ops::repair as decisions;

    let batch_size = read_input(BATCH_SIZE_PROMPT);
    let countdown_raw = read_input(BATCH_COUNTDOWN_PROMPT);
    let message = decisions::sync_message(kind, &batch_size, &countdown_raw)
        .unwrap_or_else(|message| fail(message));
    let config = pidash_jobs::AmqpConfig::from_env().unwrap_or_else(|e| fail(e));
    let publisher = pidash_jobs::Publisher::connect(&config)
        .await
        .unwrap_or_else(|e| fail(e));
    let wire = CeleryTaskMessage::new(message.task, vec![], message.kwargs);
    publisher.publish(&wire).await.unwrap_or_else(|e| fail(e));
    publisher.close().await.unwrap_or_else(|e| fail(e));
    println!("{}", kind.done_line());
}

/// `sync_issue_version` (`:15-21`).
pub async fn run_sync_version() {
    run_sync(pidash_services::ops::repair::SyncKind::IssueVersion).await;
}

/// `sync_issue_description_version` (`:17-23`).
pub async fn run_sync_description_version() {
    run_sync(pidash_services::ops::repair::SyncKind::IssueDescriptionVersion).await;
}

/// `update_deleted_workspace_slug` (`:25-71`). Every branch exits 0,
/// including the save-failure line (stdout, like Django).
pub async fn run_slug(args: SlugArgs) {
    use pidash_db::ops::repair as db;
    use pidash_services::ops::repair as decisions;

    let pools = connect_primary().await;
    let pool = pools.primary();
    let workspace = db::find_workspace(pool, &args.slug)
        .await
        .unwrap_or_else(|e| fail(e));
    match decisions::decide_slug(workspace.as_ref(), &args.slug, args.dry_run) {
        decisions::SlugDecision::Print(line) => println!("{line}"),
        decisions::SlugDecision::Write {
            workspace_id,
            name,
            new_slug,
            success_line,
        } => match db::apply_slug_update(pool, &workspace_id, &new_slug).await {
            Ok(()) => println!("{success_line}"),
            Err(error) => println!("{}", decisions::slug_save_error_line(&name, &error)),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::super::OpsCommand;
    use clap::Parser;

    /// Probe parser: `Subcommand` enums parse only under a `Parser`.
    #[derive(Debug, Parser)]
    struct Probe {
        #[command(subcommand)]
        command: OpsCommand,
    }

    fn parse(argv: &[&str]) -> OpsCommand {
        Probe::try_parse_from(argv).expect("parses").command
    }

    #[test]
    fn subcommand_names_are_django_verbatim() {
        assert!(matches!(
            parse(&["probe", "copy_issue_comment_to_description"]),
            OpsCommand::CopyIssueCommentToDescription
        ));
        assert!(matches!(
            parse(&["probe", "sync_issue_version"]),
            OpsCommand::SyncIssueVersion
        ));
        assert!(matches!(
            parse(&["probe", "sync_issue_description_version"]),
            OpsCommand::SyncIssueDescriptionVersion
        ));
        match parse(&["probe", "fix_duplicate_sequences", "FX-7"]) {
            OpsCommand::FixDuplicateSequences(args) => {
                assert_eq!(args.issue_identifier, "FX-7");
            }
            other => panic!("wrong variant: {other:?}"),
        }
        match parse(&["probe", "update_deleted_workspace_slug", "ws"]) {
            OpsCommand::UpdateDeletedWorkspaceSlug(args) => {
                assert_eq!(args.slug, "ws");
                assert!(!args.dry_run);
            }
            other => panic!("wrong variant: {other:?}"),
        }
        match parse(&["probe", "update_deleted_workspace_slug", "ws", "--dry-run"]) {
            OpsCommand::UpdateDeletedWorkspaceSlug(args) => assert!(args.dry_run),
            other => panic!("wrong variant: {other:?}"),
        }
    }

    #[test]
    fn prompts_are_django_verbatim() {
        assert_eq!(super::WORKSPACE_SLUG_PROMPT, "Workspace slug: ");
        assert_eq!(super::BATCH_SIZE_PROMPT, "Enter the batch size: ");
        assert_eq!(super::BATCH_COUNTDOWN_PROMPT, "Enter the batch countdown: ");
    }
}
