# Design: one shared tool core under MCP and the cloud agent tools

- **Status:** draft for implementation, 2026-11-11
- **Scope:** Rust backend only (`rust-api/` here, plus the MCP module in the cloud overlay's `rust-cloud/`)
- **Code baseline:** pi-dash `main` at `1e706002e`; cloud overlay `main` at `3baf8de0`. Paths are relative to `rust-api/crates/` unless they start with another root.
- **Decision already taken:** keep two tool surfaces (MCP, and the cloud agent's in-process tools) and make both thin wrappers over the same backend functions. The alternative, having the cloud agent call the MCP server, was rejected (section 2).

Every "today" statement was read from the code at the baseline. Nothing was run.

## 1. Summary

The MCP connector and the cloud agent both read and write Pi Dash issues, comments, workpads and relations, but they share almost no code above the row-level domain writes. The cloud agent re-implements each action in `jobs/src/oss_worker/model_runtime/tools_exec.rs`; MCP calls the `operations` layer in `services/src/operations/`, which the `/api/v1` handlers also use.

The agent cannot simply call `operations` as it stands, for one hard reason: every operations function opens and commits its own database transaction, and every agent write must commit in the same transaction as its audit ledger row.

The plan adds a **tool core**: one typed function per business action that runs on a transaction the caller supplies. The existing operations functions and the agent's tool executors both become wrappers around it.

| Phase | Work | Behaviour change |
| --- | --- | --- |
| 0 | Characterization tests that pin each agent write tool's rows, result and side effects | None |
| 1 | One `run_write` wrapper for agent writes, built on `WriteTxn`; ledger statements move onto it | None |
| 2 | Extract the core, one action at a time; `operations::*` and the agent executors call it | None |
| 3 | Shared read functions for the tools both surfaces expose | None |
| 4 | New agent tools as thin wrappers (edit the current issue, create from an issue run) | New capability, needs a decision |
| 5 | Move the assistant chat tools onto the core; converge accidental differences | Per decision |

Phases 0 to 3 are a pure refactor: the existing contract suites must stay green with no change on the Django side.

## 2. Why not have the agent call MCP

- **Dependency direction.** The cloud agent is open source; the MCP server exists only in the cloud overlay (there is no MCP server in `rust-api/`, only the assistant's MCP client). Open source cannot call into the overlay.
- **Atomic audit.** An agent write and its `agent_run_tool_call` row commit together today. An HTTP hop to MCP makes that two transactions.
- **Credentials.** MCP authenticates with a user API token. The agent would need a token minted per run.
- **Blast radius.** A run is confined to one project, three writes and its own issue. MCP's 30 tools are workspace-wide and include moving issues and managing schedulers. Issue text and GitHub content are untrusted model input.
- **Prompt cost.** Thirty tool definitions on every model request instead of fifteen.

## 3. Current state

### 3.1 Four callers, three stacks

| Caller | Where | Calls | Transaction |
| --- | --- | --- | --- |
| `/api/v1` handlers (used by the `pidash` CLI) | `api/src/v1_work_items/handlers_*.rs` | `operations::*` | Owned by operations |
| MCP tools (30) | overlay `rust-cloud/crates/cloud-api/src/mcp/tools_*.rs` | `sdk::operations::*` for writes; `sdk::read::*` and local SQL for reads | Owned by operations |
| Cloud agent tools (15) | `jobs/src/oss_worker/model_runtime/tools_exec.rs` | `domain::*` row writes and inline queries | Owned by the agent's audit wrapper |
| Assistant chat tools (18) | `api/src/turn/tools_local.rs`, `tools_local_store.rs` | `domain::*` row writes and own SQL | Owned by the chat tool store |

Nothing in `jobs/` or `api/src/turn/` calls `operations`.

### 3.2 What `operations` is

`services/src/operations/mod.rs:1-18` describes it as "the serializer step of the v1 write endpoints": validate the v1 payload, apply defaults, call the domain write, render the response.

- **Functions:** `issues::{create, update, move_to_project}`, `comments::{create, update}`, `relations::{relate, unrelate, fetch_source, grouped_relations}`, `workpad::update`, `pull_requests::attach`.
- **Input:** `&Deps` (pool, orm, transition hook, enqueue hook), `&WriteCtx` (actor, origin, scope, empty options) and a v1 JSON body as `&Value`.
- **Output:** `Outcome { status: u16, body: String, drain_pod }` or `Error` with DRF-shaped bodies. Results are pre-rendered JSON strings.
- **Auth:** none. The caller authenticates and checks roles first.
- **Reads:** none, apart from the two relation helpers.

Transaction ownership, which is the blocker:

| Function | How it writes |
| --- | --- |
| `issues::create`, `issues::update`, `issues::move_to_project`, `workpad::update` | `domain::run(ctx, &deps.orm, …)`, which calls `WriteTxn::begin`, commits, then drains the after-commit queue. `apply_execution_target` runs in a second transaction afterwards |
| `comments::create`, `comments::update` | `pool.begin()`; the `created_at` restamp then runs outside that transaction |
| `relations::relate`, `relations::unrelate` | No transaction: each insert or soft-delete autocommits on a pool connection |
| `pull_requests::attach` | Pool store writes inside `block_in_place` |

`WriteTxn` (`services/src/domain/txn.rs`) can only be created by `begin`; it cannot adopt an existing transaction. It carries an after-commit queue and a hook registry, and deliberately forbids savepoints.

### 3.3 How an agent write works today

There is no shared write wrapper. Each write tool's arm in `dispatch_fn` (`tools_exec.rs:3619`) repeats the same sequence:

1. Tool-side validators (length caps, UUID parsing). A failure here leaves no ledger row.
2. `write_begin` (`tools_exec.rs:602`): kill switches, replay check, then `Transaction::begin(&ctx.pool)` (a `pidash_db::tx::Transaction`, the sqlx wrapper).
3. Inside that transaction: `load_scope` (creator active, not a bot, workspace member, project role admin or member, run not cancelled), write admission (write limit, single-use rule), ledger insert as `prepared` then `submitted`.
4. The tool's own write, on the same transaction through `TxConn`.
5. `write_commit_tx` (`tools_exec.rs:723`): bound the result, store the replay value, mark the ledger row `succeeded`, commit.
6. After commit: append the `tool_completed` event.

An error at step 4 rolls back the write and the ledger row together.

Two tools already break the single-transaction rule:

- **Create issue** commits the issue in a separate `domain::run` before the audit commits (`tools_exec.rs:1527-1540`), because `domain::issues::create` needs a `WriteTxn`.
- **Transition** keeps its `FOR UPDATE` lock in the audit transaction and cannot use a domain transaction without deadlocking (`tools_exec.rs:2490`).

Both are symptoms of the audit wrapper and the domain layer using different transaction types.

### 3.4 Where the same action is implemented twice

| Action | Agent implementation | Operations implementation | Shared today |
| --- | --- | --- | --- |
| Add comment | `op_add_current_issue_comment` (`tools_exec.rs:1193`) | `comments::create` (`comments.rs:279`) | The three row writers in `domain::comments` |
| Update workpad | `op_update_current_issue_workpad` (`:1323`) → `domain::issues::update_workpad` (bare update) | `workpad::update` (`workpad.rs:46`) → `domain::issues::set_workpad` | Nothing: two different domain functions |
| Change state | `op_transition_current_issue` (`:2410`) → `update_issue_state` + explicit `handle_issue_state_transition` | `issues::update` (`issues.rs:3668`) → `partial_update` + transition hook | The orchestration entry, reached two ways |
| Create issue | `op_create_project_issue` (`:1435`) → `domain::issues::create` with `skip_fire` + bespoke post-commit fire | `issues::create` (`issues.rs:3405`) → `domain::issues::create` with the in-chain fire | `domain::issues::create` |
| Relate / unrelate | `op_relate_issues` (`:2890`), `op_unrelate_issues` (`:3041`) | `relations::relate` (`relations.rs:1384`), `unrelate` (`:1406`) | Verdict helpers in `rel::*`; the batch logic is a duplicate port |
| Grouped relations (read) | `grouped_tx` (`:3112`) | `relations::grouped_relations` (`:761`) | Same logic, ported twice |

### 3.5 Behaviour that differs between the two today

These are the differences a naive "just call operations" change would introduce. Phases 0 to 3 must preserve the agent column exactly.

| Action | Cloud agent | Operations (as MCP calls it) |
| --- | --- | --- |
| Comment body | Markdown, converted with `assistant::markdown::to_safe_html`; capped at 10,000 characters | HTML as given; MCP escapes plain text into `<p>`; no cap |
| Comment attribution | `speaker_type="agent"`, `speaker_label="Pi Dash Cloud Agent"`, `speaker_agent_run_id=run`; actor is the run creator | Speaker fields read from the request body (MCP sends label `"MCP"`); body can also override `created_by` |
| Comment side effects | None | `issue_activity` and webhook, each behind an option (MCP turns both off and emits its own activity) |
| Workpad | Bare update of `workpad` and `updated_at`; capped at 32,000 characters; a missing issue still answers success | Sparse save that sets `updated_by` and runs the completion chain; no cap; missing issue is `NotFound` |
| State change | Three bare columns; same-state is a no-op; fire runs with `moved_by_run` and a preflight that fails closed, so an agent move never dispatches | Full-row partial update; no same-state shortcut; fire runs through the transition hook with `FireSpec.moved_by_run` |
| Create | Scheduler's project only; `created_via="cloud_agent"`; no default assignee; fire runs after the audit commit with the live preflight | Any project; default assignee applied; fire runs in-chain |
| Relations input | `related_issues`, 1 to 50; UUID or `PROJ-123`; source narrowed to the run's project | `issues`; no cap; source pre-fetched by UUID |
| Relations activity | Enqueued inside the transaction, rolled back with it | Enqueued best-effort after each autocommit |
| Error text | Python `str(exc)` strings; "issues not found or not accessible" | DRF bodies; "work items not found or not accessible" |
| Result | Small dict, e.g. `{created, comment_id}` | Full v1 render |

## 4. Target design

### 4.1 Layers

```
 v1 handlers        MCP tools          cloud agent tools      assistant chat tools
 (HTTP, auth)       (token auth,       (run scope, limits,    (thread scope)
      |              workspace scope)   audit ledger)               |
      |                  |                    |                     |
      +---- operations::* (v1 body in, ------+                     |
            rendered v1 body out;            |                     |
            opens the transaction)           |                     |
                     |                       |                     |
                     +-----------+-----------+---------------------+
                                 |
                     tool core: one typed function per action,
                     on a caller-supplied WriteTxn
                                 |
                     domain::* row writes, hooks, after-commit queue
```

- **Core** owns the business action: which rows are written, in what order, with which hooks and after-commit effects.
- **Wrappers** own everything specific to their surface: authentication, scope, input format, validation limits, result shape, and who opens the transaction.

### 4.2 The core module

New module `services/src/operations/core/` (re-exported through `pidash_sdk::operations::core`), with one file per entity: `comments.rs`, `workpad.rs`, `issues.rs`, `relations.rs`.

Rules for every core function:

1. **Runs on the caller's transaction.** The first parameter is `txn: &mut WriteTxn`. It never begins, commits or rolls back.
2. **Typed in, typed out.** No JSON bodies, no HTTP statuses, no rendered strings. Inputs are structs of already-validated values; outputs are structs with the ids and fields a wrapper needs to render its own result.
3. **No authentication and no scope lookup.** The caller has already decided the actor may do this to this row. Core functions take explicit ids.
4. **Side effects go on the after-commit queue.** Activity, webhook and transition effects are queued with `txn.on_commit`, so they run only if the caller commits.
5. **Options exist only for documented product differences.** Each option names a row in the table in section 3.5. No option exists to reproduce an accident.
6. **Errors are a typed enum** (`CoreError`), which each wrapper maps to its own surface: DRF bodies for operations, Python-style strings for the agent.

Shared types:

```rust
/// Who is acting and on whose behalf. Replaces speaker fields and
/// run markers travelling through untrusted request bodies.
pub enum WriteSource {
    Api,
    Mcp,
    CloudAgent { run_id: Uuid },
    Assistant { thread_id: Uuid },
}

pub struct Speaker {
    pub speaker_type: SpeakerType,   // human | agent | system | integration
    pub label: String,
    pub agent_run_id: Option<Uuid>,
}

pub struct Effects {
    pub activity: bool,        // issue_activity job
    pub model_activity: bool,  // webhook
}
```

`WriteSource` is added to `WriteCtx.options` (`domain/ctx.rs:31`, today an empty struct). It gives every write an attributable origin without reading it from a payload.

### 4.3 Core functions and their wrappers

| Core function | Replaces in operations | Replaces in the agent | MCP tool | Agent tool |
| --- | --- | --- | --- | --- |
| `core::comments::create(txn, ctx, CommentCreate { issue_id, project_id, workspace_id, html, speaker, access, external }, effects) -> CommentCreated { id, created_at }` | Body of `comments::create` between validation and render | `op_add_current_issue_comment` after the bound-issue lookup | `pidash_comment_issue` | `pidash_add_current_issue_comment` |
| `core::comments::update(txn, ctx, CommentUpdate { … }, effects)` | Body of `comments::update` | none | `pidash_update_comment` | none |
| `core::workpad::set(txn, ctx, WorkpadSet { issue_id, project_id, body, mode }) -> WorkpadSaved { updated_at }` | `workpad::update` after the probe | `op_update_current_issue_workpad` | `pidash_update_workpad` | `pidash_update_current_issue_workpad` |
| `core::issues::set_state(txn, ctx, StateChange { issue_id, state_id, moved_by_run, dispatch }) -> StateChanged { changed, from, to }` | The state branch of `issues::update` | `op_transition_current_issue` | `pidash_update_issue` (state field) | `pidash_transition_current_issue` |
| `core::issues::update(txn, ctx, IssuePatch { … typed fields … }, effects) -> IssueUpdated` | Body of `issues::update` | none today (phase 4) | `pidash_update_issue` | new `pidash_update_current_issue` |
| `core::issues::create(txn, ctx, IssueCreate { project_id, name, description_html, state, created_via, apply_default_assignee, fire }, effects) -> IssueCreated` | Body of `issues::create` | `op_create_project_issue` | `pidash_create_issue` | `pidash_create_project_issue` |
| `core::relations::relate(txn, ctx, RelationBatch { source, relation_type, targets, pair_exact }, effects) -> RelateResult { created, unchanged, conflicts }` | `relations::relate` | `op_relate_issues` | `pidash_relate_issues` | `pidash_relate_issues` |
| `core::relations::unrelate(txn, ctx, RelationBatch { …, newest_only }, effects) -> UnrelateResult { removed, not_related }` | `relations::unrelate` | `op_unrelate_issues` | `pidash_unrelate_issues` | `pidash_unrelate_issues` |

Options that encode a real product difference (rule 5):

| Option | Values | Why it exists |
| --- | --- | --- |
| `WorkpadSet.mode` | `Sparse` (operations: stamps `updated_by`, runs the completion chain) or `Bare` (agent: two columns) | Section 3.5, workpad. Marked for convergence in phase 5 |
| `StateChange.dispatch` | `InChain` or `NeverDispatch` | An agent move must queue or park, never dispatch (`agent_moves_never_dispatch_now`, `tools_exec.rs:4295`) |
| `IssueCreate.fire` | `InChain` or `AfterCommit(preflight)` | The agent fires after its audit commits, with the live preflight (pinned by `test_create_project_issue_transition.py`) |
| `IssueCreate.apply_default_assignee` | bool | The agent does not apply the project default assignee |
| `Effects` | per call | The agent emits no activity for comment, create and transition; operations callers choose |

`pull_requests::attach` and `issues::move_to_project` stay as they are. The agent has no equivalent tool, and MCP bypasses the first and cannot reach the second (BUG-950-1, BUG-942-4 are ported Django bugs).

### 4.4 The transaction bridge

The agent's audit wrapper moves from `pidash_db::tx::Transaction` to `WriteTxn`, so the ledger statements, the scope check and the core call all run on one transaction of the type the domain layer already requires.

New wrapper in `tools_exec.rs`, replacing the six open-coded write arms:

```rust
async fn run_write<T>(
    ctx: &ToolCtx,
    run_id: Uuid,
    tool: &'static str,
    args: &Value,
    op: impl for<'t> AsyncFnOnce(&'t mut WriteTxn, &ToolScope) -> Result<T, ToolExecError>,
    shape: impl FnOnce(&T) -> Value,
) -> Result<Value, ToolExecError>
```

Inside one `domain::run`:

1. Kill switches and the replay check (unchanged, before the transaction).
2. `load_scope` on `txn.conn()` with `write = true`.
3. Write admission on `txn.conn()`.
4. Ledger insert `prepared`, then `submitted`.
5. `op(txn, &scope)`, which calls the core function.
6. Bound the shaped result, store the replay value, mark the ledger row `succeeded`.
7. Return; `domain::run` commits and drains the after-commit queue.
8. Append `tool_completed` (queued with `txn.on_commit`, so it still runs after the commit).

Consequences:

- **Create issue** joins the audit transaction. The separate early commit at `tools_exec.rs:1527` goes away; the post-commit fire becomes an after-commit step.
- **Transition** takes its row lock on the same `WriteTxn` the core function writes through, so the deadlock noted at `tools_exec.rs:2490` cannot occur.
- **Relate** can no longer use `SAVEPOINT` plus retry (`relate_insert_tx`): `WriteTxn` forbids savepoints. It uses `domain::relations::relate_row_or_nothing`, as operations does.
- **Raw sqlx statements** that run on the old transaction (`bound_issue_sql` and similar) move to SeaORM on `txn.conn()`.

The operations side needs no bridge: each operations function keeps its signature and becomes "parse the v1 body, `domain::run`, call core, render".

- `comments::*` moves from `pool.begin()` to `domain::run`. The out-of-transaction `created_at` restamp moves inside.
- `relations::*` moves from per-row autocommit to one `domain::run` per call. This makes a batch atomic, which is a behaviour change for `/api/v1` and MCP on partial failure. Phase 2 keeps today's behaviour by committing per target (one `domain::run` each) and leaves batch atomicity as a decision in section 8.

### 4.5 What stays in each wrapper

| Concern | v1 handler / operations | MCP | Cloud agent |
| --- | --- | --- | --- |
| Authentication | Session or API token | API token, machine token or session JWT (`mcp/auth.rs`) | The run: creator re-checked by `load_scope` on every call |
| Scope | Route params | `workspace_slug`, then `resolve_project` / `resolve_issue` (`mcp/resolve.rs`) | Fixed by the run: bound issue, scheduler project or pod project |
| Role gate | Handler preamble | `ensure_can_write_project`, `ensure_project_member` | `project_role_ok` in `load_scope` |
| Input format | v1 JSON body | Tool arguments, turned into a v1 body | Tool arguments, validated against the golden schema |
| Limits | DRF validation | List limit clamp | Comment 10,000, workpad 32,000, title 255, description 20,000, relations 1 to 50, search 50; write budget and single-use rule |
| Markdown | `description_markdown` converted for issues only | Plain text escaped into `<p>` | `to_safe_html` for comment and description |
| Audit | Activity rows | Own activity emission (`mcp/tool_io.rs`) | `agent_run_tool_call` ledger and `tool_completed` event |
| Result | Rendered v1 body | v1 body, pruned, as MCP content | Small dict, bounded and replay-collapsed |
| Errors | DRF bodies | `isError` or `-32000` | Python-style strings; the ledger `error_code` is the Python type name |

### 4.6 Shared reads

Reads have no transaction or audit coupling, so they are simpler. New module `services/src/operations/read/` with typed functions that both surfaces call:

| Function | Agent tool | MCP tool | Today |
| --- | --- | --- | --- |
| `issue_by_id(conn, id, project) -> IssueView` | `pidash_get_current_issue`, `pidash_get_project_issue` | `pidash_get_issue` | `dispatch::tool_issues::*` vs `sdk::read::fetch_issue_for_write` |
| `search_issues(conn, scope, query, limit) -> Vec<IssueView>` | `pidash_search_project_issues` | `pidash_search_issues`, `pidash_list_issues` | `tool_issues::search_project_issues` vs `sdk::read::search_issues` |
| `list_comments(conn, issue, order, limit)` | `pidash_list_current_issue_comments` | `pidash_list_comments` | Inline SeaORM vs `sdk::read::fetch_comment_list_ascending` |
| `list_states(conn, project)` | `pidash_list_project_states` | `pidash_list_states` | Inline SeaORM vs `sdk::read::fetch_state_columns` |
| `grouped_relations(conn, source, visible_to)` | `pidash_list_issue_relations` | `pidash_list_issue_relations` | Ported twice (`tools_exec.rs:3112`, `relations.rs:761`, and again in `mcp/resolve.rs:979`) |
| `linked_reviews(conn, issue, limit)` | `pidash_list_linked_code_reviews` | `pidash_list_github_pull_requests` | Inline SeaORM vs `sdk::read::fetch_pr_link_list` |

Each returns a typed view with the union of fields both wrappers need. The agent shapes its capped dict (`plan::issue_data`, 20,000-character description, 32,000-character workpad); MCP renders the v1 shape.

Differences the read functions must take as parameters, not hide: the agent's search matches title and description with a 50-row cap inside one project; MCP's search is workspace-wide. The agent's `bound_issue` lookup has no `deleted_at` filter.

## 5. Implementation plan

### Phase 0: pin the agent's behaviour

The contract suites pin the agent through the Django oracle, but mostly at run level. Before moving code, add Rust integration tests (live Postgres, like the existing worker tests) for each of the six write tools:

- rows written, with exact column values (attribution, `created_via`, `updated_by` unset on workpad);
- result JSON keys and values;
- jobs enqueued: none for comment, workpad, create and transition; `issue_activity` for relate and unrelate;
- ledger row states, and that an injected failure after the write leaves neither the write nor the ledger row;
- the same-state transition no-op and the missing-issue workpad success.

Location: `jobs/src/oss_worker/model_runtime/tools_exec.rs` test module, or a new `tools_exec_tests.rs` beside it.

Done when: the tests pass on `main` unchanged, and each fails if its tool's behaviour is altered.

### Phase 1: the transaction bridge

| # | Change | File |
| --- | --- | --- |
| 1.1 | Add `WriteSource` to `WriteOptions`; default `Api` | `services/src/domain/ctx.rs:31` |
| 1.2 | Add `run_write` (section 4.4) built on `domain::run` | `tools_exec.rs`, beside `run_read` (`:3597`) |
| 1.3 | Move `load_scope`, write admission and the ledger statements to `txn.conn()` | `tools_exec.rs:440-533`, `:723` |
| 1.4 | Convert the write arms one at a time to `run_write`, keeping each `op_*` body calling the same domain functions as today | `tools_exec.rs:3722-4071` |
| 1.5 | Create issue: run `domain::issues::create` on the audit `WriteTxn`; turn `fire_create_transition` into an after-commit step | `tools_exec.rs:1435`, `:2200` |
| 1.6 | Transition: take the lock and call `handle_issue_state_transition` on the audit `WriteTxn` | `tools_exec.rs:2410`, `:2547` |
| 1.7 | Relate: replace the savepoint insert with `relate_row_or_nothing` | `tools_exec.rs:2890` |

Order within the phase: workpad, comment, relate, unrelate, transition, create. The first two are the simplest and prove the wrapper.

Done when: the phase 0 tests and the dispatch contract suite (`rust-api/contract-tests/dispatch/`) pass unchanged, and no write arm opens a `pidash_db::tx::Transaction`.

### Phase 2: extract the core

One pull request per entity, in this order. Each follows the same four steps: write the typed core function by lifting the middle of the operations function; make the operations function call it; make the agent `op_*` call it; delete the agent's private copy.

| Order | Entity | Notes |
| --- | --- | --- |
| 2.1 | Workpad | Two domain functions exist (`update_workpad`, `set_workpad`); the core takes `mode` and calls one or the other. Smallest change, establishes the pattern and `CoreError` |
| 2.2 | Comments | Add typed `Speaker`. `operations::comments::create` keeps reading speaker fields from the body for v1 and MCP, but validates them into `Speaker` before the core call. The `created_by` override stays an operations-only input |
| 2.3 | Relations | Move the batch verdict loop into the core once. Operations keeps per-target commits (section 4.4); the agent passes its narrowed source and its 1 to 50 cap stays in the wrapper |
| 2.4 | State change | `core::issues::set_state` with `dispatch`. Verify first that the domain chain's `FireSpec { moved_by_run }` path and the agent's explicit `handle_issue_state_transition` call produce the same clock and ticker rows; if not, the core keeps both behind `dispatch` |
| 2.5 | Issue create | `core::issues::create` with `fire` and `apply_default_assignee`. `drain_pod` becomes part of `IssueCreated` so each wrapper acts on it |
| 2.6 | Issue update | `core::issues::update` with a typed `IssuePatch`. Only operations calls it in this phase; it exists so phase 4 has something to wrap |

For each: `SURFACE.txt` and `sdk/tests/surface.rs` gain the new `operations::core` paths. The overlay needs no change, because the `operations::*` signatures it calls are unchanged.

Done when, per entity: the phase 0 tests, `contract-tests/dispatch/`, `contract-tests/v1_work_items/` and the overlay's `contract-tests/mcp_suite/` all pass unchanged; and `tools_exec.rs` no longer calls `domain::comments`, `domain::relations` or `domain::issues` directly for that entity.

### Phase 3: shared reads

| # | Change |
| --- | --- |
| 3.1 | Add `services/src/operations/read/` with the functions in section 4.6, each taking `&impl ConnectionTrait` |
| 3.2 | Point the agent's read executors at them; `dispatch::tool_issues` becomes a thin shaping module |
| 3.3 | Point `operations::relations::grouped_relations` at the shared function and delete the copy in `tools_exec.rs:3112` |
| 3.4 | Overlay: point the MCP read tools at `sdk::operations::read::*` where the function covers their query; delete `resolve::group_issue_relations` |

Done when: the same suites pass, and grouped relations exist in exactly one place.

### Phase 4: new agent tools (needs the decisions in section 8)

With the core in place, a new agent tool is a wrapper plus its registration. The checklist for adding one tool, from the current wiring:

1. `READ_TOOLS` or `WRITE_TOOLS` in `services/src/dispatch/policy.rs:78-98`, and `CURRENT_ISSUE_WRITE_TOOLS` / `REPEATABLE_WRITE_TOOLS` / `NO_ISSUE_STRIPPED` if they apply; run-kind rules in `build_tool_plan` (`:616`).
2. `TOOL_CATALOG` in `services/src/dispatch/tools.rs:356` (size changes from 15).
3. A `TOOL_*` const and a `dispatch_fn` arm in `tools_exec.rs`.
4. The tool's schema in `fixtures/dispatch/fx-disp-08-model-runtime.golden.json` (`function_schemas` and `wire_tool_order`). This is required at run time: `invoke.rs:123` panics if a granted tool has no schema.
5. The name lists in `fx-disp-05-tools.golden.json` and the plan outputs in `fx-disp-03-policy.golden.json`.
6. No prompt change: `prompting/sections/cloud-capabilities.md` renders the tool list from the plan.

Proposed first additions:

| Tool | Wraps | Scope and limits |
| --- | --- | --- |
| `pidash_update_current_issue` | `core::issues::update` | Bound issue only. Fields: title, description (markdown), priority, labels, assignees. State stays with the transition tool. Single use per run |
| `pidash_create_project_issue` in issue runs | `core::issues::create` | Lift the scheduler-only rule in `build_tool_plan`; project is the bound issue's project |
| `pidash_update_comment` | `core::comments::update` | Only comments this run created (`speaker_agent_run_id = run`) |

### Phase 5: follow-on

- **Assistant chat tools.** `api/src/turn/tools_local_store.rs` has its own issue create and update, comment create, and relate / unrelate on domain row writes. Move them onto the core the same way; they become the fourth wrapper.
- **Converge accidental differences**, each as its own decision: workpad `Bare` vs `Sparse`; whether agent writes emit `issue_activity`; one wording for the relation not-found error; batch atomicity for relations.
- **Retire the two bypasses** in the overlay MCP once the Django oracle is no longer the reference: PR attach through `operations::pull_requests::attach`, and cross-project move.

## 6. Parity and tests

- **Django oracle.** The dispatch contract suite compares the Rust agent to Django's `cloud_agent/tools.py`. Phases 0 to 3 change no observable behaviour, so the oracle and the fixtures are untouched. Django has no operations package; its MCP uses the DRF serializers and its agent uses the ORM directly, so there is nothing to refactor on that side.
- **Phase 4 breaks parity on purpose.** New tools change `fx-disp-08` and the tool lists. Either Django gains the same tools, or the affected contract cases are marked Rust-only. That choice is decision 3 in section 8.
- **Known drift to carry, not fix here.** Django's `pidash_create_project_issue` now takes `assigned_runner` and `assigned_pod` (commit `f92fe3c07`); the Rust golden and `tools_exec.rs:1580` deliberately omit them.
- **MCP goldens.** The overlay's `fixtures/mcp/**` and `contract-tests/mcp_suite/` pin MCP descriptors and results. They must pass unchanged through phase 3.

| Suite | Guards | Must stay green in phases |
| --- | --- | --- |
| Phase 0 integration tests (new) | Agent write rows, results, side effects, atomicity | 1, 2, 3 |
| `rust-api/contract-tests/dispatch/` | Agent runs against the Django oracle | 1, 2, 3 |
| `rust-api/contract-tests/v1_work_items/` | v1 endpoints over operations | 2, 3 |
| Overlay `contract-tests/mcp_suite/` | MCP tools | 2, 3 |
| `fx-disp-03`, `fx-disp-05`, `fx-disp-08` replay tests | Plan, guards, schemas | 1, 2, 3 (edited in 4) |

## 7. Risks

| Risk | Mitigation |
| --- | --- |
| The agent's transition and the operations transition do not produce the same orchestration rows | Phase 2.4 starts with a comparison test; if they differ, the core keeps both paths behind `dispatch` and the difference is documented |
| `WriteTxn` forbids savepoints and some agent code relies on one (relate retry) | Replace with `relate_row_or_nothing` in phase 1.7; search `tools_exec.rs` for `SAVEPOINT` before starting |
| Moving `comments::create` from `pool.begin()` to `domain::run` changes when the `created_at` restamp is visible | Covered by `v1_work_items` contract tests; restamp moves inside the same transaction, so readers see it no later than before |
| After-commit steps on the audit transaction panic and skip later steps (`txn.rs:38`) | Queue only infallible steps; each fallible effect handles its own error and warns, as `enqueue_completion_comment` does |
| The create tool now holds the project lock for the whole audit transaction | Measure under the concurrency contract test (`test_concurrency.py`); the lock is already taken today, later in the same transaction |
| A core option is added to reproduce an accident and becomes permanent | Rule 5: every option must cite a row in section 3.5 and appears in the phase 5 convergence list |
| SDK surface churn breaks the overlay build | Phases 1 to 3 only add `operations::core` and `operations::read` paths; existing paths keep their signatures |

## 8. Open decisions

| # | Decision | Recommendation |
| --- | --- | --- |
| 1 | Should relation batches become atomic for `/api/v1` and MCP? | Not in this work. Keep per-target commits in phase 2; decide separately |
| 2 | Should agent writes emit `issue_activity` like human writes? | Yes, eventually, so agent comments and edits show in activity feeds; needs the same change in Django or a Rust-only decision |
| 3 | Phase 4 tools: add to Django too, or Rust-only? | Rust-only, with the affected contract cases marked; Django is the retiring side |
| 4 | Per-run write budget once the agent can edit and create | Raise `CLOUD_AGENT_WRITE_CALL_LIMIT` from 3 to 8 and keep single-use per tool |
| 5 | Where `core` lives: inside `operations` or as a sibling module | Inside, as `operations::core`, so the SDK surface gains one subtree |
| 6 | Whether the cloud agent executor is selectable again for issue work | Out of scope here. Today no UI can select it and writes normalise to `local_runner`; this refactor is still needed by scheduler runs and by the chat tools in phase 5 |

## 9. Out of scope

- Having the cloud agent call the MCP server.
- Changing MCP tool names, schemas or results.
- Scheduler tools (the ten MCP scheduler tools use a separate service with its own contract).
- Django changes.
- The chat-turn runtime in the overlay (`CloudTurnSeam::open_stream`), which is still a stub.
