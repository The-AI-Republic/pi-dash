# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Server data migration for the single Runner executor (PIDESKAPP-123).

``.ai_design/built_engine_as_runner/design.md`` §13 (revision 4.0), in the
order stated there: executors, then pruning never-used engine runners.
(Token expiries, the third step, belong to the MachineToken expiry work and
are not part of this migration.) Nothing here reads or writes
``Issue.assigned_runner``: dedicating an issue to one runner belongs to the
dedicated-runner design (HA §23), built later.

1. **Executors.**
   - Every engine runner (``provisioning = desktop_bundled``) becomes
     ``pinned_only``: it keeps serving the legacy ``managed_runner`` runs
     already pinned to it and takes no new issue work until its owner turns
     pod work on.
   - Projects whose default is ``managed_runner`` become ``local_runner``,
     and the live engine runners in those projects become ``pod_member`` so
     each person's own work still reaches their own computer (PO-11).
   - Projects whose default is ``cloud_agent`` become ``local_runner``.
   - Issues set to ``managed_runner`` or ``cloud_agent`` become
     ``local_runner``, pod-level: ``assigned_pod`` is kept, or set to the
     project's default pod when it is empty, and no runner is named.
   - Runs are not touched. ``AgentRun.executor_kind`` keeps
     ``managed_runner`` / ``cloud_agent``; queued legacy managed runs stay
     pinned to their engine and finish (or expire) as before, and queued or
     running cloud runs finish on the Cloud Agent.
2. **Prune.** Live engine runners with no run at all (as runner or as pin)
   are revoked. These are the rows older desktop builds created just by
   opening a project (PIDESKAPP-118). The revoke is cloud-side only
   (``user_revoke``): the local install is left alone.

The project, issue and pruning steps are idempotent (a second pass finds
nothing to change). The ``pod_dispatch`` step assumes the column defaults
``0034`` just wrote, as a migration runs once. Not reversible in data (the
reverse is a no-op): the old executor values cannot be told apart once
rewritten; the schema migration before it still reverses.
"""

from django.db import migrations
from django.db.models import Exists, OuterRef
from django.utils import timezone

ENGINE = "desktop_bundled"
LOCAL = "local_runner"
MANAGED = "managed_runner"
CLOUD = "cloud_agent"
PINNED_ONLY = "pinned_only"
POD_MEMBER = "pod_member"
PRUNE_REASON = "user_revoke"


def migrate_executors(apps):
    Runner = apps.get_model("runner", "Runner")
    Project = apps.get_model("db", "Project")

    managed_project_ids = list(
        Project._base_manager.filter(default_agent_executor=MANAGED).values_list("id", flat=True)
    )
    # Engine runners take no new issue work until the owner turns pod work on;
    # they keep serving the legacy runs already pinned to them. ``0034`` gave
    # every row the ``pod_member`` column default, so this runs against
    # untouched rows.
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


def migrate_issue_executors(apps):
    Issue = apps.get_model("db", "Issue")
    Pod = apps.get_model("runner", "Pod")

    legacy = Issue._base_manager.filter(agent_executor__in=[MANAGED, CLOUD])
    # Pod-level: keep the issue's pod, or give it its project's default pod.
    missing_pod_projects = set(legacy.filter(assigned_pod__isnull=True).values_list("project_id", flat=True))
    for project_id in missing_pod_projects:
        default_pod = (
            Pod._base_manager.filter(project_id=project_id, is_default=True, deleted_at__isnull=True)
            .values_list("pk", flat=True)
            .first()
        )
        if default_pod is not None:
            legacy.filter(project_id=project_id, assigned_pod__isnull=True).update(assigned_pod_id=default_pod)
    legacy.update(agent_executor=LOCAL)


def prune_never_used_engine_runners(apps):
    Runner = apps.get_model("runner", "Runner")
    RunnerSession = apps.get_model("runner", "RunnerSession")
    AgentRun = apps.get_model("runner", "AgentRun")

    used_as_runner = AgentRun._base_manager.filter(runner_id=OuterRef("pk"))
    used_as_pin = AgentRun._base_manager.filter(pinned_runner_id=OuterRef("pk"))
    never_used = list(
        Runner._base_manager.filter(provisioning=ENGINE, revoked_at__isnull=True)
        .annotate(_run=Exists(used_as_runner), _pin=Exists(used_as_pin))
        .filter(_run=False, _pin=False)
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
    migrate_issue_executors(apps)
    prune_never_used_engine_runners(apps)


class Migration(migrations.Migration):
    dependencies = [
        ("runner", "0034_engine_dispatch_fields"),
        ("db", "0170_issueview_anchor"),
    ]

    operations = [
        migrations.RunPython(forwards, migrations.RunPython.noop),
    ]
