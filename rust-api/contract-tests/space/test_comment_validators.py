"""Contract: space comment validators trim whitespace and count characters.

PIDASHCONV-1261: ``IssueCommentSerializer`` is a ``ModelSerializer``, so its
text fields are DRF ``CharField(trim_whitespace=True)`` and ``max_length``
counts characters (``len(str)``). The Rust port must trim input and compare
``chars().count()`` against the limit — not UTF-8 byte length.
"""

import pytest

pytestmark = pytest.mark.contract

BASE = "/api/public"


def _comments_url(world, comment_id=None):
    url = f"{BASE}/anchor/{world['anchor']}/issues/{world['issue']['id']}/comments/"
    return url if comment_id is None else f"{url}{comment_id}/"


def test_patch_speaker_label_counts_characters_not_bytes(user_client, world):
    label = "é" * 100  # 100 chars, 200 UTF-8 bytes; max_length is 128 chars.
    response = user_client.patch(
        _comments_url(world, world["comment"]["id"]), json={"speaker_label": label}
    )
    assert response.status_code == 200
    assert response.json()["speaker_label"] == label


def test_patch_speaker_label_over_char_limit_rejected(user_client, world):
    response = user_client.patch(
        _comments_url(world, world["comment"]["id"]), json={"speaker_label": "x" * 129}
    )
    assert response.status_code == 400
    assert response.json() == {
        "speaker_label": ["Ensure this field has no more than 128 characters."]
    }


def test_create_comment_html_surrounding_whitespace_trimmed(user_client, world):
    response = user_client.post(_comments_url(world), json={"comment_html": "  <p>hi</p>\n"})
    assert response.status_code == 201
    assert response.json()["comment_html"] == "<p>hi</p>"
    reread = user_client.get(_comments_url(world, response.json()["id"]))
    assert reread.status_code == 200
    assert reread.json()["comment_html"] == "<p>hi</p>"
