# Hybrid Issue Assignment — a Pod or One Dedicated Runner

**Status:** design (PDASHOSS01-296, HA-1). H-1 to H-10 settled; H-11 to H-14, for engine runners (§23), adopt H-1, H-2 and H-8 by default and need the product owner's confirmation. Coordinator: PDASHOSS01-295.
**Builds on:** `.ai_design/issue_runner/design.md` (pods, `Issue.assigned_pod`, the matcher), `.ai_design/n_runners_in_same_machine/new_pod_project_relationship/design.md` (pods are project-scoped), `.ai_design/issue_run_improve/design.md` §5.7 (run pinning), `.ai_design/managed_runner/design.md` §8.4–§8.5 (a pin that is an execution target, and waiting visibly).

Today an issue is routed to a **pod**: any free runner in that squad takes the run. This doc adds a second mode: the issue is routed to **one dedicated runner**, chosen by a person, because of the machine, tools or access that runner has.

Every decision in §1 to §22 is settled. The offline policy (§11) was decided by the project owner on 2026-10-12, the five open questions H-2 to H-6 were answered on 2026-10-15, and H-7 to H-9 on 2026-10-21; §21 lists the answers and where each one lands. §23, added on 2026-11-05, brings in the engine runners and carries H-10 (decided) and H-11 to H-14 (to confirm).

**Implementation status (2026-11-05).** Nothing of this design is implemented on the server. At `main` `08be546d6` no Django or `rust-api/` code mentions `assigned_runner`, `pin_kind`, `dispatch_wait`, `dedicated_runner_lost_action` or `assignable_for`; HA-2 to HA-6 are open. The new frontend already ships the UI against this contract, merged with `apps/web_new` on 2026-10-30:

- the two-level pod and runner picker: `apps/web_new/src/shared/pickers/pods.ts` (`buildExecutionTargets`, the E-code sentences, the status line) and `PodPicker.tsx`, reading the runner list with `assignable_for` (`src/shared/pickers/reference.ts`);
- the execution-target write: `src/features/issues/api/executionTarget.ts`, one issue `PATCH` of `assigned_pod_id`, `assigned_runner_id` and `agent_executor`; the create form sends `assigned_runner_id` too (`src/features/issues/create/createForm.ts`);
- the waiting banner and badge: `src/features/issues/detail/waiting/waiting.ts` and `WaitingBanner.tsx`, reading `agent_status.active_run.pinned_runner`, `pinned_runner_name`, `pin_kind` and `dispatch_wait`, and calling `POST /api/runners/runs/<id>/release-pin/`;
- the AI Workers project settings page: `src/features/project-settings/ai-workers/` and the route `src/routes/$ws/_gated/settings/projects/$projectId/ai-workers.tsx`, writing `dedicated_runner_lost_action` through the project `PATCH`;
- the shapes, in `packages/api-client/src/contracts/`: `issues.ts` (`assigned_runner_id`, `AssignedRunnerDetail`), `issue-agent.ts` and `runs.ts` (`DispatchWait`, the pin fields, `ReleaseRunPinResult`), `runners.ts` (`assignable`, `not_assignable_code`, `is_busy`, `queue_depth`, `assignableFor`), `projects.ts` (`dedicated_runner_lost_action`).

The server work must match the contract `apps/web_new` already sends and reads. Checked against this doc, these match: the `assigned_runner_id` write and read on the app API (§14.1), including the list field and the 400 body keyed by `assigned_runner_id`; `assigned_runner_detail` field for field; the four `assignable_for` row fields (§7.4); the run pin fields and `dispatch_wait` field for field (§14.3); the release-pin path and its `assignment` note (§12.2); the banner's three actions (§14.4); the two `dedicated_runner_lost_action` values (§6.6). Differences found:

| #   | Where                                        | Difference                                                                                                                                                                                                                                                                                                                                                                                                                                                                                    |
| --- | -------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| M1  | Issue and project `PATCH` today              | Not a shape mismatch but a live gap: the server has no `assigned_runner_id` or `dedicated_runner_lost_action` field, so DRF drops both without an error. A runner picked in `apps/web_new` shows as chosen until the refetch, then reads as the pod; the AI Workers setting does not persist. This is the "saved choice that dispatch ignores" §19 rules out, reached from the client side. Until HA-2b, `apps/web_new` should not offer the runner level (or HA-2a should reject the field). |
| M2  | `pods.ts` agent rows, `executionTargetPatch` | The picker still offers Cloud Agent and Pi Dash Agent (desktop) as peers, from `agent_executor_options`, and an agent pick writes `assigned_pod_id: null`. §8 only says the runner is cleared. Once the engine design removes the executor choice (§23.6), the picker must hide options marked `deprecated`, and `AgentExecutorOption` has no such field yet.                                                                                                                                 |
| M3  | `waiting.ts`, `DispatchWait`                 | No `runner_paused` case and no `pause_reason` field (§23.5). An unknown reason renders as the raw code ("Waiting for runner X — runner_paused."), so it degrades but reads badly.                                                                                                                                                                                                                                                                                                             |
| M4  | `pods.ts` runner rows                        | Rows are labelled `runner.name`. An engine runner should read "Built-in engine · <machine>" (§23.6), from `provisioning` and `dev_machine_detail`, which the runner contract already carries. `RUNNER_NOT_ASSIGNABLE_REASONS` keeps `runner_desktop_bundled`, which §23.1 removes.                                                                                                                                                                                                            |
| M5  | `ai-workers/rules.ts`                        | The `fall_back` explainer says "Queued runs continue on any runner in the pod instead". §13.1 falls back to the issue's pod, then the project's default pod, then the no-eligible-runner bounce. The text should not promise the pod.                                                                                                                                                                                                                                                         |
| M6  | Runner list (`listRunners`)                  | The picker reads the list without `include_bundled`, so engines are hidden today by the server filter. The engine design removes that filter and has the picker skip `desktop_bundled` rows itself until §23 lands; §23 then lists them.                                                                                                                                                                                                                                                      |

**Sequencing (product owner, 2026-11-05).** The built-in engine design (`.ai_design/built_engine_as_runner/design.md`, PIDESKAPP-121, PR #1496) is implemented and finished first. Implementation of this design, §23 included, starts only after that work is merged, and builds on its `Runner.pod_dispatch`, engine usage rule and machine pause fields as they are on `main` by then. The Rust port is on `main` now (not `origin/rust-dev`, as §17 says) and reads and writes through SeaORM (#2896, the raw-SQL ratchet guard #2881); the HA work touches files the SeaORM migration batches changed, the runner endpoints in Batches 1a to 1g (#2899 to #2916) and the issue, transition and clock domains in Batches 2a to 3c (#2918 to #2939), so it starts from `main` after those and uses SeaORM for reads and writes.

---

## 1. Why

A pod answers "who is free?". It cannot answer "who is able?". Two runners in the same pod can differ in ways the cloud does not model: one sits on the machine with the GPU, the VPN, the signing key, the licensed tool or the 200 GB dataset. Users work around this today by creating a one-runner pod per special machine, which multiplies pods and breaks the squad for everything else.

The dispatch layer already has what is needed: `AgentRun.pinned_runner` routes a queued run to exactly one runner, and `next_for_runner()` serves a runner's pinned runs before the pod's shared queue. What is missing is a way for a user to ask for it, a rule for what the pin means when the user asked for it, and the surfaces to show a run that is waiting for its runner.

## 2. Goals

- An issue is assigned either to a pod (today's behaviour, unchanged) or to one runner inside a pod.
- Every run of a dedicated issue — implement, review, test, ticks, comment-triggered, Run AI — goes to that runner and to no other, unless a person releases it.
- A run waiting for its runner is visible, says why it waits, and can be released.
- An issue with no dedicated runner behaves exactly as it does today. Existing matcher and pin tests pass unchanged.
- One validation path shared by the web API, the v1 API, the CLI and the MCP tools.

## 3. Non-goals

- **Routing by capability.** v1 picks a runner by identity. "Any runner that has label X" is sketched in §16 and deferred.
- **Scheduler bindings.** `SchedulerBinding.pod` keeps its squad-only shape in v1 (§15).
- **Cloud Agent and managed (desktop) runs.** They have no user-choosable runner (§8). The built-in engine as a runner, which replaces managed runs, is §23.
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
| **Release**          | Dropping the pin on one queued run so the pod's shared queue serves it. Run-scoped; it does not change the issue's assignment (§12.2).       |
| **Shared queue**     | The queued runs of a pod with no pin. Any eligible runner in the pod may take them.                                                          |
| **Lost-runner rule** | The project setting that says what happens to a dedicated issue when its runner is revoked or deleted (§6.6, §13.1).                         |

## 5. What exists today

- `Issue.assigned_pod` (`apps/api/pi_dash/db/models/issue.py`) is the only routing field. `Issue.save()` resolves it to the project's default pod on creation. `Issue.agent_executor` selects `local_runner`, `cloud_agent` or `managed_runner`; NULL inherits the project default (`core/agent_execution.py:effective_executor_for_issue`).
- `AgentRun.pinned_runner` (`runner/models.py`) is set only by the system: by `_pinned_runner_for()` in `orchestration/service.py` (locality) and by `cloud_agent/creation.py:_managed_execution_fields()` (the creator's desktop runner).
- `runner/services/matcher.py`: `next_for_runner()` returns runs pinned to the runner first, then unpinned runs in its pod, each tier oldest first. `next_queued_run_for_pod()` skips pinned runs. `drain_pod()` and `drain_for_runner()` skip a runner that has a run in `BUSY_STATUSES`.
- `runner/tasks.py:release_pins_on_offline_runners` un-pins a queued `local_runner` run once its pinned runner has been silent for `RUNNER_OFFLINE_PIN_RELEASE_SECS` (300 s). `Runner.revoke()` un-pins queued runs and cancels active ones. `runner/views/runs.py:AgentRunReleasePinEndpoint` un-pins by hand (session auth only). `runner/services/run_lifecycle.py:apply_run_resume_unavailable` re-queues a run and drops its pin.
- `runner/services/permissions.py:filter_runs_usable_by_runner`: a private runner (the only visibility today) takes a run only if its owner created the run, is billed for it, created the issue, or is a current assignee. Tick runs on local runners are created by the agent system user (`orchestration/scheduling.py:_resolve_creator_for_trigger`), so for ticks only the last two clauses can match.
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

| #   | Invariant                                                                                                                                                        | Enforced by                                                                                          |
| --- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- |
| I1  | `assigned_runner` set ⇒ `assigned_pod` set and `assigned_runner.pod_id == assigned_pod_id`.                                                                      | DB check for "pod is set"; the shared service (§7.3) and the lifecycle hooks (§13) for pod equality. |
| I2  | `assigned_runner` set ⇒ `agent_executor == "local_runner"` (explicit, not inherited).                                                                            | DB check constraint.                                                                                 |
| I3  | `assigned_runner` is never a revoked or `desktop_bundled` runner. For engine runners, §23.1 reduces it to "never revoked".                                       | Shared service on write; `Runner.revoke()` on revoke (§13).                                          |
| I4  | A queued run of an issue with a dedicated runner carries `pinned_runner == issue.assigned_runner` and `pin_kind == "dedicated"`, unless it was released (§12.2). | The pin resolver (§9.1) and the re-point step (§13.2).                                               |
| I5  | `pin_kind == "dedicated"` ⇒ `executor_kind == "local_runner"`.                                                                                                   | DB check constraint on `AgentRun`.                                                                   |

I2 is explicit on purpose. If the executor were inherited, flipping the project default to the Cloud Agent would silently strip the meaning from every dedicated issue in the project. Setting a runner therefore stamps `agent_executor = "local_runner"` on the issue (§8).

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
- The kind is stamped at creation from the issue's assignment and rewritten only by the re-point step (§13.2) and by a release (§12.2).

Rejected: inferring the kind at read time from `run.work_item.assigned_runner`. The sweep and the matcher would need a join, and a run released by hand could not be told apart from one that never had a dedicated pin.

### 6.5 Migrations

Two migrations, both additive, numbered with the next free number at implementation time (`db` is at 0168 and `runner` at 0029 on main today).

1. `db`: add `issues.assigned_runner_id` (nullable, no default — a metadata-only change on Postgres); add a partial index `WHERE assigned_runner_id IS NOT NULL` with `AddIndexConcurrently` in a non-atomic migration; add two check constraints: `assigned_runner_id IS NULL OR assigned_pod_id IS NOT NULL`, and `assigned_runner_id IS NULL OR agent_executor = 'local_runner'`.
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
| E6  | The executor after this request is `local_runner` (§8).                                      | `executor_not_local`        |
| E7  | `R.owner` is the issue's creator or one of its current assignees.                            | `runner_owner_not_on_issue` |
| E8  | The issue has no run that a runner holds; `QUEUED` and `PAUSED_AWAITING_INPUT` pass (§13.2). | `active_run_in_progress`    |

E7 is `filter_runs_usable_by_runner` reduced to the clauses that hold for _every_ run of the issue. A tick run is created by the agent system user, so "the run's creator owns the runner" cannot be relied on. On issue creation the requester is the creator, so a user's own runner always passes E7. Combined with E1, in v1 a user dedicates an issue to **their own** runner, on an issue they created or are assigned to.

**Decision (project owner, 2026-10-15, H-4): confirmed as written.** The requester must be the runner's owner (E1) and also the issue's creator or a current assignee (E7). An issue has no separate owner field; "issue owner" means its creator. A request that fails E7 is rejected with `runner_owner_not_on_issue` and a message that says to add the runner's owner as an assignee. The owner is **not** added as an assignee automatically. When a non-private visibility is added to runners, E1 and E7 widen with `can_use_runner` and `filter_runs_usable_by_runner` and nothing else in this doc changes.

**Decision (project owner, 2026-10-21, H-7): any creator or current assignee may change or clear the dedicated runner**, whoever set it. If A dedicates the issue to A's runner and B is an assignee, B may switch it to B's own runner or clear it back to the pod; the rules check only the requester's relation to the runner they pick. There is no rule protecting the previous choice. The change is recorded in the activity log (§14.7).

### 7.2 Reject or warn

**Decision: structural problems are rejected on save; transient ones are allowed and shown.**

- E1–E8 are structural: saving such a choice would create an issue that can never run, or desync a live run. The request fails with HTTP 400, `{"assigned_runner_id": [message], "code": <code>}`.
- Offline and busy are transient: the choice is saved. The response carries the runner's status and queue depth (§14.1) and the picker shows them before the user confirms (§14.4).
- The rules run only when the request changes the runner. Re-sending the current value is a no-op and is never rejected, as with the pod today: the web form re-PATCHes its full editable set, so a dedicated issue must stay editable after its eligibility has drifted.

Eligibility can drift after a save: the owner is removed from the assignees, or leaves the workspace. Save-time validation cannot prevent that, so §9.4 re-checks at run creation and §13 handles each event.

### 7.3 One shared service

A new module, `apps/api/pi_dash/runner/services/issue_assignment.py`, owns the rules. Both serializers call it; neither re-implements them.

```python
def validate_execution_target(*, issue, project_id, user, pod, runner, executor, changed) -> ExecutionTarget
def apply_execution_target(issue, target, *, actor) -> None      # saves, re-points a queued run (§13.2), writes activity
def clear_dedicated_runner(issue, *, reason, actor=None) -> None  # lifecycle hooks (§13)
def assignable_runners(*, project, user, issue=None) -> QuerySet  # §7.4
```

The existing pod and executor guards in the two serializers move into `validate_execution_target` unchanged, which also removes the small difference between them today (the app serializer skips the active-run guard when the current pod is NULL; the v1 serializer does not). The v1 behaviour wins.

### 7.4 Listing assignable runners

No new endpoint. The runner list gains one query parameter on both surfaces:

- web: `GET /api/runners/?workspace=&project=&assignable_for=<issue-id | "new">`
- v1 (added by PDASHOSS01-245): `GET /api/v1/workspaces/<slug>/projects/<project>/runners/?assignable_for=<issue-id | "new">`

With the parameter, each row gains `assignable` (bool), `not_assignable_code` (an E-code or `""`), `is_busy` (bool) and `queue_depth` (int, §10.1). `"new"` evaluates E7 with the requester as the future creator. The queryset comes from `assignable_runners()`, which starts from the shared helper PDASHOSS01-245 introduces. Rows the requester cannot see are not returned at all.

## 8. Executor kinds

**Decision: only `local_runner` carries a dedicated runner.**

| Executor         | Dedicated runner | Why                                                                                                                              |
| ---------------- | ---------------- | -------------------------------------------------------------------------------------------------------------------------------- |
| `local_runner`   | Yes              |                                                                                                                                  |
| `cloud_agent`    | No               | A cloud run has no runner; `agent_run_cloud_has_no_local_assignment` forbids a pin.                                              |
| `managed_runner` | No               | The pin is already the creator's own desktop runner, chosen per run by `_managed_execution_fields`; it changes with who runs it. |

Rules for the service:

- Setting a runner sets `agent_executor = "local_runner"` in the same write. Sending a runner together with an explicit non-local executor fails with `executor_not_local`.
- Setting `agent_executor` to `cloud_agent` or `managed_runner`, or clearing it to NULL (inherit), clears `assigned_runner` in the same write and records it in the activity log.
- The existing mid-flight guard on executor changes applies as it does today.

## 9. Dispatch

### 9.1 One pin resolver

`_pinned_runner_for()` is replaced by a resolver that returns the runner and the kind together, used by every creation path:

```python
def resolve_run_pin(issue, *, parent, pod, fresh_session) -> tuple[Optional[Runner], str]:
    runner = issue.assigned_runner
    if runner is not None:
        if runner.status == RunnerStatus.REVOKED or runner.pod_id != pod.id:
            clear_dedicated_runner(issue, reason="stale_at_dispatch")   # self-heal, §13
        else:
            return runner, PinKind.DEDICATED        # offline or busy: still pinned, the run waits
    if fresh_session or parent is None:
        return None, PinKind.LOCALITY
    return _locality_runner_for(parent, pod), PinKind.LOCALITY   # today's _pinned_runner_for, renamed
```

Precedence: a managed pin from `execution_fields()` (only for `managed_runner`, which cannot have a dedicated runner) > dedicated > locality. A dedicated pin does **not** require the runner to be online; that is the difference from a locality pin, which is only set on a live runner.

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

Comment coalescing, the one-active-run rule and the ticker are unchanged.

### 9.3 Matcher

`next_for_runner()` changes in one place: the tier order, which now depends on what the runner served last (§10.2). Everything else stays: it already serves only runs pinned to this runner or unpinned, already applies `filter_runs_usable_by_runner`; `next_queued_run_for_pod()` already excludes pinned runs; desktop-bundled runners already take only managed runs (for engine runners, §23.2 replaces this). For a runner whose last run was not a dedicated one the order is exactly today's, so a pod with no dedicated issues sees no change and the existing matcher and pin tests pass unmodified. HA-3 adds the alternation tests (§18).

`apply_run_resume_unavailable` changes: for a `dedicated` pin it re-queues the run and keeps the pin and the kind (it still clears the parent's `thread_id`, so the next attempt is a fresh session on the same runner). For a `locality` pin it behaves as today.

### 9.4 Preflight and messages

`pod_has_runner_for_issue_principal()` gains a branch for a dedicated issue: it checks that one runner instead of the pod — not revoked, not `desktop_bundled`, in the run's pod, and owned by the run creator, the issue creator or a current assignee. For an engine runner, §23.2 replaces "not `desktop_bundled`" with the engine usage rule. On failure the existing bounce runs (`_bounce_issue_no_eligible_runner`: back to Backlog, system comment) with reason `dedicated-runner-not-eligible`, and the comment names the runner and the failing rule.

Offline is not a bounce. The run is created and waits (§11). When a run is created for a dedicated runner that is not online, creation also posts one system comment:

> Queued for runner **gpu-box**, which is offline (last seen 2 h ago). The run starts when it reconnects. To run it elsewhere, release the run to pod **Default** or change the issue's runner.

Bot comments do not wake the agent, so this cannot loop. The Run AI response carries the same fact in `dispatch_wait` (§14.3).

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

**What the runner served last is derived, not stored.** `last_dedicated` is true when the runner's most recent run by `assigned_at` (filtered to `assigned_at IS NOT NULL`, since Postgres sorts NULLs first in a descending order; one indexed lookup, §6.5) has `pinned_runner` set and `pin_kind = dedicated`, read inside `next_for_runner` before the queue query. When `next_for_runner` runs, the runner is idle, so that run is terminal or paused. A run's `pin_kind` does not change after assignment: a release (§12.2) and a re-point (§13.2) both happen only while the run is `QUEUED`, so a released run counts as shared work, which is what it was. A runner with no runs yet counts as `last_dedicated` false.

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
3. **The user can end the wait** in three ways, all available while the run is still `QUEUED`:
   - _Release this run to the pod_ (§12.2). One run goes to the squad; the issue stays dedicated.
   - _Change or clear the issue's runner_ (§13.2). The queued run is re-pointed in the same transaction: to the new runner, or to the pod's shared queue when cleared.
   - _Cancel the run._ As today.

## 11. Offline policy

The dedicated runner is offline and a run is queued for it. Options:

| Option                                   | For                                                                             | Against                                                                                                          |
| ---------------------------------------- | ------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------- |
| (a) Wait for the runner                  | Honours the choice. Same as a pod whose runners are all offline: the run waits. | The issue is blocked until someone notices.                                                                      |
| (b) Fall back to the pod after a timeout | Work keeps moving.                                                              | Runs the issue on a machine the user ruled out. It may fail, or succeed wrongly (wrong data, wrong credentials). |
| (c) Per-issue choice                     | Lets the user say which of the two they meant.                                  | One more field and one more control on every surface, before there is evidence it is needed.                     |

**Decided by the project owner on 2026-10-12: (a). The issue stays assigned to its runner and the queued run stays queued for that runner. No automatic fallback to the pod and no timeout in v1.** This section says how to build it; it does not reopen the choice.

- The run stays `QUEUED` and pinned. It is not failed and not released by any sweep (§12.1). Waiting costs nothing but the one run (§10.3).
- The wait is announced: the creation-time comment (§9.4) and the `dispatch_wait` state on every surface (§14.3).
- Only a person ends the wait: release the run, change or clear the issue's runner, or cancel the run (§10.3).
- When the runner reconnects, session-open and the poll-ready path already call `drain_for_runner_by_id` (`runner/services/session_service.py`, `runner/views/sessions.py`), which serves the personal queue first. No new code.

(b) and (c) are not part of v1 and v1 adds no setting or field for them. Possible later additions, listed in §22:

- (b) for a whole instance: an operator setting, a number of seconds after which the sweep releases a dedicated queued run exactly as a manual release would (§12.2) and posts a comment. The issue's assignment would not change.
- (c): one nullable field on `Issue`, read by the same sweep.

Neither needs a change to the model of this design; both reuse `pin_kind` and the release path.

Runs already `ASSIGNED` or `RUNNING` on a runner that goes silent are not touched by this doc: `fail_runs_on_offline_runners` (`RUNNER_OFFLINE_RUN_FAIL_SECS`, 3600 s) fails them as today, and the next run is pinned to the dedicated runner again and waits.

## 12. User-chosen pin versus locality pin

| Mechanism                                         | Locality pin                                                | Dedicated pin                                                       |
| ------------------------------------------------- | ----------------------------------------------------------- | ------------------------------------------------------------------- |
| Set when                                          | Parent's runner is online with a fresh heartbeat, same pod  | Issue has a dedicated runner, whatever its status                   |
| `release_pins_on_offline_runners`                 | Released after `RUNNER_OFFLINE_PIN_RELEASE_SECS` (as today) | **Not released** (§11)                                              |
| `Runner.revoke()`                                 | Un-pinned (as today)                                        | Assignment cleared; run failed or re-routed by project rule (§13.1) |
| Manual release endpoint                           | Un-pinned (as today)                                        | Un-pinned for this run only; assignment unchanged; says so (§12.2)  |
| `apply_run_resume_unavailable`                    | Re-queued without the pin (as today)                        | Re-queued **with** the pin (§9.3)                                   |
| `fresh_session`                                   | No pin                                                      | Pin kept (§9.2)                                                     |
| Runner pod move guard (`runner/views/runners.py`) | Blocks the move while the run is non-terminal (as today)    | Same                                                                |

### 12.1 The sweep

`release_pins_on_offline_runners` adds `pin_kind = "locality"` to its existing filter (and to the re-check under the row lock); that is the whole change for locality pins. No sweep releases a dedicated pin, however long its runner is silent (§11).

A second pass in the same task handles one case for dedicated queued runs, and it is not an offline case. Stranded: when the pinned runner is online, heartbeat-fresh and idle, the run is older than one sweep interval, and `filter_runs_usable_by_runner` excludes it, fail the run with `dedicated_runner_not_permitted` (§9.4). The runner is there and will never take the run, so waiting cannot help.

### 12.2 Manual release

`POST /api/runners/runs/<run_id>/release-pin/` keeps its contract for locality pins. For a dedicated pin:

- Allowed only for the issue's creator and its current assignees (**decision, project owner, 2026-10-21, H-8**), only while the run is `QUEUED`. Anyone else gets HTTP 403. The same check gates the issue's waiting-banner actions (§14.4) and `dispatch_wait.releasable` (§14.3). Restricting cancel to the same people for every issue run is tracked outside this design, in PDASHOSS01-317.
- Sets `pinned_runner = NULL`, `pin_kind = "locality"`, clears the parent's `thread_id` as today, drains the pod.
- Does **not** change `Issue.assigned_runner`. The response says so: the run payload plus `"assignment": {"assigned_runner_id": "<uuid>", "changed": false}`.
- Posts a system comment: "Run released from runner **gpu-box** to pod **Default** by Ana. The issue stays assigned to gpu-box."

To move the issue itself, change its runner (§13.2). The CLI and the MCP tools expose only that second route (§14.5, §14.6): one mechanism for scripts, two buttons for people.

## 13. Lifecycle of the assignment

### 13.1 Events

Every row that clears the assignment calls `clear_dedicated_runner(issue, reason=…)`, which sets `assigned_runner = NULL`, leaves `assigned_pod` alone, writes an activity entry, and posts one system comment naming the runner and the reason.

| Event                                                                  | Issue                                                                                     | Its queued run                                                     | Its running run                         |
| ---------------------------------------------------------------------- | ----------------------------------------------------------------------------------------- | ------------------------------------------------------------------ | --------------------------------------- |
| Runner revoked (any reason, including the owner losing membership)     | Cleared. Then the project's lost-runner rule (below).                                     | Per the lost-runner rule (below)                                   | Cancelled (today)                       |
| Runner deleted                                                         | Same; delete is revoke then delete. `SET_NULL` is the net.                                | Same                                                               | Same                                    |
| Runner moved to another pod of the same project                        | Follows the runner: `assigned_pod` is updated to the new pod in the move's transaction.   | Cannot exist: the move is refused while a run is bound (see below) | Cannot exist (same guard)               |
| Issue's pod changed to another pod, no runner in the request           | Cleared.                                                                                  | Cannot exist: pod changes are refused during an active run         | —                                       |
| Issue's executor changed to `cloud_agent` / `managed_runner` / inherit | Cleared (§8).                                                                             | Cannot exist: same guard                                           | —                                       |
| Issue moved to another project (`utils/issue_move.py`)                 | Cleared in the same `save(update_fields=…)` that resets `assigned_pod`.                   | Cancelled and replaced by the handoff run, unpinned (today)        | Handoff as today                        |
| Runner's owner removed from the issue's assignees (and is not creator) | Not cleared (confirmed, H-9). The next run is bounced by preflight with a comment (§9.4). | Failed by the sweep as stranded (§12.1)                            | Finishes                                |
| Runner offline                                                         | Unchanged.                                                                                | Waits (§11)                                                        | `fail_runs_on_offline_runners` as today |
| Sub-issue created under a dedicated issue                              | The child does **not** inherit. It gets the project default pod, like any new issue.      | —                                                                  | —                                       |

The runner-move guard (`runner/views/runners.py`) refuses a move while a non-terminal run has `runner` or `pinned_runner` set to the runner. A run released by hand (§12.2) is no longer pinned and may be queued or running elsewhere in the old pod, so the guard gains one clause: the move is also refused while any issue dedicated to the runner has a non-terminal run. Without it the move would change `assigned_pod` under an active run, which the issue-side guard forbids.

**The revoke and delete rows (project owner, 2026-10-15, H-2).** The runner is gone for good, so waiting cannot help. `Runner.revoke()` reads `project.dedicated_runner_lost_action` (§6.6) for each issue dedicated to the runner, inside the revoke transaction, after it has cleared the assignment:

- **`fail_to_backlog` (default).** The issue's queued run is failed with `error_code = "dedicated_runner_revoked"` instead of being un-pinned. The issue moves to Backlog through the existing bounce (`orchestration/scheduling.py:_bounce_issue_no_eligible_runner`, reason `dedicated-runner-revoked`), whose system comment (a new message for this reason) names the runner and says it was revoked or deleted and that a person must choose a new runner or the pod. This applies whether or not the issue had a queued run: an issue in a ticking state is moved to Backlog so no tick creates a run on a squad the user did not choose. An issue in another state (Backlog, Todo, Done, …) only gets the cleared assignment and the comment.
- **`fall_back`.** The issue is routed to a squad, in this order:
  1. Its own pod, if that pod still has a runner that could serve the issue after this revoke. The queued run is un-pinned and served by the pod's shared queue (today's revoke behaviour).
  2. Otherwise the project's default pod (`Pod.default_for_project`), if it has such a runner. `assigned_pod` is set to it in the same transaction. `AgentRun.pod` is immutable, so the queued run is cancelled and, on commit, a replacement is created in the default pod through `_create_and_dispatch_run`, as the project-move handoff does.
  3. Otherwise the existing no-eligible-runner bounce applies: back to Backlog with its comment (reason `no-eligible-runner`).

  "A runner that could serve the issue" is `pod_has_runner_for_issue_principal(pod, issue, run.created_by_id)`: non-revoked, not `desktop_bundled`, and owned by the run's creator, the issue's creator or a current assignee (for engine runners, §23.2). A bare "non-revoked runner" check is not enough: a pod whose only other runners are desktop-bundled, or private to users not on the issue, would pass it, and the un-pinned run would sit in the shared queue that no runner serves, with no `dispatch_wait` (it has no pin) and no bounce. The same check is what the replacement run's preflight applies in step 2, so steps 2 and 3 cannot disagree.

  When the issue has no queued run, the same order applies with `run_creator_id=None` (the function then counts only the issue's creator and current assignees): step 2 only changes `assigned_pod` and creates no run, and step 3 moves the issue to Backlog only if it is in a ticking state, as for `fail_to_backlog`.

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
  "releasable": true
}
```

`reason` is one of `runner_offline` (status is not online, or the heartbeat is older than `HEARTBEAT_GRACE`), `runner_busy` (the runner has a busy run, older runs in its personal queue, or — when its last run was dedicated — a shared run it takes first under §10.2; `runs_ahead` counts them), `runner_not_permitted` (§9.4), or `dispatching` (the runner is free and the drain has not run yet). §23.5 adds `runner_paused` for engine runners. `releasable` tells the client whether the requester may release. This also makes waiting on a locality pin visible, which it is not today.

### 14.4 Web picker (`apps/web`)

- `dropdowns/pod/base.tsx` becomes a two-level picker. Each pod is a group: its first option is "Any runner in <pod>" (pod assignment, today's value); under it, the pod's runners that the viewer can see. Cloud Agent and Pi Dash Agent stay as peers (until the engine design removes them; §23.6).
- Each runner row: name, status line (§10.1), owner. A runner with `assignable = false` is disabled and shows the reason for its code.
- `execution-target.ts`: the dropdown value becomes a tagged value (`pod:<id>`, `runner:<id>`, or an executor sentinel). `executionTargetPatch` returns `{agent_executor: "local_runner", assigned_pod_id, assigned_runner_id}` for a runner and `{…, assigned_runner_id: null}` for a pod.
- Collapsed label: "gpu-box" with the pod as secondary text; when waiting, the `dispatch_wait` reason as a badge.
- It appears wherever the pod is shown: the create modal (`issue-modal/components/default-properties.tsx`), the detail sidebar, peek view, and the list, board and spreadsheet property columns.
- Waiting banner in the agent-status area of the issue: "Waiting for runner **gpu-box** — offline, last seen 2 h ago." Actions: **Run on any runner in <pod> once** (§12.2), **Change runner** (opens the picker; §13.2), **Cancel run**.
- Errors from §7.1 are shown in place, next to the picker.
- **AI Workers project settings page** (new; project owner, 2026-10-15). Settings → Projects → <project> → **AI Workers**, a new entry `ai_workers` in `PROJECT_SETTINGS` (`packages/constants/src/settings/project.ts`) next to `schedulers`, at `settings/projects/[projectId]/ai-workers/`. No such page exists today. Its first and only v1 setting is "When a dedicated runner is revoked or deleted": **Fail the run and move the issue to Backlog** (default) or **Fall back to the pod**, with one line under each saying what happens (§13.1). It writes `dedicated_runner_lost_action` (§6.6) through the project PATCH; members who cannot edit project settings see it read-only.
- `desktop-overlay/` shares the dropdown; it must build and render the new value shape.

### 14.5 CLI (`runner/src/cli/issue.rs`)

- `pidash issue create|patch … --runner <name-or-UUID>`; names resolve within the issue's project, among runners the caller can see; an ambiguous name is an error that lists the matches.
- `pidash issue patch … --clear-runner` returns the issue to "any runner in the pod". `--runner` and `--clear-runner` are mutually exclusive.
- `pidash issue create|patch … --pod <name-or-UUID>` — **added scope.** The CLI cannot set a pod today, and the coordinator's "done" needs both modes from the CLI. `--pod` without `--runner` clears the runner (§13.1); with `--runner` the two must agree.
- `pidash issue get` prints `assigned_pod`, `assigned_runner`, `assigned_runner_detail`, and `agent_status.active_run.dispatch_wait` when present.
- `pidash project runners --assignable-for <issue | new>` (extends PDASHOSS01-245) prints `assignable`, the reason, `is_busy` and `queue_depth`.
- Releasing from the CLI is `--clear-runner` or `--runner <other>`; there is no separate release command (§12.2).
- Validation failures print the API's `code` and message in the usual `{"error": …}` envelope.

### 14.6 MCP connector and in-run tools

- `pidash_create_issue` / `pidash_update_issue` (private repo, `pi_dash_cloud/mcp/tools.py`): new optional inputs `assigned_runner` (UUID or name; `null` clears) and `assigned_pod` (UUID or name) — the pod input is added scope for the same reason as the CLI flag. `pidash_get_issue` returns both plus `assigned_runner_detail` and `dispatch_wait`.
- `pidash_list_project_runners` (PDASHOSS01-246) gains `assignable_for`.
- In-run cloud-agent tools (`apps/api/pi_dash/cloud_agent/tools.py`): `pidash_create_project_issue`, the only tool there that creates an issue today, accepts `assigned_runner` and `assigned_pod` through the same service. The acting user is the run's creator, so E1 and E7 apply to that user. An in-run update tool, if one is added later, uses the same service.

### 14.7 Issue activity

`assigned_runner` and — added scope — `assigned_pod` get trackers in `bgtasks/issue_activities_task.py:ISSUE_ACTIVITY_MAPPER`, with the pod and runner names as old and new values. System-initiated changes (§13.1) are attributed to the system actor and carry the reason.

### 14.8 Prompting and docs

`prompting/sections/pidash-cli.md` and `docs/mcp-connector.md` document the flags and inputs, and tell agents:

- Leave an issue on its pod unless the work needs something only one runner has. A dedicated runner is a bottleneck by design.
- A sub-issue does not inherit its parent's runner. Set it on the child only if the child needs it.
- Check `--assignable-for` before choosing; do not choose a runner that is not assignable.
- A run that reports `dispatch_wait` is waiting, not broken. Do not create another run or another issue for it.

## 15. Schedulers

**Decision: follow-up, not v1.** `SchedulerBinding.pod` stays squad-only.

A scheduler binding has no issue, so `Issue.assigned_runner` does not reach it. The follow-up is a nullable `SchedulerBinding.runner` beside `pod`, resolved late like the pod, feeding the same `PinKind.DEDICATED` in `dispatch_scheduler_run`. The sweep, the release endpoint and `dispatch_wait` of this design then apply without change, and `filter_runs_usable_by_runner` already lets a private runner take scheduler runs its owner authored. HA-6 files the issue.

## 16. Runner specialities

**Decision: v1 picks by identity.** The picker shows what the cloud already knows that helps a person choose: name, owner, `host_label`, `os`/`arch`, the dev machine, and the agent and model from `dev_metadata`.

How labels build on this later, without changing the model of this design:

1. `Runner.capabilities` (a JSON list, already on the model and in `RunnerSerializer`) becomes the home of user-visible labels such as `gpu`, `vpn`, `ios-signing`: reported by the daemon and editable by the owner.
2. The picker and `--assignable-for` filter and display by label. "Assign to a runner with label `gpu`" is a client-side resolution to one runner, stored in the same `assigned_runner` field.
3. A true third mode — "any runner in the pod that has label X" — is a different feature: it needs a requirement on the issue and a filter in `next_for_runner`. `AgentRun.required_capabilities` already exists on the run side for it. That is a model change and is out of scope here; this design neither needs it nor blocks it.

## 17. Other codebases

Neither is on `main`; neither is changed here. Follow-up issues are filed in their projects and linked from PDASHOSS01-295.

- **Rust port** (`rust-api/` on `origin/rust-dev`, project PIDASHCONV): the new columns (including `Project.dedicated_runner_lost_action`, §6.6) and their constraints, the matcher's alternation (§10.2), the shared validation rules and codes (§7.1), the pin resolver and the phase table (§9), the sweep split (§12.1), the release semantics (§12.2), the lifecycle hooks (§13), and the `dispatch_wait` contract (§14.3). The port must not fall behind on the matcher guarantees in §18.
- **New frontend** (`apps/web_new`, project NEWFRONT): the two-level picker, the tagged value, the waiting banner and its three actions (§14.4), the new fields in its issue types, and the AI Workers project settings page with the lost-runner rule (§6.6, §14.4).
- **Private repo** (project PRIVATEPI1): the MCP tool inputs and outputs (§14.6), and a check that the cloud overlay's copies of the picker and settings code do not drift from the OSS dropdown.

## 18. Tests

| Layer                            | What                                                                                                                                                                                                                                                                                                                | Where                                                                             |
| -------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------- |
| Model                            | Both check constraints reject bad rows; `SET_NULL` on runner delete; defaults on existing rows, including `fail_to_backlog` on projects.                                                                                                                                                                            | `tests/unit/runner/test_issue_assigned_runner.py`                                 |
| Shared service                   | One test per E-code; pod follows runner; executor stamped; executor change clears; re-point on `QUEUED`; refusal past `QUEUED`; allowed on `PAUSED_AWAITING_INPUT`; the race where the run is assigned mid-request.                                                                                                 | `tests/unit/runner/test_issue_assignment_service.py`                              |
| Contract, both API surfaces      | Set, change, clear; every error code and shape; `assigned_runner_detail`; `assignable_for`; a pod-only issue serializes as before plus two null fields.                                                                                                                                                             | `tests/contract/app/`, `tests/contract/api/`                                      |
| Pin resolver and phases          | One test per row of §9.2, including `fresh_session` keeping the pin and an offline runner still being pinned; stale assignment self-heals.                                                                                                                                                                          | `tests/unit/orchestration/test_service.py`, `test_scheduling.py`                  |
| Matcher                          | A dedicated run is never returned to another runner by `next_for_runner`, `next_queued_run_for_pod`, `drain_pod` or `drain_for_runner`; it goes to its runner even when another is idle and its own is busy; personal queue is FIFO across pin kinds. **Existing matcher and pin tests pass unmodified.**           | `tests/unit/runner/test_matcher.py`, `test_drain_pod.py`                          |
| Sweep and release                | Locality pins released as today; dedicated pins never released by the sweep, however long the runner is silent; stranded run failed; manual release leaves the assignment and reports it; `apply_run_resume_unavailable` keeps a dedicated pin.                                                                     | `tests/unit/runner/test_offline_runner_recovery.py`, `test_runs_views.py`         |
| Matcher alternation              | After a dedicated run the runner takes the oldest shared run, then a dedicated one; either side empty → the other; locality pins first in both orders; no dedicated runs → today's order; a released run counts as shared.                                                                                          | `tests/unit/runner/test_matcher.py`                                               |
| Lifecycle                        | One test per row of §13.1: revoke and delete under each lost-runner rule (fail to Backlog; fall back to own pod, to the default pod, and bounce when neither has a runner), runner pod move (and its refusal while a dedicated issue has a run), issue pod change, executor change, project move, assignee removal. | `test_revocation.py`, `test_runner_pod_move.py`, `test_issue_move_run_repoint.py` |
| Preflight and waiting state      | Bounce with the dedicated reason; creation-time offline comment posted once; `dispatch_wait` for each `reason`.                                                                                                                                                                                                     | `tests/unit/orchestration/test_eligibility_preflight.py`                          |
| Web                              | Option building, disabled states and reasons, tagged value round-trip, hydration of the selected runner, the waiting banner's actions.                                                                                                                                                                              | `apps/web/tests/issues/execution-target*.test.ts`                                 |
| CLI                              | Argument mapping for `--runner`, `--clear-runner`, `--pod`; name resolution and ambiguity; error passthrough.                                                                                                                                                                                                       | `runner/` unit tests                                                              |
| MCP                              | The new inputs and outputs.                                                                                                                                                                                                                                                                                         | private repo                                                                      |
| End to end, two runners, one pod | The six scenarios in HA-6, with the offline scenario asserting the decision of §11, the revoke scenario asserting both lost-runner rules of §13.1, and a busy-runner scenario asserting the alternation of §10.2.                                                                                                   | HA-6                                                                              |

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

| Epic                        | Implements                                                                                                                                                  |
| --------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------- |
| HA-2 data model and API     | HA-2a: §6 (incl. §6.6), read side of §14.1 and §14.2, §14.7. HA-2b: §7, §8, §13.1 (the issue-side rows), §13.2, write side of §14.1 and §14.2. Order in §19 |
| HA-3 dispatch and lifecycle | §9, §10.2 (alternation), §11, §12, §13.1 (the runner-side rows, incl. the lost-runner rule), §14.3                                                          |
| HA-4 web UI                 | §10.1, §10.3, §14.4 (incl. the AI Workers settings page)                                                                                                    |
| HA-5 CLI, MCP, prompting    | §14.5, §14.6, §14.8                                                                                                                                         |
| HA-6 tests, docs, rollout   | §18 (end to end), §19, the follow-ups of §15, §16 and §17                                                                                                   |

Scope this design adds to the epics as first written: `--pod` on the CLI and `assigned_pod` on the MCP tools (HA-5); activity tracking for `assigned_pod` (HA-2); `dispatch_wait` for locality pins as well (HA-3); the extra clause on the runner-move guard (HA-3, §13.1); re-pointing a queued run when the assignment changes (HA-2); from the 2026-10-15 decisions, the project lost-runner setting and its API (HA-2) and lifecycle (HA-3), the AI Workers settings page (HA-4), the matcher alternation (HA-3), and the HA-2a/HA-2b split with its merge order (§19). Scope it removes: the temporary rollout setting (H-5); a separate "list eligible runners" endpoint (a parameter on the existing list instead, §7.4); a separate CLI release command (§12.2).

## 21. Decisions

H-1 by the project owner on 2026-10-12; H-2 to H-6 on 2026-10-15; H-7 to H-9 on 2026-10-21. H-10 was decided on 2026-11-04 as PO-10 of the engine design and moved here on 2026-11-05. H-11 to H-14 are proposed defaults that need the product owner's confirmation (§23.8).

| #    | Question                                                                                         | Decision                                                                                                                                                                                                | Section     |
| ---- | ------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------- |
| H-1  | Dedicated runner offline with a run queued                                                       | Wait, visibly; no automatic fallback, no timeout.                                                                                                                                                       | §11         |
| H-2  | Dedicated runner revoked or deleted                                                              | A project setting (§6.6) on a new AI Workers page. Default: fail the queued run, move the issue to Backlog, comment. Alternative: fall back to the pod, then the default pod, then the existing bounce. | §6.6, §13.1 |
| H-3  | Dedicated work versus squad work on the same runner; queue limit                                 | Strict 1:1 alternation between dedicated runs and the shared queue; locality pins keep first priority; no cap; queue depth shown and warned.                                                            | §10         |
| H-4  | Who may dedicate an issue to a runner                                                            | Confirmed: the runner's owner, who is also the issue's creator or a current assignee. Otherwise rejected; the owner is not auto-added.                                                                  | §7.1        |
| H-5  | Rollout gate                                                                                     | No gate. HA-2's write path lands after HA-3's dispatch, recorded as `blocked_by` relations.                                                                                                             | §19         |
| H-6  | Showing a dedicated runner's name, status and owner on the issue                                 | Confirmed: shown to everyone who can see the issue.                                                                                                                                                     | §14.1       |
| H-7  | Who may change or clear a dedicated runner someone else set                                      | Any creator or current assignee, whoever set it. No rule protects the previous choice.                                                                                                                  | §7.1        |
| H-8  | Who may release a waiting dedicated run to the pod                                               | Only the issue's creator and current assignees. (Cancel for every issue run is restricted the same way in PDASHOSS01-317, outside this design.)                                                         | §12.2       |
| H-9  | The dedicated runner's owner is removed from the assignees                                       | Confirmed as written: the assignment is not cleared; a queued run is failed as stranded and the next run is bounced to Backlog.                                                                         | §13.1       |
| H-10 | A human run on an issue dedicated to someone else's engine, which the engine usage rule excludes | Refused at creation with `assigned_runner_not_owner`, offering "Run on my engine instead" and "Run on any runner in <pod> once". Decided as the engine design's PO-10.                                  | §23.3       |
| H-11 | Run dedicated to an offline or paused engine                                                     | **Proposed:** H-1 applies to engines as written.                                                                                                                                                        | §23.8       |
| H-12 | Engine removed or revoked while issues are dedicated to it                                       | **Proposed:** H-2 applies to engines as written.                                                                                                                                                        | §23.8       |
| H-13 | Who may release a waiting run dedicated to an engine                                             | **Proposed:** H-8 applies to engines as written.                                                                                                                                                        | §23.8       |
| H-14 | The changes to this design for engine runners                                                    | **Proposed:** §23.1 to §23.7.                                                                                                                                                                           | §23         |

## 22. Deferred

- Automatic fallback to the pod when the dedicated runner is offline: an instance-wide setting (§11, option b) or a per-issue choice (§11, option c).
- A dedicated runner on scheduler bindings (§15).
- Capability labels and routing by capability (§16).
- Moving a waiting issue to a runner in another pod without cancelling the run (§13.2).
- A cap on a runner's personal queue (§10.1).
- Bulk-editing the dedicated runner across issues.
- Runners that belong to no pod (§3).
- Many-to-many membership between runners and pods (§3). v1 keeps one pod per runner.

## 23. Engine runners (built-in engine)

Added on 2026-11-05. The engine design (`.ai_design/built_engine_as_runner/design.md`, PIDESKAPP-121, "ENGINE" below) makes the built-in engine an ordinary runner: a `Runner` row with `provisioning = desktop_bundled` per machine and project, hosted by the Pi Dash CLI daemon, billed to its owner's wallet. ENGINE revision 4.0 gives an engine issue work only through its pod and moves everything about dedicating an issue to one engine here. ENGINE revisions 3.3 and 3.4 are where this section comes from.

**Depends on ENGINE (PR #1496) being merged and implemented.** This section uses ENGINE's concepts as they are on `main` when HA implementation starts (Sequencing, top of this doc): `Runner.pod_dispatch` (`pinned_only` / `pod_member`), the engine usage rule (ENGINE section 8: an engine takes only runs its owner created, plus automatic runs on the owner's issues and schedulers, unless `DevMachine.engine_team_runs` is on), the machine pause fields `DevMachine.engine_paused_by_owner` and `engine_auto_pause_reason`, the removal of the executor choice, and the merge endpoint. Where this section says nothing, the rest of this design applies to engines as written.

### 23.1 Engines can be dedicated runners

- E5 and I3 are lifted for engine runners. I3 becomes "never a revoked runner"; the code `runner_desktop_bundled` is removed. An engine on a legacy desktop machine (an older app build) can be chosen too.
- E1 and E7 are unchanged, so only the engine's owner can dedicate an issue to it, on an issue they created or are assigned to. A runner is visible only to its owner, so nobody else can pick someone's engine.
- Every phase is pinned (§9.1, §9.2), whatever the engine's `pod_dispatch`: a `pinned_only` engine takes the runs dedicated to it, and only those. Automatic runs (ticks) on an issue dedicated to it are admitted under the usage rule.

### 23.2 The engine usage rule where this design tests "not `desktop_bundled`"

- **Matcher (§9.3).** "Desktop-bundled runners already take only managed runs" is replaced by ENGINE section 7: an engine takes runs dedicated to it, and, when it is a `pod_member`, its locality pins and the pod's shared queue, all under the usage rule. A `pinned_only` engine serves no shared queue, so under H-3 it works through its dedicated queue in order.
- **Preflight (§9.4) and `fall_back` (§13.1).** "Not `desktop_bundled`" is replaced, for engine runners only, by the engine usage rule, which also reads the trigger (human versus automatic, `HUMAN_TRIGGERS` in `apps/api/pi_dash/runner/models.py:261-268` at `08be546d6`). Ordinary runners keep this design's test. `pod_has_runner_for_issue_principal` is the one function both use, so they cannot disagree.
- **Stranded sweep (§12.1).** It also fails a queued engine run the usage rule stops admitting while it waits, for example when the owner turns the opt-in off or is removed from the assignees.

### 23.3 A teammate's run on an issue dedicated to an engine (H-10)

This design creates a run the dedicated runner will not take whenever the runner's owner is on the issue, and the stranded sweep fails it later. For an engine that would spend nothing but would confuse the teammate, so:

- A human-triggered run by someone the engine usage rule excludes is refused at creation with `assigned_runner_not_owner`, and nothing is queued. This covers a teammate's Run, Run AI or Comment & Run, and a teammate's state move into a ticking state, on an issue dedicated to the owner's engine while the opt-in is off.
- The UI offers **Run on my engine instead**, which re-assigns the issue to the caller's own runner under E1 and E7 (allowed by H-7), and **Run on any runner in <pod> once**, which creates the run unpinned in the pod for that run only, as a release would (§12.2).
- For Comment & Run the comment is saved and the issue gets one system comment saying why no run started. For a state move the move stands and gets the same system comment.
- **A refused run does not arm the clock.** Every human event reaches `scheduling.reconcile` before the run is created: in `handle_issue_state_transition` at `apps/api/pi_dash/orchestration/service.py:243`, with the preflight at `:286`; in `handle_issue_comment` at `:417`, before `_create_continuation_run` (all at `08be546d6`). A refused state move or Comment & Run would otherwise leave the issue ticking, and the next tick, an automatic run on the owner's issue that the usage rule admits, would start the very run the refusal stopped. The refusal check therefore runs **before** `reconcile`: when the issue is dedicated to an engine and the rule refuses the event's human run, the event is applied to the issue (the state changes, the comment is saved) but not to the clock, which keeps its previous state. A clock the owner or an admitted run already armed keeps ticking; that work is the owner's.
- The Rust counterpart is the human-run reconcile in `rust-api/crates/api/src/runner_runs/dispatch.rs` (`clock::reconcile` at `:1657` at `08be546d6`); the check goes before it, and in the Rust state-transition path the same way.

### 23.4 Lifecycle for engines

- **Offline (§11, H-11).** A run dedicated to an offline engine is created and waits, with no timeout and no fallback; only a person ends the wait. Creation posts the §9.4 comment naming "Built-in engine · <machine>". Session open drains the personal queue first, as for any runner.
- **Removed or revoked (§13.1, H-12).** "Remove from project" and "Remove this computer" revoke the engine runner, so the project's `dedicated_runner_lost_action` applies.
- **Release (§12.2, H-13).** As written. The released run joins the pod's shared queue, where an engine takes it only as a pod member and under the usage rule.
- **Resume and busy rejection (§9.3).** `apply_run_resume_unavailable` keeps a dedicated pin, so a cancellation race cannot send the run to another machine.
- **Merge of two machines.** ENGINE's merge endpoint retires the source machine's engine runner when both machines have one for the same project. Once this design is built, that retirement first re-assigns each issue dedicated to the source runner to the survivor through `apply_execution_target` with the system actor (E7 holds: same owner). When the survivor is in the same pod, a `QUEUED` run is re-pointed (§13.2). When it is in another pod of the project, the issue's `assigned_pod` changes, so a `QUEUED` run is cancelled and, on commit, recreated in the survivor's pod through `_create_and_dispatch_run`, the handoff `fall_back` uses. Nothing is then dedicated to the source runner, so the lost-runner rule does not fire.
- **Pod move (§13.1).** A move to a pod of **another project** is refused while any issue is dedicated to the runner, with `runner_has_assigned_issues`, and the UI lists the issues. Otherwise I1 and E2 would break. This applies to every runner, not only engines: §13.1 covers moves within one project only. ENGINE already restricts engine runners to pods of their own project; this rule covers own-agent runners.

### 23.5 `dispatch_wait.reason = runner_paused`

A paused engine is treated as offline for matching. A dedicated run waiting for it reports `reason = runner_paused` and a `pause_reason`: `owner_paused` for `DevMachine.engine_paused_by_owner`, else the value of `engine_auto_pause_reason` (`engine_sign_in_required`, `gateway_session_revoked`, `wallet_empty`, `profile_unavailable`, `engine_disabled`). When the engine is both paused and offline, `runner_paused` wins, because a person must act on it. The issue then says, for example, "Waiting for Built-in engine · <machine>: the wallet is empty". `pause_reason` is `null` for every other reason. `apps/web_new` needs the case (Implementation status, M3).

### 23.6 Picker and executors

- There is no executor choice once ENGINE is built: `managed_runner` and `cloud_agent` are no longer offered for issues and projects, and old clients that send them get `local_runner`, pod-level (ENGINE section 6). §8's table then describes legacy rows only, and the picker (§14.4) has no Cloud Agent or Pi Dash Agent peers.
- The picker's runner level lists engines as "Built-in engine · <machine>", read from `provisioning` and the runner's dev machine. ENGINE's interim rule that the picker skips `desktop_bundled` rows is removed. `assignable_for` reports engines like any runner.
- ENGINE's pod-level translation of `agent_executor = managed_runner` from old desktop builds stays; this design does not bring back the per-caller engine.

### 23.7 Legacy `managed_runner` runs: no exemption needed

ENGINE revision 3.4 exempted a legacy queued `managed_runner` run from I4 and made §13.2's re-point cancel and recreate it, because I5 forbids `pin_kind = dedicated` on it and its migration backfilled `assigned_runner` onto such issues. Neither cause remains:

- ENGINE now migrates issues set to the built-in engine to pod-level `local_runner` and writes no `assigned_runner`, so at the upgrade no issue with a legacy run is dedicated to anything.
- After the upgrade no new `managed_runner` run is created (ENGINE section 6), and the queued ones are failed by the existing sweep after `MANAGED_RUNNER_QUEUED_MAX_AGE_SECS` (12 h).
- This design is implemented only after ENGINE is merged and released, so by the time HA-2b's write path can set `assigned_runner`, every legacy queued run has ended. No path re-points one.

Conclusion: the exemption is dropped. HA-6's rollout check confirms there is no `QUEUED` run with `executor_kind = managed_runner` before HA-2b ships.

### 23.8 Decisions to confirm

H-10 is decided (ENGINE's PO-10). H-11 to H-13 adopt this design's decisions for engines by default, as ENGINE revision 3.3 announced on PIDESKAPP-121; the product owner may confirm or reverse each.

| #    | Choice                                                     | Default                                                                                             | Not chosen                                                                                                                                                     |
| ---- | ---------------------------------------------------------- | --------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| H-11 | Run dedicated to an offline or paused engine               | H-1: every run waits, visibly, with no timeout; only a person ends the wait (§23.4, §23.5).         | ENGINE revision 3.2's rule: refuse a person's run and expire automatic runs after 12 h. That would change H-1, for engines only or for every dedicated runner. |
| H-12 | Engine removed or revoked while issues are dedicated to it | H-2: the project's `dedicated_runner_lost_action`, default `fail_to_backlog`.                       | ENGINE revision 3.2's `assigned_runner_removed`, leaving the issue pod-level, for engines only.                                                                |
| H-13 | Who may release a waiting run dedicated to an engine       | H-8: the issue's creator and current assignees, for that run only.                                  | Refuse release for engines, so no pod runner takes their runs.                                                                                                 |
| H-14 | The changes in §23.1 to §23.7                              | Adopt them. They follow from ENGINE's executor removal, usage rule and engine-in-the-CLI decisions. | Keep engines out of dedicated assignment.                                                                                                                      |

### 23.9 Epics and tests

- **HA-2:** E5 and I3 for engines (§23.1); `assigned_runner_not_owner` at creation (§23.3, the issue side).
- **HA-3:** the matcher, preflight, `fall_back` and stranded-sweep rules (§23.2); the refusal check before `reconcile` (§23.3); `runner_paused` (§23.5); the cross-project pod-move refusal and the merge re-assignment (§23.4).
- **HA-4:** the engine label, the `runner_paused` banner text and "Run on my engine instead" in `apps/web_new` (§23.3, §23.6; M3, M4).
- **Rust port:** every item above, with SeaORM, in the files the runner and clock batches changed (Sequencing, top of this doc).
- **Scenarios** (from ENGINE revision 3.4, to add to HA-6): an issue dedicated to an engine gets a person's Run and a tick while the engine is offline, and both wait as "Waiting for Built-in engine · <machine>" with no timeout; the engine is removed and the lost-runner rule applies; a busy rejection requeues the run still pinned to the engine; a tick on an issue dedicated to a `pinned_only` engine runs on it without a preflight bounce; a teammate's Run is refused with `assigned_runner_not_owner`; a teammate's state move changes the state, creates no run and leaves no tick to start one, and the owner's later Run ticks as usual; a move to another project's pod is refused while issues are dedicated to the runner; two machines merge and the dedicated issues follow the surviving engine.
