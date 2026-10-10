#![forbid(unsafe_code)]

//! Live dispatch inputs for the worker-owned fire tasks (PIDASHCONV-861).
//!
//! The two fire handlers (`fire_tick`, `fire_scheduler_binding`) run
//! their D-12 dispatch through the jobs drivers, which need per-call
//! [`StoreDeps`]: settings, an admission cache, the `H`/`E`/`P`
//! closures, and `now`. This module builds those inputs live:
//!
//! * [`CallCache`]: the per-call admission cache. Mirrors the api
//!   crate's `RequestCache` exactly (a `Mutex<HashMap>`; the sync
//!   [`AdmissionCache`] trait cannot reach the async Redis client, and
//!   the drivers consult it only when building a cloud-executor run).
//!   A jobs-side type because jobs cannot depend on api (crate graph).
//! * [`JobsMatcher`]: the live [`PodRunnerMatcher`]. Mirrors the api
//!   crate's `LiveMatcher` arm for arm over the same services
//!   [`guards`](pidash_services::runner_sessions::guards) helpers.
//! * [`creator_universe`] + [`fetch_presence`]: the per-fire creator
//!   universe (actor, issue creator, project lead, default assignee,
//!   live assignees — the `handlers_actions::creator_universe` shape,
//!   without the request's workspace-slug scope) with LLM-key presence
//!   read from `assistant_user_llm_config` the way the assistant gate
//!   reads it. Reads are committed-state (outside the driver's
//!   transaction), exactly like the HTTP path's prefetch.
//! * [`has_key_fn`] / [`profile_fn`]: the `H`/`P` closures over a
//!   prefetched map (a user outside the map answers keyless, the same
//!   as a missing config row).
//! * [`load_states`]: the boot-time state cache the sync seam methods
//!   resolve through (the shim docs bless a cached resolver: "cutover
//!   wiring provides a cached one").
//!
//! The universe queries carry no soft-delete guards: presence is keyed
//! by id, so an extra id is harmless while a missing one would answer
//! keyless — the prefetch errs toward the superset.

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};

use sqlx::PgPool;
use uuid::Uuid;

use pidash_db::assistant::models::has_secret;
use pidash_services::assistant::seams::{agent_model_profile_for_user, has_usable_llm_config};
use pidash_services::dispatch::admission::AdmissionCache;
use pidash_services::dispatch::LlmProfile;
use pidash_services::orchestration::dispatch::{
    DispatchError, PodRunnerMatcher, LIVE_ASSIGNEE_CANDIDATES_SQL,
};
use pidash_services::runner_sessions::guards as g;

use super::fire_tick_seam::StateView;

// ---------------------------------------------------------------------------
// Call cache
// ---------------------------------------------------------------------------

/// Per-call admission cache: [`CallCache::default`] per dispatch, the
/// same call-scoped shape as the L6 drivers' caches.
#[derive(Debug, Default)]
pub(crate) struct CallCache {
    buckets: Mutex<HashMap<String, i64>>,
}

/// The [`AdmissionCache::Error`] for [`CallCache`] (a poisoned mutex).
#[derive(Debug)]
pub(crate) struct CacheError(pub(crate) String);

impl std::fmt::Display for CacheError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for CacheError {}

impl AdmissionCache for CallCache {
    type Error = CacheError;

    fn bucket_count(&self, key: &str) -> Result<Option<i64>, Self::Error> {
        self.buckets
            .lock()
            .map(|buckets| buckets.get(key).copied())
            .map_err(|error| CacheError(error.to_string()))
    }

    fn add_or_incr(&self, key: &str, _timeout_secs: i64) -> Result<(), Self::Error> {
        self.buckets
            .lock()
            .map(|mut buckets| {
                buckets
                    .entry(key.to_owned())
                    .and_modify(|count| *count += 1)
                    .or_insert(1);
            })
            .map_err(|error| CacheError(error.to_string()))
    }
}

// ---------------------------------------------------------------------------
// Jobs matcher
// ---------------------------------------------------------------------------

/// Live [`PodRunnerMatcher`] over the merged D-14 guards helpers
/// (`runner/services/matcher.py:375-412`): the managed narrow branch or
/// the general owner-set branch, with the same no-query short-circuits.
/// Mirrors the api crate's `LiveMatcher` (jobs cannot name it).
pub(crate) struct JobsMatcher {
    pool: PgPool,
}

impl JobsMatcher {
    pub(crate) fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

impl PodRunnerMatcher for JobsMatcher {
    fn pod_has_runner_for_issue_principal(
        &self,
        pod_id: Uuid,
        issue_id: Uuid,
        creator_id: Option<Uuid>,
    ) -> Pin<Box<dyn Future<Output = Result<bool, DispatchError>> + Send + '_>> {
        let pool = self.pool.clone();
        Box::pin(async move {
            use pidash_services::orchestration::creation::CreationError;
            let db = |error: sqlx::Error| DispatchError::Db(error.to_string());
            let issue: Option<(Option<String>, Option<Uuid>, Option<Uuid>)> = sqlx::query_as(
                r#"SELECT "agent_executor", "created_by_id", "project_id" FROM "issues" WHERE "id" = $1"#,
            )
            .bind(issue_id)
            .fetch_optional(&pool)
            .await
            .map_err(db)?;
            let Some((agent_executor, created_by_id, project_id)) = issue else {
                return Ok(false);
            };
            let project_id = project_id.ok_or_else(|| {
                DispatchError::Store(CreationError::MissingRow("issue has no project".to_owned()))
            })?;
            let default_executor: Option<String> = sqlx::query_scalar(
                r#"SELECT "default_agent_executor" FROM "projects" WHERE "id" = $1"#,
            )
            .bind(project_id)
            .fetch_optional(&pool)
            .await
            .map_err(db)?
            .flatten();
            let default_executor = default_executor.unwrap_or_default();
            if g::is_managed_runner_issue(agent_executor.as_deref(), &default_executor) {
                let Some(creator) = creator_id else {
                    return Ok(false);
                };
                let hit: Option<(i32,)> = sqlx::query_as(g::POD_HAS_RUNNER_MANAGED_SQL)
                    .bind(creator)
                    .bind(pod_id)
                    .fetch_optional(&pool)
                    .await
                    .map_err(db)?;
                return Ok(hit.is_some());
            }
            let assignees: Vec<Uuid> = sqlx::query_scalar(g::ISSUE_ASSIGNEE_IDS_SQL)
                .bind(issue_id)
                .fetch_all(&pool)
                .await
                .map_err(db)?;
            let owners = g::eligible_owner_ids(creator_id, created_by_id, &assignees);
            let Some(sql) = g::pod_has_runner_general_sql(owners.len()) else {
                return Ok(false);
            };
            let mut query = sqlx::query_as::<_, (i32,)>(&sql);
            for owner in &owners {
                query = query.bind(owner);
            }
            query = query.bind(pod_id);
            let hit: Option<(i32,)> = query.fetch_optional(&pool).await.map_err(db)?;
            Ok(hit.is_some())
        })
    }
}

// ---------------------------------------------------------------------------
// Creator universe + presence
// ---------------------------------------------------------------------------

/// Order the creator universe: the actor first, then the fallback
/// chain (issue creator, project lead, default assignee, live
/// assignees), deduplicated. Pure so the offline tests pin it.
pub(crate) fn order_universe(
    actor_id: Option<Uuid>,
    issue_created_by_id: Option<Uuid>,
    project_lead_id: Option<Uuid>,
    default_assignee_id: Option<Uuid>,
    assignees: &[(Uuid, bool, bool)],
) -> Vec<Uuid> {
    let mut ids: Vec<Uuid> = Vec::new();
    for id in [
        actor_id,
        issue_created_by_id,
        project_lead_id,
        default_assignee_id,
    ]
    .into_iter()
    .flatten()
    {
        if !ids.contains(&id) {
            ids.push(id);
        }
    }
    for (id, _, _) in assignees {
        if !ids.contains(id) {
            ids.push(*id);
        }
    }
    ids
}

/// The project fallback-creator candidates (lead, default
/// assignee). A missing project row narrows to `(None, None)` — the
/// driver's own reads surface the missing row as designed.
pub(crate) async fn project_leads(
    pool: &PgPool,
    project_id: Uuid,
) -> Result<(Option<Uuid>, Option<Uuid>), sqlx::Error> {
    let row: Option<(Option<Uuid>, Option<Uuid>)> = sqlx::query_as(
        r#"SELECT "project_lead_id", "default_assignee_id" FROM "projects" WHERE "id" = $1"#,
    )
    .bind(project_id)
    .fetch_optional(pool)
    .await?;
    Ok(row.unwrap_or((None, None)))
}

/// The union of users the fire's dispatch may query for presence:
/// the actor plus every fallback-creator candidate (issue creator,
/// project lead, default assignee, live assignees). A missing issue
/// or project row narrows the universe instead of failing — the
/// driver's own reads surface the missing row as designed.
pub(crate) async fn creator_universe(
    pool: &PgPool,
    actor_id: Option<Uuid>,
    issue_id: Uuid,
) -> Result<Vec<Uuid>, sqlx::Error> {
    let issue: Option<(Option<Uuid>, Option<Uuid>)> =
        sqlx::query_as(r#"SELECT "created_by_id", "project_id" FROM "issues" WHERE "id" = $1"#)
            .bind(issue_id)
            .fetch_optional(pool)
            .await?;
    let (issue_created_by_id, project_id) = match issue {
        Some((created_by_id, project_id)) => (created_by_id, project_id),
        None => (None, None),
    };
    let (project_lead_id, default_assignee_id) = match project_id {
        Some(project_id) => project_leads(pool, project_id).await?,
        None => (None, None),
    };
    let assignees: Vec<(Uuid, bool, bool)> = sqlx::query_as(LIVE_ASSIGNEE_CANDIDATES_SQL)
        .bind(issue_id)
        .fetch_all(pool)
        .await?;
    Ok(order_universe(
        actor_id,
        issue_created_by_id,
        project_lead_id,
        default_assignee_id,
        &assignees,
    ))
}

/// LLM-key presence for every universe id, read from
/// `assistant_user_llm_config` the way the assistant gate reads it.
pub(crate) async fn fetch_presence(
    pool: &PgPool,
    user_ids: &[Uuid],
) -> Result<HashMap<Uuid, bool>, sqlx::Error> {
    let mut presence: HashMap<Uuid, bool> = HashMap::with_capacity(user_ids.len());
    if user_ids.is_empty() {
        return Ok(presence);
    }
    let rows: Vec<(Uuid, Option<Vec<u8>>)> =
        sqlx::query_as(r#"SELECT "user_id", "api_key_encrypted" FROM "assistant_user_llm_config" WHERE "user_id" = ANY($1)"#)
            .bind(user_ids)
            .fetch_all(pool)
            .await?;
    for id in user_ids {
        presence.insert(*id, false);
    }
    for (id, key) in rows {
        presence.insert(id, has_secret(&key));
    }
    Ok(presence)
}

/// The `has_usable_llm_config` closure over pre-fetched presence. A
/// user outside the pre-fetched universe (only reachable through a
/// concurrent insert between the prefetch and the driver) answers
/// keyless, the same as a missing config row.
pub(crate) fn has_key_fn(
    presence: Arc<HashMap<Uuid, bool>>,
) -> impl Fn(Uuid) -> bool + Clone + Send + Sync {
    move |uid: Uuid| -> bool { has_usable_llm_config(presence.get(&uid).copied().unwrap_or(false)) }
}

/// The `managed_llm_profile` closure over pre-fetched presence.
pub(crate) fn profile_fn(
    presence: Arc<HashMap<Uuid, bool>>,
) -> impl Fn(Option<Uuid>) -> LlmProfile + Clone + Send + Sync {
    move |uid: Option<Uuid>| -> LlmProfile {
        let key = uid
            .and_then(|uid| presence.get(&uid).copied())
            .unwrap_or(false);
        let profile = agent_model_profile_for_user(key);
        LlmProfile {
            available: profile.available,
            reason_code: profile.reason_code,
        }
    }
}

// ---------------------------------------------------------------------------
// Boot-time state cache
// ---------------------------------------------------------------------------

/// Every live state row: the sync seam methods resolve
/// `is_ticking_state` / tick intervals through this boot-loaded map
/// (states change rarely; the shim docs bless a cached resolver).
pub(crate) async fn load_states(pool: &PgPool) -> Result<HashMap<Uuid, StateView>, sqlx::Error> {
    let rows: Vec<(Uuid, String, String)> =
        sqlx::query_as(r#"SELECT "id", "name", "group" FROM "states" WHERE "deleted_at" IS NULL"#)
            .fetch_all(pool)
            .await?;
    Ok(rows
        .into_iter()
        .map(|(id, name, group)| (id, StateView { id, name, group }))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Parse one statement with the Postgres dialect (the scan-suite
    /// precedent): new SQL must at least parse offline.
    fn parse(sql: &str) -> sqlparser::ast::Statement {
        sqlparser::parser::Parser::parse_sql(&sqlparser::dialect::PostgreSqlDialect {}, sql)
            .expect("SQL parses")
            .pop()
            .expect("one statement")
    }

    #[test]
    fn cache_counts_like_request_cache() {
        let cache = CallCache::default();
        assert_eq!(cache.bucket_count("k").unwrap(), None);
        cache.add_or_incr("k", 60).unwrap();
        assert_eq!(cache.bucket_count("k").unwrap(), Some(1));
        cache.add_or_incr("k", 60).unwrap();
        assert_eq!(cache.bucket_count("k").unwrap(), Some(2));
        assert_eq!(cache.bucket_count("other").unwrap(), None);
    }

    #[test]
    fn universe_orders_actor_first_deduped() {
        let actor = Uuid::from_u128(1);
        let creator = Uuid::from_u128(2);
        let lead = Uuid::from_u128(3);
        let assignee = Uuid::from_u128(4);
        let ordered = order_universe(
            Some(actor),
            Some(creator),
            Some(lead),
            Some(lead),
            &[(assignee, true, false), (creator, true, false)],
        );
        assert_eq!(ordered, vec![actor, creator, lead, assignee]);
        let empty = order_universe(None, None, None, None, &[]);
        assert!(empty.is_empty());
    }

    #[test]
    fn key_and_profile_closures_answer_keyless_outside_map() {
        let present = Uuid::from_u128(7);
        let presence: Arc<HashMap<Uuid, bool>> = Arc::new([(present, true)].into_iter().collect());
        let has_key = has_key_fn(Arc::clone(&presence));
        assert!(has_key(present));
        assert!(!has_key(Uuid::from_u128(8)));
        let profile = profile_fn(presence);
        let keyed = profile(Some(present));
        assert!(!keyed.available);
        assert!(!keyed.reason_code.is_empty());
        let keyless = profile(None);
        assert!(!keyless.available);
    }

    #[test]
    fn live_input_sql_parses() {
        for sql in [
            r#"SELECT "created_by_id", "project_id" FROM "issues" WHERE "id" = $1"#,
            r#"SELECT "project_lead_id", "default_assignee_id" FROM "projects" WHERE "id" = $1"#,
            r#"SELECT "user_id", "api_key_encrypted" FROM "assistant_user_llm_config" WHERE "user_id" = ANY($1)"#,
            r#"SELECT "id", "name", "group" FROM "states" WHERE "deleted_at" IS NULL"#,
            r#"SELECT "agent_executor", "created_by_id", "project_id" FROM "issues" WHERE "id" = $1"#,
            r#"SELECT "default_agent_executor" FROM "projects" WHERE "id" = $1"#,
        ] {
            assert!(
                matches!(parse(sql), sqlparser::ast::Statement::Query(_)),
                "{sql}"
            );
        }
    }
}
