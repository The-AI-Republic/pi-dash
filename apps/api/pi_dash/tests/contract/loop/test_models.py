# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Loop model constraints — conditional uniques survive soft-delete."""

from __future__ import annotations

import pytest
from django.db import IntegrityError, transaction

from pi_dash.db.models import LoopUserPreference
from pi_dash.tests.contract.loop.conftest import make_job, make_target, set_pref

pytestmark = pytest.mark.django_db


def test_job_slug_unique_when_active():
    make_job(slug="dup")
    with pytest.raises(IntegrityError):
        with transaction.atomic():
            make_job(slug="dup")


def test_job_slug_reusable_after_soft_delete():
    j = make_job(slug="reuse")
    j.delete()  # soft delete sets deleted_at
    # A new active row with the same slug is allowed (tombstone excluded).
    again = make_job(slug="reuse")
    assert again.pk != j.pk


def test_target_edge_unique_when_active(world):
    j = make_job()
    make_target(j, world.ws, world.member)
    with pytest.raises(IntegrityError):
        with transaction.atomic():
            make_target(j, world.ws, world.member)


def test_master_pref_unique_per_user(world):
    LoopUserPreference.objects.create(user=world.member, job=None, enabled=False)
    with pytest.raises(IntegrityError):
        with transaction.atomic():
            LoopUserPreference.objects.create(user=world.member, job=None, enabled=True)


def test_job_pref_unique_per_user_job(world):
    j = make_job()
    set_pref(world.member, j, False)
    with pytest.raises(IntegrityError):
        with transaction.atomic():
            set_pref(world.member, j, True)
