# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Availability and admission policy for the desktop-bundled managed runner.

One function, :func:`managed_runner_availability`, answers "can this viewer run
this project's work on their own desktop right now" for every caller: the
executor picker, the desktop's profile endpoint, and run creation. Keeping the
gates and their order in a single place is what makes the reason a user sees in
the UI match the reason the API refuses with.

Availability here is **viewer-and-device scoped** — "is *this user's* desktop
app open" — which is precisely what a project-scoped local-runner check cannot
express, and the reason the managed runner is its own executor kind rather than
a flavour of ``local_runner``.
"""

from __future__ import annotations

from django.utils import timezone

from pi_dash.core.agent_execution import managed_runner_is_enabled
from pi_dash.managed_runner.errors import ManagedRunnerReason


def managed_llm_profile(user):
    """The desktop model profile for ``user``, via the EE-overlayable seam.

    Delegates to the same module Pi Dash AI and the Cloud Agent resolve models
    through, so the desktop can never drift onto a different notion of "which
    provider is this user on".
    """
    from pi_dash.ee.assistant.model_provider import agent_model_profile_for_user

    return agent_model_profile_for_user(user)


def enrolled_managed_runners(project, user):
    """Non-revoked bundled runners ``user`` owns on ``project``'s pod.

    Structural membership only — a closed laptop still counts as enrolled, the
    same way an OFFLINE local runner does for
    ``pod_has_runner_for_issue_principal``. Online-ness is a separate question
    answered by :func:`managed_runner_availability`.
    """
    from pi_dash.runner.models import Runner, RunnerProvisioning

    return Runner.objects.filter(
        owner=user,
        provisioning=RunnerProvisioning.DESKTOP_BUNDLED,
        pod__project_id=project.id,
        workspace_id=project.workspace_id,
        revoked_at__isnull=True,
    )


def online_managed_runner(project, user):
    """The viewer's bundled runner on ``project`` that can take work now."""
    from pi_dash.runner.services.matcher import HEARTBEAT_GRACE
    from pi_dash.runner.models import RunnerStatus

    if user is None:
        return None
    return (
        enrolled_managed_runners(project, user)
        .filter(
            status=RunnerStatus.ONLINE,
            last_heartbeat_at__gte=timezone.now() - HEARTBEAT_GRACE,
        )
        .order_by("-last_heartbeat_at")
        .first()
    )


def managed_runner_availability(project, user) -> tuple[bool, str]:
    """``(available, reason_code)`` for the managed runner on ``project``.

    Gates are evaluated in a fixed order and the first failure wins, so a user
    who is both BYOK-only and offline always sees the BYOK explanation (the one
    they can act on) rather than a race between two true statements.
    """
    if not managed_runner_is_enabled():
        return False, ManagedRunnerReason.DISABLED
    if user is None or not getattr(user, "is_active", False) or getattr(user, "is_bot", False):
        return False, ManagedRunnerReason.NOT_CONNECTED

    profile = managed_llm_profile(user)
    if not profile.available:
        return False, profile.reason_code or ManagedRunnerReason.LLM_CONFIG_MISSING

    if not enrolled_managed_runners(project, user).exists():
        return False, ManagedRunnerReason.NO_RUNNER_FOR_PROJECT
    if online_managed_runner(project, user) is None:
        return False, ManagedRunnerReason.NOT_CONNECTED
    return True, ""
