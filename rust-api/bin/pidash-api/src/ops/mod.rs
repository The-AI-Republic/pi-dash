//! `pidash-api ops`: Django management-command ports (D-37, PIDASHCONV-74).
//!
//! One module per command issue; each issue owns its `mod` line, its enum
//! variants and its match arms below, and nothing else. Groups stay in the
//! order boot, users, repair, instance, prompting so sibling rebases stay
//! mechanical. Subcommand names are the Django command names verbatim so
//! runbooks transfer unchanged.
//!
//! * [`repair`] — the data-repair commands (PIDASHCONV-808).
//! * [`prompting`] — the prompting reseed + revalidate commands
//!   (PIDASHCONV-810).

pub mod prompting;
pub mod repair;

use clap::Subcommand;

/// Management commands, grouped by owning issue.
#[derive(Debug, Subcommand)]
pub enum OpsCommand {
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
/// stdout and returns `Ok` (exit 0).
pub async fn run(command: OpsCommand) -> Result<(), Box<dyn std::error::Error>> {
    use pidash_services::ops::prompting::TemplateKind;
    match command {
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
