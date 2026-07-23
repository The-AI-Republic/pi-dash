# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Shared logic for moving a work item to another project in the same workspace.

Both the API-key surface (``pi_dash.api.views.issue.IssueMoveAPIEndpoint``,
used by the ``pidash issue move`` CLI) and the session-authed web-app surface
(``pi_dash.app.views.issue.move.IssueMoveEndpoint``) delegate here so the two
entry points stay byte-for-byte identical.
"""

import json
import logging

from django.core.serializers.json import DjangoJSONEncoder
from django.db import connection, transaction
from django.db.models import Max, Q
from django.utils import timezone

from pi_dash.app.permissions import ROLE
from pi_dash.bgtasks.issue_activities_task import issue_activity
from pi_dash.bgtasks.webhook_task import model_activity
from pi_dash.db.models import (
    CommentReaction,
    CycleIssue,
    Description,
    FileAsset,
    Issue,
    IssueActivity,
    IssueAssignee,
    IssueComment,
    IssueDescriptionVersion,
    IssueLabel,
    IssueLink,
    IssueMention,
    IssueReaction,
    IssueRelation,
    IssueSequence,
    IssueSubscriber,
    IssueVersion,
    IssueVote,
    ModuleIssue,
    Project,
    ProjectMember,
    State,
)
from pi_dash.runner.models import AgentRun, AgentRunStatus, Pod
from pi_dash.utils.uuid import convert_uuid_to_integer

logger = logging.getLogger(__name__)

_PROJECT_MOVE_HANDOFF_STATUSES = (
    AgentRunStatus.QUEUED,
    AgentRunStatus.ASSIGNED,
    AgentRunStatus.WAITING_FOR_WORKTREE,
    AgentRunStatus.RUNNING,
    AgentRunStatus.CANCEL_REQUESTED,
    AgentRunStatus.AWAITING_APPROVAL,
    AgentRunStatus.AWAITING_REAUTH,
    AgentRunStatus.PAUSED_AWAITING_INPUT,
)

_IMMEDIATE_HANDOFF_STATUSES = (
    AgentRunStatus.QUEUED,
    AgentRunStatus.PAUSED_AWAITING_INPUT,
)


def _send_project_move_cancel(runner_id, run_id) -> None:
    """Best-effort cancellation request for a project-move handoff.

    Cancel frames are deliberately not buffered for offline runners. The
    persisted CANCEL_REQUESTED status makes session-open return a cancel frame
    when that runner reconnects, so a transient delivery failure cannot lose
    the handoff intent.
    """
    from pi_dash.runner.services.outbox import RunnerOfflineError
    from pi_dash.runner.services.pubsub import send_to_runner

    try:
        send_to_runner(
            runner_id,
            {
                "v": 1,
                "type": "cancel",
                "run_id": str(run_id),
                "reason": "issue_moved_projects",
            },
        )
    except RunnerOfflineError:
        logger.info(
            "issue_move: runner %s offline; cancellation for run %s will be "
            "redelivered at session open",
            runner_id,
            run_id,
        )
    except Exception:
        # Delivery is deliberately best-effort. The persisted
        # CANCEL_REQUESTED status is authoritative and reconnect handling
        # retries the cancel, so a transient outbox failure must not turn a
        # successfully committed issue move into an HTTP 500.
        logger.exception(
            "issue_move: failed to deliver cancellation for run %s",
            run_id,
        )


class IssueMoveError(Exception):
    """Raised for the recoverable failure modes of a move.

    ``status_code`` is the HTTP status the calling view should return, and
    ``message`` is the human-readable ``{"error": ...}`` payload.
    """

    def __init__(self, message, status_code):
        super().__init__(message)
        self.message = message
        self.status_code = status_code


def move_work_item_to_project(*, slug, project_id, pk, target_ref, actor, origin):
    """Move work item ``pk`` (in ``project_id``) into the project ``target_ref``.

    ``target_ref`` may be a project UUID or workspace-scoped identifier.
    ``actor`` is the acting user; ``origin`` is the request host for webhooks.

    Returns the refreshed :class:`Issue`. Raises :class:`IssueMoveError`
    (400/403) for recoverable failures; ``Issue.DoesNotExist`` propagates so
    callers surface a 404.
    """
    # Import lazily to avoid a module-load cycle: the api serializer module
    # pulls in view helpers that would otherwise re-enter during app startup.
    from pi_dash.api.serializers import IssueSerializer

    target_ref = str(target_ref or "").strip()
    if not target_ref:
        raise IssueMoveError("project is required", 400)

    issue = Issue.issue_objects.get(workspace__slug=slug, project_id=project_id, pk=pk)
    target_project = Project.resolve(slug, target_ref)
    if str(target_project.id) == str(issue.project_id):
        # Already in the target project — nothing to do.
        return issue

    can_access_target = ProjectMember.objects.filter(
        workspace__slug=slug,
        project=target_project,
        member=actor,
        role__gte=ROLE.MEMBER.value,
        is_active=True,
    ).exists()
    if not can_access_target:
        raise IssueMoveError(
            "You do not have permission to move work items into the target project",
            403,
        )

    current_instance = json.dumps(IssueSerializer(issue).data, cls=DjangoJSONEncoder)
    requested_data = json.dumps({"project": str(target_project.id)}, cls=DjangoJSONEncoder)

    target_state = (
        State.objects.filter(
            ~Q(is_triage=True),
            project=target_project,
            default=True,
        ).first()
        or State.objects.filter(~Q(is_triage=True), project=target_project).first()
    )
    if target_state is None:
        raise IssueMoveError("Target project does not have a workflow state", 400)

    with transaction.atomic():
        # ``of=("self",)`` scopes the row lock to the base ``issues`` row.
        # ``Issue.issue_objects`` excludes triage states via
        # ``.exclude(state__group=...)`` and ``state`` is a nullable FK, so
        # the queryset carries a LEFT OUTER JOIN to ``states``. A bare
        # ``SELECT ... FOR UPDATE`` then asks Postgres to lock the nullable
        # side of that outer join, which it refuses ("FOR UPDATE cannot be
        # applied to the nullable side of an outer join") — a 500 on every
        # move. Same fix as ``IssueWorkpadAPIEndpoint.patch``.
        issue = Issue.issue_objects.select_for_update(of=("self",)).get(
            workspace__slug=slug, project_id=project_id, pk=pk
        )
        lock_key = convert_uuid_to_integer(target_project.id)
        with connection.cursor() as cursor:
            cursor.execute("SELECT pg_advisory_xact_lock(%s)", [lock_key])

        # Lock the issue's outstanding work before changing its project. The
        # source matcher uses SELECT FOR UPDATE SKIP LOCKED, so once these rows
        # are held it cannot concurrently assign a QUEUED run in the old pod.
        handoff_runs = list(
            AgentRun.objects.select_for_update()
            .filter(
                work_item=issue,
                status__in=_PROJECT_MOVE_HANDOFF_STATUSES,
            )
            .order_by("-created_at")
        )

        last_sequence = IssueSequence.objects.filter(project=target_project).aggregate(largest=Max("sequence"))[
            "largest"
        ]
        next_sequence = last_sequence + 1 if last_sequence else 1

        target_pod = Pod.default_for_project_id(target_project.id)
        if handoff_runs and target_pod is None:
            raise IssueMoveError("Target project does not have a default runner pod", 409)
        source_project_id = issue.project_id
        issue.project = target_project
        issue.workspace = target_project.workspace
        issue.sequence_id = next_sequence
        issue.state = target_state
        issue.assigned_pod = target_pod
        issue.parent = None
        issue.estimate_point = None
        issue.type = None
        if handoff_runs:
            # The state-change signal must still arm the target ticker, but it
            # must not dispatch its own run. The explicit handoff below owns
            # replacement creation after the source cancellation barrier.
            issue._orchestration_dispatch_immediate = False
        issue.save(
            update_fields=[
                "project",
                "workspace",
                "sequence_id",
                "state",
                "assigned_pod",
                "parent",
                "estimate_point",
                "type",
                "updated_at",
            ]
        )

        # A project move is a run handoff, not an in-place pod mutation:
        #
        # - QUEUED / PAUSED work has no executing agent, so close the source
        #   row and create a fresh target-project row in this transaction.
        # - ASSIGNED / WAITING / RUNNING / AWAITING work first enters
        #   CANCEL_REQUESTED. The old row remains busy until the source runner
        #   confirms its process has stopped; only then does the lifecycle hook
        #   create the target-project replacement.
        #
        # The fresh row gets a newly rendered prompt/repository snapshot and no
        # source-runner pin. Keeping the old row as parent preserves lineage
        # without leaking source-project execution context.
        immediate_handoff_parent = None
        cancel_after_commit = None
        source_pods_to_drain: set = set()
        now = timezone.now()
        if handoff_runs:
            executing_runs = [
                run
                for run in handoff_runs
                if run.status not in _IMMEDIATE_HANDOFF_STATUSES
                and run.runner_id is not None
            ]
            if len(executing_runs) > 1:
                # This should be prevented by orchestration's one-active-run
                # rule. Refuse the move instead of declaring live duplicate
                # processes cancelled in the database without stopping them.
                raise IssueMoveError(
                    "Issue has multiple active agent runs; cancel them before moving it",
                    409,
                )

            handoff_parent = (
                executing_runs[0] if executing_runs else handoff_runs[0]
            )
            inert_runs = [
                run for run in handoff_runs if run.id != handoff_parent.id
            ]
            if not executing_runs:
                inert_runs.append(handoff_parent)

            for inert_run in inert_runs:
                inert_run.status = AgentRunStatus.CANCELLED
                inert_run.ended_at = now
                inert_run.queue_position = None
                inert_run.save(
                    update_fields=["status", "ended_at", "queue_position"]
                )
                if inert_run.pod_id:
                    source_pods_to_drain.add(inert_run.pod_id)

            if executing_runs:
                from pi_dash.orchestration.service import (
                    PROJECT_MOVE_HANDOFF_CONFIG_KEY,
                )

                run_config = dict(handoff_parent.run_config or {})
                run_config[PROJECT_MOVE_HANDOFF_CONFIG_KEY] = {
                    "source_project_id": str(source_project_id),
                    "target_project_id": str(target_project.id),
                    "target_pod_id": str(target_pod.id),
                }
                handoff_parent.status = AgentRunStatus.CANCEL_REQUESTED
                handoff_parent.run_config = run_config
                handoff_parent.queue_position = None
                handoff_parent.save(update_fields=["status", "run_config", "queue_position"])
                cancel_after_commit = (
                    handoff_parent.runner_id,
                    handoff_parent.id,
                )
            else:
                immediate_handoff_parent = handoff_parent

        IssueSequence.objects.filter(issue=issue).exclude(project=target_project).update(issue=None)
        IssueSequence.objects.create(issue=issue, sequence=next_sequence, project=target_project)

        valid_assignees = ProjectMember.objects.filter(
            project=target_project,
            member_id__in=IssueAssignee.objects.filter(issue=issue).values_list("assignee_id", flat=True),
            role__gte=ROLE.MEMBER.value,
            is_active=True,
        ).values_list("member_id", flat=True)
        IssueAssignee.objects.filter(issue=issue).exclude(assignee_id__in=valid_assignees).delete()
        IssueAssignee.objects.filter(issue=issue).update(
            project=target_project,
            workspace=target_project.workspace,
        )

        IssueLabel.objects.filter(issue=issue).delete()
        CycleIssue.objects.filter(issue=issue).delete()
        ModuleIssue.objects.filter(issue=issue).delete()
        IssueRelation.objects.filter(Q(issue=issue) | Q(related_issue=issue)).delete()
        Issue.objects.filter(parent=issue).exclude(project=target_project).update(parent=None)

        comment_ids = list(IssueComment.objects.filter(issue=issue).values_list("id", flat=True))
        description_ids = list(
            IssueComment.objects.filter(issue=issue, description__isnull=False).values_list("description_id", flat=True)
        )

        update_kwargs = {
            "project": target_project,
            "workspace": target_project.workspace,
        }
        IssueLink.objects.filter(issue=issue).update(**update_kwargs)
        IssueMention.objects.filter(issue=issue).update(**update_kwargs)
        IssueSubscriber.objects.filter(issue=issue).update(**update_kwargs)
        IssueReaction.objects.filter(issue=issue).update(**update_kwargs)
        IssueVote.objects.filter(issue=issue).update(**update_kwargs)
        IssueVersion.objects.filter(issue=issue).update(**update_kwargs)
        IssueDescriptionVersion.objects.filter(issue=issue).update(**update_kwargs)
        IssueActivity.objects.filter(Q(issue=issue) | Q(issue_comment_id__in=comment_ids)).update(**update_kwargs)
        IssueComment.objects.filter(issue=issue).update(**update_kwargs)
        CommentReaction.objects.filter(comment_id__in=comment_ids).update(**update_kwargs)
        Description.objects.filter(id__in=description_ids).update(**update_kwargs)
        FileAsset.objects.filter(Q(issue=issue) | Q(comment_id__in=comment_ids)).update(**update_kwargs)

        if immediate_handoff_parent is not None and target_pod is not None:
            from pi_dash.orchestration.service import (
                _create_project_move_handoff_run,
            )

            _create_project_move_handoff_run(
                issue=issue,
                parent=immediate_handoff_parent,
                pod=target_pod,
            )

        if cancel_after_commit is not None:
            runner_id, run_id = cancel_after_commit
            transaction.on_commit(lambda rid=runner_id, run=run_id: _send_project_move_cancel(rid, run))

        if source_pods_to_drain:
            from pi_dash.runner.services.matcher import drain_pod_by_id

            for source_pod_id in source_pods_to_drain:
                transaction.on_commit(lambda pid=source_pod_id: drain_pod_by_id(pid))

    issue_activity.delay(
        type="issue.activity.updated",
        requested_data=requested_data,
        actor_id=str(actor.id),
        issue_id=str(pk),
        project_id=str(target_project.id),
        current_instance=current_instance,
        epoch=int(timezone.now().timestamp()),
    )
    model_activity.delay(
        model_name="issue",
        model_id=str(pk),
        requested_data={"project": str(target_project.id)},
        current_instance=current_instance,
        actor_id=actor.id,
        slug=slug,
        origin=origin,
    )

    return Issue.issue_objects.select_related("project", "workspace", "state").get(pk=pk)
