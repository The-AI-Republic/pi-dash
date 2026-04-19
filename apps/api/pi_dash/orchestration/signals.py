# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Issue state-transition hook.

We intercept Issue saves with a pre_save snapshot of the prior ``state_id`` and
compare in post_save. Using signals keeps the trigger out of every writer path
(REST views, admin, importers) while still routing every transition through
``orchestration.service``.
"""

from __future__ import annotations

import logging

from django.db.models.signals import post_save, pre_save
from django.dispatch import receiver

from pi_dash.db.models.issue import Issue
from pi_dash.db.models.state import State
from pi_dash.orchestration.service import handle_issue_state_transition

logger = logging.getLogger(__name__)

_PREVIOUS_STATE = "_orchestration_prev_state_id"


@receiver(pre_save, sender=Issue, dispatch_uid="orchestration.issue_presave")
def capture_prior_state(sender, instance: Issue, **kwargs) -> None:
    if not instance.pk:
        setattr(instance, _PREVIOUS_STATE, None)
        return
    try:
        prior = Issue.all_objects.only("state_id").get(pk=instance.pk)
    except Issue.DoesNotExist:
        setattr(instance, _PREVIOUS_STATE, None)
        return
    setattr(instance, _PREVIOUS_STATE, prior.state_id)


@receiver(post_save, sender=Issue, dispatch_uid="orchestration.issue_postsave")
def fire_state_transition(sender, instance: Issue, created: bool, **kwargs) -> None:
    prev_state_id = getattr(instance, _PREVIOUS_STATE, None)
    current_state_id = instance.state_id
    if prev_state_id == current_state_id:
        return

    from_state = _lookup_state(prev_state_id)
    to_state = instance.state if current_state_id else _lookup_state(current_state_id)

    try:
        handle_issue_state_transition(
            issue=instance,
            from_state=from_state,
            to_state=to_state,
            actor=None,
        )
    except Exception:  # noqa: BLE001 — never let orchestration crash issue save
        logger.exception(
            "orchestration: handle_issue_state_transition failed for issue %s",
            instance.pk,
        )


def _lookup_state(state_id) -> State | None:
    if state_id is None:
        return None
    try:
        return State.all_state_objects.get(pk=state_id)
    except State.DoesNotExist:
        return None
