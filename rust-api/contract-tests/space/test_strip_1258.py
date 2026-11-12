"""Contract: stripped text follows the MLStripper rule (PIDASHCONV-1258).

``Issue.save`` and ``IssueComment.save`` compute stripped text with
``pi_dash.utils.html_processor.strip_tags`` (``db/models/issue.py:21``) —
``MLStripper`` (``HTMLParser`` with ``convert_charrefs=True``), so entities
decode and a ``<`` that is not a tag stays as text. The space slice used
the wrong strippers (a naive ``<...>`` skipper for comments,
Django-verbatim for intake), so rows written by Rust differed from
Django's for the same input.
"""

import pytest

pytestmark = pytest.mark.contract

BASE = "/api/public"

HTML = "<p>Tom &amp; Jerry &lt;3 I <3 x</p>"
DECODED = "Tom & Jerry <3 I <3 x"
# The descriptions mirror row on comment create is recomputed by
# ``Description.save`` with Django's own verbatim ``strip_tags``
# (``db/models/description.py:22-29``), unlike the decoded comment column.
VERBATIM = "Tom &amp; Jerry &lt;3 I <3 x"


def _comments_url(world):
    return f"{BASE}/anchor/{world['anchor']}/issues/{world['issue']['id']}/comments/"


def _inbox_url(world):
    return f"{BASE}/anchor/{world['anchor']}/intakes/{world['intake']['id']}/inbox-issues/"


def test_comment_create_stripped_decodes_entities(user_client, world, db):
    # PIDASHCONV-1258: IssueComment.save (db/models/issue.py:606) strips
    # with MLStripper, so the 201 body and issue_comments carry DECODED ...
    response = user_client.post(_comments_url(world), json={"comment_html": HTML})
    assert response.status_code == 201
    body = response.json()
    assert body["comment_stripped"] == DECODED
    row = db.fetchone(
        "SELECT comment_stripped, description_id FROM issue_comments WHERE id = %s",
        (body["id"],),
    )
    assert row["comment_stripped"] == DECODED
    # ... while the descriptions mirror row keeps entities verbatim.
    mirror = db.fetchone(
        "SELECT description_stripped FROM descriptions WHERE id = %s",
        (row["description_id"],),
    )
    assert mirror["description_stripped"] == VERBATIM


def test_intake_create_stripped_decodes_entities(user_client, world, db):
    # PIDASHCONV-1258: Issue.save (db/models/issue.py:330-334) strips with
    # MLStripper, so the response body and the issues row carry DECODED.
    response = user_client.post(
        _inbox_url(world), json={"issue": {"name": "entities idea", "description_html": HTML}}
    )
    assert response.status_code == 200
    body = response.json()
    assert body["description_stripped"] == DECODED
    row = db.fetchone(
        "SELECT description_stripped FROM issues WHERE id = %s", (body["id"],)
    )
    assert row["description_stripped"] == DECODED
