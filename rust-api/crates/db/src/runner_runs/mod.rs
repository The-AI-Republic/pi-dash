//! Runner runs models (D-15, stage 5).
//!
//! Ports the run/chat/dedupe models in
//! `apps/api/pi_dash/runner/models.py` plus the generated-column read
//! semantics of `runner/fields.py:21-61`, adopting the Django-owned
//! schema column-for-column; migrations are not ported — Django stays
//! schema owner until switchover.
//!
//! * [`pod`] — `Pod` (`models.py:52-177`): manager soft-delete scope,
//!   `clean`/`save` workspace denorm + autofill, `default_for_project_id`
//!   (`:173-176`), constraints/indexes. Pod *views* are D-13's; the
//!   model lives here per the epic and D-13's split reuses this module.
//! * [`agent_run`] — `AgentRun` (`models.py:872-1161`): all 41
//!   columns/defaults/constraints/indexes, `save` pod-resolution order
//!   + owner→created_by mirror (`:1104-1134`), token pseudo-fields as
//!     `(usage->>key)::bigint` reads (`fields.py:56-58`).
//! * [`event`] — `AgentRunEvent` (`models.py:1162-1176`).
//! * [`tool_call`] — `AgentRunToolCall` (`models.py:1187-1216`).
//! * [`approval`] — `ApprovalRequest` (`models.py:1219-1246`).
//! * [`run_dedupe`] — `RunMessageDedupe` (`models.py:788-808`).
//! * [`chat_session`] — `AgentChatSession` (`models.py:1249-1307`).
//! * [`chat_message`] — `AgentChatMessage` (`models.py:1310-1350`).
//! * [`chat_event`] — `AgentChatEvent` (`models.py:1353-1388`).
//! * [`chat_approval`] — `AgentChatApprovalRequest`
//!   (`models.py:1391-1430`).
//! * [`chat_dedupe`] — `ChatMessageDedupe` (`models.py:1433-1451`).
//! * [`live_state`] — `RunnerLiveState` read subset only
//!   (`models.py:1454-1527`): the six FX-RUN-03 columns plus the
//!   `runner_id` PK that attributes the read. The write path belongs
//!   to D-14's session service, which extends this module later.
//!
//! Fixture source of truth:
//! `rust-api/fixtures/runner_runs/fx-run-02-models-runs.golden.json`
//! (FX-RUN-02) and `fx-run-03-models-chat.golden.json` (FX-RUN-03),
//! recorded by PIDASHCONV-549; each file's `#[cfg(test)]` suite
//! replays its fixture section.
//!
//! # Reuse, not forks
//!
//! Choice columns use the L1 enums from
//! [`pidash_types::runner_runs`] (PIDASHCONV-527); `executor_kind`
//! reuses [`pidash_types::dispatch::AgentExecutorKind`] (PIDASHCONV-482)
//! like the D-11 read shape does; FK delete behavior reuses
//! [`crate::integrations::OnDelete`]; the live-state token properties
//! reuse L1 [`pidash_types::runner_runs::flat_token_fields`]. D-11
//! keeps its own `db::dispatch` read-shape copies of `AgentRun` /
//! `AgentRunEvent` / `AgentRunToolCall` / status enums (partial
//! projections for dispatch queries); this module is the
//! runner-domain full port the D-15 layers build on. No code here
//! calls into D-13/D-14.
//!
//! # Reads vs writes
//!
//! `Pod.default_for_project_id` is the only executing query in this
//! layer ([`pod::default_for_project_id`], runtime `sqlx::query` —
//! no `query!` macros: no build-time database, no offline cache,
//! following the `license/queries` precedent; Django's `%s`
//! placeholders render as Postgres `$N`). Everything else is pure:
//! column lists, defaults (all Django-side — the live tables carry no
//! `column_default`, so Rust inserts must supply these values
//! explicitly), save-rule decision functions over pre-fetched inputs,
//! and SQL text consts. Row structs carry no `FromRow` derive (there
//! is none in this crate); only [`pod`] maps rows, one `try_get` per
//! column in `COLUMNS` order like the `v1_cli_auth` precedent.

pub mod agent_run;
pub mod approval;
pub mod chat_approval;
pub mod chat_dedupe;
pub mod chat_event;
pub mod chat_message;
pub mod chat_session;
pub mod event;
pub mod live_state;
pub mod pod;
pub mod run_dedupe;
pub mod tool_call;

pub use agent_run::AgentRun;
pub use approval::ApprovalRequest;
pub use chat_approval::AgentChatApprovalRequest;
pub use chat_dedupe::ChatMessageDedupe;
pub use chat_event::AgentChatEvent;
pub use chat_message::AgentChatMessage;
pub use chat_session::AgentChatSession;
pub use event::AgentRunEvent;
pub use live_state::RunnerLiveState;
pub use pod::Pod;
pub use run_dedupe::RunMessageDedupe;
pub use tool_call::AgentRunToolCall;

#[cfg(test)]
pub(crate) mod test_support {
    use serde_json::Value;

    static FX02: &str =
        include_str!("../../../../fixtures/runner_runs/fx-run-02-models-runs.golden.json");
    static FX03: &str =
        include_str!("../../../../fixtures/runner_runs/fx-run-03-models-chat.golden.json");

    pub(crate) fn fx02() -> Value {
        serde_json::from_str(FX02).expect("FX-RUN-02 parses")
    }

    pub(crate) fn fx03() -> Value {
        serde_json::from_str(FX03).expect("FX-RUN-03 parses")
    }

    /// One `models[]` entry by Django model name.
    pub(crate) fn model<'a>(v: &'a Value, name: &str) -> &'a Value {
        v["models"]
            .as_array()
            .expect("models is an array")
            .iter()
            .find(|m| m["model"].as_str() == Some(name))
            .unwrap_or_else(|| panic!("model {name} in fixture"))
    }

    /// Physical columns in fixture order.
    pub(crate) fn columns(m: &Value) -> Vec<String> {
        m["fields"]
            .as_array()
            .expect("fields is an array")
            .iter()
            .map(|f| f["column"].as_str().expect("column").to_string())
            .collect()
    }

    /// One field entry by Django field name.
    pub(crate) fn field<'a>(m: &'a Value, name: &str) -> &'a Value {
        m["fields"]
            .as_array()
            .expect("fields is an array")
            .iter()
            .find(|f| f["name"].as_str() == Some(name))
            .unwrap_or_else(|| panic!("field {name} in fixture"))
    }

    /// One constraint entry by name.
    pub(crate) fn constraint<'a>(m: &'a Value, name: &str) -> &'a Value {
        m["constraints"]
            .as_array()
            .expect("constraints is an array")
            .iter()
            .find(|c| c["name"].as_str() == Some(name))
            .unwrap_or_else(|| panic!("constraint {name} in fixture"))
    }

    /// One index entry by name.
    pub(crate) fn index<'a>(m: &'a Value, name: &str) -> &'a Value {
        m["indexes"]
            .as_array()
            .expect("indexes is an array")
            .iter()
            .find(|i| i["name"].as_str() == Some(name))
            .unwrap_or_else(|| panic!("index {name} in fixture"))
    }

    /// Copy a const into an owned vec so assertions compare two
    /// runtime values (clippy denies asserting on constants directly).
    pub(crate) fn owned(cols: &[&str]) -> Vec<String> {
        cols.iter().map(|c| (*c).to_string()).collect()
    }
}
