# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Writes to an existing :class:`Scheduler` definition.

One implementation shared by the web app surface
(``pi_dash/app/views/scheduler``) and the external ``/api/v1/`` surface
(``pi_dash/api/views/scheduler``), so the two cannot drift: both validate
through ``SchedulerSerializer``.
"""

from __future__ import annotations

from typing import Mapping

from pi_dash.db.models import Scheduler
from pi_dash.db.models.scheduler import SchedulerSource


def update_definition(scheduler: Scheduler, data: Mapping) -> Scheduler:
    """Apply a partial edit to ``scheduler`` and return it.

    Raises ``rest_framework.serializers.ValidationError`` on bad input.
    """
    from pi_dash.app.serializers.scheduler import SchedulerSerializer

    serializer = SchedulerSerializer(scheduler, data=data, partial=True)
    serializer.is_valid(raise_exception=True)
    serializer.save()
    return scheduler


def is_builtin(scheduler: Scheduler) -> bool:
    """Whether ``scheduler`` is a seeded built-in definition.

    ``ensure_builtin_schedulers`` overwrites the name, description and prompt
    of those rows, so an edit to one would not survive a reseed.

    NOTE (PDASHOSS01-290): ``Scheduler.source`` defaults to ``builtin`` and
    the create view never sets it, so today this is also true for
    user-created schedulers. That is 290's bug to fix; this is the one place
    to adjust if its fix changes how built-ins are told apart.
    """
    return scheduler.source == SchedulerSource.BUILTIN
