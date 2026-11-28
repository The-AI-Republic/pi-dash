//! Assistant write surface (`assistant_user_llm_config`,
//! `assistant_message`, plus the named `run_turn` builders).
//!
//! Connection-generic like [`super::tokens`]: pool- and
//! transaction-held callers share one spelling (a pool via
//! `sea_orm_connection`, an sqlx transaction via `TxConn`), and the
//! flows stay caller-driven because they span aggregates (locks,
//! thread writes, cancel/clear, and the claim race stay with their
//! callers; only the message/config tables plus the issue-named
//! turn/event builders move here). Every write is a bare-column
//! statement (Django `save(update_fields=...)`, `QuerySet.update`,
//! `objects.create` on hook-less models): none of these entities has
//! an `ActiveModelBehavior` hook, so `Entity::insert` / `update_many`
//! / `delete_many` match the Django writes exactly. Errors stay
//! `sqlx::Error` (via [`into_sqlx`]) so callers keep classifying
//! SQLSTATEs (the `23505` config race).

use chrono::{DateTime, Utc};
use sea_orm::sea_query::{Alias, Expr, Func, InsertStatement, Query, ReturningClause, SimpleExpr};
use sea_orm::{
    ActiveValue, ColumnTrait, ConnectionTrait, DbBackend, EntityTrait, QueryFilter, QueryResult,
    UpdateMany,
};
use serde_json::Value;
use uuid::Uuid;

use pidash_db::entity::{
    assistant_event, assistant_message, assistant_turn, assistant_user_llm_config,
};
use pidash_db::orm_tx::into_sqlx;

/// Run one bare-column `UPDATE`/`DELETE`-shaped statement for its row
/// count.
async fn exec_update<E: EntityTrait>(
    db: &impl ConnectionTrait,
    update: UpdateMany<E>,
) -> Result<u64, sqlx::Error> {
    update
        .exec(db)
        .await
        .map(|done| done.rows_affected)
        .map_err(into_sqlx)
}

/// Run one built `INSERT ... RETURNING` and decode its single row.
async fn insert_returning(
    db: &impl ConnectionTrait,
    stmt: &InsertStatement,
) -> Result<QueryResult, sqlx::Error> {
    db.query_one(DbBackend::Postgres.build(stmt))
        .await
        .map_err(into_sqlx)?
        .ok_or(sqlx::Error::RowNotFound)
}

fn get<T: sea_orm::TryGetable>(row: &QueryResult, column: &str) -> Result<T, sqlx::Error> {
    row.try_get::<T>("", column).map_err(into_sqlx)
}

/// `(SELECT COALESCE(MAX(seq), 0) + 1 FROM <table> WHERE thread_id = $thread)`:
/// the in-insert `seq` allocation of the tools/error/event legs.
fn next_seq_on_thread<E: EntityTrait>(
    table: E,
    seq: E::Column,
    thread: E::Column,
    thread_id: Uuid,
) -> SimpleExpr {
    SimpleExpr::SubQuery(
        None,
        Box::new(
            Query::select()
                .expr(
                    Expr::expr(Func::coalesce([Expr::col(seq).max(), Expr::val(0).into()])).add(1),
                )
                .from(table)
                .and_where(Expr::col(thread).eq(thread_id))
                .to_owned()
                .into_sub_query_statement(),
        ),
    )
}

// ---------------------------------------------------------------------------
// assistant_user_llm_config
// ---------------------------------------------------------------------------

/// Resolved inputs for [`save_llm_config`]: the handler validates,
/// encrypts and resolves defaults first; `existed` comes from its
/// pre-read (get-or-create split across fetch + save).
pub struct LlmConfigSave<'a> {
    pub user_id: Uuid,
    pub provider_kind: &'a str,
    pub base_url: &'a str,
    pub model_name: &'a str,
    pub api_key_encrypted: &'a Option<Vec<u8>>,
    pub existed: bool,
}

/// `cfg.save()` (`llm_config.py:71`) when the row exists: refreshes
/// `updated_at` (`auto_now`); `created_at`, `user_id` and
/// `last_verified_at` keep their stored values.
fn config_save_update(
    params: &LlmConfigSave<'_>,
    now: DateTime<Utc>,
) -> UpdateMany<assistant_user_llm_config::Entity> {
    use assistant_user_llm_config::Column as C;
    assistant_user_llm_config::Entity::update_many()
        .col_expr(C::ProviderKind, Expr::value(params.provider_kind))
        .col_expr(C::BaseUrl, Expr::value(params.base_url))
        .col_expr(C::ModelName, Expr::value(params.model_name))
        .col_expr(
            C::ApiKeyEncrypted,
            Expr::value(params.api_key_encrypted.clone()),
        )
        .col_expr(C::UpdatedAt, Expr::value(now))
        .filter(C::UserId.eq(params.user_id))
}

/// The get-or-create insert: `last_verified_at` NULL (unverified until
/// the probe stamps it).
fn config_insert_row(
    params: &LlmConfigSave<'_>,
    now: DateTime<Utc>,
) -> assistant_user_llm_config::ActiveModel {
    assistant_user_llm_config::ActiveModel {
        user_id: ActiveValue::Set(params.user_id),
        provider_kind: ActiveValue::Set(params.provider_kind.to_owned()),
        base_url: ActiveValue::Set(params.base_url.to_owned()),
        model_name: ActiveValue::Set(params.model_name.to_owned()),
        api_key_encrypted: ActiveValue::Set(params.api_key_encrypted.clone()),
        last_verified_at: ActiveValue::Set(None),
        created_at: ActiveValue::Set(now.into()),
        updated_at: ActiveValue::Set(now.into()),
        ..Default::default()
    }
}

/// Save one user's LLM config (update when the row exists, insert
/// when it does not).
pub async fn save_llm_config(
    db: &impl ConnectionTrait,
    params: &LlmConfigSave<'_>,
) -> Result<(), sqlx::Error> {
    let now = Utc::now();
    if params.existed {
        exec_update(db, config_save_update(params, now)).await?;
    } else {
        assistant_user_llm_config::Entity::insert(config_insert_row(params, now))
            .exec_without_returning(db)
            .await
            .map_err(into_sqlx)?;
    }
    Ok(())
}

/// Delete one user's LLM config row
/// (`DELETE users/me/ai-assistant/config/`, `llm_config.py:76-78`).
pub async fn delete_llm_config(
    db: &impl ConnectionTrait,
    user_id: Uuid,
) -> Result<(), sqlx::Error> {
    assistant_user_llm_config::Entity::delete_many()
        .filter(assistant_user_llm_config::Column::UserId.eq(user_id))
        .exec(db)
        .await
        .map_err(into_sqlx)?;
    Ok(())
}

/// Stamp `last_verified_at` after a successful probe.
pub async fn stamp_llm_config_verified(
    db: &impl ConnectionTrait,
    user_id: Uuid,
    now: DateTime<Utc>,
) -> Result<(), sqlx::Error> {
    let update = assistant_user_llm_config::Entity::update_many()
        .col_expr(
            assistant_user_llm_config::Column::LastVerifiedAt,
            Expr::value(now),
        )
        .filter(assistant_user_llm_config::Column::UserId.eq(user_id));
    exec_update(db, update).await?;
    Ok(())
}

/// Resolved inputs for [`insert_llm_config`]: the full non-pk row.
/// The caller owns the clock (`created_at`/`updated_at`) and the
/// defaults (the overlay passes its field defaults); `last_verified_at`
/// is bound as given, not forced `NULL`.
pub struct LlmConfigInsert<'a> {
    pub user_id: Uuid,
    pub provider_kind: &'a str,
    pub base_url: &'a str,
    pub model_name: &'a str,
    pub api_key_encrypted: &'a Option<Vec<u8>>,
    pub last_verified_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Insert one LLM config row; returns the new id (`RETURNING id`, as
/// Django fetches the new pk) for the driver's subsequent `WHERE id`
/// update and returned struct (`connections.py:319` miss arm; the
/// `23505` race arm stays with the driver).
pub async fn insert_llm_config(
    db: &impl ConnectionTrait,
    params: &LlmConfigInsert<'_>,
) -> Result<i64, sqlx::Error> {
    let row = assistant_user_llm_config::ActiveModel {
        user_id: ActiveValue::Set(params.user_id),
        provider_kind: ActiveValue::Set(params.provider_kind.to_owned()),
        base_url: ActiveValue::Set(params.base_url.to_owned()),
        model_name: ActiveValue::Set(params.model_name.to_owned()),
        api_key_encrypted: ActiveValue::Set(params.api_key_encrypted.clone()),
        last_verified_at: ActiveValue::Set(params.last_verified_at.map(Into::into)),
        created_at: ActiveValue::Set(params.created_at.into()),
        updated_at: ActiveValue::Set(params.updated_at.into()),
        ..Default::default()
    };
    assistant_user_llm_config::Entity::insert(row)
        .exec(db)
        .await
        .map(|done| done.last_insert_id)
        .map_err(into_sqlx)
}

/// Resolved inputs for [`save_llm_config_full`]: the full non-pk row
/// plus the pk. The caller copies the projected values in (notably
/// `last_verified_at` from the selected connection row) and stamps
/// `updated_at` with the save instant first; `user_id` and `created_at`
/// carry their stored values.
pub struct LlmConfigFullSave<'a> {
    pub id: i64,
    pub user_id: Uuid,
    pub provider_kind: &'a str,
    pub base_url: &'a str,
    pub model_name: &'a str,
    pub api_key_encrypted: &'a Option<Vec<u8>>,
    pub last_verified_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Full `cfg.save()` (`connections.py:325`): every non-pk column of
/// the `id` row. Unlike [`save_llm_config`], `last_verified_at` is
/// written (copied from the selected connection row — deliberate
/// Python behavior, Tier-1: stored rows differ) and `user_id` plus the
/// `auto_now_add` `created_at` are written back with their stored
/// values.
fn config_full_save_update(
    params: &LlmConfigFullSave<'_>,
) -> UpdateMany<assistant_user_llm_config::Entity> {
    use assistant_user_llm_config::Column as C;
    assistant_user_llm_config::Entity::update_many()
        .col_expr(C::UserId, Expr::value(params.user_id))
        .col_expr(C::ProviderKind, Expr::value(params.provider_kind))
        .col_expr(C::BaseUrl, Expr::value(params.base_url))
        .col_expr(C::ModelName, Expr::value(params.model_name))
        .col_expr(
            C::ApiKeyEncrypted,
            Expr::value(params.api_key_encrypted.clone()),
        )
        .col_expr(C::LastVerifiedAt, Expr::value(params.last_verified_at))
        .col_expr(C::CreatedAt, Expr::value(params.created_at))
        .col_expr(C::UpdatedAt, Expr::value(params.updated_at))
        .filter(C::Id.eq(params.id))
}

/// Full `cfg.save()` (`connections.py:325`): overwrite every non-pk
/// column of the `id` row, `last_verified_at` included.
pub async fn save_llm_config_full(
    db: &impl ConnectionTrait,
    params: &LlmConfigFullSave<'_>,
) -> Result<(), sqlx::Error> {
    exec_update(db, config_full_save_update(params)).await?;
    Ok(())
}

// ---------------------------------------------------------------------------
// assistant_message
// ---------------------------------------------------------------------------

/// A freshly created user message, as the response envelope reads
/// it (the handler maps this onto its row struct field-for-field).
#[derive(Debug, Clone)]
pub struct CreatedMessage {
    pub id: Uuid,
    pub kind: String,
    pub display_content: String,
    pub status: String,
    pub seq: i64,
    pub turn_id: Option<Uuid>,
    pub payload_text: String,
    pub created_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
}

/// User message create (`events.create_message`, `messages.py:93-95`):
/// the id is `gen_random_uuid()` (the handler passes no id), `seq`
/// is the caller's `MAX+1` under the thread lock, `created_at` the
/// database clock, `completed_at` NULL. The full row comes back for
/// the response envelope, `payload` as its jsonb text.
fn create_user_message_insert(
    thread_id: Uuid,
    turn_id: Uuid,
    seq: i64,
    content: &str,
) -> InsertStatement {
    use assistant_message::Column as C;
    Query::insert()
        .into_table(assistant_message::Entity)
        .columns([
            C::Id,
            C::ThreadId,
            C::TurnId,
            C::Seq,
            C::Kind,
            C::DisplayContent,
            C::Payload,
            C::Status,
            C::CreatedAt,
            C::CompletedAt,
        ])
        .values_panic([
            Func::cust(Alias::new("gen_random_uuid")).into(),
            thread_id.into(),
            turn_id.into(),
            seq.into(),
            "user".into(),
            content.into(),
            serde_json::json!({}).into(),
            "completed".into(),
            Expr::current_timestamp().into(),
            Expr::value(Option::<DateTime<Utc>>::None),
        ])
        .returning(ReturningClause::Exprs(vec![
            Expr::col(C::Id).into(),
            Expr::col(C::Kind).into(),
            Expr::col(C::DisplayContent).into(),
            Expr::col(C::Status).into(),
            Expr::col(C::Seq).into(),
            Expr::col(C::TurnId).into(),
            // Output name stays `payload` (Postgres names a cast after
            // its column); decoded as text.
            Expr::col(C::Payload).cast_as(Alias::new("text")),
            Expr::col(C::CreatedAt).into(),
            Expr::col(C::CompletedAt).into(),
        ]))
        .to_owned()
}

/// Create the turn's user message; `seq` is the caller's `MAX+1`.
pub async fn create_user_message(
    db: &impl ConnectionTrait,
    thread_id: Uuid,
    turn_id: Uuid,
    seq: i64,
    content: &str,
) -> Result<CreatedMessage, sqlx::Error> {
    let row = insert_returning(
        db,
        &create_user_message_insert(thread_id, turn_id, seq, content),
    )
    .await?;
    Ok(CreatedMessage {
        id: get(&row, "id")?,
        kind: get(&row, "kind")?,
        display_content: get(&row, "display_content")?,
        status: get(&row, "status")?,
        seq: get(&row, "seq")?,
        turn_id: get(&row, "turn_id")?,
        payload_text: get(&row, "payload")?,
        created_at: get(&row, "created_at")?,
        completed_at: get(&row, "completed_at")?,
    })
}

/// Create the loop's prompt message (`dispatch.py:103-109` via
/// `events.create_message`); the caller generates the id and computes
/// `seq` as `MAX+1`. Returns the id (the old `RETURNING id` echoed the
/// bound value).
pub async fn create_prompt_message(
    db: &impl ConnectionTrait,
    id: Uuid,
    thread_id: Uuid,
    turn_id: Uuid,
    seq: i64,
    prompt: &str,
    now: DateTime<Utc>,
) -> Result<Uuid, sqlx::Error> {
    let row = assistant_message::ActiveModel {
        id: ActiveValue::Set(id),
        thread_id: ActiveValue::Set(thread_id),
        turn_id: ActiveValue::Set(Some(turn_id)),
        seq: ActiveValue::Set(seq),
        kind: ActiveValue::Set("user".to_owned()),
        display_content: ActiveValue::Set(prompt.to_owned()),
        payload: ActiveValue::Set(serde_json::json!({})),
        status: ActiveValue::Set("completed".to_owned()),
        created_at: ActiveValue::Set(now.into()),
        ..Default::default()
    };
    assistant_message::Entity::insert(row)
        .exec_without_returning(db)
        .await
        .map_err(into_sqlx)?;
    Ok(id)
}

/// A message insert whose `seq` allocates inside the statement
/// (`COALESCE(MAX(seq), 0) + 1` on the thread): the tool-result and
/// error legs. `created_at` is an explicit bound `now` (`auto_now_add`
/// with no database default).
#[allow(clippy::too_many_arguments)]
fn seq_message_insert(
    id: Uuid,
    thread_id: Uuid,
    turn_id: Uuid,
    kind: &str,
    display_content: &str,
    payload: &Value,
    status: &str,
    now: DateTime<Utc>,
) -> InsertStatement {
    use assistant_message::Column as C;
    Query::insert()
        .into_table(assistant_message::Entity)
        .columns([
            C::Id,
            C::ThreadId,
            C::TurnId,
            C::Seq,
            C::Kind,
            C::DisplayContent,
            C::Payload,
            C::Status,
            C::CreatedAt,
        ])
        .values_panic([
            id.into(),
            thread_id.into(),
            turn_id.into(),
            next_seq_on_thread(assistant_message::Entity, C::Seq, C::ThreadId, thread_id),
            kind.into(),
            display_content.into(),
            payload.clone().into(),
            status.into(),
            now.into(),
        ])
        .to_owned()
}

/// Record a tool-result message (`events.create_message` via the tools
/// port); `seq` allocates inside the insert. Returns
/// `(id, seq, created_at)` like the tools port.
pub async fn record_tool_message(
    db: &impl ConnectionTrait,
    id: Uuid,
    thread_id: Uuid,
    turn_id: Uuid,
    display_content: &str,
    payload: &Value,
    now: DateTime<Utc>,
) -> Result<(Uuid, i64, DateTime<Utc>), sqlx::Error> {
    use assistant_message::Column as C;
    let mut stmt = seq_message_insert(
        id,
        thread_id,
        turn_id,
        "tool_result",
        display_content,
        payload,
        "completed",
        now,
    );
    stmt.returning(Query::returning().columns([C::Id, C::Seq, C::CreatedAt]));
    let row = insert_returning(db, &stmt).await?;
    Ok((
        get(&row, "id")?,
        get(&row, "seq")?,
        get(&row, "created_at")?,
    ))
}

/// Record the turn-failure error message (`events.create_message`,
/// `tasks.py:191-193`): `error`-kind, `failed`-status, `seq` allocated
/// inside the insert. The caller picks `detail or code` for the
/// content.
pub async fn record_error_message(
    db: &impl ConnectionTrait,
    id: Uuid,
    thread_id: Uuid,
    turn_id: Uuid,
    content: &str,
    now: DateTime<Utc>,
) -> Result<(), sqlx::Error> {
    let stmt = seq_message_insert(
        id,
        thread_id,
        turn_id,
        "error",
        content,
        &serde_json::json!({}),
        "failed",
        now,
    );
    db.execute(DbBackend::Postgres.build(&stmt))
        .await
        .map_err(into_sqlx)?;
    Ok(())
}

/// Close still-streaming rows (`tasks.py:173-178`): every message of
/// the turn stuck in `streaming` moves to the terminal `status` with
/// `completed_at` the database clock.
fn finalize_open_messages_update(
    turn_id: Uuid,
    status: &str,
) -> UpdateMany<assistant_message::Entity> {
    use assistant_message::Column as C;
    assistant_message::Entity::update_many()
        .col_expr(C::Status, Expr::value(status))
        .col_expr(C::CompletedAt, Expr::current_timestamp().into())
        .filter(C::TurnId.eq(turn_id))
        .filter(C::Status.eq("streaming"))
}

/// Move a turn's `streaming` messages to the terminal `status`.
pub async fn finalize_open_messages(
    db: &impl ConnectionTrait,
    turn_id: Uuid,
    status: &str,
) -> Result<(), sqlx::Error> {
    exec_update(db, finalize_open_messages_update(turn_id, status)).await?;
    Ok(())
}

/// Finalize one message row with its final content and status
/// (`message.save(update_fields=["display_content", "status",
/// "completed_at"])`, `tasks.py:143-149`): the stamp is the ORM clock
/// value Python assigns, so it is bound. The caller owns the clock
/// read (`now` also stamps the completion envelope downstream).
pub async fn finalize_message(
    db: &impl ConnectionTrait,
    message_id: Uuid,
    display_content: &str,
    status: &str,
    now: DateTime<Utc>,
) -> Result<(), sqlx::Error> {
    use assistant_message::Column as C;
    let update = assistant_message::Entity::update_many()
        .col_expr(C::DisplayContent, Expr::value(display_content))
        .col_expr(C::Status, Expr::value(status))
        .col_expr(C::CompletedAt, Expr::value(now))
        .filter(C::Id.eq(message_id));
    exec_update(db, update).await?;
    Ok(())
}

/// The cut-off payload save's statement text, kept only for the SDK
/// surface (`pidash_sdk::assistant::MARK_CUT_OFF_SQL`, re-exported by
/// `turn::live`); nothing in rust-api executes it —
/// [`save_message_payload`] is the write.
pub const MESSAGE_PAYLOAD_SAVE_SQL: &str =
    "UPDATE assistant_message SET payload = $2 WHERE id = $1";

/// `message.save(update_fields=["payload"])` (`tasks.py:523`): flag
/// the turn's last reply as cut off — the payload with `truncated`
/// merged in. The caller owns the read and the merge (it needs the row
/// for the re-sent `message_completed` envelope downstream).
pub async fn save_message_payload(
    db: &impl ConnectionTrait,
    message_id: Uuid,
    payload: &Value,
) -> Result<(), sqlx::Error> {
    let update = assistant_message::Entity::update_many()
        .col_expr(
            assistant_message::Column::Payload,
            Expr::value(payload.clone()),
        )
        .filter(assistant_message::Column::Id.eq(message_id));
    exec_update(db, update).await?;
    Ok(())
}

/// Delete a thread's messages (thread-delete cascade leg; the turn,
/// event and thread legs stay with the cascade — other tables).
pub async fn delete_thread_messages(
    db: &impl ConnectionTrait,
    thread_id: Uuid,
) -> Result<(), sqlx::Error> {
    assistant_message::Entity::delete_many()
        .filter(assistant_message::Column::ThreadId.eq(thread_id))
        .exec(db)
        .await
        .map_err(into_sqlx)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Turn/event builders (the issue-named `run_turn` writes plus the `db`
// `create_message`/`append_event` shapes and the tools event leg — moved
// per the issue so callers stop executing the SQL text themselves)
// ---------------------------------------------------------------------------

/// Terminal complete write (`tasks.py:157-165`): guarded on
/// `running` — only the first terminal writer owns the row.
fn complete_turn_update(
    turn_id: Uuid,
    model_messages: &Value,
    usage: &Value,
    model_used: &str,
) -> UpdateMany<assistant_turn::Entity> {
    use assistant_turn::Column as C;
    assistant_turn::Entity::update_many()
        .col_expr(C::Status, Expr::value("completed"))
        .col_expr(C::ModelMessages, Expr::value(model_messages.clone()))
        .col_expr(C::Usage, Expr::value(usage.clone()))
        .col_expr(C::ModelUsed, Expr::value(model_used))
        .col_expr(C::CompletedAt, Expr::current_timestamp().into())
        .filter(C::Id.eq(turn_id))
        .filter(C::Status.eq("running"))
}

/// Terminal fail write (`tasks.py:181-188`): the caller truncates
/// code/detail first (Python slices count code points). Guarded on
/// `running` like the complete write.
fn fail_turn_update(turn_id: Uuid, code: &str, detail: &str) -> UpdateMany<assistant_turn::Entity> {
    use assistant_turn::Column as C;
    assistant_turn::Entity::update_many()
        .col_expr(C::Status, Expr::value("failed"))
        .col_expr(C::ErrorCode, Expr::value(code))
        .col_expr(C::ErrorDetail, Expr::value(detail))
        .col_expr(C::CompletedAt, Expr::current_timestamp().into())
        .filter(C::Id.eq(turn_id))
        .filter(C::Status.eq("running"))
}

/// Complete a turn; returns the guarded write's row count (0 when a
/// concurrent terminal write won).
pub async fn complete_turn(
    db: &impl ConnectionTrait,
    turn_id: Uuid,
    model_messages: &Value,
    usage: &Value,
    model_used: &str,
) -> Result<u64, sqlx::Error> {
    exec_update(
        db,
        complete_turn_update(turn_id, model_messages, usage, model_used),
    )
    .await
}

/// Fail a turn; returns the guarded write's row count (0 when a
/// concurrent terminal write won — the caller rolls back and emits
/// nothing).
pub async fn fail_turn(
    db: &impl ConnectionTrait,
    turn_id: Uuid,
    code: &str,
    detail: &str,
) -> Result<u64, sqlx::Error> {
    exec_update(db, fail_turn_update(turn_id, code, detail)).await
}

/// Delete a finished turn's `assistant_delta` events
/// (`events.prune_turn_deltas`, `tasks.py:170,201,213`).
pub async fn prune_turn_deltas(
    db: &impl ConnectionTrait,
    turn_id: Uuid,
) -> Result<(), sqlx::Error> {
    assistant_event::Entity::delete_many()
        .filter(assistant_event::Column::TurnId.eq(turn_id))
        .filter(assistant_event::Column::Kind.eq("assistant_delta"))
        .exec(db)
        .await
        .map_err(into_sqlx)?;
    Ok(())
}

/// Resolved inputs for [`append_event`]: the caller truncates `kind`
/// to 64 chars and defaults a missing payload to `{}` first.
pub struct NewEvent<'a> {
    pub thread_id: Uuid,
    pub turn_id: Option<Uuid>,
    pub seq: i64,
    pub kind: &'a str,
    pub message_id: Option<Uuid>,
    pub payload: &'a Value,
    pub created_at: DateTime<Utc>,
}

/// Append one assistant event row (`append_event`, `events.py:94-101`):
/// every concrete column, the `BigAutoField` pk left to its default.
pub async fn append_event(
    db: &impl ConnectionTrait,
    params: &NewEvent<'_>,
) -> Result<(), sqlx::Error> {
    let row = assistant_event::ActiveModel {
        thread_id: ActiveValue::Set(params.thread_id),
        turn_id: ActiveValue::Set(params.turn_id),
        seq: ActiveValue::Set(params.seq),
        kind: ActiveValue::Set(params.kind.to_owned()),
        message_id: ActiveValue::Set(params.message_id),
        payload: ActiveValue::Set(params.payload.clone()),
        created_at: ActiveValue::Set(params.created_at.into()),
        ..Default::default()
    };
    assistant_event::Entity::insert(row)
        .exec_without_returning(db)
        .await
        .map_err(into_sqlx)?;
    Ok(())
}

/// Resolved inputs for [`create_message_row`]: the caller generates
/// the id, allocates `seq` under the thread lock, and picks the
/// status (usually `completed`).
pub struct NewMessage<'a> {
    pub id: Uuid,
    pub thread_id: Uuid,
    pub turn_id: Option<Uuid>,
    pub seq: i64,
    pub kind: &'a str,
    pub display_content: &'a str,
    pub payload: &'a Value,
    pub status: &'a str,
    pub created_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
}

/// Insert one assistant message row (the `create_message` shape,
/// `events.py:117-125`): every concrete column, caller-generated pk.
pub async fn create_message_row(
    db: &impl ConnectionTrait,
    params: &NewMessage<'_>,
) -> Result<(), sqlx::Error> {
    let row = assistant_message::ActiveModel {
        id: ActiveValue::Set(params.id),
        thread_id: ActiveValue::Set(params.thread_id),
        turn_id: ActiveValue::Set(params.turn_id),
        seq: ActiveValue::Set(params.seq),
        kind: ActiveValue::Set(params.kind.to_owned()),
        display_content: ActiveValue::Set(params.display_content.to_owned()),
        payload: ActiveValue::Set(params.payload.clone()),
        status: ActiveValue::Set(params.status.to_owned()),
        created_at: ActiveValue::Set(params.created_at.into()),
        completed_at: ActiveValue::Set(params.completed_at.map(Into::into)),
    };
    assistant_message::Entity::insert(row)
        .exec_without_returning(db)
        .await
        .map_err(into_sqlx)?;
    Ok(())
}

/// An event insert whose `seq` allocates inside the statement
/// (`COALESCE(MAX(seq), 0) + 1` on the thread): the `turn_failed`
/// (`tasks.py:194-200`) and tool-result (`_results.py:60-68`) legs.
/// The `BigAutoField` pk is omitted.
fn seq_event_insert(
    thread_id: Uuid,
    turn_id: Uuid,
    kind: &str,
    message_id: Uuid,
    payload: &Value,
    now: DateTime<Utc>,
) -> InsertStatement {
    use assistant_event::Column as C;
    Query::insert()
        .into_table(assistant_event::Entity)
        .columns([
            C::ThreadId,
            C::TurnId,
            C::Seq,
            C::Kind,
            C::MessageId,
            C::Payload,
            C::CreatedAt,
        ])
        .values_panic([
            thread_id.into(),
            turn_id.into(),
            next_seq_on_thread(assistant_event::Entity, C::Seq, C::ThreadId, thread_id),
            kind.into(),
            message_id.into(),
            payload.clone().into(),
            now.into(),
        ])
        .to_owned()
}

/// Append the turn-failure event; returns the new event id
/// (`RETURNING id`) for the post-commit publish.
pub async fn append_turn_failed_event(
    db: &impl ConnectionTrait,
    thread_id: Uuid,
    turn_id: Uuid,
    message_id: Uuid,
    payload: &Value,
    now: DateTime<Utc>,
) -> Result<i64, sqlx::Error> {
    let mut stmt = seq_event_insert(thread_id, turn_id, "turn_failed", message_id, payload, now);
    stmt.returning_col(assistant_event::Column::Id);
    let row = insert_returning(db, &stmt).await?;
    get(&row, "id")
}

/// Record a tool-result event; returns `(id, seq, created_at)` like
/// the message leg.
pub async fn record_tool_event(
    db: &impl ConnectionTrait,
    thread_id: Uuid,
    turn_id: Uuid,
    message_id: Uuid,
    payload: &Value,
    now: DateTime<Utc>,
) -> Result<(i64, i64, DateTime<Utc>), sqlx::Error> {
    use assistant_event::Column as C;
    let mut stmt = seq_event_insert(thread_id, turn_id, "tool_result", message_id, payload, now);
    stmt.returning(Query::returning().columns([C::Id, C::Seq, C::CreatedAt]));
    let row = insert_returning(db, &stmt).await?;
    Ok((
        get(&row, "id")?,
        get(&row, "seq")?,
        get(&row, "created_at")?,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use sea_orm::QueryTrait;

    fn uid(n: u128) -> Uuid {
        Uuid::from_u128(n)
    }

    fn pin_time() -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2026-11-05T10:00:00Z")
            .expect("pin time")
            .with_timezone(&Utc)
    }

    fn render_update<E: EntityTrait>(update: UpdateMany<E>) -> String {
        update.build(DbBackend::Postgres).to_string()
    }

    fn render_insert(stmt: &InsertStatement) -> String {
        DbBackend::Postgres.build(stmt).to_string()
    }

    // Statement-shape pins for the SeaORM builders (SQL text is not a
    // contract — parity rule 13 — but the column sets, guards and the
    // in-insert seq allocation are).
    #[test]
    fn assistant_statement_shapes() {
        let key = Some(b"k".to_vec());
        let save = LlmConfigSave {
            user_id: uid(1),
            provider_kind: "anthropic",
            base_url: "https://b",
            model_name: "m",
            api_key_encrypted: &key,
            existed: true,
        };
        let full = LlmConfigFullSave {
            id: 7,
            user_id: uid(1),
            provider_kind: "anthropic",
            base_url: "https://b",
            model_name: "m",
            api_key_encrypted: &key,
            last_verified_at: None,
            created_at: pin_time(),
            updated_at: pin_time(),
        };
        let json = serde_json::json!({"a": 1});
        let got = [
            render_update(config_save_update(&save, pin_time())),
            render_update(config_full_save_update(&full)),
            render_insert(&create_user_message_insert(uid(2), uid(3), 4, "hi")),
            render_insert(&seq_message_insert(
                uid(5),
                uid(2),
                uid(3),
                "tool_result",
                "sum",
                &json,
                "completed",
                pin_time(),
            )),
            render_update(finalize_open_messages_update(uid(3), "failed")),
            render_update(complete_turn_update(uid(3), &json, &json, "mdl")),
            render_update(fail_turn_update(uid(3), "c", "d")),
            render_insert(&seq_event_insert(
                uid(2),
                uid(3),
                "turn_failed",
                uid(5),
                &json,
                pin_time(),
            )),
        ];
        let want = [
            r#"UPDATE "assistant_user_llm_config" SET "provider_kind" = 'anthropic', "base_url" = 'https://b', "model_name" = 'm', "api_key_encrypted" = '\x6B', "updated_at" = '2026-11-05 10:00:00.000000 +00:00' WHERE "assistant_user_llm_config"."user_id" = '00000000-0000-0000-0000-000000000001'"#,
            r#"UPDATE "assistant_user_llm_config" SET "user_id" = '00000000-0000-0000-0000-000000000001', "provider_kind" = 'anthropic', "base_url" = 'https://b', "model_name" = 'm', "api_key_encrypted" = '\x6B', "last_verified_at" = NULL, "created_at" = '2026-11-05 10:00:00.000000 +00:00', "updated_at" = '2026-11-05 10:00:00.000000 +00:00' WHERE "assistant_user_llm_config"."id" = 7"#,
            r#"INSERT INTO "assistant_message" ("id", "thread_id", "turn_id", "seq", "kind", "display_content", "payload", "status", "created_at", "completed_at") VALUES (gen_random_uuid(), '00000000-0000-0000-0000-000000000002', '00000000-0000-0000-0000-000000000003', 4, 'user', 'hi', '{}', 'completed', CURRENT_TIMESTAMP, NULL) RETURNING "id", "kind", "display_content", "status", "seq", "turn_id", CAST("payload" AS text), "created_at", "completed_at""#,
            r#"INSERT INTO "assistant_message" ("id", "thread_id", "turn_id", "seq", "kind", "display_content", "payload", "status", "created_at") VALUES ('00000000-0000-0000-0000-000000000005', '00000000-0000-0000-0000-000000000002', '00000000-0000-0000-0000-000000000003', (SELECT COALESCE(MAX("seq"), 0) + 1 FROM "assistant_message" WHERE "thread_id" = '00000000-0000-0000-0000-000000000002'), 'tool_result', 'sum', E'{\"a\":1}', 'completed', '2026-11-05 10:00:00.000000 +00:00')"#,
            r#"UPDATE "assistant_message" SET "status" = 'failed', "completed_at" = CURRENT_TIMESTAMP WHERE "assistant_message"."turn_id" = '00000000-0000-0000-0000-000000000003' AND "assistant_message"."status" = 'streaming'"#,
            r#"UPDATE "assistant_turn" SET "status" = 'completed', "model_messages" = E'{\"a\":1}', "usage" = E'{\"a\":1}', "model_used" = 'mdl', "completed_at" = CURRENT_TIMESTAMP WHERE "assistant_turn"."id" = '00000000-0000-0000-0000-000000000003' AND "assistant_turn"."status" = 'running'"#,
            r#"UPDATE "assistant_turn" SET "status" = 'failed', "error_code" = 'c', "error_detail" = 'd', "completed_at" = CURRENT_TIMESTAMP WHERE "assistant_turn"."id" = '00000000-0000-0000-0000-000000000003' AND "assistant_turn"."status" = 'running'"#,
            r#"INSERT INTO "assistant_event" ("thread_id", "turn_id", "seq", "kind", "message_id", "payload", "created_at") VALUES ('00000000-0000-0000-0000-000000000002', '00000000-0000-0000-0000-000000000003', (SELECT COALESCE(MAX("seq"), 0) + 1 FROM "assistant_event" WHERE "thread_id" = '00000000-0000-0000-0000-000000000002'), 'turn_failed', '00000000-0000-0000-0000-000000000005', E'{\"a\":1}', '2026-11-05 10:00:00.000000 +00:00')"#,
        ];
        assert_eq!(got.len(), want.len());
        for (got, want) in got.iter().zip(want) {
            assert_eq!(got, want);
        }
    }

    /// Live-db half: config save/delete/stamp plus message
    /// create/finalize/delete, all inside a rolled-back transaction
    /// (every FK on both tables is deferred, so the rollback skips
    /// the checks and no parents or cleanup are needed).
    #[tokio::test]
    async fn assistant_flow_roundtrip() {
        let url = match std::env::var("DATABASE_URL") {
            Ok(url) => url,
            Err(_) => {
                eprintln!("skipping live-db test: DATABASE_URL is not set");
                return;
            }
        };
        let pool = sqlx::postgres::PgPoolOptions::new()
            .max_connections(1)
            .connect(&url)
            .await
            .expect("connect to scratch DATABASE_URL");
        let mut tx = pool.begin().await.expect("begin");
        let user_id = Uuid::new_v4();
        let key = Some(b"live-key-blob".to_vec());
        save_llm_config(
            &pidash_db::orm_tx::TxConn::new(&mut tx),
            &LlmConfigSave {
                user_id,
                provider_kind: "openai_compatible",
                base_url: "https://live.example.com/v1",
                model_name: "live-model",
                api_key_encrypted: &key,
                existed: false,
            },
        )
        .await
        .expect("insert config");
        save_llm_config(
            &pidash_db::orm_tx::TxConn::new(&mut tx),
            &LlmConfigSave {
                user_id,
                provider_kind: "openai_compatible",
                base_url: "https://live.example.com/v2",
                model_name: "live-model-2",
                api_key_encrypted: &key,
                existed: true,
            },
        )
        .await
        .expect("update config");
        stamp_llm_config_verified(
            &pidash_db::orm_tx::TxConn::new(&mut tx),
            user_id,
            Utc::now(),
        )
        .await
        .expect("stamp config");
        let row: (String, Option<DateTime<Utc>>) = sqlx::query_as(
            "SELECT base_url, last_verified_at FROM assistant_user_llm_config WHERE user_id = $1",
        )
        .bind(user_id)
        .fetch_one(&mut *tx)
        .await
        .expect("read config");
        assert_eq!(row.0, "https://live.example.com/v2");
        assert!(row.1.is_some(), "verified stamp set");
        delete_llm_config(&pidash_db::orm_tx::TxConn::new(&mut tx), user_id)
            .await
            .expect("delete");
        // The overlay projection shapes (`connections.py:319-325`):
        // insert with field defaults returns the new id, and the full
        // save copies every projected value — `last_verified_at`
        // included — onto the `id` row.
        let proj_user = Uuid::new_v4();
        let no_key: Option<Vec<u8>> = None;
        let proj_created = Utc::now();
        let proj_id = insert_llm_config(
            &pidash_db::orm_tx::TxConn::new(&mut tx),
            &LlmConfigInsert {
                user_id: proj_user,
                provider_kind: "openai_compatible",
                base_url: "",
                model_name: "",
                api_key_encrypted: &no_key,
                last_verified_at: None,
                created_at: proj_created,
                updated_at: proj_created,
            },
        )
        .await
        .expect("insert config returning id");
        assert!(proj_id > 0, "new id returned");
        let verified = DateTime::from_timestamp(1_779_000_000, 0).expect("fixed stamp");
        save_llm_config_full(
            &pidash_db::orm_tx::TxConn::new(&mut tx),
            &LlmConfigFullSave {
                id: proj_id,
                user_id: proj_user,
                provider_kind: "anthropic",
                base_url: "https://proj.example.com",
                model_name: "proj-model",
                api_key_encrypted: &key,
                last_verified_at: Some(verified),
                created_at: proj_created,
                updated_at: Utc::now(),
            },
        )
        .await
        .expect("full save");
        #[derive(sqlx::FromRow)]
        struct ProjectedConfig {
            user_id: Uuid,
            provider_kind: String,
            base_url: String,
            model_name: String,
            api_key_encrypted: Option<Vec<u8>>,
            last_verified_at: Option<DateTime<Utc>>,
            created_at: DateTime<Utc>,
            updated_at: DateTime<Utc>,
        }
        let proj: ProjectedConfig = sqlx::query_as(
            "SELECT user_id, provider_kind, base_url, model_name, api_key_encrypted, \
             last_verified_at, created_at, updated_at \
             FROM assistant_user_llm_config WHERE id = $1",
        )
        .bind(proj_id)
        .fetch_one(&mut *tx)
        .await
        .expect("read projected row");
        assert_eq!(proj.user_id, proj_user);
        assert_eq!(proj.provider_kind, "anthropic");
        assert_eq!(proj.base_url, "https://proj.example.com");
        assert_eq!(proj.model_name, "proj-model");
        assert_eq!(proj.api_key_encrypted, key);
        assert_eq!(proj.last_verified_at, Some(verified));
        assert!(
            proj.updated_at >= proj.created_at,
            "save instant stamps updated_at"
        );
        delete_llm_config(&pidash_db::orm_tx::TxConn::new(&mut tx), proj_user)
            .await
            .expect("delete projection");
        let thread_id = Uuid::new_v4();
        let turn_id = Uuid::new_v4();
        let created = create_user_message(
            &pidash_db::orm_tx::TxConn::new(&mut tx),
            thread_id,
            turn_id,
            1,
            "live hello",
        )
        .await
        .expect("create message");
        assert_eq!(created.seq, 1);
        assert_eq!(created.status, "completed");
        assert_eq!(created.kind, "user");
        assert_eq!(created.display_content, "live hello");
        assert_eq!(created.turn_id, Some(turn_id));
        assert_eq!(created.payload_text, "{}");
        assert_eq!(created.completed_at, None);
        assert_ne!(created.id, Uuid::nil());
        let prompt_id = create_prompt_message(
            &pidash_db::orm_tx::TxConn::new(&mut tx),
            Uuid::new_v4(),
            thread_id,
            turn_id,
            2,
            "live prompt",
            Utc::now(),
        )
        .await
        .expect("create prompt");
        assert_ne!(prompt_id, created.id);
        finalize_message(
            &pidash_db::orm_tx::TxConn::new(&mut tx),
            created.id,
            "live final",
            "completed",
            Utc::now(),
        )
        .await
        .expect("finalize row");
        let done: (String, String) =
            sqlx::query_as("SELECT display_content, status FROM assistant_message WHERE id = $1")
                .bind(created.id)
                .fetch_one(&mut *tx)
                .await
                .expect("read finalized");
        assert_eq!(done.0, "live final");
        assert_eq!(done.1, "completed");
        save_message_payload(
            &pidash_db::orm_tx::TxConn::new(&mut tx),
            created.id,
            &serde_json::json!({"truncated": true}),
        )
        .await
        .expect("save payload");
        let flagged: (Value,) =
            sqlx::query_as("SELECT payload FROM assistant_message WHERE id = $1")
                .bind(created.id)
                .fetch_one(&mut *tx)
                .await
                .expect("read payload");
        assert_eq!(flagged.0, serde_json::json!({"truncated": true}));
        finalize_open_messages(&pidash_db::orm_tx::TxConn::new(&mut tx), turn_id, "failed")
            .await
            .expect("finalize");
        let (tool_id, tool_seq, _) = record_tool_message(
            &pidash_db::orm_tx::TxConn::new(&mut tx),
            Uuid::new_v4(),
            thread_id,
            turn_id,
            "live tool summary",
            &serde_json::json!({"links": []}),
            Utc::now(),
        )
        .await
        .expect("record tool message");
        assert_eq!(tool_seq, 3);
        record_error_message(
            &pidash_db::orm_tx::TxConn::new(&mut tx),
            Uuid::new_v4(),
            thread_id,
            turn_id,
            "live boom",
            Utc::now(),
        )
        .await
        .expect("record error message");
        append_event(
            &pidash_db::orm_tx::TxConn::new(&mut tx),
            &NewEvent {
                thread_id,
                turn_id: Some(turn_id),
                seq: 1,
                kind: "assistant_delta",
                message_id: None,
                payload: &serde_json::json!({}),
                created_at: Utc::now(),
            },
        )
        .await
        .expect("append event");
        create_message_row(
            &pidash_db::orm_tx::TxConn::new(&mut tx),
            &NewMessage {
                id: Uuid::new_v4(),
                thread_id,
                turn_id: Some(turn_id),
                seq: 5,
                kind: "user",
                display_content: "live row",
                payload: &serde_json::json!({}),
                status: "completed",
                created_at: Utc::now(),
                completed_at: None,
            },
        )
        .await
        .expect("create message row");
        let failed_id = append_turn_failed_event(
            &pidash_db::orm_tx::TxConn::new(&mut tx),
            thread_id,
            turn_id,
            tool_id,
            &serde_json::json!({"turn_id": turn_id.to_string()}),
            Utc::now(),
        )
        .await
        .expect("append turn_failed");
        assert!(failed_id > 0);
        let (_, event_seq, _) = record_tool_event(
            &pidash_db::orm_tx::TxConn::new(&mut tx),
            thread_id,
            turn_id,
            tool_id,
            &serde_json::json!({"turn_id": turn_id.to_string()}),
            Utc::now(),
        )
        .await
        .expect("record tool event");
        assert_eq!(event_seq, 3);
        // A finished turn owns its row exactly once: complete wins,
        // the guarded fail loses, prune drops the delta leg.
        sqlx::query(
            "INSERT INTO assistant_turn (id, thread_id, status, model_used, error_code, error_detail, created_at) VALUES ($1, $2, 'running', '', '', '', now())",
        )
        .bind(turn_id)
        .bind(thread_id)
        .execute(&mut *tx)
        .await
        .expect("seed turn");
        let won = complete_turn(
            &pidash_db::orm_tx::TxConn::new(&mut tx),
            turn_id,
            &serde_json::json!([]),
            &serde_json::json!({}),
            "live-model",
        )
        .await
        .expect("complete turn");
        assert_eq!(won, 1);
        let lost = fail_turn(
            &pidash_db::orm_tx::TxConn::new(&mut tx),
            turn_id,
            "code",
            "detail",
        )
        .await
        .expect("guarded fail loses");
        assert_eq!(lost, 0);
        prune_turn_deltas(&pidash_db::orm_tx::TxConn::new(&mut tx), turn_id)
            .await
            .expect("prune");
        let deltas: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM assistant_event WHERE turn_id = $1 AND kind = 'assistant_delta'",
        )
        .bind(turn_id)
        .fetch_one(&mut *tx)
        .await
        .expect("count deltas");
        assert_eq!(deltas.0, 0);
        delete_thread_messages(&pidash_db::orm_tx::TxConn::new(&mut tx), thread_id)
            .await
            .expect("delete messages");
        let left: (i64,) =
            sqlx::query_as("SELECT COUNT(*) FROM assistant_message WHERE thread_id = $1")
                .bind(thread_id)
                .fetch_one(&mut *tx)
                .await
                .expect("count");
        assert_eq!(left.0, 0);
        tx.rollback().await.expect("rollback");
    }
}
