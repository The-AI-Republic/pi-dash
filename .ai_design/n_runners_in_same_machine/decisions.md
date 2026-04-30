# Decisions

Record of design questions raised during scoping and how each was resolved. The detailed design (`design.md`) reflects these answers; this doc preserves the rationale so future readers don't have to re-litigate.

---

## Q1 — Auth model

**Question**: when the daemon connects to the cloud, what authenticates? Per-runner secrets (one per runner, like today) or a single machine credential that owns N runners?

**Decision**: **machine credential**, surfaced in the UI as a **token** with a user-supplied title.

Mental model: **one dev machine == one daemon == one token == one WS connection == N runner instances.**

Each runner still has its own `runner_id` (used as a routing key on the wire and a display key in the UI), but no per-runner secret. The token is the security primitive; the runner is the operational primitive.

**Rationale**:
- Clean rotation story — rotate the token without touching runner records.
- Audit logs name the host doing the work, which is what's actually wanted when investigating an incident.
- Removes the bootstrap-vs-rest auth asymmetry that an in-band multi-Hello scheme would have.

**Affects**: `design.md` §5 (token-only auth model), §8 persistence layout (`token/` directory), §9 config (`[token]` block in `credentials.toml`).

---

## Q2 — Cloud team's appetite for v2 protocol

**Question**: will the cloud team prioritise the v2 wire protocol (envelope `runner_id`)? If not, runner side should fall back to the N-WebSocket-per-process variant that needs zero cloud changes.

**Decision**: **cloud team has committed to v2.** Cloud and runner side ship the change together.

**Rationale**: explicitly committed by the cloud team. No need to build the N-WS fallback.

**Consequences**:
- The N-WS variant is no longer a fallback. ADR §15 in `design.md` is retained as a historical record but marked final.
- Migration window is bounded (~one release cycle of v1/v2 coexistence) rather than indefinite — see `design.md` §13.

**Affects**: `design.md` §13 simplified, ADR §15 closed, `implementation-plan.md` risk register cleaned up.

---

## Q3 — Failure-coupling acceptance

**Question**: with one shared WebSocket, a connection blip stalls *all* runners on this daemon at once. Concretely: approval requests queue locally for ~seconds, all heartbeats miss a tick or two, all runners flip to `Reconnecting` together. Runs themselves continue locally and catch up on reconnect — only the cloud-visible signalling pauses. Is this acceptable, or do we need per-runner failure isolation badly enough to use N WebSockets?

**Decision**: **acceptable.** Connection is always shared among runners on the same dev machine.

**Rationale**: the cost of a connection blip (sub-second to a few seconds of cloud-invisible time, then catch-up) is small enough that paying for failure isolation isn't worth the protocol complexity and per-machine connection count of N-WS.

**Affects**: locks in the shared-WS design. No design changes; just confirms direction.

---

## Q4 — Per-runner Revoke vs token Revoke

**Question**: today there's a single `Revoke` message that shuts the runner down. With multiple runners on one connection, what should it do — surgically remove one runner, or kill the whole connection?

**Decision**: **split into two distinct concepts on the cloud-side UI:**

1. **Tokens section**: lists active tokens by title. Each entry shows the runners associated with the token. Tokens have a **Revoke** action — kills the token, the connection drops on next auth check, all runners under that token go offline together.
2. **Runners section**: lists runners (under their owning tokens). Runners have a **Remove** action only. **No Revoke action on runners.**

**Wire-protocol consequence**:
- `Revoke` is connection-scoped only — `Envelope.runner_id` is always `None`. No per-runner Revoke.
- New variant `RemoveRunner { runner_id, reason }` — cloud-initiated decommission of one runner, leaves connection and other runners alone.

**Rationale**: revocation is a security action against a credential; if a single runner needs to go away (because it's no longer needed, or moved hosts), that's an operational action — Remove. Conflating them under one verb produces UI confusion ("does revoking this runner kill my other ones?") that the split avoids.

**Affects**: `design.md` §4.2 routing table, §5.3 UI surface, §11.4 (renamed to "Removing a runner"), §11.5 (new "Token revocation" section), §12 failure semantics table.

---

## Q5 — Instance count cap

**Question**: should there be a hard limit on how many runners one daemon can host?

**Decision**: **default cap = 50.** Both daemon-side validation (refuses to start if `config.toml` lists more) and cloud-side enforcement (rejects `Hello` beyond the cap) use this number.

**Rationale**: 50 is well above any plausible legitimate use (an 8-core dev box can productively serve maybe 4–6 concurrent codex agents) and well below "unbounded." The cap is a foot-gun guard and an abuse mitigation, not a tuning knob. Single configurable number keeps the policy simple.

Cap can be revisited later — lower it if 50 turns out to mask configuration mistakes; raise it if real fleet-style usage emerges.

**Affects**: `design.md` §16 (new section), §9 validation rule.

---

## Q6 — Workspace collision policy

**Question**: each runner has a working directory. If two runners point at the same directory, their git operations corrupt each other. Should the daemon refuse to start, or just warn? And how strict — exact match, or also nested paths?

**Decision**: **hard error, daemon refuses to start, with a detailed message.** Each runner must have its own unique working directory. Both exact-path collisions and nested paths (one is a strict prefix of another) are refused.

Example error:
```
configuration error: runners "main" and "side-project" share working_dir "/home/rich/work".
Each runner must have its own working directory. Update one of them in config.toml.
```

**Rationale**: two runners on the same workspace cause silent data corruption (git index races, mid-run `checkout` clobbering another run's tree). Once the daemon starts and accepts assignments, the corruption is irrecoverable from logs alone. Catching at startup with a clear pointer to the conflicting paths is far cheaper than triaging the corrupted runs after the fact.

**Affects**: `design.md` §9 validation rules with explicit error format.

---

## Q7 — Multi-workspace per machine

**Question**: in Pi Dash, "workspace" is a tenant. Should one machine be able to host runners across different Pi Dash workspaces (e.g. personal + employer)?

**Decision**: **defer.** All runners on one daemon belong to a single Pi Dash workspace — the workspace the token was created in. Multi-workspace per machine is out of scope for this design.

**Rationale**: scope discipline. Each axis of identity added to the multi-runner project roughly doubles design complexity (CLI commands need a `--workspace` selector, UI needs cross-workspace runner views, billing/quota gets cross-workspace). Multi-runner alone is enough work; cross-workspace is a separate project.

If the need emerges later, the runner-side change is small (it's a CLI selector and a credentials-per-workspace tweak), but solving it concurrently roughly doubles cloud-side work too.

**Affects**: `design.md` §2 non-goals (explicit workspace cardinality), §5.1 token entity (`workspace_id` field, scoped to one workspace).

---

## Q8 — Token rotation

**Question**: how does a user rotate a token's secret? In-place rotation flow, or create-new + revoke-old?

**Decision**: **no in-place rotation.** Tokens have only two operations: create and revoke. To "rotate," the user creates a new token in the UI, runs `pidash configure token` to install it on the machine, then revokes the old token in the UI.

**Rationale**: keeps the cloud-side surface minimal. In-place rotation would need an overlap window, careful coordination of which secret is current, and additional UI affordances. The create-new + revoke-old workflow already produces the same security outcome with no new code paths.

**Affects**: `design.md` §5.2 (rotation explicitly called out as not supported, with the workaround documented).

---

## Q9 — REST API auth scope

**Question**: the runner has two distinct credentials — `token_secret` (for WS upgrade auth) and `api_token` (for the `/api/v1/` REST surface via `X-Api-Key`; `runner/src/api_client.rs:142`, `runner/src/config/schema.rs:166`). Should both move to one credential, or stay separate?

**Decision**: **separate credentials, one per surface.** WS uses `X-Token-Id` + `Bearer token_secret`. REST stays on `X-Api-Key: <api_token>`. Unifying both surfaces onto one credential is deferred.

| Surface | Credential | Header |
|---|---|---|
| WS (`/ws/runner/`) | `token_secret` | `X-Token-Id` + `Bearer token_secret` |
| REST (`/api/v1/...`) | `api_token` | `X-Api-Key: <api_token>` |

**Rationale**: unifying WS and REST onto one token is a separate auth-system redesign that reaches into:

- Every `/api/v1/` endpoint and the cloud middleware that validates `X-Api-Key`.
- The `PIDASH_TOKEN` env-var path in the CLI client (`runner/src/api_client.rs:8`).
- Every contract test on the v1 surface (`runner/tests/pidash_cli_contract.rs`).
- The `pidash login` retrofit flow (`schema.rs:165`).

That work has its own scoping conversation. Punting it lets multi-runner stay focused on the WS-side multiplex; a follow-up can unify both surfaces if and when it's wanted.

**Affects**: `design.md` §5.2.1 (the two-surfaces table).

---

## Q10 — Behavior on token revocation with in-flight runs

**Question**: when a token is revoked while runs are in flight, does the daemon attempt graceful shutdown (commit-and-push WIP) or hard-cancel?

**Decision**: **hard cancel.** Same path as any other cancellation — agent subprocess gets 5s grace then SIGKILL. Uncommitted agent work is lost.

**Rationale**: token revocation is a security action; the user is explicitly saying "this credential is compromised, kill everything now." Adding a graceful "commit WIP" step would (a) extend the window during which a compromised token is still authoring git commits, and (b) require new code paths and design decisions (commit message format, branch handling, what if the agent is mid-merge). The hard-cancel path already exists.

**Affects**: `design.md` §5.2 (lifecycle note), §11.5 (token revocation), §11.4 (Remove uses the same hard-cancel path for consistency).

---

## Q11 — Runner data after removal

**Question**: when a runner is removed (locally via IPC or cloud-initiated via `RemoveRunner`), what happens to its local data directory (`data_dir/runners/<runner_id>/` — history, logs, identity)?

**Decision**: **delete it.** Once a runner is removed, its data is gone. No archival, no GC delay.

**Rationale**: a removed runner can't be brought back — its `runner_id` is decommissioned cloud-side and the user can't address its history through any CLI verb. Keeping orphan data forever just wastes disk. Users who want to keep history before removing should use `pidash status --runner <name>` to inspect first.

**Affects**: `design.md` §11.4 (step 5 deletes the data dir).

---

## Q12 — `ConfigPush` scope (per-runner vs daemon-wide)

**Question**: with multi-runner, the protocol allows `ConfigPush` at either scope (per-runner via `Envelope.runner_id = Some(id)`, or daemon-wide via `None`). Which fields go at which scope?

**Decision**: **per-runner only at this stage.** `ConfigPush` updates one runner's `approval_policy` slice. The daemon-wide path (`runner_id = None`) is reserved by the protocol shape but has no current consumer.

**Rationale**: today's `ConfigPush` already only touches `approval_policy`, which is per-runner by design (each runner can have a different policy for different repo sensitivities). No daemon-level field is currently remotely pushable (`cloud_url` and `heartbeat_interval_secs` come from `Welcome`; `log_level` is local-only). Keeping the door open for daemon-wide pushes without designing a use case avoids over-engineering.

**Affects**: `design.md` §4.2 routing table (`ConfigPush` row), §9.2 (new subsection on config scopes).

