# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Issue state-transition hook for GitHub completion comment-back.

Pre-save snapshot of prior `state_id`, post-save fires
`post_completion_comment` when an issue moves into a `completed`-group state
and is mirrored from GitHub. See .ai_design/github_sync/design.md §6.5.

We use a separate dispatch_uid namespace from `orchestration.signals` so the
two pre/post_save pairs run independently (each module owns its own snapshot
attribute on the instance).
"""

from __future__ import annotations

import logging

from django.db.models.signals import post_save, pre_save
from django.dispatch import receiver

from pi_dash.db.models.issue import Issue
from pi_dash.db.models.state import StateGroup

logger = logging.getLogger(__name__)

_PREVIOUS_STATE = "_github_sync_prev_state_id"


@receiver(pre_save, sender=Issue, dispatch_uid="github_sync.issue_presave")
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


@receiver(post_save, sender=Issue, dispatch_uid="github_sync.issue_postsave")
def trigger_completion_comment(sender, instance: Issue, created: bool, **kwargs) -> None:
    if created:
        return  # creation can't be a "transition"; freshly-imported synced issues land here too
    prev_state_id = getattr(instance, _PREVIOUS_STATE, None)
    if prev_state_id == instance.state_id:
        return
    if instance.state is None or instance.state.group != StateGroup.COMPLETED.value:
        return

    # Lazy-import to avoid loading models at app-config time and to dodge a
    # circular-import risk through the bgtasks → db → bgtasks chain.
    from pi_dash.db.models.integration.github import GithubIssueSync
    from pi_dash.bgtasks.github_sync_task import post_completion_comment

    issue_sync = GithubIssueSync.objects.filter(issue=instance).first()
    if issue_sync is None:
        return  # not a synced issue
    if issue_sync.metadata.get("completion_comment_id"):
        return  # already commented for a prior completion

    post_completion_comment.delay(str(issue_sync.id))
