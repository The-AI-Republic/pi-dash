"""v2 workspace-asset link/unlink stamps the entity audit columns.

``PATCH`` (link) and ``DELETE`` (unlink) for ``WORKSPACE_LOGO`` /
``PROJECT_COVER`` run a full Django ``workspace.save()`` /
``project.save()`` (``v2.py:259,281,293,309``), so the workspaces /
projects row gets ``updated_at`` (``auto_now``) and ``updated_by``
(CRUM request user) — not just the logo/cover columns.
PIDASHCONV-1398; twin of the bulk-cover fix PIDASHCONV-929.
"""

from .test_workspace_assets import ws_base, ws_detail, ws_payload


def _backdate(conn, table, row_id, other_id):
    conn.execute(
        f"UPDATE {table} SET updated_at = now() - interval '1 hour',"
        " updated_by_id = %s WHERE id = %s",
        (other_id, row_id),
    )
    return conn.execute(
        f"SELECT updated_at FROM {table} WHERE id = %s", (row_id,)
    ).fetchone()[0]


def _stamps(conn, table, row_id):
    return conn.execute(
        f"SELECT updated_at, updated_by_id FROM {table} WHERE id = %s",
        (row_id,),
    ).fetchone()


def _link(org, entity_type, entity_id):
    payload = ws_payload(org, entity_type=entity_type)
    payload["entity_identifier"] = entity_id
    asset_id = org.request("POST", ws_base(org), "admin", json=payload).json()["asset_id"]
    r = org.request("PATCH", ws_detail(org, asset_id), "admin", json={})
    assert r.status_code == 204, r.text
    return asset_id


def test_logo_link_stamps_workspace(org):
    before = _backdate(org.conn, "workspaces", org.workspace["id"], org.outsider["id"])
    _link(org, "WORKSPACE_LOGO", org.workspace["id"])
    updated_at, updated_by = _stamps(org.conn, "workspaces", org.workspace["id"])
    assert str(updated_by) == org.admin["id"]
    assert updated_at > before


def test_logo_unlink_stamps_workspace(org):
    asset_id = _link(org, "WORKSPACE_LOGO", org.workspace["id"])
    before = _backdate(org.conn, "workspaces", org.workspace["id"], org.outsider["id"])
    r = org.request("DELETE", ws_detail(org, asset_id), "admin")
    assert r.status_code == 204, r.text
    updated_at, updated_by = _stamps(org.conn, "workspaces", org.workspace["id"])
    assert str(updated_by) == org.admin["id"]
    assert updated_at > before


def test_cover_link_stamps_project(org):
    before = _backdate(org.conn, "projects", org.project["id"], org.outsider["id"])
    _link(org, "PROJECT_COVER", org.project["id"])
    updated_at, updated_by = _stamps(org.conn, "projects", org.project["id"])
    assert str(updated_by) == org.admin["id"]
    assert updated_at > before


def test_cover_unlink_stamps_project(org):
    asset_id = _link(org, "PROJECT_COVER", org.project["id"])
    before = _backdate(org.conn, "projects", org.project["id"], org.outsider["id"])
    r = org.request("DELETE", ws_detail(org, asset_id), "admin")
    assert r.status_code == 204, r.text
    updated_at, updated_by = _stamps(org.conn, "projects", org.project["id"])
    assert str(updated_by) == org.admin["id"]
    assert updated_at > before
