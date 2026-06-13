# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Eligibility — each predicate flips eligibility exactly once, with a
deterministic skip reason and precedence."""

from __future__ import annotations

import pytest

from pi_dash.db.models import SkipReason, WorkspaceMember
from pi_dash.loop import eligibility
from pi_dash.tests.contract.assistant.conftest import configure_llm
from pi_dash.tests.contract.loop.conftest import make_job, make_target, set_pref

pytestmark = pytest.mark.django_db


def _eligible(world):
    """A target whose user has a working LLM config and member role."""
    job = make_job(min_role=15)
    configure_llm(world.member)
    return job, make_target(job, world.ws, world.member)


def test_eligible_target_passes(world, fernet_key):
    job, target = _eligible(world)
    assert eligibility.check(target) is None
    assert list(eligibility.eligible_due_targets().values_list("id", flat=True)) == [target.id]


def test_no_llm_config_skipped(world):
    job = make_job()
    target = make_target(job, world.ws, world.member)  # no configure_llm
    assert eligibility.check(target) == SkipReason.LLM_CONFIG_MISSING
    assert eligibility.eligible_due_targets().count() == 0


def test_job_disabled_pref_skipped(world, fernet_key):
    job, target = _eligible(world)
    set_pref(world.member, job, False)
    assert eligibility.check(target) == SkipReason.USER_DISABLED
    assert eligibility.eligible_due_targets().count() == 0


def test_master_pause_skipped(world, fernet_key):
    job, target = _eligible(world)
    set_pref(world.member, None, False)
    assert eligibility.check(target) == SkipReason.MASTER_PAUSED


def test_below_min_role_skipped(world, fernet_key):
    job = make_job(min_role=20)  # admin-only
    configure_llm(world.member)  # member is role 15 < 20
    target = make_target(job, world.ws, world.member)
    assert eligibility.check(target) == SkipReason.MIN_ROLE


def test_membership_gone_skipped(world, fernet_key):
    job, target = _eligible(world)
    WorkspaceMember.objects.filter(workspace=world.ws, member=world.member).update(is_active=False)
    assert eligibility.check(target) == SkipReason.MEMBERSHIP_GONE


def test_master_pause_precedes_job_off(world, fernet_key):
    job, target = _eligible(world)
    set_pref(world.member, None, False)  # master paused
    set_pref(world.member, job, False)  # also job-off
    # Master pause is checked first.
    assert eligibility.check(target) == SkipReason.MASTER_PAUSED
