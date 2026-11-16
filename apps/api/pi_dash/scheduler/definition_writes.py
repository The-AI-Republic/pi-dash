# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Writes to :class:`Scheduler` definitions: create, edit, delete.

One implementation shared by the web app surface
(``pi_dash/app/views/scheduler``) and the external ``/api/v1/`` surface
(``pi_dash/api/views/scheduler``), so the two cannot drift: both validate
through ``SchedulerSerializer``.
"""

from __future__ import annotations

from typing import Mapping

from django.db import transaction
from django.utils import timezone

from pi_dash.db.models import Scheduler, SchedulerBinding


def create_definition(workspace, data: Mapping) -> Scheduler:
    """Create a scheduler definition in ``workspace`` and return it.

    The workspace goes into the serializer context so a duplicate slug is
    a ``{"slug": [...]}`` field error rather than an IntegrityError.
    Raises ``rest_framework.serializers.ValidationError`` on bad input.
    """
    from pi_dash.app.serializers.scheduler import SchedulerSerializer

    serializer = SchedulerSerializer(data=data, context={"workspace": workspace})
    serializer.is_valid(raise_exception=True)
    return serializer.save(workspace=workspace)


def update_definition(scheduler: Scheduler, data: Mapping) -> Scheduler:
    """Apply a partial edit to ``scheduler`` and return it.

    Raises ``rest_framework.serializers.ValidationError`` on bad input.
    """
    from pi_dash.app.serializers.scheduler import SchedulerSerializer

    serializer = SchedulerSerializer(scheduler, data=data, partial=True)
    serializer.is_valid(raise_exception=True)
    serializer.save()
    return scheduler


def delete_definition(scheduler: Scheduler) -> None:
    """Soft-delete ``scheduler`` and every active install of it.

    The SoftDeleteModel cascade is async (see db/mixins.py
    ``soft_delete_related_objects``), which leaves a window where active
    bindings still point at a soft-deleted parent. The scanner already
    filters those out, but they remain addressable via the bindings list
    endpoint and would survive a same-slug recreate as orphans. Soft-delete
    bindings inline so the API view of the world is consistent the moment
    this returns.
    """
    now = timezone.now()
    with transaction.atomic():
        SchedulerBinding.objects.filter(
            scheduler=scheduler,
            deleted_at__isnull=True,
        ).update(deleted_at=now)
        scheduler.delete()  # SoftDeleteModel: sets deleted_at


def is_builtin_slug(slug: str) -> bool:
    """Whether ``slug`` belongs to the built-in catalog."""
    from pi_dash.scheduler.builtins import BUILTINS

    return any(builtin.slug == slug for builtin in BUILTINS)


def is_builtin(scheduler: Scheduler) -> bool:
    """Whether ``scheduler`` is a seeded built-in definition.

    ``ensure_builtin_schedulers`` matches rows by slug and overwrites their
    name, description and prompt, and recreates a deleted one, so neither an
    edit nor a delete of one would survive a reseed. The slug is the test,
    not ``source``: ``Scheduler.source`` defaults to ``builtin`` for
    user-created rows too (PDASHOSS01-290).
    """
    return is_builtin_slug(scheduler.slug)
