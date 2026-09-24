"""Contract tests: estimate URL module (5 routes)."""

from conftest import assert_keys

ESTIMATE_KEYS = [
    "created_at", "created_by", "deleted_at", "description", "id",
    "last_used", "name", "points", "project", "type", "updated_at",
    "updated_by", "workspace",
]

POINT_KEYS = [
    "created_at", "created_by", "deleted_at", "description", "estimate",
    "id", "key", "project", "updated_at", "updated_by", "value", "workspace",
]


def _make_estimate(client, ws, project, name="Contract Est", points=None):
    if points is None:
        points = [{"key": 1, "value": "1", "description": "one"}]
    resp = client.post(
        f"/api/workspaces/{ws['slug']}/projects/{project['id']}/estimates/",
        json={"estimate": {"name": name, "type": "points"},
              "estimate_points": points})
    assert resp.status_code == 200, resp.text[:500]
    return resp.json()


def test_project_estimates_empty(world):
    client, _, ws, project = world.full_stack()
    resp = client.get(
        f"/api/workspaces/{ws['slug']}/projects/{project['id']}/project-estimates/")
    assert resp.status_code == 200
    assert resp.json() == []


def test_bulk_create_shape(world, db):
    client, _, ws, project = world.full_stack()
    created = _make_estimate(client, ws, project, points=[
        {"key": 1, "value": "1", "description": "one"},
        {"key": 2, "value": "2", "description": "two"},
    ])
    assert_keys(created, ESTIMATE_KEYS, "estimate-create")
    assert len(created["points"]) == 2
    for point in created["points"]:
        assert_keys(point, POINT_KEYS, "estimate-create-point")
    with db.cursor() as cur:
        cur.execute("SELECT COUNT(*) AS n FROM estimate_points WHERE estimate_id=%s",
                    (created["id"],))
        assert cur.fetchone()["n"] == 2


def test_bulk_create_empty_points_200(world):
    # No points is valid: the estimate row is still created with points=[].
    client, _, ws, project = world.full_stack()
    resp = client.post(
        f"/api/workspaces/{ws['slug']}/projects/{project['id']}/estimates/",
        json={"estimate": {"name": "Empty Est", "type": "points"}, "estimate_points": []})
    assert resp.status_code == 200
    assert_keys(resp.json(), ESTIMATE_KEYS, "estimate-create-empty")
    assert resp.json()["points"] == []


def test_bulk_create_long_value_400(world):
    client, _, ws, project = world.full_stack()
    resp = client.post(
        f"/api/workspaces/{ws['slug']}/projects/{project['id']}/estimates/",
        json={"estimate": {"name": "Bad", "type": "points"},
              "estimate_points": [{"key": 1, "value": "x" * 21, "description": ""}]})
    assert resp.status_code == 400


def test_bulk_list_shape(world):
    client, _, ws, project = world.full_stack()
    created = _make_estimate(client, ws, project)
    resp = client.get(f"/api/workspaces/{ws['slug']}/projects/{project['id']}/estimates/")
    assert resp.status_code == 200
    body = resp.json()
    assert isinstance(body, list) and len(body) == 1
    assert_keys(body[0], ESTIMATE_KEYS, "estimate-list")
    assert body[0]["id"] == created["id"]


def test_bulk_retrieve_shape(world):
    client, _, ws, project = world.full_stack()
    created = _make_estimate(client, ws, project)
    resp = client.get(
        f"/api/workspaces/{ws['slug']}/projects/{project['id']}/estimates/{created['id']}/")
    assert resp.status_code == 200
    assert_keys(resp.json(), ESTIMATE_KEYS, "estimate-retrieve")


def test_bulk_retrieve_missing_404(world):
    client, _, ws, project = world.full_stack()
    resp = client.get(
        f"/api/workspaces/{ws['slug']}/projects/{project['id']}/estimates/"
        "00000000-0000-0000-0000-000000000000/")
    assert resp.status_code == 404
    assert resp.json() == {"error": "The required object does not exist."}


def test_bulk_partial_update(world):
    client, _, ws, project = world.full_stack()
    created = _make_estimate(client, ws, project, points=[
        {"key": 1, "value": "1", "description": "one"},
        {"key": 2, "value": "2", "description": "two"},
    ])
    point_id = created["points"][0]["id"]
    resp = client.patch(
        f"/api/workspaces/{ws['slug']}/projects/{project['id']}/estimates/{created['id']}/",
        json={"estimate": {"name": "Renamed Est"},
              "estimate_points": [{"id": point_id, "key": 5, "value": "5"}]})
    assert resp.status_code == 200
    assert_keys(resp.json(), ESTIMATE_KEYS, "estimate-partial")
    assert resp.json()["name"] == "Renamed Est"
    updated = [p for p in resp.json()["points"] if p["id"] == point_id][0]
    assert updated["key"] == 5 and updated["value"] == "5"


def test_bulk_partial_update_no_points_400(world):
    client, _, ws, project = world.full_stack()
    created = _make_estimate(client, ws, project)
    resp = client.patch(
        f"/api/workspaces/{ws['slug']}/projects/{project['id']}/estimates/{created['id']}/",
        json={"estimate": {"name": "Nope"}})
    assert resp.status_code == 400
    assert resp.json() == {"error": "Estimate points are required"}


def test_bulk_destroy(world, db):
    client, _, ws, project = world.full_stack()
    created = _make_estimate(client, ws, project)
    resp = client.delete(
        f"/api/workspaces/{ws['slug']}/projects/{project['id']}/estimates/{created['id']}/")
    assert resp.status_code == 204
    # Deletes are soft: the row stays with deleted_at set.
    with db.cursor() as cur:
        cur.execute("SELECT deleted_at FROM estimates WHERE id=%s", (created["id"],))
        assert cur.fetchone()["deleted_at"] is not None


def test_project_estimates_active(world):
    client, _, ws, project = world.full_stack()
    created = _make_estimate(client, ws, project)
    resp = client.patch(f"/api/workspaces/{ws['slug']}/projects/{project['id']}/",
                        json={"estimate": created["id"]})
    assert resp.status_code == 200
    resp = client.get(
        f"/api/workspaces/{ws['slug']}/projects/{project['id']}/project-estimates/")
    assert resp.status_code == 200
    body = resp.json()
    assert isinstance(body, list) and len(body) == 1
    assert_keys(body[0], POINT_KEYS, "project-estimates")


def test_point_create_shape(world):
    client, _, ws, project = world.full_stack()
    created = _make_estimate(client, ws, project)
    resp = client.post(
        f"/api/workspaces/{ws['slug']}/projects/{project['id']}/estimates/"
        f"{created['id']}/estimate-points/",
        json={"key": 3, "value": "3"})
    assert resp.status_code == 200
    assert_keys(resp.json(), POINT_KEYS, "point-create")
    assert resp.json()["key"] == 3


def test_point_create_missing_key_400(world):
    client, _, ws, project = world.full_stack()
    created = _make_estimate(client, ws, project)
    resp = client.post(
        f"/api/workspaces/{ws['slug']}/projects/{project['id']}/estimates/"
        f"{created['id']}/estimate-points/",
        json={"value": "3"})
    assert resp.status_code == 400
    assert resp.json() == {"error": "Key and value are required"}


def test_point_partial_update(world):
    client, _, ws, project = world.full_stack()
    created = _make_estimate(client, ws, project)
    point_id = created["points"][0]["id"]
    resp = client.patch(
        f"/api/workspaces/{ws['slug']}/projects/{project['id']}/estimates/"
        f"{created['id']}/estimate-points/{point_id}/",
        json={"value": "updated"})
    assert resp.status_code == 200
    assert_keys(resp.json(), POINT_KEYS, "point-partial")
    assert resp.json()["value"] == "updated"


def test_point_destroy_rearranges_keys(world):
    client, _, ws, project = world.full_stack()
    created = _make_estimate(client, ws, project, points=[
        {"key": 1, "value": "1", "description": ""},
        {"key": 2, "value": "2", "description": ""},
        {"key": 3, "value": "3", "description": ""},
    ])
    first_id = [p for p in created["points"] if p["key"] == 1][0]["id"]
    resp = client.request(
        "DELETE",
        f"/api/workspaces/{ws['slug']}/projects/{project['id']}/estimates/"
        f"{created['id']}/estimate-points/{first_id}/",
        json={})
    assert resp.status_code == 200
    remaining = sorted(resp.json(), key=lambda p: p["key"])
    assert [p["key"] for p in remaining] == [1, 2]
