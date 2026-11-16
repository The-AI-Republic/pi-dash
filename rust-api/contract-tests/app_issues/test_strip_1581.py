"""Contract: comment writes use Django's strippers + changed-subset sync (PIDASHCONV-1581).

``IssueComment.save`` recomputes ``comment_stripped`` with MLStripper
(entities decoded, ``db/models/issue.py:606``); the linked ``Description``
row is recomputed by ``Description.save`` with Django's own
``strip_tags`` (entities verbatim, NULL-if-empty,
``db/models/description.py:23-29``). On update only the changed subset
syncs (``db/models/issue.py:637-646``). The app writers bound the ML
text to the description column on create/mint and overwrote the full
triple on update.
"""

import json as jsonlib

import psycopg.rows


def _base(ws, pid):
    return f"/api/workspaces/{ws}/projects/{pid}"


def _issue(ws, pid, iid):
    return f"{_base(ws, pid)}/issues/{iid}"


HTML = "<p>Tom &amp; Jerry &lt;3 I <3 x</p>"
DECODED = "Tom & Jerry <3 I <3 x"
VERBATIM = "Tom &amp; Jerry &lt;3 I <3 x"


def _dict_cur(db_conn):
    return db_conn.cursor(row_factory=psycopg.rows.dict_row)


def test_comment_create_description_stripped_verbatim(clients, seed, db_conn):
    # PIDASHCONV-1581 item 1: the comment column decodes entities
    # (MLStripper) while the descriptions mirror row keeps them
    # verbatim (Description.save recompute).
    admin = clients["admin"]
    ws, pid, i1 = seed["ws_slug"], seed["project"], seed["issue1"]
    resp = admin.post(
        f"{_issue(ws, pid, i1)}/comments/",
        json={"comment_html": HTML},
    )
    assert resp.status_code == 201
    body = resp.json()
    assert body["comment_stripped"] == DECODED
    comment_id = body["id"]
    try:
        with _dict_cur(db_conn) as cur:
            cur.execute(
                "SELECT comment_stripped, description_id FROM issue_comments WHERE id = %s",
                (comment_id,),
            )
            row = cur.fetchone()
        assert row["comment_stripped"] == DECODED
        with _dict_cur(db_conn) as cur:
            cur.execute(
                "SELECT description_stripped FROM descriptions WHERE id = %s",
                (row["description_id"],),
            )
            mirror = cur.fetchone()
        assert mirror["description_stripped"] == VERBATIM
    finally:
        admin.delete(f"{_issue(ws, pid, i1)}/comments/{comment_id}/")


def test_comment_update_mint_description_stripped_verbatim(clients, seed, db_conn):
    # PIDASHCONV-1581 item 1 (update-mint): an unlinked comment mints
    # its Description via objects.create, so the same recompute
    # applies — the ML text must not leak into the mirror row.
    admin = clients["admin"]
    ws, pid, i1 = seed["ws_slug"], seed["project"], seed["issue1"]
    created = admin.post(
        f"{_issue(ws, pid, i1)}/comments/",
        json={"comment_html": "<p>mint me</p>"},
    )
    assert created.status_code == 201
    comment_id = created.json()["id"]
    try:
        with db_conn.cursor() as cur:
            cur.execute(
                "UPDATE issue_comments SET description_id = NULL WHERE id = %s",
                (comment_id,),
            )
        db_conn.commit()
        resp = admin.patch(
            f"{_issue(ws, pid, i1)}/comments/{comment_id}/",
            json={"comment_html": HTML},
        )
        assert resp.status_code == 200
        with _dict_cur(db_conn) as cur:
            cur.execute(
                "SELECT comment_stripped, description_id FROM issue_comments WHERE id = %s",
                (comment_id,),
            )
            row = cur.fetchone()
        assert row["comment_stripped"] == DECODED
        assert row["description_id"] is not None
        with _dict_cur(db_conn) as cur:
            cur.execute(
                "SELECT description_stripped FROM descriptions WHERE id = %s",
                (row["description_id"],),
            )
            mirror = cur.fetchone()
        assert mirror["description_stripped"] == VERBATIM
    finally:
        admin.delete(f"{_issue(ws, pid, i1)}/comments/{comment_id}/")


def test_comment_patch_syncs_changed_subset_only(clients, seed, db_conn):
    # PIDASHCONV-1581 item 2: PATCH {comment_html: <changed>} syncs
    # only the changed subset into the mirror row — an independently
    # diverged description_json survives. (The description FK is a
    # OneToOneField, so a shared-row re-link 400s on validation;
    # out-of-band divergence is the observable instead.)
    admin = clients["admin"]
    ws, pid, i1 = seed["ws_slug"], seed["project"], seed["issue1"]
    created = admin.post(
        f"{_issue(ws, pid, i1)}/comments/",
        json={"comment_html": "<p>first version</p>", "comment_json": {"mine": 1}},
    )
    assert created.status_code == 201
    comment_id = created.json()["id"]
    try:
        with _dict_cur(db_conn) as cur:
            cur.execute(
                "SELECT description_id FROM issue_comments WHERE id = %s",
                (comment_id,),
            )
            description_id = cur.fetchone()["description_id"]
        with db_conn.cursor() as cur:
            cur.execute(
                "UPDATE descriptions SET description_json = %s::jsonb WHERE id = %s",
                (jsonlib.dumps({"foreign": True}), description_id),
            )
        db_conn.commit()
        resp = admin.patch(
            f"{_issue(ws, pid, i1)}/comments/{comment_id}/",
            json={"comment_html": "<p>second version</p>"},
        )
        assert resp.status_code == 200
        with _dict_cur(db_conn) as cur:
            cur.execute(
                "SELECT description_html, description_stripped, description_json"
                " FROM descriptions WHERE id = %s",
                (description_id,),
            )
            mirror = cur.fetchone()
        assert mirror["description_html"] == "<p>second version</p>"
        assert mirror["description_stripped"] == "second version"
        assert mirror["description_json"] == {"foreign": True}
    finally:
        admin.delete(f"{_issue(ws, pid, i1)}/comments/{comment_id}/")
