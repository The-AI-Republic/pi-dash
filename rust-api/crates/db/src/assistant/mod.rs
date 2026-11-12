//! Assistant domain tables (D-06, stage 5).
//!
//! Ports `apps/api/pi_dash/assistant/models.py:1-293` for the db layer:
//!
//! * [`models`] — `ThreadKind`, `AssistantThread`, `TurnStatus`,
//!   `AssistantTurn`, `MessageKind`, `MessageStatus`, `AssistantMessage`,
//!   `AssistantEvent`, `ProviderKind`, `AssistantMCPServer`,
//!   `UserLLMConfig`, `UserSTTConfig` (PIDASHCONV-247, struct +
//!   column/constraint mapping only). Serializers and errors live in
//!   `pidash-types::assistant` (PIDASHCONV-246); crypto, SSRF, queries,
//!   and runtime belong to later D-06 layer issues.
//!
//! * [`event_queries`] — `runtime/events.py:1-145` + `runtime/history.py:1-60`
//!   (PIDASHCONV-248, SQL builders + wire shapes over [`models`]).
//! * [`fernet_keys`] — multi-key Fernet primitive backing the services
//!   crypto backend (PIDASHCONV-248; the `fernet` dependency lives here).

pub mod event_queries;
pub mod fernet_keys;
pub mod models;

pub use models::{
    assistant_event::AssistantEvent, assistant_mcp_server::AssistantMCPServer,
    assistant_message::AssistantMessage, assistant_thread::AssistantThread,
    assistant_turn::AssistantTurn, user_llm_config::UserLLMConfig, user_stt_config::UserSTTConfig,
    MessageKind, MessageStatus, OnDelete, ProviderKind, ThreadKind, TurnStatus,
};
