"""Contract: space comment PATCH re-link audit + stripped recompute (PIDASHCONV-1640).

``IssueComment.save`` (``db/models/issue.py:606-626``) recomputes
``comment_stripped`` from the current ``comment_html`` on every save, and
when the comment has no ``description_id`` it runs
``Description.objects.create`` — whose ``BaseModel.save``
(``db/models/base.py:21-41``) stamps ``created_by`` with the requesting
user and ``updated_by = None``. The space writer copied the comment's old
audit columns into the new description row and kept the stored stripped
text when the PATCH carried no ``comment_html``.
"""

import pytest

pytestmark = pytest.mark.contract

BASE = "/api/public"


def _comments_url(world, comment_id=None):
    base = f"{BASE}/anchor/{world['anchor']}/issues/{world['issue']['id']}/comments/"
    return base if comment_id is None else f"{base}{comment_id}/"


def test_comment_patch_relink_stamps_requester_audit(user_client, world, db, other_user):
    # A legacy comment row (NULL description_id, foreign audit columns)
    # re-links on PATCH: the fresh Description is a create(), so its
    # audit columns are the requesting user / NULL — not the comment's
    # old values.
    created = user_client.post(
        _comments_url(world), json={"comment_html": "<p>first</p>"}
    )
    assert created.status_code == 201
    comment_id = created.json()["id"]
    db.execute(
        "UPDATE issue_comments SET description_id = NULL, created_by_id = %s,"
        " updated_by_id = %s WHERE id = %s",
        (other_user["id"], other_user["id"], comment_id),
    )
    patched = user_client.patch(
        _comments_url(world, comment_id), json={"comment_html": "<p>new</p>"}
    )
    assert patched.status_code == 200
    row = db.fetchone(
        "SELECT description_id FROM issue_comments WHERE id = %s",
        (comment_id,),
    )
    assert row["description_id"] is not None
    mirror = db.fetchone(
        "SELECT created_by_id, updated_by_id FROM descriptions WHERE id = %s",
        (row["description_id"],),
    )
    assert str(mirror["created_by_id"]) == world["owner"]["id"]
    assert mirror["updated_by_id"] is None


def test_comment_patch_without_html_recomputes_stripped(user_client, world, db):
    # save() recomputes comment_stripped from comment_html on every save,
    # including a PATCH that changes only comment_json: a stale stored
    # value (verbatim entities, pre-1258/1581 rows) is overwritten with
    # the MLStripper output.
    created = user_client.post(
        _comments_url(world), json={"comment_html": "<p>R&amp;D</p>"}
    )
    assert created.status_code == 201
    comment_id = created.json()["id"]
    db.execute(
        "UPDATE issue_comments SET comment_stripped = %s WHERE id = %s",
        ("R&amp;D", comment_id),
    )
    patched = user_client.patch(
        _comments_url(world, comment_id), json={"comment_json": {}}
    )
    assert patched.status_code == 200
    row = db.fetchone(
        "SELECT comment_html, comment_stripped FROM issue_comments WHERE id = %s",
        (comment_id,),
    )
    assert row["comment_html"] == "<p>R&amp;D</p>"
    assert row["comment_stripped"] == "R&D"
