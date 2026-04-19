# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

# Third Party imports
from rest_framework.permissions import SAFE_METHODS, BasePermission

# Module import
from pi_dash.db.models import ProjectMember, WorkspaceMember
from pi_dash.db.models.project import ROLE


class ProjectBasePermission(BasePermission):
    def has_permission(self, request, view):
        if request.user.is_anonymous:
            return False

        ## Safe Methods -> Handle the filtering logic in queryset
        if request.method in SAFE_METHODS:
            return WorkspaceMember.objects.filter(
                workspace__slug=view.workspace_slug, member=request.user, is_active=True
            ).exists()

        ## Only workspace owners or admins can create the projects
        if request.method == "POST":
            return WorkspaceMember.objects.filter(
                workspace__slug=view.workspace_slug,
                member=request.user,
                role__in=[ROLE.ADMIN.value, ROLE.MEMBER.value],
                is_active=True,
            ).exists()

        project_member_qs = ProjectMember.objects.filter(
            workspace__slug=view.workspace_slug,
            member=request.user,
            project_id=view.project_id,
            is_active=True,
        )

        ## Only project admins or workspace admin who is part of the project can access

        if project_member_qs.filter(role=ROLE.ADMIN.value).exists():
            return True
        else:
            return (
                project_member_qs.exists()
                and WorkspaceMember.objects.filter(
                    member=request.user,
                    workspace__slug=view.workspace_slug,
                    role=ROLE.ADMIN.value,
                    is_active=True,
                ).exists()
            )


class ProjectMemberPermission(BasePermission):
    def has_permission(self, request, view):
        if request.user.is_anonymous:
            return False

        ## Safe Methods -> Handle the filtering logic in queryset
        if request.method in SAFE_METHODS:
            return ProjectMember.objects.filter(
                workspace__slug=view.workspace_slug, member=request.user, is_active=True
            ).exists()
        ## Only workspace owners or admins can create the projects
        if request.method == "POST":
            return WorkspaceMember.objects.filter(
                workspace__slug=view.workspace_slug,
                member=request.user,
                role__in=[ROLE.ADMIN.value, ROLE.MEMBER.value],
                is_active=True,
            ).exists()

        ## Only Project Admins can update project attributes
        return ProjectMember.objects.filter(
            workspace__slug=view.workspace_slug,
            member=request.user,
            role__in=[ROLE.ADMIN.value, ROLE.MEMBER.value],
            project_id=view.project_id,
            is_active=True,
        ).exists()


class ProjectEntityPermission(BasePermission):
    def has_permission(self, request, view):
        if request.user.is_anonymous:
            return False

        # Handle requests based on project__identifier
        if hasattr(view, "project_identifier") and view.project_identifier:
            if request.method in SAFE_METHODS:
                return ProjectMember.objects.filter(
                    workspace__slug=view.workspace_slug,
                    member=request.user,
                    project__identifier=view.project_identifier,
                    is_active=True,
                ).exists()

        ## Safe Methods -> Handle the filtering logic in queryset
        if request.method in SAFE_METHODS:
            return ProjectMember.objects.filter(
                workspace__slug=view.workspace_slug,
                member=request.user,
                project_id=view.project_id,
                is_active=True,
            ).exists()

        ## Only project members or admins can create and edit the project attributes
        return ProjectMember.objects.filter(
            workspace__slug=view.workspace_slug,
            member=request.user,
            role__in=[ROLE.ADMIN.value, ROLE.MEMBER.value],
            project_id=view.project_id,
            is_active=True,
        ).exists()


class ProjectAdminPermission(BasePermission):
    def has_permission(self, request, view):
        if request.user.is_anonymous:
            return False

        return ProjectMember.objects.filter(
            workspace__slug=view.workspace_slug,
            member=request.user,
            role=ROLE.ADMIN.value,
            project_id=view.project_id,
            is_active=True,
        ).exists()


class ProjectLitePermission(BasePermission):
    def has_permission(self, request, view):
        if request.user.is_anonymous:
            return False

        return ProjectMember.objects.filter(
            workspace__slug=view.workspace_slug,
            member=request.user,
            project_id=view.project_id,
            is_active=True,
        ).exists()


def can_mutate_states(user, slug, project_id):
    """Whether *user* can create, update, reorder, mark-default, or delete
    workflow states in the given project.

    Allowed when the user is:
    - a project admin, or
    - a project member on a project with ``members_can_edit_states=True``, or
    - a workspace admin who is an active project member (mirrors the
      workspace-admin override in ``allow_permission``).
    """
    if user.is_anonymous:
        return False

    membership = (
        ProjectMember.objects.filter(
            workspace__slug=slug,
            project_id=project_id,
            member=user,
            is_active=True,
        )
        .values("role", "project__members_can_edit_states")
        .first()
    )
    if membership is None:
        return False

    role = membership["role"]
    if role == ROLE.ADMIN.value:
        return True

    if role == ROLE.MEMBER.value and membership["project__members_can_edit_states"]:
        return True

    return WorkspaceMember.objects.filter(
        workspace__slug=slug,
        member=user,
        role=ROLE.ADMIN.value,
        is_active=True,
    ).exists()


class ProjectStateEntityPermission(BasePermission):
    """Permissions for the workflow State entity.

    Admin can always write. Member can write only when the owning project has
    ``members_can_edit_states=True``. Any active project member can read.
    """

    def has_permission(self, request, view):
        if request.user.is_anonymous:
            return False

        if request.method in SAFE_METHODS:
            return ProjectMember.objects.filter(
                workspace__slug=view.workspace_slug,
                member=request.user,
                project_id=view.project_id,
                is_active=True,
            ).exists()

        return can_mutate_states(request.user, view.workspace_slug, view.project_id)
