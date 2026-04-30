# Implementation Plan

Phased rollout. Each phase is independently mergeable and leaves the system in a working state. Phases 1–2 are cloud-side and gate the runner-side work; phases 3–7 are runner-side. There is no v1 to maintain (no production runner users yet — see `design.md` §13); the protocol described here is the only one shipped.

## Phase 1 — Cloud: wire protocol (envelope `runner_id`)

**Goal**: cloud accepts wire frames with envelope `runner_id` and routes per the §4.2 / §4.3 rules.

- Extend `Envelope` (de)serialiser with `runner_id: Option<Uuid>`.
- Routing: dispatch frames by `Envelope.runner_id`. Connection-scoped frames (`Ping`, `Bye`, connection-wide `Revoke`) carry `None`; everything else carries `Some(id)`.
- Outbound: cloud-originated frames `Welcome`, `Assign`, `Cancel`, `Decide`, `RemoveRunner`, per-runner `ConfigPush` set `Envelope.runner_id = Some(id)`. Connection-scoped frames `Ping`, `Bye`, connection-wide `Revoke` leave it `None`. There is no connection-level `Welcome` — every `Welcome` acks a specific `Hello`.

**Done when**: integration test confirms a frame with mismatching or missing `runner_id` (where one is required) is rejected.

## Phase 2 — Cloud: token (machine credential) entity + UI

**Goal**: introduce `Token` entity, auth path, registration flow, UI changes for the tokens-vs-runners split.

- Schema: `tokens` table (`token_id`, `secret_hash`, `title`, `workspace_id`, `created_at`, `last_seen_at`, `revoked_at`); `runners.token_id` FK (NOT NULL — every runner is owned by a token).
- Auth path: WS upgrade headers carry `X-Token-Id` + `Bearer token_secret`. Validates against the token record; populates the connection's authorised set with the token's `owns`.
- Registration endpoints: `POST /api/v1/token/register/` (returns `token_id` + secret, secret shown once; created via cloud UI). `POST /api/v1/runner/register/` requires `token_id`, authenticated with `Bearer token_secret`.
- Multi-Hello support: server tracks `HashSet<Uuid>` of authorised runner_ids per connection, derived from `Token.owns` at auth time.
- UI changes: tokens section (lists tokens by title with their associated runners; **Revoke** action on the token) and runners section (per-runner **Remove** action; no Revoke on runner). The runners section also exposes the **Add Runner** action, which is the entry point for both new connections and additional runners on existing connections — see `design.md` §5.3 / §10.x.

**Done when**: a daemon authenticated as a Token can send `Hello { runner_id }` for any owned runner and receive `Welcome { runner_id }`; integration test confirms a `Hello` for an unowned runner is rejected with `RemoveRunner` without dropping the connection.

## Phase 3 — Runner: persistence + config layout

**Goal**: per-instance directory tree and config shape exist; daemon still hosts exactly one instance.

- `Paths` (`runner/src/util/paths.rs`) gains `runner_dir(runner_id) -> PathBuf` and a `RunnerPaths` newtype carrying a baked-in id.
- `Config` (`runner/src/config/schema.rs`) is `DaemonConfig` (cloud_url, heartbeat, log_level) + `Vec<RunnerConfig>`. `RunnerConfig` owns `agent`, `workspace`, `approval_policy`, `name`, `runner_id`.
- `Credentials` carries a `[token]` block (`token_id`, `token_secret`, `title`), a `[[runner]]` array (each entry just `runner_id` + `name`), and the `api_token` field for REST auth. Daemon refuses to start if `credentials.toml` exists but has no `[token]` block (`design.md` §13.3).
- `HistoryWriter`, `RunsIndex`, log paths take `RunnerPaths` instead of `Paths`.

Daemon still hosts one instance — `Vec<RunnerConfig>` always has length 1 — but the per-instance plumbing is in place.

**Done when**: a fresh install produces a `data_dir/runners/<id>/history/...` layout and runs end-to-end.

## Phase 4 — Runner: per-instance state types

**Goal**: `RunnerInstance` exists as a struct; per-instance `RunnerStateHandle`, `ApprovalRouter`, mailbox. Daemon still hosts one.

- Split `StateHandle` (`runner/src/daemon/state.rs`) into `ConnectionStateHandle` (per-daemon) and `RunnerStateHandle` (per-instance).
- Define `RunnerInstance` (see design §6.2).
- `ApprovalRouter` instantiated per instance.
- `RunnerLoop` (the inner loop in `runner/src/daemon/supervisor.rs:184`) takes a `RunnerInstance` instead of supervisor-wide handles.
- IPC `StatusSnapshot` becomes `{ daemon: DaemonInfo, runners: Vec<RunnerStatusSnapshot> }` with `runners.len() == 1`. TUI and CLI consumers updated to read the new shape.

The wire is single-runner at this point — the daemon sends one `Hello` and behaves as a single-tenant runner. Demux/mux are not yet introduced; the fan-out is purely internal data structures.

**Done when**: end-to-end run still works; TUI shows runner data from a list-of-one.

## Phase 5 — Runner: connection multiplex

**Goal**: daemon sends v2 envelopes with `runner_id`. Demux/Mux in place. Heartbeat fan-out. Still one instance.

- `Envelope` gains `runner_id` field; `WIRE_VERSION = 2`. Outgoing envelopes set `runner_id` from the sending instance.
- `RunnerOut` newtype wraps shared `out_tx`; replace every existing `out.send(Envelope::new(…))` site to use it.
- `Demux` task between `ConnectionLoop`'s inbound mpsc and per-instance mailboxes; supervisor inbox for connection-scoped frames.
- Heartbeat task iterates `instances` and emits one envelope per instance per tick.
- Auth: WS connects with `X-Token-Id` + `Bearer token_secret` from the `[token]` block in `credentials.toml`. No fallback path — daemon refuses to start without a token block (see `design.md` §13.3).
- `ConnectionLoop` walks `instances` after WS upgrade and sends `Hello { runner_id }` for each.
- Per-instance `Welcome`, `Reconnecting` flag, `RunResumed` after reconnect.

**Done when**: daemon hosting one instance multiplexes correctly against the cloud (single Hello, single Welcome, heartbeats arriving with `runner_id`) and end-to-end run still works.

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
- IPC verb `pidash remove --runner foo`: the CLI calls REST `POST /api/v1/runner/<runner_id>/deregister/` (authenticated with the token) to deregister cloud-side, then asks the daemon over IPC to drop the mailbox, remove from `instances`, and delete `data_dir/runners/<runner_id>/`. **No WS frame is sent** — `Bye` is reserved for connection teardown.
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
- **Tests**: extend `protocol_roundtrip.rs` for the new envelope shape and the new ServerMsg/ClientMsg variants. `cloud_ws_fake.rs` updated to speak the new protocol. New integration test: two-instance daemon, fake cloud, simultaneous assignments.

---

## Risk register

| Risk | Likelihood | Mitigation |
|---|---|---|
| Demux/Mux concurrency bugs (lost frames, wrong-instance routing) | Medium | Phase 5 is the riskiest single PR. Add property-based tests for the demux. Run with `RUST_LOG=trace` against the fake cloud during PR review. |
| One runner's slow agent backs up the shared `out_tx` and stalls others | Medium | Buffer size 512 (was 128). If still observed, give each instance its own bounded mpsc and `select!` across them at the mux. |
| User configures > 50 runners and is confused why the daemon won't start | Low | Daemon-side validation message explicitly states the cap and points to `design.md` §16. |
| Stale dev install on a developer's machine fails to start after upgrade | Low | The "no [token] block" error message names the exact recovery path (delete `credentials.toml` + `data_dir/runners/`, run setup script). Cheap to diagnose. |

## Estimated scope

Rough order of magnitude per phase, in PRs:

| Phase | PRs | Notes |
|---|---|---|
| 1 | 1–2 | Cloud-side, focused. |
| 2 | 2–3 | New cloud entity + UI changes are the long pole. |
| 3 | 1 | Runner-side, mechanical. |
| 4 | 1–2 | Mostly type-shuffling; IPC change is the wide one. |
| 5 | 1 | The risky one. |
| 6 | 1–2 | CLI/TUI changes are the bulk. |
| 7 | 1 | Runtime mutation; small but needs care around concurrency. |

Total: ~9–13 PRs across two repos. Phases 3–7 are runner-side and can pipeline once phase 1 lands cloud-side.
