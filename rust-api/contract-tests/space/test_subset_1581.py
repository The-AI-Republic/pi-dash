"""Contract: space comment PATCH syncs only the changed description subset (PIDASHCONV-1581 item 2).

``IssueComment.save`` syncs ``{desc_field: getattr(self, comment_field)}``
for the tracked fields in ``_changes_on_save`` only
(``db/models/issue.py:637-646``). The space writer overwrote the full
``(description_html, description_stripped, description_json)`` triple
whenever ANY field changed, clobbering a description row's own value
for the unchanged fields.
"""

import json

import pytest

pytestmark = pytest.mark.contract

BASE = "/api/public"


def _comments_url(world, comment_id=None):
    base = f"{BASE}/anchor/{world['anchor']}/issues/{world['issue']['id']}/comments/"
    return base if comment_id is None else f"{base}{comment_id}/"


def test_comment_patch_preserves_unchanged_description_json(user_client, world, db):
    # Diverge the mirror row's json out of band, then PATCH only the
    # html: the sync must carry html (+ the recomputed stripped text)
    # and leave the foreign json alone.
    created = user_client.post(
        _comments_url(world), json={"comment_html": "<p>first version</p>"}
    )
    assert created.status_code == 201
    comment_id = created.json()["id"]
    row = db.fetchone(
        "SELECT description_id FROM issue_comments WHERE id = %s",
        (comment_id,),
    )
    description_id = row["description_id"]
    db.execute(
        "UPDATE descriptions SET description_json = %s::jsonb WHERE id = %s",
        (json.dumps({"foreign": True}), description_id),
    )
    patched = user_client.patch(
        _comments_url(world, comment_id), json={"comment_html": "<p>second version</p>"}
    )
    assert patched.status_code == 200
    mirror = db.fetchone(
        "SELECT description_html, description_stripped, description_json"
        " FROM descriptions WHERE id = %s",
        (description_id,),
    )
    assert mirror["description_html"] == "<p>second version</p>"
    assert mirror["description_stripped"] == "second version"
    assert mirror["description_json"] == {"foreign": True}
