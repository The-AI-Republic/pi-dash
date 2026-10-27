//! Live [`TurnSeam`] (PIDASHCONV-863, phase 7).
//!
//! The wiring behind `drive_turn` (`pidash_jobs::assistant::run_turn`), built
//! from the modules the split issues shipped: the agent loop
//! ([`super::agent`], 863 phase 6), the `ModelMessage` codec
//! ([`super::history`], 1110), provider streaming ([`super::streaming`],
//! 1111), the MCP client ([`super::mcp_client`], 1112) and the 17 local
//! tools ([`super::tools_local`], 1113), plus the D-11 model runtime in the
//! jobs crate (`Lane`, `ProviderTable`, `WireTool`) and the ported decision
//! surfaces in `pidash_services` (BYOK gates, key cache, MCP planning,
//! SSRF).
//!
//! Every method cites its Python original (`assistant/tasks.py:1-508`); the
//! SQL shapes stay with the crate that already ports them — the jobs crate
//! ([`rt::load_context_sql`] and friends, `tasks.py`) and
//! `pidash_db::assistant::event_queries` (`events.py` / `history.py`) — so
//! this module composes them instead of re-deriving SQL.
//!
//! Deliberate divergences from Python, recorded in PR #1682:
//!
//! * The agent run executes on a spawned task that feeds the driver through
//!   a channel, so `next_event` keeps the `async for` shape while the
//!   provider request stays request-atomic (`streaming::stream_turn`
//!   buffers within one request). Cancel granularity is per stream item
//!   rather than per SSE delta, and dropping the stream aborts the run.
//! * The ported [`rt::ToolsetHandle`] is a string, so each handle carries
//!   its turn id; `runtime_failures` reads the live sessions back through
//!   the per-turn record the handle names. The record is held only for the
//!   life of the turn's [`LiveStream`] and reclaimed when the driver drops
//!   it (plus on the pre-stream error exits where no stream exists), so the
//!   process-lifetime seam never retains a resolved lane or an old toolset.
//! * The async decrypt runs before the ported synchronous key cache is
//!   consulted, so a cache hit costs one extra KMS round-trip; the caching
//!   policy itself (TTL, LRU, ciphertext keying, empty-token bypass) is the
//!   ported [`KeyCache`].

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use chrono::{DateTime, Utc};
use pidash_db::assistant::event_queries as events;
use pidash_db::assistant::models::ThreadKind;
use pidash_db::config::Settings;
use pidash_jobs::assistant::run_turn as rt;
use pidash_jobs::assistant::run_turn::{
    AgentError, ModelError, SeamError, SkippedServer, StreamItem, ToolsetHandle, TurnContext,
    TurnSeam, TurnUsage,
};
use pidash_jobs::oss_worker::model_runtime::provider::{self, Lane, WireTool};
use pidash_jobs::oss_worker::model_runtime::schemas::Golden;
use pidash_jobs::oss_worker::model_runtime::usage::{ProviderTable, RunUsage, UsageNumber};
use pidash_services::assistant::agent::AssistantDeps;
use pidash_services::assistant::crypto::CryptoError;
use pidash_services::assistant::llm::{self as llm_service, KeyCache};
use pidash_services::assistant::mcp as mcp_service;
use pidash_services::assistant::ssrf;
use serde_json::{Map, Value};
use sqlx::{PgPool, Row};
use uuid::Uuid;

use super::agent::{
    run_agent, ToolCallOutcome, ToolKind, Toolset as AgentToolset, TurnAgentConfig,
    TurnAgentOutcome, DEFAULT_TIMEOUT_SECS,
};
use super::history::{self, TurnMessage};
use super::instructions::{self, InstructionsContext};
use super::mcp_client::{CallResult, McpContent, McpToolDef, McpValue, ResilientSession};
use super::tool_schemas;
use super::tools_local::{self, ToolFailure};
use super::tools_local_store::{PgToolStore, RedisClient, ToolEnv};

// --------------------------------------------------------------------------- //
// Queries the seam owns (every other shape is already ported)
// --------------------------------------------------------------------------- //

/// Column indexes of [`rt::load_context_sql`]'s select list (`t.*`, `th.*`,
/// `w.id/slug/name`, `m.id/display_content`).
const CTX_COL_TURN_ID: usize = 0;
const CTX_COL_TURN_USER_MESSAGE_ID: usize = 2;
const CTX_COL_TURN_STATUS: usize = 3;
const CTX_COL_THREAD_ID: usize = 12;
const CTX_COL_THREAD_WORKSPACE_ID: usize = 13;
const CTX_COL_THREAD_USER_ID: usize = 14;
const CTX_COL_THREAD_KIND: usize = 16;
const CTX_COL_WORKSPACE_SLUG: usize = 22;
const CTX_COL_WORKSPACE_NAME: usize = 23;
const CTX_COL_USER_MESSAGE_CONTENT: usize = 25;

/// `user.display_name or user.email or ""` (`tasks.py:88`); the unauthored
/// fallback is the empty string (`or ""`).
const USER_DISPLAY_SQL: &str =
    "SELECT COALESCE(NULLIF(display_name, ''), email, '') FROM users WHERE id = $1";

/// `core.permissions.workspace_role` (`permissions.py:37-45`): the first
/// active membership role for the workspace; `or 0` at the call site
/// (`tasks.py:85`).
const WORKSPACE_ROLE_SQL: &str = "SELECT role FROM workspace_members \
     WHERE workspace_id = $1 AND member_id = $2 AND is_active LIMIT 1";

/// `UserLLMConfig.objects.filter(user=user).first()` (`llm.py:67-68`).
const LLM_CONFIG_SQL: &str = "SELECT provider_kind, base_url, model_name, api_key_encrypted \
     FROM assistant_user_llm_config WHERE user_id = $1 \
     ORDER BY created_at ASC LIMIT 1";

/// `AssistantMCPServer.objects.filter(user=user, is_enabled=True)`
/// (`mcp.py:250`) in the model's default `created_at` order.
const MCP_SERVERS_SQL: &str = "SELECT id, name, url, auth_header_encrypted \
     FROM assistant_mcp_server WHERE user_id = $1 AND is_enabled \
     ORDER BY created_at ASC, id ASC";

/// Read back one message row for the `message_completed` envelope
/// (`events.message_envelope(message)`, `tasks.py:150-153`).
const MESSAGE_ROW_SQL: &str =
    "SELECT seq, payload, created_at FROM assistant_message WHERE id = $1";

/// The turn's last `assistant` row (`_mark_cut_off`, `tasks.py:518-520`):
/// highest `seq` first, for the payload flag and the re-sent envelope.
const LAST_ASSISTANT_ROW_SQL: &str = "SELECT id, seq, payload, display_content, status, \
     created_at, completed_at FROM assistant_message \
     WHERE turn_id = $1 AND kind = 'assistant' ORDER BY seq DESC LIMIT 1";

/// Flag the row's payload (`message.save(update_fields=["payload"])`,
/// `tasks.py:523-524`).
const MARK_CUT_OFF_SQL: &str = "UPDATE assistant_message SET payload = $2 WHERE id = $1";

/// [`LAST_ASSISTANT_ROW_SQL`] row: id, seq, payload, display_content,
/// status, created_at, completed_at.
type LastAssistantRow = (
    Uuid,
    i64,
    Value,
    String,
    String,
    DateTime<Utc>,
    Option<DateTime<Utc>>,
);

/// Lift a ported decision-surface skip (`mcp.SkippedServer`, services) into
/// the seam's boundary type (`rt::SkippedServer`, jobs); same two fields.
fn port_skipped(server: &mcp_service::SkippedServer) -> SkippedServer {
    SkippedServer {
        name: server.name.clone(),
        reason: server.reason.clone(),
    }
}

fn db_error(error: sqlx::Error) -> SeamError {
    SeamError(error.to_string())
}

fn render_stamp(now: DateTime<Utc>) -> String {
    crate::runner_enroll::enroll::render_isoformat(now)
}

fn empty_payload() -> Value {
    Value::Object(Map::new())
}

/// `_timeout_setting` / `_read_timeout_setting` (`mcp.py:299-304`): the
/// `ASSISTANT_MCP_*` settings are not part of the ported `Settings`, so the
/// environment is read directly (unset in every shipped profile, where the
/// pinned defaults apply).
///
/// Public for the cloud overlay's turn seam (OVL-07), which copied this
/// read; the overlay's twin stays until OVL-12 re-points it here.
pub fn mcp_timeouts() -> (f64, f64) {
    let read = |name: &str| -> Option<f64> {
        std::env::var(name)
            .ok()
            .and_then(|raw| raw.trim().parse::<f64>().ok())
    };
    (
        mcp_service::timeout_setting(read("ASSISTANT_MCP_TIMEOUT_S")),
        mcp_service::read_timeout_setting(read("ASSISTANT_MCP_READ_TIMEOUT_S")),
    )
}

/// `max_servers` (`mcp.py:307-309`), same environment read as above.
///
/// Public for the cloud overlay's turn seam (OVL-07), which copied this
/// read; the overlay's twin stays until OVL-12 re-points it here.
pub fn mcp_max_servers() -> usize {
    let override_value = std::env::var("ASSISTANT_MCP_MAX_SERVERS")
        .ok()
        .and_then(|raw| raw.trim().parse::<i64>().ok());
    mcp_service::max_servers(override_value)
}

fn usage_number_to_i64(number: UsageNumber) -> i64 {
    match number {
        UsageNumber::Int(value) => value,
        UsageNumber::Float(value) => value as i64,
    }
}

/// `_extract_usage`'s copy (`tasks.py:428-439`): the five counters, each
/// present on a live `RunUsage`.
fn turn_usage_from_run(usage: &RunUsage) -> TurnUsage {
    TurnUsage {
        input_tokens: Some(usage_number_to_i64(usage.input_tokens)),
        output_tokens: Some(usage_number_to_i64(usage.output_tokens)),
        total_tokens: Some(usage_number_to_i64(usage.total_tokens())),
        requests: Some(usage.requests),
        tool_calls: Some(usage.tool_calls),
    }
}

// --------------------------------------------------------------------------- //
// The stream
// --------------------------------------------------------------------------- //

/// One turn's agent stream: the driver's receive half of the spawned run,
/// the outcome slot `dump_messages`/`extract_usage` read, the task handle so
/// dropping the stream (cancel, failure) aborts the run, and the shared
/// per-turn record so dropping the stream reclaims it.
pub struct LiveStream {
    rx: tokio::sync::mpsc::UnboundedReceiver<StreamMessage>,
    outcome: Arc<Mutex<Option<TurnAgentOutcome>>>,
    handle: tokio::task::JoinHandle<()>,
    /// The seam's per-turn record map (shared, so the seam's process-lifetime
    /// owner and this stream agree). The driver holds the stream for the whole
    /// drive and reads the record through the seam up to `complete_turn`/
    /// `cancel_turn`/`fail_turn`, so reclaiming on `Drop` is the point where
    /// the record is provably no longer needed.
    runs: Arc<Mutex<HashMap<Uuid, PreparedTurn>>>,
    turn_id: Uuid,
}

enum StreamMessage {
    Item(StreamItem),
    /// Terminal record. The outcome is already in the slot when this
    /// arrives, so the reads after the stream ends never race the task.
    End(Result<(), AgentError>),
}

impl Drop for LiveStream {
    fn drop(&mut self) {
        self.handle.abort();
        // Release the resolved lane (decrypted BYOK key included) and the
        // toolset as soon as the drive is done with them, like Python's
        // `_run_turn` releasing its `model`/`toolsets` locals.
        reclaim_runs(&self.runs, self.turn_id);
    }
}

/// Drop one turn's record from the shared map. Removing an absent record is
/// a no-op, so the explicit reclaims, the stream's `Drop` and the
/// [`OpenStreamGuard`] overlap safely.
fn reclaim_runs(runs: &Mutex<HashMap<Uuid, PreparedTurn>>, turn_id: Uuid) {
    if let Ok(mut runs) = runs.lock() {
        runs.remove(&turn_id);
    }
}

/// Reclaims the per-turn record when `open_stream` exits without a stream.
///
/// `drive_turn` wraps `open_stream` in `timeout_at`: when the soft limit
/// fires during MCP entry the future is dropped at the `enter()`/`tools()`
/// awaits, where no explicit reclaim runs and no [`LiveStream`] exists yet.
/// The guard is defused once the stream takes ownership of the reclaim (its
/// `Drop`).
struct OpenStreamGuard {
    runs: Arc<Mutex<HashMap<Uuid, PreparedTurn>>>,
    turn_id: Uuid,
    defused: bool,
}

impl OpenStreamGuard {
    fn defuse(mut self) {
        self.defused = true;
    }
}

impl Drop for OpenStreamGuard {
    fn drop(&mut self) {
        if !self.defused {
            reclaim_runs(&self.runs, self.turn_id);
        }
    }
}

// --------------------------------------------------------------------------- //
// The concrete toolset: local executors + per-run MCP sessions
// --------------------------------------------------------------------------- //

struct McpLeg {
    server_name: String,
    session: tokio::sync::Mutex<ResilientSession>,
    /// Tools listed at toolset entry (`MCPToolset.get_tools`); the agent
    /// loop's `wire_tools` is synchronous, so listing happens in
    /// `open_stream`.
    defs: Mutex<Vec<McpToolDef>>,
}

impl McpLeg {
    fn names(&self) -> Vec<String> {
        match self.defs.lock() {
            Ok(defs) => defs.iter().map(|def| def.name.clone()).collect(),
            Err(_) => Vec::new(),
        }
    }

    fn has_tool(&self, name: &str) -> bool {
        match self.defs.lock() {
            Ok(defs) => defs.iter().any(|def| def.name == name),
            Err(_) => false,
        }
    }

    fn wire_tools(&self, anthropic: bool) -> Vec<WireTool> {
        match self.defs.lock() {
            Ok(defs) => defs
                .iter()
                .map(|def| {
                    wire_tool(
                        &def.name,
                        def.description.as_deref().unwrap_or_default(),
                        &def.input_schema,
                        anthropic,
                    )
                })
                .collect(),
            Err(_) => Vec::new(),
        }
    }
}

/// One turn's tool boundary: the 17 local function tools (1113's schemas
/// and executors) followed by the user's enabled MCP toolsets (1112's
/// resilient sessions), in the registration order the captures pin
/// (`tool_local`/`tool_mcp`: local functions in `@assistant.tool` import
/// order, then the per-run MCP toolsets).
pub struct LiveToolset {
    store: Arc<PgToolStore>,
    deps: tokio::sync::Mutex<AssistantDeps>,
    env: ToolEnv,
    anthropic: bool,
    local: Vec<tool_schemas::ToolSchema>,
    legs: Vec<McpLeg>,
}

impl AgentToolset for LiveToolset {
    fn wire_tools(&self) -> Vec<WireTool> {
        let mut tools: Vec<WireTool> = self
            .local
            .iter()
            .map(|schema| {
                wire_tool(
                    schema.name,
                    schema.description,
                    &schema.parameters,
                    self.anthropic,
                )
            })
            .collect();
        for leg in &self.legs {
            tools.extend(leg.wire_tools(self.anthropic));
        }
        tools
    }

    fn names_sorted(&self) -> Vec<String> {
        let mut names: Vec<String> = self
            .local
            .iter()
            .map(|schema| schema.name.to_owned())
            .collect();
        for leg in &self.legs {
            names.extend(leg.names());
        }
        names.sort();
        names
    }

    fn kind(&self, name: &str) -> ToolKind {
        if self.local.iter().any(|schema| schema.name == name) {
            return ToolKind::Function;
        }
        if self.legs.iter().any(|leg| leg.has_tool(name)) {
            return ToolKind::Mcp;
        }
        ToolKind::Unknown
    }

    async fn call(&self, name: &str, args: Map<String, Value>, _call_id: &str) -> ToolCallOutcome {
        if self.local.iter().any(|schema| schema.name == name) {
            // `deps` is per-run mutable state (`RunBudget` counters); the
            // agent loop issues tool calls sequentially, so the lock is
            // never contended.
            let mut deps = self.deps.lock().await;
            return match tools_local::execute_local_tool(
                self.store.as_ref(),
                &mut deps,
                &self.env,
                name,
                &Value::Object(args),
            )
            .await
            {
                Ok(value) => ToolCallOutcome::Ok(value),
                Err(ToolFailure::Retry { message }) => ToolCallOutcome::Retry(message),
                Err(ToolFailure::InvalidArgs { details }) => ToolCallOutcome::InvalidArgs(details),
                Err(ToolFailure::Internal { message }) => ToolCallOutcome::Internal(message),
            };
        }
        let Some(leg) = self.legs.iter().find(|leg| leg.has_tool(name)) else {
            return ToolCallOutcome::Retry(tools_local::unknown_tool_message(name));
        };
        let mut session = leg.session.lock().await;
        match session.call(name, &Value::Object(args)).await {
            CallResult::Value(value) => ToolCallOutcome::Ok(mcp_value_content(&value)),
            CallResult::Retry { message } => ToolCallOutcome::Retry(message),
            // `ResilientToolset` absorbs an outage into the tool result
            // text (`mcp.py:168-170`): the call succeeded as far as the
            // model is concerned.
            CallResult::Absorbed { text } => ToolCallOutcome::Ok(Value::String(text)),
        }
    }
}

/// Transform one tool schema for the lane and infer `strict`
/// (`_customize_tool_def` through the model profile; the turn path never
/// sets `strict`).
fn wire_tool(name: &str, description: &str, parameters: &Value, anthropic: bool) -> WireTool {
    if anthropic {
        WireTool {
            name: name.to_owned(),
            description: description.to_owned(),
            parameters: provider::transform_anthropic_schema(parameters),
            strict: None,
        }
    } else {
        let (parameters, _compatible) = provider::transform_openai_schema(parameters);
        WireTool {
            name: name.to_owned(),
            description: description.to_owned(),
            parameters,
            strict: None,
        }
    }
}

/// `_map_mcp_tool_result`'s return value as the `ToolReturnPart.content`
/// the codec stores: the mapped object itself, dumped by the
/// `ModelMessagesTypeAdapter` rules.
fn mcp_value_content(value: &McpValue) -> Value {
    match value {
        McpValue::Single(content) => mcp_content_value(content),
        McpValue::List(items) => Value::Array(items.iter().map(mcp_content_value).collect()),
        McpValue::Structured(value) => value.clone(),
    }
}

fn mcp_content_value(content: &McpContent) -> Value {
    match content {
        McpContent::Text(text) => Value::String(text.clone()),
        McpContent::Json(value) => value.clone(),
        // `BinaryContent` (`pydantic_ai/messages.py:509-537`, pinned
        // 1.107.0): `ser_json_bytes="base64"` over `data`, then
        // `media_type`, `vendor_metadata` and the `kind` discriminator;
        // `identifier` is `exclude=True` and never reaches the dump.
        McpContent::Image { media_type, data } | McpContent::Binary { media_type, data } => {
            use base64::Engine as _;
            let mut out = Map::with_capacity(4);
            out.insert(
                "data".to_owned(),
                Value::String(base64::engine::general_purpose::STANDARD.encode(data)),
            );
            out.insert("media_type".to_owned(), Value::String(media_type.clone()));
            out.insert("vendor_metadata".to_owned(), Value::Null);
            out.insert("kind".to_owned(), Value::String("binary".to_owned()));
            Value::Object(out)
        }
    }
}

// --------------------------------------------------------------------------- //
// Public event/message steps (OVL-07)
// --------------------------------------------------------------------------- //
// The `events.py` steps as free functions over the pool (and the
// multiplexed redis client for the publish): the cloud overlay's turn
// seam copied all three, and calls these once OVL-12 re-points it.
// [`LiveTurnSeam`]'s private methods below delegate to them.

/// `append_event` (`events.py:90-104`): own transaction, thread row
/// locked, thread-scoped `seq`; the publish happens after commit and
/// swallows every failure (`events.py:53-70`).
#[allow(clippy::too_many_arguments)]
pub async fn append_event(
    pool: &PgPool,
    redis: Option<&RedisClient>,
    thread_id: Uuid,
    turn_id: Option<Uuid>,
    kind: &str,
    payload: &Value,
    message_id: Option<Uuid>,
    now: DateTime<Utc>,
) -> Result<(), SeamError> {
    let kind = events::truncate_kind(kind);
    let mut tx = pool.begin().await.map_err(db_error)?;
    let locked: Option<Uuid> =
        sqlx::query_scalar("SELECT id FROM assistant_thread WHERE id = $1 FOR UPDATE")
            .bind(thread_id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(db_error)?;
    if locked.is_none() {
        return Err(SeamError(
            "AssistantThread matching query does not exist.".to_owned(),
        ));
    }
    let max: Option<i64> = sqlx::query_scalar(&format!(
        "SELECT MAX(m) FROM ({}) AS seqs",
        events::next_event_seq_sql()
    ))
    .bind(thread_id)
    .fetch_one(&mut *tx)
    .await
    .map_err(db_error)?;
    let seq = max.unwrap_or(0) + 1;
    pidash_services::domain::assistant::append_event(
        &mut *tx,
        &pidash_services::domain::assistant::NewEvent {
            thread_id,
            turn_id,
            seq,
            kind,
            message_id,
            payload,
            created_at: now,
        },
    )
    .await
    .map_err(db_error)?;
    tx.commit().await.map_err(db_error)?;
    publish_event(pool, redis, thread_id, seq, kind, payload, message_id, now).await;
    Ok(())
}

/// `publish_event` (`events.py:53-70`): re-read the committed row id,
/// render the frame, publish, swallow every failure.
#[allow(clippy::too_many_arguments)]
pub async fn publish_event(
    pool: &PgPool,
    redis: Option<&RedisClient>,
    thread_id: Uuid,
    seq: i64,
    kind: &str,
    payload: &Value,
    message_id: Option<Uuid>,
    now: DateTime<Utc>,
) {
    let Some(redis) = redis else {
        return;
    };
    let event_id: Option<i64> = match sqlx::query_scalar(
        "SELECT id FROM assistant_event WHERE thread_id = $1 AND seq = $2",
    )
    .bind(thread_id)
    .bind(seq)
    .fetch_optional(pool)
    .await
    {
        Ok(id) => id,
        Err(error) => {
            tracing::warn!(%error, "assistant: event id re-read failed");
            return;
        }
    };
    let Some(event_id) = event_id else {
        return;
    };
    let thread = thread_id.to_string();
    let message = message_id.map(|id| id.to_string());
    let payload_json = pidash_jobs::tasks_webhooks::activity_dispatch::django_dumps(payload);
    let created_at = render_stamp(now);
    let frame = events::serialize_event_json(&events::EventParts {
        id: event_id,
        thread_id: &thread,
        message_id: message.as_deref(),
        seq,
        kind,
        payload_json: &payload_json,
        created_at: &created_at,
    });
    let (channel, body) = events::publish_frame(&thread, &frame);
    let publish = async {
        use redis::AsyncCommands;
        let mut connection = redis.get_multiplexed_async_connection().await?;
        connection.publish::<_, _, ()>(&channel, &body).await
    };
    if let Err(error) = publish.await {
        tracing::warn!(%error, "assistant: publish failed for thread {thread_id}");
    }
}

/// `create_message` (`events.py:108-125`): own transaction, thread row
/// locked, thread-scoped message `seq`.
#[allow(clippy::too_many_arguments)]
pub async fn create_message(
    pool: &PgPool,
    thread_id: Uuid,
    turn_id: Option<Uuid>,
    kind: &str,
    display_content: &str,
    payload: &Value,
    status: &str,
    now: DateTime<Utc>,
) -> Result<(Uuid, i64), SeamError> {
    let message_id = Uuid::new_v4();
    let mut tx = pool.begin().await.map_err(db_error)?;
    let locked: Option<Uuid> =
        sqlx::query_scalar("SELECT id FROM assistant_thread WHERE id = $1 FOR UPDATE")
            .bind(thread_id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(db_error)?;
    if locked.is_none() {
        return Err(SeamError(
            "AssistantThread matching query does not exist.".to_owned(),
        ));
    }
    let max: Option<i64> = sqlx::query_scalar(&format!(
        "SELECT MAX(m) FROM ({}) AS seqs",
        events::next_message_seq_sql()
    ))
    .bind(thread_id)
    .fetch_one(&mut *tx)
    .await
    .map_err(db_error)?;
    let seq = max.unwrap_or(0) + 1;
    pidash_services::domain::assistant::create_message_row(
        &mut *tx,
        &pidash_services::domain::assistant::NewMessage {
            id: message_id,
            thread_id,
            turn_id,
            seq,
            kind,
            display_content,
            payload,
            status,
            created_at: now,
            completed_at: None,
        },
    )
    .await
    .map_err(db_error)?;
    tx.commit().await.map_err(db_error)?;
    Ok((message_id, seq))
}

// --------------------------------------------------------------------------- //
// The seam
// --------------------------------------------------------------------------- //

/// Per-turn state the seam keeps between the `drive_turn` steps (Python
/// carries it on the resolved model and toolset objects).
struct PreparedTurn {
    lane: Lane,
    model_label: String,
    tools: Option<Arc<LiveToolset>>,
}

/// The live [`TurnSeam`]: sqlx over the worker pool, the multiplexed redis
/// client for the cancel key and the SSE publishes, the D-11 provider lane,
/// and the 1112/1113 toolset halves.
pub struct LiveTurnSeam {
    pool: PgPool,
    redis: Option<RedisClient>,
    settings: Arc<Settings>,
    http: reqwest::Client,
    extract: ProviderTable,
    tool_store: Arc<PgToolStore>,
    ssrf_resolver: Arc<dyn ssrf::HostResolver + Send + Sync>,
    key_cache: Mutex<KeyCache>,
    /// Per-turn state between the `drive_turn` steps, keyed by turn id. Shared
    /// with each [`LiveStream`] so the stream reclaims its entry on drop; the
    /// seam is process-lifetime (`worker()` builds it once), so an entry kept
    /// past its turn would retain the decrypted BYOK key forever.
    runs: Arc<Mutex<HashMap<Uuid, PreparedTurn>>>,
    started: Instant,
}

impl LiveTurnSeam {
    /// Build the seam. Construction performs no I/O: the golden table
    /// compiles, the HTTP client and tool store are built without
    /// connecting, and `redis` is the worker's multiplexed client (`None`
    /// when `REDIS_URL` is unset — the cancel read then answers `false`,
    /// exactly like Django's swallowed `redis_instance()`).
    pub fn new(
        pool: PgPool,
        settings: Arc<Settings>,
        redis: Option<RedisClient>,
    ) -> Result<Self, SeamError> {
        let golden = Golden::load().map_err(|error| SeamError(error.to_string()))?;
        let extract = ProviderTable::load(&golden).map_err(|error| SeamError(error.to_string()))?;
        let http = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(DEFAULT_TIMEOUT_SECS))
            .build()
            .map_err(|error| SeamError(error.to_string()))?;
        let tool_store = Arc::new(
            PgToolStore::new(
                pool.clone(),
                redis.clone(),
                settings.cloud_agent.clone(),
                settings.managed_runner.clone(),
            )
            .map_err(|error| SeamError(error.message()))?,
        );
        let key_cache = KeyCache::new(
            settings.assistant.key_cache_ttl_secs,
            settings.assistant.key_cache_maxsize.max(1) as usize,
        );
        Ok(Self {
            pool,
            redis,
            settings,
            http,
            extract,
            tool_store,
            ssrf_resolver: Arc::new(ssrf::SystemResolver),
            key_cache: Mutex::new(key_cache),
            runs: Arc::new(Mutex::new(HashMap::new())),
            started: Instant::now(),
        })
    }

    /// Drop a turn's record. Used where the drive exits before a [`LiveStream`]
    /// exists to own the reclaim (a history read failure), so no path leaves a
    /// record behind.
    fn reclaim_turn(&self, turn_id: Uuid) {
        reclaim_runs(&self.runs, turn_id);
    }

    /// The tool environment for one run (`ToolEnv`: settings + clock).
    fn run_env(&self, now: DateTime<Utc>) -> ToolEnv {
        ToolEnv {
            pr_lookups_cap: self.settings.loop_tuning.pr_lookups_per_run.max(0) as u32,
            gitlab_hosts: self.settings.gitlab_allowed_hosts.clone(),
            block_private_urls: self.settings.assistant.block_private_urls,
            ssrf_resolver: self.ssrf_resolver.clone(),
            now,
        }
    }

    /// `AssistantDeps` for the run (`deps.py`; `tasks.py:86-95`).
    fn deps_for(ctx: &TurnContext) -> AssistantDeps {
        AssistantDeps {
            user_id: ctx.user_id,
            user_display: ctx.user_display.clone(),
            workspace_id: ctx.workspace_id,
            workspace_slug: ctx.workspace_slug.clone(),
            workspace_name: ctx.workspace_name.clone(),
            workspace_role: ctx.workspace_role,
            thread_id: ctx.thread_id,
            turn_id: ctx.turn_id,
            mode: ctx.thread_kind.clone(),
            ..AssistantDeps::default()
        }
    }

    /// Decrypt one stored ciphertext (`crypto.decrypt`) as the turn's
    /// resolution failure shape.
    async fn decrypt(&self, token: &[u8]) -> Result<String, ModelError> {
        let config = crate::assistant::common::crypto_config_from_settings(&self.settings);
        let decrypted = crate::assistant::common::decrypt_api_key(&config, token)
            .await
            .map_err(crypto_model_error)?;
        if !llm_service::cache_enabled(self.settings.assistant.key_cache_ttl_secs) {
            return Ok(decrypted);
        }
        let mut cache = self.key_cache.lock().map_err(internal_model_error)?;
        Ok(cache.get_or_decrypt(token, || decrypted.clone()))
    }

    /// The public `append_event` step over this seam's handles.
    async fn append_event(
        &self,
        thread_id: Uuid,
        turn_id: Option<Uuid>,
        kind: &str,
        payload: &Value,
        message_id: Option<Uuid>,
        now: DateTime<Utc>,
    ) -> Result<(), SeamError> {
        append_event(
            &self.pool,
            self.redis.as_ref(),
            thread_id,
            turn_id,
            kind,
            payload,
            message_id,
            now,
        )
        .await
    }

    /// The public `create_message` step over this seam's pool.
    #[allow(clippy::too_many_arguments)]
    async fn create_message(
        &self,
        thread_id: Uuid,
        turn_id: Option<Uuid>,
        kind: &str,
        display_content: &str,
        payload: &Value,
        status: &str,
        now: DateTime<Utc>,
    ) -> Result<(Uuid, i64), SeamError> {
        create_message(
            &self.pool,
            thread_id,
            turn_id,
            kind,
            display_content,
            payload,
            status,
            now,
        )
        .await
    }

    /// `events.message_envelope` (`events.py:127-140`) as the JSON embedded
    /// in the `message_created`/`message_completed` payloads.
    ///
    /// Public for the cloud overlay's turn seam (OVL-07), which copied
    /// this envelope; the overlay's twin stays until OVL-12 re-points
    /// it here.
    #[allow(clippy::too_many_arguments)]
    pub fn envelope_json(
        message_id: Uuid,
        kind: &str,
        display_content: &str,
        status: &str,
        seq: i64,
        turn_id: Option<Uuid>,
        payload: &Value,
        created_at: DateTime<Utc>,
        completed_at: Option<DateTime<Utc>>,
    ) -> Result<Value, SeamError> {
        let id = message_id.to_string();
        let turn = turn_id.map(|id| id.to_string());
        let created = render_stamp(created_at);
        let completed = completed_at.map(render_stamp);
        let payload_json = serde_json::to_string(payload).map_err(|e| SeamError(e.to_string()))?;
        let rendered = events::message_envelope_json(&events::MessageParts {
            id: &id,
            kind,
            display_content,
            status,
            seq,
            turn_id: turn.as_deref(),
            payload_json: &payload_json,
            created_at: &created,
            completed_at: completed.as_deref(),
        });
        serde_json::from_str(&rendered).map_err(|e| SeamError(e.to_string()))
    }

    /// Append one `message_completed` event for an already-written row:
    /// the envelope plus the turn-scoped event (`tasks.py:150-153` in
    /// `_finalize_row`, `tasks.py:526-530` in `_mark_cut_off`).
    #[allow(clippy::too_many_arguments)]
    async fn emit_message_completed(
        &self,
        ctx: &TurnContext,
        message_id: Uuid,
        text: &str,
        status: &str,
        seq: i64,
        payload: &Value,
        created_at: DateTime<Utc>,
        completed_at: Option<DateTime<Utc>>,
        now: DateTime<Utc>,
    ) -> Result<(), SeamError> {
        let envelope = Self::envelope_json(
            message_id,
            rt::KIND_ASSISTANT,
            text,
            status,
            seq,
            Some(ctx.turn_id),
            payload,
            created_at,
            completed_at,
        )?;
        let event_payload = serde_json::json!({
            "turn_id": ctx.turn_id.to_string(),
            "message": envelope,
        });
        self.append_event(
            ctx.thread_id,
            Some(ctx.turn_id),
            rt::EVENT_MESSAGE_COMPLETED,
            &event_payload,
            Some(message_id),
            now,
        )
        .await
    }

    /// The `[], []` toolset (`tasks.py:370-389`): no MCP legs, the local
    /// schemas kept — what a turn runs with when the MCP lookup fails.
    fn empty_toolset(&self, ctx: &TurnContext, lane: &Lane) -> LiveToolset {
        LiveToolset {
            store: self.tool_store.clone(),
            deps: tokio::sync::Mutex::new(Self::deps_for(ctx)),
            env: self.run_env(Utc::now()),
            anthropic: matches!(lane, Lane::Anthropic { .. }),
            local: tool_schemas::all_tool_schemas(),
            legs: Vec::new(),
        }
    }
}

fn internal_model_error(error: impl std::fmt::Display) -> ModelError {
    ModelError {
        code: rt::CODE_INTERNAL.to_owned(),
        detail: error.to_string(),
    }
}

/// `crypto.decrypt` failures crossing into the turn's model-resolution
/// arm: an `AssistantError` carries its own code/detail (the
/// `try/except AssistantError` at `tasks.py:316-318`); an operational
/// failure has no `AssistantError` counterpart and takes the ported
/// `internal` bucket.
fn crypto_model_error(error: CryptoError) -> ModelError {
    match error {
        CryptoError::NotConfigured(assistant) => ModelError {
            code: assistant.code().to_owned(),
            detail: assistant.detail().to_owned(),
        },
        CryptoError::Transport(message) => ModelError {
            code: rt::CODE_INTERNAL.to_owned(),
            detail: message,
        },
    }
}

impl TurnSeam for LiveTurnSeam {
    type Stream = LiveStream;

    /// `_load_context` (`tasks.py:73-96`): the turn with its thread,
    /// workspace and user message; `None` unless QUEUED/RUNNING.
    async fn load_context(&self, turn_id: Uuid) -> Result<Option<TurnContext>, SeamError> {
        let row = sqlx::query(&rt::load_context_sql())
            .bind(turn_id)
            .fetch_optional(&self.pool)
            .await
            .map_err(db_error)?;
        let Some(row) = row else {
            return Ok(None);
        };
        let status: String = row.try_get(CTX_COL_TURN_STATUS).map_err(db_error)?;
        if !rt::context_eligible(&status) {
            return Ok(None);
        }
        let turn_id: Uuid = row.try_get(CTX_COL_TURN_ID).map_err(db_error)?;
        let thread_id: Uuid = row.try_get(CTX_COL_THREAD_ID).map_err(db_error)?;
        let workspace_id: Uuid = row.try_get(CTX_COL_THREAD_WORKSPACE_ID).map_err(db_error)?;
        let user_id: Uuid = row.try_get(CTX_COL_THREAD_USER_ID).map_err(db_error)?;
        let thread_kind: String = row.try_get(CTX_COL_THREAD_KIND).map_err(db_error)?;
        let workspace_slug: String = row.try_get(CTX_COL_WORKSPACE_SLUG).map_err(db_error)?;
        let workspace_name: String = row.try_get(CTX_COL_WORKSPACE_NAME).map_err(db_error)?;
        let user_message_id: Option<Uuid> = row
            .try_get(CTX_COL_TURN_USER_MESSAGE_ID)
            .map_err(db_error)?;
        // `turn.user_message.display_content if turn.user_message_id else ""`.
        let user_text: String = if user_message_id.is_some() {
            row.try_get::<Option<String>, _>(CTX_COL_USER_MESSAGE_CONTENT)
                .map_err(db_error)?
                .unwrap_or_default()
        } else {
            String::new()
        };
        let user_display: Option<String> = sqlx::query_scalar(USER_DISPLAY_SQL)
            .bind(user_id)
            .fetch_optional(&self.pool)
            .await
            .map_err(db_error)?;
        let role: Option<i16> = sqlx::query_scalar(WORKSPACE_ROLE_SQL)
            .bind(workspace_id)
            .bind(user_id)
            .fetch_optional(&self.pool)
            .await
            .map_err(db_error)?;
        Ok(Some(TurnContext {
            turn_id,
            turn_status: status,
            thread_id,
            thread_kind,
            workspace_id,
            workspace_slug,
            workspace_name,
            workspace_role: i32::from(role.unwrap_or(0)),
            user_id,
            user_display: user_display.unwrap_or_default(),
            user_text,
        }))
    }

    /// `_mark_running` (`tasks.py:99-107`): atomic claim plus the
    /// `turn_started` event.
    async fn mark_running(&self, ctx: &TurnContext) -> Result<bool, SeamError> {
        let updated = sqlx::query(&rt::mark_running_update())
            .bind(ctx.turn_id)
            .execute(&self.pool)
            .await
            .map_err(db_error)?;
        if updated.rows_affected() == 0 {
            return Ok(false);
        }
        let payload = serde_json::json!({ "turn_id": ctx.turn_id.to_string() });
        self.append_event(
            ctx.thread_id,
            Some(ctx.turn_id),
            rt::EVENT_TURN_STARTED,
            &payload,
            None,
            Utc::now(),
        )
        .await?;
        Ok(true)
    }

    /// `_is_cancelled` (`tasks.py:110-115`): redis `GET`, every failure
    /// swallowed as `false`.
    async fn is_cancelled(&self, turn_id: Uuid) -> Result<bool, SeamError> {
        let Some(redis) = &self.redis else {
            return Ok(false);
        };
        let key = rt::cancel_key(&turn_id);
        let read = async {
            use redis::AsyncCommands;
            let mut connection = redis.get_multiplexed_async_connection().await?;
            connection.get::<_, Option<String>>(&key).await
        };
        match read.await {
            Ok(value) => Ok(value.map(|value| !value.is_empty()).unwrap_or(false)),
            Err(error) => {
                tracing::warn!(%error, "assistant: cancel read failed");
                Ok(false)
            }
        }
    }

    /// `resolve_model_for_user` (`tasks.py:315`): the BYOK gates, the
    /// execution-time SSRF re-check, the cached decrypt and the lane
    /// selection; the resolved lane is kept for `open_stream`.
    async fn resolve_model(&self, ctx: &TurnContext) -> Result<String, ModelError> {
        let row: Option<(String, String, String, Option<Vec<u8>>)> = sqlx::query_as(LLM_CONFIG_SQL)
            .bind(ctx.user_id)
            .fetch_optional(&self.pool)
            .await
            .map_err(internal_model_error)?;
        let (provider_kind, base_url, model_name, token) = row.unwrap_or_default();
        let token = token.unwrap_or_default();
        let has_api_key = !token.is_empty();
        let base_url_blocked = !base_url.is_empty()
            && ssrf::is_blocked(
                &base_url,
                self.settings.assistant.block_private_urls,
                self.ssrf_resolver.as_ref(),
            );
        let model = llm_service::resolve_byok_model(
            has_api_key,
            &model_name,
            &provider_kind,
            &base_url,
            base_url_blocked,
        )
        .map_err(|error| ModelError {
            code: error.code().to_owned(),
            detail: error.detail().to_owned(),
        })?;
        let api_key = self.decrypt(&token).await?;
        let lane = provider::lane_for(&model, &api_key);
        let label = llm_service::model_label(&provider_kind, &model_name);
        let mut runs = self.runs.lock().map_err(internal_model_error)?;
        runs.insert(
            ctx.turn_id,
            PreparedTurn {
                lane,
                model_label: label.clone(),
                tools: None,
            },
        );
        Ok(label)
    }

    /// `_resolve_toolsets` (`tasks.py:370-389`): the enabled MCP rows
    /// through the ported planning and construction gates; the sessions are
    /// entered (and their tools listed) in `open_stream`, which is where
    /// the agent boundary first needs them.
    async fn resolve_toolsets(
        &self,
        ctx: &TurnContext,
    ) -> Result<(Vec<ToolsetHandle>, Vec<SkippedServer>), SeamError> {
        let rows: Vec<(Uuid, String, String, Option<Vec<u8>>)> =
            match sqlx::query_as(MCP_SERVERS_SQL)
                .bind(ctx.user_id)
                .fetch_all(&self.pool)
                .await
            {
                Ok(rows) => rows,
                Err(error) => {
                    // A resolver that blows up entirely costs the turn its
                    // tools, not its life (`tasks.py:370-389`): install the
                    // `[], []` toolset so `open_stream` runs with no MCP
                    // legs, and still return the error so the driver emits
                    // the pseudo-entry. A poisoned lock or a missing record
                    // cannot install — the turn then fails as before, with
                    // nothing left behind that the reclaim would not drop.
                    tracing::warn!(
                        %error,
                        "assistant: MCP lookup failed; running turn without tools"
                    );
                    if let Ok(mut runs) = self.runs.lock() {
                        if let Some(prepared) = runs.get_mut(&ctx.turn_id) {
                            let toolset = self.empty_toolset(ctx, &prepared.lane);
                            prepared.tools = Some(Arc::new(toolset));
                        }
                    }
                    return Err(db_error(error));
                }
            };
        let server_rows: Vec<mcp_service::ServerRow> = rows
            .iter()
            .map(|(id, name, url, token)| mcp_service::ServerRow {
                pk: id.to_string(),
                name: name.clone(),
                url: url.clone(),
                has_auth_header: token.as_deref().map(|t| !t.is_empty()).unwrap_or(false),
                url_blocked: ssrf::is_blocked(
                    url,
                    self.settings.assistant.block_private_urls,
                    self.ssrf_resolver.as_ref(),
                ),
            })
            .collect();
        let (plans, skipped) = mcp_service::plan_servers(&server_rows, mcp_max_servers());
        // The decrypts are async, so they run before the (synchronous)
        // `assemble_toolsets` closures consult them.
        let mut headers: HashMap<String, Result<Option<String>, String>> = HashMap::new();
        for plan in &plans {
            let token = rows
                .iter()
                .find(|(id, _, _, _)| id.to_string() == plan.pk)
                .and_then(|(_, _, _, token)| token.clone());
            let resolved = match (plan.needs_auth, token) {
                (false, _) => Ok(None),
                (true, Some(token)) => match self.decrypt(&token).await {
                    Ok(header) => Ok(Some(header)),
                    Err(error) => Err(error.code),
                },
                (true, None) => Ok(None),
            };
            headers.insert(plan.pk.clone(), resolved);
        }
        let (sessions, skipped_auth) = mcp_service::assemble_toolsets(
            &plans,
            |plan| {
                headers
                    .get(&plan.pk)
                    .cloned()
                    .unwrap_or(Ok(None))
                    .map(|header| header.filter(|value| !value.is_empty()))
            },
            |plan, header| {
                let (timeout, read_timeout) = mcp_timeouts();
                let spec = mcp_service::build_toolset_spec(
                    &plan.url,
                    header.as_deref(),
                    Some(plan.prefix.as_str()),
                    false,
                    timeout,
                    read_timeout,
                    &plan.name,
                );
                ResilientSession::new(spec).ok_or(())
            },
        );
        let mut skipped: Vec<SkippedServer> = skipped.iter().map(port_skipped).collect();
        skipped.extend(skipped_auth.iter().map(port_skipped));
        let mut runs = self
            .runs
            .lock()
            .map_err(|error| SeamError(error.to_string()))?;
        let Some(prepared) = runs.get_mut(&ctx.turn_id) else {
            return Ok((Vec::new(), skipped));
        };
        let mut toolset = self.empty_toolset(ctx, &prepared.lane);
        toolset.legs = sessions
            .into_iter()
            .map(|session| {
                // `server_name or url` (`mcp.py:209`); the ported
                // session carries the resolved name.
                let server_name = session.server_name().to_owned();
                McpLeg {
                    server_name,
                    session: tokio::sync::Mutex::new(session),
                    defs: Mutex::new(Vec::new()),
                }
            })
            .collect();
        prepared.tools = Some(Arc::new(toolset));
        // `_resolve_toolsets` returns the constructed toolsets themselves;
        // the handle names the turn and the server so
        // `runtime_failures` can read the session's recorded failure back.
        let handles = prepared
            .tools
            .as_ref()
            .map(|tools| {
                tools
                    .legs
                    .iter()
                    .map(|leg| format!("{}|{}", ctx.turn_id, leg.server_name))
                    .collect()
            })
            .unwrap_or_default();
        Ok((handles, skipped))
    }

    /// `_emit_skipped` (`tasks.py:392-403`): no-op for an empty list.
    async fn emit_skipped(
        &self,
        ctx: &TurnContext,
        servers: &[SkippedServer],
    ) -> Result<(), SeamError> {
        let Some(payload) = rt::skipped_payload(servers) else {
            return Ok(());
        };
        self.append_event(
            ctx.thread_id,
            Some(ctx.turn_id),
            rt::EVENT_TOOL_SERVERS_SKIPPED,
            &payload,
            None,
            Utc::now(),
        )
        .await
    }

    /// `_report_runtime_tool_failures` (`tasks.py:406-419`): every handle
    /// whose session recorded a failure, however the turn ends.
    ///
    /// The teardown (`__aexit__`) runs first: Python's agent run exits its
    /// toolset `async with` before this report, so a server that died during
    /// close is reported here too (the ported `ResilientSession::close`
    /// records `close_failed`).
    async fn runtime_failures(
        &self,
        toolsets: &[ToolsetHandle],
    ) -> Result<Vec<SkippedServer>, SeamError> {
        let mut failed = Vec::new();
        for handle in toolsets {
            let Some((turn_raw, server)) = handle.split_once('|') else {
                continue;
            };
            let Ok(turn_id) = Uuid::parse_str(turn_raw) else {
                continue;
            };
            let tools = {
                let runs = self
                    .runs
                    .lock()
                    .map_err(|error| SeamError(error.to_string()))?;
                let Some(prepared) = runs.get(&turn_id) else {
                    continue;
                };
                let Some(tools) = prepared.tools.as_ref() else {
                    continue;
                };
                tools.clone()
            };
            let Some(leg) = tools.legs.iter().find(|leg| leg.server_name == server) else {
                continue;
            };
            let mut session = leg.session.lock().await;
            session.close().await;
            if let Some(class) = session.failure() {
                failed.push(SkippedServer {
                    name: server.to_owned(),
                    reason: class.to_owned(),
                });
            }
        }
        Ok(failed)
    }

    /// `history.load_history` (`tasks.py:327`): the newest completed turns'
    /// `model_messages`, oldest first.
    async fn load_history(&self, ctx: &TurnContext) -> Result<Value, SeamError> {
        let kind = if ctx.thread_kind == "loop" {
            ThreadKind::Loop
        } else {
            ThreadKind::Chat
        };
        let limit = events::history_limit(
            kind,
            self.settings.assistant.history_max_turns,
            self.settings.assistant.loop_history_max_turns,
        );
        let blobs: Vec<Value> = match sqlx::query_scalar(&events::load_history_sql())
            .bind(ctx.thread_id)
            .bind(limit)
            .fetch_all(&self.pool)
            .await
        {
            Ok(blobs) => blobs,
            Err(error) => {
                // `drive_turn` returns on this error before `open_stream`, so
                // no stream exists to reclaim the record.
                self.reclaim_turn(ctx.turn_id);
                return Err(db_error(error));
            }
        };
        let mut blobs = blobs;
        blobs.reverse();
        Ok(Value::Array(history::join_valid_blobs(&blobs)))
    }

    /// `_model_label` (`tasks.py:329`): `<provider_kind>:<model_name>` of
    /// the resolved config; empty when the config has no row
    /// (`model_label(cfg) if cfg else ""`).
    async fn model_label(&self, ctx: &TurnContext) -> Result<String, SeamError> {
        let runs = self
            .runs
            .lock()
            .map_err(|error| SeamError(error.to_string()))?;
        match runs.get(&ctx.turn_id) {
            Some(prepared) => Ok(prepared.model_label.clone()),
            None => Ok(String::new()),
        }
    }

    /// `assistant.run(...)` (`tasks.py:337-345`): parse the replayed
    /// history, enter each MCP toolset and list its tools, then run the
    /// agent loop on a spawned task feeding the driver's stream.
    async fn open_stream(
        &self,
        ctx: &TurnContext,
        history: &Value,
        toolsets: &[ToolsetHandle],
    ) -> Result<Self::Stream, AgentError> {
        // The record's reclaim on every exit without a stream — including
        // cancellation: the drive drops this future at the MCP-entry awaits
        // when the soft limit fires. Defused once the stream below takes
        // ownership; reclaiming an absent record is a no-op.
        let reclaim = OpenStreamGuard {
            runs: self.runs.clone(),
            turn_id: ctx.turn_id,
            defused: false,
        };
        let (lane, tools) = {
            let guard = self.runs.lock().map_err(internal_agent_error)?;
            let resolved = guard
                .get(&ctx.turn_id)
                .map(|prepared| (prepared.lane.clone(), prepared.tools.clone()));
            drop(guard);
            match resolved {
                Some((lane, Some(tools))) => (lane, tools),
                // Unreachable in practice (`resolve_model` inserts the record
                // and `resolve_toolsets` fills `tools`); the guard reclaims
                // the partial record on return.
                Some((_, None)) => {
                    return Err(AgentError::Typed {
                        code: rt::CODE_INTERNAL.to_owned(),
                        detail: "assistant turn: toolsets were not resolved".to_owned(),
                    });
                }
                None => {
                    return Err(AgentError::Typed {
                        code: rt::CODE_INTERNAL.to_owned(),
                        detail: "assistant turn: model was not resolved".to_owned(),
                    });
                }
            }
        };
        let _ = toolsets;
        // Toolset entry (`MCPToolset.__aenter__` then `get_tools`):
        // connect, then list, before the first request body needs the
        // schemas.
        for leg in &tools.legs {
            let mut session = leg.session.lock().await;
            session.enter().await;
            let defs = session.tools().await;
            drop(session);
            if let Ok(mut slot) = leg.defs.lock() {
                *slot = defs;
            }
        }
        let messages: Vec<TurnMessage> = if history.is_null() {
            Vec::new()
        } else {
            match history::parse_blob(history) {
                Ok(messages) => messages,
                Err(error) => {
                    return Err(AgentError::Typed {
                        code: rt::CODE_INTERNAL.to_owned(),
                        detail: error.to_string(),
                    });
                }
            }
        };
        let today = Utc::now().format("%Y-%m-%d").to_string();
        let instructions = instructions::instructions(&InstructionsContext {
            workspace_name: &ctx.workspace_name,
            workspace_slug: &ctx.workspace_slug,
            user_display: &ctx.user_display,
            workspace_role: ctx.workspace_role,
            mode: &ctx.thread_kind,
            today: &today,
            loop_max_writes: self.settings.loop_tuning.max_writes,
        });
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let outcome = Arc::new(Mutex::new(None));
        let outcome_slot = outcome.clone();
        let prompt = ctx.user_text.clone();
        let client = self.http.clone();
        let extract = self.extract.clone();
        let limits = super::agent::turn_usage_limits();
        let handle = tokio::spawn(async move {
            let config = TurnAgentConfig {
                lane: &lane,
                client: &client,
                extract: &extract,
                instructions: &instructions,
                history: &messages,
                prompt: &prompt,
                limits: &limits,
                timeout_secs: DEFAULT_TIMEOUT_SECS,
                endpoint_url: None,
            };
            let sink_tx = tx.clone();
            let mut sink = move |item: StreamItem| {
                // The terminal `End` is replaced by the `StreamMessage::End`
                // record below so the outcome slot is always populated
                // before the driver can read it.
                if item == StreamItem::End {
                    return;
                }
                let _ = sink_tx.send(StreamMessage::Item(item));
            };
            let terminal = match run_agent(&config, tools.as_ref(), &mut sink).await {
                Ok(value) => {
                    if let Ok(mut slot) = outcome_slot.lock() {
                        *slot = Some(value);
                    }
                    Ok(())
                }
                Err(error) => Err(AgentError::from(error)),
            };
            let _ = tx.send(StreamMessage::End(terminal));
        });
        // The stream owns the reclaim from here (its `Drop`).
        reclaim.defuse();
        Ok(LiveStream {
            rx,
            outcome,
            handle,
            runs: self.runs.clone(),
            turn_id: ctx.turn_id,
        })
    }

    /// Next stream item; the terminal record ends the stream (`End`) or
    /// raises the classified failure (`tasks.py:346-361`).
    async fn next_event(
        &self,
        stream: &mut Self::Stream,
    ) -> Result<Option<StreamItem>, AgentError> {
        match stream.rx.recv().await {
            Some(StreamMessage::Item(item)) => Ok(Some(item)),
            Some(StreamMessage::End(result)) => match result {
                Ok(()) => Ok(Some(StreamItem::End)),
                Err(error) => Err(error),
            },
            // The run task died without a terminal record (a panic inside
            // the loop): fail the turn rather than reporting a completion
            // that never happened.
            None => Err(AgentError::Typed {
                code: rt::CODE_INTERNAL.to_owned(),
                detail: "assistant turn: agent stream ended unexpectedly".to_owned(),
            }),
        }
    }

    /// Monotonic milliseconds for the flush rule
    /// (`asyncio.get_running_loop().time()`).
    fn now_ms(&self) -> u64 {
        self.started.elapsed().as_millis() as u64
    }

    /// `_start_assistant_row` (`tasks.py:118-129`).
    async fn start_row(&self, ctx: &TurnContext) -> Result<Uuid, SeamError> {
        let now = Utc::now();
        let payload = empty_payload();
        let (message_id, seq) = self
            .create_message(
                ctx.thread_id,
                Some(ctx.turn_id),
                rt::KIND_ASSISTANT,
                "",
                &payload,
                rt::MSG_STREAMING,
                now,
            )
            .await?;
        let envelope = Self::envelope_json(
            message_id,
            rt::KIND_ASSISTANT,
            "",
            rt::MSG_STREAMING,
            seq,
            Some(ctx.turn_id),
            &payload,
            now,
            None,
        )?;
        let event_payload = serde_json::json!({
            "turn_id": ctx.turn_id.to_string(),
            "message": envelope,
        });
        self.append_event(
            ctx.thread_id,
            Some(ctx.turn_id),
            rt::EVENT_MESSAGE_CREATED,
            &event_payload,
            Some(message_id),
            now,
        )
        .await?;
        Ok(message_id)
    }

    /// `_emit_delta` (`tasks.py:132-139`).
    async fn emit_delta(
        &self,
        ctx: &TurnContext,
        message_id: Uuid,
        chunk: &str,
    ) -> Result<(), SeamError> {
        let payload = serde_json::json!({
            "params": { "delta": chunk },
            "turn_id": ctx.turn_id.to_string(),
        });
        self.append_event(
            ctx.thread_id,
            Some(ctx.turn_id),
            rt::EVENT_ASSISTANT_DELTA,
            &payload,
            Some(message_id),
            Utc::now(),
        )
        .await
    }

    /// `_finalize_row` (`tasks.py:142-154`): the content/status write, then
    /// the `message_completed` envelope on the completed arm only.
    async fn finalize_row(
        &self,
        ctx: &TurnContext,
        message_id: Uuid,
        text: &str,
        status: &'static str,
    ) -> Result<(), SeamError> {
        let now = Utc::now();
        pidash_services::domain::assistant::finalize_message(
            &self.pool, message_id, text, status, now,
        )
        .await
        .map_err(db_error)?;
        if status != rt::MSG_COMPLETED {
            return Ok(());
        }
        let row: Option<(i64, Value, DateTime<Utc>)> = sqlx::query_as(MESSAGE_ROW_SQL)
            .bind(message_id)
            .fetch_optional(&self.pool)
            .await
            .map_err(db_error)?;
        let Some((seq, payload, created_at)) = row else {
            return Ok(());
        };
        self.emit_message_completed(
            ctx,
            message_id,
            text,
            status,
            seq,
            &payload,
            created_at,
            Some(now),
            now,
        )
        .await
    }

    /// `history.dump_new_messages` (`tasks.py:365`).
    async fn dump_messages(&self, stream: &Self::Stream) -> Result<Value, SeamError> {
        let outcome = stream
            .outcome
            .lock()
            .map_err(|error| SeamError(error.to_string()))?;
        match outcome.as_ref() {
            Some(value) => Ok(history::dump_blob(&value.messages)),
            None => Ok(Value::Array(Vec::new())),
        }
    }

    /// `_extract_usage` (`tasks.py:428-439`); a read failure yields `None`
    /// (the `{}` store) exactly like the swallowed `result.usage()`.
    async fn extract_usage(&self, stream: &Self::Stream) -> Result<Option<TurnUsage>, SeamError> {
        let outcome = stream
            .outcome
            .lock()
            .map_err(|error| SeamError(error.to_string()))?;
        Ok(outcome
            .as_ref()
            .map(|value| turn_usage_from_run(&value.usage)))
    }

    /// `_was_cut_off` (`tasks.py:502-507`): the final reply ended
    /// because the output limit was reached. A missing outcome reads as
    /// "not truncated", like the swallowed lookup in Python.
    async fn was_cut_off(&self, stream: &Self::Stream) -> Result<bool, SeamError> {
        let outcome = stream
            .outcome
            .lock()
            .map_err(|error| SeamError(error.to_string()))?;
        Ok(outcome
            .as_ref()
            .and_then(|value| value.finish)
            .is_some_and(|finish| finish == history::TurnFinish::Length))
    }

    /// `_mark_cut_off` (`tasks.py:510-530`): flag the turn's last reply
    /// as cut off — `payload.truncated` on the row, then the envelope
    /// re-sent as a second `message_completed` for clients to merge.
    async fn mark_cut_off(&self, ctx: &TurnContext) -> Result<(), SeamError> {
        let row: Option<LastAssistantRow> = sqlx::query_as(LAST_ASSISTANT_ROW_SQL)
            .bind(ctx.turn_id)
            .fetch_optional(&self.pool)
            .await
            .map_err(db_error)?;
        let Some((message_id, seq, payload, text, status, created_at, completed_at)) = row else {
            return Ok(());
        };
        let mut flagged = payload.as_object().cloned().unwrap_or_default();
        flagged.insert("truncated".to_owned(), Value::Bool(true));
        let merged = Value::Object(flagged);
        sqlx::query(MARK_CUT_OFF_SQL)
            .bind(message_id)
            .bind(&merged)
            .execute(&self.pool)
            .await
            .map_err(db_error)?;
        self.emit_message_completed(
            ctx,
            message_id,
            &text,
            &status,
            seq,
            &merged,
            created_at,
            completed_at,
            Utc::now(),
        )
        .await
    }

    /// `_complete_turn` (`tasks.py:158-172`), including the delta prune.
    async fn complete_turn(
        &self,
        ctx: &TurnContext,
        model_messages: Value,
        usage: Value,
        model_used: String,
        truncated: bool,
    ) -> Result<(), SeamError> {
        let mut tx = self.pool.begin().await.map_err(db_error)?;
        sqlx::query(&rt::lock_turn_select())
            .bind(ctx.turn_id)
            .execute(&mut *tx)
            .await
            .map_err(db_error)?;
        let won = pidash_services::domain::assistant::complete_turn(
            &mut *tx,
            ctx.turn_id,
            &model_messages,
            &usage,
            &model_used,
        )
        .await
        .map_err(db_error)?;
        if won == 0 {
            // Another terminal writer (sweep, timeout) already owned the
            // row: roll back and emit nothing — Python never reaches here
            // (its hard kill precedes the sweep), so the event stream keeps
            // exactly one terminal event either way.
            return Ok(());
        }
        sqlx::query(&rt::clear_active_turn_update())
            .bind(ctx.thread_id)
            .bind(ctx.turn_id)
            .execute(&mut *tx)
            .await
            .map_err(db_error)?;
        tx.commit().await.map_err(db_error)?;
        let mut payload = serde_json::json!({
            "turn_id": ctx.turn_id.to_string(),
            "usage": usage,
        });
        if truncated {
            payload["truncated"] = Value::Bool(true);
        }
        self.append_event(
            ctx.thread_id,
            Some(ctx.turn_id),
            rt::EVENT_TURN_COMPLETED,
            &payload,
            None,
            Utc::now(),
        )
        .await?;
        pidash_services::domain::assistant::prune_turn_deltas(&self.pool, ctx.turn_id)
            .await
            .map_err(db_error)?;
        Ok(())
    }

    /// `_fail_turn` (`tasks.py:181-201`): the atomic terminal write (turn
    /// status/code/detail, close the streaming rows, clear the in-flight
    /// pointer), then — in their own transactions exactly like Python — the
    /// `error` message row (`events.create_message`), the `turn_failed`
    /// event with its envelope and Redis publish (`events.append_event`) and
    /// the delta prune.
    async fn fail_turn(
        &self,
        ctx: &TurnContext,
        code: &str,
        detail: &str,
    ) -> Result<(), SeamError> {
        let code = rt::truncate_code(code).to_owned();
        let detail = rt::truncate_detail(detail).to_owned();
        let now = Utc::now();
        let mut tx = self.pool.begin().await.map_err(db_error)?;
        sqlx::query(&rt::lock_turn_select())
            .bind(ctx.turn_id)
            .execute(&mut *tx)
            .await
            .map_err(db_error)?;
        let won =
            pidash_services::domain::assistant::fail_turn(&mut *tx, ctx.turn_id, &code, &detail)
                .await
                .map_err(db_error)?;
        if won == 0 {
            // Another terminal writer already owned the row (see
            // `complete_turn`): roll back and emit nothing, so a lost race
            // never writes a second terminal event.
            return Ok(());
        }
        pidash_services::domain::assistant::finalize_open_messages(
            &mut *tx,
            ctx.turn_id,
            rt::MSG_FAILED,
        )
        .await
        .map_err(db_error)?;
        sqlx::query(&rt::clear_active_turn_update())
            .bind(ctx.thread_id)
            .bind(ctx.turn_id)
            .execute(&mut *tx)
            .await
            .map_err(db_error)?;
        tx.commit().await.map_err(db_error)?;
        // `events.create_message(..., display_content=detail or code,
        // status=FAILED)` (`tasks.py:191-193`): own transaction, thread
        // locked, thread-scoped seq.
        let content = if detail.is_empty() {
            code.clone()
        } else {
            detail.clone()
        };
        let payload = empty_payload();
        let (message_id, _seq) = self
            .create_message(
                ctx.thread_id,
                Some(ctx.turn_id),
                rt::KIND_ERROR,
                &content,
                &payload,
                rt::MSG_FAILED,
                now,
            )
            .await?;
        // `events.append_event(..., "turn_failed", ...)` (`tasks.py:194-200`).
        let event_payload = serde_json::json!({
            "turn_id": ctx.turn_id.to_string(),
            "error_code": code,
            "detail": detail,
        });
        self.append_event(
            ctx.thread_id,
            Some(ctx.turn_id),
            rt::EVENT_TURN_FAILED,
            &event_payload,
            Some(message_id),
            now,
        )
        .await?;
        pidash_services::domain::assistant::prune_turn_deltas(&self.pool, ctx.turn_id)
            .await
            .map_err(db_error)?;
        Ok(())
    }

    /// `_cancel_turn` (`tasks.py:204-213`).
    async fn cancel_turn(&self, ctx: &TurnContext) -> Result<(), SeamError> {
        let mut tx = self.pool.begin().await.map_err(db_error)?;
        sqlx::query(&rt::lock_turn_select())
            .bind(ctx.turn_id)
            .execute(&mut *tx)
            .await
            .map_err(db_error)?;
        let won = sqlx::query(&rt::cancel_turn_update())
            .bind(ctx.turn_id)
            .execute(&mut *tx)
            .await
            .map_err(db_error)?
            .rows_affected();
        if won == 0 {
            // Another terminal writer already owned the row (see
            // `complete_turn`): roll back and emit nothing, so a lost race
            // never writes a second terminal event.
            return Ok(());
        }
        pidash_services::domain::assistant::finalize_open_messages(
            &mut *tx,
            ctx.turn_id,
            rt::MSG_CANCELLED,
        )
        .await
        .map_err(db_error)?;
        sqlx::query(&rt::clear_active_turn_update())
            .bind(ctx.thread_id)
            .bind(ctx.turn_id)
            .execute(&mut *tx)
            .await
            .map_err(db_error)?;
        tx.commit().await.map_err(db_error)?;
        let payload = serde_json::json!({ "turn_id": ctx.turn_id.to_string() });
        self.append_event(
            ctx.thread_id,
            Some(ctx.turn_id),
            rt::EVENT_TURN_CANCELLED,
            &payload,
            None,
            Utc::now(),
        )
        .await?;
        pidash_services::domain::assistant::prune_turn_deltas(&self.pool, ctx.turn_id)
            .await
            .map_err(db_error)?;
        Ok(())
    }

    /// `turn_error_for_user` (`ee/assistant/model_provider.py:72-87`), CE:
    /// every connection is BYOK. The model names the failure's own when
    /// it carries one, else the prepared lane's — the lane was built
    /// from the user's configured model, exactly what the hook's
    /// `cfg.model_name` fallback reads.
    async fn turn_error_for_user(
        &self,
        ctx: &TurnContext,
        message: &str,
        status_code: Option<u16>,
        body_detail: &str,
        model_name: &str,
    ) -> Result<Option<(String, String)>, SeamError> {
        use pidash_jobs::assistant::{classify_provider_error, BYOK};
        let model = if model_name.is_empty() {
            let runs = self
                .runs
                .lock()
                .map_err(|error| SeamError(error.to_string()))?;
            runs.get(&ctx.turn_id)
                .map(|prepared| prepared.lane.model_name().to_owned())
                .unwrap_or_default()
        } else {
            model_name.to_owned()
        };
        Ok(classify_provider_error(
            status_code,
            message,
            body_detail,
            BYOK,
            &model,
        ))
    }
}

fn internal_agent_error(error: impl std::fmt::Display) -> AgentError {
    AgentError::Typed {
        code: rt::CODE_INTERNAL.to_owned(),
        detail: error.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn fixture(name: &str) -> Value {
        let path = format!(
            "{}/../../fixtures/assistant/turn-runtime/{name}.json",
            env!("CARGO_MANIFEST_DIR")
        );
        serde_json::from_str(&std::fs::read_to_string(&path).expect("fixture exists"))
            .expect("fixture parses")
    }

    /// The capture's first request body: the wire tools the run exposes.
    fn capture_wire_tools(name: &str) -> Vec<(String, Value)> {
        let fixture = fixture(name);
        let anthropic = fixture["_meta"]["lane"] == json!("anthropic");
        let body = &fixture["provider_exchanges"][0]["request"]["body"];
        body["tools"]
            .as_array()
            .expect("tools")
            .iter()
            .map(|tool| {
                if anthropic {
                    (
                        tool["name"].as_str().expect("name").to_owned(),
                        tool["input_schema"].clone(),
                    )
                } else {
                    (
                        tool["function"]["name"].as_str().expect("name").to_owned(),
                        tool["function"]["parameters"].clone(),
                    )
                }
            })
            .collect()
    }

    /// The local tools must be exposed in `@assistant.tool` registration
    /// order, then the MCP toolsets — exactly the order the captures pin.
    #[test]
    fn local_tool_order_matches_capture() {
        let expected: Vec<String> = capture_wire_tools("tool_mcp")
            .into_iter()
            .map(|(name, _)| name)
            .collect();
        let local: Vec<String> = tool_schemas::all_tool_schemas()
            .iter()
            .map(|schema| schema.name.to_owned())
            .collect();
        assert_eq!(local.len(), 17, "the 17 local tools");
        assert_eq!(
            &expected[..local.len()],
            local.as_slice(),
            "local tools first, in schema order"
        );
        assert_eq!(expected[local.len()], "mcp_demo_greet");
    }

    /// The wire transform is the lane's profile transform, pinned byte for
    /// byte against both captures for the same tool: `tool_local`'s
    /// `function.parameters` (OpenAI) and `tool_anthropic`'s `input_schema`
    /// (Anthropic).
    #[test]
    fn wire_tool_uses_the_lane_transform() {
        let schema = &tool_schemas::tool_schema("create_comment").expect("local tool");
        let openai = wire_tool(schema.name, schema.description, &schema.parameters, false);
        let anthropic = wire_tool(schema.name, schema.description, &schema.parameters, true);

        let (name, parameters) = capture_wire_tools("tool_local")
            .into_iter()
            .find(|(name, _)| name == "create_comment")
            .expect("captured OpenAI local tool");
        assert_eq!(name, openai.name);
        assert_eq!(parameters, openai.parameters, "OpenAI lane bytes");

        let (name, parameters) = capture_wire_tools("tool_anthropic")
            .into_iter()
            .find(|(name, _)| name == "create_comment")
            .expect("captured Anthropic local tool");
        assert_eq!(name, anthropic.name);
        assert_eq!(parameters, anthropic.parameters, "Anthropic lane bytes");
    }

    /// `_map_mcp_tool_result`: a text block is a string, a JSON text block
    /// the parsed value, structured content passed through, and a list in
    /// order.
    #[test]
    fn mcp_values_map_to_tool_return_content() {
        assert_eq!(
            mcp_value_content(&McpValue::Single(McpContent::Text("hi".to_owned()))),
            json!("hi")
        );
        assert_eq!(
            mcp_value_content(&McpValue::Single(McpContent::Json(json!({"a": 1})))),
            json!({"a": 1})
        );
        assert_eq!(
            mcp_value_content(&McpValue::Structured(json!([1, 2]))),
            json!([1, 2])
        );
        assert_eq!(
            mcp_value_content(&McpValue::List(vec![
                McpContent::Text("a".to_owned()),
                McpContent::Json(json!(7)),
            ])),
            json!(["a", 7])
        );
        // `BinaryContent` (`messages.py:509-537`): base64 `data`,
        // `media_type`, null `vendor_metadata`, `kind: "binary"`.
        assert_eq!(
            mcp_value_content(&McpValue::Single(McpContent::Image {
                media_type: "image/png".to_owned(),
                data: vec![0x89, b'P', b'N', b'G'],
            })),
            json!({
                "data": "iVBORw==",
                "media_type": "image/png",
                "vendor_metadata": null,
                "kind": "binary",
            })
        );
    }

    /// `_extract_usage`: the five counters, in the stored dict shape.
    #[test]
    fn usage_maps_to_the_stored_dict() {
        let usage = RunUsage {
            input_tokens: UsageNumber::Int(11),
            output_tokens: UsageNumber::Int(22),
            requests: 2,
            tool_calls: 3,
            ..RunUsage::default()
        };
        let mapped = turn_usage_from_run(&usage);
        assert_eq!(
            serde_json::to_value(&mapped).expect("serializes"),
            json!({
                "input_tokens": 11,
                "output_tokens": 22,
                "total_tokens": 33,
                "requests": 2,
                "tool_calls": 3,
            })
        );
    }

    /// The events the seam writes must be the ported shapes: the message
    /// envelopes render through `events.message_envelope_json`, so an
    /// unset `completed_at` stays `null` and the wire `role` maps from the
    /// message kind.
    #[test]
    fn message_envelope_matches_events_py() {
        let now = DateTime::parse_from_rfc3339("2026-10-17T07:29:03+00:00")
            .expect("stamp")
            .with_timezone(&Utc);
        let message_id = Uuid::parse_str("11111111-2222-3333-4444-555555555555").expect("uuid");
        let turn_id = Uuid::parse_str("66666666-7777-8888-9999-aaaaaaaaaaaa").expect("uuid");
        let envelope = LiveTurnSeam::envelope_json(
            message_id,
            rt::KIND_ASSISTANT,
            "",
            rt::MSG_STREAMING,
            4,
            Some(turn_id),
            &empty_payload(),
            now,
            None,
        )
        .expect("envelope renders");
        assert_eq!(
            envelope,
            json!({
                "id": "11111111-2222-3333-4444-555555555555",
                "role": "assistant",
                "content": "",
                "status": "streaming",
                "seq": 4,
                "turn_id": "66666666-7777-8888-9999-aaaaaaaaaaaa",
                "payload": {},
                "created_at": "2026-10-17T07:29:03+00:00",
                "completed_at": null,
            })
        );
    }

    /// The cancel key is the ported one (`tasks.cancel_key`), so the SSE
    /// cancel POST and the turn read agree.
    #[test]
    fn cancel_key_is_the_ported_shape() {
        let id = Uuid::parse_str("12345678-1234-5678-1234-567812345678").expect("uuid");
        assert_eq!(
            rt::cancel_key(&id),
            "assistant:cancel:12345678-1234-5678-1234-567812345678"
        );
    }

    /// Dropping the stream reclaims its per-turn record. The seam outlives
    /// every turn (`worker()` builds it once), so a record kept past its turn
    /// would retain the decrypted BYOK key and grow the map without bound
    /// (PR #1682 review).
    #[tokio::test]
    async fn prepared_turn_is_reclaimed_when_the_stream_drops() {
        let turn_id = Uuid::parse_str("66666666-7777-8888-9999-aaaaaaaaaaaa").expect("uuid");
        let runs: Arc<Mutex<HashMap<Uuid, PreparedTurn>>> = Arc::new(Mutex::new(HashMap::new()));
        runs.lock().expect("lock").insert(
            turn_id,
            PreparedTurn {
                lane: Lane::Anthropic {
                    model: "claude-3-5-sonnet".to_owned(),
                    api_key: "sk-secret".to_owned(),
                },
                model_label: "anthropic:claude-3-5-sonnet".to_owned(),
                tools: None,
            },
        );
        let (_tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let handle = tokio::spawn(async {});
        let stream = LiveStream {
            rx,
            outcome: Arc::new(Mutex::new(None)),
            handle,
            runs: runs.clone(),
            turn_id,
        };
        assert!(runs.lock().expect("lock").contains_key(&turn_id));
        drop(stream);
        assert!(
            !runs.lock().expect("lock").contains_key(&turn_id),
            "the per-turn record is gone once the stream drops"
        );
    }

    /// Soft-limit expiry during MCP entry reclaims the per-turn record
    /// (PIDASHCONV-1572). `drive_turn` drops the `open_stream` future at the
    /// `session.enter()`/`tools()` awaits, where none of the explicit reclaims
    /// run and no `LiveStream` exists yet — without the guard the record (the
    /// decrypted BYOK key and the MCP sessions) stays in the seam's
    /// process-wide map for the life of the worker.
    #[tokio::test]
    async fn open_stream_timeout_during_mcp_entry_reclaims_the_record() {
        use std::sync::atomic::{AtomicUsize, Ordering};

        // The hanging MCP server: accepts the TCP connection, then never
        // answers the initialize request.
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("stub binds");
        let port = listener.local_addr().expect("stub port").port();
        let hits = Arc::new(AtomicUsize::new(0));
        let accept_hits = hits.clone();
        let acceptor = tokio::spawn(async move {
            loop {
                let Ok((socket, _)) = listener.accept().await else {
                    return;
                };
                accept_hits.fetch_add(1, Ordering::SeqCst);
                tokio::spawn(async move {
                    tokio::time::sleep(std::time::Duration::from_secs(3600)).await;
                    drop(socket);
                });
            }
        });

        let pool = PgPool::connect_lazy("postgres://127.0.0.1:1/unused").expect("lazy pool builds");
        let seam = LiveTurnSeam::new(pool, Arc::new(Settings::test_defaults()), None)
            .expect("seam builds");
        let turn_id = Uuid::new_v4();
        let spec = mcp_service::build_toolset_spec(
            &format!("http://127.0.0.1:{port}/mcp"),
            None,
            Some("hang"),
            false,
            3600.0,
            3600.0,
            "hang",
        );
        let session = ResilientSession::new(spec).expect("session builds");
        let tools = LiveToolset {
            store: seam.tool_store.clone(),
            deps: tokio::sync::Mutex::new(AssistantDeps::default()),
            env: seam.run_env(Utc::now()),
            anthropic: false,
            local: Vec::new(),
            legs: vec![McpLeg {
                server_name: "hang".to_owned(),
                session: tokio::sync::Mutex::new(session),
                defs: Mutex::new(Vec::new()),
            }],
        };
        seam.runs.lock().expect("lock").insert(
            turn_id,
            PreparedTurn {
                lane: Lane::Anthropic {
                    model: "claude-3-5-sonnet".to_owned(),
                    api_key: "sk-secret".to_owned(),
                },
                model_label: "anthropic:claude-3-5-sonnet".to_owned(),
                tools: Some(Arc::new(tools)),
            },
        );
        let ctx = TurnContext {
            turn_id,
            turn_status: "running".to_owned(),
            thread_id: Uuid::new_v4(),
            thread_kind: "chat".to_owned(),
            workspace_id: Uuid::new_v4(),
            workspace_slug: "ws".to_owned(),
            workspace_name: "ws".to_owned(),
            workspace_role: 15,
            user_id: Uuid::new_v4(),
            user_display: "user".to_owned(),
            user_text: "do a thing".to_owned(),
        };
        // The drive's soft-limit deadline, far inside the stub's (absent)
        // answer and the session's hour-long timeouts: the timeout must win.
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_millis(500);
        let outcome =
            tokio::time::timeout_at(deadline, seam.open_stream(&ctx, &Value::Null, &[])).await;
        assert!(
            outcome.is_err(),
            "the deadline must fire while MCP entry hangs"
        );
        assert!(
            hits.load(Ordering::SeqCst) >= 1,
            "the turn must have reached MCP entry"
        );
        assert!(
            !seam.runs.lock().expect("lock").contains_key(&turn_id),
            "the per-turn record is gone after the deadline fires during MCP entry"
        );
        acceptor.abort();
    }
}
