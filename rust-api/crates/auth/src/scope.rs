//! Tenant scope: the workspace an access acts in.
//!
//! Primitive for the F-06 permission kernel. Comparison is deny-by-default:
//! two scopes authorize each other only when their workspace ids match
//! exactly. There is no "global" scope that passes every check.

use pidash_types::WorkspaceId;
use thiserror::Error as ThisError;

/// The workspace a request is scoped to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TenantScope {
    workspace_id: WorkspaceId,
}

impl TenantScope {
    pub fn new(workspace_id: WorkspaceId) -> Self {
        Self { workspace_id }
    }

    pub fn workspace_id(&self) -> &WorkspaceId {
        &self.workspace_id
    }

    /// True when `other` names the same workspace.
    pub fn authorizes(&self, other: &TenantScope) -> Result<(), ScopeError> {
        if self.workspace_id == other.workspace_id {
            Ok(())
        } else {
            Err(ScopeError::CrossWorkspace)
        }
    }
}

/// Why an access was refused at the scope check.
#[derive(Debug, Clone, PartialEq, Eq, ThisError)]
pub enum ScopeError {
    #[error("access outside the request workspace is denied")]
    CrossWorkspace,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scope(id: &str) -> TenantScope {
        TenantScope::new(WorkspaceId::from(id))
    }

    #[test]
    fn same_workspace_authorizes() {
        assert!(scope("ws-1").authorizes(&scope("ws-1")).is_ok());
    }

    #[test]
    fn other_workspace_is_denied() {
        assert_eq!(
            scope("ws-1").authorizes(&scope("ws-2")).unwrap_err(),
            ScopeError::CrossWorkspace
        );
    }
}
