# PR Review: #300 — PDASHOSS01-50 Runner auto spawn instead of manually run pidash cli

**Reviewed**: 2026-07-14
**Author**: irichard00
**Branch**: pi-dash/pdashoss01-50 → main
**Decision**: APPROVE (with comments)

## Summary

Solid three-layer feature (Django cloud endpoints, Rust daemon machine-control session with in-process hot-add, web modal picker) that closely mirrors the existing per-runner session architecture, with good test coverage (12 + 14 Django, 4 Rust, 2 web new tests) and a passing live end-to-end run. No CRITICAL or HIGH findings; the MEDIUMs below are hardening and observability follow-ups, none block merge.

## Findings

### CRITICAL

None.

### HIGH

None.

### MEDIUM

1. **Result write-back is not bound to the originating machine** — `apps/api/pi_dash/runner/views/machine_commands.py:196` (`MachineCommandResultEndpoint.post`). The endpoint verifies the `mt_` token is bound to the _URL's_ `dev_machine_id`, but not that `request_id` was issued _for that machine_. A valid token for machine B could overwrite the result of machine A's create-runner request if it learned the `request_id` (a non-guessable uuid4, so exploitability is low; impact is limited to forging the modal's ok/error panel). **Fix**: store `dev_machine_id` in the pending marker written at enqueue (`MachineCreateRunnerEndpoint`), and reject a write-back whose machine doesn't match.

2. **Silent enqueue failure reads as success** — `apps/api/pi_dash/runner/views/machine_commands.py:146` + `services/pubsub.py:send_to_machine`. `send_to_machine` re-raises `MachineOfflineError` but swallows every other exception (logs only). If the Redis `XADD` fails for any other reason, the endpoint still returns `202` and the modal polls "pending" for 90s before showing a timeout. **Fix**: let `enqueue_for_machine` failures propagate (or return a delivery flag) and answer `503` so the operator gets immediate feedback.

3. **Hot-added runner is invisible to IPC/TUI until restart** — `runner/src/daemon/supervisor.rs` (`RunnerSpawnCtx::add_runner`, documented in its doc comment). The IPC server's instance snapshot is built at startup, so `pidash tui` / `pidash status` won't list a cloud-created runner until the next daemon restart, even though it is online and taking work. Documented gap; worth a follow-up issue (make the IPC instance map live, mirroring `mailboxes`/`hello_runners`).

### LOW

4. **Dedupe set cleared wholesale at cap** — `runner/src/daemon/machine_control.rs` (`SEEN_CAP` handling). `seen_mids.clear()` wipes the entire at-least-once guard once 256 mids accumulate, so a crash-redelivery straddling the clear could double-execute `create_runner` (second attempt then fails cloud-side with `runner_name_taken` for named runners, or creates a duplicate auto-named runner). Consider a generational/LRU eviction instead. The at-least-once caveat is already documented in the file.

5. **Partial-success error message after post-registration failure** — `machine_control.rs::create_runner_inner`. Cloud rollback covers the config-write failure, but if the in-process hot-add (`spawn_ctx.add_runner`) fails, the runner row _and_ config block persist (the runner will come up on next daemon restart) while the modal reports "creation failed". Consider a distinct message ("created; will start on next daemon restart").

6. **Cross-module private imports** — `machine_commands.py` imports `_RUNNER_NAME_RE` (enrollment), `_auth_dev_machine` (machine_sessions), `_machine_is_in_workspace_scope`/`_request_workspace_id` (runners). Works, but promotes underscore-private helpers into a de-facto shared API; consider moving them to a shared module.

7. **Legacy `pidash connect` installs never get machine control** — those configs lack `dev_machine_id`/`[cli].token`, so the daemon logs one info line and the machine never appears "connected" in the modal. Consider a `pidash doctor` check and a docs note pointing at `pidash auth login`.

8. **Modal picker silently snaps to manual** — `add-runner-modal.tsx` (offline snap-back effect). If the selected machine drops offline mid-form, the picker reverts to "Run manually" without telling the user. Minor UX; a hint text would help.

## Validation Results

| Check                                                                                  | Result                                                                               |
| -------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------ |
| Rust clippy (`cargo clippy -p pidash --lib`)                                           | Pass (0 warnings)                                                                    |
| Rust tests (`cargo test -p pidash --lib`)                                              | Pass (420/420)                                                                       |
| Django runner unit suite (`pytest pi_dash/tests/unit/runner/`)                         | Pass (383/383)                                                                       |
| Web tests (`vitest run`, apps/web)                                                     | Pass (42/42)                                                                         |
| Type check (`turbo check:types` web/services/types)                                    | Pass                                                                                 |
| Lint (oxlint changed files)                                                            | Pass (0 warnings)                                                                    |
| Migration drift (`makemigrations --dry-run`)                                           | Pre-existing drift on main only; PR's model change is additive and covered by `0019` |
| Live e2e (zero-runner daemon → session → create_runner → hot-add → online → result ok) | Pass                                                                                 |

## Files Reviewed

- A `apps/api/pi_dash/runner/migrations/0019_machine_session.py`
- M `apps/api/pi_dash/runner/models.py` (additive: `MachineSession`)
- M `apps/api/pi_dash/runner/serializers.py` (`control_online`)
- A `apps/api/pi_dash/runner/services/machine_outbox.py`
- M `apps/api/pi_dash/runner/services/pubsub.py` (`send_to_machine`)
- M `apps/api/pi_dash/runner/urls.py`, `web_urls.py`, `views/__init__.py`
- A `apps/api/pi_dash/runner/views/machine_commands.py`
- A `apps/api/pi_dash/runner/views/machine_sessions.py`
- M `apps/api/pi_dash/runner/views/runners.py` (`control_online` annotation)
- A `apps/api/pi_dash/tests/unit/runner/test_machine_commands.py`, `test_machine_outbox.py`, `test_machine_sessions.py`
- M `apps/web/core/components/runners/add-runner-modal.tsx`, `apps/web/tests/runners/add-runner-modal.test.tsx`
- M `packages/i18n/…/en/translations.ts`, `packages/services/…/runner.service.ts`, `packages/types/src/runner.ts`
- M `runner/src/cli/run.rs` (zero-runner gate), `runner/src/cloud/http.rs` (`MachineClient`), `runner/src/cloud/protocol.rs` (`MachineMsg`)
- A `runner/src/daemon/machine_control.rs`
- M `runner/src/daemon/mod.rs`, `runner/src/daemon/supervisor.rs` (`RunnerSpawnCtx` extraction)
