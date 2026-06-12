# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Workspace-membership and role helpers for the runner app.

Extracted so runner views, pod views, validation, and orchestration can share a
single source of truth. See ``.ai_design/issue_runner/design.md`` §5.1.

Role values come from ``pi_dash.db.models.workspace.ROLE_CHOICES``:
``Admin=20``, ``Member=15``, ``Guest=5``.
"""

from __future__ import annotations

from django.db.models import Exists, OuterRef, Q

from pi_dash.runner.models import Visibility

# Workspace membership/role helpers now live in the neutral ``pi_dash.core``
# package so the assistant app can share them without importing ``runner``.
# Re-exported here for backwards compatibility with existing runner callers.
from pi_dash.core.permissions import (  # noqa: F401
    ROLE_ADMIN,
    ROLE_GUEST,
    ROLE_MEMBER,
    is_at_least_member,
    is_workspace_admin,
    is_workspace_member,
    workspace_role,
)


def _authenticated_user_id(user):
    if user is None or not getattr(user, "is_authenticated", False):
        return None
    return getattr(user, "id", None)


def runner_visible_to_user_q(user, *, prefix: str = "") -> Q:
    """ORM predicate for runners visible to ``user``.

    Visibility currently has one public value: ``PRIVATE=0``. Private means the
    row is visible only to its owner, even inside a shared workspace.
    """
    user_id = _authenticated_user_id(user)
    if user_id is None:
        return Q(**{f"{prefix}pk__isnull": True})
    return Q(
        **{
            f"{prefix}owner_id": user_id,
            f"{prefix}visibility": Visibility.PRIVATE,
        }
    )


def can_view_dev_machine(user, dev_machine) -> bool:
    user_id = _authenticated_user_id(user)
    if user_id is None:
        return False
    if dev_machine.visibility == Visibility.PRIVATE:
        return dev_machine.owner_id == user_id
    return False


def can_use_dev_machine(user, dev_machine) -> bool:
    return can_view_dev_machine(user, dev_machine)


def can_view_runner(user, runner) -> bool:
    user_id = _authenticated_user_id(user)
    if user_id is None:
        return False
    if runner.visibility == Visibility.PRIVATE:
        return runner.owner_id == user_id
    return False


def can_use_runner(user, runner) -> bool:
    return can_view_runner(user, runner)


def filter_runs_usable_by_runner(qs, runner):
    """Limit an AgentRun queryset to work this runner may consume.

    Private runners can process work initiated by their owner, work already
    billed to their owner, issue work involving their owner, or project
    scheduler work explicitly authored by their owner.
    """
    if runner.visibility == Visibility.PRIVATE:
        from pi_dash.db.models.issue import Issue
        from pi_dash.db.models.scheduler import SchedulerBinding

        visible_issue = Issue.objects.filter(pk=OuterRef("work_item_id")).filter(
            Q(created_by_id=runner.owner_id) | Q(assignees__id=runner.owner_id)
        )
        visible_scheduler = SchedulerBinding.objects.filter(
            pk=OuterRef("scheduler_binding_id"),
            actor_id=runner.owner_id,
        )
        return qs.annotate(
            _runner_visible_issue=Exists(visible_issue),
            _runner_visible_scheduler=Exists(visible_scheduler),
        ).filter(
            Q(created_by_id=runner.owner_id)
            | Q(owner_id=runner.owner_id)
            | Q(_runner_visible_issue=True)
            | Q(_runner_visible_scheduler=True)
        )
    return qs.none()


def can_manage_runner(user, runner) -> bool:
    """True if ``user`` may mutate the runner.

    Shared by every runner view (revoke/revive/delete/patch) and both
    delete surfaces (web session auth + the X-Api-Key v1 endpoint), so
    the rule lives in one place. Private runners are owner-managed only.
    """
    if not can_view_runner(user, runner):
        return False
    if runner.visibility == Visibility.PRIVATE:
        return runner.owner_id == user.id
    return runner.owner_id == user.id or is_workspace_admin(user, runner.workspace_id)
