# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Shared "which runners can this user see?" queryset builder.

Three read surfaces list a workspace's runners and must apply identical
rules: the web AI Workers panel (``RunnerListEndpoint``, session auth),
the token-auth CLI endpoint (``GET /api/v1/workspaces/<slug>/projects/
<project>/runners/``), and the hosted MCP ``pidash_list_project_runners``
tool. This module is the single source of truth for those rules so the
surfaces cannot drift apart:

- the caller must be a member of the workspace (``PermissionDenied``
  otherwise);
- private runners are visible only to their owner, even inside a shared
  workspace (``runner_visible_to_user_q``);
- desktop-bundled runners are excluded unless explicitly asked for —
  the user never registered them and cannot manage them;
- optional narrowing to one project (every pod the project owns) or one
  pod.
"""

from __future__ import annotations

from django.core.exceptions import PermissionDenied

from pi_dash.runner.models import Runner, RunnerProvisioning
from pi_dash.runner.services.permissions import (
    is_workspace_member,
    runner_visible_to_user_q,
)


def project_runners_queryset(
    user,
    workspace_id,
    *,
    project_id=None,
    pod_id=None,
    include_bundled=False,
):
    """Runners in ``workspace_id`` that ``user`` may see, newest-updated first.

    Raises :class:`~django.core.exceptions.PermissionDenied` when ``user`` is
    not a member of the workspace, so callers cannot forget the membership
    gate. The returned queryset joins ``pod__project`` and ``dev_machine``
    because ``RunnerSerializer``'s nested mini serializers read them (avoids
    N+1 on every list render).
    """
    if not is_workspace_member(user, workspace_id):
        raise PermissionDenied("caller is not a member of this workspace")
    qs = (
        Runner.objects.filter(workspace_id=workspace_id)
        .filter(runner_visible_to_user_q(user))
        .select_related("pod__project", "dev_machine")
        .order_by("-updated_at")
    )
    if pod_id:
        qs = qs.filter(pod_id=pod_id)
    if not include_bundled:
        qs = qs.exclude(provisioning=RunnerProvisioning.DESKTOP_BUNDLED)
    if project_id:
        qs = qs.filter(pod__project_id=project_id)
    return qs


def parse_include_bundled(query_params) -> bool:
    """Truthiness rule for the ``include_bundled`` query flag, shared by the
    session and token endpoints so "true" means the same thing on both."""
    return query_params.get("include_bundled") in ("1", "true", "yes")
