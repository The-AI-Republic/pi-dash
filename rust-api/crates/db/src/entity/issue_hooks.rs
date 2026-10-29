//! `Issue.save()` as SeaORM hooks (ORM-03, PIDASHCONV-1521 part A).
//!
//! Hand-written: the generator never emits this file, so regeneration is
//! safe — but the generated `issues.rs` must keep its empty
//! `ActiveModelBehavior` impl deleted (see the cleanup list in the
//! [`super`] docs), or the compiler rejects the duplicate impl.
//!
//! Django reference: `Issue.save()` (`db/models/issue.py:267-351`).
//! Every branch below cites its Python lines. Services reach these hooks
//! through `ActiveModel::insert` / `update` / `save` only — `Entity::insert`,
//! `update_many`, `insert_many` and raw statements skip them, exactly as
//! Django bulk paths skip signals (design D4).

use chrono::{DateTime, Timelike, Utc};
use sea_orm::{
    ActiveModelBehavior, ActiveModelTrait, ActiveValue, ConnectionTrait, DbBackend, DbErr, Set,
    Statement, Value,
};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use super::issue_sequences;
use super::issues::{ActiveModel, Model};
use crate::app_pages::strip::sync_description_stripped;

/// `now()` truncated to microseconds: Postgres `timestamptz` keeps micros,
/// and two successive `now()` calls must not drift on the sub-micro tail.
/// Mirrors the api handlers' `utc_now_micros`.
pub(crate) fn now_micros() -> DateTime<Utc> {
    let now = Utc::now();
    now.with_nanosecond(now.nanosecond() / 1000 * 1000)
        .unwrap_or(now)
}

/// `convert_uuid_to_integer` (`utils/uuid.py:19-26`): sha256 of the
/// hyphenated-lowercase id, first 8 bytes big-endian signed. The single
/// copy — the api handlers' twins are deleted.
pub(crate) fn advisory_lock_key(project_id: &Uuid) -> i64 {
    let mut hasher = Sha256::new();
    hasher.update(project_id.to_string().as_bytes());
    let digest = hasher.finalize();
    let mut bytes = [0u8; 8];
    bytes.copy_from_slice(&digest[..8]);
    i64::from_be_bytes(bytes)
}

/// `Pod.default_for_project_id` (`runner/models.py:174-176`):
/// `Pod.objects` (live only), project-default first.
const DEFAULT_POD_SQL: &str = "SELECT id FROM pod WHERE project_id = $1 AND is_default \
    AND deleted_at IS NULL ORDER BY is_default DESC, created_at ASC LIMIT 1";

/// The NULL-state default assignment (`issue.py:288-301`): the default
/// state first, else the first non-triage state, both `ORDER BY sequence`.
/// Predicates are the `State.objects` manager (live + non-triage group)
/// plus the explicit `~is_triage` filter — `is_triage` is a plain boolean
/// with no save-sync, so both predicates stay.
///
/// The two pre-consolidation copies spelled the group guard differently
/// (`NOT (s."group" = 'triage')` vs `s."group" != 'triage'`); the forms
/// agree on every input including NULL (both exclude it), so one spelling
/// serves both.
const DEFAULT_STATE_SQL: &str = "SELECT s.id FROM states AS s WHERE s.project_id = $1 \
    AND s.deleted_at IS NULL AND NOT (s.\"group\" = 'triage') AND NOT s.is_triage \
    ORDER BY s.sequence ASC LIMIT 1";

const DEFAULT_STATE_FALLBACK_SQL: &str = "SELECT s.id FROM states AS s WHERE s.project_id = $1 \
    AND s.deleted_at IS NULL AND NOT (s.\"group\" = 'triage') AND NOT s.is_triage \
    AND s.\"default\" ORDER BY s.sequence ASC LIMIT 1";

/// `self.state` on a save (`issue.py:306`): a forward-FK read, which Django
/// 4.2 resolves through `State._base_manager` (a plain `Manager` — `State`
/// sets no `base_manager_name`), so triage and soft-deleted states still
/// resolve and only a dangling FK misses.
const STATE_GROUP_SQL: &str = "SELECT \"group\" FROM states WHERE id = $1";

/// `IssueSequence.objects.filter(project=...).aggregate(Max("sequence"))`
/// (`issue.py:325-327`): the default manager excludes soft-deleted rows.
const MAX_SEQUENCE_SQL: &str =
    "SELECT MAX(sequence) AS largest FROM issue_sequences WHERE project_id = $1 \
     AND deleted_at IS NULL";

/// `Issue.objects.filter(project=..., state=...).aggregate(Max("sort_order"))`
/// (`issue.py:335-337`), plus the NULL-state leg the queryset implies.
const MAX_SORT_SQL: &str = "SELECT MAX(sort_order) AS largest FROM issues \
    WHERE project_id = $1 AND state_id = $2 AND deleted_at IS NULL";

const MAX_SORT_NULL_STATE_SQL: &str = "SELECT MAX(sort_order) AS largest FROM issues \
    WHERE project_id = $1 AND state_id IS NULL AND deleted_at IS NULL";

async fn query_uuid<C: ConnectionTrait>(
    db: &C,
    sql: &str,
    values: Vec<Value>,
) -> Result<Option<Uuid>, DbErr> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Postgres,
            sql,
            values,
        ))
        .await?;
    match row {
        None => Ok(None),
        Some(row) => row.try_get("", "id").map(Some),
    }
}

async fn query_string<C: ConnectionTrait>(
    db: &C,
    sql: &str,
    values: Vec<Value>,
) -> Result<Option<String>, DbErr> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Postgres,
            sql,
            values,
        ))
        .await?;
    match row {
        None => Ok(None),
        Some(row) => row.try_get("", "group").map(Some),
    }
}

async fn query_max_i64<C: ConnectionTrait>(
    db: &C,
    sql: &str,
    values: Vec<Value>,
) -> Result<Option<i64>, DbErr> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Postgres,
            sql,
            values,
        ))
        .await?;
    match row {
        None => Ok(None),
        Some(row) => row.try_get("", "largest"),
    }
}

async fn query_max_f64<C: ConnectionTrait>(
    db: &C,
    sql: &str,
    values: Vec<Value>,
) -> Result<Option<f64>, DbErr> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Postgres,
            sql,
            values,
        ))
        .await?;
    match row {
        None => Ok(None),
        Some(row) => row.try_get("", "largest"),
    }
}

fn active_uuid(value: &ActiveValue<Uuid>) -> Option<Uuid> {
    match value {
        ActiveValue::Set(id) | ActiveValue::Unchanged(id) => Some(*id),
        ActiveValue::NotSet => None,
    }
}

fn active_opt_uuid(value: &ActiveValue<Option<Uuid>>) -> Option<Uuid> {
    match value {
        ActiveValue::Set(id) | ActiveValue::Unchanged(id) => *id,
        ActiveValue::NotSet => None,
    }
}

fn active_string(value: &ActiveValue<String>) -> Option<&str> {
    match value {
        ActiveValue::Set(text) | ActiveValue::Unchanged(text) => Some(text.as_str()),
        ActiveValue::NotSet => None,
    }
}

/// Read an `Option<Uuid>` out of a save: `Set`/`Unchanged` on every path
/// the services build, `NotSet` only on misuse.
fn require_uuid(value: &ActiveValue<Uuid>, column: &'static str) -> Result<Uuid, DbErr> {
    active_uuid(value).ok_or_else(|| DbErr::Custom(format!("issue save without {column}")))
}

#[async_trait::async_trait]
impl ActiveModelBehavior for ActiveModel {
    async fn before_save<C>(mut self, db: &C, insert: bool) -> Result<Self, DbErr>
    where
        C: ConnectionTrait,
    {
        let project_id = require_uuid(&self.project_id, "project_id")?;

        // `if self._state.adding and self.assigned_pod_id is None`
        // (`issue.py:277-286`): creation only; PATCH-driven changes ride
        // the validated overlay instead.
        if insert && active_opt_uuid(&self.assigned_pod_id).is_none() {
            let pod = query_uuid(db, DEFAULT_POD_SQL, vec![Value::from(project_id)]).await?;
            if let Some(pod) = pod {
                self.assigned_pod_id = Set(Some(pod));
            }
        }

        // `if self.state is None: <default assignment> else: <completed_at
        // recompute>` (`issue.py:288-311`). Runs on every save, insert and
        // update alike. When the id starts `None` the `completed_at` branch
        // is skipped even if a default state is found — the assembled
        // value persists, exactly as Django's `if/else` falls.
        match active_opt_uuid(&self.state_id) {
            None => {
                let default = query_uuid(
                    db,
                    DEFAULT_STATE_FALLBACK_SQL,
                    vec![Value::from(project_id)],
                )
                .await?;
                let default = match default {
                    Some(id) => Some(id),
                    None => {
                        query_uuid(db, DEFAULT_STATE_SQL, vec![Value::from(project_id)]).await?
                    }
                };
                if let Some(id) = default {
                    self.state_id = Set(Some(id));
                }
            }
            Some(state_id) => {
                let group = query_string(db, STATE_GROUP_SQL, vec![Value::from(state_id)])
                    .await?
                    .ok_or_else(|| DbErr::RecordNotFound("state for issue save".to_owned()))?;
                if group == "completed" {
                    // `DateTimeWithTimeZone` is `FixedOffset`; the instant
                    // is the same one the handlers stamp as `Utc`.
                    self.completed_at = Set(Some(now_micros().into()));
                } else {
                    self.completed_at = Set(None);
                }
            }
        }

        // `description_stripped` sync (`issue.py:329-334` on create,
        // `issue.py:345-350` on update): unconditional on both legs.
        if let Some(html) = active_string(&self.description_html) {
            self.description_stripped = Set(sync_description_stripped(Some(html)));
        }

        // The `adding` block (`issue.py:313-341`): advisory lock, sequence,
        // sort order. The sequence assignment is unconditional — a passed
        // `sequence_id` is ignored on create, in Django and here.
        if insert {
            db.execute(Statement::from_sql_and_values(
                DbBackend::Postgres,
                "SELECT pg_advisory_xact_lock($1)",
                [Value::from(advisory_lock_key(&project_id))],
            ))
            .await?;
            let largest =
                query_max_i64(db, MAX_SEQUENCE_SQL, vec![Value::from(project_id)]).await?;
            // `last_sequence + 1 if last_sequence else 1`: `0` and `None`
            // both fall to `1` (Python falsiness).
            let next = match largest {
                Some(last) if last != 0 => last + 1,
                _ => 1,
            };
            self.sequence_id = Set(i32::try_from(next)
                .map_err(|_| DbErr::Custom("issue sequence_id out of i32 range".to_owned()))?);
            let state_id = active_opt_uuid(&self.state_id);
            let largest = match state_id {
                Some(state_id) => {
                    query_max_f64(
                        db,
                        MAX_SORT_SQL,
                        vec![Value::from(project_id), Value::from(state_id)],
                    )
                    .await?
                }
                None => {
                    query_max_f64(db, MAX_SORT_NULL_STATE_SQL, vec![Value::from(project_id)])
                        .await?
                }
            };
            if let Some(largest) = largest {
                self.sort_order = Set(largest + 10000.0);
            }
        }

        Ok(self)
    }

    async fn after_save<C>(model: Model, db: &C, insert: bool) -> Result<Model, DbErr>
    where
        C: ConnectionTrait,
    {
        if insert {
            // `IssueSequence.objects.create(...)` (`issue.py:343`): the
            // workspace rides the issue row, `created_by` is the issue's
            // own (both stamped from the same crum user), `created_at` /
            // `updated_at` are two `now()` calls like Django's
            // `auto_now_add` / `auto_now` pair. Runs here, not in
            // `before_save`: the `issue_id` FK points at the issue row,
            // which does not exist until the INSERT lands. Same
            // transaction either way — only the order is load-bearing.
            // Written through the entity (no raw SQL), so the write
            // guard sees no new site for the sanctioned writer.
            issue_sequences::ActiveModel {
                id: Set(Uuid::new_v4()),
                created_at: Set(now_micros().into()),
                updated_at: Set(now_micros().into()),
                created_by_id: Set(model.created_by_id),
                updated_by_id: Set(None),
                deleted_at: Set(None),
                project_id: Set(model.project_id),
                workspace_id: Set(model.workspace_id),
                issue_id: Set(Some(model.id)),
                sequence: Set(i64::from(model.sequence_id)),
                deleted: Set(false),
            }
            .insert(db)
            .await?;
        }
        Ok(model)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sea_orm::{ActiveModelTrait, EntityTrait, TransactionTrait};

    use super::super::issues::Entity as IssueEntity;

    #[test]
    fn lock_key_matches_uuid_vector() {
        // `convert_uuid_to_integer` (`utils/uuid.py:19-26`): sha256 of the
        // hyphenated id, first 8 bytes big-endian signed. Vectors pinned
        // against CPython's hashlib (moved here with the single copy).
        assert_eq!(
            advisory_lock_key(&Uuid::from_u128(0x11111111111111111111111111111111)),
            -4972562656765536426
        );
        assert_eq!(advisory_lock_key(&Uuid::nil()), 1349170572285598868);
    }

    #[test]
    fn now_micros_truncates_sub_micro_tail() {
        let stamped = now_micros();
        assert_eq!(stamped.nanosecond() % 1000, 0);
    }

    async fn live_db() -> Option<sea_orm::DatabaseConnection> {
        let url = match std::env::var("DATABASE_URL") {
            Ok(url) => url,
            Err(_) => {
                eprintln!("skipping live-db test: DATABASE_URL is not set");
                return None;
            }
        };
        let pool = sqlx::postgres::PgPoolOptions::new()
            .max_connections(1)
            .connect(&url)
            .await
            .expect("connect to scratch DATABASE_URL");
        Some(crate::sea_orm_connection(&pool))
    }

    struct Seed {
        user_id: Uuid,
        workspace_id: Uuid,
        project_id: Uuid,
        backlog_id: Uuid,
        completed_id: Uuid,
    }

    async fn exec(txn: &sea_orm::DatabaseTransaction, sql: &str, values: Vec<Value>) {
        txn.execute(Statement::from_sql_and_values(
            DbBackend::Postgres,
            sql,
            values,
        ))
        .await
        .expect("seed statement");
    }

    /// Workspace + project + two states (backlog default, completed),
    /// each id fresh per test so parallel tests never share a project.
    /// Everything runs inside the caller's transaction, which rolls back.
    async fn seed(txn: &sea_orm::DatabaseTransaction) -> Seed {
        let seed = Seed {
            user_id: Uuid::new_v4(),
            workspace_id: Uuid::new_v4(),
            project_id: Uuid::new_v4(),
            backlog_id: Uuid::new_v4(),
            completed_id: Uuid::new_v4(),
        };
        let username = format!("hook-{}", seed.user_id.simple());
        exec(
            txn,
            "INSERT INTO users (id, password, username, email, first_name, last_name, avatar, \
             date_joined, created_at, updated_at, last_location, created_location, is_superuser, \
             is_managed, is_password_expired, is_active, is_staff, is_email_verified, \
             is_password_autoset, token, user_timezone, last_login_ip, last_logout_ip, \
             last_login_medium, last_login_uagent, is_bot, display_name, is_email_valid, \
             is_password_reset_required) VALUES ($1, '!', $2, $3, 'CT', 'User', '', now(), now(), \
             now(), '', '', false, false, false, true, false, true, false, $4, 'UTC', '', '', \
             '', '', false, $2, true, false)",
            vec![
                Value::from(seed.user_id),
                Value::from(username.clone()),
                Value::from(format!("{username}@example.test")),
                Value::from("t".repeat(64)),
            ],
        )
        .await;
        exec(
            txn,
            "INSERT INTO workspaces (id, created_at, updated_at, name, slug, timezone, \
             background_color, owner_id) VALUES ($1, now(), now(), $2, $3, 'UTC', '#ffffff', $4)",
            vec![
                Value::from(seed.workspace_id),
                Value::from("hook test ws"),
                Value::from(format!("hook-{}", seed.workspace_id)),
                Value::from(seed.user_id),
            ],
        )
        .await;
        exec(
            txn,
            "INSERT INTO projects (id, name, description, network, identifier, workspace_id, \
             created_by_id, cycle_view, module_view, issue_views_view, page_view, intake_view, \
             archive_in, close_in, logo_props, is_time_tracking_enabled, is_issue_type_enabled, \
             guest_view_all_features, timezone, members_can_edit_states, repo_url, base_branch, \
             agent_default_interval_seconds, agent_default_max_ticks, agent_ticking_enabled, \
             is_default, agent_review_default_interval_seconds, default_agent_executor, \
             agent_test_default_interval_seconds, created_at, updated_at) VALUES ($1, $2, '', 0, \
             $3, $4, $5, true, true, true, true, false, 0, 0, '{}', true, true, false, 'UTC', \
             false, '', '', 0, 0, false, false, 0, '', 0, now(), now())",
            vec![
                Value::from(seed.project_id),
                Value::from("hook test project"),
                Value::from(format!(
                    "H{:08X}",
                    (seed.project_id.as_u128() & 0xffff_ffff) as u32
                )),
                Value::from(seed.workspace_id),
                Value::from(seed.user_id),
            ],
        )
        .await;
        for (id, name, slug, group, sequence, default) in [
            (
                seed.backlog_id,
                "Backlog",
                "backlog",
                "backlog",
                100.0,
                true,
            ),
            (seed.completed_id, "Done", "done", "completed", 200.0, false),
        ] {
            exec(
                txn,
                "INSERT INTO states (id, created_at, updated_at, name, description, color, slug, \
                 project_id, workspace_id, sequence, \"group\", \"default\", is_triage) \
                 VALUES ($1, now(), now(), $2, '', '#ff0000', $3, $4, $5, $6, $7, $8, false)",
                vec![
                    Value::from(id),
                    Value::from(name),
                    Value::from(format!(
                        "hook-{slug}-{sequence}-{}",
                        seed.project_id.simple()
                    )),
                    Value::from(seed.project_id),
                    Value::from(seed.workspace_id),
                    Value::from(sequence),
                    Value::from(group),
                    Value::from(default),
                ],
            )
            .await;
        }
        seed
    }

    /// A fully assembled issue insert: every column the services set.
    fn assemble(seed: &Seed) -> ActiveModel {
        ActiveModel {
            id: Set(Uuid::new_v4()),
            created_at: Set(now_micros().into()),
            updated_at: Set(now_micros().into()),
            created_by_id: Set(None),
            updated_by_id: Set(None),
            deleted_at: Set(None),
            project_id: Set(seed.project_id),
            workspace_id: Set(seed.workspace_id),
            parent_id: Set(None),
            state_id: Set(None),
            point: Set(None),
            estimate_point_id: Set(None),
            name: Set("hook test issue".to_owned()),
            description_json: Set(serde_json::Value::Object(serde_json::Map::new())),
            description_html: Set("<p>Hi</p>".to_owned()),
            description_stripped: Set(None),
            description_binary: Set(None),
            priority: Set("none".to_owned()),
            complexity_score: Set(0),
            start_date: Set(None),
            target_date: Set(None),
            sequence_id: Set(0),
            sort_order: Set(65535.0),
            completed_at: Set(None),
            archived_at: Set(None),
            is_draft: Set(false),
            external_source: Set(None),
            external_id: Set(None),
            type_id: Set(None),
            git_work_branch: Set(String::new()),
            workpad: Set(String::new()),
            created_via: Set(None),
            assigned_pod_id: Set(None),
            agent_executor: Set(None),
            work_type: Set(None),
        }
    }

    async fn sequence_row(
        txn: &sea_orm::DatabaseTransaction,
        issue_id: Uuid,
    ) -> Option<(i64, Uuid, Uuid)> {
        let row = txn
            .query_one(Statement::from_sql_and_values(
                DbBackend::Postgres,
                "SELECT sequence, project_id, workspace_id FROM issue_sequences \
                 WHERE issue_id = $1",
                [Value::from(issue_id)],
            ))
            .await
            .expect("sequence read");
        match row {
            None => None,
            Some(row) => {
                let sequence: i64 = row.try_get("", "sequence").expect("sequence decodes");
                let project_id: Uuid = row.try_get("", "project_id").expect("project decodes");
                let workspace_id: Uuid =
                    row.try_get("", "workspace_id").expect("workspace decodes");
                Some((sequence, project_id, workspace_id))
            }
        }
    }

    #[tokio::test]
    async fn insert_assigns_sequence_sort_default_state_stripped_and_sequence_row() {
        let Some(db) = live_db().await else { return };
        let txn = db.begin().await.expect("begin");
        let seed = seed(&txn).await;
        let saved = assemble(&seed).insert(&txn).await.expect("insert");
        assert_eq!(saved.sequence_id, 1);
        assert_eq!(saved.sort_order, 65535.0);
        assert_eq!(saved.state_id, Some(seed.backlog_id));
        // State started `None`: the `completed_at` branch is skipped even
        // though a default state was found (`issue.py:288-301`).
        assert!(saved.completed_at.is_none());
        assert_eq!(saved.description_stripped.as_deref(), Some("Hi"));
        assert!(saved.assigned_pod_id.is_none());
        let (sequence, project_id, workspace_id) =
            sequence_row(&txn, saved.id).await.expect("sequence row");
        assert_eq!(sequence, 1);
        assert_eq!(project_id, seed.project_id);
        assert_eq!(workspace_id, seed.workspace_id);
        txn.rollback().await.expect("rollback");
    }

    #[tokio::test]
    async fn null_state_insert_skips_triage_states() {
        // The default-state lookup excludes triage twice (`issue.py:290`
        // manager + `:297` filter): a triage state first in sequence
        // order is never picked.
        let Some(db) = live_db().await else { return };
        let txn = db.begin().await.expect("begin");
        let seed = seed(&txn).await;
        exec(
            &txn,
            "INSERT INTO states (id, created_at, updated_at, name, description, color, slug, \
             project_id, workspace_id, sequence, \"group\", \"default\", is_triage) \
             VALUES ($1, now(), now(), 'Triage', '', '#ff0000', $2, $3, $4, 0, 'triage', false, true)",
            vec![
                Value::from(Uuid::new_v4()),
                Value::from(format!("hook-triage-{}", seed.project_id)),
                Value::from(seed.project_id),
                Value::from(seed.workspace_id),
            ],
        )
        .await;
        let saved = assemble(&seed).insert(&txn).await.expect("insert");
        assert_eq!(saved.state_id, Some(seed.backlog_id));
        txn.rollback().await.expect("rollback");
    }

    #[tokio::test]
    async fn second_insert_increments_sequence_and_sort() {
        let Some(db) = live_db().await else { return };
        let txn = db.begin().await.expect("begin");
        let seed = seed(&txn).await;
        let first = assemble(&seed).insert(&txn).await.expect("first insert");
        let mut second = assemble(&seed);
        second.state_id = Set(Some(first.state_id.expect("default state")));
        let second = second.insert(&txn).await.expect("second insert");
        assert_eq!(second.sequence_id, 2);
        assert_eq!(second.sort_order, 65535.0 + 10000.0);
        txn.rollback().await.expect("rollback");
    }

    #[tokio::test]
    async fn insert_with_completed_state_stamps_completed_at() {
        let Some(db) = live_db().await else { return };
        let txn = db.begin().await.expect("begin");
        let seed = seed(&txn).await;
        let mut am = assemble(&seed);
        am.state_id = Set(Some(seed.completed_id));
        let saved = am.insert(&txn).await.expect("insert");
        assert!(saved.completed_at.is_some());
        txn.rollback().await.expect("rollback");
    }

    #[tokio::test]
    async fn insert_ignores_passed_sequence_id() {
        let Some(db) = live_db().await else { return };
        let txn = db.begin().await.expect("begin");
        let seed = seed(&txn).await;
        let mut am = assemble(&seed);
        am.sequence_id = Set(999);
        let saved = am.insert(&txn).await.expect("insert");
        assert_eq!(saved.sequence_id, 1);
        txn.rollback().await.expect("rollback");
    }

    #[tokio::test]
    async fn insert_assigns_default_pod_and_keeps_explicit_pod() {
        let Some(db) = live_db().await else { return };
        let txn = db.begin().await.expect("begin");
        let seed = seed(&txn).await;
        let default_pod = Uuid::new_v4();
        let other_pod = Uuid::new_v4();
        for (id, name, is_default) in [(default_pod, "default", true), (other_pod, "other", false)]
        {
            exec(
                &txn,
                "INSERT INTO pod (id, name, description, is_default, workspace_id, project_id, \
                 created_at, updated_at) VALUES ($1, $2, '', $3, $4, $5, now(), now())",
                vec![
                    Value::from(id),
                    Value::from(name),
                    Value::from(is_default),
                    Value::from(seed.workspace_id),
                    Value::from(seed.project_id),
                ],
            )
            .await;
        }
        let saved = assemble(&seed).insert(&txn).await.expect("insert");
        assert_eq!(saved.assigned_pod_id, Some(default_pod));
        let mut explicit = assemble(&seed);
        explicit.assigned_pod_id = Set(Some(other_pod));
        let kept = explicit.insert(&txn).await.expect("explicit insert");
        assert_eq!(kept.assigned_pod_id, Some(other_pod));
        txn.rollback().await.expect("rollback");
    }

    #[tokio::test]
    async fn update_recomputes_completed_and_stripped() {
        let Some(db) = live_db().await else { return };
        let txn = db.begin().await.expect("begin");
        let seed = seed(&txn).await;
        let saved = assemble(&seed).insert(&txn).await.expect("insert");
        let mut am: ActiveModel = IssueEntity::find_by_id(saved.id)
            .one(&txn)
            .await
            .expect("find")
            .expect("row")
            .into();
        am.state_id = Set(Some(seed.completed_id));
        am.description_html = Set("<p><b>Bold</b></p>".to_owned());
        let updated = am.update(&txn).await.expect("update");
        assert!(updated.completed_at.is_some());
        assert_eq!(updated.description_stripped.as_deref(), Some("Bold"));
        // Moving back out of the completed group clears the stamp.
        let mut am: ActiveModel = IssueEntity::find_by_id(saved.id)
            .one(&txn)
            .await
            .expect("find")
            .expect("row")
            .into();
        am.state_id = Set(Some(seed.backlog_id));
        let updated = am.update(&txn).await.expect("update");
        assert!(updated.completed_at.is_none());
        txn.rollback().await.expect("rollback");
    }

    #[tokio::test]
    async fn null_state_update_takes_default_and_keeps_completed() {
        let Some(db) = live_db().await else { return };
        let txn = db.begin().await.expect("begin");
        let seed = seed(&txn).await;
        let saved = assemble(&seed).insert(&txn).await.expect("insert");
        let fixed: chrono::DateTime<chrono::FixedOffset> =
            chrono::DateTime::parse_from_rfc3339("2020-01-01T00:00:00Z").expect("fixed time");
        let mut am: ActiveModel = IssueEntity::find_by_id(saved.id)
            .one(&txn)
            .await
            .expect("find")
            .expect("row")
            .into();
        am.state_id = Set(None);
        am.completed_at = Set(Some(fixed));
        let updated = am.update(&txn).await.expect("update");
        assert_eq!(updated.state_id, Some(seed.backlog_id));
        assert_eq!(updated.completed_at, Some(fixed));
        txn.rollback().await.expect("rollback");
    }

    #[tokio::test]
    async fn update_with_dangling_state_is_not_found() {
        let Some(db) = live_db().await else { return };
        let txn = db.begin().await.expect("begin");
        let seed = seed(&txn).await;
        let saved = assemble(&seed).insert(&txn).await.expect("insert");
        let mut am: ActiveModel = IssueEntity::find_by_id(saved.id)
            .one(&txn)
            .await
            .expect("find")
            .expect("row")
            .into();
        am.state_id = Set(Some(Uuid::new_v4()));
        let error = am.update(&txn).await.expect_err("dangling state misses");
        assert!(
            matches!(error, DbErr::RecordNotFound(_)),
            "unexpected error: {error:?}"
        );
        txn.rollback().await.expect("rollback");
    }
}
