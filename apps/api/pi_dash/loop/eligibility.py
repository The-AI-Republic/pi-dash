# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Loop eligibility — the single place "may this edge run this job now?" is
answered, in two forms: a queryset pre-filter for the scanner's bulk fan-out
and a per-row re-check for the fire task (freshest-wins under SFU).

This module is also the **Cloud seam**: the BYOK credential check is one
overridable function (:func:`llm_available_q`), mirroring how
``resolve_model_for_user`` lives behind ``pi_dash.ee.assistant.model_provider``.
The cloud overlay replaces it to also admit plan-entitled users with platform
keys. See ``.ai_design/loop_project_management/design.md`` §7.8.
"""

from __future__ import annotations

from typing import Optional

from django.db.models import Exists, OuterRef, Q, QuerySet
from django.utils import timezone

from pi_dash.assistant.models import UserLLMConfig
from pi_dash.db.models import LoopTarget, LoopUserPreference, SkipReason, WorkspaceMember


def _usable_llm_filter() -> dict:
    """Single source of truth for "this user has usable LLM credentials".

    CE: a ``UserLLMConfig`` row with a stored key. The cloud overlay widens
    this (or overrides :func:`llm_available_q` / :func:`user_has_llm` together)
    to also admit plan-entitled users. Keeping the filter in one place is what
    guarantees the scanner pre-filter and the fire-time re-check can't diverge.
    """
    return {"api_key_encrypted__isnull": False}


def llm_available_q() -> Exists:
    """``Exists`` subquery: does ``OuterRef('user_id')`` have usable LLM creds?"""
    return Exists(
        UserLLMConfig.objects.filter(user_id=OuterRef("user_id"), **_usable_llm_filter())
    )


def user_has_llm(user_id) -> bool:
    """Row-level form of :func:`llm_available_q` for the fire-time re-check.

    Shares ``_usable_llm_filter`` with the queryset form so the scanner and the
    per-target ``check`` always agree on who has credentials.
    """
    return UserLLMConfig.objects.filter(user_id=user_id, **_usable_llm_filter()).exists()


def _member_q() -> Exists:
    return Exists(
        WorkspaceMember.objects.filter(
            workspace_id=OuterRef("workspace_id"),
            member_id=OuterRef("user_id"),
            is_active=True,
            deleted_at__isnull=True,
            role__gte=OuterRef("job__min_role"),
        )
    )


def _job_off_q() -> Exists:
    return Exists(
        LoopUserPreference.objects.filter(
            user_id=OuterRef("user_id"),
            job_id=OuterRef("job_id"),
            enabled=False,
            deleted_at__isnull=True,
        )
    )


def _master_paused_q() -> Exists:
    return Exists(
        LoopUserPreference.objects.filter(
            user_id=OuterRef("user_id"),
            job__isnull=True,
            enabled=False,
            deleted_at__isnull=True,
        )
    )


def due_targets(now=None) -> QuerySet:
    """All targets whose cursor is due (or NULL = newly created), for enabled,
    non-deleted jobs. Does NOT apply eligibility — used by the scanner to
    compute both the eligible set and the ineligible-to-advance set."""
    now = now or timezone.now()
    return (
        LoopTarget.objects.filter(
            deleted_at__isnull=True,
            job__enabled=True,
            job__deleted_at__isnull=True,
        )
        .filter(Q(next_run_at__lte=now) | Q(next_run_at__isnull=True))
    )


def eligible_due_targets(now=None) -> QuerySet:
    """Due targets that pass every eligibility predicate — safe to dispatch."""
    return (
        due_targets(now)
        .annotate(
            _member=_member_q(),
            _job_off=_job_off_q(),
            _paused=_master_paused_q(),
            _llm=llm_available_q(),
        )
        .filter(_member=True, _job_off=False, _paused=False, _llm=True)
    )


def check(target: LoopTarget) -> Optional[str]:
    """Fire-time re-check of one claimed target.

    Returns a :class:`SkipReason` value, or ``None`` when the target is eligible.
    Predicates are evaluated in a fixed precedence order so the recorded skip
    reason is deterministic: master pause → job opt-out → membership/role →
    LLM credentials.
    """
    user_id = target.user_id

    if LoopUserPreference.objects.filter(
        user_id=user_id, job__isnull=True, enabled=False, deleted_at__isnull=True
    ).exists():
        return SkipReason.MASTER_PAUSED

    if LoopUserPreference.objects.filter(
        user_id=user_id, job_id=target.job_id, enabled=False, deleted_at__isnull=True
    ).exists():
        return SkipReason.USER_DISABLED

    membership = (
        WorkspaceMember.objects.filter(
            workspace_id=target.workspace_id,
            member_id=user_id,
            is_active=True,
            deleted_at__isnull=True,
        )
        .values_list("role", flat=True)
        .first()
    )
    if membership is None:
        return SkipReason.MEMBERSHIP_GONE
    if membership < target.job.min_role:
        return SkipReason.MIN_ROLE

    if not user_has_llm(user_id):
        return SkipReason.LLM_CONFIG_MISSING

    return None
