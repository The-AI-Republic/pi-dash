# Hybrid Issue Assignment — a Pod or One Dedicated Runner

**Status:** design, revision 2 (PDASHOSS01-296, HA-1). Every decision is settled: H-1 to H-10 as before, H-11 to H-14 decided by the product owner on 2026-11-06. H-11 supersedes H-1 and H-13 supersedes H-8, for every dedicated runner. Coordinator: PDASHOSS01-295.
**Builds on:** `.ai_design/issue_runner/design.md` (pods, `Issue.assigned_pod`, the matcher), `.ai_design/n_runners_in_same_machine/new_pod_project_relationship/design.md` (pods are project-scoped), `.ai_design/issue_run_improve/design.md` §5.7 (run pinning), `.ai_design/managed_runner/design.md` §8.4–§8.5 (a pin that is an execution target, and waiting visibly).

Today an issue is routed to a **pod**: any free runner in that squad takes the run. This doc adds a second mode: the issue is routed to **one dedicated runner**, chosen by a person, because of the machine, tools or access that runner has.

Every decision in this doc is settled. The offline policy (§11) was first decided by the project owner on 2026-10-12 (H-1) and replaced on 2026-11-06 (H-11). The five open questions H-2 to H-6 were answered on 2026-10-15, H-7 to H-9 on 2026-10-21, and H-11 to H-14 on 2026-11-06; §21 lists the answers and where each one lands. §23, added on 2026-11-05, brings in the engine runners and carries H-10.

**What changed in revision 2 (2026-11-06).**

- **H-11 (replaces H-1), every dedicated runner.** When the issue's runner is offline or paused, a person's run is refused at creation with `assigned_runner_offline` or `assigned_runner_paused`, nothing is queued and the clock is not armed; the refusal offers **Run on any runner in <pod> once** (`pod_once`) and **Change runner**. Automatic runs are created, wait pinned, and fail with `dedicated_runner_unavailable` after 12 hours if the runner has not taken them; the failure stops the clock (§11).
- **H-13 (replaces H-8), every dedicated runner.** A dedicated run is never released to the pod. `release-pin` refuses it with HTTP 409 `dedicated_pin_not_releasable`; `dispatch_wait.releasable` is always false for it; the waiting banner, CLI and MCP have no release. Locality-pin release is unchanged. `pod_once` stays, with its own authorization: the issue's creator and current assignees (§12.2, §23.3).
- **H-12 and H-14** keep their defaults: the project's lost-runner rule applies to engines, and §23.1 to §23.7 are adopted.
- **I2 and E6 removed.** Since #2903 `local_runner` is the only executor an issue can have, so a dedicated runner no longer needs or sets one. The I2 check constraint, its `IS NOT NULL` text and test (#2980), the code `executor_not_local` and §8's rules that tied the runner to the executor are gone. Numbers are not reused.
- **Refreshed against `main` `0e5b395dc`**: §5, §8, §9.1, §9.3, §9.4, §12 and §23 now describe the merged engine work (`c0ee3148b`, #2903) instead of a future one; migration numbers, line references, the Rust section (§17) and the web_new mismatches (M1 to M8) are updated.

**Implementation status (2026-11-06).** Nothing of this design is implemented on the server. At `main` `0e5b395dc` no Django or `rust-api/` code reads or writes `assigned_runner`, `pin_kind`, `dispatch_wait`, `dedicated_runner_lost_action` or `assignable_for` (the only mentions are docstrings in #2903 that point here); HA-2 to HA-6 are open. The built-in engine work this design builds on is merged: the design (`.ai_design/built_engine_as_runner/design.md` revision 4.0, #1496), the server and web part (#2903, squash `c0ee3148b`) and the CLI, daemon and desktop part (`38a95820f`). The new frontend already ships the UI against this contract, merged with `apps/web_new` on 2026-10-30 and adjusted by #2903:

- the two-level pod and runner picker: `apps/web_new/src/shared/pickers/pods.ts` (`buildExecutionTargets`, the E-code sentences, the status line) and `PodPicker.tsx`, reading the runner list with `assignable_for` (`src/shared/pickers/reference.ts`);
- the execution-target write: `src/features/issues/api/executionTarget.ts`, one issue `PATCH` of `assigned_pod_id`, `assigned_runner_id` and `agent_executor`; the create form sends `assigned_runner_id` too (`src/features/issues/create/createForm.ts`);
- the waiting banner and badge: `src/features/issues/detail/waiting/waiting.ts` and `WaitingBanner.tsx`, reading `agent_status.active_run.pinned_runner`, `pinned_runner_name`, `pin_kind` and `dispatch_wait`, and calling `POST /api/runners/runs/<id>/release-pin/`;
- the AI Workers project settings page: `src/features/project-settings/ai-workers/` and the route `src/routes/$ws/_gated/settings/projects/$projectId/ai-workers.tsx`, writing `dedicated_runner_lost_action` through the project `PATCH`;
- the shapes, in `packages/api-client/src/contracts/`: `issues.ts` (`assigned_runner_id`, `AssignedRunnerDetail`), `issue-agent.ts` and `runs.ts` (`DispatchWait`, the pin fields, `ReleaseRunPinResult`), `runners.ts` (`assignable`, `not_assignable_code`, `is_busy`, `queue_depth`, `assignableFor`), `projects.ts` (`dedicated_runner_lost_action`).

The server work must match the contract `apps/web_new` already sends and reads. Checked against this doc, these match: the `assigned_runner_id` write and read on the app API (§14.1), including the list field and the 400 body keyed by `assigned_runner_id`; `assigned_runner_detail` field for field; the four `assignable_for` row fields (§7.4); the run pin fields and the `dispatch_wait` fields it had before revision 2 (§14.3); the two `dedicated_runner_lost_action` values (§6.6). Differences found (M2 and M6 are resolved by #2903 and kept for history; M7 and M8 come from revision 2):

| #   | Where                                        | Difference                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                |
| --- | -------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| M1  | Issue and project `PATCH` today              | Not a shape mismatch but a live gap: the server has no `assigned_runner_id` or `dedicated_runner_lost_action` field, so DRF drops both without an error. A runner picked in `apps/web_new` shows as chosen until the refetch, then reads as the pod; the AI Workers setting does not persist. This is the "saved choice that dispatch ignores" §19 rules out, reached from the client side. Until HA-2b, `apps/web_new` should not offer the runner level (or HA-2a should reject the field).                             |
| M2  | `pods.ts` agent rows, `executionTargetPatch` | Resolved by #2903: the picker offers no agent rows ("Runner is the only executor"), and a pod or runner pick writes `agent_executor: "local_runner"`. `AgentExecutorOption` has the `deprecated` field. Sending `agent_executor` with a runner stays harmless after I2's removal (§8).                                                                                                                                                                                                                                    |
| M3  | `waiting.ts`, `DispatchWait`                 | No `runner_paused` case and no `pause_reason` field (§23.5, now for every runner), and no `expires_at` (§14.3). An unknown reason renders as the raw code ("Waiting for runner X — runner_paused."), so it degrades but reads badly. Nor does any client handle the run-creation refusals `assigned_runner_offline` and `assigned_runner_paused` (§11) or send `pin: "pod_once"` (§23.3).                                                                                                                                 |
| M4  | `pods.ts` runner rows                        | Rows are labelled `runner.name`, with the machine name beside it since #2903 (`runnerMachineName`). Engines are skipped (`isEngineRunner`) until §23 lands; then they are listed and read "Built-in engine · <machine>" (§23.6). `RUNNER_NOT_ASSIGNABLE_REASONS` keeps `runner_desktop_bundled`, which §23.1 removes.                                                                                                                                                                                                     |
| M5  | `ai-workers/rules.ts`                        | The `fall_back` explainer says "Queued runs continue on any runner in the pod instead". §13.1 falls back to the issue's pod, then the project's default pod, then the no-eligible-runner bounce. The text should not promise the pod.                                                                                                                                                                                                                                                                                     |
| M6  | Runner list (`listRunners`)                  | Resolved by #2903: the server no longer hides engines and the picker skips them itself (M4).                                                                                                                                                                                                                                                                                                                                                                                                                              |
| M7  | `WaitingBanner.tsx`, `waitingRuns.ts`        | The banner's release action calls `POST /api/runners/runs/<id>/release-pin/` (`releaseRunPin`) and is enabled by `dispatch_wait.releasable`. Under H-13 a dedicated run is never released: the server answers 409 `dedicated_pin_not_releasable` and `releasable` is always false, so the button is always disabled for dedicated runs. The banner should drop the release action for `pin_kind = "dedicated"` and keep Change runner and Cancel run (§14.4); `ReleaseRunPinResult`'s `assignment` note no longer occurs. |
| M8  | `pods.ts` `RUNNER_NOT_ASSIGNABLE_REASONS`    | Keeps `executor_not_local`, a code the server will never send now that E6 is removed (§7.1). Harmless; remove it with M4's cleanup.                                                                                                                                                                                                                                                                                                                                                                                       |

**Sequencing (product owner, 2026-11-05; updated 2026-11-06).** The built-in engine design (`.ai_design/built_engine_as_runner/design.md`, PIDESKAPP-121, PR #1496) was implemented and finished first, and that work is now merged to `main`: #2903 (squash `c0ee3148b`, server and web: pods-only dispatch, `Runner.pod_dispatch`, the engine usage rule, the machine pause fields, migrations `runner` 0034 to 0038 and `db` 0172) and `38a95820f` (CLI, daemon and desktop). Implementation of this design, §23 included, starts now and builds on `main` as it is: `Runner.pod_dispatch`, the engine usage rule in `filter_runs_usable_by_runner` and `pod_has_runner_for_issue_principal`, and the `DevMachine` pause fields. The Rust side follows §17: `rust-api/` lives on `main`, reads and writes through SeaORM, and the HA work there starts from `main` after the SeaORM migration batches that changed the runner endpoints (Batches 1a to 1g, #2899 to #2916) and the issue, transition and clock domains (Batches 2a to 3c, #2918 to #2939).

---

## 1. Why

A pod answers "who is free?". It cannot answer "who is able?". Two runners in the same pod can differ in ways the cloud does not model: one sits on the machine with the GPU, the VPN, the signing key, the licensed tool or the 200 GB dataset. Users work around this today by creating a one-runner pod per special machine, which multiplies pods and breaks the squad for everything else.

The dispatch layer already has what is needed: `AgentRun.pinned_runner` routes a queued run to exactly one runner, and `next_for_runner()` serves a runner's pinned runs before the pod's shared queue. What is missing is a way for a user to ask for it, a rule for what the pin means when the user asked for it, and the surfaces to show a run that is waiting for its runner.

## 2. Goals

- An issue is assigned either to a pod (today's behaviour, unchanged) or to one runner inside a pod.
- Every run of a dedicated issue — implement, review, test, ticks, comment-triggered, Run AI — goes to that runner and to no other. A person can send one run to the pod on purpose (`pod_once`, §23.3) or change the issue's runner; nothing else moves the work.
- A run waiting for its runner is visible and says why it waits. A person's run is not queued for a runner that is offline or paused (§11); an automatic run waits at most 12 hours.
- An issue with no dedicated runner behaves exactly as it does today. Existing matcher and pin tests pass unchanged.
- One validation path shared by the web API, the v1 API, the CLI and the MCP tools.

## 3. Non-goals

- **Routing by capability.** v1 picks a runner by identity. "Any runner that has label X" is sketched in §16 and deferred.
- **Scheduler bindings.** `SchedulerBinding.pod` keeps its squad-only shape in v1 (§15).
- **Cloud Agent and managed (desktop) runs.** Since #2903 neither can be chosen for an issue or a project (§8); old runs keep their `executor_kind` and finish as before. The built-in engine as a runner, which replaced managed runs, is §23.
- **Multi-run concurrency on one runner.** A runner still works one issue run at a time.
- **Changing the pod model.** Every runner still belongs to exactly one pod; a dedicated runner is "the pod, narrowed to one runner". Runners with no pod, and a runner in several pods, are deferred (§22).
- **Product code in this issue.** This doc only.

## 4. Vocabulary

| Term                 | Meaning                                                                                                                                      |
| -------------------- | -------------------------------------------------------------------------------------------------------------------------------------------- |
| **Pod assignment**   | `Issue.assigned_pod` set, `Issue.assigned_runner` NULL. Any eligible runner in the pod takes the run. Also called squad mode.                |
| **Dedicated runner** | `Issue.assigned_runner` set. Only that runner takes the issue's runs.                                                                        |
| **Pin**              | `AgentRun.pinned_runner`: this queued run may only be taken by this runner.                                                                  |
| **Locality pin**     | A pin the system set as a best-effort hint, so a follow-up run lands where the repo is already on disk. May be dropped at any time.          |
| **Dedicated pin**    | A pin that exists because the issue has a dedicated runner. A user choice. Never dropped by the system without a rule in this doc saying so. |
| **Personal queue**   | The queued runs pinned to one runner, oldest first. Served before the pod's shared queue.                                                    |
| **Release**          | Dropping the pin on one queued run so the pod's shared queue serves it. Locality pins only; a dedicated pin is never released (H-13, §12.2). |
| **Pod-once run**     | A run a person asks for with `pin: "pod_once"`: created unpinned in the pod, for that run only; the issue stays dedicated (§23.3).           |
| **Shared queue**     | The queued runs of a pod with no pin. Any eligible runner in the pod may take them.                                                          |
| **Lost-runner rule** | The project setting that says what happens to a dedicated issue when its runner is revoked or deleted (§6.6, §13.1).                         |

## 5. What exists today

- `Issue.assigned_pod` (`apps/api/pi_dash/db/models/issue.py`) is the only routing field. `Issue.save()` resolves it to the project's default pod on creation. Since #2903 `Issue.agent_executor` can only be `local_runner` or NULL (inherit the project default, which is `local_runner`); older clients' `cloud_agent` and `managed_runner` writes are translated (§8).
- `AgentRun.pinned_runner` (`runner/models.py`) is set only by the system, for locality: by `_pinned_runner_for()` (`orchestration/service.py:531`), which pins a follow-up to the parent's runner only when that runner is online, heartbeat-fresh, not paused, in the target pod and, for an engine, a `pod_member` whose engine is not paused and whose usage rule admits the new run's creator and trigger. `cloud_agent/creation.py:execution_fields()` no longer pins anything: every new machine run is `local_runner`, and only legacy queued `managed_runner` runs still carry a pin to an engine.
- `runner/services/matcher.py`: `next_for_runner()` (`:142`) returns runs pinned to the runner first, then unpinned runs in its pod, each tier oldest first. An own-agent runner takes the unpinned tier only as a `pod_member`; an engine takes locality pins and the unpinned tier only while `engine_takes_pod_work` (a `pod_member` whose machine's engine is not paused), plus its legacy `managed_runner` pins. `next_queued_run_for_pod()` skips pinned runs. `drain_pod()` and `drain_for_runner()` skip a runner that has a run in `BUSY_STATUSES`, and both go through `Runner.objects.assignable()`, which excludes offline, heartbeat-stale, operator-paused (`paused_at`) and engine-paused runners.
- Pins are dropped by four paths, all for `local_runner` runs only: `runner/tasks.py:release_pins_on_offline_runners` (`:207`) once the pinned runner has been silent for `RUNNER_OFFLINE_PIN_RELEASE_SECS` (300 s), or is paused, engine-paused or an engine that is `pinned_only`; `release_pins_for_paused_runner` (`:230`) when an owner pauses a runner (both read `_releasable_pins()`, `:184`); `runner/services/dispatch_rules.py:release_locality_pins` (`:125`) when an engine's machine is paused or its pod work is turned off; and `runner/views/runs.py:AgentRunReleasePinEndpoint` (`:682`), by hand, session auth only. `Runner.revoke()` un-pins queued runs and cancels active ones. `runner/services/run_lifecycle.py:apply_run_resume_unavailable` re-queues a run and drops its pin.
- `runner/services/permissions.py:filter_runs_usable_by_runner` (`:83`): an own-agent private runner (the only visibility today) takes a run only if its owner created the run, is billed for it, created the issue, is a current assignee, or authored the run's scheduler binding. An engine (`provisioning = desktop_bundled`) is narrower, unless its machine has `engine_team_runs` on: runs its owner created, plus automatic runs (trigger not in `HUMAN_TRIGGERS`) on the owner's issues or from the owner's bindings. `dispatch_rules.py:runner_may_take` (`:70`) is the Python form used by preflight and locality pinning. Tick runs are created by the agent system user (`orchestration/scheduling.py:_resolve_creator_for_trigger`), so for ticks only the issue and binding clauses can match.
- One active run per issue (`agent_run_one_active_per_work_item`). `Issue.has_active_run` blocks pod and executor changes mid-flight in both serializers (`app/serializers/issue.py`, `api/serializers/issue.py`).
- Runners are visible only to their owner (`runner_visible_to_user_q`). Run payloads already show `runner_name` to anyone who can see the issue.
- Gaps found while reading, relevant below: the CLI and the MCP tools cannot set a pod at all; a change of `assigned_pod` is not written to the issue activity log.

## 6. Data model

### 6.1 `Issue.assigned_runner` (new)

**Decision: confirm the recommended starting point.**

```python
# apps/api/pi_dash/db/models/issue.py, next to assigned_pod
assigned_runner = models.ForeignKey(
    "runner.Runner",
    null=True,
    blank=True,
    on_delete=models.SET_NULL,
    db_index=False,          # replaced by the partial index below
    related_name="dedicated_issues",
)
```

NULL means pod assignment. Every existing row stays NULL.

Rejected alternatives:

- _A one-runner pod per special machine._ What users do today; it is the problem.
- _A mode enum plus a generic target id._ Loses the FK and its integrity for no gain; the two columns already encode the mode.
- _Only a run-level field._ Every creation path would need the choice passed in, and ticks have nobody to pass it.

### 6.2 Invariants

| #   | Invariant                                                                                                                                                                            | Enforced by                                                                                          |
| --- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ---------------------------------------------------------------------------------------------------- |
| I1  | `assigned_runner` set ⇒ `assigned_pod` set and `assigned_runner.pod_id == assigned_pod_id`.                                                                                          | DB check for "pod is set"; the shared service (§7.3) and the lifecycle hooks (§13) for pod equality. |
| I2  | _Removed (2026-11-06)._ Was: `assigned_runner` set ⇒ `agent_executor == "local_runner"`, explicit.                                                                                   | —                                                                                                    |
| I3  | `assigned_runner` is never a revoked or `desktop_bundled` runner. For engine runners, §23.1 reduces it to "never revoked".                                                           | Shared service on write; `Runner.revoke()` on revoke (§13).                                          |
| I4  | A queued run of an issue with a dedicated runner carries `pinned_runner == issue.assigned_runner` and `pin_kind == "dedicated"`, unless a person created it with `pod_once` (§23.3). | The pin resolver (§9.1) and the re-point step (§13.2). No path releases a dedicated pin (H-13).      |
| I5  | `pin_kind == "dedicated"` ⇒ `executor_kind == "local_runner"`.                                                                                                                       | DB check constraint on `AgentRun`.                                                                   |

I2 was there so that flipping the project default to the Cloud Agent could not strip the meaning from every dedicated issue. Since #2903 there is nothing to flip to: `local_runner` is the only executor an issue or a project can be set to, older clients' other values are translated on write, and migration `runner` 0035 rewrote the stored ones (§8). The invariant, its check constraint and the error code E6 are removed; a dedicated runner neither needs nor sets an executor.

The pod follows the runner. A caller sets the runner; the service sets `assigned_pod = runner.pod`. A caller that sends both must send a matching pair.

### 6.3 `on_delete`

`SET_NULL`. Runner rows are hard-deleted by `runner/services/runner_delete.py` (revoke, then delete). `Runner.revoke()` clears the assignment first and records why (§13), so `SET_NULL` is only the safety net for a row that disappears some other way. `PROTECT` was rejected: a user must always be able to remove their own machine.

`assigned_pod` stays `PROTECT`. Pod deletion already requires the pod to have no non-revoked runners (`runner/views/pods.py`), so no dedicated issue can point into a deleted pod.

### 6.4 `AgentRun.pin_kind` (new)

**Decision: a pin-kind field on the run.**

```python
class PinKind(models.TextChoices):
    LOCALITY = "locality", "Locality hint"
    DEDICATED = "dedicated", "Dedicated runner"

pin_kind = models.CharField(max_length=16, choices=PinKind.choices, default=PinKind.LOCALITY)
```

- Meaningful only while `pinned_runner` is set. A run with no pin keeps the default.
- Existing rows default to `locality`, which is what every non-managed pin is today. No backfill.
- Managed runs are not given a third kind. They are already identified by `executor_kind == "managed_runner"` and every sweep already branches on that; adding a kind would mean a backfill for no behaviour change.
- The kind is stamped at creation from the issue's assignment and rewritten only by the re-point step (§13.2). A release applies to locality pins only (§12.2) and leaves the kind `locality`.

Rejected: inferring the kind at read time from `run.work_item.assigned_runner`. The sweep and the matcher would need a join, and a run created before the issue's runner changed would read the new assignment instead of the one it was created under.

### 6.5 Migrations

Two migrations, both additive, numbered with the next free number at implementation time. On `main` at `0e5b395dc` the latest are `db` 0172 (`0172_clidevicecode_engine_scope`; 0171 is unused) and `runner` 0038 (`0038_rename_engine_runners`), so these would be `db` 0173 and `runner` 0039. Both are Django migrations and land first; the Rust port reads the new columns through SeaORM entities and adds no migration of its own (§17).

1. `db`: add `issues.assigned_runner_id` (nullable, no default — a metadata-only change on Postgres); add a partial index `WHERE assigned_runner_id IS NOT NULL` with `AddIndexConcurrently` in a non-atomic migration; add one check constraint, `assigned_runner_id IS NULL OR assigned_pod_id IS NOT NULL` (I1's "pod is set"). The second constraint of earlier revisions, for I2, is removed (2026-11-06) together with its NULL-executor test.
   In the same migration, add `projects.dedicated_runner_lost_action` (§6.6) with its default. The default is a constant, so on Postgres 11+ this is also metadata-only.
2. `runner`: add `agent_run.pin_kind` with a default; add the check constraint for I5; add an index on `agent_run (runner_id, assigned_at DESC)` with `AddIndexConcurrently` for the matcher's last-served lookup (§10.2).

Every existing row satisfies every new constraint. HA-6 times the constraint validation scans against the production row counts before release (§19).

### 6.6 `Project.dedicated_runner_lost_action` (new)

**Decision (project owner, 2026-10-15, H-2): what happens when a dedicated runner is revoked or deleted is a project setting.**

```python
# apps/api/pi_dash/db/models/project.py, next to the agent_* settings
class DedicatedRunnerLostAction(models.TextChoices):
    FAIL_TO_BACKLOG = "fail_to_backlog", "Fail the run and move the issue to Backlog"
    FALL_BACK = "fall_back", "Fall back to the pod"

dedicated_runner_lost_action = models.CharField(
    max_length=24,
    choices=DedicatedRunnerLostAction.choices,
    default=DedicatedRunnerLostAction.FAIL_TO_BACKLOG,
)
```

- Default `fail_to_backlog` for new and existing projects. No backfill.
- Read by the runner-side lifecycle (§13.1) and nothing else.
- Exposed on the app project serializer and the v1 project serializer as a writable field; written by project admins, as the other `agent_*` settings are. Edited on a new project settings page, **AI Workers** (§14.4).

## 7. Eligibility

### 7.1 Which runner can be chosen for an issue

A runner `R` can be the dedicated runner of issue `X`, when user `U` makes the request, if all of the following hold:

| #   | Rule                                                                                         | Error code                  |
| --- | -------------------------------------------------------------------------------------------- | --------------------------- |
| E1  | `U` can use `R` (`can_use_runner`): today, `U` owns it. Otherwise `R` "does not exist".      | `runner_not_found`          |
| E2  | `R.pod.project_id == X.project_id` and the pod is not soft-deleted.                          | `runner_wrong_project`      |
| E3  | If the request also names a pod, it is `R.pod`.                                              | `runner_pod_mismatch`       |
| E4  | `R.status != revoked`.                                                                       | `runner_revoked`            |
| E5  | `R.provisioning != desktop_bundled`. Lifted for engine runners (§23.1).                      | `runner_desktop_bundled`    |
| E6  | _Removed (2026-11-06)_: `local_runner` is the only executor (§8).                            | —                           |
| E7  | `R.owner` is the issue's creator or one of its current assignees.                            | `runner_owner_not_on_issue` |
| E8  | The issue has no run that a runner holds; `QUEUED` and `PAUSED_AWAITING_INPUT` pass (§13.2). | `active_run_in_progress`    |

E7 is `filter_runs_usable_by_runner` reduced to the clauses that hold for _every_ run of the issue. A tick run is created by the agent system user, so "the run's creator owns the runner" cannot be relied on. On issue creation the requester is the creator, so a user's own runner always passes E7. Combined with E1, in v1 a user dedicates an issue to **their own** runner, on an issue they created or are assigned to.

**Decision (project owner, 2026-10-15, H-4): confirmed as written.** The requester must be the runner's owner (E1) and also the issue's creator or a current assignee (E7). An issue has no separate owner field; "issue owner" means its creator. A request that fails E7 is rejected with `runner_owner_not_on_issue` and a message that says to add the runner's owner as an assignee. The owner is **not** added as an assignee automatically. When a non-private visibility is added to runners, E1 and E7 widen with `can_use_runner` and `filter_runs_usable_by_runner` and nothing else in this doc changes.

**Decision (project owner, 2026-10-21, H-7): any creator or current assignee may change or clear the dedicated runner**, whoever set it. If A dedicates the issue to A's runner and B is an assignee, B may switch it to B's own runner or clear it back to the pod; the rules check only the requester's relation to the runner they pick. There is no rule protecting the previous choice. The change is recorded in the activity log (§14.7).

### 7.2 Reject or warn

**Decision: structural problems are rejected on save; transient ones are allowed and shown.**

- E1–E8 (E6 removed) are structural: saving such a choice would create an issue that can never run, or desync a live run. The request fails with HTTP 400, `{"assigned_runner_id": [message], "code": <code>}`.
- Offline and busy are transient: the choice is saved. The response carries the runner's status and queue depth (§14.1) and the picker shows them before the user confirms (§14.4).
- The rules run only when the request changes the runner. Re-sending the current value is a no-op and is never rejected, as with the pod today: the web form re-PATCHes its full editable set, so a dedicated issue must stay editable after its eligibility has drifted.

Eligibility can drift after a save: the owner is removed from the assignees, or leaves the workspace. Save-time validation cannot prevent that, so §9.4 re-checks at run creation and §13 handles each event.

### 7.3 One shared service

`apps/api/pi_dash/runner/services/issue_assignment.py` owns the rules. The module exists since #2903 with one function, `normalize_issue_executor` (`:26`), which both issue serializers call (`app/serializers/issue.py:276`, `api/serializers/issue.py:193`) to translate legacy executor values and refuse an executor change under a live run. HA adds the functions below to it. Both serializers call them; neither re-implements them.

```python
def validate_execution_target(*, issue, project_id, user, pod, runner, changed) -> ExecutionTarget
def apply_execution_target(issue, target, *, actor) -> None      # saves, re-points a queued run (§13.2), writes activity
def clear_dedicated_runner(issue, *, reason, actor=None) -> None  # lifecycle hooks (§13)
def assignable_runners(*, project, user, issue=None) -> QuerySet  # §7.4
```

The executor stays with `normalize_issue_executor`, which does not look at the runner. The existing pod guards in the two serializers move into `validate_execution_target` unchanged, which also removes the small difference between them today (the app serializer skips the active-run guard when the current pod is NULL; the v1 serializer does not). The v1 behaviour wins.

### 7.4 Listing assignable runners

No new endpoint. The runner list gains one query parameter on both surfaces:

- web: `GET /api/runners/?workspace=&project=&assignable_for=<issue-id | "new">`
- v1 (added by PDASHOSS01-245): `GET /api/v1/workspaces/<slug>/projects/<project>/runners/?assignable_for=<issue-id | "new">`

With the parameter, each row gains `assignable` (bool), `not_assignable_code` (an E-code or `""`), `is_busy` (bool) and `queue_depth` (int, §10.1). `"new"` evaluates E7 with the requester as the future creator. The queryset comes from `assignable_runners()`, which starts from the shared helper PDASHOSS01-245 introduces. Rows the requester cannot see are not returned at all.

## 8. Executor kinds

**Since #2903 `local_runner` is the only executor an issue or a project can be set to** (`core/agent_execution.py`, `AgentExecutorKind` and `.ai_design/built_engine_as_runner/design.md` section 6). Which machine runs the work is a dispatch question, answered by the pod and, with this design, by the dedicated runner. So the executor plays no part in a dedicated runner. What remains of the other two values:

- **On write.** Older clients may still send `cloud_agent` or `managed_runner` for an issue or a project. `normalize_executor_write` (`core/agent_execution.py:39`) stores `local_runner` instead; for issues it is called by `runner/services/issue_assignment.py:normalize_issue_executor`, for projects by both project serializers. `agent_executor_options` still returns the two entries marked `deprecated: true`, so older builds keep showing them and new ones hide them.
- **Stored rows.** Migration `runner` 0035 rewrote projects and issues set to either value to `local_runner`, pod-level, and wrote no `assigned_runner`. At run creation `cloud_agent/policy.py:resolve_executor_kind` maps a leftover `managed_runner` value to `local_runner` too.
- **Runs.** `AgentRun.executor_kind` keeps `cloud_agent` and `managed_runner` on old rows: queued and running cloud runs finish on the Cloud Agent, and legacy queued managed runs stay pinned to their engine until they finish or `managed_runner.expire_waiting_runs` fails them after `MANAGED_RUNNER_QUEUED_MAX_AGE_SECS` (12 h). None of them can be a dedicated run (I5, §23.7).

Rules for the service:

- Setting, changing or clearing the runner does not read or write `agent_executor`. A request may carry an executor as well; it is normalized and guarded by `normalize_issue_executor` as today, independently of the runner.
- An executor write never clears `assigned_runner`. A translated legacy write (`managed_runner` from an old desktop build) leaves the dedicated runner as it is; "pod-level" in #2903's translation describes the state before this design, when no issue has a runner. Only an `assigned_runner_id` write, a pod change (§13.1) or a lifecycle event (§13.1) changes the runner.
- Rules removed on 2026-11-06 with I2: "setting a runner stamps `agent_executor = local_runner`", "a runner with a non-local executor fails with `executor_not_local`", and "an executor change to `cloud_agent`, `managed_runner` or inherit clears the runner".

## 9. Dispatch

### 9.1 One pin resolver

`_pinned_runner_for()` (`orchestration/service.py:531`, called by `_create_continuation_run` at `:456` and `_create_and_dispatch_run` at `:805`) is replaced by a resolver that returns the runner and the kind together, used by every creation path:

```python
def resolve_run_pin(issue, *, parent, pod, fresh_session, creator, trigger, pod_once=False) -> tuple[Optional[Runner], str]:
    runner = issue.assigned_runner
    if runner is not None and not pod_once:      # pod_once: §23.3
        if runner.status == RunnerStatus.REVOKED or runner.pod_id != pod.id:
            clear_dedicated_runner(issue, reason="stale_at_dispatch")   # self-heal, §13
        else:
            return runner, PinKind.DEDICATED        # offline, paused or busy: still pinned
    if fresh_session or parent is None or pod_once:
        return None, PinKind.LOCALITY
    # today's _pinned_runner_for, renamed, with its current rules
    return _locality_runner_for(parent, pod, creator=creator, trigger=trigger, issue=issue), PinKind.LOCALITY
```

`_locality_runner_for` keeps every rule `_pinned_runner_for` has on `main`: online, heartbeat-fresh, not paused, same pod, `locality_pin_allowed` (an engine only while `engine_takes_pod_work`), and for an engine `runner_may_take` for the new run's creator and trigger.

Precedence: dedicated > locality. `execution_fields()` no longer sets a pin (§5), so the managed pin of earlier revisions is gone. A dedicated pin does **not** require the runner to be online, unpaused or a `pod_member`; that is the difference from a locality pin, which is only set on a runner that takes pod work now. A person's run is refused before it reaches the resolver when the runner is offline or paused (§11), so in practice only automatic runs, and runs created while the runner was up, are pinned to a runner that is not.

### 9.2 Run phases

All paths end in one of five `AgentRun.objects.create` sites. Each takes its pin from `resolve_run_pin`.

| Path                                               | Creation site                                                             | Today                                | With a dedicated runner                          |
| -------------------------------------------------- | ------------------------------------------------------------------------- | ------------------------------------ | ------------------------------------------------ |
| First run, state transition into In Progress       | `_create_and_dispatch_run`                                                | no pin                               | dedicated pin                                    |
| Entering review or test (`fresh_session_on_entry`) | `_create_and_dispatch_run(fresh_session=True)`                            | clears `parent_run` and the pin      | clears `parent_run`; **keeps the dedicated pin** |
| Hand-back into In Progress                         | `_create_and_dispatch_run`                                                | locality pin when the runner is live | dedicated pin                                    |
| Tick continuation                                  | `scheduling.py` → `_create_continuation_run` / `_create_and_dispatch_run` | locality pin                         | dedicated pin                                    |
| Comment & Run                                      | `handle_issue_comment` → `_create_continuation_run`                       | locality pin                         | dedicated pin                                    |
| Run AI                                             | `scheduling.py` → either helper                                           | locality pin                         | dedicated pin                                    |
| Direct run bound to a work item                    | `runner/views/runs.py` (run create)                                       | no pin                               | dedicated pin                                    |
| Project-move handoff                               | `_create_project_move_handoff_run`                                        | no pin                               | no pin; the move cleared the assignment (§13)    |

`fresh_session` keeps its meaning: no parent, a new agent session. It never meant "any machine"; it only dropped the pin because the pin was a session-continuity hint. A dedicated pin is not one.

The human rows (first run and hand-back by a person's state move, Comment & Run, Run AI, direct run; triggers in `HUMAN_TRIGGERS`) are refused before creation when the dedicated runner is offline or paused (§11, H-11). The automatic rows (ticks, and the agent's own hand-offs, which the clock fires as `tick` runs) are created and wait.

Comment coalescing, the one-active-run rule and the ticker are unchanged.

### 9.3 Matcher

`next_for_runner()` (`runner/services/matcher.py:142`) changes in two places:

1. **What an engine may take.** On `main` an engine's eligible set is its legacy `managed_runner` pins plus, only while `engine_takes_pod_work(runner)`, `local_runner` runs pinned to it or unpinned. HA adds `local_runner` runs pinned to it with `pin_kind = dedicated`, whatever its `pod_dispatch` (§23.1). An own-agent runner needs no change: it already takes every run pinned to it, and the unpinned tier only as a `pod_member`, so a `pinned_only` own-agent runner serves its dedicated queue and nothing else.
2. **The tier order**, which now depends on what the runner served last (§10.2).

Everything else stays: the queue is already limited to the runner's pod, `QUEUED` and `MACHINE_EXECUTORS`, and already filtered by `filter_runs_usable_by_runner`, whose engine branch admits a dedicated automatic run on the owner's issue and refuses a teammate's human run (which §23.3 refuses at creation anyway). A paused or offline runner is never asked: `drain_pod` and `drain_for_runner` select runners through `Runner.objects.assignable()`, which excludes offline, heartbeat-stale, `paused_at` and engine-paused runners, so a dedicated run to such a runner waits. `next_queued_run_for_pod()` already excludes pinned runs, and `select_runner_in_pod` already excludes `pinned_only` runners from unpinned work. For a runner whose last run was not a dedicated one the order is exactly today's, so a pod with no dedicated issues sees no change and the existing matcher and pin tests pass unmodified. HA-3 adds the alternation tests (§18).

`apply_run_resume_unavailable` changes: for a `dedicated` pin it re-queues the run and keeps the pin and the kind (it still clears the parent's `thread_id`, so the next attempt is a fresh session on the same runner). For a `locality` pin it behaves as today.

### 9.4 Preflight and messages

The preflight is `scheduling.preflight_eligibility_or_bounce` (`orchestration/scheduling.py:948`), which calls `pod_has_runner_for_issue_principal(pod, issue, run_creator_id, *, human=True)` (`runner/services/matcher.py:375`) with `human` from the trigger. On `main` that function looks at the pod: a non-revoked runner that is not `pinned_only`, owned by the run creator, the issue creator or a current assignee; an engine counts only for its owner's own work (the run creator, or with `human=False` the issue's people), or for anyone when its machine has `engine_team_runs` on. Offline and paused runners count (transient).

It gains a branch for a dedicated issue: it checks that one runner instead of the pod — not revoked, in the run's pod, and `dispatch_rules.runner_may_take(runner, created_by_id=run_creator_id, human=human, issue=issue)`, the Python form of the usage rule, which covers own-agent runners and engines alike. `pod_dispatch` is ignored in this branch: a `pinned_only` runner takes the runs dedicated to it (§23.1). The pod branch is unchanged and is the one a `pod_once` run uses (§23.3). On failure the existing bounce runs (`_bounce_issue_no_eligible_runner`: back to Backlog, system comment) with reason `dedicated-runner-not-eligible`, and the comment names the runner and the failing rule.

Offline and paused are not a bounce. For a person's run they are a refusal at creation (§11, H-11), checked before the clock is touched; for an automatic run the run is created and waits, for at most 12 hours (§11). When an automatic run is created for a dedicated runner that is offline or paused, creation also posts one system comment:

> Queued for runner **gpu-box**, which is offline (last seen 2 h ago). The run starts when it reconnects. If it has not started by 08:14 tomorrow (12 hours), it fails and automatic runs on this issue stop. To run it elsewhere, cancel it and use **Run on any runner in Default once**, or change the issue's runner.

Bot comments do not wake the agent, so this cannot loop. The run payload carries the same fact in `dispatch_wait` (§14.3).

A dedicated run can still be stranded after creation if eligibility drifts while it is queued (the owner is un-assigned). The sweep in §12.1 detects a dedicated queued run whose runner is online and idle but which `filter_runs_usable_by_runner` excludes, fails it with `error_code = "dedicated_runner_not_permitted"`, and the normal failure comment names the runner.

## 10. Busy runner queueing

### 10.1 Bottleneck

Several issues dedicated to one runner run one after another while the rest of the pod may be idle. That is the meaning of the choice, so it is allowed, and it is made visible.

**Decision: show queue depth; warn; no hard limit.**

- `queue_depth(R)` = the number of `QUEUED` runs with `pinned_runner = R`. `is_busy(R)` = `R` has a run in `BUSY_STATUSES`. Both are computed in the runner list (§7.4), not stored.
- The picker shows one status line per runner: `online · idle`, `busy · 3 waiting`, `offline · last seen 2 h ago`.
- Choosing a runner that is busy or has a queue shows an inline note: "gpu-box runs one issue at a time. This issue will wait behind 3 others." It does not block the save.
- No cap in v1. A cap needs a number nobody has data for; the queue depth gives that data. Revisit after rollout.

### 10.2 Dedicated work versus shared work

**Decision (project owner, 2026-10-15, H-3): strict 1:1 alternation between the runner's dedicated runs and the pod's shared queue.** After a runner finishes a dedicated run, its next pick is the oldest run in the pod's shared queue if one is waiting; after that it goes back to its dedicated runs. If either side is empty it takes from the other. A runner with a steady stream of dedicated issues therefore still takes every second run from the shared queue while shared work is waiting.

Locality pins keep today's first priority. They are follow-ups of issues with no dedicated runner, so for the alternation they count as shared work. (This reading is the owner's stated interpretation; it keeps today's behaviour for every issue that has no dedicated runner.)

The matcher's tier rank, per run, given `last_dedicated` = "the last run this runner was assigned had `pin_kind = dedicated`":

| Run                               | `last_dedicated` false (today's order) | `last_dedicated` true |
| --------------------------------- | -------------------------------------- | --------------------- |
| Pinned to me, `locality`          | 0                                      | 0                     |
| Pinned to me, `dedicated`         | 0                                      | 2                     |
| Unpinned (the pod's shared queue) | 1                                      | 1                     |

Inside a rank, oldest `created_at` first, as today. With `last_dedicated` false the ranks are identical to today's `_is_mine` annotation.

**What the runner served last is derived, not stored.** `last_dedicated` is true when the runner's most recent run by `assigned_at` (filtered to `assigned_at IS NOT NULL`, since Postgres sorts NULLs first in a descending order; one indexed lookup, §6.5) has `pinned_runner` set and `pin_kind = dedicated`, read inside `next_for_runner` before the queue query. When `next_for_runner` runs, the runner is idle, so that run is terminal or paused. A run's `pin_kind` does not change after assignment: a re-point (§13.2) happens only while the run is `QUEUED`, and a release (§12.2) applies only to locality pins, which count as shared work anyway. A `pod_once` run (§23.3) has no pin and counts as shared work, which is what it is. A runner with no runs yet counts as `last_dedicated` false.

Rejected: a stored `Runner.last_served_kind`. It would have to be written by every assignment path (`drain_pod`, `drain_for_runner`, the session-open drain), and it can drift from the runs it summarises. The derived value cannot.

Effects the epics must keep:

- `drain_pod` assigns at most one run per idle runner per pass, so the alternation holds inside one pass too.
- A dedicated run can now wait behind one shared run on its own runner. `dispatch_wait.reason` is `runner_busy` and `runs_ahead` counts that shared run (§14.3).
- Alternation does not make the shared queue reach a runner that `filter_runs_usable_by_runner` excludes; a private runner still only takes shared runs its owner may run.
- No cap on the dedicated queue; queue depth is shown and warned (§10.1, unchanged).

### 10.3 A waiting run blocks its issue

While a run is `QUEUED` the issue has an active run: no other run can start, and today its pod cannot change. Three things follow.

1. **The wait is visible.** Every pinned queued run reports `dispatch_wait` (§14.3). The issue shows "Waiting for runner gpu-box — busy, 2 runs ahead" or "— offline, last seen 2 h ago".
2. **The wait costs one run.** The waiting run is the active run, so no tick creates another; the tick budget is not drained while an issue waits.
3. **The wait ends** when the runner takes the run, when the 12-hour bound fails it (§11; only while the runner is offline or paused), or when a person acts. A person has two ways, both available while the run is still `QUEUED`:
   - _Change or clear the issue's runner_ (§13.2). The queued run is re-pointed in the same transaction: to the new runner, or to the pod's shared queue when cleared.
   - _Cancel the run._ As today. To have this one piece of work done elsewhere, the person then asks for **Run on any runner in <pod> once** (§23.3), which needs the issue to have no active run.

   Releasing the waiting run to the pod is not offered (H-13, §12.2).

## 11. Offline policy

The dedicated runner is offline and a run is queued for it. Options:

| Option                                   | For                                                                             | Against                                                                                                          |
| ---------------------------------------- | ------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------- |
| (a) Wait for the runner                  | Honours the choice. Same as a pod whose runners are all offline: the run waits. | The issue is blocked until someone notices.                                                                      |
| (b) Fall back to the pod after a timeout | Work keeps moving.                                                              | Runs the issue on a machine the user ruled out. It may fail, or succeed wrongly (wrong data, wrong credentials). |
| (c) Per-issue choice                     | Lets the user say which of the two they meant.                                  | One more field and one more control on every surface, before there is evidence it is needed.                     |

**History.** On 2026-10-12 the project owner chose (a) with no timeout (H-1): every run waits, visibly, until a person ends the wait. **On 2026-11-06 H-11 replaced it, for every dedicated runner, own-agent and engine alike:** the issue stays on its runner, there is still no fallback to the pod, but a person's run is refused instead of queued, and an automatic run waits at most 12 hours. This section says how to build it.

### 11.1 Unavailable runner

The dedicated runner is **unavailable** when it is offline or paused:

- **Offline:** `status` is not `online`, or `last_heartbeat_at` is NULL or older than `HEARTBEAT_GRACE` (90 s, `runner/services/matcher.py:43`), the same test `Runner.objects.assignable()` applies.
- **Paused:** `Runner.paused_at` is set (the owner's dispatch pause, `dispatch_paused` in the runner serializer), or, for an engine, its machine's `engine_paused` (`DevMachine.engine_paused_by_owner` or a non-empty `engine_auto_pause_reason`).

Paused wins over offline when both hold, because a person must act on it. Busy is not unavailable: a busy runner is online and takes the run when it is free (§10). Revoked is not handled here: the resolver self-heals a stale assignment (§9.1) and revoke applies the lost-runner rule (§13.1).

### 11.2 A person's run is refused at creation

A run a person starts directly — Run AI (web button and `run_ai_for_human` on the v1 API), Comment & Run, a direct run bound to the issue, or a person's state move into a ticking state; the triggers in `HUMAN_TRIGGERS` (`runner/models.py:327`) with a person behind the request — is refused when the issue's dedicated runner is unavailable. Nothing is queued and the issue is not bounced.

- **One check**, `dedicated_run_refusal(issue, *, actor, trigger) -> Optional[Refusal]` in `runner/services/issue_assignment.py`, used by every human entry point. It makes H-10's usage-rule check (§23.3) first and this availability check second, so a teammate excluded by an engine's usage rule gets `assigned_runner_not_owner` whatever the engine's state. It is skipped when the request carries `pin: "pod_once"`.
- **Codes:**
  - `assigned_runner_offline`: the runner is offline (§11.1). `pause_reason` is `null`.
  - `assigned_runner_paused`: the runner is paused. `pause_reason` says why: `dispatch_paused` (`Runner.paused_at`), `owner_paused` (`DevMachine.engine_paused_by_owner`), or the machine's `engine_auto_pause_reason`: `engine_sign_in_required`, `gateway_session_revoked`, `wallet_empty`, `profile_unavailable`, `engine_disabled`.
- **Response** on Run AI and the direct-run endpoint: HTTP 409, the shape Run AI refusals already use (`{"error", "reason"}`, `api/views/issue.py:1238`), extended:

  ```json
  {
    "error": "Built-in engine · laptop is paused: the wallet is empty.",
    "reason": "assigned_runner_paused",
    "pause_reason": "wallet_empty",
    "runner": "…",
    "runner_name": "Built-in engine · laptop",
    "runner_last_seen_at": "2026-11-06T07:02:11Z",
    "pod": "…",
    "pod_name": "Default",
    "options": ["pod_once", "change_runner"]
  }
  ```

  The `error` text names the reason in plain words: "offline, last seen 2 h ago", "paused by its owner", "the engine's sign-in expired", "the wallet is empty", and so on. `options` lists what the UI and the CLI offer next: **Run on any runner in <pod> once** (retry with `pin: "pod_once"`, §23.3) and **Change runner** (§13.2).

- **Comment & Run** saves the comment; **a state move** stands. Each gets one system comment saying no run started, why, and the two options.
- **The clock is not armed.** The check runs before `scheduling.reconcile`, exactly as for H-10's refusal (§23.3, which now applies to every dedicated runner): the event is applied to the issue but not to the clock. Otherwise the next tick, an automatic run, would create the very run the refusal stopped and leave it waiting.
- **A person's request the clock holds.** A person who asks while another run is active gets a free entry on the clock (`_queue_entry`), which `fire_tick` creates later with the person's trigger (`bgtasks/agent_ticker.py:227`). The check ran when the person asked. If the runner becomes unavailable before the entry fires, nobody is there to receive a refusal, so the run is created and waits as an automatic run does (§11.3).

### 11.3 An automatic run waits, at most 12 hours

Automatic runs are ticks and the agent's own hand-offs (which the clock fires as `tick` runs); scheduler runs have no dedicated runner in v1 (§15).

- The run is created `QUEUED` and pinned, `pin_kind = dedicated`. Creation posts the comment of §9.4, and `dispatch_wait` reports `runner_offline` or `runner_paused` with its `pause_reason` and `expires_at` (§14.3). Waiting costs nothing but the one run (§10.3).
- No sweep releases it (§12.1).
- **A runner that comes back drains its queue first, as today.** Session open and the poll-ready path call `drain_for_runner_by_id` (`runner/services/session_service.py:334`, `runner/views/sessions.py:449`); resuming a paused runner drains it (`runner/views/runners.py`, the resume endpoint); clearing an engine pause drains the machine's engine runners (`dispatch_rules.py:on_engine_pause_changed`). Each serves the personal queue before the pod's (§10.2). No new code.
- **The 12-hour bound.** A new setting, `DEDICATED_RUN_QUEUED_MAX_AGE_SECS`, default 43200 (12 h), registered in `config/registry.py` next to `MANAGED_RUNNER_QUEUED_MAX_AGE_SECS`, with a system check that refuses a value `<= 0` as `managed.E004` does for the managed one. A third pass of `runner.release_pins_on_offline_runners` (every minute) selects `QUEUED` `local_runner` runs with `pin_kind = dedicated`, `created_at` older than the setting, whose pinned runner is unavailable now (§11.1). That test is its own `Q`, not `_pinned_runner_gone_q`: a `pinned_only` runner is not unavailable for a dedicated pin. Each run is failed through `finalize_agent_run(run_id, FAILED, expected_status=QUEUED, updates=…)`, with the unavailability re-checked under the row lock as `_release_pins` does, so a runner that came back in the meantime keeps its run.
  - `error_code = "dedicated_runner_unavailable"`.
  - `error`, which the existing failure path posts as the issue comment ("Run failed." and the text, `run_lifecycle._post_failure_comment` via `apply_terminal_effects`): "Runner gpu-box did not take this run within 12 hours: it is offline (last seen 14 h ago). Automatic runs on this issue are stopped. The issue stays assigned to gpu-box. When the runner is back, use Run or Re-tick. To run it elsewhere, use Run on any runner in Default once, or change the issue's runner."
  - `done_payload = {"stop_ticking": true}`.
- **The clock stops**, through the existing stop-signal handling: `_on_run_ended` (`orchestration/scheduling.py:458`) sees `stop_ticking` and stops the clock with `TickerDisarmReason.STOP_SIGNAL`, as when an agent yields `--stop-ticking`. The issue stays in its state; there is no auto-pause and no Backlog bounce. The human levers that clear a stop signal today restart it, and each is a person's run, so it is refused again (§11.2) while the runner is still unavailable. `_on_run_ended`'s guards apply unchanged: a pending entry on the clock is kept, and a run from a stage the issue has left does not stop the clock. Not chosen: the default failure handling, which keeps the clock ticking (as `managed_runner.expire_waiting_runs` does for legacy managed runs). The next tick would create another run at once, which would wait another 12 hours: one failure comment and one pool slot every 12 hours until `CAP_HIT` pauses the issue.
- The bound applies whatever the run's trigger, so it also covers a person's run created while the runner was up that then went offline, and a held free entry (§11.2). A run waiting behind a busy, online runner is not failed: it is moving, and the queue depth is shown (§10.1).

### 11.4 Not in v1

(b) and (c) of the table stay out, and v1 adds no setting or field for them (§22). Both would move a dedicated run to the pod without a person asking, which H-13 rules out; a person who wants that asks for `pod_once` (§23.3).

Runs already `ASSIGNED` or `RUNNING` on a runner that goes silent are not touched by this doc: `fail_runs_on_offline_runners` (`RUNNER_OFFLINE_RUN_FAIL_SECS`, 3600 s) fails them as today, and the next automatic run is pinned to the dedicated runner again and waits under §11.3.

## 12. User-chosen pin versus locality pin

| Mechanism                                                           | Locality pin                                                                                                                                                            | Dedicated pin                                                               |
| ------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------- |
| Set when                                                            | Parent's runner takes pod work now: online, fresh heartbeat, not paused, same pod; an engine also `pod_member`, not engine-paused, usage rule admits the run (as today) | Issue has a dedicated runner, whatever its status; not for a `pod_once` run |
| `release_pins_on_offline_runners`                                   | Released after `RUNNER_OFFLINE_PIN_RELEASE_SECS`, or at once when paused, engine-paused or `pinned_only` (as today)                                                     | **Not released.** Failed after 12 h while the runner is unavailable (§11.3) |
| `release_pins_for_paused_runner` (owner pauses the runner)          | Released (as today)                                                                                                                                                     | **Not released**; the run waits for the resume (§11.3)                      |
| `release_locality_pins` (engine paused, or its pod work turned off) | Released (as today)                                                                                                                                                     | **Not released**; the run waits (§11.3, §23.4)                              |
| `Runner.revoke()`                                                   | Un-pinned (as today)                                                                                                                                                    | Assignment cleared; run failed or re-routed by project rule (§13.1)         |
| Manual release endpoint                                             | Un-pinned (as today)                                                                                                                                                    | **Refused**, HTTP 409 `dedicated_pin_not_releasable` (§12.2, H-13)          |
| `apply_run_resume_unavailable`                                      | Re-queued without the pin (as today)                                                                                                                                    | Re-queued **with** the pin (§9.3)                                           |
| `fresh_session`                                                     | No pin                                                                                                                                                                  | Pin kept (§9.2)                                                             |
| Runner pod move guard (`runner/views/runners.py`)                   | Blocks the move while the run is non-terminal (as today)                                                                                                                | Same                                                                        |

### 12.1 The sweep

Three paths on `main` drop pins (§5), and each gains `pin_kind = "locality"` in its filter (and in the re-check under the row lock): `_releasable_pins()` (`runner/tasks.py:184`), which serves both `release_pins_on_offline_runners` and `release_pins_for_paused_runner`, and `dispatch_rules.release_locality_pins` (`:125`), whose name already says what it may touch. That is the whole change for locality pins. No path releases a dedicated pin, however long its runner is silent or paused (H-13).

Two more passes in `release_pins_on_offline_runners` handle dedicated queued runs:

- **Stranded.** When the pinned runner is online, heartbeat-fresh, not paused and idle, the run is older than one sweep interval, and `filter_runs_usable_by_runner` excludes it, fail the run with `dedicated_runner_not_permitted` (§9.4). The runner is there and will never take the run, so waiting cannot help.
- **Expired.** A run older than `DEDICATED_RUN_QUEUED_MAX_AGE_SECS` (12 h) whose runner is unavailable is failed with `dedicated_runner_unavailable`, and the clock stops (§11.3).

### 12.2 Manual release

**Decision (product owner, 2026-11-06, H-13; replaces H-8): a waiting dedicated run is never released to the pod**, for every dedicated runner. The issue was dedicated because of what that runner has; a release would run it where the person ruled it out.

`POST /api/runners/runs/<run_id>/release-pin/` (`runner/views/runs.py:682`, `AgentRunReleasePinEndpoint`) keeps its contract for locality pins. For a dedicated pin:

- It is refused with HTTP 409, `{"error": "This run is dedicated to runner gpu-box and cannot be released. Change the issue's runner, or cancel the run and use Run on any runner in Default once.", "code": "dedicated_pin_not_releasable"}`, the `{"error", "code"}` shape the endpoint's other 409s use. The check is made on the locked row, next to the existing "run not queued" and "run not pinned" checks, after the existing 404s (no such run, or `_can_cancel_run` fails). Nothing changes on the run. (The endpoint's existing `executor_not_local` 409 for Cloud Agent runs is unrelated to the removed E6 and stays.)
- `dispatch_wait.releasable` (§14.3) is always `false` for a dedicated pin. For a locality pin it keeps today's rule, the endpoint's own permission check.
- The waiting banner (§14.4), the CLI (§14.5) and the MCP tools (§14.6) offer no release for a dedicated run. The `assignment` note of earlier revisions is dropped: no response needs it.

To end the wait, a person changes or clears the issue's runner (§13.2) or cancels the run. To have one piece of work done elsewhere without changing the issue, a person asks for a `pod_once` run (§23.3), which creates a new run and releases nothing. H-8's history: on 2026-10-21 the release was allowed for the issue's creator and current assignees; restricting cancel to the same people for every issue run is tracked outside this design, in PDASHOSS01-317.

## 13. Lifecycle of the assignment

### 13.1 Events

Every row that clears the assignment calls `clear_dedicated_runner(issue, reason=…)`, which sets `assigned_runner = NULL`, leaves `assigned_pod` alone, writes an activity entry, and posts one system comment naming the runner and the reason.

| Event                                                                  | Issue                                                                                     | Its queued run                                                                     | Its running run                                                    |
| ---------------------------------------------------------------------- | ----------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------- | ------------------------------------------------------------------ |
| Runner revoked (any reason, including the owner losing membership)     | Cleared. Then the project's lost-runner rule (below).                                     | Per the lost-runner rule (below)                                                   | Cancelled (today)                                                  |
| Runner deleted                                                         | Same; delete is revoke then delete. `SET_NULL` is the net.                                | Same                                                                               | Same                                                               |
| Runner moved to another pod of the same project                        | Follows the runner: `assigned_pod` is updated to the new pod in the move's transaction.   | Cannot exist: the move is refused while a run is bound (see below)                 | Cannot exist (same guard)                                          |
| Issue's pod changed to another pod, no runner in the request           | Cleared.                                                                                  | Cannot exist: pod changes are refused during an active run                         | —                                                                  |
| Issue's executor written (any value)                                   | Unchanged (§8). _The "executor changed → cleared" row was removed on 2026-11-06 with I2._ | —                                                                                  | —                                                                  |
| Issue moved to another project (`utils/issue_move.py`)                 | Cleared in the same `save(update_fields=…)` that resets `assigned_pod`.                   | Cancelled and replaced by the handoff run, unpinned (today)                        | Handoff as today                                                   |
| Runner's owner removed from the issue's assignees (and is not creator) | Not cleared (confirmed, H-9). The next run is bounced by preflight with a comment (§9.4). | Failed by the sweep as stranded (§12.1)                                            | Finishes                                                           |
| Runner offline or paused (owner pause, engine pause)                   | Unchanged. A person's new run is refused (§11.2).                                         | Waits; failed after 12 h if the runner is still unavailable, clock stopped (§11.3) | Offline: `fail_runs_on_offline_runners` as today. Paused: finishes |
| Sub-issue created under a dedicated issue                              | The child does **not** inherit. It gets the project default pod, like any new issue.      | —                                                                                  | —                                                                  |

The runner-move guard (`runner/views/runners.py`) refuses a move while a non-terminal run has `runner` or `pinned_runner` set to the runner. A `pod_once` run (§23.3) is not pinned and may be queued or running elsewhere in the old pod, so the guard gains one clause: the move is also refused while any issue dedicated to the runner has a non-terminal run. Without it the move would change `assigned_pod` under an active run, which the issue-side guard forbids.

**The revoke and delete rows (project owner, 2026-10-15, H-2).** The runner is gone for good, so waiting cannot help. `Runner.revoke()` reads `project.dedicated_runner_lost_action` (§6.6) for each issue dedicated to the runner, inside the revoke transaction, after it has cleared the assignment:

- **`fail_to_backlog` (default).** The issue's queued run is failed with `error_code = "dedicated_runner_revoked"` instead of being un-pinned. The issue moves to Backlog through the existing bounce (`orchestration/scheduling.py:_bounce_issue_no_eligible_runner`, reason `dedicated-runner-revoked`), whose system comment (a new message for this reason) names the runner and says it was revoked or deleted and that a person must choose a new runner or the pod. This applies whether or not the issue had a queued run: an issue in a ticking state is moved to Backlog so no tick creates a run on a squad the user did not choose. An issue in another state (Backlog, Todo, Done, …) only gets the cleared assignment and the comment.
- **`fall_back`.** The issue is routed to a squad, in this order:
  1. Its own pod, if that pod still has a runner that could serve the issue after this revoke. The queued run is un-pinned and served by the pod's shared queue (today's revoke behaviour).
  2. Otherwise the project's default pod (`Pod.default_for_project`), if it has such a runner. `assigned_pod` is set to it in the same transaction. `AgentRun.pod` is immutable, so the queued run is cancelled and, on commit, a replacement is created in the default pod through `_create_and_dispatch_run`, as the project-move handoff does.
  3. Otherwise the existing no-eligible-runner bounce applies: back to Backlog with its comment (reason `no-eligible-runner`).

  "A runner that could serve the issue" is the pod branch of `pod_has_runner_for_issue_principal(pod, issue, run.created_by_id, human=…)`, with `human` from the queued run's trigger: as on `main`, a non-revoked runner that is not `pinned_only`, owned by the run's creator, the issue's creator or a current assignee, and an engine only under its usage rule (§9.4, §23.2). A bare "non-revoked runner" check is not enough: a pod whose only other runners are `pinned_only`, engines that would not admit the run, or private to users not on the issue, would pass it, and the un-pinned run would sit in the shared queue that no runner serves, with no `dispatch_wait` (it has no pin) and no bounce. The same check is what the replacement run's preflight applies in step 2, so steps 2 and 3 cannot disagree.

  When the issue has no queued run, the same order applies with `run_creator_id=None` and `human=False` (the function then counts only the issue's creator and current assignees, the people a later automatic run would be checked for): step 2 only changes `assigned_pod` and creates no run, and step 3 moves the issue to Backlog only if it is in a ticking state, as for `fail_to_backlog`.

  The system comment names the runner, the reason, and the pod the issue now uses.

Both outcomes write the activity entry for the cleared runner (§14.7), and for a changed pod. The running run, if any, is cancelled as today in both cases.

Not inheriting on sub-issues is a decision, not an oversight: a split usually produces work of a different shape, and E7 could fail for a child created by an agent.

### 13.2 Changing the assignment while a run is active

Today a pod change is refused whenever the issue has an active run, because `AgentRun.pod` is immutable. A runner change inside the same pod does not have that problem while the run is still `QUEUED`. The rule, by the state of the issue's non-terminal run:

| Run state                                               | Change runner within the same pod, or clear it                               | Change the pod  |
| ------------------------------------------------------- | ---------------------------------------------------------------------------- | --------------- |
| No non-terminal run                                     | Allowed                                                                      | Allowed (today) |
| `QUEUED`                                                | **Allowed; the run is re-pointed**                                           | Refused (today) |
| `PAUSED_AWAITING_INPUT`                                 | Allowed; the paused run is not touched, the next run uses the new assignment | Refused (today) |
| `ASSIGNED`, `RUNNING`, `CANCEL_REQUESTED`, `AWAITING_*` | Refused, `active_run_in_progress`                                            | Refused (today) |

Re-pointing, inside `apply_execution_target`, in one transaction: lock the issue row, then the run row (the same issue → run order `complete_project_move_handoff` uses); re-check the run is still `QUEUED` with `runner IS NULL`; set `pinned_runner` and `pin_kind` to the new runner and `dedicated`, or to NULL and `locality` when the assignment is cleared; clear the parent's `thread_id`; on commit, `drain_pod` the run's pod. If the run was assigned between the read and the lock, the request fails with `active_run_in_progress`.

Moving a waiting issue to a runner in a _different_ pod is not supported in v1: cancel the run, then reassign.

## 14. Surfaces

### 14.1 Web app API (`apps/api/pi_dash/app/serializers/issue.py`)

- Write: `assigned_runner_id` (UUID or `null`), a `PrimaryKeyRelatedField(source="assigned_runner")` mirroring `assigned_pod_id`. The bare model field is excluded, as `assigned_pod` is.
- Read: `assigned_runner_id`, plus `assigned_runner_detail` on the detail serializer:

```json
{
  "id": "…",
  "name": "gpu-box",
  "status": "offline",
  "pod": "…",
  "owner": "…",
  "owner_display_name": "Ana",
  "host_label": "ana-desktop",
  "last_heartbeat_at": "2026-10-11T18:02:11Z",
  "is_busy": false,
  "queue_depth": 2
}
```

- Errors: HTTP 400 `{"assigned_runner_id": ["…"], "code": "runner_owner_not_on_issue"}` with the codes of §7.1.
- `assigned_runner_id` joins `assigned_pod_id` and `agent_executor` in the lite/list serializer so list, board and spreadsheet layouts can render it.
- The public space serializer (`space/serializer/issue.py`) does **not** expose the runner.

`assigned_runner_detail` shows a runner's name, status and owner to everyone who can see the issue, although the runner list is owner-only. Run payloads already show `runner_name` the same way. **Decision (project owner, 2026-10-15, H-6): confirmed, shown to everyone who can see the issue.** An issue that says "waiting for a runner" without saying which one is not useful.

### 14.2 v1 API (`apps/api/pi_dash/api/serializers/issue.py`)

- `assigned_runner` (UUID or `null`) on create and update, mirroring the v1 `assigned_pod` field name; returned on reads together with `assigned_runner_detail`.
- Same service, same codes.
- The runner list of PDASHOSS01-245 gains `assignable_for` (§7.4).

### 14.3 The waiting state

Computed, not stored. The run serializers (`AgentRunSerializer`, and `_serialize_agent_run` behind the issue's `agent_status`) add three fields to every run — `pinned_runner`, `pinned_runner_name`, `pin_kind` — and, for a `QUEUED` run with a pin, a `dispatch_wait` object (otherwise `null`):

```json
"dispatch_wait": {
  "reason": "runner_offline",
  "pin_kind": "dedicated",
  "runner": "…", "runner_name": "gpu-box",
  "queued_at": "2026-10-11T20:14:03Z",
  "runner_last_seen_at": "2026-10-11T18:02:11Z",
  "runs_ahead": 0,
  "pause_reason": null,
  "expires_at": "2026-10-12T08:14:03Z",
  "releasable": false
}
```

`reason` is one of `runner_offline` (§11.1), `runner_paused` (§11.1 and §23.5, for every runner; wins over `runner_offline`), `runner_busy` (the runner has a busy run, older runs in its personal queue, or — when its last run was dedicated — a shared run it takes first under §10.2; `runs_ahead` counts them), `runner_not_permitted` (§9.4), or `dispatching` (the runner is free and the drain has not run yet).

- `pause_reason` is set only with `runner_paused`, from the list in §11.2 (`dispatch_paused`, `owner_paused`, or the machine's `engine_auto_pause_reason`); otherwise `null`.
- `expires_at` is `queued_at` plus `DEDICATED_RUN_QUEUED_MAX_AGE_SECS` for a dedicated pin: the time after which the run fails if its runner is still unavailable (§11.3). `null` for a locality pin.
- `releasable` tells the client whether the requester may release the pin. It is always `false` for a dedicated pin (H-13, §12.2); for a locality pin it follows the release endpoint's permission check.

This also makes waiting on a locality pin visible, which it is not today. `apps/web_new` reads none of `pause_reason`, `expires_at` and the `runner_paused` case yet (M3).

### 14.4 Web picker (`apps/web`)

- `dropdowns/pod/base.tsx` becomes a two-level picker. Each pod is a group: its first option is "Any runner in <pod>" (pod assignment, today's value); under it, the pod's runners that the viewer can see. There are no Cloud Agent or Pi Dash Agent peers: since #2903 `execution-target.ts` hides executor options marked `deprecated` (§23.6).
- Each runner row: name, status line (§10.1), owner. A runner with `assignable = false` is disabled and shows the reason for its code.
- `execution-target.ts`: the dropdown value becomes a tagged value (`pod:<id>` or `runner:<id>`). `executionTargetPatch` returns `{assigned_pod_id, assigned_runner_id}` for a runner and `{…, assigned_runner_id: null}` for a pod; it may keep sending `agent_executor: "local_runner"`, which the server normalizes independently (§8).
- Collapsed label: "gpu-box" with the pod as secondary text; when waiting, the `dispatch_wait` reason as a badge.
- It appears wherever the pod is shown: the create modal (`issue-modal/components/default-properties.tsx`), the detail sidebar, peek view, and the list, board and spreadsheet property columns.
- Waiting banner in the agent-status area of the issue: "Waiting for runner **gpu-box** — offline, last seen 2 h ago. Fails at 08:14 if it does not reconnect." (the last sentence from `expires_at`, only while the runner is unavailable). Actions for a dedicated run: **Change runner** (opens the picker; §13.2) and **Cancel run**. There is no release action for a dedicated run (H-13); a locality pin keeps the release action when `releasable` is true. `apps/web_new` still shows the release action (M7).
- Refusal of a person's run (§11.2, and H-10's `assigned_runner_not_owner`, §23.3): Run AI, Comment & Run and a state move show the reason in place ("Built-in engine · laptop is paused: the wallet is empty.") with **Run on any runner in <pod> once** (retries with `pin: "pod_once"`; after a state move it sends Run AI with `pod_once`) and **Change runner**; for `assigned_runner_not_owner` also **Run on my engine instead**.
- Errors from §7.1 are shown in place, next to the picker.
- **AI Workers project settings page** (new; project owner, 2026-10-15). Settings → Projects → <project> → **AI Workers**, a new entry `ai_workers` in `PROJECT_SETTINGS` (`packages/constants/src/settings/project.ts`) next to `schedulers`, at `settings/projects/[projectId]/ai-workers/`. No such page exists today. Its first and only v1 setting is "When a dedicated runner is revoked or deleted": **Fail the run and move the issue to Backlog** (default) or **Fall back to the pod**, with one line under each saying what happens (§13.1). It writes `dedicated_runner_lost_action` (§6.6) through the project PATCH; members who cannot edit project settings see it read-only.
- `desktop-overlay/` shares the dropdown; it must build and render the new value shape.

### 14.5 CLI (`runner/src/cli/issue.rs`)

- `pidash issue create|patch … --runner <name-or-UUID>`; names resolve within the issue's project, among runners the caller can see; an ambiguous name is an error that lists the matches.
- `pidash issue patch … --clear-runner` returns the issue to "any runner in the pod". `--runner` and `--clear-runner` are mutually exclusive.
- `pidash issue create|patch … --pod <name-or-UUID>` — **added scope.** The CLI cannot set a pod today, and the coordinator's "done" needs both modes from the CLI. `--pod` without `--runner` clears the runner (§13.1); with `--runner` the two must agree.
- `pidash issue get` prints `assigned_pod`, `assigned_runner`, `assigned_runner_detail`, and `agent_status.active_run.dispatch_wait` when present.
- `pidash project runners --assignable-for <issue | new>` (extends PDASHOSS01-245) prints `assignable`, the reason, `is_busy` and `queue_depth`.
- There is no release for a dedicated run (H-13, §12.2). To move the issue, use `--runner <other>` or `--clear-runner`.
- The CLI's run commands accept `--pod-once`, which sends `pin: "pod_once"` (§23.3). A refusal (`assigned_runner_offline`, `assigned_runner_paused`, `assigned_runner_not_owner`) prints the reason, the `pause_reason` and the `options`, and exits non-zero.
- Validation failures print the API's `code` and message in the usual `{"error": …}` envelope.

### 14.6 MCP connector and in-run tools

- `pidash_create_issue` / `pidash_update_issue` (private repo, `pi_dash_cloud/mcp/tools.py`): new optional inputs `assigned_runner` (UUID or name; `null` clears) and `assigned_pod` (UUID or name) — the pod input is added scope for the same reason as the CLI flag. `pidash_get_issue` returns both plus `assigned_runner_detail` and `dispatch_wait`.
- `pidash_list_project_runners` (PDASHOSS01-246) gains `assignable_for`.
- The run tools accept `pin: "pod_once"` and return the refusal body of §11.2 unchanged. No tool releases a dedicated run (H-13).
- In-run cloud-agent tools (`apps/api/pi_dash/cloud_agent/tools.py`): `pidash_create_project_issue`, the only tool there that creates an issue today, accepts `assigned_runner` and `assigned_pod` through the same service. The acting user is the run's creator, so E1 and E7 apply to that user. An in-run update tool, if one is added later, uses the same service.

### 14.7 Issue activity

`assigned_runner` and — added scope — `assigned_pod` get trackers in `bgtasks/issue_activities_task.py:ISSUE_ACTIVITY_MAPPER`, with the pod and runner names as old and new values. System-initiated changes (§13.1) are attributed to the system actor and carry the reason.

### 14.8 Prompting and docs

`prompting/sections/pidash-cli.md` and `docs/mcp-connector.md` document the flags and inputs, and tell agents:

- Leave an issue on its pod unless the work needs something only one runner has. A dedicated runner is a bottleneck by design.
- A sub-issue does not inherit its parent's runner. Set it on the child only if the child needs it.
- Check `--assignable-for` before choosing; do not choose a runner that is not assignable.
- A run that reports `dispatch_wait` is waiting, not broken. Do not create another run or another issue for it.
- A run refused with `assigned_runner_offline` or `assigned_runner_paused` is not an error to retry in a loop. Use `--pod-once` only when the work does not need the dedicated runner's machine; otherwise report that the runner is unavailable and stop.

## 15. Schedulers

**Decision: follow-up, not v1.** `SchedulerBinding.pod` stays squad-only.

A scheduler binding has no issue, so `Issue.assigned_runner` does not reach it. The follow-up is a nullable `SchedulerBinding.runner` beside `pod`, resolved late like the pod, feeding the same `PinKind.DEDICATED` in `dispatch_scheduler_run`. The sweeps (including the 12-hour bound for automatic runs, §11.3), the release refusal and `dispatch_wait` of this design then apply without change, and `filter_runs_usable_by_runner` already lets a private runner, and an engine, take scheduler runs its owner authored. HA-6 files the issue.

## 16. Runner specialities

**Decision: v1 picks by identity.** The picker shows what the cloud already knows that helps a person choose: name, owner, `host_label`, `os`/`arch`, the dev machine, and the agent and model from `dev_metadata`.

How labels build on this later, without changing the model of this design:

1. `Runner.capabilities` (a JSON list, already on the model and in `RunnerSerializer`) becomes the home of user-visible labels such as `gpu`, `vpn`, `ios-signing`: reported by the daemon and editable by the owner.
2. The picker and `--assignable-for` filter and display by label. "Assign to a runner with label `gpu`" is a client-side resolution to one runner, stored in the same `assigned_runner` field.
3. A true third mode — "any runner in the pod that has label X" — is a different feature: it needs a requirement on the issue and a filter in `next_for_runner`. `AgentRun.required_capabilities` already exists on the run side for it. That is a model change and is out of scope here; this design neither needs it nor blocks it.

## 17. Other codebases

None is changed here. Follow-up issues are filed in their projects and linked from PDASHOSS01-295.

- **Rust port** (`rust-api/`, on `main`). It is built in the same issues as the Django work, as each issue's New-system PR, following the Pi Dash OSS double-write rule. It is not a separate NEWPIDASH issue. Rules for the HA work there:
  - **Django schema first.** The Django migrations of §6.5 land first; the Rust port adds no migration and reads the new columns through SeaORM entities.
  - **SeaORM for reads and writes.** No new raw SQL (the `rust-api-raw-sql-guard` workflow ratchets it); domain write services own the writes to OSS tables.
  - **Standalone.** The Rust API implements every rule itself and forwards nothing to Django.
  - **Scope:** the new columns (including `Project.dedicated_runner_lost_action`, §6.6) and their constraint, the matcher's engine eligibility and alternation (§9.3, §10.2), the shared validation rules and codes (§7.1), the pin resolver and the phase table (§9), the refusal of a person's run and its codes (§11.2), the 12-hour bound and its clock stop (§11.3), the sweep split (§12.1), the release refusal (§12.2), the lifecycle hooks (§13), the `dispatch_wait` contract (§14.3), and §23. The port must not fall behind on the matcher guarantees in §18.
  - It builds on the Rust port of #2903's dispatch rules (`pod_dispatch`, the engine usage rule, the machine pause fields), which at `0e5b395dc` is not yet in `rust-api/` and is tracked separately.
- **New frontend** (`apps/web_new`, project NEWFRONT): the two-level picker, the tagged value, the waiting banner and its two actions for a dedicated run (§14.4, M7), the refusal UI and `pod_once` (§14.4, §23.3, M3), the new fields in its issue types, and the AI Workers project settings page with the lost-runner rule (§6.6, §14.4).
- **Private repo** (project PRIVATEPI1): the MCP tool inputs and outputs (§14.6), and a check that the cloud overlay's copies of the picker and settings code do not drift from the OSS dropdown.

## 18. Tests

| Layer                            | What                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                          | Where                                                                             |
| -------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------- |
| Model                            | The issue check constraint (I1's pod is set) and the run constraint (I5) reject bad rows; no constraint ties the runner to the executor; `SET_NULL` on runner delete; defaults on existing rows, including `fail_to_backlog` on projects.                                                                                                                                                                                                                                                                                                                                                                                                                                     | `tests/unit/runner/test_issue_assigned_runner.py`                                 |
| Shared service                   | One test per E-code (E6 removed); pod follows runner; an executor write, including a translated legacy one, leaves the runner unchanged; re-point on `QUEUED`; refusal past `QUEUED`; allowed on `PAUSED_AWAITING_INPUT`; the race where the run is assigned mid-request.                                                                                                                                                                                                                                                                                                                                                                                                     | `tests/unit/runner/test_issue_assignment_service.py`                              |
| Contract, both API surfaces      | Set, change, clear; every error code and shape; `assigned_runner_detail`; `assignable_for`; a pod-only issue serializes as before plus two null fields.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                       | `tests/contract/app/`, `tests/contract/api/`                                      |
| Pin resolver and phases          | One test per row of §9.2, including `fresh_session` keeping the pin, an offline or paused runner still being pinned for an automatic run, and `pod_once` giving no pin; stale assignment self-heals; the locality rules of `_pinned_runner_for` hold unchanged.                                                                                                                                                                                                                                                                                                                                                                                                               | `tests/unit/orchestration/test_service.py`, `test_scheduling.py`                  |
| Matcher                          | A dedicated run is never returned to another runner by `next_for_runner`, `next_queued_run_for_pod`, `drain_pod` or `drain_for_runner`; it goes to its runner even when another is idle and its own is busy; a `pinned_only` runner, own-agent or engine, takes its dedicated runs; a paused runner takes none; personal queue is FIFO across pin kinds. **Existing matcher and pin tests pass unmodified.**                                                                                                                                                                                                                                                                  | `tests/unit/runner/test_matcher.py`, `test_drain_pod.py`                          |
| Sweep and release                | Locality pins released as today by all three paths; dedicated pins never released by `release_pins_on_offline_runners`, `release_pins_for_paused_runner` or `release_locality_pins`; stranded run failed; a dedicated run whose runner is unavailable is failed after 12 h with `dedicated_runner_unavailable`, the failure comment and a stopped clock (`STOP_SIGNAL`), and not before; a run behind a busy online runner is not failed; a runner back before the bound is re-checked under the lock and keeps its run; release-pin on a dedicated pin returns 409 `dedicated_pin_not_releasable` and changes nothing; `apply_run_resume_unavailable` keeps a dedicated pin. | `tests/unit/runner/test_offline_runner_recovery.py`, `test_runs_views.py`         |
| Matcher alternation              | After a dedicated run the runner takes the oldest shared run, then a dedicated one; either side empty → the other; locality pins first in both orders; no dedicated runs → today's order; a released run counts as shared.                                                                                                                                                                                                                                                                                                                                                                                                                                                    | `tests/unit/runner/test_matcher.py`                                               |
| Lifecycle                        | One test per row of §13.1: revoke and delete under each lost-runner rule (fail to Backlog; fall back to own pod, to the default pod, and bounce when neither has a runner), runner pod move (and its refusal while a dedicated issue has a run), issue pod change, executor change, project move, assignee removal.                                                                                                                                                                                                                                                                                                                                                           | `test_revocation.py`, `test_runner_pod_move.py`, `test_issue_move_run_repoint.py` |
| Preflight, refusal, waiting      | Bounce with the dedicated reason; a person's Run AI, Comment & Run, direct run and state move refused with `assigned_runner_offline` and with `assigned_runner_paused` for each `pause_reason`, for own-agent and engine runners, nothing queued, clock unchanged, the comment saved and the state move kept; `pod_once` bypasses the refusal; creation-time comment posted once for an automatic run; `dispatch_wait` for each `reason`, with `pause_reason`, `expires_at` and `releasable: false` for a dedicated pin.                                                                                                                                                      | `tests/unit/orchestration/test_eligibility_preflight.py`                          |
| Web                              | Option building, disabled states and reasons, tagged value round-trip, hydration of the selected runner, the waiting banner's actions (no release for a dedicated run), the refusal notice and its `pod_once` retry.                                                                                                                                                                                                                                                                                                                                                                                                                                                          | `apps/web/tests/issues/execution-target*.test.ts`                                 |
| CLI                              | Argument mapping for `--runner`, `--clear-runner`, `--pod`; name resolution and ambiguity; error passthrough.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                 | `runner/` unit tests                                                              |
| MCP                              | The new inputs and outputs.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                   | private repo                                                                      |
| End to end, two runners, one pod | The six scenarios in HA-6, with the offline scenario asserting H-11 (a person's run refused with the `pod_once` offer, a tick waiting and failing after 12 h with the clock stopped, a runner back in time draining its queue first), the revoke scenario asserting both lost-runner rules of §13.1, and a busy-runner scenario asserting the alternation of §10.2.                                                                                                                                                                                                                                                                                                           | HA-6                                                                              |

## 19. Rollout

**Decision (project owner, 2026-10-15, H-5): no rollout gate.** There is no `DEDICATED_RUNNER_ASSIGNMENT_ENABLED` setting and no `dedicated_runner_disabled` error code. Instead the merges are ordered so that a saved choice is never ignored.

The schema is additive and NULL means "today", so the migrations ship at any time. The risk is a window in which the API accepts `assigned_runner` but dispatch does not honour it: a saved choice that dispatch ignores would be a lie. The order:

1. **HA-2a (PDASHOSS01-304) — model, migrations and read fields** (§6, the read side of §14.1 and §14.2, §14.7 for `assigned_pod`). Nothing can set `assigned_runner` yet; every row stays NULL. The project field of §6.6 is writable here: it changes nothing until a runner can be dedicated.
2. **HA-3 — dispatch and lifecycle** (§9 to §13), built against the model of HA-2a and tested by setting the field directly in tests.
3. **HA-2b (PDASHOSS01-305) — the write path for `assigned_runner`** (§7, §8, §13.2, the write side of §14.1 and §14.2) lands only after HA-3's dispatch is on `main`.
4. HA-4 and HA-5 build on HA-2b. HA-6 last.

This order is recorded as relations, not only in this text: HA-2b is `blocked_by` HA-3, and HA-3 is `blocked_by` HA-2a (not the whole of HA-2). Merging HA-2b and HA-3 in one PR also satisfies the rule.

- No other setting is added. In particular there is no offline-fallback setting in v1 (§11).
- HA-6 runs the migrations against a copy of production data and records the lock time of the constraint validations and the concurrent index builds.
- Rollback: revert HA-2b. Existing assignments keep routing, because dispatch reads the field whoever wrote it. A full rollback also clears `assigned_runner` on every issue with a data migration.

## 20. Epic map

**Issues (2026-11-06).** The epics below are replaced by three issues by layer, each built as an Old-system PR and a New-system PR:

- PDASHOSS01-297 is the backend: HA-2 and HA-3, plus the backend parts of HA-6.
- PDASHOSS01-349 is the UI in `apps/web` and `apps/web_new`: HA-4.
- PDASHOSS01-350 is the CLI, MCP and prompting: HA-5, plus the docs part of HA-6.

The hosted MCP tools are PRIVATEPI1-117. The table is kept as the map of sections to scope.

| Epic                        | Implements                                                                                                                                                  |
| --------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------- |
| HA-2 data model and API     | HA-2a: §6 (incl. §6.6), read side of §14.1 and §14.2, §14.7. HA-2b: §7, §8, §13.1 (the issue-side rows), §13.2, write side of §14.1 and §14.2. Order in §19 |
| HA-3 dispatch and lifecycle | §9, §10.2 (alternation), §11 (the refusal of a person's run and the 12-hour bound), §12, §13.1 (the runner-side rows, incl. the lost-runner rule), §14.3    |
| HA-4 web UI                 | §10.1, §10.3, §14.4 (incl. the AI Workers settings page)                                                                                                    |
| HA-5 CLI, MCP, prompting    | §14.5, §14.6, §14.8                                                                                                                                         |
| HA-6 tests, docs, rollout   | §18 (end to end), §19, the follow-ups of §15, §16 and §17                                                                                                   |

Scope this design adds to the epics as first written: `--pod` on the CLI and `assigned_pod` on the MCP tools (HA-5); activity tracking for `assigned_pod` (HA-2); `dispatch_wait` for locality pins as well (HA-3); the extra clause on the runner-move guard (HA-3, §13.1); re-pointing a queued run when the assignment changes (HA-2); from the 2026-10-15 decisions, the project lost-runner setting and its API (HA-2) and lifecycle (HA-3), the AI Workers settings page (HA-4), the matcher alternation (HA-3), and the HA-2a/HA-2b split with its merge order (§19). From the 2026-11-06 decisions it adds the refusal codes and the 12-hour bound (H-11, HA-3) and the `pod_once` option for every dedicated runner (HA-3, HA-4, HA-5). Scope it removes: the temporary rollout setting (H-5); a separate "list eligible runners" endpoint (a parameter on the existing list instead, §7.4); a separate CLI release command (§12.2); the release of a dedicated run on every surface (H-13); the I2 constraint and E6 (2026-11-06).

## 21. Decisions

H-1 by the project owner on 2026-10-12; H-2 to H-6 on 2026-10-15; H-7 to H-9 on 2026-10-21. H-10 was decided on 2026-11-04 as PO-10 of the engine design and moved here on 2026-11-05. H-11 to H-14 were decided by the product owner on 2026-11-06 (§23.8); H-11 and H-13 apply to every dedicated runner and supersede H-1 and H-8, whose rows are kept for history.

| #    | Question                                                                                         | Decision                                                                                                                                                                                                                                                                                                                                           | Section      |
| ---- | ------------------------------------------------------------------------------------------------ | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------ |
| H-1  | Dedicated runner offline with a run queued                                                       | _Superseded by H-11 (2026-11-06)._ Was: wait, visibly; no automatic fallback, no timeout.                                                                                                                                                                                                                                                          | §11          |
| H-2  | Dedicated runner revoked or deleted                                                              | A project setting (§6.6) on a new AI Workers page. Default: fail the queued run, move the issue to Backlog, comment. Alternative: fall back to the pod, then the default pod, then the existing bounce.                                                                                                                                            | §6.6, §13.1  |
| H-3  | Dedicated work versus squad work on the same runner; queue limit                                 | Strict 1:1 alternation between dedicated runs and the shared queue; locality pins keep first priority; no cap; queue depth shown and warned.                                                                                                                                                                                                       | §10          |
| H-4  | Who may dedicate an issue to a runner                                                            | Confirmed: the runner's owner, who is also the issue's creator or a current assignee. Otherwise rejected; the owner is not auto-added.                                                                                                                                                                                                             | §7.1         |
| H-5  | Rollout gate                                                                                     | No gate. HA-2's write path lands after HA-3's dispatch, recorded as `blocked_by` relations.                                                                                                                                                                                                                                                        | §19          |
| H-6  | Showing a dedicated runner's name, status and owner on the issue                                 | Confirmed: shown to everyone who can see the issue.                                                                                                                                                                                                                                                                                                | §14.1        |
| H-7  | Who may change or clear a dedicated runner someone else set                                      | Any creator or current assignee, whoever set it. No rule protects the previous choice.                                                                                                                                                                                                                                                             | §7.1         |
| H-8  | Who may release a waiting dedicated run to the pod                                               | _Superseded by H-13 (2026-11-06)._ Was: only the issue's creator and current assignees. (Cancel for every issue run is restricted the same way in PDASHOSS01-317, outside this design.)                                                                                                                                                            | §12.2        |
| H-9  | The dedicated runner's owner is removed from the assignees                                       | Confirmed as written: the assignment is not cleared; a queued run is failed as stranded and the next run is bounced to Backlog.                                                                                                                                                                                                                    | §13.1        |
| H-10 | A human run on an issue dedicated to someone else's engine, which the engine usage rule excludes | Refused at creation with `assigned_runner_not_owner`, offering "Run on my engine instead" and "Run on any runner in <pod> once". Decided as the engine design's PO-10.                                                                                                                                                                             | §23.3        |
| H-11 | A dedicated runner (any kind) offline or paused                                                  | A person's run is refused at creation with `assigned_runner_offline` or `assigned_runner_paused` (with `pause_reason`), nothing queued, clock not armed, offering `pod_once` and Change runner. Automatic runs wait pinned and fail with `dedicated_runner_unavailable` after 12 h; the clock stops. A runner that returns drains its queue first. | §11, §23.4   |
| H-12 | Engine removed or revoked while issues are dedicated to it                                       | H-2 applies to engines as written: the project's `dedicated_runner_lost_action`.                                                                                                                                                                                                                                                                   | §13.1, §23.4 |
| H-13 | Releasing a waiting dedicated run (any kind)                                                     | Never. `release-pin` refuses a dedicated pin with 409 `dedicated_pin_not_releasable`; `releasable` is always false; no release on any surface. Locality release unchanged. `pod_once` stays, for the issue's creator and current assignees.                                                                                                        | §12.2, §23.3 |
| H-14 | The changes to this design for engine runners                                                    | §23.1 to §23.7 adopted.                                                                                                                                                                                                                                                                                                                            | §23          |

## 22. Deferred

- Automatic fallback to the pod when the dedicated runner is offline: an instance-wide setting (§11, option b) or a per-issue choice (§11, option c). Either would first need H-13 reopened (§11.4).
- A dedicated runner on scheduler bindings (§15).
- Capability labels and routing by capability (§16).
- Moving a waiting issue to a runner in another pod without cancelling the run (§13.2).
- A cap on a runner's personal queue (§10.1).
- Bulk-editing the dedicated runner across issues.
- Runners that belong to no pod (§3).
- Many-to-many membership between runners and pods (§3). v1 keeps one pod per runner.

## 23. Engine runners (built-in engine)

Added on 2026-11-05. The engine design (`.ai_design/built_engine_as_runner/design.md`, PIDESKAPP-121, "ENGINE" below) makes the built-in engine an ordinary runner: a `Runner` row with `provisioning = desktop_bundled` per machine and project, hosted by the Pi Dash CLI daemon, billed to its owner's wallet. ENGINE revision 4.0 gives an engine issue work only through its pod and moves everything about dedicating an issue to one engine here. ENGINE revisions 3.3 and 3.4 are where this section comes from.

**ENGINE is merged and implemented**: the design in #1496, the server and web part in #2903 (squash `c0ee3148b`), the CLI, daemon and desktop part in `38a95820f`. This section builds on ENGINE's concepts as they are on `main` at `0e5b395dc`: `Runner.pod_dispatch` (`pinned_only` / `pod_member`, `runner/models.py:625`; new engines start `pinned_only`), the engine usage rule (ENGINE section 8: an engine takes only runs its owner created, plus automatic runs on the owner's issues and schedulers, unless `DevMachine.engine_team_runs` is on; `permissions.filter_runs_usable_by_runner` and `dispatch_rules.runner_may_take`), the machine pause fields `DevMachine.engine_paused_by_owner` and `engine_auto_pause_reason` (`runner/models.py:434-444`), the removal of the executor choice, and the merge endpoint. Where this section says nothing, the rest of this design applies to engines as written; H-11 and H-13, decided for every dedicated runner, are in §11 and §12.2.

### 23.1 Engines can be dedicated runners

- E5 and I3 are lifted for engine runners. I3 becomes "never a revoked runner"; the code `runner_desktop_bundled` is removed. An engine on a legacy desktop machine (an older app build) can be chosen too.
- E1 and E7 are unchanged, so only the engine's owner can dedicate an issue to it, on an issue they created or are assigned to. A runner is visible only to its owner, so nobody else can pick someone's engine.
- Every phase is pinned (§9.1, §9.2), whatever the engine's `pod_dispatch`: a `pinned_only` engine takes the runs dedicated to it, and only those. Automatic runs (ticks) on an issue dedicated to it are admitted under the usage rule.

### 23.2 The engine usage rule where this design tests "not `desktop_bundled`"

- **Matcher (§9.3).** On `main` (ENGINE section 7) an engine takes its legacy managed pins and, while `engine_takes_pod_work`, its locality pins and the pod's shared queue, all under the usage rule. HA adds the runs dedicated to it, whatever its `pod_dispatch`, also under the usage rule; a paused engine takes none, because `Runner.objects.assignable()` excludes it. A `pinned_only` engine serves no shared queue, so under H-3 it works through its dedicated queue in order.
- **Preflight (§9.4) and `fall_back` (§13.1).** "Not `desktop_bundled`" is replaced, for engine runners only, by the engine usage rule, which also reads the trigger (human versus automatic, `HUMAN_TRIGGERS` in `apps/api/pi_dash/runner/models.py:327-334` at `0e5b395dc`; the function's `human` argument). On `main` the pod branch already does this; the dedicated branch of §9.4 calls `runner_may_take`, which applies the same rule to one runner. `pod_has_runner_for_issue_principal` is the one function both use, so they cannot disagree.
- **Stranded sweep (§12.1).** It also fails a queued engine run the usage rule stops admitting while it waits, for example when the owner turns the opt-in off or is removed from the assignees.

### 23.3 A teammate's run on an issue dedicated to an engine (H-10), and `pod_once` for every dedicated runner

This design creates a run the dedicated runner will not take whenever the runner's owner is on the issue, and the stranded sweep fails it later. For an engine that would spend nothing but would confuse the teammate, so:

- A human-triggered run by someone the engine usage rule excludes is refused at creation with `assigned_runner_not_owner`, and nothing is queued. The response is the 409 body of §11.2 with `reason = "assigned_runner_not_owner"`, `pause_reason = null` and `options = ["run_on_my_runner", "pod_once"]`. This check runs before H-11's availability check (§11.2), in the same `dedicated_run_refusal`. This covers a teammate's Run, Run AI or Comment & Run, and a teammate's state move into a ticking state, on an issue dedicated to the owner's engine while the opt-in is off.
- The UI offers **Run on my engine instead**, which re-assigns the issue to the caller's own runner under E1 and E7 (allowed by H-7), and **Run on any runner in <pod> once**, defined below.
- **Run on any runner in <pod> once**, for **every** dedicated runner, own-agent or engine (generalised on 2026-11-06 with H-11 and H-13). It is the one way to have a dedicated issue's work done elsewhere without changing the issue, and it creates a new run; it releases nothing (H-13). It is a request option on run creation: `"pin": "pod_once"` on the Run AI, Comment & Run and direct-run requests. The refusals of §11.2 and of this section name it in `options`, so the UI and the CLI can retry with it. After a refused state move, the button sends Run AI with `pod_once`; the state move itself already stands.
  - **Who:** the issue's creator and its current assignees (live `IssueAssignee` rows). This is its own rule, not borrowed from a release. Anyone else gets HTTP 403 `pod_once_not_allowed`.
  - **When:** only on an issue with a dedicated runner. Otherwise HTTP 400 `pod_once_not_applicable`. Only when the issue has no active run, as for any new run: otherwise the existing refusal `active_run_exists`; a waiting dedicated run is cancelled first (§10.3). It applies to this request's run only: `Issue.assigned_runner` is unchanged, so the run's own continuations, the next tick and every later hand-off are dedicated again (§9.2).
  - **The pin:** `resolve_run_pin` (§9.1) gains `pod_once: bool`. When it is set, the dedicated branch is skipped and the resolver returns `(None, PinKind.LOCALITY)`: no pin of either kind. The parent's `thread_id` is cleared, so whichever runner takes the run starts a fresh session.
  - **Preflight:** the run is created in `issue.assigned_pod`, the dedicated runner's pod (I1). `pod_has_runner_for_issue_principal` runs its **pod** branch for the caller, not the dedicated branch, with the engine usage rule (§23.2), so an engine in that pod counts only as a pod member that admits the caller. When no runner could serve, the request is refused with HTTP 409 `no_eligible_runner_in_pod`, nothing is queued, and the issue is **not** bounced to Backlog: it is still dedicated, and only this one-off failed. The clock rule below applies to this refusal too.
  - **Record:** the issue gets one system comment: "Run started on any runner in pod **Default** by Ana, for this run only. The issue stays assigned to **Built-in engine · laptop**."
  - **Rust:** the same option, codes and resolver branch in the `rust-api/` run-creation handlers (`crates/api/src/runner_runs/dispatch.rs` and the v1 run-AI and comment-and-run actions).
- For Comment & Run the comment is saved and the issue gets one system comment saying why no run started. For a state move the move stands and gets the same system comment.
- **A refused run does not arm the clock**, for this refusal and for H-11's (§11.2), on every dedicated runner. Every human event reaches `scheduling.reconcile` before the run is created: in `handle_issue_state_transition` at `apps/api/pi_dash/orchestration/service.py:245`, with the preflight at `:288`; in `handle_issue_comment` at `:419`, before `_create_continuation_run` (all at `0e5b395dc`). A refused state move or Comment & Run would otherwise leave the issue ticking, and the next tick, an automatic run, would start the very run the refusal stopped: on an engine because the usage rule admits automatic runs on the owner's issue, on an unavailable runner because automatic runs are created and wait (§11.3). The refusal check (`dedicated_run_refusal`) therefore runs **before** `reconcile`: when the issue is dedicated and the check refuses the event's human run, the event is applied to the issue (the state changes, the comment is saved) but not to the clock, which keeps its previous state. A clock that was already armed keeps ticking; its automatic runs follow §11.3. Run AI already rolls the clock back when no run is created (`run_ai_for_human`), so there the check may sit with the dispatch.
- The Rust counterpart is the human-run reconcile in `rust-api/crates/api/src/runner_runs/dispatch.rs` (`clock::reconcile` at `:1657` at `0e5b395dc`); the check goes before it, and in the Rust state-transition path the same way.

### 23.4 Lifecycle for engines

- **Offline or paused (§11, H-11).** As for every dedicated runner: a person's run is refused with `assigned_runner_offline` or `assigned_runner_paused` (with the engine's `pause_reason`, for example `wallet_empty` or `engine_sign_in_required`), offering `pod_once` and Change runner; an automatic run is created, waits, and fails with `dedicated_runner_unavailable` after 12 hours while the engine is still unavailable, which stops the clock. Creation of a waiting automatic run posts the §9.4 comment naming "Built-in engine · <machine>". Session open, and clearing the engine pause (`on_engine_pause_changed`), drain the personal queue first, as for any runner. Pausing the engine releases its locality pins (`release_locality_pins`) and leaves its dedicated pins (§12.1).
- **Removed or revoked (§13.1, H-12).** "Remove from project" and "Remove this computer" revoke the engine runner, so the project's `dedicated_runner_lost_action` applies.
- **Release (§12.2, H-13).** Never, as for every dedicated runner. A `pod_once` run (§23.3) goes to the pod's shared queue, where an engine takes it only as a pod member and under the usage rule.
- **Resume and busy rejection (§9.3).** `apply_run_resume_unavailable` keeps a dedicated pin, so a cancellation race cannot send the run to another machine.
- **Merge of two machines.** ENGINE's merge endpoint retires the source machine's engine runner when both machines have one for the same project. Once this design is built, that retirement first moves each issue dedicated to the source runner to the survivor. This is a **lifecycle re-assignment**, like the self-heal and lost-runner paths of §13.1, not a user write. So it does not run `apply_execution_target`'s request validation (E1 to E8), which could refuse it for reasons the merge does not cause:
  - **Eligibility is carried over, not re-checked.** The merge requires one owner for both machines, so the survivor's owner is the source's owner, and E1 holds by construction. E7 is **not** re-checked. If the owner has already left the issue's assignees, H-9 kept the assignment against E7 on purpose, and the merge keeps it the same way. H-9's consequences then apply to the survivor exactly as they applied to the source: the next run is bounced by preflight (§9.4), and a queued run is failed as stranded (§12.1). The merge checks only what it could break itself: E2 (the survivor's pod is in the issue's project) and E4 (the survivor is not revoked). When either fails, the issue is handled as for a lost runner, by the project's `dedicated_runner_lost_action` (§13.1), and the merge goes on.
  - **Runs by state** (the §13.2 table, applied by the merge with the system actor):

    | Source runner's run on the issue                        | Survivor in the same pod                                                                                                                                                                                                                                                                                 | Survivor in another pod of the project                                                                                                                                                                                                     |
    | ------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
    | None                                                    | Assignment moved                                                                                                                                                                                                                                                                                         | Assignment and `assigned_pod` moved                                                                                                                                                                                                        |
    | `QUEUED`                                                | Re-pointed (§13.2)                                                                                                                                                                                                                                                                                       | Cancelled, and on commit recreated in the survivor's pod through `_create_and_dispatch_run`, the hand-off `fall_back` uses                                                                                                                 |
    | `PAUSED_AWAITING_INPUT`                                 | Not touched; the reply that resumes it runs on the survivor                                                                                                                                                                                                                                              | Cancelled, because its pod is fixed and no runner of the old pod is dedicated to the issue any more (I1). The system comment says the run was waiting for input on the retired machine, and that a reply now starts a run on the survivor. |
    | `ASSIGNED`, `RUNNING`, `CANCEL_REQUESTED`, `AWAITING_*` | Not moved, and the merge is not blocked (E8 does not apply). ENGINE only merges a source machine with no live session, so the run belongs to a gone machine. It ends through the existing offline-runner reaper, as for any runner that vanished mid-run, and the next run is dedicated to the survivor. | Same                                                                                                                                                                                                                                       |

  - Nothing is then dedicated to the source runner, so the lost-runner rule does not fire for it.
  - **Tests:** a merge where the owner was removed from the assignees (H-9) still moves the assignment and then strands the queued run; a merge with a `RUNNING` run on the source completes, and the run is reaped; a paused run in another pod is cancelled with the comment, and a reply starts a run on the survivor.

- **Pod move (§13.1).** A move to a pod of **another project** is refused while any issue is dedicated to the runner, with `runner_has_assigned_issues`, and the UI lists the issues. Otherwise I1 and E2 would break. This applies to every runner, not only engines: §13.1 covers moves within one project only. ENGINE already restricts engine runners to pods of their own project; this rule covers own-agent runners.

### 23.5 `dispatch_wait.reason = runner_paused`

A paused runner is treated as offline for matching: `Runner.objects.assignable()` excludes `paused_at` and engine-paused runners (`engine_paused_q`). A dedicated run waiting for it reports `reason = runner_paused` and a `pause_reason`. Added for engines on 2026-11-05 and extended to every runner on 2026-11-06 with H-11: `dispatch_paused` for an own-agent or engine runner whose `Runner.paused_at` is set; for an engine, `owner_paused` for `DevMachine.engine_paused_by_owner`, else the value of `engine_auto_pause_reason` (`engine_sign_in_required`, `gateway_session_revoked`, `wallet_empty`, `profile_unavailable`, `engine_disabled`; `EngineAutoPauseReason` in `runner/models.py`). When the runner is both paused and offline, `runner_paused` wins, because a person must act on it. The issue then says, for example, "Waiting for Built-in engine · <machine>: the wallet is empty". `pause_reason` is `null` for every other reason. The same values are the `pause_reason` of the `assigned_runner_paused` refusal (§11.2). `apps/web_new` needs the case (Implementation status, M3).

### 23.6 Picker and executors

- There is no executor choice since #2903: `managed_runner` and `cloud_agent` are no longer offered for issues and projects (`agent_executor_options` marks them `deprecated`), and old clients that send them get `local_runner` (ENGINE section 6, `normalize_executor_write`). §8 describes what is left of them, and the picker (§14.4) has no Cloud Agent or Pi Dash Agent peers; `apps/web_new` already shows none (M2).
- The picker's runner level lists engines as "Built-in engine · <machine>", read from `provisioning` and the runner's dev machine. ENGINE's interim rule that the picker skips engine rows (`isEngineRunner` in `apps/web_new/src/shared/pickers/pods.ts`) is removed. `assignable_for` reports engines like any runner.
- ENGINE's translation of `agent_executor = managed_runner` from old desktop builds stays; this design does not bring back the per-caller engine. The translation does not touch `assigned_runner` (§8).

### 23.7 Legacy `managed_runner` runs: no exemption needed

ENGINE revision 3.4 exempted a legacy queued `managed_runner` run from I4 and made §13.2's re-point cancel and recreate it, because I5 forbids `pin_kind = dedicated` on it and its migration backfilled `assigned_runner` onto such issues. Neither cause remains:

- ENGINE now migrates issues set to the built-in engine to pod-level `local_runner` and writes no `assigned_runner`, so at the upgrade no issue with a legacy run is dedicated to anything.
- After the upgrade no new `managed_runner` run is created (ENGINE section 6), and the queued ones are failed by the existing sweep after `MANAGED_RUNNER_QUEUED_MAX_AGE_SECS` (12 h).
- This design is implemented only after ENGINE is merged and released, so by the time HA-2b's write path can set `assigned_runner`, every legacy queued run has ended. No path re-points one.

Conclusion: the exemption is dropped. HA-6's rollout check confirms there is no `QUEUED` run with `executor_kind = managed_runner` before HA-2b ships.

### 23.8 Decisions (settled 2026-11-06)

H-10 is decided (ENGINE's PO-10). H-11 to H-14 were proposed on 2026-11-05 with this design's own decisions as defaults, as ENGINE revision 3.3 announced on PIDESKAPP-121. The product owner decided them on 2026-11-06: H-12 and H-14 as proposed; H-11 and H-13 differently, and for **every** dedicated runner, not only engines.

| #    | Choice                                                     | Proposed (2026-11-05)                                                                               | Decided (2026-11-06)                                                                                                                                                                                                                                                                                |
| ---- | ---------------------------------------------------------- | --------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| H-11 | Run dedicated to an offline or paused runner               | H-1: every run waits, visibly, with no timeout; only a person ends the wait.                        | ENGINE revision 3.2's rule, for every dedicated runner (replaces H-1): a person's run is refused at creation with `assigned_runner_offline` / `assigned_runner_paused`, offering `pod_once`; automatic runs wait and fail after 12 h with `dedicated_runner_unavailable`, stopping the clock (§11). |
| H-12 | Engine removed or revoked while issues are dedicated to it | H-2: the project's `dedicated_runner_lost_action`, default `fail_to_backlog`.                       | As proposed (§13.1, §23.4).                                                                                                                                                                                                                                                                         |
| H-13 | Releasing a waiting dedicated run                          | H-8: the issue's creator and current assignees may release, for that run only.                      | Never released, for every dedicated runner (replaces H-8): `release-pin` refuses with 409 `dedicated_pin_not_releasable`, `releasable` is always false, no release on any surface; `pod_once` stays with its own authorization (§12.2, §23.3).                                                      |
| H-14 | The changes in §23.1 to §23.7                              | Adopt them. They follow from ENGINE's executor removal, usage rule and engine-in-the-CLI decisions. | As proposed.                                                                                                                                                                                                                                                                                        |

### 23.9 Epics and tests

- **HA-2:** E5 and I3 for engines (§23.1); `assigned_runner_not_owner` at creation (§23.3, the issue side). The I2 constraint and its NULL-executor test of #2980 are dropped (§6.5).
- **HA-3:** the matcher, preflight, `fall_back` and stranded-sweep rules (§23.2); the refusal check before `reconcile` for H-10 and H-11 (§23.3, §11.2); the 12-hour bound and its clock stop (§11.3); the release refusal and the locality-only release paths (§12); `runner_paused` for every runner (§23.5); the `pod_once` run option for every dedicated runner with its authorization, resolver branch and pod preflight (§23.3); the cross-project pod-move refusal and the merge re-assignment as a lifecycle change (§23.4).
- **HA-4:** the engine label, the `runner_paused` banner text, the refusal notice with "Run on my engine instead" and "Run on any runner in <pod> once", and no release action for a dedicated run, in `apps/web_new` (§14.4, §23.3, §23.6; M3, M4, M7).
- **HA-5:** `pod_once` on the CLI and MCP run commands, the refusal output, and no release (§14.5, §14.6, §23.3).
- **Rust port:** every item above, with SeaORM for reads and writes and no new raw SQL, standalone, after the Django migrations, in the files the runner and clock batches changed (§17; the New-system PR of the same issue).
- **Scenarios** (from ENGINE revision 3.4, to add to HA-6): an issue dedicated to an engine gets a person's Run while the engine is offline, and the Run is refused with `assigned_runner_offline` and the `pod_once` offer, nothing queued and the clock unchanged; a tick on the same issue waits as "Waiting for Built-in engine · <machine>", fails after 12 hours with `dedicated_runner_unavailable` and stops the clock; the same with the engine paused for `wallet_empty`, refused with `assigned_runner_paused`; the engine comes back within 12 hours and takes the waiting tick first; an own-agent runner paused by its owner gives the same refusal with `pause_reason = dispatch_paused`; `release-pin` on the waiting run returns 409 `dedicated_pin_not_releasable`; the engine is removed and the lost-runner rule applies; a busy rejection requeues the run still pinned to the engine; a tick on an issue dedicated to a `pinned_only` engine runs on it without a preflight bounce; a teammate's Run is refused with `assigned_runner_not_owner`; a teammate's state move changes the state, creates no run and leaves no tick to start one, and the owner's later Run ticks as usual; a move to another project's pod is refused while issues are dedicated to the runner; two machines merge and the dedicated issues follow the surviving engine, including after the owner left the assignees (H-9) and with a run still `RUNNING` on the retired machine; a teammate who is an assignee uses "Run on any runner in <pod> once", the run goes to another pod member, and the next tick is dedicated to the engine again; the same option in a pod with no eligible runner is refused with `no_eligible_runner_in_pod` and leaves the issue where it was.
