//! Runner session models (D-14, stage 5).
//!
//! Ports `RunnerSession` (`apps/api/pi_dash/runner/models.py:688-720`)
//! and `MachineSession` (`:723-762`): columns, `db_table`
//! (`runner_session` / `machine_session`), `ordering =
//! ("-created_at",)`, the partial unique constraints and the indexes,
//! plus the session-row SQL this domain executes (open: prior
//! select-for-update + revoke update + insert; delete: get-or-404 +
//! revoke; poll: get + `last_seen_at` touch; active-session id
//! lookup).
//!
//! Fixture sources of truth, recorded by PIDASHCONV-546:
//! `rust-api/fixtures/runner_sessions/fx-rses-01-models.json`
//! (FX-RSES-01 column/constraint/index lists) and
//! `fx-rses-05-outbox-ops.json` (FX-RSES-05 `active_session_id_*`
//! SQL). Each file's `#[cfg(test)]` suite replays its fixture
//! section; the open/delete/poll statements are pinned against SQL
//! captured from Django 4.2.30 (see the capture notes on each
//! const).
//!
//! # Reads vs writes
//!
//! Everything here is pure: table/column/constraint/index consts,
//! Django-side defaults, row structs with manual `try_get` mapping
//! (no `FromRow` derive exists in this crate, following the
//! `v1_cli_auth` / `runner_runs::pod` precedent), and SQL text
//! consts in Django shape (quoted identifiers, `%s` params rendered
//! as Postgres `$N`). There are no executing queries: open's three
//! statements share one transaction owned by the handler
//! (PIDASHCONV-557/559), so the models layer hands over text and
//! stays out of the transaction.
//!
//! # FK / column contracts (docs only, no code)
//!
//! The tables below are owned by D-13 (PIDASHCONV-50:
//! `Runner`, `DevMachine`) and D-15 (PIDASHCONV-52: `AgentRun`,
//! `Pod`, `RunnerLiveState`, `AgentRunEvent`). D-14 SQL references
//! exactly these columns on them (full column lists live in FX-RSES-01
//! `fk_contract_column_lists`; Django field names below, `*_id`
//! attnames are the physical columns). The queries/handlers
//! sub-issues compile their statements against these names.
//!
//! ## `runner` (D-13)
//!
//! `id` (UUID pk), `owner_id` (UUID FK, `select_runner_for_run` /
//! `count_active` / `pod_has_runner_for_issue_principal` filters,
//! drain's `run.owner_id` source), `workspace_id` (UUID FK,
//! `select_runner_for_run` / `count_active` filters), `pod_id`
//! (UUID FK, pod-scoped matcher filters), `status` (varchar,
//! ONLINE/BUSY/OFFLINE filters + updates, REVOKED exclusion,
//! poll snapshot), `provisioning` (varchar, DESKTOP_BUNDLED
//! excludes/filters), `capabilities` (jsonb, conditional hello
//! update), `os` / `arch` / `runner_version` (varchar, hello
//! updates), `dev_metadata` (jsonb, hello update),
//! `last_heartbeat_at` (timestamptz null, freshness filters,
//! `-last_heartbeat_at` order, updates, poll snapshot),
//! `created_at` (timestamptz, second key of `Meta.ordering`
//! `["-last_heartbeat_at", "-created_at"]` for unordered
//! `.first()` calls). `runner_session.runner_id` targets
//! `runner.id` (`related_name="sessions"`, `CASCADE`). Not
//! referenced by D-14 SQL: `name`, `host_label`, `visibility`,
//! refresh/enrollment token columns, `enrolled_at`,
//! `protocol_version`, `free_worktrees` (explicitly retired,
//! `sessions.py` poll ignores the hint), `updated_at`,
//! `revoked_at` / `revoked_reason` (revocation is tested via
//! `status` only).
//!
//! ## `dev_machine` (D-13)
//!
//! `id` (UUID pk; `machine_session.dev_machine_id` target,
//! `related_name="sessions"`) and `last_seen_at` (timestamptz null;
//! machine open + poll touch via `filter(pk).update`). D-14 emits no
//! other predicate, projection, or ordering on this table
//! (`Meta.ordering` `["-last_seen_at", "-created_at"]` is never
//! exercised: both writes are keyed updates).
//!
//! ## `agent_run` (D-15)
//!
//! `id` (UUID pk; filters, excludes, `values_list`, assign payload),
//! `pod_id` (UUID FK; pod filters, `values_list`), `runner_id`
//! (UUID null; reaper/redeliver scope, assign update), `owner_id`
//! (UUID null; assign update), `pinned_runner_id` (UUID null;
//! `__isnull`, `Q`, pin-rank `CASE`), `status` (varchar; every
//! filter/update, resume_ack read), `executor_kind` (varchar;
//! `MACHINE_EXECUTORS` + managed/local splits), `assigned_at`
//! (timestamptz null; reaper cutoff, FIFO order, assign update),
//! `queue_position` (smallint null; cancel-barrier clear),
//! `created_at` (timestamptz; FIFO order and `Meta.ordering`
//! `["-created_at"]` for unordered `.first()`), `ended_at`
//! (timestamptz null; cancel-barrier update). Row-reads from fetched
//! rows (never predicates): `workspace_id` / `owner_id` as legacy
//! matcher filter values, `work_item_id` / `prompt` / `run_config` /
//! `thread_id` for the assign / resume_ack payloads. `is_terminal`
//! is a Python property over `status`, not SQL. The reaper's
//! `error` / `error_code` writes go through D-15's
//! `finalize_agent_run` (indirect; that service owns the SQL).
//!
//! ## `pod` (D-15 model)
//!
//! `id` (UUID pk; `drain_pod_by_id` lookup, matcher join target),
//! `project_id` (UUID FK; `select_related("pod__project")` join for
//! `resolve_runner_project_slug`), `deleted_at` (timestamptz null;
//! `Pod.objects` manager scope on the `drain_pod_by_id` lookup),
//! `is_default` + `created_at` (`Meta.ordering` `["-is_default",
//! "created_at"]` for that lookup's unordered `.first()`).
//!
//! ## `runner_live_state` (D-15 model, D-14 writes)
//!
//! `runner_id` (UUID pk, OneToOne; `get_or_create`), `observed_run_id`
//! (UUID null; wipe driver + update), the eight `SNAPSHOT_FIELDS`
//! (`last_event_at`, `last_event_kind`, `last_event_summary`,
//! `agent_pid`, `agent_subprocess_alive`, `approvals_pending`,
//! `llm_model`, `turn_count`; wipe + conditional updates), `usage`
//! (jsonb; wipe + `tokens` update), `updated_at` (appended to every
//! upsert's `update_fields`). No `Meta.ordering`. D-15's
//! `db::runner_runs::live_state` ports the read subset; the upsert
//! write path is this domain's (PIDASHCONV-556).
//!
//! ## `agent_run_event` (D-15)
//!
//! `agent_run_id` (UUID FK filter) + `seq` (`-seq` order,
//! `values_list`, resume_ack `last_seq`). `id`, `kind`, `payload`,
//! `created_at` are never referenced (`Meta.ordering`
//! `["agent_run", "seq"]` is overridden by the explicit `-seq`).
//!
//! Other cross-domain reads (pointers, owned elsewhere):
//! `Project.identifier` (via the `pod__project` join),
//! `IssueAssignee.assignee_id` (matcher preflight through-table
//! read), `MachineToken.dev_machine_id` (machine auth, D-13).
//!
//! # Sibling ownership (not here)
//!
//! * `close_runner_session`'s active-list select (`pubsub.py:96-99`,
//!   full-row, default ordering, no `LIMIT`) is PIDASHCONV-553's;
//!   its per-row revoke reuses [`models::runner_session::REVOKE_SQL`].
//! * The `DevMachine.last_seen_at` keyed updates
//!   (`machine_sessions.py:106-108`, `:201`) execute in the machine
//!   handlers (PIDASHCONV-559) against the D-13 table.
//! * `Runner` snapshot/heartbeat writes on the poll path
//!   (`sessions.py:376-397`) execute in the runner poll handler
//!   (PIDASHCONV-558); hello/reaper/live-state/redeliver SQL is
//!   PIDASHCONV-556's; matcher SQL is PIDASHCONV-552/555's.
//!
//! Ported bugs: none found in these two models on read-through.
//! Two open-path asymmetries observed while capturing (handler-owned,
//! listed in the PR, not this layer's to fix): runner open alone runs
//! `_bound_txn_waits` and maps `OperationalError` to 503
//! (`sessions.py:172-213`; machine open has neither), and machine
//! open calls `timezone.now()` twice (session row + `DevMachine`
//! touch get distinct timestamps, `machine_sessions.py:104-108`).
//!
//! [`outbox`] ports the per-runner Redis Streams verbs
//! (`services/outbox.py:139-737`) on top of these models and the
//! `types::runner_sessions` shapes (PIDASHCONV-550).
//!
//! [`machine_outbox`] ports the per-dev-machine twin
//! (`services/machine_outbox.py:111-393`): group mgmt, enqueue,
//! drain, read/ack, PEL markers, eviction, command results and
//! stream delete — no claim/reap/trim, those have no machine side
//! (PIDASHCONV-551).

pub mod machine_outbox;
pub mod models;
pub mod outbox;

pub use models::machine_session::MachineSession;
pub use models::runner_session::RunnerSession;

#[cfg(test)]
pub(crate) mod test_support {
    use serde_json::Value;

    static FX01: &str = include_str!("../../../../fixtures/runner_sessions/fx-rses-01-models.json");
    static FX05: &str =
        include_str!("../../../../fixtures/runner_sessions/fx-rses-05-outbox-ops.json");

    pub(crate) fn fx01() -> Value {
        serde_json::from_str(FX01).expect("FX-RSES-01 parses")
    }

    pub(crate) fn fx05() -> Value {
        serde_json::from_str(FX05).expect("FX-RSES-05 parses")
    }

    /// One `owned` model entry by Django model name.
    pub(crate) fn owned<'a>(v: &'a Value, name: &str) -> &'a Value {
        &v["owned"][name]
    }

    /// `COLUMNS`-shaped physical column list for comparison.
    pub(crate) fn owned_cols(columns: &[&str]) -> Vec<String> {
        columns.iter().map(ToString::to_string).collect()
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

    /// Replace the single `'...'::uuid` literal in fixture SQL with `$1`.
    pub(crate) fn dollarize_param(sql: &str) -> String {
        let start = sql.find('\'').expect("fixture SQL carries a literal");
        let tail = &sql[start..];
        let end = tail.find("::uuid").expect("uuid-cast literal") + "::uuid".len();
        format!("{}${}{}", &sql[..start], 1, &sql[start + end..])
    }
}
