"""Contract: public intake endpoints.

Covers ``space/urls/intake.py``: the intake-issue list/create/retrieve/patch/
delete quartet (plus the ``inbox-issues`` alias, which shares the view) and
``workspaces/<slug>/project-boards/``.

Every action here requires a session (``BaseViewSet`` default): the
unauthenticated cases are part of the suite's denied-permission tripwire.
"""

import pytest

pytestmark = pytest.mark.contract

BASE = "/api/public"


def _list_url(world):
    return f"{BASE}/anchor/{world['anchor']}/intakes/{world['intake']['id']}/intake-issues/"


def _detail_url(world, pk):
    return f"{_list_url(world)}{pk}/"


def _inbox_url(world):
    return f"{BASE}/anchor/{world['anchor']}/intakes/{world['intake']['id']}/inbox-issues/"


def test_list_requires_auth(anon_client, world):
    assert anon_client.get(_list_url(world)).status_code in (401, 403)


def test_list_shape(user_client, world, seeder, db):
    issue = seeder.create_issue(world["workspace"]["id"], world["project"]["id"], world["state"]["id"])
    seeder.create_intake_issue(
        world["workspace"]["id"], world["project"]["id"], world["intake"]["id"], issue["id"]
    )
    response = user_client.get(_list_url(world))
    assert response.status_code == 200
    rows = response.json()
    assert isinstance(rows, list) and len(rows) == 1
    row = rows[0]
    for key in ("id", "name", "priority", "state_detail", "project_detail", "sub_issues_count", "issue_intake"):
        assert key in row, f"intake row missing {key}"
    assert row["project_detail"]["id"] == world["project"]["id"]


def test_inbox_alias_matches_list(user_client, world):
    listed = user_client.get(_list_url(world))
    inbox = user_client.get(_inbox_url(world))
    assert inbox.status_code == 200
    assert inbox.json() == listed.json()


def test_create_shape(user_client, world):
    payload = {"issue": {"name": "Intake idea", "priority": "low", "description_html": "<p>why</p>"}}
    response = user_client.post(_list_url(world), json=payload)
    assert response.status_code == 200
    body = response.json()
    assert body["name"] == "Intake idea"
    assert body["state_detail"] is not None


def test_create_requires_name(user_client, world):
    response = user_client.post(_list_url(world), json={"issue": {"priority": "low"}})
    assert response.status_code == 400
    assert response.json() == {"error": "Name is required"}


def test_create_rejects_bad_priority(user_client, world):
    response = user_client.post(_list_url(world), json={"issue": {"name": "x", "priority": "eventual"}})
    assert response.status_code == 400
    assert response.json() == {"error": "Invalid priority"}


def test_create_requires_auth(anon_client, world):
    response = anon_client.post(_list_url(world), json={"issue": {"name": "anon idea"}})
    assert response.status_code in (401, 403)


def test_create_without_intake_is_400(user_client, world, seeder):
    # One live project board per project (unique constraint on
    # deploy_boards(entity_name, entity_identifier) where deleted_at is null),
    # so the intake-less board lives on a fresh project, not world's.
    project = seeder.create_project(world["workspace"]["id"])
    board = seeder.create_board(world["workspace"]["id"], project["id"])
    url = f"{BASE}/anchor/{board['anchor']}/intakes/{world['intake']['id']}/intake-issues/"
    response = user_client.post(url, json={"issue": {"name": "no intake"}})
    assert response.status_code == 400
    assert response.json() == {"error": "Intake is not enabled for this Project Board"}


def test_retrieve_patch_delete_by_creator(user_client, world, seeder, db):
    issue = seeder.create_issue(world["workspace"]["id"], world["project"]["id"], world["state"]["id"])
    db.execute("UPDATE issues SET created_by_id=%s WHERE id=%s", (world["owner"]["id"], issue["id"]))
    bridge = seeder.create_intake_issue(
        world["workspace"]["id"], world["project"]["id"], world["intake"]["id"], issue["id"]
    )
    db.execute(
        "UPDATE intake_issues SET created_by_id=%s WHERE id=%s", (world["owner"]["id"], bridge["id"])
    )

    got = user_client.get(_detail_url(world, bridge["id"]))
    assert got.status_code == 200
    assert got.json()["id"] == issue["id"]

    patched = user_client.patch(
        _detail_url(world, bridge["id"]),
        json={"issue": {"name": "Renamed idea", "description_html": "<p>new</p>"}},
    )
    assert patched.status_code == 200

    deleted = user_client.delete(_detail_url(world, bridge["id"]))
    assert deleted.status_code == 204


def test_patch_by_other_user_is_rejected(other_client, world, seeder, db):
    issue = seeder.create_issue(world["workspace"]["id"], world["project"]["id"], world["state"]["id"])
    bridge = seeder.create_intake_issue(
        world["workspace"]["id"], world["project"]["id"], world["intake"]["id"], issue["id"]
    )
    db.execute(
        "UPDATE intake_issues SET created_by_id=%s WHERE id=%s", (world["owner"]["id"], bridge["id"])
    )
    response = other_client.patch(
        _detail_url(world, bridge["id"]), json={"issue": {"name": "hijack"}}
    )
    assert response.status_code == 400
    assert response.json() == {"error": "You cannot edit intake issues"}


def test_delete_by_other_user_is_rejected(other_client, world, seeder, db):
    issue = seeder.create_issue(world["workspace"]["id"], world["project"]["id"], world["state"]["id"])
    bridge = seeder.create_intake_issue(
        world["workspace"]["id"], world["project"]["id"], world["intake"]["id"], issue["id"]
    )
    db.execute(
        "UPDATE intake_issues SET created_by_id=%s WHERE id=%s", (world["owner"]["id"], bridge["id"])
    )
    response = other_client.delete(_detail_url(world, bridge["id"]))
    assert response.status_code == 400
    assert response.json() == {"error": "You cannot delete intake issue"}


def test_cross_board_detail_is_not_found(user_client, world, seeder):
    # The detail lookup is scoped to the anchor's workspace+project+intake:
    # another board's bridge id must not resolve here.
    owner2 = seeder.create_user()
    workspace2 = seeder.create_workspace(owner2["id"])
    project2 = seeder.create_project(workspace2["id"])
    intake2 = seeder.create_intake(workspace2["id"], project2["id"])
    board2 = seeder.create_board(workspace2["id"], project2["id"], intake_id=intake2["id"])
    state2 = seeder.create_state(workspace2["id"], project2["id"])
    issue2 = seeder.create_issue(workspace2["id"], project2["id"], state2["id"])
    bridge2 = seeder.create_intake_issue(workspace2["id"], project2["id"], intake2["id"], issue2["id"])
    assert board2["anchor"] != world["anchor"]

    response = user_client.get(_detail_url(world, bridge2["id"]))
    assert response.status_code == 404


def test_workspace_project_boards_broken_contract(anon_client, world):
    # NOTE (ported bug): ``WorkspaceProjectDeployBoardEndpoint.get`` takes an
    # ``anchor`` kwarg the ``workspaces/<slug>/project-boards/`` route never
    # provides (and then indexes ``.values_list`` without calling it), so this
    # endpoint 500s on Django today. Pinned as 5xx so the Rust port reproduces
    # the failure instead of silently "fixing" it; the real fix is a follow-up.
    response = anon_client.get(f"{BASE}/workspaces/{world['workspace']['slug']}/project-boards/")
    assert response.status_code >= 500
