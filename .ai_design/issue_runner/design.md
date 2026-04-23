# Pods, Runners, and Issue Delegation

> Directory: `.ai_design/issue_runner/`
>
> This doc supersedes the earlier draft in this directory. The earlier version
> proposed pre-assigning a specific runner to each issue. We've since shifted
> the conceptual model: runners are **digital employees** scoped to a
> workspace, **pods** group runners into cooperation units, and **issues pin
> to a pod** rather than a specific runner. A proper **queue** drains work
> within each pod.
>
> **Revision log**
>
> - Initial pod-queue design.
> - Post-codex-feedback pass: added decisions #9–#12 (run identity split via
>   `AgentRun.created_by`, synchronous revoke cleanup, pod soft-delete,
>   workspace-consistency validation on run creation). See §5, §6.5, §7.2,
>   §7.5 for the normative sections. The earlier draft conflated
>   `AgentRun.owner` with the run's triggering principal; this version
>   explicitly separates creator identity from billing identity.
> - Final review pass (pre-implementation): reconciled decision #3 with
>   soft-delete semantics; fixed `AgentRun.owner` `on_delete` to `SET_NULL`
>   (prevents cascade-deleting run history when an operator is removed);
>   moved billing capture explicitly into `drain_pod` at assignment time;
>   inlined `PodManager` definition; tightened `DELETE /pods/<id>/` API doc
>   to reflect soft-delete preconditions; removed vestigial validation bullet
>   and renamed `_resolve_owner` consistently to `_resolve_fallback_creator`;
>   added billing-capture and user-deletion tests.
> - Fresh-install simplification: no production data exists yet, so the
>   three-stage migration (schema → backfill → tighten) collapses to a
>   single initial schema migration. Decision #7 (opt-in cooperation) is
>   removed — there's nothing to migrate from. New decision: every workspace
>   auto-creates a default pod named `<workspace.name>-pod` on creation, so
>   the first-run experience requires zero pod setup. Runner registration
>   and issue creation both default to the workspace pod when unspecified.
>   Soft-delete now guards against removing the last pod of a workspace.

## 1. Goal

- Reframe runners as cooperative AI-agent instances available to a workspace, not personal machines owned by an individual.
- Introduce `Pod` — a workspace-scoped group of runners that shares a work queue.
- Let issues pre-assign to a **pod** (stable, doesn't go offline) instead of a runner (which does).
- Fix the stranded-QUEUED bug: when a runner finishes a job, its pod's queue drains automatically.
- Preserve individual accountability: a runner still has an `owner` for management and billing, but not for access gating.

## 2. Conceptual Model

| Concept               | What it is                                                                   | Who can use it                   | Who can manage it               |
| --------------------- | ---------------------------------------------------------------------------- | -------------------------------- | ------------------------------- |
| **Account** (User)    | A human                                                                      | —                                | themselves                      |
| **Workspace**         | The cooperation boundary — a team                                            | any member                       | workspace admin                 |
| **Pod**               | A logical group of AI agents (runners)                                       | any workspace member             | pod creator or workspace admin  |
| **Runner**            | An AI agent instance (a running daemon on some host)                         | any workspace member (via a pod) | runner owner or workspace admin |
| **Owner** (of runner) | The human who registered the runner and is responsible for its host / budget | —                                | —                               |
| **Issue**             | Work item                                                                    | its workspace members            | per existing issue permissions  |

**Key conceptual shift**: runner ownership is an _administrative bond_ (who pays for its uptime, who can revoke it), **not** an access gate. Any workspace member can delegate work to any runner in the workspace — mediated through pods.

## 3. Decisions Locked In

Carried over from prior conversations; several reframed to match the new model.

| #   | Question                                              | Decision                                                                                                                                                                                                                                                                                                                                                                                                                | Source                                     |
| --- | ----------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------ |
| 1   | Whose runner pool does "default" draw from?           | Reframed: it's the workspace's default pod. No personal pool.                                                                                                                                                                                                                                                                                                                                                           | revised                                    |
| 2   | Pinned pod's runners all busy at run time?            | Queue the run in the pod's queue. Drain on runner completion.                                                                                                                                                                                                                                                                                                                                                           | earlier "trust the runner to queue"        |
| 3   | Pod deleted / all runners in pod revoked?             | Pods are **soft-deleted** (see decision #11), so `Issue.assigned_pod` FKs remain valid. On soft-delete, the same transaction explicitly sweeps pointing issues to `assigned_pod=NULL`, re-defaults at next run creation, and notifies creators. No `SET_NULL` FK behavior is relied on.                                                                                                                                 | earlier revoke policy, reconciled with #11 |
| 4   | Backfill existing issues?                             | No. `Issue.assigned_pod` nullable, existing rows stay NULL.                                                                                                                                                                                                                                                                                                                                                             | earlier                                    |
| 5   | Billing (who pays for tokens/compute)?                | **Runner owner.** Digital-employee metaphor: each agent has a cost center attached to its operator. Deferred implementation; design keeps the hook explicit.                                                                                                                                                                                                                                                            | new                                        |
| 6   | Approvals routing (runner asks before write/network)? | **Run creator** approves per-run. Runner owner retains revoke-any-time as the circuit breaker.                                                                                                                                                                                                                                                                                                                          | new                                        |
| 7   | Fresh-install rollout (no production data exists)     | **Zero-setup default pod.** On workspace creation, the backend auto-creates a pod named `<workspace.name>-pod` with `is_default=True`. Runner registration defaults to the workspace pod when no pod is specified. Issue creation auto-fills `assigned_pod = workspace.default_pod`. First-time users never touch pod UI to get a working delegation flow. No migration of legacy data is required because none exists. | new, supersedes the earlier opt-in plan    |
| 8   | Workspace admin role                                  | Reuse `WorkspaceMember.role=20 (Admin)` at `apps/api/pi_dash/db/models/workspace.py:19`.                                                                                                                                                                                                                                                                                                                                | verified                                   |
| 9   | Run identity — `owner` vs `created_by`?               | Introduce `AgentRun.created_by` as a **non-null** FK to User, set at every creation path. **All permission checks migrate to `created_by`**: list, detail, cancel, approval list/decide. `AgentRun.owner` is retired from permission logic and semantically reinterpreted as "billable party" — populated as `runner.owner` at assignment time (nullable until assigned).                                               | codex-feedback                             |
| 10  | Runner revocation and in-flight runs                  | Revocation must **synchronously finalize** all non-terminal runs belonging to the revoked runner. Current `Runner.revoke()` only flips the runner row; design adds explicit cancellation of active `AgentRun` rows in the same transaction. Without this, runs stay `ASSIGNED/RUNNING` forever after a force-close.                                                                                                     | codex-feedback                             |
| 11  | Pod deletion vs. AgentRun history                     | **Soft-delete pods**: add `Pod.deleted_at` + `objects` manager that filters it out. Pod rows are never physically deleted, so `AgentRun.pod` FK stays valid and historical attribution is preserved. Deletion requires (a) zero runners in the pod AND (b) zero non-terminal `AgentRun` rows in the pod. Issues pointing at a soft-deleted pod get their `assigned_pod` cleared (see §7.2).                             | codex-feedback                             |
| 12  | Direct run-creation endpoint — workspace validation   | `POST /api/v1/runner/runs/` **must** validate, before creation: (a) caller is a `WorkspaceMember` of `workspace_id` (403 otherwise), (b) `work_item.workspace_id == workspace_id` if `work_item` provided (400), (c) `pod.workspace_id == workspace_id` if pod is derived/passed (400), (d) caller has at least Member role in the workspace.                                                                           | codex-feedback                             |
| 13  | Every workspace has at least one pod (invariant)      | Workspace creation auto-creates a default pod (decision #7). Soft-deletion of a pod is blocked if it's the workspace's last active pod — admin must create a replacement first. This invariant lets every downstream code path (issue creation, runner registration, dispatch) assume `workspace.default_pod` is always resolvable without a null-check fallback.                                                       | new                                        |

## 4. Data Model

### 4.1 Pod (new)

```python
# apps/api/pi_dash/runner/models.py

class PodManager(models.Manager):
    """Default manager: excludes soft-deleted pods from routine queries."""

    def get_queryset(self):
        return super().get_queryset().filter(deleted_at__isnull=True)


class Pod(models.Model):
    """A workspace-scoped group of runners that share a work queue."""

    MAX_PER_WORKSPACE = 20  # sanity cap; revisit if needed

    id = models.UUIDField(primary_key=True, default=uuid.uuid4, editable=False)
    workspace = models.ForeignKey(
        "db.Workspace", on_delete=models.CASCADE, related_name="pods"
    )
    name = models.CharField(max_length=128)
    description = models.CharField(max_length=512, blank=True, default="")
    created_by = models.ForeignKey(
        settings.AUTH_USER_MODEL,
        on_delete=models.SET_NULL,
        null=True,
        related_name="pods_created",
    )
    is_default = models.BooleanField(default=False)
    deleted_at = models.DateTimeField(null=True, blank=True, db_index=True)
    created_at = models.DateTimeField(auto_now_add=True)
    updated_at = models.DateTimeField(auto_now=True)

    # Default manager filters out soft-deleted pods; all_objects exposes them
    # for admin/audit views and referential-integrity checks.
    objects = PodManager()           # filters deleted_at IS NULL
    all_objects = models.Manager()

    class Meta:
        db_table = "pod"
        constraints = [
            # Active-row uniqueness: (workspace, name) unique among non-deleted pods.
            models.UniqueConstraint(
                fields=["workspace", "name"],
                condition=models.Q(deleted_at__isnull=True),
                name="pod_unique_name_per_workspace_when_active",
            ),
            # At most one active default pod per workspace.
            models.UniqueConstraint(
                fields=["workspace"],
                condition=models.Q(is_default=True) & models.Q(deleted_at__isnull=True),
                name="pod_one_default_per_workspace_when_active",
            ),
        ]
```

- **Soft-delete** (decision #11): pods are never physically removed. `deleted_at` flags a tombstone; all FKs from `AgentRun.pod` and `Issue.assigned_pod` remain valid. This preserves historical run attribution and avoids the `on_delete=SET_NULL` contradiction.
- `PodManager.get_queryset()` filters `deleted_at__isnull=True` so default listings exclude tombstones. Admin / history views can use `Pod.all_objects`.
- Unique constraints are conditional on `deleted_at IS NULL` so a new pod can reuse a deleted pod's name within the same workspace.

- Workspace-scoped. Not user-scoped. **This is the deliberate break from the current "owner" model.**
- `is_default`: at most one per workspace. When a workspace creates its first shared pod, it's marked default. Existing personal-migration pods (see §9) are not marked default.
- `created_by` records who set it up (for audit/UI) but grants no special permissions beyond admin-equivalent on that pod.

### 4.2 Runner (modified)

Add:

```python
pod = models.ForeignKey(
    "runner.Pod",
    on_delete=models.PROTECT,
    related_name="runners",
    null=False,   # Every runner belongs to exactly one pod. Enforced at DB level.
)
```

- `on_delete=PROTECT`: deleting (soft-deleting) a pod with runners is blocked; admin must move or revoke them first.
- `null=False` from the start — there's no legacy data to backfill. Registration flow resolves a pod before insert (defaulting to `workspace.default_pod` when none is explicitly requested).
- **`owner` stays** — it continues to mean "responsible human for this specific runner instance" (billing, revoke). It no longer gates usage.
- Name uniqueness changes from `(workspace, name)` to `(pod, name)` — so two pods in the same workspace can each have a runner named "mac-mini". Tradeoff: human-friendly addressing via `(workspace, name)` is less direct, but pod scope is the natural namespace.

Keep `MAX_PER_USER = 5` as a per-operator cap on how many agents one human can onboard. Pod membership is orthogonal.

### 4.3 AgentRun (modified)

Add:

```python
# Existing field changes semantics and becomes nullable because billable party
# is unknown until a runner is assigned. on_delete is relaxed from CASCADE to
# SET_NULL so deleting a runner's operator user does not erase historical run
# records — audit trail survives, we just lose billing attribution for that
# operator's historical runs.
owner = models.ForeignKey(
    settings.AUTH_USER_MODEL,
    on_delete=models.SET_NULL,
    related_name="agent_runs",
    null=True,
    blank=True,
)

pod = models.ForeignKey(
    "runner.Pod",
    on_delete=models.PROTECT,  # See §7.2 — pods are soft-deleted so the FK is always safe.
    null=False,                # Every run belongs to a pod. Resolved before insert (§6.5).
    related_name="agent_runs",
)

# Run identity split — see decision #9.
# `created_by` is the user who triggered the run. Mandatory at creation and is
# the authoritative principal for list / detail / cancel / approval permissions.
created_by = models.ForeignKey(
    settings.AUTH_USER_MODEL,
    on_delete=models.PROTECT,
    null=False,
    related_name="agent_runs_created",
)
```

- `pod` is set at run creation. Derived from `issue.assigned_pod` when the run was triggered by an issue, or from `workspace.default_pod` if the issue has no pin, or from an explicit `pod` parameter when directly POSTed. Because every workspace has a default pod (invariant #13), resolution effectively never fails in normal operation — the `no pod available` path in §6.5 remains as a defense-in-depth guard for pathological states (e.g. all workspace pods soft-deleted by an admin, which §7.2 blocks). `on_delete=PROTECT` is safe because pods are soft-deleted, not physically removed.
- The run's queue position is implicitly `pod` × `status=QUEUED` × `created_at`.
- `runner` FK is still nullable; it's populated only when a runner actually picks up the run.

**Reinterpretation of `AgentRun.owner`** (decision #9):

- **Before this change**: `owner` was the access-gating principal. Listing, detail, cancel, and approval endpoints all filter by `owner=request.user` (`runs.py:29, 92, 109`; `approvals.py:31, 54`).
- **After this change**: `owner` is deprecated from permission logic and reinterpreted as "billable party" = `runner.owner` once the run is assigned. It is set to `NULL` at creation (not the creator). A nightly background task (or the assignment step in `drain_pod`) can snapshot `runner.owner` into `AgentRun.owner` for billing attribution.
- **All permission checks migrate to `created_by`**:
  - `AgentRunListEndpoint.get` → filter by `created_by=request.user` for "my runs," widened to workspace-scoped listings where `(workspace membership + role)` permits.
  - `AgentRunDetailEndpoint.get` → require (`created_by=request.user`) OR (workspace admin) OR (runner.owner) to read.
  - `AgentRunCancelEndpoint.post` → require `created_by=request.user` OR (runner.owner) OR (workspace admin).
  - `ApprovalListEndpoint.get`, `ApprovalDecideEndpoint.post` → filter by `agent_run.created_by=request.user`. Per decision #6, only the run creator approves.

Optional (deferred): `selection_reason: enum{pinned,fallback,none}` for observability. Skip for MVP.

### 4.4 Issue (modified)

Add:

```python
assigned_pod = models.ForeignKey(
    "runner.Pod",
    null=True,
    blank=True,
    on_delete=models.PROTECT,  # Safe because pods are soft-deleted (§7.2).
    related_name="assigned_issues",
)
```

- Nullable. NULL means "use the workspace default pod at run time."
- Auto-populated at issue creation to `workspace.default_pod`. Because invariant #13 guarantees every workspace has a default pod, this always resolves to a concrete pod on new issues.
- User can change on the issue detail page; choice restricted to non-soft-deleted pods in the issue's workspace.
- When a pinned pod is soft-deleted (see §7.2), pointing issues are swept to `assigned_pod=NULL` in the same transaction as the soft-delete, and their creators are notified. The next run creation re-resolves to `workspace.default_pod`.

## 5. Permission Model

Every endpoint migrates from `owner=request.user` to one of two explicit checks: **workspace membership** (widening) or **`AgentRun.created_by`** (narrowing). The `owner` field is retired from permission logic entirely (decision #9).

### 5.1 Principal helpers (new)

```python
# apps/api/pi_dash/runner/services/permissions.py

def is_workspace_member(user, workspace_id) -> bool:
    return WorkspaceMember.objects.filter(
        workspace_id=workspace_id, member=user
    ).exists()

def workspace_role(user, workspace_id) -> Optional[int]:
    return (
        WorkspaceMember.objects
        .filter(workspace_id=workspace_id, member=user)
        .values_list("role", flat=True)
        .first()
    )

def is_workspace_admin(user, workspace_id) -> bool:
    return (workspace_role(user, workspace_id) or 0) >= 20
```

### 5.2 Access matrix

| Action                          | Who                                                                                     | Check                                                                                 |
| ------------------------------- | --------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------- |
| List pods in workspace          | any member                                                                              | `is_workspace_member(request.user, workspace_id)`                                     |
| Create pod                      | workspace admin                                                                         | `is_workspace_admin(...)`                                                             |
| Rename / toggle default         | admin **or** pod's `created_by`                                                         |                                                                                       |
| Soft-delete pod                 | admin **or** pod's `created_by`, and pod must be empty of runners and non-terminal runs | see §7.2                                                                              |
| List runners in workspace       | any member                                                                              | replaces `runners.py:21` `owner=request.user`                                         |
| View runner detail              | any member                                                                              | replaces `runners.py:34`                                                              |
| Register runner (consume token) | token issuer                                                                            | unchanged                                                                             |
| Move runner between pods        | runner owner **or** workspace admin                                                     |                                                                                       |
| Revoke runner                   | runner owner **or** workspace admin                                                     | widened from owner-only at `runners.py:46`                                            |
| Create AgentRun                 | any member (with validation)                                                            | see §6.5                                                                              |
| List "my runs"                  | anyone                                                                                  | `created_by=request.user`                                                             |
| List "runs in workspace"        | any member                                                                              | `workspace=workspace_id` + membership check                                           |
| View AgentRun detail            | `created_by=request.user` **or** `runner.owner=request.user` **or** workspace admin     | narrowed to exclude non-involved members by default; admins retain oversight          |
| Cancel AgentRun                 | `created_by=request.user` **or** `runner.owner=request.user` **or** workspace admin     |                                                                                       |
| Approve `AWAITING_APPROVAL`     | **`created_by=request.user`** only (decision #6)                                        | `approvals.py:31` and `:54` switch from `agent_run__owner` to `agent_run__created_by` |

### 5.3 `AgentRun.owner` semantics after migration

- **At creation**: `owner = NULL`. This requires changing the field to `null=True, blank=True`; the field is no longer written by views or the orchestrator.
- **At assignment (inside `drain_pod`)**: `owner = runner.owner` — the assigned runner's administrative operator, captured for billing attribution. This is set in the same write that sets `run.runner` and `run.status = ASSIGNED` (see §6.3 code block); no background task is involved.
- **Legacy rows**: pi-dash has no production data, so there's no backfill to run. The `AgentRun.created_by` field is `NOT NULL` from migration day 1 because every future row sets it. In an environment with legacy data, a separate data migration would copy `owner → created_by` for historical rows (under the old model, `owner` was always the triggering user, so this would be an accurate interpretation) — out of scope here.
- **Why not reuse `owner`**: the old `owner` semantic conflated two identities (run creator vs. billable party). Once runners become workspace-shared, these diverge — creator = who typed the prompt; billable party = whose machine ran it. The split is structural, not cosmetic.

> Removed: the earlier draft mentioned a nightly background task as an alternative billing-capture path. Rejected because (a) it doubles the write for every run, (b) it leaves a window where `owner` is NULL and billing is ambiguous, and (c) `drain_pod` already has the transaction and the data. Assignment-time capture is strictly better.

## 6. Dispatch & Queue

### 6.1 Queue semantics

- Queue is **per pod**, FIFO by `created_at` within `status=QUEUED`.
- No priorities in MVP. Worth revisiting once we have real usage patterns.
- No cross-pod spillover. Pods exist precisely so users can segregate workloads; an idle runner in pod A does not drain pod B's queue.

### 6.2 Matcher (rewrite)

Replace `select_runner_for_run` with pod-scoped logic.

```python
# apps/api/pi_dash/runner/services/matcher.py

def select_runner_in_pod(pod: Pod) -> Optional[Runner]:
    """Pick an idle runner in the pod. None if none free."""
    alive_threshold = timezone.now() - HEARTBEAT_GRACE
    return (
        Runner.objects.select_for_update(skip_locked=True)
        .filter(
            pod=pod,
            status=RunnerStatus.ONLINE,
            last_heartbeat_at__gte=alive_threshold,
        )
        .exclude(agent_runs__status__in=BUSY_STATES)
        .order_by("-last_heartbeat_at")
        .first()
    )


def next_queued_run_for_pod(pod: Pod) -> Optional[AgentRun]:
    return (
        AgentRun.objects.select_for_update(skip_locked=True)
        .filter(pod=pod, status=AgentRunStatus.QUEUED)
        .order_by("created_at")
        .first()
    )
```

No more `owner=run.owner` filter. Workspace isolation comes from `pod.workspace`.

### 6.3 Drain triggers

A pod's queue drains on three events:

1. **Run creation** — when an AgentRun is created in a pod, try to dispatch immediately.
2. **Run finalization** (`consumers.py:_finalize_run`, line 436) — when `COMPLETED/FAILED/CANCELLED` fires, the runner has freed up, so we attempt to dispatch the next QUEUED run in that runner's pod.
3. **Runner heartbeat transitions to ONLINE** — when a previously offline/stale runner reconnects, it may be able to drain stuck work.

Each trigger calls a single entry point:

```python
def drain_pod(pod: Pod) -> None:
    """Assign as many QUEUED runs in this pod to idle runners as possible."""
    with transaction.atomic():
        while True:
            run = next_queued_run_for_pod(pod)
            if run is None:
                break
            runner = select_runner_in_pod(pod)
            if runner is None:
                break
            run.runner = runner
            run.owner = runner.owner  # Capture billable party at assignment (§5.3).
            run.status = AgentRunStatus.ASSIGNED
            run.assigned_at = timezone.now()
            run.save(update_fields=["runner", "owner", "status", "assigned_at"])
            transaction.on_commit(
                lambda r=run, rn=runner: send_to_runner(rn.id, build_assign_msg(r))
            )
```

The `select_for_update(skip_locked=True)` combination prevents two concurrent drain calls from double-assigning. `run.owner` is populated here (not at creation) so billing always reflects the runner that actually executed the work — §5.3 decision #9.

**Lock-holding caveat**: the `while True` loop keeps row locks for every run/runner it touches until the transaction commits. For MVP this is fine (queues are short). At scale, revisit by committing per-assignment — out of scope.

### 6.4 Orchestration integration

`orchestration/service._dispatch_to_runner` (`service.py:159-203`) is rewritten:

1. Resolve `run.pod` from `issue.assigned_pod`, falling back to `workspace.default_pod`, falling back to none.
2. If no pod resolves → **abort run creation / dispatch** with a structured `"no pod available"` outcome. No new `AgentRun` row is inserted on the direct API path; the orchestration path returns a non-created outcome and emits a notification to the triggering actor / would-be `created_by`.
3. Otherwise set `run.pod` and call `drain_pod(run.pod)`.

`runs.py` POST (the direct API entrypoint for creating runs without the issue state machine) follows the same pattern, plus the validation rules in §6.5.

### 6.5 Run-creation validation (all entrypoints)

Both the orchestration path and the direct `POST /api/v1/runner/runs/` endpoint (and any future API) **must** enforce these checks before writing the `AgentRun` row. The current `runs.py:35-83` accepts arbitrary `workspace_id` and `work_item_id` from the request with no validation — once runners become workspace-shared, this becomes a cross-workspace integrity hole (decision #12).

Mandatory pre-create validation, in order:

1. **Workspace membership** — `is_workspace_member(request.user, workspace_id)` must be true. Else `403 Forbidden`. Covers the baseline "you can't post runs into someone else's workspace."
2. **Work-item consistency** — if `work_item_id` is provided, `Issue.objects.get(id=work_item_id).workspace_id == workspace_id`. Else `400 Bad Request` with `{"error": "work_item does not belong to workspace"}`. Prevents a member of workspace A from creating a run in workspace B by passing B's ID plus A's issue.
3. **Pod presence + consistency** — the resolved pod (from `issue.assigned_pod` or `workspace.default_pod` or explicit `pod_id` body field) must exist and satisfy `pod.workspace_id == workspace_id` **and** `pod.deleted_at IS NULL`. If no pod can be resolved at all, fail before insert with `409 Conflict` and `{"error": "no pod available"}`. If a pod resolves but belongs to another workspace or is soft-deleted, return `400`.
4. **Creator identity** — `AgentRun.created_by = request.user`, always. Never accept a `created_by` override from the request body (the orchestration internal call is the only non-request caller; it sets `created_by = actor`).

The orchestration path (`service._create_and_dispatch_run`) bypasses the HTTP layer but still enforces 2–4 before writing the `AgentRun` row. The `actor` parameter is the authoritative `created_by`; if `actor is None`, fall back to `_resolve_fallback_creator(issue)` (renamed from the legacy `_resolve_owner`, see §12) for back-compat only, and log a warning. **Implementation TODO**: grep for all callers of `handle_issue_state_transition` during implementation and ensure each passes an explicit `actor`; a follow-up change should promote `actor=None` from "warn" to "reject" once call sites are audited.

A shared helper `validate_run_creation(user, workspace_id, work_item_id, pod_id)` is factored out and called by both paths.

## 7. Lifecycle

### 7.1 Pod creation

Two pod-creation paths:

**Automatic — on workspace creation** (decision #7, invariant #13):

- A `post_save` signal on `Workspace` (or equivalent hook in the workspace-creation view) fires when `created=True`. If the workspace has zero pods, the handler inserts one row:
  - `name = f"{workspace.name}-pod"`
  - `description = "Auto-created default pod. Rename or add more pods anytime."`
  - `is_default = True`
  - `created_by = workspace.owner` (or the workspace-creator user)
- The signal is idempotent: if a pod already exists (e.g. fixtures seeded one), the handler no-ops. This makes test setup easy.
- Pod name is taken verbatim from `workspace.name`. Django `CharField(max_length=128)` accommodates workspace names up to 124 characters (`name + "-pod"` fits comfortably since workspace names are capped shorter in practice). If the user creates a second workspace with the same name in a hypothetical edge case, it's a different workspace row with its own pod — names are unique per-workspace only.

**Manual — via API** (admin / power-user flow):

- Admin calls `POST /api/v1/workspaces/<id>/pods/` with `name` and optional `description`.
- The new pod is **not** marked default. Existing default pod stays default until the admin explicitly toggles via `PATCH` (see §7.3).

### 7.2 Pod soft-deletion

Per decision #11, pods are **never physically deleted**. The `DELETE` endpoint soft-deletes: sets `deleted_at = now()` and clears `is_default`. This preserves `AgentRun.pod` history and `Issue.assigned_pod` referential integrity without relying on `on_delete=SET_NULL` (which would wipe historical attribution).

**Pre-deletion guards** (enforced atomically; violation → `409 Conflict`):

1. `pod.runners.count() == 0` — all runners must be moved to another pod or revoked first. The UI forces a "move or revoke runners" step.
2. `pod.agent_runs.filter(status__in=NON_TERMINAL_STATUSES).count() == 0` — no active (QUEUED / ASSIGNED / RUNNING / AWAITING_APPROVAL / AWAITING_REAUTH) runs. Prevents the "silently strand QUEUED runs" failure Codex flagged. Operator must cancel or let them complete first.
3. **Last-pod guard** (invariant #13) — if this is the workspace's only active (non-soft-deleted) pod, deletion is rejected with `{"error": "cannot delete the last pod in a workspace; create a replacement first"}`. Admin must `POST` a new pod before deleting the old one. Keeps every workspace resolvable to a default pod at all times.

**Soft-delete side effects** (same transaction):

1. Clear `is_default` (if this was the default pod; the last-pod guard ensures another pod exists and the admin should promote one).
2. Sweep all `Issue.assigned_pod = pod` → `assigned_pod = NULL`. These issues will use `workspace.default_pod` at next run creation.
3. Terminal `AgentRun.pod` FKs are left intact — the row is a tombstone, not gone, so historical analytics still resolve.
4. Fire notification to each affected issue's creator: "Pod _<name>_ was deleted. Issue _<X>_ has been unassigned."

**Interaction with `is_default`**: if the admin soft-deletes the current default pod (permitted because guard #3 confirms another pod exists), the default flag is cleared. The system does **not** auto-promote another pod. A separate `PATCH` call from the admin is required to mark a replacement as default. During the window, `workspace.default_pod` resolves to NULL — which is fine because the `get_default_pod(workspace)` helper falls back to "any active pod in this workspace, oldest first" as a safety net. The admin should promote a real default promptly via UI nudge.

**Restoration**: not exposed in MVP. The schema supports it (clearing `deleted_at` resurrects the row, subject to the conditional unique constraints) but no API endpoint is provided. Mistaken deletion is recovered via DB fixup. Revisit if product demand appears.

### 7.3 Pod default change

- Admin marks another pod as default. Constraint `pod_one_default_per_workspace_when_active` enforces one-at-a-time among non-deleted pods.
- Existing issues with `assigned_pod=NULL` silently start using the new default at next run creation. No backfill.

### 7.4 Runner moved between pods

- Owner or admin reassigns `runner.pod`. Any in-flight run on that runner completes normally. New `QUEUED` runs go to the new pod's queue.
- No automatic rebalancing of existing QUEUED work.

### 7.5 Runner revoked — synchronous in-flight cleanup

**Problem** (decision #10): the current `Runner.revoke()` at `runner/models.py:107-110` only updates `status` + `revoked_at`. The revoke endpoint (`runners.py:44-56`) sends a WS revoke message and closes the socket. Nothing finalizes non-terminal `AgentRun` rows attached to the revoked runner — because `_finalize_run` at `consumers.py:436` requires a `run_completed / run_failed / run_cancelled` message from the runner, which may never arrive after a force-close. Left as-is, runs stay `ASSIGNED/RUNNING` forever, `drain_pod` never refires, and the UI lies to the user.

**Resolution**: revocation must synchronously finalize all non-terminal runs. Rewrite `Runner.revoke()`:

```python
def revoke(self) -> None:
    with transaction.atomic():
        now = timezone.now()
        Runner.objects.filter(pk=self.pk).update(
            status=RunnerStatus.REVOKED,
            revoked_at=now,
        )
        # Finalize every non-terminal run attached to this runner.
        affected = list(
            AgentRun.objects.select_for_update()
            .filter(runner=self)
            .filter(status__in=NON_TERMINAL_STATUSES)
            .values_list("id", "pod_id")
        )
        AgentRun.objects.filter(
            runner=self, status__in=NON_TERMINAL_STATUSES
        ).update(
            status=AgentRunStatus.CANCELLED,
            ended_at=now,
            error="runner revoked",
        )
    # After commit, refire drains for each affected pod — stranded QUEUED
    # runs can't go to the revoked runner but can now land elsewhere (if
    # the pod has other online runners), and the user is notified if the
    # pod is now empty.
    for _run_id, pod_id in affected:
        if pod_id is not None:
            transaction.on_commit(lambda pid=pod_id: drain_pod_by_id(pid))
```

- `NON_TERMINAL_STATUSES = {QUEUED, ASSIGNED, RUNNING, AWAITING_APPROVAL, AWAITING_REAUTH}`. Note: QUEUED runs on a revoked runner shouldn't exist (QUEUED runs have `runner_id IS NULL`), but the filter is defensive.
- The cancellation is visible to users and to downstream analytics immediately, without depending on a cooperating runner daemon.
- If the revoked runner was the last in its pod, the pod's remaining QUEUED runs stay QUEUED; `drain_pod` is still fired but finds no idle runner. Notify those runs' creators ("runner in pod _<name>_ was revoked; no runners left to execute this run — reassign to another pod or add a runner").
- Racing `_finalize_run` messages that arrive after revocation find the run already terminal and are no-ops (existing `is_terminal` check at `consumers.py:277`).

### 7.6 Issue.assigned_pod set to NULL via detail PATCH

- User explicitly unassigns. Next run attempt uses `workspace.default_pod`.

## 8. API Changes

### 8.1 Pods

- `GET /api/v1/workspaces/<id>/pods/` — list pods in workspace (excludes soft-deleted by default); any member. Query param `?include_deleted=1` exposes tombstones for admin views.
- `POST /api/v1/workspaces/<id>/pods/` — create, admin. First pod created in a workspace is auto-marked `is_default=True`.
- `PATCH /api/v1/pods/<id>/` — rename, change description, toggle `is_default`; admin **or** pod `created_by`. Cannot target a soft-deleted pod (404 from default manager).
- `DELETE /api/v1/pods/<id>/` — **soft-delete** (sets `deleted_at`); admin **or** pod `created_by`. Returns `409 Conflict` unless both preconditions hold: zero runners AND zero non-terminal `AgentRun` rows (§7.2). On success, `is_default` is cleared and pointing `Issue.assigned_pod` FKs are swept to NULL in the same transaction.

### 8.2 Runners (updated)

- `GET /api/v1/runners/?workspace=<id>&pod=<id>` — any member; filters by pod optional.
- `PATCH /api/v1/runners/<id>/` — new endpoint: move to another pod (same workspace), rename. Owner or admin.
- `POST /api/v1/runners/<id>/revoke/` — owner or admin (widened from owner-only).

### 8.3 Issues

- Existing issue PATCH accepts `assigned_pod` (UUID of a pod in the same workspace, or null).
- Validation: pod must belong to the issue's workspace and must not be soft-deleted.
- Permitted roles: **Member (15)** and **Admin (20)** can PATCH `assigned_pod`. **Guest (5)** cannot — pod pinning is a workflow decision and matches the existing issue-edit gating. This is enforced alongside the normal issue PATCH permission check, not as a separate layer.

### 8.4 Serializers

- `PodSerializer`: id, name, description, is_default, created_by (minimal), runner_count (derived), workspace.
- `RunnerSerializer` adds: `pod` (UUID) and `pod_detail` (nested `{id, name}`).
- `IssueSerializer` adds: `assigned_pod` (UUID, writeable) and `assigned_pod_detail` (nested, read-only).

## 9. Migration — Initial schema (no legacy data)

pi-dash has no production data yet, so the three-stage migration (nullable → backfill → tighten) that earlier drafts proposed is unnecessary. The design collapses to **two migrations**, one per Django app, both applying final constraints directly.

### 9.1 `apps/api/pi_dash/runner/migrations/0004_add_pod_and_run_identity.py`

Single schema migration, no `RunPython` step:

- Create `pod` table with `deleted_at`, `PodManager`, conditional unique constraints (§4.1).
- Add `runner.pod_id` as `NOT NULL` FK with `on_delete=PROTECT`. Because no `Runner` rows exist yet, the NOT NULL constraint is satisfied trivially.
- Add `agent_run.pod_id` as `NOT NULL` FK with `on_delete=PROTECT`.
- Add `agent_run.created_by_id` as `NOT NULL` FK to `auth_user` with `on_delete=PROTECT`.
- Alter `agent_run.owner_id` to `NULL=TRUE`, `on_delete=SET_NULL` (decision #9).
- Drop unique constraint `(workspace, name)` on `runner`; add `(pod, name)`.

### 9.2 `apps/api/pi_dash/db/migrations/0126_issue_assigned_pod.py`

- Add `issue.assigned_pod_id` as nullable FK to `runner.Pod` with `on_delete=PROTECT` (§4.4).

### 9.3 Post-migration setup

Nothing required. When the backend starts, any existing workspaces (none in prod; possibly some in dev/test fixtures) can lazily get their default pods via the `post_save` signal on new workspaces. For dev/test fixtures that create workspaces before the signal is wired in, a management command `pi_dash.runner.management.commands.ensure_workspace_pods` idempotently backfills default pods for any workspace that doesn't have one — run it once after deploy in non-prod environments to be safe.

### 9.4 First-run behavior

- User creates workspace → signal fires → `<workspace.name>-pod` pod created with `is_default=True`.
- User generates runner registration token → registers runner → runner lands in `workspace.default_pod`.
- User creates issue → `issue.assigned_pod = workspace.default_pod`.
- User moves issue to In Progress → dispatcher resolves `assigned_pod`, finds the runner, dispatches work.

Zero pod-related setup required for the golden path. Pods only become visible in UI when the user explicitly navigates to the pod section or the issue's advanced settings.

## 10. Tests (pytest — matches `rules/python/testing.md`)

Unit — `tests/unit/runner/test_matcher.py`:

- `select_runner_in_pod` returns None when pod empty.
- Returns None when all runners in pod are busy / offline / stale.
- Returns freshest-heartbeat idle runner among multiple candidates.
- Ignores runners from a different pod.
- `next_queued_run_for_pod` FIFO order.
- `drain_pod` assigns K runs when K runners are idle and N>K runs are queued.

Unit — `tests/unit/runner/test_pod.py`:

- Creating a pod with duplicate name in same workspace (both active) fails.
- Creating a pod with the same name as a soft-deleted pod in the same workspace **succeeds** (conditional unique constraint).
- Only one `is_default=True` pod per workspace among active pods.
- Soft-deleting a pod with runners is blocked (409).
- Soft-deleting a pod with non-terminal runs is blocked (409).
- Soft-deleting a pod with only terminal runs succeeds; `AgentRun.pod` FKs remain intact.
- Issues pointing at a soft-deleted pod are swept to `assigned_pod=NULL`.
- **Last-pod guard**: soft-deleting the workspace's only active pod returns 409; soft-deleting when a second pod exists succeeds.

Unit — `tests/unit/runner/test_workspace_signal.py` **(new)**:

- Creating a new `Workspace` auto-creates one pod named `<workspace.name>-pod` with `is_default=True`.
- Creating a workspace with pre-existing pods (e.g. via fixture) is a no-op — no duplicate default pod.
- Signal is idempotent: calling `ensure_workspace_pods` management command on a workspace that already has a default pod creates nothing.

Integration — `tests/integration/runner/test_dispatch.py`:

- Create AgentRun with pod, no idle runners → QUEUED, no assignment.
- Runner completes → next QUEUED run in same pod gets dispatched.
- Two workspaces, two pods: runs stay in their lane.
- Runner moves to different pod mid-queue → old pod's queue not served by that runner anymore.
- Direct `POST /runner/runs/` with `workspace_id` the caller isn't a member of → 403.
- Direct `POST` with `work_item_id` whose workspace ≠ `workspace_id` → 400.
- Direct `POST` with `pod_id` whose workspace ≠ `workspace_id` → 400.
- Direct `POST` with `pod_id` pointing at a soft-deleted pod → 400.
- Request body attempting to override `created_by` is ignored; DB row has `created_by = request.user`.
- Direct run creation with no explicit pod and no workspace default returns `409 {"error": "no pod available"}` and creates no row.
- **Billing capture**: at assignment, `AgentRun.owner` is set to `runner.owner` in the same transaction as `runner`/`status=ASSIGNED`; no row transitions to `ASSIGNED` with `owner=NULL`.
- **User deletion survives audit**: deleting the `runner.owner` user → associated `AgentRun.owner` becomes NULL but row remains; `created_by` is unaffected and permission checks still work for the creator.

Integration — `tests/integration/space/test_issue.py`:

- New issue in workspace with default pod → `assigned_pod` populated.
- New issue in workspace without default pod → `assigned_pod=NULL`, creation succeeds.
- PATCH `assigned_pod` to a pod in a different workspace → 400.
- PATCH `assigned_pod` to a soft-deleted pod → 400.
- PATCH `assigned_pod=null` → clears.
- Issue goes In Progress with resolvable pod → AgentRun created with `pod=issue.assigned_pod` or `workspace.default_pod`, and `created_by=actor`.
- Issue goes In Progress with no resolvable pod → no AgentRun row created; transition outcome is `"no-pod-available"` and notification fires to actor.

Integration — `tests/integration/runner/test_permissions.py`:

- Non-owner workspace member can list runners, create runs, cancel their own runs.
- Non-admin non-creator cannot delete (soft-delete) a pod.
- Non-owner non-admin cannot revoke a runner.
- List/detail/cancel now filter by `created_by`, not `owner`; a workspace admin sees all.
- Approval endpoints (`approvals.py`) filter by `agent_run.created_by=request.user`; runner owner cannot approve someone else's run.

Integration — `tests/integration/runner/test_revocation.py` **(new)**:

- Revoking a runner with an in-flight ASSIGNED run → run transitions to CANCELLED with `error="runner revoked"`, `ended_at` set, within the same transaction as the revoke.
- Revoking a runner with an AWAITING_APPROVAL run → run is CANCELLED; any PENDING approvals become moot (status not changed, but `agent_run.status=CANCELLED` makes them unactionable).
- Late-arriving `run_completed` message after revocation is a no-op (terminal state preserved).
- Revoking the last runner in a non-empty-queue pod → QUEUED runs remain QUEUED; notification fires to each run's `created_by`.
- Pod drain is re-invoked after revoke so any remaining runners in the pod pick up the work.

Schema / fresh-install — `tests/integration/runner/test_fresh_install.py` **(replaces the legacy-migration test)**:

- A brand-new workspace has exactly one pod (`<name>-pod`, `is_default=True`) immediately after creation.
- Registering a runner with no explicit pod lands it in the workspace default pod.
- Creating an issue with no explicit pod pins it to the workspace default pod.
- End-to-end: create workspace → register runner → create issue → move to In Progress → run dispatches to runner. No pod UI touched.

## 11. Open Questions / Deferred

1. **Billing hooks**: decision #5 says owner-billed. This design doesn't implement metering; it just keeps `runner.owner` as the billable anchor. Actual metering lives in a separate design.
2. **Per-pod approval policies**: decision #6 says run creator approves. A future iteration could add pod-level policies (e.g., `pod-prod` auto-denies all writes without a second approver). Out of scope.
3. **Stranded QUEUED on empty pod**: §7.5 notes that revoking all runners in a pod strands QUEUED work. MVP: notify creator; require manual reassignment. Future: optional auto-migrate to default pod.
4. **Priority & deadlines**: §6.1 uses FIFO. Add priority when we see real need.
5. **Cross-pod spillover**: explicitly out of scope. Revisit if users demand it.
6. **Pod-level capacity metrics / dashboards**: future.
7. **Cross-workspace runner reuse**: a physical runner host can only serve one workspace today (one `workspace` FK). No change in MVP.

## 12. Files Touched

### Backend (apps/api/pi_dash)

- `runner/models.py` — add `PodManager` (filters `deleted_at IS NULL`) and `Pod` model (with `deleted_at`, conditional unique constraints); add `runner.pod`, `agent_run.pod`, `agent_run.created_by`. Rewrite `Runner.revoke()` to synchronously cancel in-flight runs and refire pod drain (§7.5).
- `runner/models.py` — alter `AgentRun.owner` to `null=True, on_delete=SET_NULL` and reinterpret it as billable party captured at assignment time (§5.3).
- `runner/services/matcher.py` — replace `select_runner_for_run` with pod-scoped `select_runner_in_pod`, `next_queued_run_for_pod`; add `drain_pod`.
- `runner/services/permissions.py` — **new** `is_workspace_member`, `workspace_role`, `is_workspace_admin` helpers.
- `runner/services/validation.py` — **new** `validate_run_creation(user, workspace_id, work_item_id, pod_id)` shared by orchestration + direct POST (§6.5).
- `runner/views/runners.py` — switch `owner=request.user` filters to workspace membership; add `PATCH` for pod moves; widen revoke permission to owner-or-admin.
- `runner/views/runs.py` — pod-aware run creation; enforce §6.5 validation; permission checks on list/detail/cancel switch to `created_by`.
- `runner/views/approvals.py` — permission filters switch from `agent_run__owner` to `agent_run__created_by` (decision #6).
- `runner/views/pods.py` — **new** CRUD for pods (soft-delete semantics).
- `runner/serializers.py` — add `PodSerializer`, extend `RunnerSerializer` with `pod`/`pod_detail`, extend `AgentRunSerializer` with `pod` + `created_by` detail; keep `owner` as billable-party read-only.
- `runner/consumers.py` — `_finalize_run` triggers `drain_pod(runner.pod)` on completion.
- `runner/signals.py` — **new** `post_save` handler on `Workspace` that creates `<workspace.name>-pod` with `is_default=True` when `created=True` and the workspace has no pods (§7.1).
- `runner/apps.py` — wire the signal in `ready()`.
- `runner/management/commands/ensure_workspace_pods.py` — **new** idempotent backfill for dev/test environments where workspaces may have been created before the signal was wired in.
- `runner/views/register.py` — runner registration resolves `pod` from the request (or falls back to `workspace.default_pod`) and validates the pod belongs to the workspace.
- `orchestration/service.py` — `_dispatch_to_runner` rewritten to resolve pod and call `drain_pod`. `_create_and_dispatch_run` sets `AgentRun.created_by = actor` (required). `_resolve_owner` becomes `_resolve_fallback_creator` and is used only when `actor is None` on legacy call sites.
- `db/models/issue.py` — add `assigned_pod`.
- `space/views/issue.py` — populate default pod on create; accept `assigned_pod` on PATCH with workspace validation and non-deleted pod check.
- `space/serializers/issue.py` — expose `assigned_pod` + nested detail.
- Migrations (two, both with final constraints applied directly — no backfill ceremony because no production data exists):
  - `apps/api/pi_dash/runner/migrations/0004_add_pod_and_run_identity.py` — create `pod` table; add `runner.pod` (NOT NULL), `agent_run.pod` (NOT NULL), `agent_run.created_by` (NOT NULL); alter `agent_run.owner` to nullable + `SET_NULL`; replace `(workspace, name)` uniqueness on `runner` with `(pod, name)`.
  - `apps/api/pi_dash/db/migrations/0126_issue_assigned_pod.py` — add `issue.assigned_pod` (nullable).
- Tests as listed in §10.

### Frontend (apps/web)

- Runner section: pods list, pod detail with runners inside, "Create Pod" (admin), "Move Runner" UI.
- Issue detail: pod picker (dropdown of workspace pods), read-only if user lacks edit permission on the issue.
- Issue create: shows workspace default pod as a hint, editable before create.

Specific files deferred until implementation.

## 13. Non-Goals

- No priority queues, no deadlines, no cross-pod spillover.
- No cross-workspace pods or runners.
- No runner-side queue protocol changes. The daemon continues to receive `assign` messages one at a time; the cloud never sends more than one assignment at a time to a given runner (the busy-state exclusion in `drain_pod` enforces this).
- No migration of existing `AgentRun.pod` data — historical rows stay NULL. Analytics can still resolve pod via `runner.pod` for assigned rows.
- No change to the runner authentication / registration handshake.
- No change to the Rust runner binary in this phase.
- **No change to `AgentRun.owner` semantics on historical rows.** Legacy rows retain whatever value they had; `owner` on new rows becomes the runner-owner-at-assignment (nullable until assigned). All permission logic moves to `created_by` to sidestep the ambiguity rather than rewriting `owner` in place.
- **No legacy data migration.** pi-dash has no production data; the initial schema migration applies final constraints directly. No `(owner, workspace)`-based personal-pod creation, no `AgentRun.created_by` backfill, no constraint-tightening stage. If this design is later applied to an environment with real data, a separate migration PR would add those stages.
