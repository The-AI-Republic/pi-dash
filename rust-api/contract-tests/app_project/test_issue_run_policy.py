"""Contract tests: "Who can run issues" state gates (NEWPIDASH-261).

Web state moves (PATCH issue, bulk-operation-issues) follow the
project's `issue_run_policy`: default mode allows only the issue's
creator and its current assignees; `project_members` allows any
Member/Admin (guests excluded). Django has no such gate (the setting
is Rust-only; Django stores the bag unvalidated), so the member-denied
cases pin both behaviours via DIFF-48: Django moves (204), Rust
answers 403 `not_issue_runner` without writing.
"""

from _harness.diffs import expect, is_rust

REASON = "not_issue_runner"
MESSAGE = (
    "Only the issue's creator and assignees can do this in this project. "
    "Ask the creator to assign you, or ask a project admin to change the "
    "project's 'Who can run issues' setting."
)


def _states(client, ws, project):
    resp = client.get(f"/api/workspaces/{ws['slug']}/projects/{project['id']}/states/")
    assert resp.status_code == 200, resp.text[:500]
    return resp.json()


def _other_state(states, current_id):
    for state in states:
        if state["id"] != current_id:
            return state["id"]
    raise AssertionError("need two states")


def _make_issue(client, ws, project, name="Policy issue"):
    resp = client.post(
        f"/api/workspaces/{ws['slug']}/projects/{project['id']}/issues/",
        json={"name": name},
    )
    assert resp.status_code in (200, 201), resp.text[:500]
    return resp.json()


def _patch_issue(client, ws, project, issue_id, payload):
    return client.patch(
        f"/api/workspaces/{ws['slug']}/projects/{project['id']}/issues/{issue_id}/",
        json=payload,
    )


def _get_issue(client, ws, project, issue_id):
    resp = client.get(
        f"/api/workspaces/{ws['slug']}/projects/{project['id']}/issues/{issue_id}/"
    )
    assert resp.status_code == 200, resp.text[:500]
    return resp.json()


def _set_policy(client, ws, project, value):
    url = f"/api/workspaces/{ws['slug']}/projects/{project['id']}/"
    resp = client.patch(url, json={"settings": {"issue_run_policy": value}})
    assert resp.status_code == 200, resp.text[:500]
    assert resp.json()["settings"]["issue_run_policy"] == value


def test_creator_moves_state(world):
    client, user, ws, project = world.full_stack()
    issue = _make_issue(client, ws, project)
    target = _other_state(_states(client, ws, project), issue["state_id"])
    resp = _patch_issue(client, ws, project, issue["id"], {"state_id": target})
    assert resp.status_code == 204, resp.text[:500]
    assert _get_issue(client, ws, project, issue["id"])["state_id"] == target


def test_assignee_moves_state(world):
    client, user, ws, project = world.full_stack()
    member_user, member_client = world.add_member(ws, project, ws_role=15, proj_role=15)
    issue = _make_issue(client, ws, project)
    resp = _patch_issue(
        client, ws, project, issue["id"], {"assignee_ids": [str(member_user["id"])]}
    )
    assert resp.status_code == 204, resp.text[:500]
    target = _other_state(_states(client, ws, project), issue["state_id"])
    resp = _patch_issue(member_client, ws, project, issue["id"], {"state_id": target})
    assert resp.status_code == 204, resp.text[:500]
    assert _get_issue(member_client, ws, project, issue["id"])["state_id"] == target


def test_other_member_cannot_move_state(world):
    # DIFF-48: Django moves (204), Rust refuses (403, whole change).
    client, user, ws, project = world.full_stack()
    member_user, member_client = world.add_member(ws, project, ws_role=15, proj_role=15)
    issue = _make_issue(client, ws, project)
    target = _other_state(_states(client, ws, project), issue["state_id"])
    resp = _patch_issue(member_client, ws, project, issue["id"], {"state_id": target})
    assert resp.status_code == expect("DIFF-48", django=204, rust=403), resp.text[:500]
    if is_rust():
        assert resp.json() == {"error": MESSAGE, "reason": REASON}
        assert (
            _get_issue(client, ws, project, issue["id"])["state_id"]
            == issue["state_id"]
        )


def test_removed_assignee_cannot_move_state(world):
    # DIFF-48: un-assigning withdraws the right at once.
    client, user, ws, project = world.full_stack()
    member_user, member_client = world.add_member(ws, project, ws_role=15, proj_role=15)
    issue = _make_issue(client, ws, project)
    resp = _patch_issue(
        client, ws, project, issue["id"], {"assignee_ids": [str(member_user["id"])]}
    )
    assert resp.status_code == 204, resp.text[:500]
    resp = _patch_issue(
        client, ws, project, issue["id"], {"assignee_ids": []}
    )
    assert resp.status_code == 204, resp.text[:500]
    target = _other_state(_states(client, ws, project), issue["state_id"])
    resp = _patch_issue(member_client, ws, project, issue["id"], {"state_id": target})
    assert resp.status_code == expect("DIFF-48", django=204, rust=403), resp.text[:500]
    if is_rust():
        assert resp.json() == {"error": MESSAGE, "reason": REASON}
        assert (
            _get_issue(client, ws, project, issue["id"])["state_id"]
            == issue["state_id"]
        )


def test_other_member_can_edit_non_state(world):
    # Title edits keep today's rules under a default policy.
    client, user, ws, project = world.full_stack()
    member_user, member_client = world.add_member(ws, project, ws_role=15, proj_role=15)
    issue = _make_issue(client, ws, project)
    resp = _patch_issue(member_client, ws, project, issue["id"], {"name": "Renamed"})
    assert resp.status_code == 204, resp.text[:500]
    assert _get_issue(member_client, ws, project, issue["id"])["name"] == "Renamed"


def test_same_state_restatement_is_allowed(world):
    # Re-stating the current state is not a move: no gate on either side.
    client, user, ws, project = world.full_stack()
    member_user, member_client = world.add_member(ws, project, ws_role=15, proj_role=15)
    issue = _make_issue(client, ws, project)
    resp = _patch_issue(
        member_client, ws, project, issue["id"], {"state_id": issue["state_id"]}
    )
    assert resp.status_code == 204, resp.text[:500]


def test_guest_cannot_move_state(world):
    # Guests hold no run rights in either mode; the generic permission
    # gate answers 403 before the run-policy gate (guests already
    # cannot edit issues today).
    client, user, ws, project = world.full_stack()
    guest_user, guest_client = world.add_member(ws, project, ws_role=15, proj_role=5)
    issue = _make_issue(client, ws, project)
    target = _other_state(_states(client, ws, project), issue["state_id"])
    resp = _patch_issue(guest_client, ws, project, issue["id"], {"state_id": target})
    assert resp.status_code == 403, resp.text[:500]


def test_project_members_allows_member(world):
    client, user, ws, project = world.full_stack()
    member_user, member_client = world.add_member(ws, project, ws_role=15, proj_role=15)
    issue = _make_issue(client, ws, project)
    _set_policy(client, ws, project, "project_members")
    target = _other_state(_states(client, ws, project), issue["state_id"])
    resp = _patch_issue(member_client, ws, project, issue["id"], {"state_id": target})
    assert resp.status_code == 204, resp.text[:500]
    assert _get_issue(member_client, ws, project, issue["id"])["state_id"] == target


def test_project_members_still_excludes_guests(world):
    # Guests are excluded in project_members too (generic gate).
    client, user, ws, project = world.full_stack()
    guest_user, guest_client = world.add_member(ws, project, ws_role=15, proj_role=5)
    issue = _make_issue(client, ws, project)
    _set_policy(client, ws, project, "project_members")
    target = _other_state(_states(client, ws, project), issue["state_id"])
    resp = _patch_issue(guest_client, ws, project, issue["id"], {"state_id": target})
    assert resp.status_code == 403, resp.text[:500]


def _bulk(client, ws, project, issue_ids, properties):
    return client.post(
        f"/api/workspaces/{ws['slug']}/projects/{project['id']}/bulk-operation-issues/",
        json={"issue_ids": issue_ids, "properties": properties},
    )


def test_bulk_state_denied_as_a_whole(world):
    # DIFF-48: Django moves (200), Rust refuses (403, nothing written).
    client, user, ws, project = world.full_stack()
    member_user, member_client = world.add_member(ws, project, ws_role=15, proj_role=15)
    issue = _make_issue(client, ws, project)
    target = _other_state(_states(client, ws, project), issue["state_id"])
    resp = _bulk(member_client, ws, project, [issue["id"]], {"state_id": target})
    assert resp.status_code == expect("DIFF-48", django=200, rust=403), resp.text[:500]
    if is_rust():
        assert resp.json() == {"error": MESSAGE, "reason": REASON}
        assert (
            _get_issue(client, ws, project, issue["id"])["state_id"]
            == issue["state_id"]
        )


def test_bulk_state_allowed_for_assignee(world):
    client, user, ws, project = world.full_stack()
    member_user, member_client = world.add_member(ws, project, ws_role=15, proj_role=15)
    issue = _make_issue(client, ws, project)
    resp = _patch_issue(
        client, ws, project, issue["id"], {"assignee_ids": [str(member_user["id"])]}
    )
    assert resp.status_code == 204, resp.text[:500]
    target = _other_state(_states(client, ws, project), issue["state_id"])
    resp = _bulk(member_client, ws, project, [issue["id"]], {"state_id": target})
    assert resp.status_code == 200, resp.text[:500]
    assert _get_issue(member_client, ws, project, issue["id"])["state_id"] == target
