"""Contract tests: out-of-range JSON float literals in request bodies.

Django's DRF `JSONParser` decodes with `json.load`, so a JSON number
literal past `f64` range (`1e999`) becomes `inf`; the port must do the
same. Three live shapes pin the difference (PIDASHCONV-1140):

(a) `POST .../project-views/` `{"sort_order": 1e999}` -> `204`; the
    value binds into `float8` and the row stores `Infinity`.
(b) `PATCH .../projects/<id>/` `{"logo_props": {"x": 1e999}}` -> the
    DRF `JSONField` re-encodes input with `allow_nan=False`, so the
    field error is `{"logo_props": ["Value must be valid JSON."]}` (400,
    nothing written).
(c) `POST .../projects/` `{"agent_default_max_ticks": 1e999}` -> the
    `IntegerField` runs `int(str(inf))` -> `{"agent_default_max_ticks":
    ["A valid integer is required."]}` (400).

The bodies are sent as raw bytes: `httpx`'s `json=` would fail to encode
`inf`/`1e999` client-side and would never reach the server.
"""

import math
import uuid

from psycopg.rows import dict_row

from .conftest import assert_keys
from .test_project import DETAIL_KEYS

JSON = {"Content-Type": "application/json"}


def _unique():
    tag = uuid.uuid4().hex[:8].upper()
    return "F %s" % tag, ("F%s" % tag)[:10]


def test_project_views_sort_order_out_of_range_float_is_204(world, db):
    """(a) Django stores `Infinity`; the port must not 500/400."""
    client, user, ws, project = world.full_stack()
    resp = client.post(
        f"/api/workspaces/{ws['slug']}/projects/{project['id']}/project-views/",
        content=b'{"sort_order": 1e999}',
        headers=JSON,
    )
    assert resp.status_code == 204, resp.text[:300]
    assert resp.content == b"", resp.content[:120]
    with db.cursor(row_factory=dict_row) as cur:
        cur.execute(
            "SELECT sort_order FROM project_members"
            " WHERE project_id = %s AND member_id = %s",
            (str(project["id"]), str(user["id"])),
        )
        stored = cur.fetchone()["sort_order"]
    assert isinstance(stored, float) and math.isinf(stored) and stored > 0, stored


def test_project_views_sort_order_negative_out_of_range_float(world, db):
    """The sign survives: `-1e999` stores `-Infinity`."""
    client, user, ws, project = world.full_stack()
    resp = client.post(
        f"/api/workspaces/{ws['slug']}/projects/{project['id']}/project-views/",
        content=b'{"sort_order": -1e999}',
        headers=JSON,
    )
    assert resp.status_code == 204, resp.text[:300]
    with db.cursor(row_factory=dict_row) as cur:
        cur.execute(
            "SELECT sort_order FROM project_members"
            " WHERE project_id = %s AND member_id = %s",
            (str(project["id"]), str(user["id"])),
        )
        stored = cur.fetchone()["sort_order"]
    assert isinstance(stored, float) and math.isinf(stored) and stored < 0, stored


def test_project_patch_logo_props_out_of_range_float_400(world, db):
    """(b) DRF `JSONField` rejects the non-finite float; no write."""
    client, _, ws, project = world.full_stack()
    resp = client.patch(
        f"/api/workspaces/{ws['slug']}/projects/{project['id']}/",
        content=b'{"logo_props": {"x": 1e999}}',
        headers=JSON,
    )
    assert resp.status_code == 400, resp.text[:300]
    assert resp.json() == {"logo_props": ["Value must be valid JSON."]}
    with db.cursor(row_factory=dict_row) as cur:
        cur.execute("SELECT logo_props FROM projects WHERE id = %s", (str(project["id"]),))
        assert cur.fetchone()["logo_props"] == {}, "logo_props was written despite the 400"


def test_project_patch_other_json_fields_out_of_range_float_400(world, db):
    """Every serializer `JSONField` shares the rejection (field in error)."""
    for field in ("description_text", "description_html", "icon_prop"):
        client, _, ws, project = world.full_stack()
        body = ('{"%s": {"x": 1e999}}' % field).encode()
        resp = client.patch(
            f"/api/workspaces/{ws['slug']}/projects/{project['id']}/",
            content=body,
            headers=JSON,
        )
        assert resp.status_code == 400, (field, resp.text[:300])
        assert resp.json() == {field: ["Value must be valid JSON."]}, (field, resp.text)


def test_project_create_logo_props_out_of_range_float_400(world):
    """Create validates the same JSONField."""
    client, _, ws, _ = world.full_stack()
    name, identifier = _unique()
    body = (
        '{"name": "%s", "identifier": "%s", "logo_props": {"x": 1e999}}'
        % (name, identifier)
    ).encode()
    resp = client.post(f"/api/workspaces/{ws['slug']}/projects/", content=body, headers=JSON)
    assert resp.status_code == 400, resp.text[:300]
    assert resp.json() == {"logo_props": ["Value must be valid JSON."]}


def test_project_create_agent_default_max_ticks_out_of_range_float_400(world):
    """(c) The `IntegerField` error body, not a parser error."""
    client, _, ws, _ = world.full_stack()
    name, identifier = _unique()
    body = (
        '{"name": "%s", "identifier": "%s", "agent_default_max_ticks": 1e999}'
        % (name, identifier)
    ).encode()
    resp = client.post(f"/api/workspaces/{ws['slug']}/projects/", content=body, headers=JSON)
    assert resp.status_code == 400, resp.text[:300]
    assert resp.json() == {"agent_default_max_ticks": ["A valid integer is required."]}


def test_project_views_jsonb_out_of_range_float_is_500(world):
    """A jsonb write takes the `DataError` 500 (psycopg emits `Infinity`)."""
    client, _, ws, project = world.full_stack()
    resp = client.post(
        f"/api/workspaces/{ws['slug']}/projects/{project['id']}/project-views/",
        content=b'{"view_props": {"x": 1e999}}',
        headers=JSON,
    )
    assert resp.status_code == 500, resp.text[:300]
    assert resp.json() == {"error": "Something went wrong please try again later"}


def test_huge_integer_literal_is_accepted_as_numeric(world):
    """An out-of-range *integer* literal is not a non-finite float.

    CPython keeps it an `int`; the jsonb column stores the exact numeric,
    so Django answers 200. The port must not reject it."""
    client, _, ws, project = world.full_stack()
    body = ('{"logo_props": {"x": 1%s}}' % ("0" * 400)).encode()
    resp = client.patch(
        f"/api/workspaces/{ws['slug']}/projects/{project['id']}/",
        content=body,
        headers=JSON,
    )
    assert resp.status_code == 200, resp.text[:300]
    assert_keys(resp.json(), DETAIL_KEYS, "huge-int-patch")
