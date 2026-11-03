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

use crate::orchestration::entries::{FireOutcome, FireRequest, PRIOR_STATE_SELECT_SQL};

/// The pre-save `state_id` snapshot (`capture_prior_state`,
/// `signals.py:61-71`) against the pool: `None` for a new instance or a
/// missing row.
pub async fn capture_prior_state_pool(
    pool: &PgPool,
    issue_pk: Option<Uuid>,
) -> Result<Option<Uuid>, sqlx::Error> {
    let Some(issue_pk) = issue_pk else {
        return Ok(None);
    };
    let row: Option<(Uuid, Option<Uuid>)> = sqlx::query_as(PRIOR_STATE_SELECT_SQL)
        .bind(issue_pk)
        .fetch_optional(pool)
        .await?;
    Ok(row.and_then(|(_, state_id)| state_id))
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
        };
        let outcome = found.fire(Uuid::nil(), request).await;
        assert!(matches!(outcome, Ok(FireOutcome::NoTransition)));
        assert_eq!(
            *hook.comments.lock().expect("lock"),
            vec![(issue_id, None, Some(issue_id))]
        );
        assert_eq!(*hook.fires.lock().expect("lock"), vec![Uuid::nil()]);
    }
}
