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
    for key in ("id", "identifier", "name", "icon_prop", "emoji", "description"):
        assert key in body, f"meta missing {key}: {sorted(body)}"
    # PDASHOSS01-237: the cover image feature is gone.
    assert "cover_image" not in body
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


def test_issues_group_keys_exclude_soft_deleted_and_triage(anon_client, world, seeder, db):
    workspace_id = world["workspace"]["id"]
    project_id = world["project"]["id"]

    deleted_state = seeder.create_state(workspace_id, project_id, name="Deleted state")
    triage_state = seeder.create_state(workspace_id, project_id, name="Triage", group="triage")
    deleted_cycle = seeder.create_cycle(workspace_id, project_id, world["owner"]["id"], name="Deleted cycle")

    db.execute("UPDATE states SET deleted_at = now() WHERE id = %s", (deleted_state["id"],))
    db.execute("UPDATE cycles SET deleted_at = now() WHERE id = %s", (deleted_cycle["id"],))

    states = anon_client.get(f"{BASE}/anchor/{world['anchor']}/issues/?group_by=state_id")
    assert states.status_code == 200
    state_keys = set(states.json()["results"])
    assert world["state"]["id"] in state_keys
    assert deleted_state["id"] not in state_keys
    assert triage_state["id"] not in state_keys

    cycles = anon_client.get(f"{BASE}/anchor/{world['anchor']}/issues/?group_by=cycle_id")
    assert cycles.status_code == 200
    cycle_keys = set(cycles.json()["results"])
    assert world["cycle"]["id"] in cycle_keys
    assert deleted_cycle["id"] not in cycle_keys


PROJECT_DETAILS_KEYS = ["id", "identifier", "name", "logo_props", "description", "is_default"]


def test_settings_project_details_have_no_cover_keys(anon_client, world):
    # PDASHOSS01-237: app ProjectLiteSerializer dropped cover_image and
    # cover_image_url; the board nest renders the remaining six keys in order.
    settings = anon_client.get(f"{BASE}/anchor/{world['anchor']}/settings/")
    assert settings.status_code == 200
    assert list(settings.json()["project_details"]) == PROJECT_DETAILS_KEYS

    slug = world["workspace"]["slug"]
    project_id = world["project"]["id"]
    anchor = anon_client.get(f"{BASE}/workspaces/{slug}/projects/{project_id}/anchor/")
    assert anchor.status_code == 200
    assert list(anchor.json()["project_details"]) == PROJECT_DETAILS_KEYS


def test_settings_soft_deleted_logo_asset_still_200(anon_client, world, seeder, db):
    logo = seeder.create_asset(
        world["workspace"]["id"], world["project"]["id"], world["owner"]["id"],
        entity_type="WORKSPACE_LOGO",
    )
    db.execute(
        "UPDATE workspaces SET logo_asset_id = %s WHERE id = %s",
        (logo["id"], world["workspace"]["id"]),
    )
    db.execute("UPDATE file_assets SET deleted_at = now() WHERE id = %s", (logo["id"],))

    response = anon_client.get(f"{BASE}/anchor/{world['anchor']}/settings/")
    assert response.status_code == 200
    assert response.json()["workspace_detail"]["logo_url"] == f"/api/assets/v2/static/{logo['id']}/"


def test_settings_soft_deleted_project_and_workspace_still_200(anon_client, world, db):
    # The board row itself is not soft-deleted; its nested project/workspace
    # rows are followed through unscoped forward FKs (PIDASHCONV-1262).
    db.execute("UPDATE projects SET deleted_at = now() WHERE id = %s", (world["project"]["id"],))
    response = anon_client.get(f"{BASE}/anchor/{world['anchor']}/settings/")
    assert response.status_code == 200
    assert response.json()["project_details"]["id"] == world["project"]["id"]
    db.execute("UPDATE projects SET deleted_at = NULL WHERE id = %s", (world["project"]["id"],))

    db.execute(
        "UPDATE workspaces SET deleted_at = now() WHERE id = %s", (world["workspace"]["id"],)
    )
    response = anon_client.get(f"{BASE}/anchor/{world['anchor']}/settings/")
    assert response.status_code == 200
    assert response.json()["workspace_detail"]["id"] == world["workspace"]["id"]
    db.execute("UPDATE workspaces SET deleted_at = NULL WHERE id = %s", (world["workspace"]["id"],))
