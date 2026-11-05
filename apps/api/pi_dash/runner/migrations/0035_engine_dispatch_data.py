# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Server data migration for the single Runner executor (PIDESKAPP-123).

``.ai_design/built_engine_as_runner/design.md`` §13, in the order stated
there: executors, then the ``assigned_runner`` backfill, then pruning
never-used engine runners. (Token expiries, the fourth step, belong to the
MachineToken expiry work and are not part of this migration.)

1. **Executors.**
   - Every engine runner (``provisioning = desktop_bundled``) becomes
     ``pinned_only``, so it takes exactly what it took before.
   - Projects whose default is ``managed_runner`` become ``local_runner``,
     and the engine runners in those projects become ``pod_member`` so each
     person's own work still reaches their own computer (PO-11).
   - Projects whose default is ``cloud_agent`` become ``local_runner``.
   - Issues set to ``cloud_agent`` become ``local_runner``, unpinned.
   - Queued runs pinned to an engine get ``pin_strict = true``. History is not
     rewritten: ``AgentRun.executor_kind`` keeps ``managed_runner`` /
     ``cloud_agent`` and queued or running cloud runs finish where they are.
2. **``assigned_runner`` backfill.** Issues set to ``managed_runner`` become
   ``local_runner`` with ``assigned_runner`` = the pinned runner of the
   issue's latest engine run, else the issue creator's most recently seen
   engine runner in the project, else none (pod-level). ``assigned_pod``
   follows the runner's pod (the dispatch-pod invariant, §7).
3. **Prune.** Live engine runners with no run (as runner or as pin) and no
   issue assigned to them are revoked. These are the rows older desktop
   builds created just by opening a project (PIDESKAPP-118). The revoke is
   cloud-side only (``user_revoke``): the local install is left alone.

The issue, run and pruning steps are idempotent (a second pass finds nothing
to change). The ``pod_dispatch`` step assumes the column defaults ``0034``
just wrote, as a migration runs once. Not reversible in data (the reverse is a
no-op): the old executor values cannot be told apart once rewritten; the
schema migrations around it still reverse.
"""

from django.db import migrations
from django.db.models import Exists, OuterRef, Q
from django.utils import timezone

ENGINE = "desktop_bundled"
LOCAL = "local_runner"
MANAGED = "managed_runner"
CLOUD = "cloud_agent"
PINNED_ONLY = "pinned_only"
POD_MEMBER = "pod_member"
QUEUED = "queued"
PRUNE_REASON = "user_revoke"


def migrate_executors(apps):
    Runner = apps.get_model("runner", "Runner")
    AgentRun = apps.get_model("runner", "AgentRun")
    Project = apps.get_model("db", "Project")
    Issue = apps.get_model("db", "Issue")

    managed_project_ids = list(
        Project._base_manager.filter(default_agent_executor=MANAGED).values_list("id", flat=True)
    )
    # Engine runners take exactly what they took before: strict pins only.
    # ``0034`` gave every row the ``pod_member`` column default, so this runs
    # against untouched rows.
    Runner._base_manager.filter(provisioning=ENGINE).update(pod_dispatch=PINNED_ONLY)
    if managed_project_ids:
        # PO-11: engines in projects whose default was the built-in engine
        # become pod members, so each person's own work still reaches them.
        Runner._base_manager.filter(
            provisioning=ENGINE,
            revoked_at__isnull=True,
            pod__project_id__in=managed_project_ids,
        ).update(pod_dispatch=POD_MEMBER)
        Project._base_manager.filter(id__in=managed_project_ids).update(default_agent_executor=LOCAL)

    Project._base_manager.filter(default_agent_executor=CLOUD).update(default_agent_executor=LOCAL)
    Issue._base_manager.filter(agent_executor=CLOUD).update(agent_executor=LOCAL)

    # Queued runs pinned to an engine wait for that engine, as before: their
    # pin is the execution target, so it is now strict.
    AgentRun._base_manager.filter(
        status=QUEUED,
        pinned_runner__provisioning=ENGINE,
        pin_strict=False,
    ).update(pin_strict=True)


def backfill_assigned_runner(apps):
    Runner = apps.get_model("runner", "Runner")
    AgentRun = apps.get_model("runner", "AgentRun")
    Issue = apps.get_model("db", "Issue")

    issues = Issue._base_manager.filter(agent_executor=MANAGED).only(
        "id", "project_id", "created_by_id", "assigned_runner_id", "assigned_pod_id"
    )
    for issue in issues.iterator():
        runner = None
        latest = (
            AgentRun._base_manager.filter(work_item_id=issue.id)
            .filter(
                Q(pinned_runner__provisioning=ENGINE, pinned_runner__revoked_at__isnull=True)
                | Q(runner__provisioning=ENGINE, runner__revoked_at__isnull=True)
            )
            .order_by("-created_at")
            .first()
        )
        if latest is not None:
            runner_id = latest.pinned_runner_id
            if runner_id is None or not Runner._base_manager.filter(
                pk=runner_id, provisioning=ENGINE, revoked_at__isnull=True
            ).exists():
                runner_id = latest.runner_id
            runner = Runner._base_manager.filter(pk=runner_id, provisioning=ENGINE, revoked_at__isnull=True).first()
        if runner is None and issue.created_by_id is not None:
            runner = (
                Runner._base_manager.filter(
                    owner_id=issue.created_by_id,
                    provisioning=ENGINE,
                    revoked_at__isnull=True,
                    pod__project_id=issue.project_id,
                )
                .order_by("-last_heartbeat_at", "-created_at")
                .first()
            )
        # Never pin across projects: the runner's pod must belong to the
        # issue's project, or the invariant would route it elsewhere.
        if runner is not None and runner.pod_id is not None:
            pod_project_id = runner.pod.project_id
            if pod_project_id != issue.project_id:
                runner = None
        updates = {"agent_executor": LOCAL}
        if runner is not None:
            updates["assigned_runner_id"] = runner.id
            updates["assigned_pod_id"] = runner.pod_id
        Issue._base_manager.filter(pk=issue.id).update(**updates)


def prune_never_used_engine_runners(apps):
    Runner = apps.get_model("runner", "Runner")
    RunnerSession = apps.get_model("runner", "RunnerSession")
    AgentRun = apps.get_model("runner", "AgentRun")
    Issue = apps.get_model("db", "Issue")

    used_as_runner = AgentRun._base_manager.filter(runner_id=OuterRef("pk"))
    used_as_pin = AgentRun._base_manager.filter(pinned_runner_id=OuterRef("pk"))
    assigned = Issue._base_manager.filter(assigned_runner_id=OuterRef("pk"))
    never_used = list(
        Runner._base_manager.filter(provisioning=ENGINE, revoked_at__isnull=True)
        .annotate(_run=Exists(used_as_runner), _pin=Exists(used_as_pin), _assigned=Exists(assigned))
        .filter(_run=False, _pin=False, _assigned=False)
        .values_list("pk", flat=True)
    )
    if not never_used:
        return
    now = timezone.now()
    Runner._base_manager.filter(pk__in=never_used).update(status="revoked", revoked_at=now, revoked_reason=PRUNE_REASON)
    RunnerSession._base_manager.filter(runner_id__in=never_used, revoked_at__isnull=True).update(
        revoked_at=now, revoked_reason=PRUNE_REASON
    )


def forwards(apps, schema_editor):
    migrate_executors(apps)
    backfill_assigned_runner(apps)
    prune_never_used_engine_runners(apps)


class Migration(migrations.Migration):
    dependencies = [
        ("runner", "0034_engine_dispatch_fields"),
        ("db", "0171_issue_assigned_runner"),
    ]

    operations = [
        migrations.RunPython(forwards, migrations.RunPython.noop),
    ]
