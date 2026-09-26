//! Primary/replica pools with Django-equivalent read routing.
//!
//! Mirrors `pi_dash.utils.core.dbrouters.ReadReplicaRouter`,
//! `pi_dash.utils.core.request_scope` and
//! `pi_dash.middleware.db_routing.ReadReplicaRoutingMiddleware`:
//!
//! - Writes always go to the primary (`default`); the router's
//!   `db_for_write` unconditionally returns `"default"`.
//! - Reads go to the replica only when the request opted in *and* a
//!   replica pool exists; otherwise they stay on the primary. The opt-in
//!   is explicit per request ([`RequestContext::with_replica`], the mirror
//!   of the view's `use_read_replica = True` attribute) or ambient for
//!   the current task ([`replica_scope`], the mirror of the middleware's
//!   `set_use_read_replica`).
//! - Migrations run on the primary only: [`Pools`] exposes no replica
//!   handle for schema work (the router's `allow_migrate` is true solely
//!   for `"default"`).
//!
//! The ambient flag defaults to the primary when no scope is set, exactly
//! like `should_use_read_replica()` returning `False` with no context.

use sqlx::postgres::{PgPool, PgPoolOptions};

use crate::config::DbConfig;
use crate::context::RequestContext;

tokio::task_local! {
    static USE_READ_REPLICA: bool;
}

/// Run `fut` with the ambient read-replica flag set.
///
/// The scope ends when the future resolves, mirroring the middleware's
/// `finally: clear_read_replica_context()` cleanup: the flag cannot leak
/// into the next request on the same task.
pub async fn replica_scope<F, T>(allow: bool, fut: F) -> T
where
    F: std::future::Future<Output = T>,
{
    USE_READ_REPLICA.scope(allow, fut).await
}

/// Ambient replica opt-in for the current task. False outside any scope.
pub fn should_use_read_replica() -> bool {
    USE_READ_REPLICA.try_get().unwrap_or(false)
}

/// Which pool a statement runs against.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Route {
    Primary,
    Replica,
}

/// True when the HTTP method performs writes, mirroring the middleware's
/// `READ_ONLY_METHODS = {"GET", "HEAD", "OPTIONS"}`: anything else routes
/// to the primary before the view is even resolved.
pub fn is_write_method(method: &str) -> bool {
    !matches!(method, "GET" | "HEAD" | "OPTIONS")
}

/// Pure routing decision, unit-testable without a database.
///
/// `is_write` always routes primary. Reads route to the replica only
/// when the request opted in (explicitly on its context or ambiently on
/// the task) and a replica pool is configured.
pub fn route_for(is_write: bool, ctx: &RequestContext, replica_available: bool) -> Route {
    if is_write {
        return Route::Primary;
    }
    if replica_available && (ctx.use_read_replica() || should_use_read_replica()) {
        Route::Replica
    } else {
        Route::Primary
    }
}

/// Primary pool plus an optional read-replica pool.
#[derive(Debug, Clone)]
pub struct Pools {
    primary: PgPool,
    replica: Option<PgPool>,
}

impl Pools {
    /// Connect both pools. `replica` is `None` when no read replica is
    /// configured (the `ENABLE_READ_REPLICA != "1"` case), in which case
    /// every statement runs on the primary.
    pub async fn connect(
        primary: &DbConfig,
        replica: Option<&DbConfig>,
    ) -> Result<Self, sqlx::Error> {
        let primary = pool_options(primary).connect(primary.url()).await?;
        let mut replica_pool = None;
        if let Some(cfg) = replica {
            replica_pool = Some(pool_options(cfg).connect(cfg.url()).await?);
        }
        Ok(Self {
            primary,
            replica: replica_pool,
        })
    }

    pub fn primary(&self) -> &PgPool {
        &self.primary
    }

    pub fn replica(&self) -> Option<&PgPool> {
        self.replica.as_ref()
    }

    pub fn has_replica(&self) -> bool {
        self.replica.is_some()
    }

    /// The pool for one request's statement. Reads use the replica only
    /// when [`route_for`] says so; anything else falls back to primary.
    pub fn pool_for(&self, ctx: &RequestContext, is_write: bool) -> &PgPool {
        match route_for(is_write, ctx, self.has_replica()) {
            Route::Primary => &self.primary,
            Route::Replica => self.replica.as_ref().unwrap_or(&self.primary),
        }
    }

    /// Write handle bound to a request context. The only path to the
    /// primary for writes.
    pub fn writes<'a>(&'a self, ctx: &'a RequestContext) -> crate::context::ScopedWrites<'a> {
        crate::context::ScopedWrites::new(&self.primary, ctx)
    }
}

fn pool_options(cfg: &DbConfig) -> PgPoolOptions {
    PgPoolOptions::new().max_connections(cfg.max_connections())
}

#[cfg(test)]
mod tests {
    use super::*;
    use pidash_types::{UserId, WorkspaceId};

    fn ctx(replica: bool) -> RequestContext {
        RequestContext::new(WorkspaceId::from("ws-1"), Some(UserId::from("u-1")))
            .with_replica(replica)
    }

    #[test]
    fn only_get_head_options_count_as_reads() {
        for method in ["GET", "HEAD", "OPTIONS"] {
            assert!(!is_write_method(method), "{method}");
        }
        for method in ["POST", "PUT", "PATCH", "DELETE"] {
            assert!(is_write_method(method), "{method}");
        }
    }

    #[test]
    fn writes_always_route_primary() {
        assert_eq!(route_for(true, &ctx(true), true), Route::Primary);
        assert_eq!(route_for(true, &ctx(false), false), Route::Primary);
    }

    #[test]
    fn reads_default_to_primary() {
        assert_eq!(route_for(false, &ctx(false), true), Route::Primary);
        assert_eq!(route_for(false, &ctx(true), false), Route::Primary);
    }

    #[test]
    fn opted_in_reads_use_replica_when_configured() {
        assert_eq!(route_for(false, &ctx(true), true), Route::Replica);
    }

    #[tokio::test]
    async fn ambient_scope_opts_reads_in_and_clears_after() {
        assert!(!should_use_read_replica());
        replica_scope(true, async {
            assert!(should_use_read_replica());
            assert_eq!(route_for(false, &ctx(false), true), Route::Replica);
        })
        .await;
        assert!(!should_use_read_replica());
        assert_eq!(route_for(false, &ctx(false), true), Route::Primary);
    }

    #[tokio::test]
    async fn false_scope_stays_on_primary() {
        replica_scope(false, async {
            assert_eq!(route_for(false, &ctx(false), true), Route::Primary);
        })
        .await;
    }
}
