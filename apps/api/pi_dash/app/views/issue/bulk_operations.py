# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Bulk issue endpoints: one request across a selection (NEWFRONT-984).

Bulk update (ISS-112) applies a properties object to every selected
issue; array-valued properties union onto the stored rows while scalars
replace. Bulk subscribe (ISS-115) subscribes the caller to every
selected issue, idempotently. Both ride the OSS stack API ungated, per
the NEWFRONT-672 precedent — the cloud gate lives in the web_new
edition module, not here. Writes validate the whole batch first, then
apply inside one transaction, mirroring the single-issue PATCH and the
cycle/module attach endpoints.
"""

# Python imports
import json

# Django imports
from django.core import serializers as django_serializers
from django.db import transaction
from django.utils import timezone

# Third Party imports
from rest_framework import status
from rest_framework.response import Response

# Module imports
from .. import BaseAPIView
from pi_dash.app.permissions import ProjectEntityPermission, ProjectLitePermission
from pi_dash.app.permissions import allow_permission, ROLE
from pi_dash.app.serializers import BulkOperationSerializer, BulkSubscribeSerializer
from pi_dash.bgtasks.issue_activities_task import issue_activity
from pi_dash.db.models import (
    Cycle,
    CycleIssue,
    EstimatePoint,
    Issue,
    IssueAssignee,
    IssueLabel,
    IssueSubscriber,
    Label,
    Module,
    ModuleIssue,
    Project,
    State,
    User,
)
from pi_dash.utils.host import base_host


def _missing(model, ids):
    found = set(model.objects.filter(pk__in=list(ids)).values_list("pk", flat=True))
    return [str(item) for item in ids if item not in found]


class BulkOperationIssuesEndpoint(BaseAPIView):
    permission_classes = [ProjectEntityPermission]

    @allow_permission([ROLE.ADMIN, ROLE.MEMBER])
    def post(self, request, slug, project_id):
        serializer = BulkOperationSerializer(data=request.data)
        if not serializer.is_valid():
            return Response(serializer.errors, status=status.HTTP_400_BAD_REQUEST)
        data = serializer.validated_data
        properties = data["properties"]

        project = Project.objects.filter(pk=project_id, workspace__slug=slug).first()
        if project is None:
            return Response({"error": "Project not found"}, status=status.HTTP_404_NOT_FOUND)

        issues = list(
            Issue.objects.filter(
                workspace__slug=slug, project_id=project_id, pk__in=data["issue_ids"]
            )
        )

        # Validate the whole batch before writing anything: an unknown
        # reference or an inconsistent merged date fails the batch with
        # nothing applied.
        if "state_id" in properties and properties["state_id"] is not None:
            if not State.all_state_objects.filter(pk=properties["state_id"]).exists():
                return Response(
                    {"error": f"State {properties['state_id']} does not exist"},
                    status=status.HTTP_400_BAD_REQUEST,
                )
        for key, model, label in (
            ("label_ids", Label, "Labels"),
            ("assignee_ids", User, "Users"),
            ("module_ids", Module, "Modules"),
        ):
            missing = _missing(model, properties.get(key, []) or [])
            if missing:
                return Response(
                    {"error": f"{label} do not exist: {', '.join(sorted(missing))}"},
                    status=status.HTTP_400_BAD_REQUEST,
                )
        if "estimate_point" in properties and properties["estimate_point"] is not None:
            if not EstimatePoint.objects.filter(pk=properties["estimate_point"]).exists():
                return Response(
                    {"error": f"Estimate point {properties['estimate_point']} does not exist"},
                    status=status.HTTP_400_BAD_REQUEST,
                )
        cycle = None
        if "cycle_id" in properties and properties["cycle_id"] is not None:
            cycle = Cycle.objects.filter(
                workspace__slug=slug, project_id=project_id, pk=properties["cycle_id"]
            ).first()
            if cycle is None:
                return Response(
                    {"error": f"Cycle {properties['cycle_id']} does not exist in this project"},
                    status=status.HTTP_400_BAD_REQUEST,
                )
            if cycle.end_date is not None and cycle.end_date < timezone.now():
                return Response(
                    {"error": "The Cycle has already been completed so no new issues can be added"},
                    status=status.HTTP_400_BAD_REQUEST,
                )
        for issue in issues:
            start = properties["start_date"] if "start_date" in properties else issue.start_date
            target = properties["target_date"] if "target_date" in properties else issue.target_date
            if start is not None and target is not None and start > target:
                return Response(
                    {"error": "Start date cannot exceed target date"},
                    status=status.HTTP_400_BAD_REQUEST,
                )

        epoch = int(timezone.now().timestamp())
        with transaction.atomic():
            scalar_fields = ["updated_at"]
            if "state_id" in properties:
                scalar_fields.append("state_id")
            if "priority" in properties:
                scalar_fields.append("priority")
            if "start_date" in properties:
                scalar_fields.append("start_date")
            if "target_date" in properties:
                scalar_fields.append("target_date")
            if "estimate_point" in properties:
                scalar_fields.append("estimate_point_id")
            now = timezone.now()
            applied = {}
            if "state_id" in properties:
                applied["state_id"] = str(properties["state_id"])
            if "priority" in properties:
                applied["priority"] = properties["priority"]
            if "start_date" in properties:
                applied["start_date"] = str(properties["start_date"])
            if "target_date" in properties:
                applied["target_date"] = str(properties["target_date"])
            if "estimate_point" in properties:
                applied["estimate_point"] = str(properties["estimate_point"])
            for issue in issues:
                previous = {}
                if "state_id" in properties:
                    previous["state_id"] = str(issue.state_id)
                    issue.state_id = properties["state_id"]
                if "priority" in properties:
                    previous["priority"] = issue.priority
                    issue.priority = properties["priority"]
                if "start_date" in properties:
                    previous["start_date"] = str(issue.start_date)
                    issue.start_date = properties["start_date"]
                if "target_date" in properties:
                    previous["target_date"] = str(issue.target_date)
                    issue.target_date = properties["target_date"]
                if "estimate_point" in properties:
                    previous["estimate_point"] = str(issue.estimate_point_id)
                    issue.estimate_point_id = properties["estimate_point"]
                issue.updated_at = now
                issue._bulk_previous = previous
            if issues:
                Issue.objects.bulk_update(issues, scalar_fields, batch_size=100)

            issue_ids = [issue.id for issue in issues]
            membership = {"project_id": project_id, "workspace_id": project.workspace_id}
            if properties.get("label_ids"):
                applied["label_ids"] = [str(item) for item in properties["label_ids"]]
                live = set(
                    IssueLabel.objects.filter(
                        issue_id__in=issue_ids, deleted_at__isnull=True
                    ).values_list("issue_id", "label_id")
                )
                IssueLabel.objects.bulk_create(
                    [
                        IssueLabel(
                            issue_id=issue.id,
                            label_id=label_id,
                            created_by=request.user,
                            updated_by=request.user,
                            **membership,
                        )
                        for issue in issues
                        for label_id in properties["label_ids"]
                        if (issue.id, label_id) not in live
                    ],
                    batch_size=10,
                    ignore_conflicts=True,
                )
            if properties.get("assignee_ids"):
                applied["assignee_ids"] = [str(item) for item in properties["assignee_ids"]]
                live = set(
                    IssueAssignee.objects.filter(
                        issue_id__in=issue_ids, deleted_at__isnull=True
                    ).values_list("issue_id", "assignee_id")
                )
                IssueAssignee.objects.bulk_create(
                    [
                        IssueAssignee(
                            issue_id=issue.id,
                            assignee_id=assignee_id,
                            created_by=request.user,
                            updated_by=request.user,
                            **membership,
                        )
                        for issue in issues
                        for assignee_id in properties["assignee_ids"]
                        if (issue.id, assignee_id) not in live
                    ],
                    batch_size=10,
                    ignore_conflicts=True,
                )
            if properties.get("module_ids"):
                module_ids = [str(item) for item in properties["module_ids"]]
                applied["module_ids"] = module_ids
                live = set(
                    ModuleIssue.objects.filter(
                        issue_id__in=issue_ids, deleted_at__isnull=True
                    ).values_list("issue_id", "module_id")
                )
                added = [
                    (issue, module_id)
                    for issue in issues
                    for module_id in properties["module_ids"]
                    if (issue.id, module_id) not in live
                ]
                if added:
                    ModuleIssue.objects.bulk_create(
                        [
                            ModuleIssue(
                                issue_id=issue.id,
                                module_id=module_id,
                                created_by=request.user,
                                updated_by=request.user,
                                **membership,
                            )
                            for issue, module_id in added
                        ],
                        batch_size=10,
                        ignore_conflicts=True,
                    )
                for issue, module_id in added:
                    issue_activity.delay(
                        type="module.activity.created",
                        requested_data=json.dumps({"module_id": str(module_id)}),
                        actor_id=str(request.user.id),
                        issue_id=str(issue.id),
                        project_id=str(project_id),
                        current_instance=None,
                        epoch=epoch,
                        notification=True,
                        origin=base_host(request=request, is_app=True),
                    )
            if "cycle_id" in properties:
                if cycle is None:
                    for issue in issues:
                        issue_activity.delay(
                            type="cycle.activity.deleted",
                            requested_data=json.dumps(
                                {"cycle_id": None, "issues": [str(issue.id)]}
                            ),
                            actor_id=str(request.user.id),
                            issue_id=str(issue.id),
                            project_id=str(project_id),
                            current_instance=None,
                            epoch=epoch,
                            notification=True,
                            origin=base_host(request=request, is_app=True),
                        )
                    CycleIssue.objects.filter(issue_id__in=issue_ids).delete()
                else:
                    applied["cycle_id"] = str(cycle.id)
                    rows = list(
                        CycleIssue.objects.filter(issue_id__in=issue_ids, deleted_at__isnull=True)
                    )
                    moved = [row for row in rows if str(row.cycle_id) != str(cycle.id)]
                    staying = {str(row.issue_id) for row in rows if str(row.cycle_id) == str(cycle.id)}
                    created = CycleIssue.objects.bulk_create(
                        [
                            CycleIssue(
                                issue_id=issue.id,
                                cycle_id=cycle.id,
                                created_by_id=request.user.id,
                                updated_by_id=request.user.id,
                                **membership,
                            )
                            for issue in issues
                            if str(issue.id) not in staying
                            and str(issue.id) not in {str(row.issue_id) for row in moved}
                        ],
                        batch_size=10,
                    )
                    moved_activity = []
                    for row in moved:
                        moved_activity.append(
                            {
                                "old_cycle_id": str(row.cycle_id),
                                "new_cycle_id": str(cycle.id),
                                "issue_id": str(row.issue_id),
                            }
                        )
                        row.cycle_id = cycle.id
                    if moved:
                        CycleIssue.objects.bulk_update(moved, ["cycle_id"], batch_size=100)
                    issue_activity.delay(
                        type="cycle.activity.created",
                        requested_data=json.dumps({"cycles_list": [str(item) for item in issue_ids]}),
                        actor_id=str(request.user.id),
                        issue_id=None,
                        project_id=str(project_id),
                        current_instance=json.dumps(
                            {
                                "updated_cycle_issues": moved_activity,
                                "created_cycle_issues": django_serializers.serialize("json", created),
                            }
                        ),
                        epoch=epoch,
                        notification=True,
                        origin=base_host(request=request, is_app=True),
                    )
            for issue in issues:
                issue_activity.delay(
                    type="issue.activity.updated",
                    requested_data=json.dumps(applied),
                    actor_id=str(request.user.id),
                    issue_id=str(issue.id),
                    project_id=str(project_id),
                    current_instance=json.dumps(issue._bulk_previous),
                    epoch=epoch,
                    notification=True,
                    origin=base_host(request=request, is_app=True),
                )

        return Response(
            {"message": f"{len(issues)} issues were updated", "updated": len(issues)},
            status=status.HTTP_200_OK,
        )


class BulkSubscribeIssuesEndpoint(BaseAPIView):
    permission_classes = [ProjectLitePermission]

    def post(self, request, slug, project_id):
        serializer = BulkSubscribeSerializer(data=request.data)
        if not serializer.is_valid():
            return Response(serializer.errors, status=status.HTTP_400_BAD_REQUEST)

        project = Project.objects.filter(pk=project_id, workspace__slug=slug).first()
        if project is None:
            return Response({"error": "Project not found"}, status=status.HTTP_404_NOT_FOUND)

        issues = list(
            Issue.objects.filter(
                workspace__slug=slug, project_id=project_id, pk__in=serializer.validated_data["issue_ids"]
            )
        )
        issue_ids = [issue.id for issue in issues]
        subscribed = set(
            IssueSubscriber.objects.filter(
                issue_id__in=issue_ids, subscriber=request.user, deleted_at__isnull=True
            ).values_list("issue_id", flat=True)
        )
        fresh = [issue for issue in issues if issue.id not in subscribed]
        if fresh:
            IssueSubscriber.objects.bulk_create(
                [
                    IssueSubscriber(
                        issue_id=issue.id,
                        subscriber_id=request.user.id,
                        project_id=project_id,
                        workspace_id=project.workspace_id,
                        created_by=request.user,
                        updated_by=request.user,
                    )
                    for issue in fresh
                ],
                batch_size=10,
                ignore_conflicts=True,
            )

        return Response(
            {"message": f"Subscribed to {len(fresh)} issues", "subscribed": len(fresh)},
            status=status.HTTP_200_OK,
        )
