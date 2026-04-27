# Implementation Plan

Phased rollout. Each phase is independently mergeable and leaves the system in a working state. Phases 1–2 are cloud-side and gate the runner-side work; phases 3–7 are runner-side. Cloud and runner teams ship v2 in coordination (see Q2 in `decisions.md`).

## Phase 1 — Cloud: protocol v2 (envelope `runner_id`)

**Goal**: cloud accepts both v1 and v2 wire frames; v2 frames must carry `runner_id`, v1 frames inherit it from the connection's authenticated identity.

- Extend `Envelope` deserialiser to accept optional `runner_id`.
- Bump server's announced `protocol_version` to `2` in `Welcome`.
- Routing layer: dispatch frames by `Envelope.runner_id` when present; fall back to "the connection's runner" when not.
- Outbound: cloud-originated frames (`Assign`, `Cancel`, `Decide`, `Ping`, `Welcome`, `Revoke`) start setting `Envelope.runner_id`. Connection-scoped frames (`Welcome` ack of connection itself, `Ping`, `Bye`, kill-switch `Revoke`) leave it `None`.
- v1 runners unaffected: still see one runner per connection.

**Done when**: a v1 runner still works end-to-end; integration test confirms a v2 envelope with mismatching `runner_id` is rejected.

## Phase 2 — Cloud: token (machine credential) entity + UI

**Goal**: introduce `Token` entity, new auth path, registration flow, UI changes for the tokens-vs-runners split.

- Schema migration: `tokens` table (`token_id`, `secret_hash`, `title`, `workspace_id`, `created_at`, `last_seen_at`, `revoked_at`); `runners.token_id` FK (nullable for legacy during migration).
- Auth path: WS upgrade headers carry `X-Token-Id` + `Bearer token_secret`. Validates against the token record; populates the connection's authorised set with the token's `owns`.
- Back-compat: existing single-runner connections keep using `X-Runner-Id` + `Bearer runner_secret` for the deprecation window. On first upgrade contact (or via a one-shot migration job), mint a one-runner Token whose title defaults to the runner's name.
- New registration endpoints: `POST /api/v1/token/register/` (returns `token_id` + secret, secret shown once). Existing `POST /api/v1/runner/register/` gains a required `token_id` parameter for v2; without it, mints a one-runner token for back-compat during the deprecation window.
- Transitional endpoint `POST /api/v1/runner/attach_token/` (authenticated with `runner_secret`, takes `token_id` + `token_secret` proof) — moves an existing v1 runner into a token's `owns` set. This is the only path by which a v1 install picks up token-based WS auth (see `decisions.md` Q13). Removed at the end of the deprecation window.
- Multi-Hello support: server tracks `HashSet<Uuid>` of authorised runner_ids per connection, derived from `Token.owns` at auth time.
- UI changes: tokens section (lists tokens by title with their associated runners; **Revoke** action on the token) and runners section (per-runner **Remove** action; no Revoke on runner). See `design.md` §5.3.

**Done when**: a daemon authenticated as a Token can send `Hello { runner_id }` for any owned runner and receive `Welcome { runner_id }`; integration test confirms a `Hello` for an unowned runner is rejected with `RemoveRunner` without dropping the connection.

## Phase 3 — Runner: persistence + config layout

**Goal**: per-instance directory tree and config shape exist; daemon still hosts exactly one instance.

- `Paths` (`runner/src/util/paths.rs`) gains `runner_dir(runner_id) -> PathBuf` and a `RunnerPaths` newtype carrying a baked-in id.
- `Config` (`runner/src/config/schema.rs`) splits into `DaemonConfig` (cloud_url, heartbeat, log_level) + `Vec<RunnerConfig>`. `RunnerConfig` owns `agent`, `workspace`, `approval_policy`, `name`, `runner_id`.
- One-shot config migration on daemon startup: if old top-level `[agent]/[workspace]/[approval_policy]` shape is detected, lift into a single `[[runner]] name="default"` block and rewrite `config.toml`.
- `Credentials` gains a `[token]` block (`token_id`, `token_secret`, `title`) and a `[[runner]]` array. The existing `runner_id` + `runner_secret` + `api_token` fields are retained during the deprecation window. **No auto-migration**: a v1 install keeps using v1 auth until the operator runs `pidash configure token` (see `design.md` §13.3 and `decisions.md` Q13). Phase 3 only lays the *structural* groundwork — adding the new fields to the schema, supporting both shapes when reading.
- `HistoryWriter`, `RunsIndex`, log paths take `RunnerPaths` instead of `Paths`. Existing `data_dir/history/...` symlinked or moved to `data_dir/runners/<existing_runner_id>/history/...` on first startup.

Daemon still hosts one instance — `Vec<RunnerConfig>` always has length 1 — but the per-instance plumbing is in place.

**Done when**: a fresh install + a migrated install both produce identical `data_dir/runners/<id>/history/...` layouts and run end-to-end.

## Phase 4 — Runner: per-instance state types

**Goal**: `RunnerInstance` exists as a struct; per-instance `RunnerStateHandle`, `ApprovalRouter`, mailbox. Daemon still hosts one.

- Split `StateHandle` (`runner/src/daemon/state.rs`) into `ConnectionStateHandle` (per-daemon) and `RunnerStateHandle` (per-instance).
- Define `RunnerInstance` (see design §6.2).
- `ApprovalRouter` instantiated per instance.
- `RunnerLoop` (the inner loop in `runner/src/daemon/supervisor.rs:184`) takes a `RunnerInstance` instead of supervisor-wide handles.
- IPC `StatusSnapshot` becomes `{ daemon: DaemonInfo, runners: Vec<RunnerStatusSnapshot> }` with `runners.len() == 1`. TUI and CLI consumers updated to read the new shape.

The wire protocol is still v1 at this point — daemon authenticates as before, sends one `Hello`. The fan-out is internal only.

**Done when**: end-to-end run still works; TUI shows the same data as before, just from a list-of-one.

## Phase 5 — Runner: connection multiplex

**Goal**: daemon sends v2 envelopes with `runner_id`. Demux/Mux in place. Heartbeat fan-out. Still one instance.

- `Envelope` gains `runner_id` field; `WIRE_VERSION = 2`. Outgoing envelopes set `runner_id` from the sending instance.
- `RunnerOut` newtype wraps shared `out_tx`; replace every existing `out.send(Envelope::new(…))` site to use it.
- `Demux` task between `ConnectionLoop`'s inbound mpsc and per-instance mailboxes; supervisor inbox for connection-scoped frames.
- Heartbeat task iterates `instances` and emits one envelope per instance per tick.
- Auth: switch WS headers to `X-Token-Id` + `Bearer token_secret`. Old `X-Runner-Id` headers retired (cloud still accepts them for the deprecation window).
- `ConnectionLoop` walks `instances` after WS upgrade and sends `Hello { runner_id }` for each.
- Per-instance `Welcome`, `Reconnecting` flag, `RunResumed` after reconnect.

**Done when**: daemon hosting one instance speaks v2 to a v2 cloud and end-to-end run still works; v1 fallback path verified manually against a v1 cloud.

## Phase 6 — Runner: lift the cap

**Goal**: daemon hosts N instances configured statically.

- Allow `Vec<RunnerConfig>` of arbitrary length; supervisor spawns N `RunnerLoop`s.
- IPC commands gain `--runner <name|id>` selector. Default to single instance when N = 1; require selector when N > 1.
- `pidash configure` splits into `configure token` (one-shot per host) and `configure runner --name` (per instance).
- `pidash status`, `pidash issue`, `pidash comment`, `pidash remove`, `pidash doctor` all gain `--runner`.
- TUI gains an instance picker / multi-pane view. Status panel shows per-instance current_run + approvals_pending.
- Validation at daemon startup (hard errors with detailed messages): duplicate `runner_id`, duplicate `name`, exact-match or nested `working_dir`, instance count > 50. See `design.md` §9.

**Done when**: a daemon configured with two instances runs concurrent assignments end-to-end and survives a WS reconnect with both runs in flight; a misconfigured daemon (duplicate working_dir) refuses to start with a clear error message naming both runners and both paths.

## Phase 7 — Runtime add + cloud-initiated remove + token revocation

**Goal**: instances can be added or removed without restarting; cloud can remove one runner without dropping the connection; cloud can revoke the whole token.

- IPC verb `pidash configure runner --name foo` registers via REST under the active token and tells the daemon "load instance foo". Supervisor inserts into `instances`, sends `Hello`, spawns `RunnerLoop`. No reconnect.
- IPC verb `pidash remove --runner foo` sends `Bye { runner_id: foo, reason: "removed" }`, drops mailbox, removes from `instances`.
- Cloud-originated `RemoveRunner { runner_id, reason }` (new wire variant) handled in supervisor: cancel in-flight run for that runner, remove instance, leave connection up. Same end state as the local IPC remove.
- Connection-scoped `Revoke { reason }` (token revocation; no `runner_id`) tears down the connection and shuts down the daemon — see `design.md` §11.5.

**Done when**: adding a runner at runtime works end-to-end; cloud-initiated `RemoveRunner` cleanly evicts one instance while a run on a sibling instance keeps running; revoking the token from the UI shuts down the daemon on next auth check (synchronous if the connection is up, on next reconnect if not).

---

## Cross-cutting work

These don't fit neatly into a single phase but should land alongside the appropriate one:

- **Logging**: every log line in the runner gains `runner_id` (or `runner_name`) in its tracing span. Add `tracing::Span` per-instance scoped to `RunnerLoop`.
- **Metrics**: every metric label set gains `runner_id`. The `instances_count` gauge on the daemon side is new.
- **Doctor**: `pidash doctor` walks each instance — checks each runner's working_dir, agent installation, and credentials independently.
- **Backwards compat shim**: `pidash status` (no `--runner`) on a multi-instance daemon prints a friendly error directing the user to `--runner <name>` or `pidash status --all`.
- **Tests**: `protocol_roundtrip.rs` test already exists for v1; add v2 cases. `cloud_ws_fake.rs` extended to act as a v2 server. New integration test: two-instance daemon, fake cloud, simultaneous assignments.

---

## Risk register

| Risk | Likelihood | Mitigation |
|---|---|---|
| Migration of existing single-runner installations corrupts history | Low | Phase 3 includes a dry-run mode and a backup of the old layout to `data_dir/.pre-multi-runner/`. |
| Cloud's `runners → token` migration leaves orphan rows or mis-titles tokens | Low | Phase 2 migration runs in two stages: (a) populate `token_id` and `tokens.title` from the runner's existing name; (b) flip auth to require `token_id`. Stage (a) is reversible. |
| Demux/Mux concurrency bugs (lost frames, wrong-instance routing) | Medium | Phase 5 is the riskiest single PR. Add property-based tests for the demux. Run with `RUST_LOG=trace` against the fake cloud during PR review. |
| One runner's slow agent backs up the shared `out_tx` and stalls others | Medium | Buffer size 512 (was 128). If still observed, give each instance its own bounded mpsc and `select!` across them at the mux. |
| Per-instance directory layout breaks IDE tooling that reads `data_dir/history/` | Low | Symlink `data_dir/history -> data_dir/runners/<default_runner_id>/history` for the single-instance case during phase 3 to avoid surprising downstream tools. |
| User configures > 50 runners and is confused why the daemon won't start | Low | Daemon-side validation message explicitly states the cap and points to `design.md` §16. |

## Estimated scope

Rough order of magnitude per phase, in PRs:

| Phase | PRs | Notes |
|---|---|---|
| 1 | 1–2 | Cloud-side, focused. |
| 2 | 2–3 | Cloud schema migration is the long pole. |
| 3 | 1 | Runner-side, mechanical. |
| 4 | 1–2 | Mostly type-shuffling; IPC change is the wide one. |
| 5 | 1 | The risky one. |
| 6 | 1–2 | CLI/TUI changes are the bulk. |
| 7 | 1 | Runtime mutation; small but needs care around concurrency. |

Total: ~9–13 PRs across two repos. Phases 3–7 are runner-side and can pipeline once phase 1 lands cloud-side.
