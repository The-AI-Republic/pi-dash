//! Issue app domain surface (D-26, stage 5).
//!
//! Ports `apps/api/pi_dash/db/models/` for the db layer, bottom-up:
//!
//! * [`models_core`] — `Issue`, `IssueAssignee`, `IssueLabel`, `Label`,
//!   `ProjectUserProperty` (PIDASHCONV-644, creates this module).
//!   Reads, serializers, guards, tasks and handlers belong to the sibling
//!   D-26 issues; the domain gate is PIDASHCONV-658.
//!
//! * [`models_links`] — `IssueLink`, `IssueRelation`,
//!   `GithubPullRequestLink`, `GitCodeReviewLink`, `FileAsset` (reuse)
//!   (PIDASHCONV-645).
//!
//! * [`models_engage`] — `IssueComment`, `CommentReaction`,
//!   `IssueReaction`, `IssueVote`, `IssueSubscriber` (PIDASHCONV-646).
//!
//! The sibling models issue (PIDASHCONV-647) adds its own `models_*.rs`
//! file here and re-exports its row structs below next to
//! [`models_core`]'s.

pub mod models_core;
pub mod models_engage;
pub mod models_links;

pub use models_core::{
    issue::Issue, issue_assignee::IssueAssignee, issue_label::IssueLabel, label::Label,
    project_user_property::ProjectUserProperty, OnDelete,
};
pub use models_engage::{
    comment_reaction::CommentReaction, issue_comment::IssueComment, issue_reaction::IssueReaction,
    issue_subscriber::IssueSubscriber, issue_vote::IssueVote,
};
pub use models_links::{
    file_asset::FileAsset, git_code_review_link::GitCodeReviewLink,
    github_pull_request_link::GithubPullRequestLink, issue_link::IssueLink,
    issue_relation::IssueRelation,
};
