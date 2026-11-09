//! The issue post-save orchestration, reachable from outside the api crate.
//!
//! `capture_prior_state_pool` moved here verbatim from
//! `pidash-api::issue_transition`; the api module re-exports it, so its
//! call sites keep working. The completion comment and the transition fire
//! need jobs code, so they live in [`pidash_jobs`] — out of reach of this
//! crate (the crate graph is `types` → `db` → `services`, and jobs sits
//! above services). [`IssueTransitionHook`] bridges that edge: the binary
//! that owns the dependencies implements it, the caller registers the
//! implementation on [`crate::domain::HookRegistry`], and domain services
//! invoke it from their after-commit steps.

use std::future::Future;
use std::pin::Pin;

use sqlx::PgPool;
use uuid::Uuid;

use pidash_db::entity::issues;
use sea_orm::{EntityTrait, QuerySelect};

use crate::orchestration::entries::{FireOutcome, FireRequest};

/// The pre-save `state_id` snapshot (`capture_prior_state`,
/// `signals.py:61-71`, the [`PRIOR_STATE_SELECT_SQL`] read) against the
/// pool: `None` for a new instance or a missing row. By primary key with
/// no soft-delete scope, like the constant's text.
///
/// [`PRIOR_STATE_SELECT_SQL`]: crate::orchestration::entries::PRIOR_STATE_SELECT_SQL
pub async fn capture_prior_state_pool(
    pool: &PgPool,
    issue_pk: Option<Uuid>,
) -> Result<Option<Uuid>, sqlx::Error> {
    let Some(issue_pk) = issue_pk else {
        return Ok(None);
    };
    let db = pidash_db::pool::sea_orm_connection(pool);
    let state: Option<Option<Uuid>> = prior_state_select(issue_pk)
        .into_tuple()
        .one(&db)
        .await
        .map_err(pidash_db::orm_tx::into_sqlx)?;
    Ok(state.flatten())
}

/// `SELECT state_id FROM issues WHERE id = $1`.
fn prior_state_select(issue_pk: Uuid) -> sea_orm::Select<issues::Entity> {
    issues::Entity::find_by_id(issue_pk)
        .select_only()
        .column(issues::Column::StateId)
}

/// The post-save orchestration as an object-safe hook (the
/// [`PodRunnerMatcher`](crate::orchestration::dispatch::PodRunnerMatcher)
/// precedent: boxed futures, no generics). The api binary implements it
/// over its transition entry points; domain services call it from
/// after-commit steps without depending on the api crate.
pub trait IssueTransitionHook: Send + Sync {
    /// Post-commit completion comment-back for a state-changing save
    /// (`github_signals.py:44-75`). Best-effort: failures only warn, the
    /// save stands.
    fn enqueue_completion_comment(
        &self,
        issue_id: Uuid,
        prev_state_id: Option<Uuid>,
        current_state_id: Option<Uuid>,
    ) -> Pin<Box<dyn Future<Output = ()> + Send + '_>>;

    /// Run the post-save transition fire for one issue. A non-transition
    /// answers [`FireOutcome::NoTransition`]; a handler failure rolls the
    /// driver's creation transaction back and answers
    /// [`FireOutcome::Failed`] — the save itself stands either way.
    fn fire(
        &self,
        actor_id: Uuid,
        request: FireRequest,
    ) -> Pin<Box<dyn Future<Output = Result<FireOutcome, ()>> + Send + '_>>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    type CommentCall = (Uuid, Option<Uuid>, Option<Uuid>);

    struct RecordingHook {
        comments: Mutex<Vec<CommentCall>>,
        fires: Mutex<Vec<Uuid>>,
    }

    impl IssueTransitionHook for RecordingHook {
        fn enqueue_completion_comment(
            &self,
            issue_id: Uuid,
            prev_state_id: Option<Uuid>,
            current_state_id: Option<Uuid>,
        ) -> Pin<Box<dyn Future<Output = ()> + Send + '_>> {
            self.comments
                .lock()
                .expect("lock")
                .push((issue_id, prev_state_id, current_state_id));
            Box::pin(async move {})
        }

        fn fire(
            &self,
            actor_id: Uuid,
            _request: FireRequest,
        ) -> Pin<Box<dyn Future<Output = Result<FireOutcome, ()>> + Send + '_>> {
            self.fires.lock().expect("lock").push(actor_id);
            Box::pin(async move { Ok(FireOutcome::NoTransition) })
        }
    }

    #[test]
    fn prior_state_select_shape() {
        use sea_orm::QueryTrait;
        assert_eq!(
            prior_state_select(Uuid::nil())
                .build(sea_orm::DbBackend::Postgres)
                .to_string(),
            "SELECT \"issues\".\"state_id\" FROM \"issues\" WHERE \"issues\".\"id\" = '00000000-0000-0000-0000-000000000000'"
        );
    }

    #[tokio::test]
    async fn hook_registry_carries_the_transition_hook() {
        let hook = Arc::new(RecordingHook {
            comments: Mutex::new(Vec::new()),
            fires: Mutex::new(Vec::new()),
        });
        let mut registry = crate::domain::HookRegistry::new();
        let erased: Arc<dyn IssueTransitionHook> = hook.clone();
        registry.register(erased);
        let found = registry
            .get::<dyn IssueTransitionHook>()
            .expect("registered hook");
        let issue_id = Uuid::nil();
        found
            .enqueue_completion_comment(issue_id, None, Some(issue_id))
            .await;
        let request = FireRequest {
            issue_id,
            prev_state_id: None,
            current_state_id: None,
            dispatch_immediate: false,
            moved_by_run: None,
            now: chrono::Utc::now(),
            jitter_secs: 0.0,
            created_by: None,
            pod_once: false,
        };
        let outcome = found.fire(Uuid::nil(), request).await;
        assert!(matches!(outcome, Ok(FireOutcome::NoTransition)));
        assert_eq!(
            *hook.comments.lock().expect("lock"),
            vec![(issue_id, None, Some(issue_id))]
        );
        assert_eq!(*hook.fires.lock().expect("lock"), vec![Uuid::nil()]);
    }

    /// Live: the prior-state read against a session temp copy of
    /// `issues` on a one-connection pool (the temp table shadows
    /// `public.issues` for that connection only).
    #[tokio::test]
    async fn live_capture_prior_state() {
        let Ok(url) = std::env::var("DATABASE_URL") else {
            eprintln!("skipping live-db test: DATABASE_URL is not set");
            return;
        };
        let pool = sqlx::postgres::PgPoolOptions::new()
            .max_connections(1)
            .connect(&url)
            .await
            .expect("connect to scratch DATABASE_URL");
        sqlx::query("CREATE TEMP TABLE issues AS SELECT * FROM public.issues WITH NO DATA")
            .execute(&pool)
            .await
            .expect("temp table");
        let (with_state, without_state, state) = (Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4());
        sqlx::query("INSERT INTO issues (id, state_id) VALUES ($1, $2), ($3, NULL)")
            .bind(with_state)
            .bind(state)
            .bind(without_state)
            .execute(&pool)
            .await
            .expect("seed");
        assert_eq!(
            capture_prior_state_pool(&pool, Some(with_state))
                .await
                .expect("read"),
            Some(state)
        );
        assert_eq!(
            capture_prior_state_pool(&pool, Some(without_state))
                .await
                .expect("read"),
            None
        );
        assert_eq!(
            capture_prior_state_pool(&pool, Some(Uuid::new_v4()))
                .await
                .expect("read"),
            None
        );
        assert_eq!(
            capture_prior_state_pool(&pool, None).await.expect("read"),
            None
        );
    }
}
