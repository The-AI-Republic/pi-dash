"""Contract tests: out-of-range JSON float literals in *string/choice* fields.

Django's DRF `JSONParser` decodes with `json.load`, so a JSON number literal
past `f64` range (`1e999`) becomes the Python float `inf`; a `CharField` /
choice field then stringifies it with `str(inf)` -> `"inf"`. serde_json
(arbitrary_precision) instead keeps the literal and normalizes it to
`"1e+999"`, so the port must route its number->string coercions through the
shared CPython helper (`python_number_str`) to match (PIDASHCONV-1146).

The bodies are sent as raw bytes: `httpx`'s `json=` would choke on the
literal and the value must reach the server unmodified.
"""

import uuid

from psycopg.rows import dict_row

JSON = {"Content-Type": "application/json"}


def _tag():
    return uuid.uuid4().hex[:8].upper()


def _stored(db, project_id, column):
    with db.cursor(row_factory=dict_row) as cur:
        cur.execute(
            "SELECT %s AS v FROM projects WHERE id = %%s" % column,
            (str(project_id),),
        )
        return cur.fetchone()["v"]


def test_name_out_of_range_float_is_inf(world, db):
    client, _, ws, _ = world.full_stack()
    tag = _tag()
    body = ('{"name": 1e999, "identifier": "N%s"}' % tag[:9]).encode()
    resp = client.post(f"/api/workspaces/{ws['slug']}/projects/", content=body, headers=JSON)
    assert resp.status_code == 201, resp.text[:300]
    assert resp.json()["name"] == "inf", resp.text[:300]
    assert _stored(db, resp.json()["id"], "name") == "inf"


def test_identifier_out_of_range_float_is_upper_inf(world, db):
    client, _, ws, _ = world.full_stack()
    tag = _tag()
    body = ('{"name": "X %s", "identifier": 1e999}' % tag).encode()
    resp = client.post(f"/api/workspaces/{ws['slug']}/projects/", content=body, headers=JSON)
    assert resp.status_code == 201, resp.text[:300]
    # `Project.save()` runs `identifier.strip().upper()`.
    assert resp.json()["identifier"] == "INF", resp.text[:300]
    assert _stored(db, resp.json()["id"], "identifier") == "INF"


def test_description_out_of_range_float_is_inf(world, db):
    client, _, ws, _ = world.full_stack()
    tag = _tag()
    body = (
        '{"name": "Y %s", "identifier": "Y%s", "description": 1e999}'
        % (tag, tag[:9])
    ).encode()
    resp = client.post(f"/api/workspaces/{ws['slug']}/projects/", content=body, headers=JSON)
    assert resp.status_code == 201, resp.text[:300]
    assert resp.json()["description"] == "inf", resp.text[:300]
    assert _stored(db, resp.json()["id"], "description") == "inf"


def test_base_branch_out_of_range_float_is_inf(world, db):
    client, _, ws, _ = world.full_stack()
    tag = _tag()
    body = (
        '{"name": "Z %s", "identifier": "Z%s", "base_branch": 1e999}'
        % (tag, tag[:9])
    ).encode()
    resp = client.post(f"/api/workspaces/{ws['slug']}/projects/", content=body, headers=JSON)
    assert resp.status_code == 201, resp.text[:300]
    assert resp.json()["base_branch"] == "inf", resp.text[:300]
    assert _stored(db, resp.json()["id"], "base_branch") == "inf"


def test_timezone_out_of_range_float_is_invalid_choice(world):
    client, _, ws, _ = world.full_stack()
    tag = _tag()
    body = (
        '{"name": "W %s", "identifier": "W%s", "timezone": 1e999}'
        % (tag, tag[:9])
    ).encode()
    resp = client.post(f"/api/workspaces/{ws['slug']}/projects/", content=body, headers=JSON)
    assert resp.status_code == 400, resp.text[:300]
    assert resp.json() == {"timezone": ['"inf" is not a valid choice.']}, resp.text[:300]


def test_other_char_fields_out_of_range_float_are_inf(world, db):
    # repo_url / external_id / cover_image / emoji / external_source all
    # share the DRF `CharField` `str()` coercion.
    for column in ("repo_url", "external_id", "cover_image", "emoji", "external_source"):
        client, _, ws, _ = world.full_stack()
        tag = _tag()
        body = (
            '{"name": "V %s", "identifier": "V%s", "%s": 1e999}'
            % (tag, tag[:9], column)
        ).encode()
        resp = client.post(f"/api/workspaces/{ws['slug']}/projects/", content=body, headers=JSON)
        assert resp.status_code == 201, (column, resp.text[:300])
        assert resp.json()[column] == "inf", (column, resp.text[:300])
        assert _stored(db, resp.json()["id"], column) == "inf", column
