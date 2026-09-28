//! Loop dispatch: turn one claimed, eligible target into an assistant turn (D-03).
//!
//! Ports `pi_dash/loop/dispatch.py` in full (`_ensure_thread`, lines 39-71,
//! and `dispatch_loop_turn`, lines 74-131) via the D-03 models layer
//! ([`pidash_db::r#loop`], owned by PIDASHCONV-153 — called, never
//! re-ported). From the `run_assistant_turn` enqueue onward, nothing is
//! loop-specific — the stock assistant runtime resolves the user,
//! credentials, history, limits, and finalization (the runtime itself is
//! out of scope; only its Celery wire name is kept).
//!
//! The dispatch mirrors the chat message POST handler
//! (`assistant/views/messages.py`) minus HTTP, plus hidden-thread
//! management with rotation.
//!
//! Translation notes (translate, don't redesign):
//!
//! * `dispatch_loop_turn` opens its **own transaction**: the fire task
//!   (PIDASHCONV-157 `fire.rs`) commits its cursor advance first, so a
//!   dispatch failure never rolls the cursor back — there is **no
//!   rollback phase** here (local row creation, not a remote pod match).
//! * `transaction.on_commit(lambda: run_assistant_turn.delay(tid))` becomes
//!   an [`enqueue_exec`][crate::queue::enqueue_exec] on the open dispatch
//!   transaction: the job row commits or rolls back with the turn, so a
//!   failed dispatch never emits a phantom run (the documented
//!   `on_commit` half of transactional enqueue). The worker loop forwards
//!   the Python-owned `assistant.run_turn` name to the broker in Celery
//!   protocol v2 — wire-identical whichever plane serves the fire task.
//! * The dispatch claim (`.get(pk=target_id)`, `dispatch.py:82-86`) carries
//!   **no** `deleted_at` filter — only the default manager's single
//!   `deleted_at IS NULL`. The fire claim (`fire.rs`) doubles it
//!   (manager + explicit `deleted_at__isnull`).
//! * Assistant rows (`assistant_thread`, `assistant_turn`,
//!   `assistant_message`) are plain `models.Model`
//!   (`assistant/models.py:28,72,123`) — no soft-delete manager, so none
//!   of the dispatch reads carry a `deleted_at` condition.
//! * `_ensure_thread` reuses while `count < threshold`; equality rotates
//!   (`dispatch.py:56`, strict `<`). A `thread_id` pointing at a missing
//!   or non-LOOP row falls through to fresh-thread creation exactly like
//!   `current is None`.
//! * `title=public_name[:255]` is a Python code-point slice
//!   ([`truncate_title`]); a byte slice would panic on a UTF-8 boundary
//!   (Porting guide semantic trap).
//! * The new message's `seq` is `MAX(seq) + 1`
//!   (`assistant/runtime/events.py:78-80`, [`message_seq_sql`]) — not the
//!   message *count* the rotation check uses. Same value on dense
//!   `0..n` threads, different after gaps.
//! * `active_turn` needs only a non-null check (`dispatch.py:90`): any
//!   in-flight turn skips with `TURN_ACTIVE`, regardless of its status.
//! * `LoopTarget.DoesNotExist` returns `False` with no write; the broad
//!   `except Exception` records `DISPATCH_ERROR` and returns `False`
//!   (`dispatch.py:122-131`). A failure of that record write itself
//!   propagates, exactly as Python re-raising out of the handler.
//!
//! Fixture: FX-LOOP-05 (`fixtures/loop/tasks/`; recorded by PIDASHCONV-151).

use chrono::{DateTime, Utc};
use serde_json::json;
use sqlx::PgPool;
use uuid::Uuid;

#[cfg(test)]
use crate::loop_scan::FIRE_LOOP_TARGET_TASK;
use crate::queue::{enqueue_exec, JobRow, NewJob};
use crate::Error;
#[cfg(test)]
use pidash_db::r#loop::SkipReason;

// ---------------------------------------------------------------------------
// Task + assistant wire names, settings
// ---------------------------------------------------------------------------

/// Downstream assistant task (`assistant/tasks.py:477-483`,
/// `@shared_task(name="assistant.run_turn", ...)`). The handoff keeps its
/// Celery wire name; the runtime itself is out of scope.
pub const RUN_ASSISTANT_TURN_TASK: &str = "assistant.run_turn";

/// Per-thread message cap (`assistant/errors.py:106`).
pub const MAX_THREAD_MESSAGES: i64 = 200;
/// Rotation headroom setting (`loop/dispatch.py:46`).
pub const LOOP_ROTATION_HEADROOM_ENV: &str = "LOOP_ROTATION_HEADROOM";
/// Default headroom (`getattr(settings, "LOOP_ROTATION_HEADROOM", 30)`).
pub const DEFAULT_ROTATION_HEADROOM: i64 = 30;
/// Thread title bound (`assistant/models.py`, `title` `max_length=255`).
pub const THREAD_TITLE_MAX_CHARS: usize = 255;

/// Assistant thread kinds (`assistant/models.py:23-25`).
pub const THREAD_KIND_LOOP: &str = "loop";
/// Queued-turn status (`assistant/models.py:64-70`).
pub const TURN_STATUS_QUEUED: &str = "queued";
/// User message kind / completed status for the prompt message
/// (`assistant/models.py:108-121`, `dispatch.py:102-109`).
pub const MESSAGE_KIND_USER: &str = "user";
pub const MESSAGE_STATUS_COMPLETED: &str = "completed";

/// Rotation threshold (`loop/dispatch.py:46-47`):
/// `max(1, MAX_THREAD_MESSAGES - headroom)`. Pure for unit tests; the env
/// wrapper is [`rotation_threshold`].
pub fn rotation_threshold_raw(headroom: i64) -> i64 {
    (MAX_THREAD_MESSAGES - headroom).max(1)
}

/// Rotation threshold from the environment. Unparseable input falls back
/// to the default instead of crashing the tick (same policy as the
/// scanner's settings reads in `loop/scan.rs`).
pub fn rotation_threshold() -> i64 {
    let raw = std::env::var(LOOP_ROTATION_HEADROOM_ENV).ok();
    let headroom = raw
        .and_then(|v| v.trim().parse::<i64>().ok())
        .unwrap_or(DEFAULT_ROTATION_HEADROOM);
    rotation_threshold_raw(headroom)
}

/// `target.job.public_name[:255]` (`dispatch.py:64`): a Python code-point
/// slice, so over-long names truncate by `char`, never by byte.
pub fn truncate_title(public_name: &str) -> String {
    if public_name.chars().count() <= THREAD_TITLE_MAX_CHARS {
        return public_name.to_owned();
    }
    public_name.chars().take(THREAD_TITLE_MAX_CHARS).collect()
}

/// The `run_assistant_turn.delay(turn_id)` payload as a queue job:
/// `args=[str(turn_id)]`, `kwargs={}` (`dispatch.py:119`).
pub fn run_turn_job(turn_id: &Uuid) -> NewJob {
    NewJob::new(
        RUN_ASSISTANT_TURN_TASK,
        json!([turn_id.to_string()]),
        json!({}),
    )
}

/// The Celery v2 message a `run_turn` queue row becomes on the wire: same
/// task name, `args=[str(turn_id)]`, empty kwargs — byte-shape identical
/// to the Python `.delay(str(turn_id))` call. Repeats
/// [`crate::worker::dispatch`]'s forward path arm for arm (that function
/// is the runtime source of truth; this one exists so tests and the fire
/// port can assert the wire contract broker-free).
pub fn run_turn_message(turn_id: &Uuid) -> crate::celery::CeleryTaskMessage {
    let job = run_turn_job(turn_id);
    let args = match job.args {
        serde_json::Value::Array(items) => items,
        other => vec![other],
    };
    let kwargs = match job.kwargs {
        serde_json::Value::Object(map) => map,
        _ => serde_json::Map::new(),
    };
    crate::celery::CeleryTaskMessage::new(RUN_ASSISTANT_TURN_TASK, args, kwargs)
}

// ---------------------------------------------------------------------------
// Dispatch SQL (mirrors the Django ORM statements arm for arm)
// ---------------------------------------------------------------------------

/// Dispatch claim (`dispatch.py:82-86`): an SFU read with the job joined
/// and `.get(pk)` semantics. `FOR UPDATE OF t` locks only the target row
/// (the F-09 SFU pattern); the job join is inner over a non-nullable FK,
/// as Django emits for `select_related`. Workspace/user ids ride on the
/// target row, so no further join is needed. Only the default manager's
/// single `deleted_at IS NULL` applies: unlike the fire claim there is no
/// explicit filter.
pub fn dispatch_claim_sql() -> &'static str {
    "SELECT t.id AS id, t.workspace_id AS workspace_id, t.user_id AS user_id, \
     t.thread_id AS thread_id, j.prompt AS prompt, j.public_name AS public_name \
     FROM loop_targets AS t \
     INNER JOIN loop_jobs AS j ON j.id = t.job_id \
     WHERE t.id = $1 AND t.deleted_at IS NULL \
     FOR UPDATE OF t"
}

/// One dispatch claim row: the FK values `_ensure_thread` copies onto a
/// fresh thread plus the job payload the turn message carries.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct DispatchClaim {
    pub id: Uuid,
    pub workspace_id: Uuid,
    pub user_id: Uuid,
    pub thread_id: Option<Uuid>,
    pub prompt: String,
    pub public_name: String,
}

/// Thread reuse probe (`dispatch.py:50-53`):
/// `AssistantThread.objects.filter(pk=thread_id, kind=LOOP).first()`.
/// Binds `$1 = thread_id`.
pub fn thread_lookup_sql() -> &'static str {
    "SELECT id FROM assistant_thread WHERE id = $1 AND kind = 'loop'"
}

/// Rotation count (`dispatch.py:55`):
/// `AssistantMessage.objects.filter(thread=current).count()`.
/// Binds `$1 = thread_id`.
pub fn message_count_sql() -> &'static str {
    "SELECT COUNT(*) FROM assistant_message WHERE thread_id = $1"
}

/// Fresh hidden thread (`dispatch.py:60-66`):
/// `AssistantThread.objects.create(workspace, user, kind=LOOP,
/// title, is_archived=False)`. `active_turn_id` stays NULL;
/// `auto_now_add`/`auto_now` ride along explicitly.
pub fn thread_insert_sql() -> &'static str {
    "INSERT INTO assistant_thread \
     (id, workspace_id, user_id, title, kind, is_archived, created_at, updated_at) \
     VALUES ($1, $2, $3, $4, 'loop', FALSE, $5, $6) \
     RETURNING id"
}

/// Archive the rotated-out thread (`dispatch.py:68`).
pub fn thread_archive_sql() -> &'static str {
    "UPDATE assistant_thread SET is_archived = TRUE, updated_at = $1 WHERE id = $2"
}

/// Repoint the target at the fresh thread (`dispatch.py:69`).
pub fn target_repoint_sql() -> &'static str {
    "UPDATE loop_targets SET thread_id = $1, updated_at = $2 WHERE id = $3"
}

/// Re-lock the ensured thread (`dispatch.py:89` via
/// `events.create_message`'s `select_for_update().get(pk)` at
/// `assistant/runtime/events.py:106-121`): full row lock, of which the
/// dispatch reads `active_turn_id`.
pub fn thread_lock_sql() -> &'static str {
    "SELECT active_turn_id FROM assistant_thread WHERE id = $1 FOR UPDATE"
}

#[derive(Debug, Clone, sqlx::FromRow)]
struct LockedThread {
    pub active_turn_id: Option<Uuid>,
}

/// In-flight skip write (`dispatch.py:95-99`).
pub fn turn_active_skip_sql() -> &'static str {
    "UPDATE loop_targets SET last_skipped_at = $1, last_skip_reason = 'turn_active', \
     updated_at = $2 WHERE id = $3"
}

/// Queued turn (`dispatch.py:102`):
/// `AssistantTurn.objects.create(thread=locked, status=QUEUED)`.
pub fn turn_insert_sql() -> &'static str {
    "INSERT INTO assistant_turn (id, thread_id, status, created_at) \
     VALUES ($1, $2, 'queued', $3) \
     RETURNING id"
}

/// Next message sequence (`assistant/runtime/events.py:78-80`):
/// `MAX("seq") or 0`, then `+ 1` at the insert site. Binds
/// `$1 = thread_id`.
pub fn message_seq_sql() -> &'static str {
    "SELECT COALESCE(MAX(seq), 0) FROM assistant_message WHERE thread_id = $1"
}

/// Prompt message (`dispatch.py:103-109` via `events.create_message`):
/// completed USER message carrying `job.prompt` verbatim, linked to the
/// turn with an empty payload. `completed_at` stays NULL.
pub fn message_insert_sql() -> &'static str {
    "INSERT INTO assistant_message \
     (id, thread_id, turn_id, seq, kind, display_content, payload, status, created_at) \
     VALUES ($1, $2, $3, $4, 'user', $5, '{}', 'completed', $6) \
     RETURNING id"
}

/// Link the message back onto the turn (`dispatch.py:110-111`,
/// `turn.save(update_fields=["user_message"])`).
pub fn turn_link_message_sql() -> &'static str {
    "UPDATE assistant_turn SET user_message_id = $1 WHERE id = $2"
}

/// Raise the one-active-turn flag (`dispatch.py:112-113`,
/// `locked.save(update_fields=["active_turn", "updated_at"])`).
pub fn thread_set_active_sql() -> &'static str {
    "UPDATE assistant_thread SET active_turn_id = $1, updated_at = $2 WHERE id = $3"
}

/// Pin the run on the target (`dispatch.py:115-117`): `last_run=turn`,
/// `last_skip_reason=""` (empty = never skipped), `updated_at` now.
/// `last_skipped_at` is deliberately untouched.
pub fn target_pin_run_sql() -> &'static str {
    "UPDATE loop_targets SET last_run_id = $1, last_skip_reason = '', \
     updated_at = $2 WHERE id = $3"
}

/// Unexpected-error record (`dispatch.py:126-130`): `DISPATCH_ERROR` with
/// `last_skipped_at` now, no `deleted_at` filter (plain `.filter(pk)`).
pub fn dispatch_error_sql() -> &'static str {
    "UPDATE loop_targets SET last_skipped_at = $1, last_skip_reason = 'dispatch_error', \
     updated_at = $2 WHERE id = $3"
}

// ---------------------------------------------------------------------------
// _ensure_thread + dispatch_loop_turn
// ---------------------------------------------------------------------------

/// Usable hidden loop thread for one target (`dispatch.py:39-71`):
/// reuse the current LOOP thread while its message count is strictly
/// below the rotation threshold, else create a fresh hidden thread,
/// archive the old one (kept for admin history), and repoint the target.
/// Rotation resets the run's cross-run memory — acceptable, because job
/// prompts must not depend on memory for correctness, only benefit from
/// it. Runs inside the caller's dispatch transaction.
pub async fn ensure_thread(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    target_id: &Uuid,
    workspace_id: &Uuid,
    user_id: &Uuid,
    thread_id: Option<Uuid>,
    public_name: &str,
    now: &DateTime<Utc>,
) -> Result<Uuid, Error> {
    let current: Option<(Uuid,)> = match thread_id {
        Some(id) => {
            sqlx::query_as(thread_lookup_sql())
                .bind(id)
                .fetch_optional(&mut **tx)
                .await?
        }
        None => None,
    };
    if let Some((id,)) = current {
        let count: i64 = sqlx::query_scalar(message_count_sql())
            .bind(id)
            .fetch_one(&mut **tx)
            .await?;
        if count < rotation_threshold() {
            return Ok(id);
        }
    }

    // Create a fresh thread; archive the old one (kept for admin history).
    let fresh = Uuid::new_v4();
    sqlx::query(thread_insert_sql())
        .bind(fresh)
        .bind(*workspace_id)
        .bind(*user_id)
        .bind(truncate_title(public_name))
        .bind(*now)
        .bind(*now)
        .execute(&mut **tx)
        .await?;
    if let Some((old,)) = current {
        sqlx::query(thread_archive_sql())
            .bind(*now)
            .bind(old)
            .execute(&mut **tx)
            .await?;
    }
    sqlx::query(target_repoint_sql())
        .bind(fresh)
        .bind(*now)
        .bind(*target_id)
        .execute(&mut **tx)
        .await?;
    Ok(fresh)
}

/// What the dispatch transaction settled.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Inner {
    /// Queued (`true`) or `TURN_ACTIVE`-skipped (`false`).
    Settled(bool),
    /// Target row gone (`LoopTarget.DoesNotExist`, `dispatch.py:122-123`):
    /// no write, `False`.
    Missing,
}

/// Dispatch body inside its own transaction (`dispatch.py:81-117`).
/// Errors propagate to [`dispatch_loop_turn`], which records
/// `DISPATCH_ERROR` — the broad `except Exception` at `:124-131`.
async fn dispatch_inner(pool: &PgPool, target_id: &Uuid) -> Result<Inner, Error> {
    let mut tx = pool.begin().await?;
    let claim: Option<DispatchClaim> = sqlx::query_as(dispatch_claim_sql())
        .bind(*target_id)
        .fetch_optional(&mut *tx)
        .await?;
    let Some(claim) = claim else {
        return Ok(Inner::Missing);
    };
    let now = Utc::now();
    let thread_id = ensure_thread(
        &mut tx,
        &claim.id,
        &claim.workspace_id,
        &claim.user_id,
        claim.thread_id,
        &claim.public_name,
        &now,
    )
    .await?;

    let locked: LockedThread = sqlx::query_as(thread_lock_sql())
        .bind(thread_id)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or(sqlx::Error::RowNotFound)?;
    if locked.active_turn_id.is_some() {
        // Previous run still in flight — skip, don't queue (same policy
        // as the scanner). The assistant stale-turn sweep guarantees
        // active_turn eventually clears even after a worker crash, so a
        // target can never wedge permanently.
        sqlx::query(turn_active_skip_sql())
            .bind(now)
            .bind(now)
            .bind(claim.id)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        return Ok(Inner::Settled(false));
    }

    let turn_id = Uuid::new_v4();
    sqlx::query(turn_insert_sql())
        .bind(turn_id)
        .bind(thread_id)
        .bind(now)
        .execute(&mut *tx)
        .await?;
    let max_seq: i64 = sqlx::query_scalar(message_seq_sql())
        .bind(thread_id)
        .fetch_one(&mut *tx)
        .await?;
    let message_id = Uuid::new_v4();
    sqlx::query(message_insert_sql())
        .bind(message_id)
        .bind(thread_id)
        .bind(turn_id)
        .bind(max_seq + 1)
        .bind(claim.prompt.clone())
        .bind(now)
        .execute(&mut *tx)
        .await?;
    sqlx::query(turn_link_message_sql())
        .bind(message_id)
        .bind(turn_id)
        .execute(&mut *tx)
        .await?;
    sqlx::query(thread_set_active_sql())
        .bind(turn_id)
        .bind(now)
        .bind(thread_id)
        .execute(&mut *tx)
        .await?;
    sqlx::query(target_pin_run_sql())
        .bind(turn_id)
        .bind(now)
        .bind(claim.id)
        .execute(&mut *tx)
        .await?;
    // `transaction.on_commit(run_assistant_turn.delay(turn_id))`: the job
    // row commits or rolls back with the turn, so a failed dispatch never
    // emits a phantom run.
    enqueue_exec(&mut *tx, &run_turn_job(&turn_id)).await?;
    tx.commit().await?;
    tracing::info!(turn = %turn_id, target = %target_id, "loop.dispatch: queued turn");
    Ok(Inner::Settled(true))
}

/// Record an unexpected dispatch failure (`dispatch.py:126-130`).
async fn record_dispatch_error(pool: &PgPool, target_id: &Uuid) -> Result<(), Error> {
    let now = Utc::now();
    sqlx::query(dispatch_error_sql())
        .bind(now)
        .bind(now)
        .bind(*target_id)
        .execute(pool)
        .await?;
    Ok(())
}

/// Create the hidden-thread turn for `target_id` and queue execution
/// (`dispatch.py:74-131`). Returns `True` when a turn was queued, `False`
/// when skipped (a previous run is still in flight) or on an unexpected
/// dispatch error.
pub async fn dispatch_loop_turn(pool: &PgPool, target_id: &Uuid) -> Result<bool, Error> {
    match dispatch_inner(pool, target_id).await {
        Ok(Inner::Settled(queued)) => Ok(queued),
        Ok(Inner::Missing) => Ok(false),
        Err(error) => {
            tracing::error!(target = %target_id, error = %error, "loop.dispatch: unexpected error");
            record_dispatch_error(pool, target_id).await?;
            Ok(false)
        }
    }
}

/// Parse the `fire_loop_target.delay(str(target_id))` wire args back into
/// a target id. The fan-out is `args=[str(id)]`, `kwargs={}`
/// (`bgtasks/loop.py:168`); anything else fails the task (with
/// `max_retries=0` there is no retry, mirroring Django raising on a bad
/// `pk` lookup instead of skipping).
pub fn target_id_from_job(job: &JobRow) -> Result<Uuid, String> {
    let first = job
        .args
        .as_array()
        .and_then(|args| args.first())
        .and_then(|value| value.as_str());
    match first {
        Some(raw) => Uuid::parse_str(raw)
            .map_err(|_| format!("fire_loop_target: invalid target id arg {raw:?}")),
        None => Err("fire_loop_target: expected args=[target_id]".to_owned()),
    }
}

/// Assert at registration time that the fire task name still matches the
/// scanner's fan-out contract. Both modules speak the same wire payloads,
/// so no fan-out is ever dropped or double-run across the handoff.
#[cfg(test)]
fn assert_fire_contract() {
    assert_eq!(
        FIRE_LOOP_TARGET_TASK,
        "pi_dash.bgtasks.loop.fire_loop_target"
    );
    assert_eq!(RUN_ASSISTANT_TURN_TASK, "assistant.run_turn");
    assert_eq!(SkipReason::TurnActive.as_str(), "turn_active");
    assert_eq!(SkipReason::DispatchError.as_str(), "dispatch_error");
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixtures_dir() -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/loop/tasks")
    }

    fn fixture(name: &str) -> serde_json::Value {
        let text = std::fs::read_to_string(fixtures_dir().join(name))
            .unwrap_or_else(|e| panic!("read fixture {name}: {e}"));
        serde_json::from_str(&text).expect("fixture is valid JSON")
    }

    fn norm(sql: &str) -> String {
        sql.to_ascii_lowercase()
            .replace('"', "")
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
    }

    fn parse(sql: &str) -> sqlparser::ast::Statement {
        let mut stmts =
            sqlparser::parser::Parser::parse_sql(&sqlparser::dialect::PostgreSqlDialect {}, sql)
                .unwrap_or_else(|e| panic!("SQL parses: {e}\n{sql}"));
        assert_eq!(stmts.len(), 1);
        stmts.pop().unwrap()
    }

    // Rotation threshold (`loop/dispatch.py:46-47`,
    // `dispatch_rotation.json`): `max(1, 200 - headroom)` with the default
    // headroom 30 giving 170. The boundary is strict `<`: a count of
    // exactly 170 rotates.
    #[test]
    fn rotation_threshold_matches_python() {
        let golden = fixture("dispatch_rotation.json");
        assert_eq!(golden["rotation_threshold"].as_i64().unwrap(), 170);
        assert_eq!(rotation_threshold_raw(30), 170);
        assert_eq!(rotation_threshold_raw(0), 200);
        assert_eq!(rotation_threshold_raw(199), 1);
        // The `max(1, ...)` floor: a headroom at or past the cap still
        // leaves a one-message grace instead of rotating every fire.
        assert_eq!(rotation_threshold_raw(200), 1);
        assert_eq!(rotation_threshold_raw(10_000), 1);
        assert_eq!(rotation_threshold_raw(-5), 205);
        // `getattr(settings, "LOOP_ROTATION_HEADROOM", 30)`: garbage falls
        // back instead of crashing the tick.
        let saved = std::env::var(LOOP_ROTATION_HEADROOM_ENV).ok();
        std::env::set_var(LOOP_ROTATION_HEADROOM_ENV, "bogus");
        assert_eq!(rotation_threshold(), 170);
        std::env::set_var(LOOP_ROTATION_HEADROOM_ENV, "10");
        assert_eq!(rotation_threshold(), 190);
        match saved {
            Some(v) => std::env::set_var(LOOP_ROTATION_HEADROOM_ENV, v),
            None => std::env::remove_var(LOOP_ROTATION_HEADROOM_ENV),
        }
    }

    // Fresh-thread title (`dispatch.py:64`, `dispatch_rotation.json`):
    // `job.public_name[:255]` counts code points — a byte slice would
    // panic or split a character on a UTF-8 boundary.
    #[test]
    fn title_truncation_counts_code_points() {
        assert_eq!(truncate_title("Pub name"), "Pub name");
        assert_eq!(truncate_title(""), "");
        let exactly: String = "a".repeat(255);
        assert_eq!(truncate_title(&exactly), exactly);
        let over: String = "b".repeat(300);
        assert_eq!(truncate_title(&over), "b".repeat(255));
        // Multi-byte content: 255 code points kept whole even though the
        // UTF-8 length is far past 255 bytes.
        let emoji: String = "�-loop-".repeat(60);
        let cut = truncate_title(&emoji);
        assert_eq!(cut.chars().count(), 255);
        assert!(emoji.starts_with(&cut));
        // A long public name would panic under `[..255]` byte slicing;
        // the char path never splits a boundary.
        assert!(cut.is_char_boundary(cut.len()));
    }

    // Downstream handoff (`dispatch.py:119`, `fire.before_after.json`):
    // `run_assistant_turn.delay(turn_id)` is `args=[str(turn_id)]` with
    // empty kwargs under the `assistant.run_turn` wire name — the Celery
    // v2 body the worker forward path publishes.
    #[test]
    fn run_turn_message_is_celery_wire_identical() {
        use serde_json::json;
        let golden = fixture("fire.before_after.json");
        assert_eq!(
            golden["run_assistant_turn_delay_calls"].as_i64().unwrap(),
            1
        );
        let turn = Uuid::parse_str("123e4567-e89b-12d3-a456-426614174000").unwrap();
        let job = run_turn_job(&turn);
        assert_eq!(job.task, RUN_ASSISTANT_TURN_TASK);
        assert_eq!(job.args, json!([turn.to_string()]));
        assert_eq!(job.kwargs, json!({}));
        let message = run_turn_message(&turn);
        assert_eq!(message.task, RUN_ASSISTANT_TURN_TASK);
        assert_eq!(message.args, vec![json!(turn.to_string())]);
        assert!(message.kwargs.is_empty());
        assert_eq!(message.retries, 0);
        assert_eq!(
            message.body(),
            json!([
                [turn.to_string()],
                {},
                {"callbacks": null, "errbacks": null, "chain": null, "chord": null}
            ])
        );
        let headers = message.headers();
        assert_eq!(headers["task"], RUN_ASSISTANT_TURN_TASK);
        assert_eq!(headers["id"], message.id);
        assert_eq!(headers["retries"], 0);
        assert_fire_contract();
    }

    // Fixture cross-checks: the recorded skip reasons are the exact
    // `SkipReason` values the updates write, and the happy path pins an
    // empty reason with the prompt carried verbatim onto the message.
    #[test]
    fn fixture_reasons_match_skip_reason_values() {
        assert_eq!(
            fixture("dispatch_turn_active.json")["last_skip_reason"]
                .as_str()
                .unwrap(),
            SkipReason::TurnActive.as_str()
        );
        assert_eq!(
            fixture("dispatch_error.json")["last_skip_reason"]
                .as_str()
                .unwrap(),
            SkipReason::DispatchError.as_str()
        );
        let happy = fixture("fire.before_after.json");
        assert!(happy["returned"].as_bool().unwrap());
        assert_eq!(happy["after"]["thread_kind"].as_str().unwrap(), "loop");
        assert_eq!(
            happy["user_message_is_prompt"].as_str().unwrap(),
            "do the thing"
        );
        assert_eq!(
            fixture("dispatch_rotation.json")["new_thread_title"]
                .as_str()
                .unwrap(),
            "Pub name"
        );
    }

    // Dispatch SQL parses and carries every ORM arm: the claim locks only
    // the target row over an inner job join with the manager's single
    // `deleted_at` filter (no explicit one — unlike the fire claim); the
    // thread probe is LOOP-kind-scoped; the turn/message writes pin the
    // exact literal statuses the contract suite asserts (`queued`,
    // `user`, `completed`); the skip writes pin the exact reason values.
    #[test]
    fn dispatch_sql_carries_every_python_arm() {
        let claim = dispatch_claim_sql();
        assert!(matches!(parse(claim), sqlparser::ast::Statement::Query(_)));
        let mine = norm(claim);
        for fragment in [
            "from loop_targets as t",
            "inner join loop_jobs as j on j.id = t.job_id",
            "t.id = $1",
            "t.deleted_at is null",
            "for update of t",
            "j.prompt as prompt",
            "j.public_name as public_name",
        ] {
            assert!(mine.contains(fragment), "missing {fragment} in:\n{claim}");
        }
        // Exactly one manager condition — the dispatch `.get()` has no
        // explicit `deleted_at__isnull` to double it.
        assert_eq!(mine.matches("t.deleted_at is null").count(), 1);

        let probe = thread_lookup_sql();
        assert!(matches!(parse(probe), sqlparser::ast::Statement::Query(_)));
        let mine = norm(probe);
        assert!(mine.contains("from assistant_thread where id = $1"));
        assert!(mine.contains("kind = 'loop'"));

        let count = message_count_sql();
        assert!(matches!(parse(count), sqlparser::ast::Statement::Query(_)));
        assert!(norm(count).contains("count(*)"));
        assert!(norm(count).contains("from assistant_message where thread_id = $1"));

        let seq = message_seq_sql();
        assert!(matches!(parse(seq), sqlparser::ast::Statement::Query(_)));
        let mine = norm(seq);
        // `MAX(seq) or 0` — the transcript-ordering source, not a count.
        assert!(mine.contains("coalesce(max(seq), 0)"));
        assert!(mine.contains("from assistant_message where thread_id = $1"));

        // Re-lock (`dispatch.py:89`): a SELECT … FOR UPDATE read of the
        // one-active-turn flag — a lock, not a write.
        let lock = thread_lock_sql();
        assert!(matches!(parse(lock), sqlparser::ast::Statement::Query(_)));
        let mine = norm(lock);
        assert!(mine.contains("select active_turn_id"));
        assert!(mine.contains("from assistant_thread where id = $1"));
        assert!(mine.contains("for update"));

        for (sql, fragments) in [
            (
                thread_insert_sql(),
                vec![
                    "insert into assistant_thread",
                    "workspace_id",
                    "user_id",
                    "title",
                    "kind",
                    "is_archived",
                    "'loop'",
                    "false",
                    "returning id",
                ],
            ),
            (
                thread_archive_sql(),
                vec!["update assistant_thread", "is_archived = true"],
            ),
            (
                target_repoint_sql(),
                vec!["update loop_targets", "thread_id = $1"],
            ),
            (
                turn_insert_sql(),
                vec![
                    "insert into assistant_turn",
                    "thread_id",
                    "'queued'",
                    "returning id",
                ],
            ),
            (
                message_insert_sql(),
                vec![
                    "insert into assistant_message",
                    "turn_id",
                    "seq",
                    "'user'",
                    "'completed'",
                    "'{}'",
                    "display_content",
                    "returning id",
                ],
            ),
            (
                turn_link_message_sql(),
                vec!["update assistant_turn", "user_message_id = $1"],
            ),
            (
                thread_set_active_sql(),
                vec!["update assistant_thread", "active_turn_id = $1"],
            ),
            (
                target_pin_run_sql(),
                vec![
                    "update loop_targets",
                    "last_run_id = $1",
                    "last_skip_reason = ''",
                ],
            ),
            (
                turn_active_skip_sql(),
                vec![
                    "update loop_targets",
                    "last_skipped_at = $1",
                    "last_skip_reason = 'turn_active'",
                ],
            ),
            (
                dispatch_error_sql(),
                vec![
                    "update loop_targets",
                    "last_skipped_at = $1",
                    "last_skip_reason = 'dispatch_error'",
                ],
            ),
        ] {
            let parsed = parse(sql);
            assert!(
                matches!(
                    parsed,
                    sqlparser::ast::Statement::Insert(_) | sqlparser::ast::Statement::Update(_)
                ),
                "not a write:\n{sql}"
            );
            let mine = norm(sql);
            for fragment in fragments {
                assert!(mine.contains(fragment), "missing {fragment} in:\n{sql}");
            }
        }
    }

    // Wire-arg parsing (`bgtasks/loop.py:168`, `args=[str(id)]`): a UUID
    // string parses, anything else fails the task instead of skipping.
    #[test]
    fn target_id_parsing_rejects_non_uuid_args() {
        use crate::queue::JobRow;
        let id = "123e4567-e89b-12d3-a456-426614174000";
        fn row_with(args: serde_json::Value) -> JobRow {
            JobRow {
                id: 1,
                celery_id: "celery-id".to_owned(),
                task: FIRE_LOOP_TARGET_TASK.to_owned(),
                args,
                kwargs: serde_json::json!({}),
                queue: "celery".to_owned(),
                status: "queued".to_owned(),
                attempts: 0,
                max_retries: 3,
                visible_at: Utc::now(),
                claimed_at: None,
                claimed_by: None,
                created_at: Utc::now(),
                last_error: None,
            }
        }
        let row = row_with(serde_json::json!([id]));
        assert_eq!(
            target_id_from_job(&row).unwrap(),
            Uuid::parse_str(id).unwrap()
        );
        let bad = row_with(serde_json::json!(["not-a-uuid"]));
        assert!(target_id_from_job(&bad).is_err());
        let empty = row_with(serde_json::json!([]));
        assert!(target_id_from_job(&empty).is_err());
    }
}
