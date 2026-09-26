//! Explicit per-request audit and tenant context.
//!
//! Mirrors two Django facts. First, every write stamps the audit mixins
//! (`TimeAuditModel` / `UserAuditModel` in `pi_dash/db/mixins.py`):
//! `created_at` / `updated_at` are server-set, `created_by` / `updated_by`
//! are nullable foreign keys to the acting user. Second, every write runs
//! inside a workspace: handlers receive the tenant on the request, never
//! through a global.
//!
//! [`RequestContext`] carries both. Write entry points ([`ScopedWrites`])
//! require it, so a handler cannot obtain an unscoped database handle.

use pidash_types::{UserId, WorkspaceId};

/// Who is acting and in which workspace, for one request.
///
/// `actor_id` is `None` for system/anonymous writes; the audit columns
/// stay NULL then, mirroring `null=True` on `created_by` / `updated_by`.
/// `use_read_replica` mirrors the `use_read_replica = True` attribute on
/// read-only DRF views: an explicit per-view opt-in, defaulting to the
/// primary (the safe default, as in the routing middleware).
#[derive(Debug, Clone)]
pub struct RequestContext {
    workspace_id: WorkspaceId,
    actor_id: Option<UserId>,
    use_read_replica: bool,
}

impl RequestContext {
    pub fn new(workspace_id: WorkspaceId, actor_id: Option<UserId>) -> Self {
        Self {
            workspace_id,
            actor_id,
            use_read_replica: false,
        }
    }

    /// Opt this request's reads into the replica pool.
    pub fn with_replica(mut self, allow: bool) -> Self {
        self.use_read_replica = allow;
        self
    }

    pub fn workspace_id(&self) -> &WorkspaceId {
        &self.workspace_id
    }

    pub fn actor_id(&self) -> Option<&UserId> {
        self.actor_id.as_ref()
    }

    pub fn use_read_replica(&self) -> bool {
        self.use_read_replica
    }

    /// The value for `created_by` / `updated_by` audit columns.
    pub fn audit_actor(&self) -> Option<&str> {
        self.actor_id.as_ref().map(|id| id.0.as_str())
    }
}

/// A write handle bound to one request's context.
///
/// Constructing this is the only way to reach the primary pool for
/// writes: the pool alone is never handed out for writes without a
/// context attached.
#[derive(Debug)]
pub struct ScopedWrites<'a> {
    primary: &'a sqlx::PgPool,
    context: &'a RequestContext,
}

impl<'a> ScopedWrites<'a> {
    pub fn new(primary: &'a sqlx::PgPool, context: &'a RequestContext) -> Self {
        Self { primary, context }
    }

    pub fn pool(&self) -> &'a sqlx::PgPool {
        self.primary
    }

    pub fn context(&self) -> &'a RequestContext {
        self.context
    }

    pub async fn begin(&self) -> Result<crate::tx::Transaction<'a>, sqlx::Error> {
        crate::tx::Transaction::begin(self.primary).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx() -> RequestContext {
        RequestContext::new(WorkspaceId::from("ws-1"), Some(UserId::from("u-9")))
    }

    #[test]
    fn replica_opt_in_defaults_off() {
        assert!(!ctx().use_read_replica());
        assert!(ctx().with_replica(true).use_read_replica());
    }

    #[test]
    fn system_writes_carry_no_actor() {
        let system = RequestContext::new(WorkspaceId::from("ws-1"), None);
        assert_eq!(system.audit_actor(), None);
        assert_eq!(ctx().audit_actor(), Some("u-9"));
    }
}
