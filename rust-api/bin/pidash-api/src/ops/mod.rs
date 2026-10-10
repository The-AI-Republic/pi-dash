//! `pidash-api ops`: Django management-command ports (D-37, PIDASHCONV-74).
//!
//! One module per command issue; each issue owns its `mod` line, its enum
//! variants and its match arms below, and nothing else. Groups stay in the
//! order boot, users, repair, instance, prompting so sibling rebases stay
//! mechanical. Subcommand names are the Django command names verbatim so
//! runbooks transfer unchanged.

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
}

/// Run one `ops` subcommand. Expected failures print Django's exact bytes
/// and exit 1 from inside the group runner; this returns only on success.
pub async fn run(command: OpsCommand) {
    match command {
        // --- repair (PIDASHCONV-808) ---
        OpsCommand::CopyIssueCommentToDescription => repair::run_copy().await,
        OpsCommand::FixDuplicateSequences(args) => repair::run_fix(args).await,
        OpsCommand::SyncIssueVersion => repair::run_sync_version().await,
        OpsCommand::SyncIssueDescriptionVersion => repair::run_sync_description_version().await,
        OpsCommand::UpdateDeletedWorkspaceSlug(args) => repair::run_slug(args).await,
    }
}
