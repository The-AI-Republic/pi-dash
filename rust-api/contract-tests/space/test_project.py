"""Contract: project-level public endpoints.

Covers ``space/urls/project.py`` (9 paths) plus ``meta.py``:
``anchor/<anchor>/meta|settings|issues|cycles|modules|states|labels|members/``
and ``workspaces/<slug>/projects/<project_id>/anchor/``.

All of these are ``AllowAny``: the permission story here is tenant isolation
(unknown/cross anchors must not leak), while the denied-permission tripwire
lives in the intake/asset/comment-write suites.
"""

import pytest

pytestmark = pytest.mark.contract

BASE = "/api/public"


def test_meta_shape(anon_client, world):
    response = anon_client.get(f"{BASE}/anchor/{world['anchor']}/meta/")
    assert response.status_code == 200
    body = response.json()
    # space/serializer/project.py ProjectLiteSerializer.
    for key in ("id", "identifier", "name", "cover_image", "icon_prop", "emoji", "description"):
        assert key in body, f"meta missing {key}: {sorted(body)}"
    assert body["name"].startswith("Contract Project")


def test_meta_unknown_anchor_is_404(anon_client):
    response = anon_client.get(f"{BASE}/anchor/does-not-exist-123/meta/")
    assert response.status_code == 404
    assert response.json() == {"error": "Project is not published"}


def test_settings_shape(anon_client, world):
    response = anon_client.get(f"{BASE}/anchor/{world['anchor']}/settings/")
    assert response.status_code == 200
    body = response.json()
    assert body["anchor"] == world["anchor"]
    assert body["entity_name"] == "project"
    assert body["is_comments_enabled"] is True
    assert body["project_details"]["name"].startswith("Contract Project")
    assert "workspace_detail" in body


def test_anchor_endpoint_shape(anon_client, world):
    slug = world["workspace"]["slug"]
    project_id = world["project"]["id"]
    response = anon_client.get(f"{BASE}/workspaces/{slug}/projects/{project_id}/anchor/")
    assert response.status_code == 200
    assert response.json()["anchor"] == world["anchor"]


def test_cycles_shape(anon_client, world):
    response = anon_client.get(f"{BASE}/anchor/{world['anchor']}/cycles/")
    assert response.status_code == 200
    rows = response.json()
    assert isinstance(rows, list)
    assert {row["name"] for row in rows} == {"Contract cycle"}
    assert all(set(row) == {"id", "name"} for row in rows)


def test_modules_shape(anon_client, world):
    response = anon_client.get(f"{BASE}/anchor/{world['anchor']}/modules/")
    assert response.status_code == 200
    rows = response.json()
    assert {row["name"] for row in rows} == {"Contract module"}
    assert all(set(row) == {"id", "name"} for row in rows)


def test_states_shape_excludes_triage(anon_client, world, seeder):
    seeder.create_state(world["workspace"]["id"], world["project"]["id"], name="Triage", group="triage")
    response = anon_client.get(f"{BASE}/anchor/{world['anchor']}/states/")
    assert response.status_code == 200
    rows = response.json()
    names = {row["name"] for row in rows}
    assert "Backlog" in names
    assert "Triage" not in names
    assert all(set(row) == {"name", "group", "color", "id", "sequence"} for row in rows)


def test_labels_shape(anon_client, world):
    response = anon_client.get(f"{BASE}/anchor/{world['anchor']}/labels/")
    assert response.status_code == 200
    rows = response.json()
    assert {row["name"] for row in rows} == {"bug"}
    assert all(set(row) == {"id", "name", "color", "parent"} for row in rows)


def test_members_shape(anon_client, world, seeder, db):
    db.execute(
        """INSERT INTO project_members
           (id, workspace_id, project_id, member_id, role, view_props, default_props,
            preferences, sort_order, is_active, created_at, updated_at)
           VALUES (gen_random_uuid(),%s,%s,%s,20,'{}','{}','{}',65535,true,now(),now())""",
        (world["workspace"]["id"], world["project"]["id"], world["owner"]["id"]),
    )
    response = anon_client.get(f"{BASE}/anchor/{world['anchor']}/members/")
    assert response.status_code == 200
    rows = response.json()
    assert len(rows) == 1
    assert set(rows[0]) == {"id", "member", "member__display_name", "member__avatar"}


def test_members_unknown_anchor_is_404(anon_client):
    response = anon_client.get(f"{BASE}/anchor/does-not-exist-123/members/")
    assert response.status_code == 404
    assert response.json() == {"error": "Invalid anchor"}


def test_issues_list_envelope_and_isolation(anon_client, world, seeder):
    response = anon_client.get(f"{BASE}/anchor/{world['anchor']}/issues/")
    assert response.status_code == 200
    body = response.json()
    # pi_dash/utils/paginator.py BasePaginator.paginate envelope.
    for key in (
        "grouped_by",
        "total_count",
        "next_cursor",
        "prev_cursor",
        "count",
        "total_results",
        "results",
    ):
        assert key in body, f"issues list missing {key}"
    assert body["total_count"] >= 1

    # Tenant isolation: a second board's issues never appear under this anchor.
    owner2 = seeder.create_user()
    workspace2 = seeder.create_workspace(owner2["id"])
    project2 = seeder.create_project(workspace2["id"])
    board2 = seeder.create_board(workspace2["id"], project2["id"])
    state2 = seeder.create_state(workspace2["id"], project2["id"])
    seeder.create_issue(workspace2["id"], project2["id"], state2["id"], name="Other tenant issue")
    again = anon_client.get(f"{BASE}/anchor/{world['anchor']}/issues/")
    names = [row["name"] for row in again.json()["results"]]
    assert "Other tenant issue" not in names
    other = anon_client.get(f"{BASE}/anchor/{board2['anchor']}/issues/")
    assert "Other tenant issue" in [row["name"] for row in other.json()["results"]]


def test_issues_list_unknown_anchor_is_404(anon_client):
    response = anon_client.get(f"{BASE}/anchor/does-not-exist-123/issues/")
    assert response.status_code == 404
    assert response.json() == {"error": "Project is not published"}
