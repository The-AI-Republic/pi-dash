"""Contract tests: board PATCH audit/PK edge inputs (PIDASHCONV-1518).

Two JSON-path divergences (plus the review-requested form twin), all
probed live on Django: ``updated_by`` present (null, blank, or UUID)
must 200 with the audit stamp winning over the validated value, and
blank-string PKs (``intake`` / ``created_by``) must 200 as null (DRF
``RelatedField.run_validation`` forces ``''`` to ``None``). Every test
below runs unchanged against Django and against Rust with identical
expectations.
"""

import urllib.parse

from psycopg.rows import dict_row

FORM = {"Content-Type": "application/x-www-form-urlencoded"}


def _form(**fields):
    return urllib.parse.urlencode(fields)


def test_board_patch_json_updated_by_stamp_wins(world, db):
    # `updated_by` validates but never writes: `BaseModel.save()`
    # re-stamps it to the current user on every update, overwriting the
    # validated value (null, blank, or another user) — the stamp wins.
    client, user, ws, project = world.full_stack()
    other, _ = world.add_member(ws, project)
    base = f"/api/workspaces/{ws['slug']}/projects/{project['id']}/project-deploy-boards/"
    board_id = client.post(base, json={}).json()["id"]
    url = f"{base}{board_id}/"
    for payload in ({"updated_by": None}, {"updated_by": ""},
                    {"updated_by": str(other["id"])}):
        resp = client.patch(url, json=payload)
        assert resp.status_code == 200, (payload, resp.text[:300])
        assert resp.json()["updated_by"] == str(user["id"])
        with db.cursor(row_factory=dict_row) as cur:
            cur.execute(
                "SELECT updated_by_id FROM deploy_boards WHERE id=%s",
                (board_id,))
            assert str(cur.fetchone()["updated_by_id"]) == str(user["id"])


def test_board_patch_json_blank_pk_strings_to_null(world, db):
    # DRF `RelatedField.run_validation` forces `''` to None on every
    # input (JSON included); `allow_null` then stores NULL.
    from _harness import seed as _seed
    client, _, ws, project = world.full_stack()
    other, _ = world.add_member(ws, project)
    intake = _seed.intake(world.conn, ws["id"], project["id"])
    base = f"/api/workspaces/{ws['slug']}/projects/{project['id']}/project-deploy-boards/"
    board_id = client.post(base, json={}).json()["id"]
    url = f"{base}{board_id}/"
    resp = client.patch(url, json={"created_by": str(other["id"]),
                                   "intake": str(intake["id"])})
    assert resp.status_code == 200, resp.text[:300]
    with db.cursor(row_factory=dict_row) as cur:
        cur.execute(
            "SELECT created_by_id, intake_id FROM deploy_boards WHERE id=%s",
            (board_id,))
        row = cur.fetchone()
        assert str(row["created_by_id"]) == str(other["id"])
        assert str(row["intake_id"]) == str(intake["id"])
    resp = client.patch(url, json={"created_by": "", "intake": ""})
    assert resp.status_code == 200, resp.text[:300]
    body = resp.json()
    assert body["created_by"] is None
    assert body["intake"] is None
    with db.cursor(row_factory=dict_row) as cur:
        cur.execute(
            "SELECT created_by_id, intake_id FROM deploy_boards WHERE id=%s",
            (board_id,))
        row = cur.fetchone()
        assert row["created_by_id"] is None
        assert row["intake_id"] is None


def test_board_patch_form_updated_by_uuid_stamp_wins(world, db):
    # Form twin of the JSON `updated_by` case (PIDASHCONV-1508 review):
    # a non-blank form `updated_by` validates, then the stamp wins.
    client, user, ws, project = world.full_stack()
    other, _ = world.add_member(ws, project)
    base = f"/api/workspaces/{ws['slug']}/projects/{project['id']}/project-deploy-boards/"
    board_id = client.post(base, json={}).json()["id"]
    resp = client.patch(f"{base}{board_id}/",
                        content=_form(updated_by=str(other["id"])),
                        headers=FORM)
    assert resp.status_code == 200, resp.text[:300]
    assert resp.json()["updated_by"] == str(user["id"])
    with db.cursor(row_factory=dict_row) as cur:
        cur.execute(
            "SELECT updated_by_id FROM deploy_boards WHERE id=%s",
            (board_id,))
        assert str(cur.fetchone()["updated_by_id"]) == str(user["id"])
