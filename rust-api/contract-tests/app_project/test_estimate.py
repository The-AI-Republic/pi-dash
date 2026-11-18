"""Contract tests: estimate URL module (5 routes)."""

from psycopg.rows import dict_row

from _harness.diffs import expect

from .conftest import assert_keys

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
    with db.cursor(row_factory=dict_row) as cur:
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


def test_bulk_create_long_value_400(world, db):
    # DIFF-23 (NEWPIDASH-274, old bug fix for PB-0590): Django inserts the
    # Estimate row before points validation, so a 400 still leaves a
    # point-less estimate behind; Rust validates first and stores nothing.
    # Both answer the same 400 — only persistence differs.
    client, _, ws, project = world.full_stack()
    resp = client.post(
        f"/api/workspaces/{ws['slug']}/projects/{project['id']}/estimates/",
        json={"estimate": {"name": "Bad", "type": "points"},
              "estimate_points": [{"key": 1, "value": "x" * 21, "description": ""}]})
    assert resp.status_code == 400
    assert resp.json() == [
        {"non_field_errors": ["Value can't be more than 20 characters"]}]
    listed = client.get(
        f"/api/workspaces/{ws['slug']}/projects/{project['id']}/estimates/")
    assert listed.status_code == 200
    assert len([r for r in listed.json() if r["name"] == "Bad"]) == expect(
        "DIFF-23", django=1, rust=0)
    with db.cursor(row_factory=dict_row) as cur:
        cur.execute(
            "SELECT COUNT(*) AS n FROM estimates WHERE name=%s AND project_id=%s",
            ("Bad", str(project["id"])))
        assert cur.fetchone()["n"] == expect("DIFF-23", django=1, rust=0)
        cur.execute(
            "SELECT COUNT(*) AS n FROM estimate_points ep "
            "JOIN estimates e ON e.id=ep.estimate_id "
            "WHERE e.name=%s AND e.project_id=%s",
            ("Bad", str(project["id"])))
        assert cur.fetchone()["n"] == 0


def test_bulk_create_missing_points_400(world, db):
    # DIFF-23 sibling: a missing `estimate_points` key takes the same
    # insert-before-validate path as the overlong value on Django.
    client, _, ws, project = world.full_stack()
    resp = client.post(
        f"/api/workspaces/{ws['slug']}/projects/{project['id']}/estimates/",
        json={"estimate": {"name": "Missing", "type": "points"}})
    assert resp.status_code == 400
    assert resp.json() == {"non_field_errors": ["No data provided"]}
    listed = client.get(
        f"/api/workspaces/{ws['slug']}/projects/{project['id']}/estimates/")
    assert listed.status_code == 200
    assert len([r for r in listed.json() if r["name"] == "Missing"]) == expect(
        "DIFF-23", django=1, rust=0)
    with db.cursor(row_factory=dict_row) as cur:
        cur.execute(
            "SELECT COUNT(*) AS n FROM estimates WHERE name=%s AND project_id=%s",
            ("Missing", str(project["id"])))
        assert cur.fetchone()["n"] == expect("DIFF-23", django=1, rust=0)


def test_bulk_create_key_above_int4_max_400(world, db):
    # NEWPIDASH-289: a point key above the int4 max is a serializer 400
    # on both backends (`EstimatePointSerializer.key` MaxValueValidator),
    # not a column `DataError` 500. Status and body match; only the
    # orphan-estimate persistence differs (DIFF-23).
    client, _, ws, project = world.full_stack()
    resp = client.post(
        f"/api/workspaces/{ws['slug']}/projects/{project['id']}/estimates/",
        json={"estimate": {"name": "BigKey", "type": "points"},
              "estimate_points": [{"key": 2147483648, "value": "1"}]})
    assert resp.status_code == 400
    assert resp.json() == [
        {"key": ["Ensure this value is less than or equal to 2147483647."]}]
    listed = client.get(
        f"/api/workspaces/{ws['slug']}/projects/{project['id']}/estimates/")
    assert listed.status_code == 200
    assert len([r for r in listed.json() if r["name"] == "BigKey"]) == expect(
        "DIFF-23", django=1, rust=0)
    with db.cursor(row_factory=dict_row) as cur:
        cur.execute(
            "SELECT COUNT(*) AS n FROM estimates WHERE name=%s AND project_id=%s",
            ("BigKey", str(project["id"])))
        assert cur.fetchone()["n"] == expect("DIFF-23", django=1, rust=0)
        cur.execute(
            "SELECT COUNT(*) AS n FROM estimate_points ep "
            "JOIN estimates e ON e.id=ep.estimate_id "
            "WHERE e.name=%s AND e.project_id=%s",
            ("BigKey", str(project["id"])))
        assert cur.fetchone()["n"] == 0
    # Boundary: int4 max itself is accepted.
    created = _make_estimate(client, ws, project, name="MaxKey", points=[
        {"key": 2147483647, "value": "1"}])
    assert created["points"][0]["key"] == 2147483647


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
    with db.cursor(row_factory=dict_row) as cur:
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


def test_point_create_long_value_diff32(world, db):
    # DIFF-32 (NEWPIDASH-275, old bug fix): Django
    # `EstimatePointEndpoint.create` never runs
    # `EstimatePointSerializer.validate()`, so POSTing `{key: 9,
    # value: "x"*21}` stores the overlong value with 200, while
    # bulk-create and point-update answer 400. Rust enforces the same
    # 20-char cap on single-create and answers the identical
    # `non_field_errors` 400, storing nothing.
    client, _, ws, project = world.full_stack()
    created = _make_estimate(client, ws, project)
    resp = client.post(
        f"/api/workspaces/{ws['slug']}/projects/{project['id']}/estimates/"
        f"{created['id']}/estimate-points/",
        json={"key": 9, "value": "x" * 21})
    assert resp.status_code == expect("DIFF-32", django=200, rust=400)
    if resp.status_code == 400:
        assert resp.json() == {
            "non_field_errors": ["Value can't be more than 20 characters"]}
    else:
        assert_keys(resp.json(), POINT_KEYS, "point-create-long-django")
        assert resp.json()["key"] == 9
        assert resp.json()["value"] == "x" * 21
    with db.cursor(row_factory=dict_row) as cur:
        cur.execute(
            "SELECT COUNT(*) AS n FROM estimate_points "
            "WHERE estimate_id=%s AND key=9 AND value=%s",
            (created["id"], "x" * 21))
        assert cur.fetchone()["n"] == expect("DIFF-32", django=1, rust=0)


def test_point_create_boundary_20_200(world):
    # The 20-char boundary holds on both backends: a 20-char value is
    # accepted with 200 and the stored point shape.
    client, _, ws, project = world.full_stack()
    created = _make_estimate(client, ws, project)
    resp = client.post(
        f"/api/workspaces/{ws['slug']}/projects/{project['id']}/estimates/"
        f"{created['id']}/estimate-points/",
        json={"key": 8, "value": "x" * 20})
    assert resp.status_code == 200
    assert_keys(resp.json(), POINT_KEYS, "point-create-boundary-20")
    assert resp.json()["key"] == 8
    assert resp.json()["value"] == "x" * 20


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
