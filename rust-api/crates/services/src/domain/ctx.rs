//! What a domain write runs as, where it came from, and what it may touch.

use uuid::Uuid;

/// The acting user: the `crum` user equivalent. Django stamps
/// `created_by`/`updated_by` from `request.user` (via `crum`); domain
/// writes stamp from this instead. Mirrors the per-module `Actor` shapes in
/// `pidash-api` (`id` plus the stored time zone name, loaded but not
/// parsed), lifted to the services layer so overlays can build one without
/// depending on the HTTP crate.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct WriteActor {
    pub user_id: Uuid,
    pub timezone: Option<String>,
}

/// The tenant scope of the write: the workspace and project the caller
/// already resolved and gated. `None` on system paths that span scopes
/// (sweeps, seeds).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct WriteScope {
    pub workspace_id: Option<Uuid>,
    pub project_id: Option<Uuid>,
}

/// Who is acting and on whose behalf, typed rather than read from an
/// untrusted request body (NEWPIDASH-190 design §4.2): the request
/// path, an MCP token, a cloud-agent run, or an assistant thread.
/// Speaker fields and run markers stop travelling through payloads;
/// every write carries an attributable origin on its `WriteCtx`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum WriteSource {
    /// The HTTP request path (default).
    #[default]
    Api,
    /// An MCP capability call (overlay token auth).
    Mcp,
    /// A cloud-agent run (`agent_run_tool_call` ledger leg).
    CloudAgent {
        /// The run that owns the write (speaker run, admission scope).
        run_id: Uuid,
    },
    /// An assistant chat thread.
    Assistant {
        /// The thread the write belongs to.
        thread_id: Uuid,
    },
}

/// Per-caller deltas (design D6). Where Django views do different things
/// for the same write, the difference becomes an explicit option here with
/// a doc comment citing the Python file:line — never a silent branch.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct WriteOptions {
    /// Which surface the write came from (NEWPIDASH-190 §4.2).
    pub source: WriteSource,
}

/// The context one [`crate::domain::run`] call runs under: actor, request
/// origin/host, tenant scope, and per-caller options.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct WriteCtx {
    pub actor: WriteActor,
    /// Request origin/host for the `model_activity` `origin` kwarg
    /// (`APP_BASE_URL`, else `WEB_URL`, else the host; `None` when the
    /// caller has no request) — worker paths pass `None`, like Django's
    /// missing-request default.
    pub origin: Option<String>,
    pub scope: WriteScope,
    pub options: WriteOptions,
}

impl WriteCtx {
    pub fn new(actor: WriteActor) -> Self {
        Self {
            actor,
            origin: None,
            scope: WriteScope::default(),
            options: WriteOptions::default(),
        }
    }

    pub fn with_origin(mut self, origin: impl Into<String>) -> Self {
        self.origin = Some(origin.into());
        self
    }

    pub fn with_scope(mut self, scope: WriteScope) -> Self {
        self.scope = scope;
        self
    }

    pub fn with_options(mut self, options: WriteOptions) -> Self {
        self.options = options;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builder_sets_origin_and_scope() {
        let actor = WriteActor {
            user_id: Uuid::nil(),
            timezone: Some("UTC".to_owned()),
        };
        let ctx = WriteCtx::new(actor.clone())
            .with_origin("https://app.example")
            .with_scope(WriteScope {
                workspace_id: Some(Uuid::nil()),
                project_id: None,
            });
        assert_eq!(ctx.actor, actor);
        assert_eq!(ctx.origin.as_deref(), Some("https://app.example"));
        assert_eq!(ctx.scope.workspace_id, Some(Uuid::nil()));
        assert_eq!(ctx.scope.project_id, None);
    }

    #[test]
    fn default_ctx_is_anonymous_system() {
        let ctx = WriteCtx::default();
        assert_eq!(ctx.actor.user_id, Uuid::nil());
        assert!(ctx.origin.is_none());
        assert!(ctx.scope.workspace_id.is_none());
    }

    #[test]
    fn default_options_carry_api_source() {
        assert_eq!(WriteOptions::default().source, WriteSource::Api);
        assert_eq!(WriteSource::default(), WriteSource::Api);
        let options = WriteOptions {
            source: WriteSource::CloudAgent {
                run_id: Uuid::nil(),
            },
        };
        assert_eq!(
            options.source,
            WriteSource::CloudAgent {
                run_id: Uuid::nil()
            }
        );
    }
}
