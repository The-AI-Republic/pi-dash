//! `pidash-api ops`: Django management-command ports (D-37, PIDASHCONV-74).
//!
//! One module per command issue; each issue owns its `mod` line, its enum
//! variants and its match arms below, and nothing else. Groups stay in the
//! order boot, users, repair, instance, prompting so sibling rebases stay
//! mechanical. Subcommand names are the Django command names verbatim so
//! runbooks transfer unchanged.
//!
//! * [`boot`] — the boot + storage commands (PIDASHCONV-806).
//! * [`users`] — the users + membership commands (PIDASHCONV-807).
//! * [`repair`] — the data-repair commands (PIDASHCONV-808).
//! * [`prompting`] — the prompting reseed + revalidate commands
//!   (PIDASHCONV-810).

pub mod boot;
pub mod prompting;
pub mod repair;
pub mod users;

use clap::Subcommand;
use pidash_services::ops::storage;
use std::time::Duration;

/// An ops failure: the one-line stderr message for paths where Python
/// raises out of `handle` (traceback, exit 1). The exit code matches
/// Python; the text is a single line instead of a traceback.
#[derive(Debug)]
pub struct OpsFailure(pub String);

impl std::fmt::Display for OpsFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for OpsFailure {}

#[derive(Debug, Subcommand)]
#[command(rename_all = "snake_case")]
pub enum OpsCommand {
    // --- boot (PIDASHCONV-806) ---
    // No `about`: Django sets no help text on `wait_for_db`
    // (`db/management/commands/wait_for_db.py` has no `help` attribute),
    // so `--help` shows the command name only. A doc comment here would
    // wrongly become clap's `about`.
    #[command(name = "wait_for_db")]
    WaitForDb,
    /// Wait for database migrations to complete before starting Celery worker/beat.
    #[command(
        name = "wait_for_migrations",
        about = "Wait for database migrations to complete before starting Celery worker/beat"
    )]
    WaitForMigrations,
    /// Clear Cache before starting the server to remove stale values.
    #[command(
        name = "clear_cache",
        about = "Clear Cache before starting the server to remove stale values"
    )]
    ClearCache(boot::ClearCacheArgs),
    /// Create the default bucket for the instance.
    #[command(
        name = "create_bucket",
        about = "Create the default bucket for the instance"
    )]
    CreateBucket,
    /// Create the default bucket for the instance.
    ///
    /// (The stale copy of create_bucket's help — ported as is from
    /// `update_bucket.py:16`.)
    #[command(
        name = "update_bucket",
        about = "Create the default bucket for the instance"
    )]
    UpdateBucket,
    // --- users (PIDASHCONV-807) ---
    /// Make the user with the given email active.
    ActivateUser(users::ActivateUserArgs),
    /// Reset password of the user with the given email.
    ResetPassword(users::ResetPasswordArgs),
    /// Add a new instance admin.
    CreateInstanceAdmin(users::CreateInstanceAdminArgs),
    /// Add a member to a project. If present in the workspace.
    CreateProjectMember(users::CreateProjectMemberArgs),
    /// Create dump issues, cycles etc. for a project in a given workspace.
    CreateDummyData(users::CreateDummyDataArgs),
    // --- repair (PIDASHCONV-808) ---
    /// Create Description records for existing IssueComment.
    #[command(name = "copy_issue_comment_to_description")]
    CopyIssueCommentToDescription,
    /// Fix duplicate sequences.
    #[command(name = "fix_duplicate_sequences")]
    FixDuplicateSequences(repair::FixArgs),
    /// Creates IssueVersion records for existing Issues in batches.
    #[command(name = "sync_issue_version")]
    SyncIssueVersion,
    /// Creates IssueDescriptionVersion records for existing Issues in batches.
    #[command(name = "sync_issue_description_version")]
    SyncIssueDescriptionVersion,
    /// Updates the slug of a soft-deleted workspace by appending the epoch timestamp.
    #[command(name = "update_deleted_workspace_slug")]
    UpdateDeletedWorkspaceSlug(repair::SlugArgs),
    // --- prompting (PIDASHCONV-810) ---
    /// Refresh the global default PromptTemplate from the ordered fragments
    /// in apps/api/pi_dash/prompting/fragments/. Does not touch
    /// workspace-scoped templates.
    #[command(name = "reseed_default_template")]
    ReseedDefaultTemplate(prompting::ReseedArgs),
    /// Refresh the global ``review`` PromptTemplate from the body in
    /// pi_dash.prompting.seed.REVIEW_TEMPLATE_BODY. Does not touch
    /// workspace-scoped templates.
    #[command(name = "reseed_review_template")]
    ReseedReviewTemplate(prompting::ReseedArgs),
    /// Refresh the global ``test`` PromptTemplate from the body in
    /// pi_dash.prompting.seed.TEST_TEMPLATE_BODY. Does not touch
    /// workspace-scoped templates.
    #[command(name = "reseed_test_template")]
    ReseedTestTemplate(prompting::ReseedArgs),
    /// Re-validate active prompt-section overrides; flag broken ones.
    #[command(name = "revalidate_section_overrides")]
    RevalidateSectionOverrides(prompting::RevalidateArgs),
}

/// Print one stdout line (Django's `self.stdout.write` appends `\n`).
fn emit(line: &str) {
    println!("{line}");
}

async fn run_create_bucket() -> Result<(), OpsFailure> {
    let env = storage::S3Env::from_env();
    let mut out = emit;
    // Setup failures land in the outer `except Exception` arm (stdout,
    // exit 0); only the symbolic-code `ValueError` escapes.
    let target = match storage::resolve_create_target(&env) {
        Ok(target) => target,
        // The client-build `ValueError` precedes the `Checking bucket...`
        // print (`create_bucket.py:20-30`), so it stands alone ...
        Err(error @ storage::S3SetupError::InvalidEndpoint(_)) => {
            out(&format!("An error occurred: {error}"));
            return Ok(());
        }
        // ... while the `None`-bucket `TypeError` arises at the
        // `head_bucket` call, after `Checking bucket...` prints (:30-32).
        Err(error @ storage::S3SetupError::BucketNone) => {
            out(storage::CHECKING_BUCKET);
            out(&format!("An error occurred: {error}"));
            return Ok(());
        }
    };
    let ops = storage::ReqwestS3::new(&target);
    storage::run_create_bucket(&ops, &target.bucket, &mut out)
        .await
        .map_err(|error| OpsFailure(error.to_string()))
}

async fn run_update_bucket() -> Result<(), OpsFailure> {
    let env = storage::S3Env::from_env();
    let mut out = emit;
    let target = match storage::resolve_update_target(&env) {
        Ok(storage::UpdateSetup::Ready(target)) => target,
        Ok(storage::UpdateSetup::MissingBucket) => {
            out(storage::PLEASE_SET_BUCKET);
            return Ok(());
        }
        // `get_s3_client()` at `update_bucket.py:138` is outside any
        // `try`: the client-build `ValueError` escapes `handle`
        // (traceback, exit 1).
        Err(error) => return Err(OpsFailure(error.to_string())),
    };
    let ops = storage::ReqwestS3::new(&target);
    storage::run_update_bucket(
        &ops,
        &target.bucket,
        &|path, content| std::fs::write(path, content),
        &mut out,
    )
    .await
    .map_err(|error| OpsFailure(error.to_string()))
}

/// Production poll sleeps, matching the Python `time.sleep` calls.
pub const WAIT_FOR_DB_SLEEP: Duration = Duration::from_secs(1);
pub const WAIT_FOR_MIGRATIONS_SLEEP: Duration = Duration::from_secs(10);

/// Render a boot-group failure exactly (one stderr line, exit 1):
/// the shared `Mode::Ops` dispatch belongs to the sibling groups, so the
/// boot group exits from inside its arms (like the repair group does).
fn boot_exit(error: OpsFailure) -> Box<dyn std::error::Error> {
    use std::io::Write;
    let _ = std::io::stdout().flush();
    eprintln!("{error}");
    std::process::exit(1);
}

/// Run one `ops` subcommand.
///
/// The four groups keep their own failure conventions: boot, users and
/// repair runners print Django's exact bytes and exit 1 from inside the
/// group runner, while prompting runners return `Err` (a boot/DB failure
/// `main` renders as a CLI boot error, exit 1 — there is no Django-oracle
/// shape for failures, only for the documented matrices). Users runners
/// need exit codes and injected stdio, so they dispatch through
/// [`run_users`], which exits from inside on failure (the repair
/// precedent) and returns `Ok` on success. Success prints Django's lines
/// to stdout and returns `Ok` (exit 0).
pub async fn run(command: OpsCommand) -> Result<(), Box<dyn std::error::Error>> {
    use pidash_services::ops::prompting::TemplateKind;
    match command {
        // --- boot (PIDASHCONV-806) ---
        OpsCommand::WaitForDb => boot::run_wait_for_db().await.map_err(boot_exit),
        OpsCommand::WaitForMigrations => boot::run_wait_for_migrations().await.map_err(boot_exit),
        OpsCommand::ClearCache(args) => boot::run_clear_cache(args.key.as_deref())
            .await
            .map_err(boot_exit),
        OpsCommand::CreateBucket => run_create_bucket().await.map_err(boot_exit),
        OpsCommand::UpdateBucket => run_update_bucket().await.map_err(boot_exit),
        // --- users (PIDASHCONV-807) ---
        users_command @ (OpsCommand::ActivateUser(_)
        | OpsCommand::ResetPassword(_)
        | OpsCommand::CreateInstanceAdmin(_)
        | OpsCommand::CreateProjectMember(_)
        | OpsCommand::CreateDummyData(_)) => run_users(users_command).await,
        // --- repair (PIDASHCONV-808) ---
        OpsCommand::CopyIssueCommentToDescription => {
            repair::run_copy().await;
            Ok(())
        }
        OpsCommand::FixDuplicateSequences(args) => {
            repair::run_fix(args).await;
            Ok(())
        }
        OpsCommand::SyncIssueVersion => {
            repair::run_sync_version().await;
            Ok(())
        }
        OpsCommand::SyncIssueDescriptionVersion => {
            repair::run_sync_description_version().await;
            Ok(())
        }
        OpsCommand::UpdateDeletedWorkspaceSlug(args) => {
            repair::run_slug(args).await;
            Ok(())
        }
        // --- prompting (PIDASHCONV-810) ---
        OpsCommand::ReseedDefaultTemplate(args) => {
            prompting::run_reseed(TemplateKind::Default, args).await
        }
        OpsCommand::ReseedReviewTemplate(args) => {
            prompting::run_reseed(TemplateKind::Review, args).await
        }
        OpsCommand::ReseedTestTemplate(args) => {
            prompting::run_reseed(TemplateKind::Test, args).await
        }
        OpsCommand::RevalidateSectionOverrides(args) => prompting::run_revalidate(args).await,
    }
}

/// Standard IO for one users-command run: prompts and results flow
/// through these so unit tests can drive the commands with in-memory
/// buffers. The binary passes the locked stdio handles.
pub struct OpsIo<'a> {
    pub stdin: &'a mut dyn std::io::BufRead,
    pub stdout: &'a mut dyn std::io::Write,
    pub stderr: &'a mut dyn std::io::Write,
}

/// Users subgroup entry (PIDASHCONV-807). Connects from the
/// environment (boot failures print Django's `CommandError` shape and
/// exit 1), wires the locked stdio handles, and dispatches to the
/// `users::run_*` runners, which return the process exit code.
/// Nonzero codes exit from inside (the repair precedent); success
/// returns `Ok`. Output stays byte-exact: nothing on this path
/// emits tracing events (the ops contract suite pins the exact
/// bytes).
async fn run_users(command: OpsCommand) -> Result<(), Box<dyn std::error::Error>> {
    let db = match pidash_db::DbConfig::from_env() {
        Ok(db) => db,
        Err(error) => {
            eprintln!("CommandError: {error}");
            std::process::exit(1);
        }
    };
    let pools = match pidash_db::Pools::connect(&db, None).await {
        Ok(pools) => pools,
        Err(error) => {
            eprintln!("CommandError: {error}");
            std::process::exit(1);
        }
    };
    let stdin = std::io::stdin();
    let mut stdin = stdin.lock();
    let stdout = std::io::stdout();
    let mut stdout = stdout.lock();
    let stderr = std::io::stderr();
    let mut stderr = stderr.lock();
    let mut io = OpsIo {
        stdin: &mut stdin,
        stdout: &mut stdout,
        stderr: &mut stderr,
    };
    let code = match command {
        OpsCommand::ActivateUser(args) => {
            users::run_activate_user(pools.primary(), &args, &mut io).await
        }
        OpsCommand::ResetPassword(args) => {
            users::run_reset_password(pools.primary(), &args, &mut io).await
        }
        OpsCommand::CreateInstanceAdmin(args) => {
            users::run_create_instance_admin(pools.primary(), &args, &mut io).await
        }
        OpsCommand::CreateProjectMember(args) => {
            users::run_create_project_member(pools.primary(), &args, &mut io).await
        }
        OpsCommand::CreateDummyData(args) => {
            users::run_create_dummy_data(pools.primary(), &args, &mut io).await
        }
        _ => unreachable!("run_users only receives users commands"),
    };
    match code {
        Ok(0) => Ok(()),
        Ok(code) => std::process::exit(code),
        Err(error) => error.exit(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Cli, Mode};
    use clap::Parser;

    fn parse_ops(argv: &[&str]) -> OpsCommand {
        let mut full = vec!["pidash-api", "ops"];
        full.extend(argv.iter());
        match Cli::try_parse_from(full)
            .unwrap_or_else(|error| panic!("parses {argv:?}: {error}"))
            .mode
        {
            Mode::Ops { command } => command,
            _ => panic!("wrong mode for {argv:?}"),
        }
    }

    /// Command names are underscored Django names, not kebab-case.
    #[test]
    fn command_names_match_django() {
        for (argv, check) in [
            ("wait_for_db", "WaitForDb"),
            ("wait_for_migrations", "WaitForMigrations"),
            ("clear_cache", "ClearCache"),
            ("create_bucket", "CreateBucket"),
            ("update_bucket", "UpdateBucket"),
        ] {
            let command = parse_ops(&[argv]);
            assert!(
                format!("{command:?}").starts_with(check),
                "{argv} parses to {check}"
            );
        }
        assert!(
            Cli::try_parse_from(["pidash-api", "ops", "wait-for-db"]).is_err(),
            "kebab-case is not a Django command name"
        );
    }

    /// `--key` takes an optional value (`nargs="?"`): absent and bare
    /// both yield the full-clear path, like argparse's `None` const.
    #[test]
    fn clear_cache_key_arg_shapes() {
        assert!(matches!(
            parse_ops(&["clear_cache"]),
            OpsCommand::ClearCache(boot::ClearCacheArgs { key: None })
        ));
        assert!(matches!(
            parse_ops(&["clear_cache", "--key"]),
            OpsCommand::ClearCache(boot::ClearCacheArgs { key: Some(_) })
        ));
        match parse_ops(&["clear_cache", "--key", "k"]) {
            OpsCommand::ClearCache(args) => assert_eq!(args.key.as_deref(), Some("k")),
            _ => panic!("wrong variant"),
        }
    }

    /// Unknown options fail with exit code 2, like argparse.
    #[test]
    fn unknown_option_exits_2() {
        let error = Cli::try_parse_from(["pidash-api", "ops", "clear_cache", "--bogus"])
            .expect_err("rejects");
        assert_eq!(error.exit_code(), 2);
    }
}
