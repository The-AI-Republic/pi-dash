//! `pidash-api ops`: Django management-command ports (D-37, PIDASHCONV-74).
//!
//! One module per command issue; each issue owns its `mod` line, its enum
//! variants and its match arms below, and nothing else. Groups stay in the
//! order boot, users, repair, instance, prompting so sibling rebases stay
//! mechanical. Subcommand names are the Django command names verbatim so
//! runbooks transfer unchanged.
//!
//! * [`users`] — the users + membership commands (PIDASHCONV-807).
//! * [`repair`] — the data-repair commands (PIDASHCONV-808).
//! * [`prompting`] — the prompting reseed + revalidate commands
//!   (PIDASHCONV-810).

pub mod prompting;
pub mod repair;
pub mod users;

use clap::Subcommand;

/// Management commands, grouped by owning issue.
#[derive(Debug, Subcommand)]
#[command(rename_all = "snake_case")]
pub enum OpsCommand {
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

/// Run one `ops` subcommand.
///
/// The two groups keep their own failure conventions: repair runners print
/// Django's exact bytes and exit 1 from inside the group runner, while
/// prompting runners return `Err` (a boot/DB failure `main` renders as a
/// CLI boot error, exit 1 — there is no Django-oracle shape for failures,
/// only for the documented matrices). Success prints Django's lines to
/// stdout and returns `Ok` (exit 0). Users runners need exit codes
/// and injected stdio, so they dispatch through [`run_users`], which
/// exits from inside on failure (the repair precedent) and returns
/// `Ok` on success.
pub async fn run(command: OpsCommand) -> Result<(), Box<dyn std::error::Error>> {
    use pidash_services::ops::prompting::TemplateKind;
    match command {
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
