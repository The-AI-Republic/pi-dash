//! Assistant tool access-control parity layer (D-06, stage 5).
//!
//! Ports `apps/api/pi_dash/assistant/tools/_scoping.py:1-103`: the two
//! retry-style error types, the member-project scope, the issue/project
//! resolvers, and the project-write gate. Fixture id F-A6-10
//! (`rust-api/fixtures/assistant/tools-tasks.json`, `scoping` section).
//!
//! Shape notes:
//!
//! * ORM execution (the `Project`/`Issue`/`State` querysets) stays with the
//!   handler layer. This module ports everything around it byte for byte:
//!   the exact denial/missing messages, the role constants, the
//!   [`check_project_role`] decision (including the workspace-admin bypass),
//!   the `my_issues` scope switch (including its fallthrough), and the
//!   malformed-UUID → not-found translation.
//! * Both error kinds subclass `ModelRetry` in Python (`_scoping.py:28-36`)
//!   so pydantic-ai feeds the message back to the model instead of failing
//!   the turn. [`ToolScopeError::is_model_retry`] pins that contract: every
//!   variant reports `true`.
//! * No `schemars` dependency is added (foundation `Cargo.toml` files are
//!   read-only for port agents); tool input schemas live beside each tool
//!   module as `serde_json` values.

use std::fmt;

/// Workspace role: full control (`core/permissions.py:23`).
pub const ROLE_ADMIN: i64 = 20;
/// Workspace role: standard member (`core/permissions.py:24`).
pub const ROLE_MEMBER: i64 = 15;
/// Workspace role: restricted guest (`core/permissions.py:25`).
pub const ROLE_GUEST: i64 = 5;

/// Roles the issue write endpoints (and therefore the write tools) accept
/// (`_scoping.py:92-99`: `[ROLE_ADMIN, ROLE_MEMBER]`, guests blocked).
pub const WRITE_ROLES: [i64; 2] = [ROLE_ADMIN, ROLE_MEMBER];

/// Denial message when a write is attempted without a write role
/// (`_scoping.py:101-103`).
pub const WRITE_DENIED_MESSAGE: &str = "You don't have permission to make changes in this project.";

fn project_not_found_message(project_id: &str) -> String {
    format!("Project {project_id} not found or not accessible.")
}

fn issue_not_found_message(issue_id: &str) -> String {
    format!("Issue {issue_id} not found or not accessible.")
}

/// Tool access errors (`_scoping.py:31-36`).
///
/// Both variants are `ModelRetry` subclasses in Python, so the message is
/// fed back to the model as a retry instead of failing the whole turn.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolScopeError {
    /// The user lacks permission for the action (`ToolPermissionError`).
    Permission(String),
    /// The referenced object is outside the user's scope or missing
    /// (`ToolNotFound`).
    NotFound(String),
}

impl ToolScopeError {
    /// Missing-project error (`get_project`, `_scoping.py:52-56`).
    pub fn project_not_found(project_id: &str) -> Self {
        Self::NotFound(project_not_found_message(project_id))
    }

    /// Missing-issue error (`get_issue`, `_scoping.py:67-84`).
    pub fn issue_not_found(issue_id: &str) -> Self {
        Self::NotFound(issue_not_found_message(issue_id))
    }

    /// Write-gate denial (`require_project_write`, `_scoping.py:92-103`).
    pub fn write_denied() -> Self {
        Self::Permission(WRITE_DENIED_MESSAGE.to_string())
    }

    /// Both variants are `ModelRetry` subclasses, so the message always
    /// goes back to the model as a retry (`_scoping.py:28-30`).
    pub fn is_model_retry(&self) -> bool {
        true
    }

    /// The message surfaced to the model.
    pub fn message(&self) -> &str {
        match self {
            Self::Permission(message) | Self::NotFound(message) => message,
        }
    }
}

impl fmt::Display for ToolScopeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.message())
    }
}

/// Pure half of `check_project_role`
/// (`core/permissions.py:73-116`, called by `require_project_write` with
/// `allow_workspace_admin_bypass=True`):
///
/// * `true` with an active `ProjectMember` row in one of `allowed_roles`;
/// * otherwise `true` when the user is an active member of the project (any
///   role) **and** an active workspace admin;
/// * `None`/anonymous users (either flag `false`) are denied.
pub fn check_project_role(
    has_allowed_role: bool,
    is_project_member: bool,
    is_workspace_admin: bool,
) -> bool {
    if has_allowed_role {
        return true;
    }
    // Workspace-admin bypass (`permissions.py:104-113`).
    if is_project_member && is_workspace_admin {
        return true;
    }
    false
}

/// `my_issues` scope switch (`core/querysets.py:33-52`, via
/// `_scoping.py:63-64`).
///
/// `"assigned"` and `"created"` narrow by that involvement; `"all"` — and,
/// exactly as in Python where any other string falls into the `else`
/// branch, **any unrecognized scope string** — means assigned OR created
/// OR subscribed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IssueScope {
    All,
    Assigned,
    Created,
}

impl IssueScope {
    /// Parse the `scope` argument. Unknown values fall through to `All`,
    /// mirroring the `else` branch in `user_issues_queryset`.
    pub fn parse(scope: &str) -> Self {
        match scope {
            "assigned" => Self::Assigned,
            "created" => Self::Created,
            _ => Self::All,
        }
    }

    /// The wire value passed back to the queryset layer.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::All => "all",
            Self::Assigned => "assigned",
            Self::Created => "created",
        }
    }
}

/// Whether `issue_id` parses as a UUID.
///
/// A malformed value (a human-readable identifier or a hallucinated string)
/// makes Django raise `ValidationError`/`ValueError` at query time, which
/// `get_issue` translates into `ToolNotFound` (`_scoping.py:67-84`).
/// The handler layer applies this check before hitting the database so the
/// model gets the retry message instead of crashing the turn.
pub fn is_well_formed_issue_id(issue_id: &str) -> bool {
    uuid::Uuid::parse_str(issue_id).is_ok()
}

/// Resolve step shared by `get_project` and `get_issue`: a scope miss —
/// including a malformed id — becomes the matching `ToolNotFound` message.
pub fn not_found_for_issue(issue_id: &str) -> ToolScopeError {
    ToolScopeError::issue_not_found(issue_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    fn fixture() -> Value {
        let path = format!(
            "{}/../../fixtures/assistant/tools-tasks.json",
            env!("CARGO_MANIFEST_DIR")
        );
        serde_json::from_str(&std::fs::read_to_string(&path).expect("fixture exists"))
            .expect("fixture parses")
    }

    #[test]
    fn role_constants_match_python() {
        assert_eq!(ROLE_ADMIN, 20);
        assert_eq!(ROLE_MEMBER, 15);
        assert_eq!(ROLE_GUEST, 5);
        assert_eq!(WRITE_ROLES, [20, 15]);
        // Fixture pins the ADMIN/MEMBER-only write rule.
        let scoping = &fixture()["scoping"];
        assert!(scoping["require_project_write"]
            .as_str()
            .unwrap()
            .contains("ADMIN/MEMBER"));
    }

    #[test]
    fn denial_messages_match_python_byte_for_byte() {
        assert_eq!(
            ToolScopeError::project_not_found("abc").to_string(),
            "Project abc not found or not accessible."
        );
        assert_eq!(
            ToolScopeError::issue_not_found("abc").to_string(),
            "Issue abc not found or not accessible."
        );
        assert_eq!(
            ToolScopeError::write_denied().to_string(),
            "You don't have permission to make changes in this project."
        );
    }

    #[test]
    fn both_error_kinds_are_model_retry() {
        assert!(ToolScopeError::write_denied().is_model_retry());
        assert!(ToolScopeError::project_not_found("x").is_model_retry());
        assert!(ToolScopeError::issue_not_found("x").is_model_retry());
        // Fixture pins the ModelRetry-subclass contract.
        assert!(fixture()["scoping"]["error_types"]
            .as_str()
            .unwrap()
            .contains("ModelRetry"));
    }

    #[test]
    fn check_project_role_mirrors_allow_permission() {
        // Direct allowed role.
        assert!(check_project_role(true, false, false));
        // Workspace-admin bypass: member of any role + workspace admin.
        assert!(check_project_role(false, true, true));
        // Member without workspace admin: denied.
        assert!(!check_project_role(false, true, false));
        // Workspace admin but not a project member: denied.
        assert!(!check_project_role(false, false, true));
        // Anonymous: denied.
        assert!(!check_project_role(false, false, false));
    }

    #[test]
    fn scope_switch_with_python_fallthrough() {
        assert_eq!(IssueScope::parse("all"), IssueScope::All);
        assert_eq!(IssueScope::parse("assigned"), IssueScope::Assigned);
        assert_eq!(IssueScope::parse("created"), IssueScope::Created);
        // Python's `else` branch: anything unrecognized behaves as "all".
        assert_eq!(IssueScope::parse("subscribed"), IssueScope::All);
        assert_eq!(IssueScope::parse(""), IssueScope::All);
        assert_eq!(IssueScope::All.as_str(), "all");
    }

    #[test]
    fn malformed_ids_become_not_found() {
        assert!(is_well_formed_issue_id(
            "123e4567-e89b-12d3-a456-426614174000"
        ));
        assert!(!is_well_formed_issue_id("PIDASHCONV-252"));
        assert!(!is_well_formed_issue_id("not a uuid"));
        assert!(!is_well_formed_issue_id(""));
        assert_eq!(
            not_found_for_issue("PIDASHCONV-252").to_string(),
            "Issue PIDASHCONV-252 not found or not accessible."
        );
    }
}
