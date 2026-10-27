//! Assistant write surface (`assistant_user_llm_config`,
//! `assistant_message`, plus the named `run_turn` builders).
//!
//! Executor-generic like [`super::tokens`]: pool- and
//! transaction-held callers share one spelling, and the flows stay
//! caller-driven because they span aggregates (locks, thread
//! writes, cancel/clear, and the claim race stay with their
//! callers; only the message/config tables plus the issue-named
//! turn/event builders move here).

use chrono::{DateTime, Utc};
use serde_json::Value;
use sqlx::FromRow;
use uuid::Uuid;

// ---------------------------------------------------------------------------
// assistant_user_llm_config
// ---------------------------------------------------------------------------

/// `cfg.save()` (`llm_config.py:71`): full-row write refreshing
/// `updated_at` (`auto_now`); `created_at`, `user_id` and
/// `last_verified_at` keep their stored values. Moved verbatim from
/// the config handler.
const CONFIG_SAVE_SQL: &str = "UPDATE assistant_user_llm_config
               SET provider_kind = $1, base_url = $2, model_name = $3,
                   api_key_encrypted = $4, updated_at = $5
               WHERE user_id = $6";

/// Config get-or-create insert: `last_verified_at` NULL (unverified
/// until the probe stamps it). Moved verbatim from the handler.
const CONFIG_INSERT_SQL: &str = "INSERT INTO assistant_user_llm_config
               (user_id, provider_kind, base_url, model_name, api_key_encrypted,
                last_verified_at, created_at, updated_at)
               VALUES ($1, $2, $3, $4, $5, NULL, $6, $7)";

/// `DELETE users/me/ai-assistant/config/` (`llm_config.py:76-78`).
const CONFIG_DELETE_SQL: &str = "DELETE FROM assistant_user_llm_config WHERE user_id = $1";

/// Verify-probe stamp (a probe `Ok` records `last_verified_at`).
const CONFIG_STAMP_VERIFIED_SQL: &str =
    "UPDATE assistant_user_llm_config SET last_verified_at = $1 WHERE user_id = $2";

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

/// Save one user's LLM config (update when the row exists, insert
/// when it does not).
pub async fn save_llm_config<'e, E>(ex: E, params: &LlmConfigSave<'_>) -> Result<(), sqlx::Error>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    let now = Utc::now();
    if params.existed {
        sqlx::query(CONFIG_SAVE_SQL)
            .bind(params.provider_kind)
            .bind(params.base_url)
            .bind(params.model_name)
            .bind(params.api_key_encrypted)
            .bind(now)
            .bind(params.user_id)
            .execute(ex)
            .await
            .map(|_| ())
    } else {
        sqlx::query(CONFIG_INSERT_SQL)
            .bind(params.user_id)
            .bind(params.provider_kind)
            .bind(params.base_url)
            .bind(params.model_name)
            .bind(params.api_key_encrypted)
            .bind(now)
            .bind(now)
            .execute(ex)
            .await
            .map(|_| ())
    }
}

/// Delete one user's LLM config row.
pub async fn delete_llm_config<'e, E>(ex: E, user_id: Uuid) -> Result<(), sqlx::Error>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    sqlx::query(CONFIG_DELETE_SQL)
        .bind(user_id)
        .execute(ex)
        .await
        .map(|_| ())
}

/// Stamp `last_verified_at` after a successful probe.
pub async fn stamp_llm_config_verified<'e, E>(
    ex: E,
    user_id: Uuid,
    now: DateTime<Utc>,
) -> Result<(), sqlx::Error>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    sqlx::query(CONFIG_STAMP_VERIFIED_SQL)
        .bind(now)
        .bind(user_id)
        .execute(ex)
        .await
        .map(|_| ())
}

// ---------------------------------------------------------------------------
// assistant_message
// ---------------------------------------------------------------------------

/// User message create (`events.create_message`, `messages.py:93-95`):
/// the id is `gen_random_uuid()` (the handler passes no id), `seq`
/// is the caller's `MAX+1` under the thread lock, `completed_at`
/// stays NULL. The full row comes back for the response envelope.
/// Moved verbatim from the messages handler.
const CREATE_USER_MESSAGE_SQL: &str = "INSERT INTO \"assistant_message\" \
         (\"id\", \"thread_id\", \"turn_id\", \"seq\", \"kind\", \"display_content\", \
         \"payload\", \"status\", \"created_at\", \"completed_at\") \
         VALUES (gen_random_uuid(), $1, $2, $3, 'user', $4, '{}', 'completed', now(), NULL) \
         RETURNING \"id\", \"kind\", \"display_content\", \"status\", \"seq\", \
         \"turn_id\", \"payload\"::text AS \"payload_text\", \"created_at\", \"completed_at\"";

/// Loop prompt message (`dispatch.py:103-109` via
/// `events.create_message`): caller-generated id, `RETURNING id`
/// only. Moved verbatim from the loop dispatcher.
const CREATE_PROMPT_MESSAGE_SQL: &str = "INSERT INTO assistant_message \
     (id, thread_id, turn_id, seq, kind, display_content, payload, status, created_at) \
     VALUES ($1, $2, $3, $4, 'user', $5, '{}', 'completed', $6) \
     RETURNING id";

/// Tool-result message (`events.create_message` via the tools port):
/// `seq` is `COALESCE(MAX(seq), 0) + 1` on the thread, `created_at`
/// is an explicit bound `now` (`auto_now_add` with no database
/// default). Moved verbatim from the tools consts.
const RECORD_TOOL_MESSAGE_SQL: &str = "INSERT INTO assistant_message (id, thread_id, turn_id, seq, kind, display_content, payload, status, created_at) \
     VALUES ($1, $2, $3, \
     (SELECT COALESCE(MAX(seq), 0) + 1 FROM assistant_message WHERE thread_id = $2), \
     'tool_result', $4, $5, 'completed', $6) RETURNING id, seq, created_at";

/// Error message row (`events.create_message`, `tasks.py:191-193`):
/// `error`-kind, `failed`-status, same `MAX+1` seq form as the tools
/// ports. Moved verbatim from the `run_turn` builders.
const RECORD_ERROR_MESSAGE_SQL: &str = "INSERT INTO assistant_message \
         (id, thread_id, turn_id, seq, kind, display_content, payload, status, created_at) \
         VALUES ($1, $2, $3, \
         (SELECT COALESCE(MAX(seq), 0) + 1 FROM assistant_message WHERE thread_id = $2), \
         'error', $4, '{}', 'failed', $5)";

/// Close still-streaming rows (`tasks.py:173-178`): every message of
/// the turn stuck in `streaming` moves to the terminal status with
/// `completed_at` now. Moved verbatim from the `run_turn` builders.
const FINALIZE_OPEN_MESSAGES_SQL: &str =
    "UPDATE assistant_message SET status = $2, completed_at = NOW() \
         WHERE turn_id = $1 AND status = 'streaming'";

/// Thread-delete cascade leg: drop the thread's messages (the turn,
/// event and thread legs stay with the cascade — other tables).
const DELETE_THREAD_MESSAGES_SQL: &str =
    "DELETE FROM \"assistant_message\" WHERE \"thread_id\" = $1";

/// A freshly created user message, as the response envelope reads
/// it (the handler maps this onto its row struct field-for-field).
#[derive(Debug, Clone, FromRow)]
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

/// Create the turn's user message; `seq` is the caller's `MAX+1`.
pub async fn create_user_message<'e, E>(
    ex: E,
    thread_id: Uuid,
    turn_id: Uuid,
    seq: i64,
    content: &str,
) -> Result<CreatedMessage, sqlx::Error>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    sqlx::query_as(CREATE_USER_MESSAGE_SQL)
        .bind(thread_id)
        .bind(turn_id)
        .bind(seq)
        .bind(content)
        .fetch_one(ex)
        .await
}

/// Create the loop's prompt message; the caller generates the id and
/// computes `seq` as `MAX+1`.
pub async fn create_prompt_message<'e, E>(
    ex: E,
    id: Uuid,
    thread_id: Uuid,
    turn_id: Uuid,
    seq: i64,
    prompt: &str,
    now: DateTime<Utc>,
) -> Result<Uuid, sqlx::Error>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    sqlx::query_scalar(CREATE_PROMPT_MESSAGE_SQL)
        .bind(id)
        .bind(thread_id)
        .bind(turn_id)
        .bind(seq)
        .bind(prompt)
        .bind(now)
        .fetch_one(ex)
        .await
}

/// Record a tool-result message; `seq` allocates inside the insert.
/// Returns `(id, seq, created_at)` like the tools port.
pub async fn record_tool_message<'e, E>(
    ex: E,
    id: Uuid,
    thread_id: Uuid,
    turn_id: Uuid,
    display_content: &str,
    payload: &Value,
    now: DateTime<Utc>,
) -> Result<(Uuid, i64, DateTime<Utc>), sqlx::Error>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    sqlx::query_as(RECORD_TOOL_MESSAGE_SQL)
        .bind(id)
        .bind(thread_id)
        .bind(turn_id)
        .bind(display_content)
        .bind(payload)
        .bind(now)
        .fetch_one(ex)
        .await
}

/// Record the turn-failure error message; `seq` allocates inside the
/// insert. The caller picks `detail or code` for the content.
pub async fn record_error_message<'e, E>(
    ex: E,
    id: Uuid,
    thread_id: Uuid,
    turn_id: Uuid,
    content: &str,
    now: DateTime<Utc>,
) -> Result<(), sqlx::Error>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    sqlx::query(RECORD_ERROR_MESSAGE_SQL)
        .bind(id)
        .bind(thread_id)
        .bind(turn_id)
        .bind(content)
        .bind(now)
        .execute(ex)
        .await
        .map(|_| ())
}

/// Move a turn's `streaming` messages to the terminal `status`.
pub async fn finalize_open_messages<'e, E>(
    ex: E,
    turn_id: Uuid,
    status: &str,
) -> Result<(), sqlx::Error>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    sqlx::query(FINALIZE_OPEN_MESSAGES_SQL)
        .bind(turn_id)
        .bind(status)
        .execute(ex)
        .await
        .map(|_| ())
}

/// `message.save(update_fields=["display_content", "status",
/// "completed_at"])` (`tasks.py:143-149`): the stamp is the ORM clock
/// value Python assigns, so it is bound rather than rendered by
/// `NOW()`. Moved verbatim from the live turn seam.
const FINALIZE_MESSAGE_SQL: &str = "UPDATE assistant_message \
     SET display_content = $2, status = $3, completed_at = $4 WHERE id = $1";

/// Finalize one message row with its final content and status. The
/// caller owns the clock read (`now` also stamps the completion
/// envelope downstream).
pub async fn finalize_message<'e, E>(
    ex: E,
    message_id: Uuid,
    display_content: &str,
    status: &str,
    now: DateTime<Utc>,
) -> Result<(), sqlx::Error>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    sqlx::query(FINALIZE_MESSAGE_SQL)
        .bind(message_id)
        .bind(display_content)
        .bind(status)
        .bind(now)
        .execute(ex)
        .await
        .map(|_| ())
}

/// Delete a thread's messages (thread-delete cascade leg).
pub async fn delete_thread_messages<'e, E>(ex: E, thread_id: Uuid) -> Result<(), sqlx::Error>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    sqlx::query(DELETE_THREAD_MESSAGES_SQL)
        .bind(thread_id)
        .execute(ex)
        .await
        .map(|_| ())
}

// ---------------------------------------------------------------------------
// Turn/event builders (the issue-named `run_turn` writes plus the `db`
// `create_message`/`append_event` shapes and the tools event leg — moved
// per the issue so callers stop executing the SQL text themselves)
// ---------------------------------------------------------------------------

/// Terminal complete write (`tasks.py:157-165`): guarded on
/// `running` — only the first terminal writer owns the row. Moved
/// verbatim from the `run_turn` builders.
const COMPLETE_TURN_SQL: &str =
    "UPDATE assistant_turn SET status = 'completed', model_messages = $2, \
         usage = $3, model_used = $4, completed_at = NOW() WHERE id = $1 \
         AND status = 'running'";

/// Terminal fail write (`tasks.py:181-188`): the caller truncates
/// code/detail first (Python slices count code points). Guarded on
/// `running` like the complete write. Moved verbatim.
const FAIL_TURN_SQL: &str = "UPDATE assistant_turn SET status = 'failed', error_code = $2, \
         error_detail = $3, completed_at = NOW() WHERE id = $1 \
         AND status = 'running'";

/// Delta pruning (`events.prune_turn_deltas`, `tasks.py:170,201,213`):
/// a finished turn's `assistant_delta` events are deleted. Moved
/// verbatim.
const PRUNE_TURN_DELTAS_SQL: &str =
    "DELETE FROM assistant_event WHERE turn_id = $1 AND kind = 'assistant_delta'";

/// `append_event` insert shape (`events.py:94-101`): every concrete
/// event column in model field order (the `BigAutoField` pk takes
/// `DEFAULT`), values bound positionally. The caller truncates `kind`
/// and defaults `payload` to `{}` first. Moved verbatim from the
/// `db` event queries.
fn append_event_sql() -> String {
    let columns = pidash_db::assistant::models::assistant_event::COLUMNS
        .iter()
        .filter(|column| **column != "id")
        .cloned()
        .collect::<Vec<_>>()
        .join("\", \"");
    let placeholders = (1..=pidash_db::assistant::models::assistant_event::COLUMNS.len() - 1)
        .map(|index| format!("${index}"))
        .collect::<Vec<_>>()
        .join(", ");
    format!("INSERT INTO \"assistant_event\" (\"{columns}\") VALUES ({placeholders})")
}

/// Complete a turn; returns the guarded write's row count (0 when a
/// concurrent terminal write won).
pub async fn complete_turn<'e, E>(
    ex: E,
    turn_id: Uuid,
    model_messages: &Value,
    usage: &Value,
    model_used: &str,
) -> Result<u64, sqlx::Error>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    sqlx::query(COMPLETE_TURN_SQL)
        .bind(turn_id)
        .bind(model_messages)
        .bind(usage)
        .bind(model_used)
        .execute(ex)
        .await
        .map(|done| done.rows_affected())
}

/// Fail a turn; returns the guarded write's row count (0 when a
/// concurrent terminal write won — the caller rolls back and emits
/// nothing).
pub async fn fail_turn<'e, E>(
    ex: E,
    turn_id: Uuid,
    code: &str,
    detail: &str,
) -> Result<u64, sqlx::Error>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    sqlx::query(FAIL_TURN_SQL)
        .bind(turn_id)
        .bind(code)
        .bind(detail)
        .execute(ex)
        .await
        .map(|done| done.rows_affected())
}

/// Delete a finished turn's `assistant_delta` events.
pub async fn prune_turn_deltas<'e, E>(ex: E, turn_id: Uuid) -> Result<(), sqlx::Error>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    sqlx::query(PRUNE_TURN_DELTAS_SQL)
        .bind(turn_id)
        .execute(ex)
        .await
        .map(|_| ())
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

/// Append one assistant event row.
pub async fn append_event<'e, E>(ex: E, params: &NewEvent<'_>) -> Result<(), sqlx::Error>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    sqlx::query(&append_event_sql())
        .bind(params.thread_id)
        .bind(params.turn_id)
        .bind(params.seq)
        .bind(params.kind)
        .bind(params.message_id)
        .bind(params.payload)
        .bind(params.created_at)
        .execute(ex)
        .await
        .map(|_| ())
}

/// `create_message` insert shape (`events.py:117-125`): every concrete
/// message column in model field order, values bound positionally in
/// `COLUMNS` order. The pk is included: the caller generates the id
/// and binds it as `$1`. Moved verbatim from the `db` event queries.
fn insert_message_row_sql() -> String {
    let columns = pidash_db::assistant::models::assistant_message::COLUMNS.join("\", \"");
    let placeholders = (1..=pidash_db::assistant::models::assistant_message::COLUMNS.len())
        .map(|index| format!("${index}"))
        .collect::<Vec<_>>()
        .join(", ");
    format!("INSERT INTO \"assistant_message\" (\"{columns}\") VALUES ({placeholders})")
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

/// Insert one assistant message row (the `create_message` shape).
pub async fn create_message_row<'e, E>(ex: E, params: &NewMessage<'_>) -> Result<(), sqlx::Error>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    sqlx::query(&insert_message_row_sql())
        .bind(params.id)
        .bind(params.thread_id)
        .bind(params.turn_id)
        .bind(params.seq)
        .bind(params.kind)
        .bind(params.display_content)
        .bind(params.payload)
        .bind(params.status)
        .bind(params.created_at)
        .bind(params.completed_at)
        .execute(ex)
        .await
        .map(|_| ())
}

/// `turn_failed` event row (`tasks.py:194-200`): kind, the error
/// message id, and the turn/error payload. The event id is a
/// `BigAutoField`, so the insert omits it — but returns it
/// (`RETURNING id`) for the post-commit publish. Moved verbatim
/// from the `run_turn` builders.
const TURN_FAILED_EVENT_SQL: &str =
    "INSERT INTO assistant_event (thread_id, turn_id, seq, kind, message_id, payload, created_at) \
     VALUES ($1, $2, \
     (SELECT COALESCE(MAX(seq), 0) + 1 FROM assistant_event WHERE thread_id = $1), \
     'turn_failed', $3, $4, $5) RETURNING id";

/// Append the turn-failure event; returns the new event id for the
/// post-commit publish.
pub async fn append_turn_failed_event<'e, E>(
    ex: E,
    thread_id: Uuid,
    turn_id: Uuid,
    message_id: Uuid,
    payload: &Value,
    now: DateTime<Utc>,
) -> Result<i64, sqlx::Error>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    sqlx::query_scalar(TURN_FAILED_EVENT_SQL)
        .bind(thread_id)
        .bind(turn_id)
        .bind(message_id)
        .bind(payload)
        .bind(now)
        .fetch_one(ex)
        .await
}

/// Tool-result event (`_results.py:60-68`): `seq` allocates inside
/// the insert like the message leg, `created_at` is an explicit
/// bound `now`. Moved verbatim from the tools consts.
const RECORD_TOOL_EVENT_SQL: &str =
    "INSERT INTO assistant_event (thread_id, turn_id, seq, kind, message_id, payload, created_at) \
     VALUES ($1, $2, \
     (SELECT COALESCE(MAX(seq), 0) + 1 FROM assistant_event WHERE thread_id = $1), \
     'tool_result', $3, $4, $5) RETURNING id, seq, created_at";

/// Record a tool-result event; returns `(id, seq, created_at)` like
/// the message leg.
pub async fn record_tool_event<'e, E>(
    ex: E,
    thread_id: Uuid,
    turn_id: Uuid,
    message_id: Uuid,
    payload: &Value,
    now: DateTime<Utc>,
) -> Result<(i64, i64, DateTime<Utc>), sqlx::Error>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    sqlx::query_as(RECORD_TOOL_EVENT_SQL)
        .bind(thread_id)
        .bind(turn_id)
        .bind(message_id)
        .bind(payload)
        .bind(now)
        .fetch_one(ex)
        .await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn collapse_continuations(sql: &str) -> String {
        // Rust `\`+newline continuations strip the newline and the
        // leading whitespace; the consts above store the collapsed
        // form, so normalize the same way before comparing.
        let mut out = String::with_capacity(sql.len());
        let mut chars = sql.chars().peekable();
        while let Some(char) = chars.next() {
            if char == '\\' && chars.peek() == Some(&'\n') {
                chars.next();
                while matches!(chars.peek(), Some(' ') | Some('\t')) {
                    chars.next();
                }
            } else {
                out.push(char);
            }
        }
        out
    }

    // Moved pins: the handler/builder/const pin tests are deleted with
    // their SQL; these carry the text here.
    #[test]
    fn assistant_sql_pins() {
        // (The config statements are raw strings with literal
        // newlines, like the handler's — pinned raw, not collapsed.)
        assert_eq!(
            CONFIG_SAVE_SQL,
            "UPDATE assistant_user_llm_config\n               SET provider_kind = $1, base_url = $2, model_name = $3,\n                   api_key_encrypted = $4, updated_at = $5\n               WHERE user_id = $6"
        );
        assert_eq!(
            CONFIG_INSERT_SQL,
            "INSERT INTO assistant_user_llm_config\n               (user_id, provider_kind, base_url, model_name, api_key_encrypted,\n                last_verified_at, created_at, updated_at)\n               VALUES ($1, $2, $3, $4, $5, NULL, $6, $7)"
        );
        assert_eq!(
            CONFIG_DELETE_SQL,
            "DELETE FROM assistant_user_llm_config WHERE user_id = $1"
        );
        assert_eq!(
            CONFIG_STAMP_VERIFIED_SQL,
            "UPDATE assistant_user_llm_config SET last_verified_at = $1 WHERE user_id = $2"
        );
        assert_eq!(
            collapse_continuations(CREATE_USER_MESSAGE_SQL),
            "INSERT INTO \"assistant_message\" (\"id\", \"thread_id\", \"turn_id\", \"seq\", \"kind\", \"display_content\", \"payload\", \"status\", \"created_at\", \"completed_at\") VALUES (gen_random_uuid(), $1, $2, $3, 'user', $4, '{}', 'completed', now(), NULL) RETURNING \"id\", \"kind\", \"display_content\", \"status\", \"seq\", \"turn_id\", \"payload\"::text AS \"payload_text\", \"created_at\", \"completed_at\""
        );
        assert_eq!(
            collapse_continuations(CREATE_PROMPT_MESSAGE_SQL),
            "INSERT INTO assistant_message (id, thread_id, turn_id, seq, kind, display_content, payload, status, created_at) VALUES ($1, $2, $3, $4, 'user', $5, '{}', 'completed', $6) RETURNING id"
        );
        assert_eq!(
            collapse_continuations(RECORD_TOOL_MESSAGE_SQL),
            "INSERT INTO assistant_message (id, thread_id, turn_id, seq, kind, display_content, payload, status, created_at) VALUES ($1, $2, $3, (SELECT COALESCE(MAX(seq), 0) + 1 FROM assistant_message WHERE thread_id = $2), 'tool_result', $4, $5, 'completed', $6) RETURNING id, seq, created_at"
        );
        assert_eq!(
            collapse_continuations(RECORD_ERROR_MESSAGE_SQL),
            "INSERT INTO assistant_message (id, thread_id, turn_id, seq, kind, display_content, payload, status, created_at) VALUES ($1, $2, $3, (SELECT COALESCE(MAX(seq), 0) + 1 FROM assistant_message WHERE thread_id = $2), 'error', $4, '{}', 'failed', $5)"
        );
        assert_eq!(
            collapse_continuations(FINALIZE_OPEN_MESSAGES_SQL),
            "UPDATE assistant_message SET status = $2, completed_at = NOW() WHERE turn_id = $1 AND status = 'streaming'"
        );
        assert_eq!(
            collapse_continuations(FINALIZE_MESSAGE_SQL),
            "UPDATE assistant_message SET display_content = $2, status = $3, completed_at = $4 WHERE id = $1"
        );
        assert_eq!(
            DELETE_THREAD_MESSAGES_SQL,
            "DELETE FROM \"assistant_message\" WHERE \"thread_id\" = $1"
        );
        assert_eq!(
            collapse_continuations(COMPLETE_TURN_SQL),
            "UPDATE assistant_turn SET status = 'completed', model_messages = $2, usage = $3, model_used = $4, completed_at = NOW() WHERE id = $1 AND status = 'running'"
        );
        assert_eq!(
            collapse_continuations(FAIL_TURN_SQL),
            "UPDATE assistant_turn SET status = 'failed', error_code = $2, error_detail = $3, completed_at = NOW() WHERE id = $1 AND status = 'running'"
        );
        assert_eq!(
            PRUNE_TURN_DELTAS_SQL,
            "DELETE FROM assistant_event WHERE turn_id = $1 AND kind = 'assistant_delta'"
        );
        assert_eq!(
            append_event_sql(),
            "INSERT INTO \"assistant_event\" (\"thread_id\", \"turn_id\", \"seq\", \"kind\", \"message_id\", \"payload\", \"created_at\") VALUES ($1, $2, $3, $4, $5, $6, $7)"
        );
        assert_eq!(
            insert_message_row_sql(),
            "INSERT INTO \"assistant_message\" (\"id\", \"thread_id\", \"turn_id\", \"seq\", \"kind\", \"display_content\", \"payload\", \"status\", \"created_at\", \"completed_at\") VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)"
        );
        assert_eq!(
            collapse_continuations(TURN_FAILED_EVENT_SQL),
            "INSERT INTO assistant_event (thread_id, turn_id, seq, kind, message_id, payload, created_at) VALUES ($1, $2, (SELECT COALESCE(MAX(seq), 0) + 1 FROM assistant_event WHERE thread_id = $1), 'turn_failed', $3, $4, $5) RETURNING id"
        );
        assert_eq!(
            collapse_continuations(RECORD_TOOL_EVENT_SQL),
            "INSERT INTO assistant_event (thread_id, turn_id, seq, kind, message_id, payload, created_at) VALUES ($1, $2, (SELECT COALESCE(MAX(seq), 0) + 1 FROM assistant_event WHERE thread_id = $1), 'tool_result', $3, $4, $5) RETURNING id, seq, created_at"
        );
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
            &mut *tx,
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
            &mut *tx,
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
        stamp_llm_config_verified(&mut *tx, user_id, Utc::now())
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
        delete_llm_config(&mut *tx, user_id).await.expect("delete");
        let thread_id = Uuid::new_v4();
        let turn_id = Uuid::new_v4();
        let created = create_user_message(&mut *tx, thread_id, turn_id, 1, "live hello")
            .await
            .expect("create message");
        assert_eq!(created.seq, 1);
        assert_eq!(created.status, "completed");
        let prompt_id = create_prompt_message(
            &mut *tx,
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
        finalize_message(&mut *tx, created.id, "live final", "completed", Utc::now())
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
        finalize_open_messages(&mut *tx, turn_id, "failed")
            .await
            .expect("finalize");
        let (tool_id, tool_seq, _) = record_tool_message(
            &mut *tx,
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
            &mut *tx,
            Uuid::new_v4(),
            thread_id,
            turn_id,
            "live boom",
            Utc::now(),
        )
        .await
        .expect("record error message");
        append_event(
            &mut *tx,
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
            &mut *tx,
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
            &mut *tx,
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
            &mut *tx,
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
            &mut *tx,
            turn_id,
            &serde_json::json!([]),
            &serde_json::json!({}),
            "live-model",
        )
        .await
        .expect("complete turn");
        assert_eq!(won, 1);
        let lost = fail_turn(&mut *tx, turn_id, "code", "detail")
            .await
            .expect("guarded fail loses");
        assert_eq!(lost, 0);
        prune_turn_deltas(&mut *tx, turn_id).await.expect("prune");
        let deltas: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM assistant_event WHERE turn_id = $1 AND kind = 'assistant_delta'",
        )
        .bind(turn_id)
        .fetch_one(&mut *tx)
        .await
        .expect("count deltas");
        assert_eq!(deltas.0, 0);
        delete_thread_messages(&mut *tx, thread_id)
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
