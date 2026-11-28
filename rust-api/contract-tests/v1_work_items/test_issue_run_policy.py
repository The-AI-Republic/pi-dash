"""Contract tests: "Who can run issues" gates on the v1 work-items API
(NEWPIDASH-261).

The v1 PATCH (`{"state": <uuid>}`) and the run-ai trigger follow the
project's `issue_run_policy`: default mode allows only the creator and
current assignees; `project_members` allows any Member/Admin. Django
has no such gate, so the member-denied cases pin both behaviours via
DIFF-53 (Django moves/trigger proceeds, Rust answers 403
`not_issue_runner` and writes nothing). Agent self-actions (an explicit
`X-Pi-Dash-Run-Id` on this issue) skip the state gate on Rust.
"""

import httpx
import pytest

from .conftest import issue_url
from _harness.diffs import expect, is_rust

REASON = "not_issue_runner"
MESSAGE = (
    "Only the issue's creator and assignees can do this in this project. "
    "Ask the creator to assign you, or ask a project admin to change the "
    "project's 'Who can run issues' setting."
)


@pytest.fixture(autouse=True)
def _project_states(seeder, world):
    # The v1 world seeds its project through SQL with no states, so the
    # API-created issue has none; the state-move cases need two to move
    # between.
    for name, group in (("Backlog", "backlog"), ("Todo", "unstarted")):
        seeder.create_state(
            world["workspace"]["id"], world["project"]["id"], name=name, group=group
        )


def _states(api, world):
    project_id = world["project"]["id"]
    slug = world["workspace"]["slug"]
    resp = api.get(f"/api/v1/workspaces/{slug}/projects/{project_id}/states/")
    assert resp.status_code == 200, resp.text[:500]
    return resp.json()["results"]


def _other_state(states, current_id):
    for state in states:
        if state["id"] != current_id:
            return state["id"]
    raise AssertionError("need two states")


def _member_client(seeder, world, settings, role=15):
    user = seeder.create_user()
    seeder.create_workspace_member(world["workspace"]["id"], user["id"], role=role)
    seeder.create_project_member(
        world["workspace"]["id"], world["project"]["id"], user["id"], role=role
    )
    token = seeder.create_api_token(user["id"], world["workspace"]["id"])
    client = httpx.Client(
        base_url=settings.base_url,
        timeout=30,
        headers={"X-Api-Key": token["token"]},
    )
    return user, client


def _get_issue(api, world, issue_id):
    resp = api.get(issue_url(world, issue_id))
    assert resp.status_code == 200, resp.text[:500]
    return resp.json()


def _set_policy(api, world, value):
    slug = world["workspace"]["slug"]
    project_id = world["project"]["id"]
    resp = api.patch(
        f"/api/v1/workspaces/{slug}/projects/{project_id}/",
        json={"settings": {"issue_run_policy": value}},
    )
    assert resp.status_code == 200, resp.text[:500]


def test_creator_moves_state(api, world):
    issue = world["issue"]
    target = _other_state(_states(api, world), issue["state"])
    resp = api.patch(issue_url(world, issue["id"]), json={"state": target})
    assert resp.status_code == 200, resp.text[:500]
    assert _get_issue(api, world, issue["id"])["state"] == target


def test_assignee_moves_state(api, world, seeder, settings):
    member, client = _member_client(seeder, world, settings)
    try:
        issue = world["issue"]
        resp = api.patch(
            issue_url(world, issue["id"]), json={"assignees": [member["id"]]}
        )
        assert resp.status_code == 200, resp.text[:500]
        # The assignee write gives the stateless issue a state, so pick the
        # target from the persisted state: moving to the same state is a
        # no-op that skips the run-policy gate and would prove nothing.
        before = _get_issue(api, world, issue["id"])["state"]
        target = _other_state(_states(api, world), before)
        assert target != before
        resp = client.patch(issue_url(world, issue["id"]), json={"state": target})
        assert resp.status_code == 200, resp.text[:500]
        assert _get_issue(client, world, issue["id"])["state"] == target
    finally:
        client.close()


def test_other_member_cannot_move_state(api, world, seeder, settings):
    # DIFF-53: Django moves (200), Rust refuses (403, whole change).
    member, client = _member_client(seeder, world, settings)
    try:
        issue = world["issue"]
        target = _other_state(_states(api, world), issue["state"])
        resp = client.patch(issue_url(world, issue["id"]), json={"state": target})
        assert resp.status_code == expect("DIFF-53", django=200, rust=403), (
            resp.text[:500]
        )
        if is_rust():
            assert resp.json() == {"error": MESSAGE, "reason": REASON}
            assert _get_issue(api, world, issue["id"])["state"] == issue["state"]
    finally:
        client.close()


def test_removed_assignee_cannot_move_state(api, world, seeder, settings):
    member, client = _member_client(seeder, world, settings)
    try:
        issue = world["issue"]
        resp = api.patch(
            issue_url(world, issue["id"]), json={"assignees": [member["id"]]}
        )
        assert resp.status_code == 200, resp.text[:500]
        resp = api.patch(issue_url(world, issue["id"]), json={"assignees": []})
        assert resp.status_code == 200, resp.text[:500]
        # The assignee writes give the stateless issue a state on both
        # backends, so compare with the state just before the move.
        before = _get_issue(api, world, issue["id"])["state"]
        target = _other_state(_states(api, world), before)
        resp = client.patch(issue_url(world, issue["id"]), json={"state": target})
        assert resp.status_code == expect("DIFF-53", django=200, rust=403), (
            resp.text[:500]
        )
        if is_rust():
            assert resp.json() == {"error": MESSAGE, "reason": REASON}
            assert _get_issue(api, world, issue["id"])["state"] == before
    finally:
        client.close()


def test_other_member_can_edit_non_state(api, world, seeder, settings):
    member, client = _member_client(seeder, world, settings)
    try:
        issue = world["issue"]
        resp = client.patch(
            issue_url(world, issue["id"]), json={"name": "Renamed"}
        )
        assert resp.status_code == 200, resp.text[:500]
        assert _get_issue(client, world, issue["id"])["name"] == "Renamed"
    finally:
        client.close()


def test_guest_cannot_move_state(api, world, guest_client):
    issue = world["issue"]
    target = _other_state(_states(api, world), issue["state"])
    resp = guest_client.patch(issue_url(world, issue["id"]), json={"state": target})
    # Guests hold no write rights in either mode: the generic permission
    # gate answers before the run-policy gate, on both backends (as on the
    # app route, app_project/test_issue_run_policy.py).
    assert resp.status_code == 403, resp.text[:500]
    assert resp.json() == {
        "detail": "You do not have permission to perform this action."
    }


def test_project_members_allows_member(api, world, seeder, settings):
    member, client = _member_client(seeder, world, settings)
    try:
        issue = world["issue"]
        _set_policy(api, world, "project_members")
        target = _other_state(_states(api, world), issue["state"])
        resp = client.patch(issue_url(world, issue["id"]), json={"state": target})
        assert resp.status_code == 200, resp.text[:500]
        assert _get_issue(client, world, issue["id"])["state"] == target
    finally:
        client.close()


def test_run_ai_refused_for_other_member(api, world, seeder, settings):
    # DIFF-53: the run-ai trigger is gated before any run is created.
    # Django proceeds to dispatch (any nonno-runner reason); Rust answers
    # 403 `not_issue_runner` and creates nothing.
    member, client = _member_client(seeder, world, settings)
    try:
        issue = world["issue"]
        slug = world["workspace"]["slug"]
        project_id = world["project"]["id"]
        run_url = (
            f"/api/v1/workspaces/{slug}/projects/{project_id}"
            f"/work-items/{issue['id']}/run-ai/"
        )
        resp = client.post(run_url, json={})
        if is_rust():
            assert resp.status_code == 403, resp.text[:500]
            assert resp.json() == {"error": MESSAGE, "reason": REASON}
        else:
            # DIFF-53: Django has no such refusal; it proceeds to
            # dispatch (201 created or a dispatch-side 409/400).
            assert resp.status_code in (201, 400, 409), resp.text[:500]
    finally:
        client.close()
