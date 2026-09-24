"""Scheduler-domain seeding: schedulers, bindings, pods, agent runs.

``scheduler_bindings`` carry the iCal-shaped recurrence (``dtstart``,
``tzid``, ``rrule``, ``rdates``/``exdates`` JSON lists) plus runner-facing
wire fields (``pod``, ``next_run_at``, ``last_run``) that installed runners
consume — the suite pins those fields on every shape assertion.
"""

from __future__ import annotations

import datetime
import uuid

from _harness.seed import now_iso, new_id


def create_scheduler(
    conn,
    *,
    workspace_id: str,
    slug: str,
    name: str,
    prompt: str = "Scan the repo.",
    color: str = "#3b82f6",
    is_enabled: bool = True,
    source: str = "builtin",
    created_by_id: str | None = None,
) -> dict:
    sid = new_id()
    now = now_iso()
    conn.execute(
        """INSERT INTO schedulers (id, slug, name, description, prompt, source,
            is_enabled, workspace_id, color, created_at, updated_at,
            created_by_id)
        VALUES (%s,%s,%s,'',%s,%s,%s,%s,%s,%s,%s,%s)""",
        (
            sid, slug, name, prompt, source, is_enabled,
            workspace_id, color, now, now, created_by_id,
        ),
    )
    return {"id": sid, "slug": slug}


def create_binding(
    conn,
    *,
    workspace_id: str,
    project_id: str,
    scheduler_id: str,
    dtstart: str,
    rrule: str = "FREQ=HOURLY",
    tzid: str = "UTC",
    enabled: bool = True,
    outcome_mode: str = "create_issue",
    actor_id: str | None = None,
    pod_id: str | None = None,
) -> dict:
    bid = new_id()
    now = now_iso()
    conn.execute(
        """INSERT INTO scheduler_bindings (id, extra_context, enabled,
            last_error, workspace_id, project_id, scheduler_id, dtstart,
            tzid, rrule, rdates, exdates, outcome_mode, created_at,
            updated_at, actor_id, pod_id)
        VALUES (%s,'',%s,'',%s,%s,%s,%s,%s,%s,'[]','[]',%s,%s,%s,%s,%s)""",
        (
            bid, enabled, workspace_id, project_id, scheduler_id, dtstart,
            tzid, rrule, outcome_mode, now, now, actor_id, pod_id,
        ),
    )
    return {"id": bid}


def create_pod(
    conn, *, workspace_id: str, project_id: str, name: str = "pod-1"
) -> dict:
    pid = new_id()
    now = now_iso()
    conn.execute(
        """INSERT INTO pod (id, name, description, is_default, created_at,
            updated_at, workspace_id, project_id)
        VALUES (%s,%s,'Seeded for contract tests',true,%s,%s,%s,%s)""",
        (pid, name, now, now, workspace_id, project_id),
    )
    return {"id": pid}


def create_agent_run(
    conn,
    *,
    workspace_id: str,
    project_id: str,
    pod_id: str,
    binding_id: str,
    created_by_id: str,
    started_at: str,
    status: str = "completed",
) -> dict:
    rid = new_id()
    now = now_iso()
    conn.execute(
        """INSERT INTO agent_run (id, status, prompt, run_config,
            required_capabilities, thread_id, error, created_at,
            workspace_id, pod_id, created_by_id, llm_model,
            refusal_category, trigger, executor_kind, dispatch_attempts,
            cancel_reason, error_code, tool_plan, phase_kind,
            agent_metadata, usage, started_at, scheduler_binding_id)
        VALUES (%s,%s,'Seeded run','{}','{}','seed-thread','',%s,%s,%s,%s,
            'seed-model','','scheduler','local_runner',0,'','','{}','',
            '{}','{"input":1,"output":1,"total":2}',%s,%s)""",
        (
            rid, status, now, workspace_id, pod_id, created_by_id,
            started_at, binding_id,
        ),
    )
    return {"id": rid}


def hours_ago(n: float) -> str:
    return (
        datetime.datetime.now(datetime.timezone.utc)
        - datetime.timedelta(hours=n)
    ).isoformat()


def days_ago(n: float) -> str:
    return (
        datetime.datetime.now(datetime.timezone.utc)
        - datetime.timedelta(days=n)
    ).isoformat()


def days_from_now(n: float) -> str:
    return (
        datetime.datetime.now(datetime.timezone.utc)
        + datetime.timedelta(days=n)
    ).isoformat()


def uid(prefix: str) -> str:
    return "%s-%s" % (prefix, uuid.uuid4().hex[:12])
