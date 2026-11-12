"""Contract tests: per-project agent ticking policy bounds.

Port of Django's tests/unit/serializers/test_project_ticking_policy.py
(new in 4ed6e16a, PDASHOSS01-220): the bounds live on ProjectSerializer,
so create and PATCH reject invalid values with the same field-error
bodies on both backends.

Stage-8 drift file (PIDASHCONV-1057): asserted behavior that existed only
on origin/main until PIDASHCONV-1055 merged main into rust-dev (2d029396).
"""

import pytest


# Agent ticking policy bounds (PDASHOSS01-220, PIDASHCONV-1057). Port of
# Django's tests/unit/serializers/test_project_ticking_policy.py (new in
# 4ed6e16a): the bounds live on ProjectSerializer, so create and PATCH
# reject invalid values with the same field-error bodies on both backends.

TICKING_INTERVAL_FIELDS = [
    "agent_default_interval_seconds",
    "agent_review_default_interval_seconds",
    "agent_test_default_interval_seconds",
]


@pytest.mark.parametrize("max_ticks", [-1, 1, 10, 500])
def test_partial_update_max_ticks_valid(world, max_ticks):
    client, _, ws, project = world.full_stack()
    resp = client.patch(f"/api/workspaces/{ws['slug']}/projects/{project['id']}/",
                        json={"agent_default_max_ticks": max_ticks})
    assert resp.status_code == 200
    assert resp.json()["agent_default_max_ticks"] == max_ticks


@pytest.mark.parametrize("max_ticks", [0, -2, -10])
def test_partial_update_max_ticks_invalid_400(world, max_ticks):
    client, _, ws, project = world.full_stack()
    resp = client.patch(f"/api/workspaces/{ws['slug']}/projects/{project['id']}/",
                        json={"agent_default_max_ticks": max_ticks})
    assert resp.status_code == 400
    assert resp.json() == {
        "agent_default_max_ticks": ["AGENT_MAX_TICKS_MUST_BE_POSITIVE_OR_INFINITE"]}


@pytest.mark.parametrize("field", TICKING_INTERVAL_FIELDS)
@pytest.mark.parametrize("interval", [60, 1800, 10800, 86400])
def test_partial_update_cadence_valid(world, field, interval):
    client, _, ws, project = world.full_stack()
    resp = client.patch(f"/api/workspaces/{ws['slug']}/projects/{project['id']}/",
                        json={field: interval})
    assert resp.status_code == 200
    assert resp.json()[field] == interval


@pytest.mark.parametrize("field", TICKING_INTERVAL_FIELDS)
@pytest.mark.parametrize("interval", [59, 0, -1])
def test_partial_update_cadence_invalid_400(world, field, interval):
    client, _, ws, project = world.full_stack()
    resp = client.patch(f"/api/workspaces/{ws['slug']}/projects/{project['id']}/",
                        json={field: interval})
    assert resp.status_code == 400
    assert resp.json() == {field: ["AGENT_CADENCE_TOO_SHORT"]}


def test_create_project_ticking_policy_valid(world):
    user = world.user("tickcreator")
    ws = world.workspace(user, "tickcreate")
    world.ws_member(ws, user, 20)
    client = world.client(user)
    resp = client.post(f"/api/workspaces/{ws['slug']}/projects/",
                       json={"name": "Ticked Project",
                             "identifier": "TICKPOL",
                             "agent_default_max_ticks": -1,
                             "agent_default_interval_seconds": 60,
                             "agent_review_default_interval_seconds": 1800,
                             "agent_test_default_interval_seconds": 86400})
    assert resp.status_code == 201
    body = resp.json()
    assert body["agent_default_max_ticks"] == -1
    assert body["agent_default_interval_seconds"] == 60
    assert body["agent_review_default_interval_seconds"] == 1800
    assert body["agent_test_default_interval_seconds"] == 86400


def test_create_project_ticking_policy_invalid_400(world):
    user = world.user("tickbadcreator")
    ws = world.workspace(user, "tickbadcreate")
    world.ws_member(ws, user, 20)
    client = world.client(user)
    resp = client.post(f"/api/workspaces/{ws['slug']}/projects/",
                       json={"name": "Unticked Project",
                             "identifier": "TICKBAD",
                             "agent_default_max_ticks": 0,
                             "agent_default_interval_seconds": 5})
    assert resp.status_code == 400
    assert resp.json() == {
        "agent_default_interval_seconds": ["AGENT_CADENCE_TOO_SHORT"],
        "agent_default_max_ticks": ["AGENT_MAX_TICKS_MUST_BE_POSITIVE_OR_INFINITE"],
    }
