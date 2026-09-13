# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""The data steps of ``db/0163_ticker_one_clock_one_pool``.

The schema steps are Django's; what can go wrong is the data: a prior
Re-tick grant must survive as ``granted``, projects on the old default
must land on the pool, and a ticker already over the (smaller) pool must
not keep advertising a live clock.
"""

from __future__ import annotations

import importlib

import pytest
from crum import impersonate
from django.apps import apps
from django.utils import timezone

from pi_dash.db.models import Issue, Project, State
from pi_dash.db.models.issue_agent_ticker import IssueAgentTicker

migration = importlib.import_module("pi_dash.db.migrations.0163_ticker_one_clock_one_pool")


@pytest.mark.unit
def test_grant_from_override():
    g = migration.grant_from_override
    assert g(None, 24) == 0
    assert g(-1, 24) == 0
    assert g(24, 24) == 0
    assert g(48, 24) == 24  # one Re-tick under the old model
    assert g(6, 3) == 3
    assert g(10, None) == 10


@pytest.fixture
def project(db, workspace, create_user):
    with impersonate(create_user):
        return Project.objects.create(name="Mig", identifier="MIG", workspace=workspace, created_by=create_user)


@pytest.fixture
def issue(workspace, project, create_user):
    with impersonate(create_user):
        todo = State.objects.create(name="Todo", project=project, group="unstarted")
        started = State.objects.create(name="In Progress", project=project, group="started")
        i = Issue.objects.create(name="Task", workspace=workspace, project=project, state=todo, created_by=create_user)
    Issue.all_objects.filter(pk=i.pk).update(state=started)
    i.refresh_from_db()
    return i


@pytest.mark.unit
def test_projects_on_the_old_default_move_to_the_pool(project):
    Project.objects.filter(pk=project.pk).update(agent_default_max_ticks=24)
    tuned = Project.objects.create(
        name="Tuned", identifier="TUN", workspace=project.workspace, created_by=project.created_by,
        agent_default_max_ticks=40,
    )
    migration.move_projects_to_new_pool_default(apps, None)
    project.refresh_from_db()
    tuned.refresh_from_db()
    assert project.agent_default_max_ticks == 10
    assert tuned.agent_default_max_ticks == 40  # explicitly tuned values are left alone


@pytest.mark.unit
def test_rows_over_the_new_pool_are_stamped_cap_hit(issue, project):
    Project.objects.filter(pk=project.pk).update(agent_default_max_ticks=10)
    over = IssueAgentTicker.objects.create(issue=issue, used=15, granted=0, enabled=True, next_run_at=timezone.now())
    migration.stamp_cap_hit_on_rows_over_the_new_pool(apps, None)
    over.refresh_from_db()
    assert over.enabled is False
    assert over.disarm_reason == "cap_hit"


@pytest.mark.unit
def test_rows_saved_by_a_grant_stay_armed(issue, project):
    Project.objects.filter(pk=project.pk).update(agent_default_max_ticks=10)
    ok = IssueAgentTicker.objects.create(issue=issue, used=15, granted=24, enabled=True, next_run_at=timezone.now())
    migration.stamp_cap_hit_on_rows_over_the_new_pool(apps, None)
    ok.refresh_from_db()
    assert ok.enabled is True


@pytest.mark.unit
def test_infinite_pool_is_never_stamped(issue, project):
    Project.objects.filter(pk=project.pk).update(agent_default_max_ticks=-1)
    t = IssueAgentTicker.objects.create(issue=issue, used=500, enabled=True, next_run_at=timezone.now())
    migration.stamp_cap_hit_on_rows_over_the_new_pool(apps, None)
    t.refresh_from_db()
    assert t.enabled is True
