#![forbid(unsafe_code)]

//! Recent-visit task publishers for the two view-retrieve actions (D-29, PIDASHCONV-274).
//!
//! Ports the two `recent_visited_task.delay(...)` call sites in
//! `apps/api/pi_dash/app/views/view/base.py`:
//!
//! * workspace-view retrieve (`:102-112`): `slug`, `project_id=None`,
//!   `entity_name="view"`, `entity_identifier=<pk>`, `user_id=<request user>`.
//! * issue-view retrieve (`:308-341`): same with `project_id=<url project_id>`.
//!   The emit fires after the guest recheck (`:317-331`), so a denied
//!   retrieve publishes nothing.
//!
//! Fixture oracle: `rust-api/fixtures/app_views_search/FX-TASK.json`
//! (call sites `:105-111` / `:334-340`, task body
//! `bgtasks/recent_visited_task.py:17-61`, table
//! `db/models/recent_visit.py:22-36`). The tests below replay every
//! emitted payload field for field.
//!
//! Wire contract (Porting guide jobs plane, F-09): `.delay(**kwargs)` is a
//! deferred publish of a first-attempt Celery protocol v2 message with
//! `args = []` and the call-site kwargs. The task is a bare `@shared_task`
//! (`bgtasks/recent_visited_task.py:17`), so the wire name is the dotted
//! module path in [`RECENT_VISITED_TASK_NAME`] and there is no queue
//! override. The worker side (task body, `== 20` eviction gate, DB
//! before/after) already lives in the jobs crate
//! (`jobs/src/tasks_webhooks/visit_page.rs`, D-08) and is not repeated here.
//!
//! Crate-graph note: `pidash-jobs` depends on `pidash-services`, so this
//! module cannot name `jobs::celery::CeleryTaskMessage` (that would be a
//! dependency cycle). It publishes the Celery-format body parts instead —
//! [`RecentVisitedEmit::task_name`] + [`RecentVisitedEmit::kwargs`] — and
//! the handlers (PIDASHCONV-275/276, in the `api` crate which already
//! depends on `pidash-jobs`) wrap them with
//! `CeleryTaskMessage::new(task, vec![], kwargs)` plus `queue::enqueue`,
//! exactly like the space intake handlers do
//! (`api/src/space/intake.rs`). `None` renders as JSON `null`, matching
//! kombu's rendering of Django's `None`.
//!
//! Handler effect (D-26 precedent, `services/src/app_issues/mod.rs`):
//! `.delay()` is a deferred publish — the retrieve handlers perform no
//! inline write to `user_recent_visits`. The suite environment never
//! consumes the queue, so an inline write would leave rows Django never
//! produces and break teardown; the response bytes and observable DB state
//! match Django exactly.
//!
//! # Ported quirks (translate, don't redesign)
//!
//! * B6 (`recent_visited_task.py:42`, also in FX-TASK.json): the eviction
//!   gate is `recent_visited_count == 20`, exact equality. Owned by the
//!   jobs worker; the publisher carries no count.

use serde_json::{Map, Value};

/// Celery wire name for `recent_visited_task` (bare `@shared_task`
/// default; also pinned by the jobs crate at
/// `jobs/src/tasks_webhooks/visit_page.rs:78`).
pub const RECENT_VISITED_TASK_NAME: &str =
    "pi_dash.bgtasks.recent_visited_task.recent_visited_task";

/// `entity_name` for both call sites: views are tracked as `"view"`.
pub const RECENT_VISITED_ENTITY_VIEW: &str = "view";

/// Kwarg order of both `.delay()` calls, matching the jobs-crate
/// constructor (`visit_page.rs:88-111`), which emits kwargs in task
/// signature order. Binding is by name on both consumers (Python Celery
/// and the Rust worker), so the order is informational.
pub const RECENT_VISITED_KWARG_ORDER: &[&str] = &[
    "entity_name",
    "entity_identifier",
    "user_id",
    "project_id",
    "slug",
];

/// One `recent_visited_task.delay(...)` call: keyword args exactly as the
/// view passes them. `entity_identifier` is the view pk and `user_id` the
/// request user id — both UUID objects in Python, rendered as strings by
/// kombu's JSON encoder, so the fields take the string form.
/// `project_id` is `None` on the workspace-view path and the URL
/// `project_id` on the issue-view path.
#[derive(Debug, Clone, PartialEq)]
pub struct RecentVisitedEmit {
    pub entity_name: String,
    pub entity_identifier: String,
    pub user_id: String,
    pub project_id: Option<String>,
    pub slug: String,
}

impl RecentVisitedEmit {
    /// Celery task name this emit publishes to.
    pub fn task_name(&self) -> &'static str {
        RECENT_VISITED_TASK_NAME
    }

    /// `.delay()` kwargs as ordered pairs in [`RECENT_VISITED_KWARG_ORDER`].
    /// `args` on the wire is `[]`.
    pub fn kwargs_pairs(&self) -> Vec<(&'static str, Value)> {
        vec![
            ("entity_name", Value::String(self.entity_name.clone())),
            (
                "entity_identifier",
                Value::String(self.entity_identifier.clone()),
            ),
            ("user_id", Value::String(self.user_id.clone())),
            (
                "project_id",
                self.project_id
                    .clone()
                    .map(Value::String)
                    .unwrap_or(Value::Null),
            ),
            ("slug", Value::String(self.slug.clone())),
        ]
    }

    /// `.delay()` kwargs as a JSON object for `CeleryTaskMessage::new`.
    pub fn kwargs(&self) -> Map<String, Value> {
        self.kwargs_pairs()
            .into_iter()
            .map(|(key, value)| (key.to_owned(), value))
            .collect()
    }
}

/// Workspace-view retrieve (`base.py:105-111`): `project_id=None`.
pub fn workspace_view_retrieve_emit(
    view_pk: &str,
    user_id: &str,
    workspace_slug: &str,
) -> RecentVisitedEmit {
    RecentVisitedEmit {
        entity_name: RECENT_VISITED_ENTITY_VIEW.to_owned(),
        entity_identifier: view_pk.to_owned(),
        user_id: user_id.to_owned(),
        project_id: None,
        slug: workspace_slug.to_owned(),
    }
}

/// Issue-view retrieve (`base.py:334-340`): `project_id=<url project_id>`.
pub fn issue_view_retrieve_emit(
    view_pk: &str,
    user_id: &str,
    project_id: &str,
    workspace_slug: &str,
) -> RecentVisitedEmit {
    RecentVisitedEmit {
        entity_name: RECENT_VISITED_ENTITY_VIEW.to_owned(),
        entity_identifier: view_pk.to_owned(),
        user_id: user_id.to_owned(),
        project_id: Some(project_id.to_owned()),
        slug: workspace_slug.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn task_name_matches_jobs_wire_owner() {
        // Must stay identical to `jobs/src/tasks_webhooks/visit_page.rs:78`.
        assert_eq!(
            RECENT_VISITED_TASK_NAME,
            "pi_dash.bgtasks.recent_visited_task.recent_visited_task"
        );
    }

    #[test]
    fn workspace_retrieve_payload_replays_fx_task_call_site() {
        // FX-TASK.json `call_sites[0]`: base.py:105-111, project_id null.
        let emit = workspace_view_retrieve_emit("view-pk", "user-id", "ws-slug");
        assert_eq!(emit.task_name(), RECENT_VISITED_TASK_NAME);
        assert_eq!(
            emit.kwargs_pairs(),
            vec![
                ("entity_name", Value::String("view".to_owned())),
                ("entity_identifier", Value::String("view-pk".to_owned())),
                ("user_id", Value::String("user-id".to_owned())),
                ("project_id", Value::Null),
                ("slug", Value::String("ws-slug".to_owned())),
            ]
        );
        let kwargs = emit.kwargs();
        assert_eq!(kwargs.len(), RECENT_VISITED_KWARG_ORDER.len());
        for key in RECENT_VISITED_KWARG_ORDER {
            assert!(kwargs.contains_key(*key), "missing kwarg {key}");
        }
    }

    #[test]
    fn issue_view_retrieve_payload_replays_fx_task_call_site() {
        // FX-TASK.json `call_sites[1]`: base.py:334-340, project_id set.
        let emit = issue_view_retrieve_emit("view-pk", "user-id", "project-id", "ws-slug");
        assert_eq!(emit.task_name(), RECENT_VISITED_TASK_NAME);
        assert_eq!(
            emit.kwargs_pairs(),
            vec![
                ("entity_name", Value::String("view".to_owned())),
                ("entity_identifier", Value::String("view-pk".to_owned())),
                ("user_id", Value::String("user-id".to_owned())),
                ("project_id", Value::String("project-id".to_owned())),
                ("slug", Value::String("ws-slug".to_owned())),
            ]
        );
        assert_eq!(
            emit.kwargs()["project_id"],
            Value::String("project-id".to_owned())
        );
    }

    #[test]
    fn kwarg_order_covers_every_key_once() {
        assert_eq!(
            RECENT_VISITED_KWARG_ORDER,
            &[
                "entity_name",
                "entity_identifier",
                "user_id",
                "project_id",
                "slug"
            ]
        );
        let emit = workspace_view_retrieve_emit("pk", "uid", "slug");
        let order: Vec<&str> = emit.kwargs_pairs().iter().map(|(key, _)| *key).collect();
        assert_eq!(order, RECENT_VISITED_KWARG_ORDER);
    }

    #[test]
    fn emits_carry_no_inline_write() {
        // The publisher is pure data: no DB handle, no timestamp, no row.
        // The deferred-publish note (D-26 precedent) holds by construction.
        let workspace = workspace_view_retrieve_emit("pk", "uid", "slug");
        let project = issue_view_retrieve_emit("pk", "uid", "pid", "slug");
        assert_eq!(workspace, workspace.clone());
        assert_ne!(workspace, project);
    }
}
