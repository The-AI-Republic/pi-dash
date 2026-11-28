"""``Issue.work_type`` on the api-v1 work-item surface (PDASHOSS01-234).

Mirrors Django's ``tests/unit/api/test_issue_work_type.py``:

- the key renders in the issue JSON right after ``agent_executor``;
- create / PATCH set it, ``""`` clears it back to the inherit state (NULL),
  an unknown key 400s with the list-wrapped registry message;
- an agent-attributed PATCH (explicit ``X-Pi-Dash-Run-Id`` naming an active
  run on the issue) may not *change* it while the issue sits in a ticking
  stage (started / review / test) — 403 — while restating the current value
  is fine and a headerless (human) PATCH succeeds even with the run active.
"""

from __future__ import annotations

import pytest

from .conftest import issue_url

UNKNOWN_BODY = {"work_type": ["unknown work type; expected one of ['general', 'software']"]}
LOCKED_BODY = {
    "error": "work_type is locked while the issue is being worked; only a human can change it"
}


def _stored(db, issue_id):
    return db.fetchone("SELECT work_type FROM issues WHERE id = %s", (issue_id,))["work_type"]


def _move_to(world, seeder, db, *, name, group):
    state = seeder.create_state(
        world["workspace"]["id"], world["project"]["id"], name=name, group=group
    )
    db.execute("UPDATE issues SET state_id = %s WHERE id = %s", (state["id"], world["issue"]["id"]))
    return state


def _active_run(world, seeder):
    pod = seeder.create_pod(world["workspace"]["id"], world["project"]["id"])
    return seeder.create_agent_run(
        world["workspace"]["id"], world["owner"]["id"], pod["id"], world["issue"]["id"]
    )


def test_work_type_renders_after_agent_executor(api, world):
    keys = list(world["issue"])
    assert keys[keys.index("agent_executor") + 1] == "work_type"
    assert world["issue"]["work_type"] is None
    detail = api.get(issue_url(world, world["issue"]["id"]))
    assert detail.status_code == 200, detail.text
    keys = list(detail.json())
    assert keys[keys.index("agent_executor") + 1] == "work_type"
    assert detail.json()["work_type"] is None


def test_create_with_work_type(api, world, db):
    created = api.post(issue_url(world), json={"name": "Typed", "work_type": "general"})
    assert created.status_code == 201, created.text
    assert created.json()["work_type"] == "general"
    assert _stored(db, created.json()["id"]) == "general"
    blank = api.post(issue_url(world), json={"name": "Blank typed", "work_type": ""})
    assert blank.status_code == 201, blank.text
    assert blank.json()["work_type"] is None
    assert _stored(db, blank.json()["id"]) is None


def test_create_rejects_unknown_work_type(api, world):
    response = api.post(issue_url(world), json={"name": "Bad", "work_type": "carpentry"})
    assert response.status_code == 400
    assert response.json() == UNKNOWN_BODY


def test_human_patch_sets_and_clears_work_type(api, world, db):
    url = issue_url(world, world["issue"]["id"])
    response = api.patch(url, json={"work_type": "general"})
    assert response.status_code == 200, response.text
    assert response.json()["work_type"] == "general"
    assert _stored(db, world["issue"]["id"]) == "general"
    # "" normalizes back to the inherit state (NULL), not an empty string.
    response = api.patch(url, json={"work_type": ""})
    assert response.status_code == 200, response.text
    assert response.json()["work_type"] is None
    assert _stored(db, world["issue"]["id"]) is None
    response = api.patch(url, json={"work_type": "software"})
    assert response.status_code == 200, response.text
    response = api.patch(url, json={"work_type": None})
    assert response.status_code == 200, response.text
    assert _stored(db, world["issue"]["id"]) is None


def test_patch_rejects_unknown_work_type(api, world, db):
    response = api.patch(issue_url(world, world["issue"]["id"]), json={"work_type": "carpentry"})
    assert response.status_code == 400
    assert response.json() == UNKNOWN_BODY
    assert _stored(db, world["issue"]["id"]) is None


def test_patch_rejects_overlong_work_type(api, world):
    response = api.patch(issue_url(world, world["issue"]["id"]), json={"work_type": "x" * 33})
    assert response.status_code == 400
    assert response.json() == {"work_type": ["Ensure this field has no more than 32 characters."]}


@pytest.mark.parametrize(
    "name,group",
    [("In Progress", "started"), ("In Review", "review"), ("In Test", "test")],
)
def test_agent_cannot_change_work_type_while_worked(api, world, seeder, db, name, group):
    _move_to(world, seeder, db, name=name, group=group)
    run = _active_run(world, seeder)
    url = issue_url(world, world["issue"]["id"])
    headers = {"X-Pi-Dash-Run-Id": run["id"]}
    response = api.patch(url, json={"work_type": "general"}, headers=headers)
    assert response.status_code == 403
    assert response.json() == LOCKED_BODY
    assert _stored(db, world["issue"]["id"]) is None
    # Restating the current value is a no-op, not an error.
    response = api.patch(url, json={"work_type": None, "priority": "high"}, headers=headers)
    assert response.status_code == 200, response.text
    assert response.json()["priority"] == "high"
    response = api.patch(url, json={"work_type": ""}, headers=headers)
    assert response.status_code == 200, response.text
    # A human (no header) may change it even while the run is active.
    response = api.patch(url, json={"work_type": "general"})
    assert response.status_code == 200, response.text
    assert _stored(db, world["issue"]["id"]) == "general"
    # And the agent restating that value is still fine; changing it is not.
    response = api.patch(url, json={"work_type": "general"}, headers=headers)
    assert response.status_code == 200, response.text
    response = api.patch(url, json={"work_type": "software"}, headers=headers)
    assert response.status_code == 403
    assert response.json() == LOCKED_BODY
    response = api.patch(url, json={"work_type": None}, headers=headers)
    assert response.status_code == 403
    assert _stored(db, world["issue"]["id"]) == "general"


def test_lock_runs_before_field_validation(api, world, seeder, db):
    # The lock check precedes the serializer: an unknown key from the agent
    # on a worked issue answers the lock's 403, not the validator's 400.
    _move_to(world, seeder, db, name="In Progress", group="started")
    run = _active_run(world, seeder)
    response = api.patch(
        issue_url(world, world["issue"]["id"]),
        json={"work_type": "carpentry"},
        headers={"X-Pi-Dash-Run-Id": run["id"]},
    )
    assert response.status_code == 403
    assert response.json() == LOCKED_BODY


def test_agent_may_change_work_type_outside_ticking_stages(api, world, seeder, db):
    # The issue sits in its default (unstarted/backlog) state: no lock.
    run = _active_run(world, seeder)
    response = api.patch(
        issue_url(world, world["issue"]["id"]),
        json={"work_type": "general"},
        headers={"X-Pi-Dash-Run-Id": run["id"]},
    )
    assert response.status_code == 200, response.text
    assert _stored(db, world["issue"]["id"]) == "general"


def test_headerless_patch_succeeds_despite_active_run(api, world, seeder, db):
    _move_to(world, seeder, db, name="In Progress", group="started")
    _active_run(world, seeder)
    response = api.patch(issue_url(world, world["issue"]["id"]), json={"work_type": "general"})
    assert response.status_code == 200, response.text
    assert _stored(db, world["issue"]["id"]) == "general"
