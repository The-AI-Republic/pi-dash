# Loop (Auto Project Management) — Design

**Status:** Draft
**Date:** 2026-06-12
**Scope:** MVP. Instance-defined "loop jobs" (prompt + timer) that periodically run each user's **AI assistant** against each workspace they belong to, performing routine issue management on their behalf. Backend (catalog + Beat + dispatch through the existing assistant runtime), a user-facing enable/disable surface in `apps/web` settings, and a job-management page in `apps/admin`.

**Naming rule:** "Loop" is the internal/code name only. Every user-visible string says **"Auto Project Management"** (settings section, descriptions, API route segment `auto-pm`). Writes the loop makes are attributed exactly like the assistant's: comments carry `speaker_label="Pi Dash AI"`. Only the instance-admin UI (`apps/admin`) may use the word "Loop".

---

## 1. Problem

PR #233 shipped a native AI assistant (`apps/api/pi_dash/assistant/`): a chat agent that can search, read, create, update, and comment on the user's issues, running under the user's identity, the user's BYOK LLM key, and the user's workspace/project permissions. But it only acts when the user types at it.

A large class of project-management work is _routine_: "close issues whose PR merged", "flag stale issues", "nudge unassigned work". Users shouldn't have to ask for these every day — the platform should run them automatically and the user should simply benefit from the result.

We want a **loop**: a set of instance-defined jobs (prompt + timer), authored by the instance operator (AI Republic on Pi Dash Cloud), that periodically execute through each user's assistant in each workspace they belong to.

## 2. Decisions already made (settled in discussion — do not re-litigate)

1. **A loop run executes _as the user_.** Same identity, same BYOK `UserLLMConfig`, same role/membership enforcement as a chat turn. What the user can do, the loop can do; what the user can't, the loop can't. There is **no** service account, no system actor, no permission bypass of any kind.
2. **The unit of execution is the membership edge.** userA∈ws1, userB∈ws1, userA∈ws2 → three independent loops. There is no workspace-level or delegate execution, ever.
3. **Jobs are instance-level and opaque in operation.** The operator defines jobs in the admin platform; they auto-apply to every membership edge. Users cannot create/edit jobs, change prompts, or change timers.
4. **Users can enable/disable.** An "Auto Project Management" section in user settings lists the jobs with on/off toggles (default **on**) plus a master pause. Toggle only — no editing.
5. **Quiet failure is fine.** A job a user lacks permission for (e.g. an admin-only job running under a member account) silently does less or nothing. In-run permission errors are already surfaced to the model as retryable tool errors (`ToolPermissionError(ModelRetry)`, `assistant/tools/_scoping.py:30`), so the agent skips what it can't touch and continues — "different loops perform according to user's authorities" falls out for free.
6. **Loop ≠ Scheduler.** The Project Scheduler dispatches `AgentRun`s to external runner pods, is workspace-authored, and is installed per-project. The loop dispatches **assistant turns** on in-process Celery workers, is instance-authored, and applies per membership edge with zero installation.

## 3. Goals

- **Zero-setup benefit.** A new builtin job lights up for every eligible user with no action on their part (and no backfill — see §6.3 `LoopUserPreference` defaults).
- **Total reuse of the assistant runtime.** A loop run _is_ an `AssistantTurn` in a hidden `AssistantThread`. No parallel agent, no parallel permission layer, no parallel finalization/usage/error machinery. `run_assistant_turn` (`assistant/tasks.py:380`) is called **unchanged**.
- **Implicit consent on OSS.** Loop runs spend the user's own BYOK key, so eligibility requires a working `UserLLMConfig` — "has configured the assistant" is the opt-in. On Cloud, the existing `pi_dash.ee.assistant.model_provider` seam supplies platform keys and the gate becomes a plan decision, not a code path (§7.8).
- **Operator observability.** The admin platform can see, per job, what ran, what was skipped and why, what it cost (`AssistantTurn.usage`), and what it did.
- **Honest attribution.** Issue writes carry `created_via="loop"`; comments show as "Pi Dash AI" acting under the user's account, so a user can always tell why an issue moved.

## 4. Non-Goals (MVP)

- **No user-authored jobs**, no per-user prompt/interval overrides, no per-workspace user preferences (the toggle is per user, global).
- **No visible transcript.** Loop threads are hidden from the assistant UI. Flipping the filter later is the "show me what it did" feature — no migration needed.
- **No event-driven jobs** — timers only.
- **No retry policy.** A failed run waits for the next tick (`AssistantTurn.error_code` records why).
- **No cross-run coordination.** Two members' loops touching the same shared issue converge by data, not locks: list-tools return only issues still needing the action, so the second run is a cheap no-op. Residual duplicate comments are a prompt-quality problem, as with the scheduler (§3 of its design).
- **No notification/digest of loop actions.** Users discover results in the issue activity itself.

## 5. Relationship to existing systems

|                   | Project Scheduler                       | Assistant (chat)                 | **Loop (this design)**                |
| ----------------- | --------------------------------------- | -------------------------------- | ------------------------------------- |
| Definition author | Workspace admin                         | n/a (user types a message)       | **Instance operator** (`apps/admin`)  |
| Bound to          | Project, via `SchedulerBinding` install | (workspace, user) thread         | **Membership edge** (job × ws × user) |
| Executes via      | `AgentRun` → external runner pod        | `run_assistant_turn` Celery task | **Same `run_assistant_turn` task**    |
| Identity          | `binding.actor`                         | `thread.user`                    | `thread.user` (the member)            |
| LLM credentials   | Runner's own (Codex session)            | User BYOK / cloud seam           | User BYOK / cloud seam                |
| User setup        | Install per project                     | Configure LLM, open chat         | **None** (toggle to opt out)          |
| Cadence           | Per-install RRULE bundle                | On demand                        | Per-job RRULE bundle                  |

All three coexist. The loop reuses the scheduler's _scheduling_ idioms (Beat scanner, SFU claim, RRULE helpers in `pi_dash/bgtasks/_rrule.py`) and the assistant's _execution_ path; it adds no third runtime.

## 6. Schema

**File placement** mirrors the scheduler exactly: models in `pi_dash/db/models/loop.py` (exported from `db/models/__init__.py`), Beat tasks in `pi_dash/bgtasks/loop.py`, app-level logic (builtins, eligibility, dispatch, views) in `pi_dash/loop/` as a plain package — no new `INSTALLED_APPS` entry, no app-local migrations. The two migrations land in `db` and `assistant` respectively.

All three models extend `BaseModel` (`pi_dash/db/models/base.py` — audit fields + soft delete), which is why every unique constraint below is conditional on `deleted_at__isnull=True` (same tombstone-collision reasoning as scheduler design §5).

### 6.1 `LoopJob` — the instance catalog entry

```python
# pi_dash/db/models/loop.py
from __future__ import annotations

from django.conf import settings
from django.db import models

from .base import BaseModel

# Reuse the scheduler's operator-error cap so admin surfaces stay consistent.
from .scheduler import LAST_ERROR_MAX_LEN  # noqa: F401  (re-exported for bgtasks)


class LoopJob(BaseModel):
    slug = models.CharField(max_length=64)                 # e.g. "auto-close-merged"
    name = models.CharField(max_length=255)                # admin-facing
    public_name = models.CharField(max_length=255)         # user-facing; never contains "loop"
    public_description = models.TextField(blank=True)      # shown on the settings toggle card
    prompt = models.TextField()                            # becomes the turn's user message verbatim
    min_role = models.PositiveSmallIntegerField(default=15)  # ROLE_CHOICES (db/models/workspace.py:19)
    enabled = models.BooleanField(default=True)             # per-job kill switch (admin)
    is_builtin = models.BooleanField(default=True)          # seeded in-tree; admin-created rows use False

    # Timer — same RRULE bundle subset as SchedulerBinding, evaluated by
    # pi_dash/bgtasks/_rrule.next_fire_from_rrule (which treats rdates/exdates
    # as optional, so adding them later is non-breaking). rrule may NOT be
    # empty here: a single-shot loop job is meaningless.
    dtstart = models.DateTimeField()
    rrule = models.CharField(max_length=255)                # e.g. "FREQ=DAILY;BYHOUR=3;BYMINUTE=0"
    tzid = models.CharField(max_length=64, default="UTC")

    class Meta:
        db_table = "loop_jobs"
        ordering = ("-created_at",)
        constraints = [
            models.UniqueConstraint(
                fields=["slug"],
                condition=models.Q(deleted_at__isnull=True),
                name="loop_job_unique_slug_when_active",
            ),
        ]
```

Validation at the API layer (§9.2): `validate_rrule_string(rrule)` (`bgtasks/_rrule.py:218`) plus a loop-specific floor — reject `FREQ=MINUTELY` and `FREQ=HOURLY` with `INTERVAL < 1` hour equivalent; loop jobs fire LLM runs per membership edge, so the minimum cadence is **hourly** (`_validate_loop_rrule` in `pi_dash/loop/admin_views.py`, returns 400 `{"error": "rrule_too_frequent"}`).

### 6.2 `LoopTarget` — the membership-edge cursor

```python
class LoopTarget(BaseModel):
    job = models.ForeignKey("db.LoopJob", on_delete=models.CASCADE, related_name="targets")
    workspace = models.ForeignKey("db.Workspace", on_delete=models.CASCADE, related_name="loop_targets")
    user = models.ForeignKey(settings.AUTH_USER_MODEL, on_delete=models.CASCADE, related_name="loop_targets")

    # The hidden conversation this target's runs land in. Recreated on
    # rotation (§7.6); SET_NULL so deleting a thread can't kill the cursor.
    thread = models.ForeignKey(
        "assistant.AssistantThread", on_delete=models.SET_NULL,
        null=True, blank=True, related_name="+",
    )

    next_run_at = models.DateTimeField(null=True, blank=True)   # NULL = newly created, stagger pending
    last_run = models.ForeignKey(
        "assistant.AssistantTurn", on_delete=models.SET_NULL,
        null=True, blank=True, related_name="+",
    )
    # Sparse skip diagnostics — overwritten in place, never accreted (a
    # row-per-skip log at edge × job × day scale would dwarf every other table).
    last_skipped_at = models.DateTimeField(null=True, blank=True)
    last_skip_reason = models.CharField(max_length=64, blank=True, default="")

    class Meta:
        db_table = "loop_targets"
        ordering = ("-created_at",)
        constraints = [
            models.UniqueConstraint(
                fields=["job", "workspace", "user"],
                condition=models.Q(deleted_at__isnull=True),
                name="loop_target_unique_edge_when_active",
            ),
        ]
        indexes = [models.Index(fields=["next_run_at"], name="loop_target_due_idx")]
```

Skip reasons (string enum, defined as `SkipReason(models.TextChoices)` in the same file):

| value                | set by         | meaning                                         |
| -------------------- | -------------- | ----------------------------------------------- |
| `user_disabled`      | scanner / fire | `LoopUserPreference(job, enabled=False)` exists |
| `master_paused`      | scanner / fire | master-pause preference row exists              |
| `min_role`           | scanner / fire | membership role < `job.min_role`                |
| `llm_config_missing` | scanner / fire | no usable LLM credentials (§7.8 seam)           |
| `membership_gone`    | scanner / fire | no active `WorkspaceMember` row for the edge    |
| `turn_active`        | fire           | previous run still in flight on the thread      |
| `dispatch_error`     | fire           | unexpected exception creating the turn (logged) |

There is **no `LoopRun` model.** The run _is_ the `AssistantTurn` — status, `usage`, `model_used`, `error_code`, timestamps are already there (`assistant/models.py:63-96`), and the transcript (what the agent actually did, tool calls included) is the thread's `AssistantMessage` rows. Admin run-history reads turns through `LoopTarget.thread`. Duplicating any of that onto a loop-side model repeats the mistake the scheduler design explicitly avoided (its §5 note on not mirroring `AgentRun.status`).

### 6.3 `LoopUserPreference` — the user's toggles

```python
class LoopUserPreference(BaseModel):
    user = models.ForeignKey(settings.AUTH_USER_MODEL, on_delete=models.CASCADE, related_name="loop_preferences")
    # NULL job = the master "pause all Auto Project Management" switch.
    job = models.ForeignKey("db.LoopJob", on_delete=models.CASCADE, null=True, blank=True,
                            related_name="user_preferences")
    enabled = models.BooleanField(default=True)

    class Meta:
        db_table = "loop_user_preferences"
        ordering = ("-created_at",)
        constraints = [
            models.UniqueConstraint(
                fields=["user", "job"],
                condition=models.Q(deleted_at__isnull=True),
                name="loop_pref_unique_user_job_when_active",
            ),
            models.UniqueConstraint(
                fields=["user"],
                condition=models.Q(job__isnull=True, deleted_at__isnull=True),
                name="loop_pref_unique_user_master_when_active",
            ),
        ]
```

**Absence of a row means enabled.** This is what makes new builtin jobs light up for everyone with zero backfill (decision §2.4 + goal §3.1). Only opt-outs are stored; the toggle endpoints upsert (`update_or_create`) rather than insert.

### 6.4 `AssistantThread.kind` — hiding loop threads

```python
# Added to AssistantThread (apps/api/pi_dash/assistant/models.py:22)
class ThreadKind(models.TextChoices):
    CHAT = "chat", "Chat"
    LOOP = "loop", "Loop"

kind = models.CharField(max_length=16, choices=ThreadKind.choices, default=ThreadKind.CHAT)
```

`AssistantThreadListCreateEndpoint.get` (`assistant/views/threads.py:22`) adds `kind=ThreadKind.CHAT` to its filter. That is the _entire_ opacity mechanism. The other thread/message/SSE endpoints stay as-is: they are already owner-scoped (`owned_thread`, `views/_base.py:31`), so a user inspecting their own loop thread by ID leaks nothing — it's their own agent's work. POST `/threads/` always creates `kind="chat"` (the field is not serializer-writable).

### 6.5 Migrations

1. **`db`** — `00XX_loop_mvp.py`: creates `loop_jobs`, `loop_targets`, `loop_user_preferences`; data-migration step seeds the builtin job (§8.1) with `enabled=False` (rollout §13). The seed imports prompt text from `pi_dash/loop/builtins.py` (module is Django-free, like `_rrule.py`, so the migration can import it).
2. **`assistant`** — `0002_thread_kind.py`: additive `kind` column with default; no backfill (existing rows are chat threads by definition).

Test DBs run with `--reuse-db --nomigrations`; CI and the local container need one `--create-db` run after this lands (same as PR #233's migration).

## 7. Execution

### 7.1 Beat entry

```python
# apps/api/pi_dash/celery.py — next to "scan-due-scheduler-bindings" (celery.py:118)
"scan-due-loop-targets": {
    "task": "pi_dash.bgtasks.loop.scan_due_targets",
    "schedule": crontab(minute="*"),
},
```

Singleton-Beat caveat is inherited from the scheduler (`bgtasks/scheduler.py:22`): double Beat doubles scan rate but SFU claims keep correctness.

### 7.2 Scanner — `pi_dash/bgtasks/loop.py::scan_due_targets`

```python
@shared_task(name="pi_dash.bgtasks.loop.scan_due_targets")
def scan_due_targets() -> int:
    if not getattr(settings, "LOOP_ENABLED", True):
        return 0
    now = timezone.now()
    _reconcile_targets(now)                      # (a) — internally throttled
    return _fan_out_due(now)                     # (b)
```

**(a) Reconcile** — create missing `LoopTarget` rows for every (enabled job × active membership edge). Runs only when `now.minute % 15 == 0` (cheap throttle; a new member waits ≤15 min for a cursor, and their first fire is next occurrence + stagger anyway):

```python
def _reconcile_targets(now) -> None:
    if now.minute % LOOP_RECONCILE_EVERY_MINUTES != 0:
        return
    for job in LoopJob.objects.filter(enabled=True, deleted_at__isnull=True):
        existing = LoopTarget.objects.filter(
            job=job, deleted_at__isnull=True,
            workspace_id=OuterRef("workspace_id"), user_id=OuterRef("member_id"),
        )
        edges = (
            WorkspaceMember.objects
            .filter(is_active=True, member__is_active=True, deleted_at__isnull=True)
            .annotate(has_target=Exists(existing))
            .filter(has_target=False)
            .values_list("workspace_id", "member_id")
        )
        nxt = next_fire_from_rrule(dtstart=job.dtstart, rrule_str=job.rrule, tzid=job.tzid, now=now)
        if nxt is None:
            continue  # bad RRULE; admin API validation should make this unreachable
        LoopTarget.objects.bulk_create(
            [
                LoopTarget(job=job, workspace_id=ws, user_id=uid,
                           next_run_at=nxt + _stagger(job.id, ws, uid))
                for ws, uid in edges.iterator(chunk_size=1000)
            ],
            ignore_conflicts=True,   # races with concurrent scans collapse on the conditional unique
            batch_size=500,
        )
```

New targets get `next_run_at = next fire + stagger`, **not** NULL-meaning-due — a freshly eligible user waits for the next scheduled occurrence rather than triggering a burst the minute a job is created. Membership _removal_ is handled at fire time (`membership_gone`), not by reconcile — no `WorkspaceMember` signal wiring in MVP.

**(b) Fan out** — claim due target IDs, pre-filtered by eligibility so no Celery task is queued for a target that would only skip:

```python
def _fan_out_due(now) -> int:
    due = eligibility.eligible_due_targets(now)          # §7.8 — returns a queryset of ids
    ids = list(due.order_by("next_run_at")[:LOOP_MAX_DISPATCH_PER_TICK].values_list("id", flat=True))
    for target_id in ids:
        fire_loop_target.delay(str(target_id))
    _advance_ineligible_due(now)                          # see below
    return len(ids)
```

Targets that are **due but ineligible** must still have their cursor advanced, or they would be re-examined every minute forever. `_advance_ineligible_due` runs one bulk pass per skip-reason class (each is a queryset difference of `due_targets - eligible`, annotated per reason), setting `next_run_at` to the job's next occurrence + stagger and stamping `last_skipped_at=now`, `last_skip_reason=<reason>` via `bulk_update` in batches. Because RRULE evaluation is per-job, group the updates by `job_id` and compute `next_fire_from_rrule` once per job per tick. Over-cap eligible targets (beyond `LOOP_MAX_DISPATCH_PER_TICK`) are _not_ advanced — they stay due and drain on subsequent ticks (backpressure by design).

### 7.3 Stagger

A daily job naively fires every eligible edge in the same minute — a thundering herd of N×M LLM calls. Each target's fire time gets a deterministic offset (no `random` in scheduling paths — reproducible tests, stable per edge):

```python
from zlib import crc32

def _stagger(job_id, workspace_id, user_id) -> timedelta:
    window = max(1, int(getattr(settings, "LOOP_STAGGER_WINDOW_MINUTES", 60)))
    seed = f"{job_id}:{workspace_id}:{user_id}".encode()
    return timedelta(minutes=crc32(seed) % window)
```

Default window 60 minutes spreads a 1000-edge instance to ~17 dispatches/minute. The stagger is applied every time `next_run_at` is advanced (reconcile, fire, ineligible-advance), so a given edge always fires at the same offset within its job's window.

### 7.4 Per-target fire — `pi_dash/bgtasks/loop.py::fire_loop_target`

Follows the scheduler's claim shape (`bgtasks/scheduler.py:144`) but with **no rollback phase**: dispatch here is local row creation inside the same database, not a remote pod match that can transiently fail. If Phase 2 raises, the cursor stays advanced, the exception is logged, and `last_skip_reason="dispatch_error"` is recorded — next occurrence retries naturally.

```python
@shared_task(name="pi_dash.bgtasks.loop.fire_loop_target", bind=True, max_retries=0)
def fire_loop_target(self, target_id: str) -> bool:
    if not getattr(settings, "LOOP_ENABLED", True):
        return False

    # ----- Phase 1: claim under SFU, re-check eligibility, advance cursor -----
    with transaction.atomic():
        target = (
            LoopTarget.objects.select_for_update(of=("self",))
            .select_related("job", "workspace", "user")
            .filter(pk=target_id, deleted_at__isnull=True)
            .first()
        )
        if target is None:
            return False
        now = timezone.now()
        if target.next_run_at is not None and target.next_run_at > now:
            return False                      # raced the scanner; another fire already claimed
        job = target.job
        nxt = next_fire_from_rrule(dtstart=job.dtstart, rrule_str=job.rrule, tzid=job.tzid, now=now)
        target.next_run_at = (nxt + _stagger(job.id, target.workspace_id, target.user_id)) if nxt else None

        skip = eligibility.check(target)      # §7.8 — None | SkipReason value
        if skip is not None:
            target.last_skipped_at = now
            target.last_skip_reason = skip
            target.save(update_fields=["next_run_at", "last_skipped_at", "last_skip_reason", "updated_at"])
            return False
        target.save(update_fields=["next_run_at", "updated_at"])

    # ----- Phase 2: dispatch a turn (own transaction; mirrors the message POST) -----
    return dispatch.dispatch_loop_turn(target_id)
```

### 7.5 Dispatch — `pi_dash/loop/dispatch.py::dispatch_loop_turn`

Mirrors the chat message POST handler (`assistant/views/messages.py:84-104`) minus HTTP, plus thread management. Reuses `events.create_message` (`assistant/runtime/events.py`) so `seq` allocation and SSE event rows behave identically to chat.

```python
def dispatch_loop_turn(target_id: str) -> bool:
    with transaction.atomic():
        target = (
            LoopTarget.objects.select_for_update(of=("self",))
            .select_related("job", "workspace", "user")
            .get(pk=target_id)
        )
        thread = _ensure_thread(target)                    # get-or-create + rotation, below

        locked = AssistantThread.objects.select_for_update().get(pk=thread.pk)
        if locked.active_turn_id is not None:
            # Previous run still in flight — skip, don't queue (scheduler §9.1 policy).
            # The stale-turn sweep (assistant/tasks.py:389) guarantees this clears
            # even after a worker crash, so a target can never wedge permanently.
            target.last_skipped_at = timezone.now()
            target.last_skip_reason = SkipReason.TURN_ACTIVE
            target.save(update_fields=["last_skipped_at", "last_skip_reason", "updated_at"])
            return False

        turn = AssistantTurn.objects.create(thread=locked, status=TurnStatus.QUEUED)
        user_msg = events.create_message(
            locked, MessageKind.USER, turn=turn,
            display_content=target.job.prompt, status=MessageStatus.COMPLETED,
        )
        turn.user_message = user_msg
        turn.save(update_fields=["user_message"])
        locked.active_turn = turn
        locked.save(update_fields=["active_turn", "updated_at"])

        target.last_run = turn
        target.last_skip_reason = ""
        target.save(update_fields=["last_run", "last_skip_reason", "updated_at"])

    transaction.on_commit(lambda tid=str(turn.id): run_assistant_turn.delay(tid))
    return True
```

`_ensure_thread(target)`:

1. If `target.thread_id` is set and the thread row exists with `kind="loop"`, count its messages; if `count < MAX_THREAD_MESSAGES - LOOP_ROTATION_HEADROOM` (`assistant/errors.py:93` minus headroom, default 200−30), return it.
2. Otherwise create `AssistantThread(workspace=target.workspace, user=target.user, kind=ThreadKind.LOOP, title=target.job.public_name, is_archived=False)`, archive the old thread (`is_archived=True`) if any, point `target.thread` at the new one, return it. Rotation resets the run's memory (§7.7) — acceptable; job prompts must not _depend_ on memory for correctness, only benefit from it.

Note the chat-side guards that do **not** apply here: no LLM-config 422 pre-check (eligibility already covered it; if the key is deleted between claim and run, `_run_turn` fails the turn with `llm_config_missing` — correct and visible to the admin), no throttle (Beat is the rate limiter), no `MAX_THREAD_MESSAGES` 409 (rotation makes it unreachable), no auto-title.

### 7.6 What runs — stock assistant machinery, end to end

From `run_assistant_turn.delay(turn_id)` onward, **nothing is loop-specific**: `_load_context` (`tasks.py:70`) resolves user/deps from the thread; `resolve_model_for_user` (imported from `pi_dash.ee.assistant.model_provider`, `tasks.py:290`) resolves credentials; `UsageLimits(request_limit=25, tool_calls_limit=20)` (`tasks.py:313`) bounds per-run cost; soft/hard time limits (`ASSISTANT_TURN_SOFT_LIMIT/HARD_LIMIT`) bound wall clock; `_complete_turn`/`_fail_turn` finalize and clear `active_turn`; the stale-turn sweep recovers crashes.

### 7.7 Loop-mode runtime seam (the only assistant changes)

Three small, mode-gated changes:

**(a) `AssistantDeps.mode`** (`runtime/deps.py:20`): add field `mode: str = "chat"` (values `"chat" | "loop"`) and a derived property:

```python
@property
def created_via(self) -> str:
    return "assistant" if self.mode == "chat" else "loop"
```

`_load_context` (`tasks.py:70`) sets `mode=thread.kind` when building deps.

**(b) Unattended instructions** (`runtime/instructions.py`): append a block in `dynamic_instructions(ctx)` when `deps.mode == "loop"` — this composes with `BASE_INSTRUCTIONS` rather than replacing it, so all standing rules (investigate-first, untrusted-content tags, error handling) still apply. Rule 3 of `BASE_INSTRUCTIONS` ("ask before bulk changes") is overridden explicitly because there is nobody to ask:

```python
LOOP_INSTRUCTIONS = """\
## Unattended mode
You are running as a scheduled maintenance task. No human reads your reply \
live, and nobody can answer questions — never ask; when a judgement is \
ambiguous, skip that item instead of guessing. Perform only the actions your \
task instructions explicitly call for. Never delete anything. The bulk-change \
confirmation rule does not apply, but act on at most {LOOP_MAX_WRITES} items \
per run; if more qualify, handle the oldest and note the remainder in your \
summary. End with a short plain-text summary of every action you took, or \
"No action needed."
"""
```

(`LOOP_MAX_WRITES` formatted from settings, default 10 — a per-run blast-radius cap enforced by instruction; the hard backstop remains `tool_calls_limit=20`.)

**(c) Attribution**: `tools/issues.py:182` changes `created_via="assistant"` → `created_via=ctx.deps.created_via` (same in `update_issue`'s activity-tracking call if it passes an origin). Comments are untouched: `speaker_label="Pi Dash AI"` already says the right thing to users (naming rule).

**(d) History budget**: `load_history` (`runtime/history.py:27`) currently reads `ASSISTANT_HISTORY_MAX_TURNS` (default 40). It gains a kind-aware cap — loop threads replay history too (memory: "yesterday I closed #42") but at a tighter default because it's a daily token cost:

```python
setting = "ASSISTANT_LOOP_HISTORY_MAX_TURNS" if thread.kind == "loop" else "ASSISTANT_HISTORY_MAX_TURNS"
default = 5 if thread.kind == "loop" else 40
max_turns = max(1, int(getattr(settings, setting, default)))
```

### 7.8 Eligibility — one seam, used by scanner and fire

`pi_dash/loop/eligibility.py` is the single place "may this edge run this job now?" is answered, in two forms (queryset for bulk pre-filter, per-row for the fire-time re-check). It is also the **Cloud seam**: the BYOK credential check is one overridable function, mirroring how `resolve_model_for_user` lives behind `pi_dash.ee.assistant.model_provider`.

```python
# pi_dash/loop/eligibility.py
def llm_available_q() -> Exists:
    """Exists() subquery: does OuterRef('user_id') have usable LLM credentials?

    CE: a UserLLMConfig with a key. The ee/cloud overlay replaces this to also
    admit plan-entitled users with platform keys."""
    return Exists(UserLLMConfig.objects.filter(
        user_id=OuterRef("user_id"), api_key_encrypted__isnull=False,
    ))

def eligible_due_targets(now) -> QuerySet[LoopTarget]:
    return (
        LoopTarget.objects
        .filter(deleted_at__isnull=True, job__enabled=True, job__deleted_at__isnull=True)
        .filter(Q(next_run_at__lte=now) | Q(next_run_at__isnull=True))
        .annotate(
            _member=Exists(WorkspaceMember.objects.filter(
                workspace_id=OuterRef("workspace_id"), member_id=OuterRef("user_id"),
                is_active=True, deleted_at__isnull=True, role__gte=OuterRef("job__min_role"),
            )),
            _job_off=Exists(LoopUserPreference.objects.filter(
                user_id=OuterRef("user_id"), job_id=OuterRef("job_id"),
                enabled=False, deleted_at__isnull=True,
            )),
            _paused=Exists(LoopUserPreference.objects.filter(
                user_id=OuterRef("user_id"), job__isnull=True,
                enabled=False, deleted_at__isnull=True,
            )),
            _llm=llm_available_q(),
        )
        .filter(_member=True, _job_off=False, _paused=False, _llm=True)
    )

def check(target) -> Optional[str]:
    """Fire-time re-check of one claimed target. Returns a SkipReason value or None.
    Same predicates as eligible_due_targets, evaluated freshest-wins under the
    caller's SFU — preferences/membership/config may have changed since the scan."""
```

`check` evaluates the predicates in fixed order (`master_paused`, `user_disabled`, `membership_gone`/`min_role`, `llm_config_missing`) so the recorded skip reason is deterministic.

## 8. Builtin job and the PR-status tool

### 8.1 The seeded job (`pi_dash/loop/builtins.py`)

```python
BUILTIN_LOOP_JOBS = [
    BuiltinLoopJob(
        slug="auto-close-merged",
        name="Auto-close merged-PR issues",
        public_name="Close issues when their PR merges",
        public_description=(
            "Checks your projects once a day and marks an issue Done when the "
            "pull request that implements it has been merged."
        ),
        min_role=15,
        rrule="FREQ=DAILY;BYHOUR=3;BYMINUTE=0",   # dtstart = seed-migration time, tzid UTC
        prompt=(
            "Review open issues in the projects you can access, oldest first. "
            "An issue is a candidate when it references a pull request — in its "
            "links, description, or comments. For each candidate, call "
            "get_pull_request_status on the PR URL. If — and only if — the tool "
            "reports state \"merged\", move the issue to a state in its "
            "project's \"completed\" state group (use list_states to find one) "
            "and add a one-line comment naming the merged PR. If merge state is "
            "\"unknown\" or the issue's state is already in the completed group, "
            "leave it untouched. Do not create or delete anything."
        ),
    ),
]
```

The module is Django-free (dataclass + list) so the seed migration imports it directly, like `_rrule.py`. The data migration upserts by `slug` (idempotent re-runs) with `enabled=False` per §13.

### 8.2 New assistant tool — `get_pull_request_status`

Nothing in the DB materializes PR merge state: `Issue.git_work_branch` is a branch name (`db/models/issue.py:171`), `IssueLink` is a free-form URL (`issue.py:444`), and the GitHub integration syncs issues/comments, not PRs (`db/models/integration/github.py`). The agent therefore gets one new read-only tool, in `pi_dash/assistant/tools/github.py`, registered by adding the module to the import list in `tools/__init__.py:11`. Registered for chat and loop alike — "is the PR for this issue merged?" is a useful chat question too.

```python
@assistant.tool
def get_pull_request_status(ctx: RunContext[AssistantDeps], url: str) -> dict:
    """Check whether a GitHub pull request is merged. Accepts a full PR URL."""
```

Implementation contract:

1. **Parse**: `^https://github\.com/(?P<owner>[\w.-]+)/(?P<repo>[\w.-]+)/pull/(?P<num>\d+)`. Non-matching input → `{"state": "unknown", "reason": "unsupported_url"}` (no exception — the prompt treats `unknown` as "don't act").
2. **SSRF**: the URL is regex-pinned to `https://github.com/`, and the _API_ call goes to the constant host `https://api.github.com`, so `ssrf.is_blocked` (`assistant/ssrf.py:25`) is consulted only for consistency with `ASSISTANT_BLOCK_PRIVATE_URLS` policy on the constructed API URL. No redirect following (`follow_redirects=False`).
3. **Credentials**: best-effort token lookup — a `GithubRepositorySync` joined through the user's member projects in this workspace (reuse `_scoping.member_projects(ctx.deps)`) whose repository matches `owner/repo`; if found, use its `credentials["access_token"]` as `Authorization: Bearer`. Otherwise call unauthenticated (public repos; 60 req/h/IP).
4. **Call**: `GET https://api.github.com/repos/{owner}/{repo}/pulls/{num}` via `httpx` (already a runtime dependency), `timeout=10s`. Map: HTTP 200 → `{"state": "merged"|"closed"|"open", "title", "merged_at"}` (merged when `merged_at` non-null, else `closed_at` non-null → closed); 404/403/451 → `unknown` with `reason` (`not_found`, `rate_limited`, `blocked`); network/timeout errors → `unknown`/`network_error`. The tool **never raises** — `unknown` is a normal answer the builtin prompt is written around.
5. **Budget**: a per-run counter on deps (`deps.pr_lookups`, cap `LOOP_PR_LOOKUPS_PER_RUN`, default 15, enforced for both modes); past the cap, return `unknown`/`budget_exhausted`. Prevents one run from burning the unauthenticated rate limit for the whole host IP.
6. **Sync tool**: plain `def` (pydantic-ai runs sync tools in a threadpool, same as the existing tools), so `httpx.Client`, not the async client.

## 9. API

### 9.1 User settings — `pi_dash/loop/views.py`, routed from `pi_dash/loop/urls.py`, included in `pi_dash/urls.py` as `path("api/", include("pi_dash.loop.urls"))` (next to the assistant include, `urls.py:19`)

All three endpoints: plain `BaseAPIView` subclasses, authenticated user only, **no workspace role check** (these are the user's own preferences; route segment is `auto-pm` per the naming rule).

```
GET   /api/users/me/auto-pm/
PATCH /api/users/me/auto-pm/
PATCH /api/users/me/auto-pm/jobs/<slug>/
```

**GET** response (jobs = `LoopJob.objects.filter(enabled=True, deleted_at__isnull=True)`; `interval_label` derived server-side from the RRULE FREQ — `"daily"`, `"weekly"`, `"hourly"` — so the client never parses RRULEs):

```json
{
  "enabled": true,
  "jobs": [
    {
      "slug": "auto-close-merged",
      "name": "Close issues when their PR merges",
      "description": "Checks your projects once a day and marks an issue Done when the pull request that implements it has been merged.",
      "interval_label": "daily",
      "enabled": true
    }
  ]
}
```

`name`/`description` map from `public_name`/`public_description`. The serializer **whitelists exactly these five keys** — `prompt`, `min_role`, internal `name`, and anything saying "loop" must never serialize here (contract-tested, §12).

**PATCH /auto-pm/** body `{"enabled": false}` → upserts the master row: `LoopUserPreference.objects.update_or_create(user=request.user, job=None, deleted_at__isnull=True, defaults={"enabled": False})`. Any key other than `enabled`, or a non-bool → 400 `{"error": "invalid_payload"}`. Response: the GET shape.

**PATCH /auto-pm/jobs/<slug>/** — same contract per job; unknown/disabled slug → 404 `{"error": "not_found"}`.

### 9.2 Instance admin — `pi_dash/loop/admin_views.py`, routed via `path("loop/", include("pi_dash.loop.admin_urls"))` added to `pi_dash/license/urls.py` (mounted under `/api/instances/`, `pi_dash/urls.py:21`)

All endpoints `permission_classes = [InstanceAdminPermission]` (`license/api/permissions/instance.py:12`), following the existing `license/api/views/` conventions.

```
GET  POST          /api/instances/loop/jobs/
GET  PATCH DELETE  /api/instances/loop/jobs/<uuid:pk>/
GET                /api/instances/loop/jobs/<uuid:pk>/targets/
```

Job serializer (full fields — this is the operator surface): `id, slug, name, public_name, public_description, prompt, min_role, enabled, is_builtin, dtstart, rrule, tzid, created_at, updated_at` + read-only annotations `target_count`, `last_24h: {completed, failed, skipped}` (three conditional `Count` aggregates over targets/turns). Writes validate: slug format (`^[a-z0-9-]{1,64}$`), `min_role ∈ {5, 15, 20}`, RRULE via `validate_rrule_string` + the hourly floor (§6.1) → 400 with `{"error": "invalid_rrule", "detail": ...}`. `is_builtin` is read-only; DELETE soft-deletes (targets cascade via the async soft-delete machinery, same as scheduler bindings).

Targets listing (paginated, default 50/page; filterable `?skip_reason=&workspace=&status=`): per row `workspace_slug, user_email, next_run_at, last_skipped_at, last_skip_reason`, and from `last_run` (select_related): `status, error_code, model_used, usage.total_tokens, completed_at`. This is the operator's answer to "is this thing working and what does it cost" — no separate run-history endpoint in MVP.

## 10. Frontend

### 10.A `apps/web` — profile settings: "Auto Project Management"

Follows the AI-assistant settings page pattern exactly (it is the adjacent feature):

| piece            | file                                                                                                                                                                                                                                                                                                                                                                                                       |
| ---------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Types            | `packages/types/src/auto-pm.ts` — `IAutoPMJob { slug; name; description; interval_label; enabled }`, `IAutoPMSettings { enabled; jobs: IAutoPMJob[] }`; star-exported from `packages/types/src/index.ts`                                                                                                                                                                                                   |
| Service          | `packages/services/src/auto-pm/auto-pm.service.ts` — `class AutoPMService extends APIService` (ctor pattern as `assistant.service.ts:15`); methods `getSettings(): Promise<IAutoPMSettings>` → GET `/api/users/me/auto-pm/`; `setMasterEnabled(enabled: boolean)` → PATCH; `setJobEnabled(slug: string, enabled: boolean)` → PATCH `/api/users/me/auto-pm/jobs/${slug}/`; exported from the services index |
| Page component   | `apps/web/core/components/settings/profile/content/pages/auto-project-management.tsx` — `useSWR("auto-pm-settings", () => service.getSettings())`, optimistic `mutate` on toggle, `setToast` on failure (mirror `ai-assistant.tsx`)                                                                                                                                                                        |
| Tab registration | `packages/constants/src/settings/profile.ts` — add key `"auto-project-management"` to `TProfileSettingsTabs`, `PROFILE_SETTINGS` (`i18n_label: "Auto Project Management"`), and `GROUPED_PROFILE_SETTINGS[YOUR_PROFILE]` directly after `"ai-assistant"`; add an icon to the sidebar `ICONS` map (`item-categories.tsx`) — `Repeat` or `RefreshCw` from lucide                                             |
| Route wiring     | the existing `[profileTabId]` page switch (`apps/web/app/(all)/settings/profile/[profileTabId]/page.tsx`) gains the `"auto-project-management"` → `<AutoProjectManagementSettings />` case                                                                                                                                                                                                                 |

Page layout: heading + explainer paragraph ("Pi Dash AI can do routine project upkeep for you automatically. It acts with your permissions and only in workspaces you belong to."), master toggle row ("Pause all"), then one card per job — `name`, `description`, cadence chip from `interval_label`, toggle. Toggles disabled (with hint) while the master switch is off. When `jobs` is empty, render nothing but the heading + "Nothing is scheduled on this instance yet." All strings via `useTranslation()` keys under `auto_pm.*`, added to **every** locale in `packages/i18n/src/locales/<lang>/` (English placeholder where untranslated, per the parity rule).

If the user has no LLM config, show an inline callout linking to the AI Assistant tab: "Auto Project Management uses your AI Assistant connection — set it up first." (`has_api_key` known from the existing `assistant-llm-config` SWR key.)

### 10.B `apps/admin` — "Loop" page

Mirrors the AI instance-settings page structure (`apps/admin/app/(all)/(dashboard)/ai/page.tsx` + `form.tsx`):

- Route: `apps/admin/app/(all)/(dashboard)/loop/page.tsx` (list) and `loop/[jobId]/page.tsx` (detail).
- Sidebar: add to `use-sidebar-menu/core.ts` — `loop: { Icon: Repeat, name: "Loop", description: "Scheduled AI project management jobs.", href: "/loop/" }`.
- Service: `packages/services/src/instance/loop.service.ts` — `InstanceLoopService extends APIService` with `list/create/update/destroy` for `/api/instances/loop/jobs/` and `listTargets(jobId, params)`.
- List page: table (name, slug, RRULE, min_role, enabled toggle PATCHing inline, builtin badge, 24h completed/failed/skipped counts) + "New job" modal (all writable fields; prompt as monospace textarea; RRULE as text input — server 400 surfaces inline, same plain-text-input choice as scheduler design §8.C).
- Detail page: the job form + the targets table (columns from §9.2) with skip-reason filter chips.

## 11. Settings

```python
# pi_dash/settings/common.py — one block next to the ASSISTANT_* keys (common.py:215)
LOOP_ENABLED = os.environ.get("LOOP_ENABLED", "true").lower() == "true"
LOOP_STAGGER_WINDOW_MINUTES = int(os.environ.get("LOOP_STAGGER_WINDOW_MINUTES", 60))
LOOP_MAX_DISPATCH_PER_TICK = int(os.environ.get("LOOP_MAX_DISPATCH_PER_TICK", 100))
LOOP_RECONCILE_EVERY_MINUTES = int(os.environ.get("LOOP_RECONCILE_EVERY_MINUTES", 15))
LOOP_ROTATION_HEADROOM = int(os.environ.get("LOOP_ROTATION_HEADROOM", 30))
LOOP_MAX_WRITES = int(os.environ.get("LOOP_MAX_WRITES", 10))
LOOP_PR_LOOKUPS_PER_RUN = int(os.environ.get("LOOP_PR_LOOKUPS_PER_RUN", 15))
ASSISTANT_LOOP_HISTORY_MAX_TURNS = int(os.environ.get("ASSISTANT_LOOP_HISTORY_MAX_TURNS", 5))
```

Per-run cost is bounded by the assistant's own machinery: `UsageLimits(request_limit=25, tool_calls_limit=20)` (`tasks.py:313`), `ASSISTANT_TURN_SOFT_LIMIT/HARD_LIMIT`, and usage recorded per turn. The eligibility chain means **zero tokens are ever spent** for users who never configured an LLM key, disabled the feature, or lack the job's role.

## 12. Testing

Contract tests in `pi_dash/tests/contract/loop/` (pattern of `tests/contract/assistant/`, reusing its `world` fixture); RRULE/stagger unit tests beside `tests/unit/scheduler/`.

| file                                       | asserts                                                                                                                                                                                                                                                                                                                                                                                    |
| ------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `test_models.py`                           | conditional uniques (re-create after soft delete works; duplicate active edge rejected); master-pref unique                                                                                                                                                                                                                                                                                |
| `test_eligibility.py`                      | each predicate flips eligibility exactly once: pref-off, master pause, role < min_role, no LLM key, inactive membership; `check()` returns the right `SkipReason` in the documented precedence order                                                                                                                                                                                       |
| `test_scanner.py`                          | reconcile creates targets only for missing active edges, idempotent under double-run; new targets get future `next_run_at` (no immediate burst); fan-out queues only eligible-due ids, respects `LOOP_MAX_DISPATCH_PER_TICK` ordering by `next_run_at`; ineligible-due rows get cursor advanced + reason stamped; `LOOP_ENABLED=False` short-circuits                                      |
| `test_fire_dispatch.py`                    | happy path: turn + user message (content == job.prompt) created, `thread.active_turn` set, `run_assistant_turn.delay` queued on commit (mocked, `django_capture_on_commit_callbacks`), `target.last_run` set; `turn_active` skip leaves no new turn; raced `next_run_at > now` no-ops; rotation: thread at `200 - headroom` messages → new thread, old archived, `target.thread` repointed |
| `test_runtime_seam.py`                     | `deps.mode` set from `thread.kind`; loop threads get `LOOP_INSTRUCTIONS` appended, chat threads don't; `created_via="loop"` on issues created under a loop turn; history cap honors `ASSISTANT_LOOP_HISTORY_MAX_TURNS` for loop threads only (extends `test_history.py`)                                                                                                                   |
| `test_thread_visibility.py`                | thread list excludes `kind="loop"`; owner GET by id still works; POST always creates chat                                                                                                                                                                                                                                                                                                  |
| `test_user_api.py`                         | GET shape (exact key whitelist — fails if `prompt`/`min_role` ever serialize); master + per-job PATCH round-trip and precedence; invalid payload 400; unknown slug 404; guest-in-every-workspace user can still toggle (no role gate)                                                                                                                                                      |
| `test_admin_api.py`                        | non-instance-admin 401/403; CRUD round-trip; RRULE validation 400s (`FREQ=MINUTELY` rejected); `is_builtin` immutable; targets listing fields + filters                                                                                                                                                                                                                                    |
| `test_github_tool.py` (unit, mocked httpx) | merged / open / closed mapping; non-GitHub URL → `unsupported_url`; 403 → `rate_limited`; timeout → `network_error`; per-run lookup budget; credentials picked from an accessible project's sync, not from inaccessible ones                                                                                                                                                               |

Local run uses the baked-image workflow (docker cp + `pip install -r requirements/test.txt`); schema changes here require one `--create-db` run.

## 13. Rollout

- All changes are additive (3 new tables, 1 defaulted column on `assistant_thread`, new settings with safe defaults). Deploy order is free; Beat picks up the new entry on restart.
- Seed migration creates the builtin job **`enabled=False`**. Combined with absence-means-enabled preferences, flipping it on in `apps/admin` is the launch act — do it on the dogfooding instance first, watch the targets table for a day (skip-reason distribution, token usage, failure rate), then enable on production.
- Kill switches, outermost-in: `LOOP_ENABLED` env → per-job `enabled` (admin) → per-user master pause → per-user per-job toggle.

## 14. Open questions (resolve before merge)

1. **PR-state auth fallback.** When no `GithubRepositorySync` credentials exist, is unauthenticated GitHub API acceptable (60 req/h/IP), or should the tool return `unknown` unless credentials exist? Recommend: try unauthenticated, treat 403 as `unknown`/`rate_limited`; the prompt's "only act when clearly established" makes `unknown` safe, and `LOOP_PR_LOOKUPS_PER_RUN` bounds the burn.
2. **Guest jobs.** `min_role` default is 15; do we ever want guest-eligible jobs? Schema supports 5, MVP ships none. Recommend: defer.
3. **`update_issue` orchestration side-effect.** `update_issue` may dispatch an orchestration run on state transition (`tools/issues.py`, `update_issue` → `handle_issue_state_transition`). Moving an issue to a completed-group state from a loop run should be inert there, but verify during PR 2 that the transition hook doesn't fire coding runs for completed-group moves; if it can, gate it on `deps.mode`.
4. **Cloud plan gating.** Lives behind `eligibility.llm_available_q()` + `resolve_model_for_user` (both `ee`-overridable); whether the plan check is a queryset join or a model-provider refusal is a private-pi-dash decision. This design guarantees the seam only.

## 15. Future work (explicitly not MVP)

- Surface loop threads in the assistant UI ("see what Auto PM did"), per-edge activity digest.
- Per-workspace user preference overrides; workspace-admin opt-out for a whole workspace.
- Event-driven jobs (on PR merge webhook, on issue stale) — likely a different trigger feeding the same `LoopTarget` dispatch path.
- User-authored jobs (where loop and scheduler authoring may converge into one catalog UX).
- More builtins: stale-issue triage, unassigned-work nudge, duplicate detection.
- A `merged_at`-materializing PR sync (would let the builtin drop its live API calls entirely).
- `rdates`/`exdates` on `LoopJob` if operators ever need calendar exceptions.
