# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Fixtures for loop (Auto Project Management) tests.

Reuses the assistant contract fixtures (``world``, ``fernet_key``,
``configure_llm``) so the access-control matrix is identical, and adds builders
for loop jobs / targets.
"""

from __future__ import annotations

import datetime

import pytest
from django.utils import timezone

# Re-export the assistant fixtures so they're discoverable in this package.
from pi_dash.tests.contract.assistant.conftest import (  # noqa: F401
    configure_llm,
    fernet_key,
    world,
)
from pi_dash.db.models import LoopJob, LoopTarget, LoopUserPreference


def make_job(
    *,
    slug="auto-close-merged",
    name="Auto-close",
    public_name="Close merged",
    prompt="do the thing",
    min_role=15,
    enabled=True,
    is_builtin=True,
    rrule="FREQ=DAILY;BYHOUR=3;BYMINUTE=0",
    dtstart=None,
) -> LoopJob:
    return LoopJob.objects.create(
        slug=slug,
        name=name,
        public_name=public_name,
        public_description="desc",
        prompt=prompt,
        min_role=min_role,
        enabled=enabled,
        is_builtin=is_builtin,
        rrule=rrule,
        tzid="UTC",
        dtstart=dtstart or (timezone.now() - datetime.timedelta(days=1)),
    )


def make_target(job, workspace, user, *, next_run_at="due", thread=None) -> LoopTarget:
    if next_run_at == "due":
        next_run_at = timezone.now() - datetime.timedelta(minutes=1)
    return LoopTarget.objects.create(
        job=job, workspace=workspace, user=user, next_run_at=next_run_at, thread=thread
    )


def set_pref(user, job, enabled) -> LoopUserPreference:
    return LoopUserPreference.objects.create(user=user, job=job, enabled=enabled)


@pytest.fixture
def job(db):
    return make_job()
