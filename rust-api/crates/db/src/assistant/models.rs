//! Assistant table models (D-06, stage 5).
//!
//! Ports `apps/api/pi_dash/assistant/models.py:1-293` (`ThreadKind`,
//! `AssistantThread`, `TurnStatus`, `AssistantTurn`, `MessageKind`,
//! `MessageStatus`, `AssistantMessage`, `AssistantEvent`, `ProviderKind`,
//! `AssistantMCPServer`, `UserLLMConfig`, `UserSTTConfig`), adopting the
//! Django-owned schema column-for-column. Migrations are not ported;
//! Django stays schema owner until switchover. Queries stay in the
//! queries layer; this module records column lists, defaults,
//! constraints, indexes, and the `__str__`/property derivations.
//!
//! Column order in each `COLUMNS` const follows Django `_meta` field order
//! (the order recorded in
//! `rust-api/fixtures/assistant/models/columns.json`, F-A6-03, traced to
//! exact Python lines in `rust-api/fixtures/assistant/TRACE.md`), **not**
//! physical `information_schema` ordinal order. FK columns use the Django
//! attnames (`workspace_id`, `thread_id`, …). Order is cosmetic for query
//! building; membership is the contract.
//!
//! # Application-level defaults
//!
//! Every default below is Django-level; the live tables carry no
//! `column_default` (established for D-01), so Rust inserts must supply
//! these values explicitly — there is no DB fallback. `CharField`s without
//! an explicit `default=` (`AssistantMessage.kind`, `AssistantEvent.kind`)
//! fall back to Django's implicit empty string; the fixture renders those
//! as `"''"`, matching the explicit `default=""` fields.
//!
//! # Ported quirks (translate, don't redesign)
//!
//! * `AssistantTurn.model_messages` is the ONLY LLM history-replay source;
//!   `AssistantMessage` rows are a UI transcript projection and
//!   `AssistantEvent` rows are the SSE replay log with an independent
//!   `seq` counter (`models.py:5-12,85-87,123-153`). Mirrored here: the
//!   three tables share no read path in this module.
//! * `AssistantThread.active_turn` is a nullable FK one-active-turn flag
//!   (`models.py:41-49`), not a boolean — cancellation/sweep use the
//!   handle. [`assistant_thread::AssistantThread::active_turn_id`] is
//!   `Option`, and the FK is `SET_NULL`.
//! * `bool()` on the encrypted BYOK/MCP columns treats empty bytes as
//!   missing (`models.py:222-223,261-262,292-293`): [`has_secret`] returns
//!   false for both `None` and `Some(b"")`, not just `None`.
//! * The fixture records `AssistantMCPServer.url` / `UserLLMConfig.base_url`
//!   / `UserSTTConfig.base_url` with kind `"CharField"` although the source
//!   declares `URLField` (`models.py:205,247,278`); `URLField` subclasses
//!   `CharField` and both store `varchar`, so the Rust type is `String`
//!   either way. No behavior turns on the distinction at this layer.
//!
//! Wiring note: the crate root declares `pub mod assistant;`
//! (foundation change, tracked separately); these files are
//! new-files-only for this issue.

use serde::{Deserialize, Serialize};
use std::str::FromStr;

/// Django-level FK delete behavior (ORM-emulated; mirrors
/// `crate::license::models::OnDelete` without coupling domains).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OnDelete {
    /// `models.SET_NULL` — FK column must be nullable.
    SetNull,
    /// `models.CASCADE` — dependent rows are deleted with the parent.
    Cascade,
}

/// `bool()` of an encrypted-at-rest column (`models.py:222-223`,
/// `has_auth_header`; `:261-262` / `:292-293`, `has_api_key`).
///
/// Python truthiness, not nullness: `None` **and** empty bytes both read
/// as missing.
pub fn has_secret(value: &Option<Vec<u8>>) -> bool {
    value.as_ref().is_some_and(|v| !v.is_empty())
}

/// Thread visibility (`models.py:23-25`).
///
/// `"chat"` = user-driven conversation (visible in the assistant UI);
/// `"loop"` = Auto Project Management run thread (hidden).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum ThreadKind {
    /// User-driven conversation (`models.py:24`).
    #[default]
    Chat,
    /// Auto PM run thread (`models.py:25`).
    Loop,
}

impl ThreadKind {
    /// The stored string (`TextChoices` value, `models.py:23-25`).
    pub fn as_str(self) -> &'static str {
        match self {
            ThreadKind::Chat => "chat",
            ThreadKind::Loop => "loop",
        }
    }

    /// The human label (`TextChoices` label, `models.py:23-25`).
    pub fn label(self) -> &'static str {
        match self {
            ThreadKind::Chat => "Chat",
            ThreadKind::Loop => "Loop",
        }
    }

    /// All values in declaration order (matches F-A6-03 `enums.ThreadKind`).
    pub const ALL: &[ThreadKind] = &[ThreadKind::Chat, ThreadKind::Loop];
}

/// Error for unknown thread-kind strings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnknownThreadKind(pub String);

impl std::fmt::Display for UnknownThreadKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "unknown assistant thread kind: {}", self.0)
    }
}

impl std::error::Error for UnknownThreadKind {}

impl FromStr for ThreadKind {
    type Err = UnknownThreadKind;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "chat" => Ok(ThreadKind::Chat),
            "loop" => Ok(ThreadKind::Loop),
            other => Err(UnknownThreadKind(other.to_string())),
        }
    }
}

impl std::fmt::Display for ThreadKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Turn lifecycle (`models.py:64-69`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum TurnStatus {
    /// Enqueued, not yet picked up (`models.py:65`).
    #[default]
    Queued,
    /// Agent run in flight (`models.py:66`).
    Running,
    /// Finished cleanly (`models.py:67`).
    Completed,
    /// Finished with an error (`models.py:68`).
    Failed,
    /// Cancelled (`models.py:69`).
    Cancelled,
}

impl TurnStatus {
    /// The stored string (`TextChoices` value, `models.py:64-69`).
    pub fn as_str(self) -> &'static str {
        match self {
            TurnStatus::Queued => "queued",
            TurnStatus::Running => "running",
            TurnStatus::Completed => "completed",
            TurnStatus::Failed => "failed",
            TurnStatus::Cancelled => "cancelled",
        }
    }

    /// The human label (`TextChoices` label, `models.py:64-69`).
    pub fn label(self) -> &'static str {
        match self {
            TurnStatus::Queued => "Queued",
            TurnStatus::Running => "Running",
            TurnStatus::Completed => "Completed",
            TurnStatus::Failed => "Failed",
            TurnStatus::Cancelled => "Cancelled",
        }
    }

    /// All values in declaration order (matches F-A6-03 `enums.TurnStatus`).
    pub const ALL: &[TurnStatus] = &[
        TurnStatus::Queued,
        TurnStatus::Running,
        TurnStatus::Completed,
        TurnStatus::Failed,
        TurnStatus::Cancelled,
    ];
}

/// Error for unknown turn-status strings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnknownTurnStatus(pub String);

impl std::fmt::Display for UnknownTurnStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "unknown assistant turn status: {}", self.0)
    }
}

impl std::error::Error for UnknownTurnStatus {}

impl FromStr for TurnStatus {
    type Err = UnknownTurnStatus;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "queued" => Ok(TurnStatus::Queued),
            "running" => Ok(TurnStatus::Running),
            "completed" => Ok(TurnStatus::Completed),
            "failed" => Ok(TurnStatus::Failed),
            "cancelled" => Ok(TurnStatus::Cancelled),
            other => Err(UnknownTurnStatus(other.to_string())),
        }
    }
}

impl std::fmt::Display for TurnStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Transcript entry shape (`models.py:108-113`).
///
/// UI projection only — never used for LLM history reconstruction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum MessageKind {
    /// User-written message (`models.py:109`).
    User,
    /// Assistant text (`models.py:110`).
    Assistant,
    /// Tool invocation (`models.py:111`).
    ToolCall,
    /// Tool output (`models.py:112`).
    ToolResult,
    /// Rendered failure (`models.py:113`).
    Error,
}

impl MessageKind {
    /// The stored string (`TextChoices` value, `models.py:108-113`).
    pub fn as_str(self) -> &'static str {
        match self {
            MessageKind::User => "user",
            MessageKind::Assistant => "assistant",
            MessageKind::ToolCall => "tool_call",
            MessageKind::ToolResult => "tool_result",
            MessageKind::Error => "error",
        }
    }

    /// The human label (`TextChoices` label, `models.py:108-113`).
    pub fn label(self) -> &'static str {
        match self {
            MessageKind::User => "User",
            MessageKind::Assistant => "Assistant",
            MessageKind::ToolCall => "Tool call",
            MessageKind::ToolResult => "Tool result",
            MessageKind::Error => "Error",
        }
    }

    /// All values in declaration order (matches F-A6-03 `enums.MessageKind`).
    pub const ALL: &[MessageKind] = &[
        MessageKind::User,
        MessageKind::Assistant,
        MessageKind::ToolCall,
        MessageKind::ToolResult,
        MessageKind::Error,
    ];
}

/// Error for unknown message-kind strings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnknownMessageKind(pub String);

impl std::fmt::Display for UnknownMessageKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "unknown assistant message kind: {}", self.0)
    }
}

impl std::error::Error for UnknownMessageKind {}

impl FromStr for MessageKind {
    type Err = UnknownMessageKind;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "user" => Ok(MessageKind::User),
            "assistant" => Ok(MessageKind::Assistant),
            "tool_call" => Ok(MessageKind::ToolCall),
            "tool_result" => Ok(MessageKind::ToolResult),
            "error" => Ok(MessageKind::Error),
            other => Err(UnknownMessageKind(other.to_string())),
        }
    }
}

impl std::fmt::Display for MessageKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Transcript entry state (`models.py:116-120`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum MessageStatus {
    /// Still streaming (`models.py:117`).
    Streaming,
    /// Finished (`models.py:118`).
    #[default]
    Completed,
    /// Failed (`models.py:119`).
    Failed,
    /// Cancelled (`models.py:120`).
    Cancelled,
}

impl MessageStatus {
    /// The stored string (`TextChoices` value, `models.py:116-120`).
    pub fn as_str(self) -> &'static str {
        match self {
            MessageStatus::Streaming => "streaming",
            MessageStatus::Completed => "completed",
            MessageStatus::Failed => "failed",
            MessageStatus::Cancelled => "cancelled",
        }
    }

    /// The human label (`TextChoices` label, `models.py:116-120`).
    pub fn label(self) -> &'static str {
        match self {
            MessageStatus::Streaming => "Streaming",
            MessageStatus::Completed => "Completed",
            MessageStatus::Failed => "Failed",
            MessageStatus::Cancelled => "Cancelled",
        }
    }

    /// All values in declaration order (matches F-A6-03
    /// `enums.MessageStatus`).
    pub const ALL: &[MessageStatus] = &[
        MessageStatus::Streaming,
        MessageStatus::Completed,
        MessageStatus::Failed,
        MessageStatus::Cancelled,
    ];
}

/// Error for unknown message-status strings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnknownMessageStatus(pub String);

impl std::fmt::Display for UnknownMessageStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "unknown assistant message status: {}", self.0)
    }
}

impl std::error::Error for UnknownMessageStatus {}

impl FromStr for MessageStatus {
    type Err = UnknownMessageStatus;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "streaming" => Ok(MessageStatus::Streaming),
            "completed" => Ok(MessageStatus::Completed),
            "failed" => Ok(MessageStatus::Failed),
            "cancelled" => Ok(MessageStatus::Cancelled),
            other => Err(UnknownMessageStatus(other.to_string())),
        }
    }
}

impl std::fmt::Display for MessageStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// BYOK provider flavor (`models.py:179-181`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum ProviderKind {
    /// OpenAI-compatible endpoint (`models.py:180`).
    #[default]
    OpenaiCompatible,
    /// Anthropic endpoint (`models.py:181`).
    Anthropic,
}

impl ProviderKind {
    /// The stored string (`TextChoices` value, `models.py:179-181`).
    pub fn as_str(self) -> &'static str {
        match self {
            ProviderKind::OpenaiCompatible => "openai_compatible",
            ProviderKind::Anthropic => "anthropic",
        }
    }

    /// The human label (`TextChoices` label, `models.py:179-181`).
    pub fn label(self) -> &'static str {
        match self {
            ProviderKind::OpenaiCompatible => "OpenAI-compatible",
            ProviderKind::Anthropic => "Anthropic",
        }
    }

    /// All values in declaration order (matches F-A6-03
    /// `enums.ProviderKind`).
    pub const ALL: &[ProviderKind] = &[ProviderKind::OpenaiCompatible, ProviderKind::Anthropic];
}

/// Error for unknown provider-kind strings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnknownProviderKind(pub String);

impl std::fmt::Display for UnknownProviderKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "unknown assistant provider kind: {}", self.0)
    }
}

impl std::error::Error for UnknownProviderKind {}

impl FromStr for ProviderKind {
    type Err = UnknownProviderKind;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "openai_compatible" => Ok(ProviderKind::OpenaiCompatible),
            "anthropic" => Ok(ProviderKind::Anthropic),
            other => Err(UnknownProviderKind(other.to_string())),
        }
    }
}

impl std::fmt::Display for ProviderKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// `assistant_thread` table (`models.py:28-61`).
pub mod assistant_thread {
    use super::{Deserialize, OnDelete, Serialize, ThreadKind};

    /// Django table name (`Meta.db_table`, `models.py:54`).
    pub const TABLE: &str = "assistant_thread";

    /// Default `ORDER BY` (`Meta.ordering = ("-updated_at",)`, `models.py:55`).
    pub const ORDERING: &[&str] = &["-updated_at"];

    /// Columns in Django `_meta` field order (matches F-A6-03
    /// `tables.AssistantThread`). FK columns use the Django attnames
    /// (`workspace_id`, `user_id`, `active_turn_id`).
    pub const COLUMNS: &[&str] = &[
        "id",
        "workspace_id",
        "user_id",
        "title",
        "kind",
        "is_archived",
        "active_turn_id",
        "created_at",
        "updated_at",
    ];

    /// Read index (`Meta.indexes`, `models.py:56-58`): the thread list
    /// orders one user's workspace threads by recency.
    pub const INDEX_NAME: &str = "asst_thread_ws_user_idx";
    /// Columns of [`INDEX_NAME`]; the `-` prefix is Django's descending
    /// marker (`-updated_at`), not part of the column name.
    pub const INDEX_FIELDS: &[&str] = &["workspace_id", "user_id", "-updated_at"];

    /// Default `title` (`models.py:36`, `blank=True, default=""`).
    pub const DEFAULT_TITLE: &str = "";
    /// Default `kind` (`models.py:39`, `default=ThreadKind.CHAT`).
    pub const DEFAULT_KIND: ThreadKind = ThreadKind::Chat;
    /// Default `is_archived` (`models.py:40`).
    pub const DEFAULT_IS_ARCHIVED: bool = false;

    /// `title` bound (`models.py:36`, `max_length=255`).
    pub const TITLE_MAX_LENGTH: usize = 255;
    /// `kind` bound (`models.py:39`, `max_length=16`).
    pub const KIND_MAX_LENGTH: usize = 16;

    /// `workspace` FK (`models.py:30-32`): required, `CASCADE`,
    /// `related_name="assistant_threads"`.
    pub const WORKSPACE_ON_DELETE: OnDelete = OnDelete::Cascade;
    pub const WORKSPACE_NULLABLE: bool = false;
    pub const WORKSPACE_RELATED_NAME: &str = "assistant_threads";

    /// `user` FK (`models.py:33-35`): required, `CASCADE`,
    /// `related_name="assistant_threads"`.
    pub const USER_ON_DELETE: OnDelete = OnDelete::Cascade;
    pub const USER_NULLABLE: bool = false;
    pub const USER_RELATED_NAME: &str = "assistant_threads";

    /// `active_turn` FK (`models.py:43-49`): the single in-flight turn for
    /// this thread (one-active-turn flag). Nullable, `SET_NULL`,
    /// `related_name="+"` (no reverse accessor).
    pub const ACTIVE_TURN_ON_DELETE: OnDelete = OnDelete::SetNull;
    pub const ACTIVE_TURN_NULLABLE: bool = true;
    pub const ACTIVE_TURN_RELATED_NAME: &str = "+";

    /// One `assistant_thread` row. `title` stores `""`, never `NULL`;
    /// timestamps are `timestamptz`.
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    pub struct AssistantThread {
        pub id: uuid::Uuid,
        pub workspace_id: uuid::Uuid,
        pub user_id: uuid::Uuid,
        pub title: String,
        pub kind: String,
        pub is_archived: bool,
        pub active_turn_id: Option<uuid::Uuid>,
        pub created_at: chrono::DateTime<chrono::Utc>,
        pub updated_at: chrono::DateTime<chrono::Utc>,
    }

    impl std::fmt::Display for AssistantThread {
        /// `__str__` (`models.py:60-61`).
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "AssistantThread({})", self.id)
        }
    }
}

/// `assistant_turn` table (`models.py:72-105`).
///
/// The unit of agent execution and the *only* source of LLM history
/// replay (`model_messages`, written once on completion).
pub mod assistant_turn {
    use super::{Deserialize, OnDelete, Serialize, TurnStatus};

    /// Django table name (`Meta.db_table`, `models.py:97`).
    pub const TABLE: &str = "assistant_turn";

    /// Default `ORDER BY` (`Meta.ordering = ("created_at",)`, `models.py:98`).
    pub const ORDERING: &[&str] = &["created_at"];

    /// Columns in Django `_meta` field order (matches F-A6-03
    /// `tables.AssistantTurn`). FK columns use the Django attnames
    /// (`thread_id`, `user_message_id`).
    pub const COLUMNS: &[&str] = &[
        "id",
        "thread_id",
        "user_message_id",
        "status",
        "model_messages",
        "usage",
        "model_used",
        "error_code",
        "error_detail",
        "created_at",
        "started_at",
        "completed_at",
    ];

    /// Read index (`Meta.indexes`, `models.py:100`): turns of one thread
    /// in creation order.
    pub const THREAD_INDEX_NAME: &str = "asst_turn_thread_idx";
    /// Columns of [`THREAD_INDEX_NAME`].
    pub const THREAD_INDEX_FIELDS: &[&str] = &["thread_id", "created_at"];

    /// Read index (`Meta.indexes`, `models.py:101`): sweep/cancel scans
    /// over in-flight turns.
    pub const STATUS_INDEX_NAME: &str = "asst_turn_status_idx";
    /// Columns of [`STATUS_INDEX_NAME`].
    pub const STATUS_INDEX_FIELDS: &[&str] = &["status", "started_at"];

    /// Default `status` (`models.py:84`, `default=TurnStatus.QUEUED`).
    pub const DEFAULT_STATUS: TurnStatus = TurnStatus::Queued;
    /// Default `model_used` (`models.py:89`, `blank=True, default=""`).
    pub const DEFAULT_MODEL_USED: &str = "";
    /// Default `error_code` (`models.py:90`, `blank=True, default=""`).
    pub const DEFAULT_ERROR_CODE: &str = "";
    /// Default `error_detail` (`models.py:91`, `blank=True, default=""`).
    pub const DEFAULT_ERROR_DETAIL: &str = "";

    /// `status` bound (`models.py:84`, `max_length=16`).
    pub const STATUS_MAX_LENGTH: usize = 16;
    /// `model_used` bound (`models.py:89`, `max_length=255`).
    pub const MODEL_USED_MAX_LENGTH: usize = 255;
    /// `error_code` bound (`models.py:90`, `max_length=64`).
    pub const ERROR_CODE_MAX_LENGTH: usize = 64;

    /// `thread` FK (`models.py:74-76`): required, `CASCADE`,
    /// `related_name="turns"`.
    pub const THREAD_ON_DELETE: OnDelete = OnDelete::Cascade;
    pub const THREAD_NULLABLE: bool = false;
    pub const THREAD_RELATED_NAME: &str = "turns";

    /// `user_message` FK (`models.py:77-83`): the turn's triggering
    /// message. Nullable, `SET_NULL`, `related_name="+"`.
    pub const USER_MESSAGE_ON_DELETE: OnDelete = OnDelete::SetNull;
    pub const USER_MESSAGE_NULLABLE: bool = true;
    pub const USER_MESSAGE_RELATED_NAME: &str = "+";

    /// One `assistant_turn` row. `model_messages` / `usage` keep `NULL`
    /// until written on completion; `model_used`, `error_code`, and
    /// `error_detail` store `""`, never `NULL`.
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    pub struct AssistantTurn {
        pub id: uuid::Uuid,
        pub thread_id: uuid::Uuid,
        pub user_message_id: Option<uuid::Uuid>,
        pub status: String,
        pub model_messages: Option<serde_json::Value>,
        pub usage: Option<serde_json::Value>,
        pub model_used: String,
        pub error_code: String,
        pub error_detail: String,
        pub created_at: chrono::DateTime<chrono::Utc>,
        pub started_at: Option<chrono::DateTime<chrono::Utc>>,
        pub completed_at: Option<chrono::DateTime<chrono::Utc>>,
    }

    impl std::fmt::Display for AssistantTurn {
        /// `__str__` (`models.py:104-105`).
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "AssistantTurn({}, {})", self.id, self.status)
        }
    }
}

/// `assistant_message` table (`models.py:123-149`).
///
/// UI transcript projection. NEVER used for LLM history reconstruction.
pub mod assistant_message {
    use super::{Deserialize, MessageStatus, OnDelete, Serialize};

    /// Django table name (`Meta.db_table`, `models.py:142`).
    pub const TABLE: &str = "assistant_message";

    /// Default `ORDER BY` (`Meta.ordering = ("seq",)`, `models.py:143`).
    pub const ORDERING: &[&str] = &["seq"];

    /// Columns in Django `_meta` field order (matches F-A6-03
    /// `tables.AssistantMessage`). FK columns use the Django attnames
    /// (`thread_id`, `turn_id`).
    pub const COLUMNS: &[&str] = &[
        "id",
        "thread_id",
        "turn_id",
        "seq",
        "kind",
        "display_content",
        "payload",
        "status",
        "created_at",
        "completed_at",
    ];

    /// Read index (`Meta.indexes`, `models.py:144-146`): transcript order
    /// within one thread.
    pub const INDEX_NAME: &str = "asst_msg_thread_seq_idx";
    /// Columns of [`INDEX_NAME`].
    pub const INDEX_FIELDS: &[&str] = &["thread_id", "seq"];

    /// Default `seq` (`models.py:133`, transcript ordering only).
    pub const DEFAULT_SEQ: i64 = 0;
    /// `kind` has no explicit `default=` (`models.py:134`); Django falls
    /// back to its implicit empty string (fixture renders `"''"`).
    pub const DEFAULT_KIND: &str = "";
    /// Default `display_content` (`models.py:135`, `blank=True, default=""`).
    pub const DEFAULT_DISPLAY_CONTENT: &str = "";
    /// Default `status` (`models.py:137`,
    /// `default=MessageStatus.COMPLETED`).
    pub const DEFAULT_STATUS: MessageStatus = MessageStatus::Completed;

    /// `kind` bound (`models.py:134`, `max_length=16`).
    pub const KIND_MAX_LENGTH: usize = 16;
    /// `status` bound (`models.py:137`, `max_length=16`).
    pub const STATUS_MAX_LENGTH: usize = 16;

    /// `thread` FK (`models.py:127-129`): required, `CASCADE`,
    /// `related_name="messages"`.
    pub const THREAD_ON_DELETE: OnDelete = OnDelete::Cascade;
    pub const THREAD_NULLABLE: bool = false;
    pub const THREAD_RELATED_NAME: &str = "messages";

    /// `turn` FK (`models.py:130-132`): the turn that produced this entry,
    /// if any. Nullable, `CASCADE`, `related_name="messages"`.
    pub const TURN_ON_DELETE: OnDelete = OnDelete::Cascade;
    pub const TURN_NULLABLE: bool = true;
    pub const TURN_RELATED_NAME: &str = "messages";

    /// Fresh default for `payload` (`JSONField(default=dict)`,
    /// `models.py:136`): a new empty object on every call.
    pub fn default_payload() -> serde_json::Value {
        serde_json::Value::Object(Default::default())
    }

    /// One `assistant_message` row. `display_content` stores `""`, never
    /// `NULL`; `payload` is `jsonb NOT NULL` defaulting to `{}`.
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    pub struct AssistantMessage {
        pub id: uuid::Uuid,
        pub thread_id: uuid::Uuid,
        pub turn_id: Option<uuid::Uuid>,
        pub seq: i64,
        pub kind: String,
        pub display_content: String,
        pub payload: serde_json::Value,
        pub status: String,
        pub created_at: chrono::DateTime<chrono::Utc>,
        pub completed_at: Option<chrono::DateTime<chrono::Utc>>,
    }

    impl std::fmt::Display for AssistantMessage {
        /// `__str__` (`models.py:148-149`).
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "AssistantMessage({}, {})", self.id, self.kind)
        }
    }
}

/// `assistant_event` table (`models.py:152-176`).
///
/// SSE replay log. Independent `seq` counter from `AssistantMessage`.
pub mod assistant_event {
    use super::{Deserialize, OnDelete, Serialize};

    /// Django table name (`Meta.db_table`, `models.py:169`).
    pub const TABLE: &str = "assistant_event";

    /// Default `ORDER BY` (`Meta.ordering = ("seq",)`, `models.py:170`).
    pub const ORDERING: &[&str] = &["seq"];

    /// Columns in Django `_meta` field order (matches F-A6-03
    /// `tables.AssistantEvent`). `message_id` is a plain `UUIDField`,
    /// not an FK, so its attname is unchanged.
    pub const COLUMNS: &[&str] = &[
        "id",
        "thread_id",
        "turn_id",
        "seq",
        "kind",
        "message_id",
        "payload",
        "created_at",
    ];

    /// Read index (`Meta.indexes`, `models.py:171-173`): SSE replay cursor
    /// within one thread.
    pub const INDEX_NAME: &str = "asst_event_thread_seq_idx";
    /// Columns of [`INDEX_NAME`].
    pub const INDEX_FIELDS: &[&str] = &["thread_id", "seq"];

    /// Default `seq` (`models.py:162`, SSE replay cursor).
    pub const DEFAULT_SEQ: i64 = 0;
    /// `kind` has no explicit `default=` (`models.py:163`); Django falls
    /// back to its implicit empty string (fixture renders `"''"`).
    pub const DEFAULT_KIND: &str = "";

    /// `kind` bound (`models.py:163`, `max_length=64`).
    pub const KIND_MAX_LENGTH: usize = 64;

    /// `thread` FK (`models.py:156-158`): required, `CASCADE`,
    /// `related_name="events"`.
    pub const THREAD_ON_DELETE: OnDelete = OnDelete::Cascade;
    pub const THREAD_NULLABLE: bool = false;
    pub const THREAD_RELATED_NAME: &str = "events";

    /// `turn` FK (`models.py:159-161`): the turn that emitted this event,
    /// if any. Nullable, `CASCADE`, `related_name="events"`.
    pub const TURN_ON_DELETE: OnDelete = OnDelete::Cascade;
    pub const TURN_NULLABLE: bool = true;
    pub const TURN_RELATED_NAME: &str = "events";

    /// Fresh default for `payload` (`JSONField(default=dict)`,
    /// `models.py:165`): a new empty object on every call.
    pub fn default_payload() -> serde_json::Value {
        serde_json::Value::Object(Default::default())
    }

    /// One `assistant_event` row. `payload` is `jsonb NOT NULL`
    /// defaulting to `{}`; `message_id` links the transcript entry the
    /// event belongs to, when there is one.
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    pub struct AssistantEvent {
        pub id: i64,
        pub thread_id: uuid::Uuid,
        pub turn_id: Option<uuid::Uuid>,
        pub seq: i64,
        pub kind: String,
        pub message_id: Option<uuid::Uuid>,
        pub payload: serde_json::Value,
        pub created_at: chrono::DateTime<chrono::Utc>,
    }

    impl std::fmt::Display for AssistantEvent {
        /// `__str__` (`models.py:175-176`).
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(
                f,
                "AssistantEvent({}, {}, seq={})",
                self.thread_id, self.kind, self.seq
            )
        }
    }
}

/// `assistant_mcp_server` table (`models.py:184-235`).
///
/// A user-configured MCP tool server the assistant may call during a
/// turn. Per-user and global across workspaces.
pub mod assistant_mcp_server {
    use super::{Deserialize, OnDelete, Serialize};

    /// Django table name (`Meta.db_table`, `models.py:212`).
    pub const TABLE: &str = "assistant_mcp_server";

    /// Default `ORDER BY` (`Meta.ordering = ("created_at",)`,
    /// `models.py:213`).
    pub const ORDERING: &[&str] = &["created_at"];

    /// Columns in Django `_meta` field order (matches F-A6-03
    /// `tables.AssistantMCPServer`).
    pub const COLUMNS: &[&str] = &[
        "id",
        "user_id",
        "name",
        "url",
        "auth_header_encrypted",
        "is_enabled",
        "created_at",
        "updated_at",
    ];

    /// Unique constraint (`Meta.constraints`, `models.py:214-216`): one
    /// server name per user.
    pub const UNIQUE_CONSTRAINT: &str = "assistant_mcp_server_user_name_uniq";
    /// Physical columns of [`UNIQUE_CONSTRAINT`] (`user` resolves to
    /// `user_id` at the DB level).
    pub const UNIQUE_FIELDS: &[&str] = &["user_id", "name"];

    /// Default `is_enabled` (`models.py:207`).
    pub const DEFAULT_IS_ENABLED: bool = true;

    /// `name` bound (`models.py:204`, `max_length=80`).
    pub const NAME_MAX_LENGTH: usize = 80;
    /// `url` bound (`models.py:205`, `max_length=500`).
    pub const URL_MAX_LENGTH: usize = 500;

    /// `user` FK (`models.py:199-203`): required, `CASCADE`,
    /// `related_name="assistant_mcp_servers"`.
    pub const USER_ON_DELETE: OnDelete = OnDelete::Cascade;
    pub const USER_NULLABLE: bool = false;
    pub const USER_RELATED_NAME: &str = "assistant_mcp_servers";

    /// One `assistant_mcp_server` row. `auth_header_encrypted` holds a
    /// full `Authorization` header value encrypted at rest, when set.
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    pub struct AssistantMCPServer {
        pub id: uuid::Uuid,
        pub user_id: uuid::Uuid,
        pub name: String,
        pub url: String,
        pub auth_header_encrypted: Option<Vec<u8>>,
        pub is_enabled: bool,
        pub created_at: chrono::DateTime<chrono::Utc>,
        pub updated_at: chrono::DateTime<chrono::Utc>,
    }

    impl std::fmt::Display for AssistantMCPServer {
        /// `__str__` (`models.py:218-219`).
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "AssistantMCPServer({}, {})", self.user_id, self.name)
        }
    }

    /// `has_auth_header` (`models.py:221-223`): Python truthiness — empty
    /// bytes read as missing (see [`super::has_secret`]).
    pub fn has_auth_header(auth_header_encrypted: &Option<Vec<u8>>) -> bool {
        super::has_secret(auth_header_encrypted)
    }

    /// `tool_prefix` (`models.py:226-235`): slugified `name` namespacing
    /// this server's tool names. The reserved `mcp_` namespace keeps
    /// user-provided tool names disjoint from the built-in tools.
    /// Falls back to the row id when the name has no alphanumeric
    /// content, so a prefix is always non-empty and stable.
    ///
    /// The slug is `re.sub(r"[^a-z0-9]+", "_", name.strip().lower())`
    /// with leading/trailing `_` stripped; the input is already
    /// lowercased, so matching ASCII alphanumerics is exactly the
    /// `[a-z0-9]` class (any cased mapping that survives `lower()` is
    /// lowercase by construction).
    pub fn tool_prefix(name: &str, id: &uuid::Uuid) -> String {
        let lowered = name.trim().to_lowercase();
        let mut slug = String::with_capacity(lowered.len());
        let mut trailing_separator = true;
        for ch in lowered.chars() {
            if ch.is_ascii_alphanumeric() {
                slug.push(ch);
                trailing_separator = false;
            } else if !trailing_separator {
                slug.push('_');
                trailing_separator = true;
            }
        }
        if trailing_separator && !slug.is_empty() {
            slug.pop();
        }
        if slug.is_empty() {
            let hex = id.hyphenated().to_string().replace('-', "");
            format!("mcp_{}", &hex[..8])
        } else {
            format!("mcp_{slug}")
        }
    }
}

/// `assistant_user_llm_config` table (`models.py:238-262`).
///
/// Per-user BYOK configuration, global across workspaces.
pub mod user_llm_config {
    use super::{Deserialize, OnDelete, ProviderKind, Serialize};

    /// Django table name (`Meta.db_table`, `models.py:255`).
    pub const TABLE: &str = "assistant_user_llm_config";

    /// No `Meta.ordering` (`models.py:254-255`): rows are read by key.
    pub const ORDERING: &[&str] = &[];

    /// Columns in Django `_meta` field order (matches F-A6-03
    /// `tables.UserLLMConfig`).
    pub const COLUMNS: &[&str] = &[
        "id",
        "user_id",
        "provider_kind",
        "base_url",
        "model_name",
        "api_key_encrypted",
        "last_verified_at",
        "created_at",
        "updated_at",
    ];

    /// The `OneToOneField` (`models.py:241-243`) creates a unique
    /// constraint on `user_id` (Django auto-names it; the name is not
    /// pinned by F-A6-03, only the column is).
    pub const UNIQUE_COLUMNS: &[&str] = &["user_id"];

    /// Default `provider_kind` (`models.py:244-246`,
    /// `default=ProviderKind.OPENAI_COMPATIBLE`).
    pub const DEFAULT_PROVIDER_KIND: ProviderKind = ProviderKind::OpenaiCompatible;
    /// Default `base_url` (`models.py:247`, `blank=True, default=""`).
    pub const DEFAULT_BASE_URL: &str = "";
    /// Default `model_name` (`models.py:248`, `blank=True, default=""`).
    pub const DEFAULT_MODEL_NAME: &str = "";

    /// `provider_kind` bound (`models.py:244-246`, `max_length=32`).
    pub const PROVIDER_KIND_MAX_LENGTH: usize = 32;
    /// `base_url` bound (`models.py:247`, `max_length=500`).
    pub const BASE_URL_MAX_LENGTH: usize = 500;
    /// `model_name` bound (`models.py:248`, `max_length=255`).
    pub const MODEL_NAME_MAX_LENGTH: usize = 255;

    /// `user` one-to-one (`models.py:241-243`): required, `CASCADE`,
    /// `related_name="assistant_llm_config"`.
    pub const USER_ON_DELETE: OnDelete = OnDelete::Cascade;
    pub const USER_NULLABLE: bool = false;
    pub const USER_RELATED_NAME: &str = "assistant_llm_config";

    /// One `assistant_user_llm_config` row. The API never exposes the
    /// stored key — only [`has_api_key`].
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    pub struct UserLLMConfig {
        pub id: i64,
        pub user_id: uuid::Uuid,
        pub provider_kind: String,
        pub base_url: String,
        pub model_name: String,
        pub api_key_encrypted: Option<Vec<u8>>,
        pub last_verified_at: Option<chrono::DateTime<chrono::Utc>>,
        pub created_at: chrono::DateTime<chrono::Utc>,
        pub updated_at: chrono::DateTime<chrono::Utc>,
    }

    impl std::fmt::Display for UserLLMConfig {
        /// `__str__` (`models.py:257-258`).
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "UserLLMConfig({}, {})", self.user_id, self.provider_kind)
        }
    }

    /// `has_api_key` (`models.py:260-262`): Python truthiness — empty
    /// bytes read as missing (see [`super::has_secret`]).
    pub fn has_api_key(api_key_encrypted: &Option<Vec<u8>>) -> bool {
        super::has_secret(api_key_encrypted)
    }
}

/// `assistant_user_stt_config` table (`models.py:265-293`).
///
/// Per-user BYO speech-to-text configuration, global across workspaces.
/// Mirrors [`user_llm_config::UserLLMConfig`] but for the
/// OpenAI-compatible `/v1/audio/transcriptions` endpoint; there is a
/// single provider kind, so unlike `UserLLMConfig` there is no
/// `provider_kind` column (`models.py:270-271`).
pub mod user_stt_config {
    use super::{Deserialize, OnDelete, Serialize};

    /// Django table name (`Meta.db_table`, `models.py:283`).
    pub const TABLE: &str = "assistant_user_stt_config";

    /// No `Meta.ordering`: rows are read by key.
    pub const ORDERING: &[&str] = &[];

    /// Columns in Django `_meta` field order (matches F-A6-03
    /// `tables.UserSTTConfig`).
    pub const COLUMNS: &[&str] = &[
        "id",
        "user_id",
        "base_url",
        "model_name",
        "api_key_encrypted",
        "last_verified_at",
        "created_at",
        "updated_at",
    ];

    /// The `OneToOneField` (`models.py:275-277`) creates a unique
    /// constraint on `user_id` (Django auto-names it; the name is not
    /// pinned by F-A6-03, only the column is).
    pub const UNIQUE_COLUMNS: &[&str] = &["user_id"];

    /// Default `base_url` (`models.py:278`, `blank=True, default=""`).
    pub const DEFAULT_BASE_URL: &str = "";
    /// Default `model_name` (`models.py:279`, `blank=True, default=""`).
    pub const DEFAULT_MODEL_NAME: &str = "";

    /// `base_url` bound (`models.py:278`, `max_length=500`).
    pub const BASE_URL_MAX_LENGTH: usize = 500;
    /// `model_name` bound (`models.py:279`, `max_length=255`).
    pub const MODEL_NAME_MAX_LENGTH: usize = 255;

    /// `user` one-to-one (`models.py:275-277`): required, `CASCADE`,
    /// `related_name="assistant_stt_config"`.
    pub const USER_ON_DELETE: OnDelete = OnDelete::Cascade;
    pub const USER_NULLABLE: bool = false;
    pub const USER_RELATED_NAME: &str = "assistant_stt_config";

    /// One `assistant_user_stt_config` row. The API never exposes the
    /// stored key — only [`has_api_key`].
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    pub struct UserSTTConfig {
        pub id: i64,
        pub user_id: uuid::Uuid,
        pub base_url: String,
        pub model_name: String,
        pub api_key_encrypted: Option<Vec<u8>>,
        pub last_verified_at: Option<chrono::DateTime<chrono::Utc>>,
        pub created_at: chrono::DateTime<chrono::Utc>,
        pub updated_at: chrono::DateTime<chrono::Utc>,
    }

    impl std::fmt::Display for UserSTTConfig {
        /// `__str__` (`models.py:288-289`).
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "UserSTTConfig({})", self.user_id)
        }
    }

    /// `has_api_key` (`models.py:291-293`): Python truthiness — empty
    /// bytes read as missing (see [`super::has_secret`]).
    pub fn has_api_key(api_key_encrypted: &Option<Vec<u8>>) -> bool {
        super::has_secret(api_key_encrypted)
    }
}

#[cfg(test)]
mod tests {
    use super::assistant_event as event;
    use super::assistant_mcp_server as mcp;
    use super::assistant_message as message;
    use super::assistant_thread as thread;
    use super::assistant_turn as turn;
    use super::user_llm_config as llm;
    use super::user_stt_config as stt;
    use super::{
        has_secret, MessageKind, MessageStatus, OnDelete, ProviderKind, ThreadKind, TurnStatus,
    };

    fn fixture_root() -> serde_json::Value {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/assistant/models/columns.json");
        let text = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
        serde_json::from_str(&text).expect("fixture is valid JSON")
    }

    /// Copy a const into an owned vec so assertions compare two runtime
    /// values (clippy denies asserting on constants directly).
    fn owned(cols: &[&str]) -> Vec<String> {
        cols.iter().map(|c| (*c).to_string()).collect()
    }

    /// Physical column names for one fixture table: `ForeignKey` /
    /// `OneToOneField` entries resolve `name` to the Django attname
    /// `name_id`; every other field stores under its own name
    /// (`message_id` is a plain `UUIDField`, so it is unchanged).
    fn fixture_columns(table: &serde_json::Value) -> Vec<String> {
        table["columns"]
            .as_array()
            .expect("table has columns array")
            .iter()
            .map(|c| {
                let name = c[0].as_str().expect("column entry has a name");
                let kind = c[1].as_str().expect("column entry has a kind");
                match kind {
                    "ForeignKey" | "OneToOneField" => format!("{name}_id"),
                    _ => name.to_string(),
                }
            })
            .collect()
    }

    fn fixture_ordering(table: &serde_json::Value) -> Vec<String> {
        table["ordering"]
            .as_array()
            .expect("table has ordering array")
            .iter()
            .map(|o| o.as_str().expect("ordering entry is a string").to_string())
            .collect()
    }

    fn fixture_table(name: &str) -> serde_json::Value {
        fixture_root()["tables"][name].clone()
    }

    fn enum_pairs(name: &str) -> Vec<(String, String)> {
        fixture_root()["enums"][name]["values"]
            .as_array()
            .unwrap_or_else(|| panic!("enum {name} has values"))
            .iter()
            .map(|e| {
                (
                    e[0].as_str().expect("enum value").to_string(),
                    e[1].as_str().expect("enum label").to_string(),
                )
            })
            .collect()
    }

    /// The row struct serializes to exactly the `COLUMNS` key set, and
    /// survives a serde round trip unchanged (field-for-field parity).
    fn assert_row<T>(row: &T, columns: &[&str])
    where
        T: serde::Serialize + serde::de::DeserializeOwned + PartialEq + std::fmt::Debug,
    {
        let value = serde_json::to_value(row).expect("row serializes");
        let mut keys: Vec<String> = value
            .as_object()
            .expect("row serializes to an object")
            .keys()
            .cloned()
            .collect();
        keys.sort();
        let mut expected: Vec<String> = columns.iter().map(|c| c.to_string()).collect();
        expected.sort();
        assert_eq!(keys, expected);
        let back: T = serde_json::from_value(value).expect("row round-trips");
        assert_eq!(&back, row);
    }

    /// The table defs feed sea-query directly: render the canonical
    /// `SELECT <columns> FROM <table>` and pin the exact SQL.
    fn select_sql(table: &str, columns: &[&str]) -> String {
        let mut stmt = sea_query::Query::select();
        for column in columns {
            stmt.column(sea_query::Alias::new(*column));
        }
        stmt.from(sea_query::Alias::new(table));
        stmt.to_string(sea_query::PostgresQueryBuilder)
    }

    fn sample_time() -> chrono::DateTime<chrono::Utc> {
        chrono::DateTime::from_timestamp(1_700_000_000, 0).expect("fixed sample time")
    }

    #[test]
    fn thread_kind_matches_fixture() {
        let expected = enum_pairs("ThreadKind");
        let actual: Vec<(String, String)> = ThreadKind::ALL
            .iter()
            .map(|k| (k.as_str().to_string(), k.label().to_string()))
            .collect();
        assert_eq!(actual, expected);
        assert_eq!(ThreadKind::ALL.len(), 2);
        for kind in ThreadKind::ALL {
            assert_eq!(kind.to_string(), kind.as_str());
            assert_eq!(kind.as_str().parse::<ThreadKind>(), Ok(*kind));
        }
        assert_eq!(
            "sms".parse::<ThreadKind>(),
            Err(super::UnknownThreadKind("sms".to_string()))
        );
        let default = ThreadKind::default();
        assert_eq!(default, ThreadKind::Chat);
        let kind_default = thread::DEFAULT_KIND;
        assert_eq!(kind_default.as_str(), "chat");
    }

    #[test]
    fn turn_status_matches_fixture() {
        let expected = enum_pairs("TurnStatus");
        let actual: Vec<(String, String)> = TurnStatus::ALL
            .iter()
            .map(|s| (s.as_str().to_string(), s.label().to_string()))
            .collect();
        assert_eq!(actual, expected);
        assert_eq!(TurnStatus::ALL.len(), 5);
        for status in TurnStatus::ALL {
            assert_eq!(status.to_string(), status.as_str());
            assert_eq!(status.as_str().parse::<TurnStatus>(), Ok(*status));
        }
        assert_eq!(
            "paused".parse::<TurnStatus>(),
            Err(super::UnknownTurnStatus("paused".to_string()))
        );
        let default = TurnStatus::default();
        assert_eq!(default, TurnStatus::Queued);
        let status_default = turn::DEFAULT_STATUS;
        assert_eq!(status_default.as_str(), "queued");
    }

    #[test]
    fn message_kinds_and_statuses_match_fixture() {
        let expected_kinds = enum_pairs("MessageKind");
        let actual_kinds: Vec<(String, String)> = MessageKind::ALL
            .iter()
            .map(|k| (k.as_str().to_string(), k.label().to_string()))
            .collect();
        assert_eq!(actual_kinds, expected_kinds);
        assert_eq!(MessageKind::ALL.len(), 5);
        for kind in MessageKind::ALL {
            assert_eq!(kind.to_string(), kind.as_str());
            assert_eq!(kind.as_str().parse::<MessageKind>(), Ok(*kind));
        }
        assert_eq!(
            "delta".parse::<MessageKind>(),
            Err(super::UnknownMessageKind("delta".to_string()))
        );

        let expected_statuses = enum_pairs("MessageStatus");
        let actual_statuses: Vec<(String, String)> = MessageStatus::ALL
            .iter()
            .map(|s| (s.as_str().to_string(), s.label().to_string()))
            .collect();
        assert_eq!(actual_statuses, expected_statuses);
        assert_eq!(MessageStatus::ALL.len(), 4);
        for status in MessageStatus::ALL {
            assert_eq!(status.to_string(), status.as_str());
            assert_eq!(status.as_str().parse::<MessageStatus>(), Ok(*status));
        }
        assert_eq!(
            "paused".parse::<MessageStatus>(),
            Err(super::UnknownMessageStatus("paused".to_string()))
        );
        let default = MessageStatus::default();
        assert_eq!(default, MessageStatus::Completed);
        let status_default = message::DEFAULT_STATUS;
        assert_eq!(status_default.as_str(), "completed");
    }

    #[test]
    fn provider_kind_matches_fixture() {
        let expected = enum_pairs("ProviderKind");
        let actual: Vec<(String, String)> = ProviderKind::ALL
            .iter()
            .map(|k| (k.as_str().to_string(), k.label().to_string()))
            .collect();
        assert_eq!(actual, expected);
        assert_eq!(ProviderKind::ALL.len(), 2);
        for kind in ProviderKind::ALL {
            assert_eq!(kind.to_string(), kind.as_str());
            assert_eq!(kind.as_str().parse::<ProviderKind>(), Ok(*kind));
        }
        assert_eq!(
            "bedrock".parse::<ProviderKind>(),
            Err(super::UnknownProviderKind("bedrock".to_string()))
        );
        let default = ProviderKind::default();
        assert_eq!(default, ProviderKind::OpenaiCompatible);
        let kind_default = llm::DEFAULT_PROVIDER_KIND;
        assert_eq!(kind_default.as_str(), "openai_compatible");
    }

    #[test]
    fn thread_columns_match_fixture() {
        let fx = fixture_table("AssistantThread");
        assert_eq!(fx["db_table"].as_str(), Some(thread::TABLE));
        assert_eq!(fixture_ordering(&fx), owned(thread::ORDERING));
        assert_eq!(fixture_columns(&fx), owned(thread::COLUMNS));
        assert_eq!(thread::COLUMNS.len(), 9);

        let index = thread::INDEX_NAME;
        assert_eq!(index, "asst_thread_ws_user_idx");
        let fields = owned(thread::INDEX_FIELDS);
        assert_eq!(fields, vec!["workspace_id", "user_id", "-updated_at"]);

        let title = thread::DEFAULT_TITLE;
        assert_eq!(title, "");
        let archived = thread::DEFAULT_IS_ARCHIVED;
        assert!(!archived);
        let title_max: usize = thread::TITLE_MAX_LENGTH;
        assert_eq!(title_max, 255);
        let kind_max: usize = thread::KIND_MAX_LENGTH;
        assert_eq!(kind_max, 16);

        let ws_delete = thread::WORKSPACE_ON_DELETE;
        assert_eq!(ws_delete, OnDelete::Cascade);
        let ws_nullable = thread::WORKSPACE_NULLABLE;
        assert!(!ws_nullable);
        let active_delete = thread::ACTIVE_TURN_ON_DELETE;
        assert_eq!(active_delete, OnDelete::SetNull);
        let active_nullable = thread::ACTIVE_TURN_NULLABLE;
        assert!(active_nullable);
        let active_related = thread::ACTIVE_TURN_RELATED_NAME;
        assert_eq!(active_related, "+");

        let row = thread::AssistantThread {
            id: uuid::Uuid::nil(),
            workspace_id: uuid::Uuid::nil(),
            user_id: uuid::Uuid::nil(),
            title: String::new(),
            kind: ThreadKind::Chat.as_str().to_string(),
            is_archived: false,
            active_turn_id: None,
            created_at: sample_time(),
            updated_at: sample_time(),
        };
        assert_row(&row, thread::COLUMNS);
        assert_eq!(
            row.to_string(),
            format!("AssistantThread({})", uuid::Uuid::nil())
        );
        assert_eq!(
            select_sql(thread::TABLE, thread::COLUMNS),
            r#"SELECT "id", "workspace_id", "user_id", "title", "kind", "is_archived", "active_turn_id", "created_at", "updated_at" FROM "assistant_thread""#
        );
    }

    #[test]
    fn turn_columns_match_fixture() {
        let fx = fixture_table("AssistantTurn");
        assert_eq!(fx["db_table"].as_str(), Some(turn::TABLE));
        assert_eq!(fixture_ordering(&fx), owned(turn::ORDERING));
        assert_eq!(fixture_columns(&fx), owned(turn::COLUMNS));
        assert_eq!(turn::COLUMNS.len(), 12);

        let thread_index = turn::THREAD_INDEX_NAME;
        assert_eq!(thread_index, "asst_turn_thread_idx");
        let thread_fields = owned(turn::THREAD_INDEX_FIELDS);
        assert_eq!(thread_fields, vec!["thread_id", "created_at"]);
        let status_index = turn::STATUS_INDEX_NAME;
        assert_eq!(status_index, "asst_turn_status_idx");
        let status_fields = owned(turn::STATUS_INDEX_FIELDS);
        assert_eq!(status_fields, vec!["status", "started_at"]);

        let model_used = turn::DEFAULT_MODEL_USED;
        assert_eq!(model_used, "");
        let error_code = turn::DEFAULT_ERROR_CODE;
        assert_eq!(error_code, "");
        let error_detail = turn::DEFAULT_ERROR_DETAIL;
        assert_eq!(error_detail, "");
        let user_message_delete = turn::USER_MESSAGE_ON_DELETE;
        assert_eq!(user_message_delete, OnDelete::SetNull);
        let thread_related = turn::THREAD_RELATED_NAME;
        assert_eq!(thread_related, "turns");

        let row = turn::AssistantTurn {
            id: uuid::Uuid::nil(),
            thread_id: uuid::Uuid::nil(),
            user_message_id: None,
            status: TurnStatus::Queued.as_str().to_string(),
            model_messages: None,
            usage: None,
            model_used: String::new(),
            error_code: String::new(),
            error_detail: String::new(),
            created_at: sample_time(),
            started_at: None,
            completed_at: None,
        };
        assert_row(&row, turn::COLUMNS);
        assert_eq!(
            row.to_string(),
            format!("AssistantTurn({}, queued)", uuid::Uuid::nil())
        );
        assert_eq!(
            select_sql(turn::TABLE, turn::COLUMNS),
            r#"SELECT "id", "thread_id", "user_message_id", "status", "model_messages", "usage", "model_used", "error_code", "error_detail", "created_at", "started_at", "completed_at" FROM "assistant_turn""#
        );
    }

    #[test]
    fn message_columns_match_fixture() {
        let fx = fixture_table("AssistantMessage");
        assert_eq!(fx["db_table"].as_str(), Some(message::TABLE));
        assert_eq!(fixture_ordering(&fx), owned(message::ORDERING));
        assert_eq!(fixture_columns(&fx), owned(message::COLUMNS));
        assert_eq!(message::COLUMNS.len(), 10);

        let index = message::INDEX_NAME;
        assert_eq!(index, "asst_msg_thread_seq_idx");
        let fields = owned(message::INDEX_FIELDS);
        assert_eq!(fields, vec!["thread_id", "seq"]);

        let seq = message::DEFAULT_SEQ;
        assert_eq!(seq, 0);
        let kind = message::DEFAULT_KIND;
        assert_eq!(kind, "");
        let display = message::DEFAULT_DISPLAY_CONTENT;
        assert_eq!(display, "");
        let turn_delete = message::TURN_ON_DELETE;
        assert_eq!(turn_delete, OnDelete::Cascade);
        let turn_nullable = message::TURN_NULLABLE;
        assert!(turn_nullable);
        let turn_related = message::TURN_RELATED_NAME;
        assert_eq!(turn_related, "messages");

        let fresh_a = message::default_payload();
        let mut fresh_b = message::default_payload();
        assert_eq!(fresh_a, serde_json::json!({}));
        fresh_b["injected"] = serde_json::json!(true);
        assert_eq!(message::default_payload(), serde_json::json!({}));

        let row = message::AssistantMessage {
            id: uuid::Uuid::nil(),
            thread_id: uuid::Uuid::nil(),
            turn_id: None,
            seq: 0,
            kind: MessageKind::User.as_str().to_string(),
            display_content: String::new(),
            payload: message::default_payload(),
            status: MessageStatus::Completed.as_str().to_string(),
            created_at: sample_time(),
            completed_at: None,
        };
        assert_row(&row, message::COLUMNS);
        assert_eq!(
            row.to_string(),
            format!("AssistantMessage({}, user)", uuid::Uuid::nil())
        );
        assert_eq!(
            select_sql(message::TABLE, message::COLUMNS),
            r#"SELECT "id", "thread_id", "turn_id", "seq", "kind", "display_content", "payload", "status", "created_at", "completed_at" FROM "assistant_message""#
        );
    }

    #[test]
    fn event_columns_match_fixture() {
        let fx = fixture_table("AssistantEvent");
        assert_eq!(fx["db_table"].as_str(), Some(event::TABLE));
        assert_eq!(fixture_ordering(&fx), owned(event::ORDERING));
        assert_eq!(fixture_columns(&fx), owned(event::COLUMNS));
        assert_eq!(event::COLUMNS.len(), 8);

        let index = event::INDEX_NAME;
        assert_eq!(index, "asst_event_thread_seq_idx");
        let fields = owned(event::INDEX_FIELDS);
        assert_eq!(fields, vec!["thread_id", "seq"]);

        let seq = event::DEFAULT_SEQ;
        assert_eq!(seq, 0);
        let kind_max: usize = event::KIND_MAX_LENGTH;
        assert_eq!(kind_max, 64);
        let turn_delete = event::TURN_ON_DELETE;
        assert_eq!(turn_delete, OnDelete::Cascade);
        let turn_related = event::TURN_RELATED_NAME;
        assert_eq!(turn_related, "events");

        let row = event::AssistantEvent {
            id: 1,
            thread_id: uuid::Uuid::nil(),
            turn_id: None,
            seq: 0,
            kind: "turn.started".to_string(),
            message_id: None,
            payload: event::default_payload(),
            created_at: sample_time(),
        };
        assert_row(&row, event::COLUMNS);
        assert_eq!(
            row.to_string(),
            format!("AssistantEvent({}, turn.started, seq=0)", uuid::Uuid::nil())
        );
        assert_eq!(
            select_sql(event::TABLE, event::COLUMNS),
            r#"SELECT "id", "thread_id", "turn_id", "seq", "kind", "message_id", "payload", "created_at" FROM "assistant_event""#
        );
    }

    #[test]
    fn mcp_server_columns_match_fixture() {
        let fx = fixture_table("AssistantMCPServer");
        assert_eq!(fx["db_table"].as_str(), Some(mcp::TABLE));
        assert_eq!(fixture_ordering(&fx), owned(mcp::ORDERING));
        assert_eq!(fixture_columns(&fx), owned(mcp::COLUMNS));
        assert_eq!(mcp::COLUMNS.len(), 8);

        let constraint = mcp::UNIQUE_CONSTRAINT;
        assert_eq!(constraint, "assistant_mcp_server_user_name_uniq");
        let unique = owned(mcp::UNIQUE_FIELDS);
        assert_eq!(unique, vec!["user_id", "name"]);

        let enabled = mcp::DEFAULT_IS_ENABLED;
        assert!(enabled);
        let name_max: usize = mcp::NAME_MAX_LENGTH;
        assert_eq!(name_max, 80);
        let url_max: usize = mcp::URL_MAX_LENGTH;
        assert_eq!(url_max, 500);
        let user_related = mcp::USER_RELATED_NAME;
        assert_eq!(user_related, "assistant_mcp_servers");

        let row = mcp::AssistantMCPServer {
            id: uuid::Uuid::nil(),
            user_id: uuid::Uuid::nil(),
            name: "github".to_string(),
            url: "https://mcp.example.invalid".to_string(),
            auth_header_encrypted: None,
            is_enabled: true,
            created_at: sample_time(),
            updated_at: sample_time(),
        };
        assert_row(&row, mcp::COLUMNS);
        assert_eq!(
            row.to_string(),
            format!("AssistantMCPServer({}, github)", uuid::Uuid::nil())
        );
        assert!(!mcp::has_auth_header(&row.auth_header_encrypted));
        assert_eq!(
            select_sql(mcp::TABLE, mcp::COLUMNS),
            r#"SELECT "id", "user_id", "name", "url", "auth_header_encrypted", "is_enabled", "created_at", "updated_at" FROM "assistant_mcp_server""#
        );
    }

    #[test]
    fn llm_config_columns_match_fixture() {
        let fx = fixture_table("UserLLMConfig");
        assert_eq!(fx["db_table"].as_str(), Some(llm::TABLE));
        assert_eq!(fixture_ordering(&fx), owned(llm::ORDERING));
        assert!(llm::ORDERING.is_empty());
        assert_eq!(fixture_columns(&fx), owned(llm::COLUMNS));
        assert_eq!(llm::COLUMNS.len(), 9);

        let unique = owned(llm::UNIQUE_COLUMNS);
        assert_eq!(unique, vec!["user_id"]);

        let base_url = llm::DEFAULT_BASE_URL;
        assert_eq!(base_url, "");
        let model_name = llm::DEFAULT_MODEL_NAME;
        assert_eq!(model_name, "");
        let provider_max: usize = llm::PROVIDER_KIND_MAX_LENGTH;
        assert_eq!(provider_max, 32);
        let user_related = llm::USER_RELATED_NAME;
        assert_eq!(user_related, "assistant_llm_config");

        let row = llm::UserLLMConfig {
            id: 1,
            user_id: uuid::Uuid::nil(),
            provider_kind: ProviderKind::OpenaiCompatible.as_str().to_string(),
            base_url: String::new(),
            model_name: String::new(),
            api_key_encrypted: None,
            last_verified_at: None,
            created_at: sample_time(),
            updated_at: sample_time(),
        };
        assert_row(&row, llm::COLUMNS);
        assert_eq!(
            row.to_string(),
            format!("UserLLMConfig({}, openai_compatible)", uuid::Uuid::nil())
        );
        assert_eq!(
            select_sql(llm::TABLE, llm::COLUMNS),
            r#"SELECT "id", "user_id", "provider_kind", "base_url", "model_name", "api_key_encrypted", "last_verified_at", "created_at", "updated_at" FROM "assistant_user_llm_config""#
        );
    }

    #[test]
    fn stt_config_columns_match_fixture() {
        let fx = fixture_table("UserSTTConfig");
        assert_eq!(fx["db_table"].as_str(), Some(stt::TABLE));
        assert_eq!(fixture_ordering(&fx), owned(stt::ORDERING));
        assert!(stt::ORDERING.is_empty());
        assert_eq!(fixture_columns(&fx), owned(stt::COLUMNS));
        assert_eq!(stt::COLUMNS.len(), 8);

        let unique = owned(stt::UNIQUE_COLUMNS);
        assert_eq!(unique, vec!["user_id"]);

        let base_max: usize = stt::BASE_URL_MAX_LENGTH;
        assert_eq!(base_max, 500);
        let model_max: usize = stt::MODEL_NAME_MAX_LENGTH;
        assert_eq!(model_max, 255);
        let user_related = stt::USER_RELATED_NAME;
        assert_eq!(user_related, "assistant_stt_config");

        let row = stt::UserSTTConfig {
            id: 1,
            user_id: uuid::Uuid::nil(),
            base_url: String::new(),
            model_name: String::new(),
            api_key_encrypted: None,
            last_verified_at: None,
            created_at: sample_time(),
            updated_at: sample_time(),
        };
        assert_row(&row, stt::COLUMNS);
        assert_eq!(
            row.to_string(),
            format!("UserSTTConfig({})", uuid::Uuid::nil())
        );
        assert_eq!(
            select_sql(stt::TABLE, stt::COLUMNS),
            r#"SELECT "id", "user_id", "base_url", "model_name", "api_key_encrypted", "last_verified_at", "created_at", "updated_at" FROM "assistant_user_stt_config""#
        );
    }

    #[test]
    fn secret_presence_is_truthiness_not_nullness() {
        let missing: Option<Vec<u8>> = None;
        assert!(!has_secret(&missing));
        let empty: Option<Vec<u8>> = Some(Vec::new());
        assert!(!has_secret(&empty));
        let set: Option<Vec<u8>> = Some(b"ciphertext".to_vec());
        assert!(has_secret(&set));

        assert!(!mcp::has_auth_header(&missing));
        assert!(!mcp::has_auth_header(&empty));
        assert!(mcp::has_auth_header(&set));
        assert!(!llm::has_api_key(&missing));
        assert!(!llm::has_api_key(&empty));
        assert!(llm::has_api_key(&set));
        assert!(!stt::has_api_key(&missing));
        assert!(!stt::has_api_key(&empty));
        assert!(stt::has_api_key(&set));
    }

    #[test]
    fn tool_prefix_slugs_and_falls_back() {
        let id = uuid::Uuid::nil();
        assert_eq!(mcp::tool_prefix("github", &id), "mcp_github");
        assert_eq!(mcp::tool_prefix("GPU Server", &id), "mcp_gpu_server");
        assert_eq!(
            mcp::tool_prefix("  Mixed CASE-Name! ", &id),
            "mcp_mixed_case_name"
        );
        assert_eq!(mcp::tool_prefix("a  b", &id), "mcp_a_b");
        assert_eq!(mcp::tool_prefix("_x_", &id), "mcp_x");
        assert_eq!(mcp::tool_prefix("a-b_c d", &id), "mcp_a_b_c_d");
        // Non-ASCII lowers to a separator, exactly like `[^a-z0-9]`
        // (live-probed: `"caf\u{e9} latt\u{e9}"` -> `mcp_caf_latt`).
        assert_eq!(
            mcp::tool_prefix("caf\u{e9} latt\u{e9}", &id),
            "mcp_caf_latt"
        );
        // No alphanumeric content: the reserved prefix can never be bare,
        // so the row id supplies a stable suffix.
        assert_eq!(mcp::tool_prefix("---", &id), "mcp_00000000");
        assert_eq!(mcp::tool_prefix("", &id), "mcp_00000000");
        assert_eq!(mcp::tool_prefix("   ", &id), "mcp_00000000");
        let prefixed = mcp::tool_prefix("linear", &id);
        assert!(prefixed.starts_with("mcp_"));
        assert!(!prefixed["mcp_".len()..].is_empty());
    }
}
