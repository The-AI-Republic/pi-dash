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
from pi_dash.runner.models import Pod
from pi_dash.utils.uuid import convert_uuid_to_integer


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

        last_sequence = IssueSequence.objects.filter(project=target_project).aggregate(largest=Max("sequence"))[
            "largest"
        ]
        next_sequence = last_sequence + 1 if last_sequence else 1

        issue.project = target_project
        issue.workspace = target_project.workspace
        issue.sequence_id = next_sequence
        issue.state = target_state
        issue.assigned_pod = Pod.default_for_project_id(target_project.id)
        issue.parent = None
        issue.estimate_point = None
        issue.type = None
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
