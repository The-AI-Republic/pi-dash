//! Runner runs L1 types: enums + pure helpers + shared consts (D-15, stage 5).
//!
//! Ports the exception-free bottom of the runner-runs domain:
//!
//! * [`enums`] — the nine `TextChoices` (`models.py:207-330,1178-1186`),
//!   `TERMINAL_RUN_STATUSES`, `is_terminal`/`is_active`, and the trigger
//!   sets (`HUMAN_TRIGGERS`, `AUTOMATIC_ISSUE_TRIGGERS`,
//!   `run_is_human_triggered`).
//! * [`usage`] — `services/usage.py:1-173` (`coerce_token`,
//!   `normalize_usage`, `merge_usage`, `flat_token_fields`).
//! * [`diagnostics`] — `diagnostics.py:1-245` (`infer_agent_label`,
//!   `enrich_run_error`, `classify_run_error`) plus the serializer's
//!   list-`None` contract (`serializers.py:266-272`).
//! * [`consts`] — pagination, dedupe TTL, payload caps, chat timeout, SSE
//!   channel prefix, stream-ticket TTL, metrics active-status set.
//!
//! D-14's session service reuses [`usage`]/[`diagnostics`] from here; L2
//! models reuse [`enums`]. Do not fork copies into other crates.
//!
//! Fixture id replayed by the unit tests alongside each module:
//! `rust-api/fixtures/runner_runs/fx-run-01-types-pure.golden.json`
//! (FX-RUN-01).

pub mod consts;
pub mod diagnostics;
pub mod enums;
pub mod usage;

pub use consts::{
    event_channel, run_message_dedupe_ttl_secs, ws_upgrade_ticket_key, ACTIVE_RUN_STATUSES,
    CHAT_ACTIVE_TIMEOUT_SECS, CHAT_EVENT_CHANNEL_PREFIX, CHAT_EVENT_PAYLOAD_MAX_BYTES,
    DEFAULT_PER_PAGE, MAX_EVENT_PAYLOAD_BYTES, MAX_PER_PAGE, RUN_MESSAGE_DEDUPE_TTL_SECS_DEFAULT,
    WS_UPGRADE_TICKET_EXPIRES_IN_SECS, WS_UPGRADE_TICKET_KEY_PREFIX,
    WS_UPGRADE_TICKET_PAYLOAD_KEYS, WS_UPGRADE_TICKET_TTL_SECS,
};
pub use diagnostics::{
    classify_run_error, enrich_run_error, error_diagnostic, infer_agent_label, DevMachineInfo,
    RunErrorDiagnostic, RunErrorKind, RunErrorSource, RunnerInfo,
};
pub use enums::{
    run_is_human_triggered, AgentChatMessageRole, AgentChatMessageStatus, AgentChatSessionStatus,
    AgentRunStatus, AgentRunTrigger, ApprovalKind, ApprovalStatus, RefusalCategory, ToolCallStatus,
    AUTOMATIC_ISSUE_TRIGGERS, HUMAN_TRIGGERS, TERMINAL_RUN_STATUSES,
};
pub use usage::{
    coerce_token, flat_token_fields, merge_usage, normalize_usage, FlatTokenFields, BIGINT_MAX,
    CANONICAL_USAGE_KEYS,
};
