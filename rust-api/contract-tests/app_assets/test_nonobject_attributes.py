"""Non-object ``attributes`` on project file assets (PIDASHCONV-1152).

``file_assets.attributes`` is a jsonb column and the v2 project PATCH handler
stores ``request.data.get("attributes")`` verbatim, so a client can persist a
JSON non-object (array, string, number or bool). Django then raises
``AttributeError`` on ``attributes.get(...)`` on every project file read path,
which the app exception handler turns into the generic 500 with no side effects
— the duplicate writes no row and copies no bytes.

The suite pins that on the three project-file read paths (fetch, both download
redirects) and on the duplicate: 500, and the duplicate adds no ``file_assets``
row. A JSON scalar ``null`` cannot be written through PATCH (the column is NOT
NULL, so the save raises ``IntegrityError`` → 400), so that row is seeded
directly — Django still reads it as ``None`` and 500s the same way.
"""

import pytest

from . import seed_assets as seed_a

SERVER_ERROR = {"error": "Something went wrong please try again later"}

# Non-object JSON values a PATCH can persist in the jsonb column.
NON_OBJECT_VALUES = [["x"], "x", 7, True]


def proj_base(org):
    return (
        f"/api/assets/v2/workspaces/{org.workspace['slug']}"
        f"/projects/{org.project['id']}/"
    )


def proj_detail(org, asset_id):
    return f"{proj_base(org)}{asset_id}/"


def ws_download(org, asset_id):
    return (
        f"/api/assets/v2/workspaces/{org.workspace['slug']}"
        f"/download/{asset_id}/"
    )


def proj_download(org, asset_id):
    return (
        f"/api/assets/v2/workspaces/{org.workspace['slug']}"
        f"/projects/{org.project['id']}/download/{asset_id}/"
    )


def dup_path(org, asset_id):
    return (
        f"/api/assets/v2/workspaces/{org.workspace['slug']}"
        f"/duplicate-assets/{asset_id}/"
    )


def mint_project_asset(org, name="nonobj.png"):
    r = org.request(
        "POST", proj_base(org), "admin",
        json={"name": name, "type": "image/png", "size": 200,
              "entity_type": "ISSUE_DESCRIPTION", "entity_identifier": None},
    )
    assert r.status_code == 200, r.text
    return r.json()["asset_id"]


def patch_nonobject(org, asset_id, value):
    r = org.request(
        "PATCH", proj_detail(org, asset_id), "admin", json={"attributes": value}
    )
    assert r.status_code == 204, r.text
    # The row now holds the non-object verbatim; the same PATCH flips
    # ``is_uploaded`` so the read paths serve instead of answering 404.
    row = seed_a.fetch(org.conn, asset_id)
    assert row["attributes"] == value
    assert row["is_uploaded"] is True


def assert_all_reads_500(org, asset_id):
    r = org.request("GET", proj_detail(org, asset_id), "admin")
    assert r.status_code == 500, r.text
    assert r.json() == SERVER_ERROR
    for url in (ws_download(org, asset_id), proj_download(org, asset_id)):
        r = org.request("GET", url, "admin")
        assert r.status_code == 500, (url, r.text)
        assert r.json() == SERVER_ERROR, url


def assert_duplicate_500_no_row(org, asset_id):
    before = {
        row[0]
        for row in org.conn.execute("SELECT id FROM file_assets").fetchall()
    }
    r = org.request(
        "POST", dup_path(org, asset_id), "admin",
        json={"entity_type": "WORKSPACE_LOGO",
              "entity_id": org.workspace["id"]},
    )
    assert r.status_code == 500, r.text
    assert r.json() == SERVER_ERROR
    after = {
        row[0]
        for row in org.conn.execute("SELECT id FROM file_assets").fetchall()
    }
    assert after == before, "duplicate inserted a row on non-object attributes"


@pytest.mark.parametrize("value", NON_OBJECT_VALUES)
def test_project_fetch_nonobject_attributes_500(org, value):
    asset_id = mint_project_asset(org)
    patch_nonobject(org, asset_id, value)
    assert_all_reads_500(org, asset_id)


@pytest.mark.parametrize("value", NON_OBJECT_VALUES)
def test_duplicate_nonobject_attributes_500_no_row(org, value):
    asset_id = mint_project_asset(org, name="nonobjdup.png")
    patch_nonobject(org, asset_id, value)
    assert_duplicate_500_no_row(org, asset_id)


def test_json_null_attributes_500(org):
    # A JSON scalar null (jsonb 'null'): the column is NOT NULL, so a PATCH
    # cannot write it (IntegrityError → 400); seed it directly. Django reads
    # it as ``None`` and raises AttributeError, exactly like a non-object.
    asset = seed_a.create_asset(
        org.conn,
        workspace_id=org.workspace["id"],
        project_id=org.project["id"],
        created_by_id=org.admin["id"],
        name="jsonnull.png",
        entity_type="ISSUE_DESCRIPTION",
        uploaded=True,
        attributes=None,
    )
    assert seed_a.fetch(org.conn, asset["id"])["attributes"] is None
    assert_all_reads_500(org, asset["id"])
    assert_duplicate_500_no_row(org, asset["id"])
