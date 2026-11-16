"""Contract: space reaction creates follow DRF CharField rules (PIDASHCONV-1636).

``IssueReactionSerializer`` (``space/serializer/issue.py:157-161``) and
``CommentReactionSerializer`` (``:425-429``) are ``ModelSerializer``s over
``reaction = models.TextField()`` (``db/models/issue.py:733,760``), so the
field is DRF ``CharField(trim_whitespace=True, allow_blank=False)``,
validated by the views at ``space/views/issue.py:375,460``. The Rust port
must strip the input, reject whitespace-only input as blank, and coerce
ints/floats the way ``CharField.to_internal_value`` does (``str(data)``).
"""

import pytest

pytestmark = pytest.mark.contract

BASE = "/api/public"


def _issue_reactions_url(world):
    return f"{BASE}/anchor/{world['anchor']}/issues/{world['issue']['id']}/reactions/"


def _comment_reactions_url(world):
    return f"{BASE}/anchor/{world['anchor']}/comments/{world['comment']['id']}/reactions/"


def test_issue_reaction_create_trims_whitespace(user_client, world, db):
    response = user_client.post(_issue_reactions_url(world), json={"reaction": " +1 "})
    assert response.status_code == 201
    body = response.json()
    assert body["reaction"] == "+1"
    row = db.fetchone(
        "SELECT reaction FROM issue_reactions WHERE id = %s",
        (body["id"],),
    )
    assert row["reaction"] == "+1"


def test_issue_reaction_create_blank_rejected(user_client, world):
    response = user_client.post(_issue_reactions_url(world), json={"reaction": "   "})
    assert response.status_code == 400
    assert response.json() == {"reaction": ["This field may not be blank."]}


def test_issue_reaction_create_number_coerced(user_client, world, db):
    response = user_client.post(_issue_reactions_url(world), json={"reaction": 5})
    assert response.status_code == 201
    body = response.json()
    assert body["reaction"] == "5"
    row = db.fetchone(
        "SELECT reaction FROM issue_reactions WHERE id = %s",
        (body["id"],),
    )
    assert row["reaction"] == "5"


def test_comment_reaction_create_trims_whitespace(user_client, world, db):
    response = user_client.post(_comment_reactions_url(world), json={"reaction": " +1 "})
    assert response.status_code == 201
    body = response.json()
    assert body["reaction"] == "+1"
    row = db.fetchone(
        "SELECT reaction FROM comment_reactions WHERE id = %s",
        (body["id"],),
    )
    assert row["reaction"] == "+1"


def test_comment_reaction_create_blank_rejected(user_client, world):
    response = user_client.post(_comment_reactions_url(world), json={"reaction": "   "})
    assert response.status_code == 400
    assert response.json() == {"reaction": ["This field may not be blank."]}


def test_comment_reaction_create_number_coerced(user_client, world, db):
    response = user_client.post(_comment_reactions_url(world), json={"reaction": 5})
    assert response.status_code == 201
    body = response.json()
    assert body["reaction"] == "5"
    row = db.fetchone(
        "SELECT reaction FROM comment_reactions WHERE id = %s",
        (body["id"],),
    )
    assert row["reaction"] == "5"
