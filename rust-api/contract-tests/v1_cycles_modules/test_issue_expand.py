"""Contract tests for PIDASHCONV-1195: `?expand=` on the issue lists.

Two divergences from Django's `BaseSerializer.to_representation`
(`serializers/base.py:72-118`) plus `IssueSerializer.to_representation`
(`serializers/issue.py:437-441`) are pinned here, for both the cycle-issue and
the module-issue lists:

* ``?expand=url`` — ``url`` is a declared ``SerializerMethodField``, so the
  ``if expand in self.fields`` fallthrough sets it to ``None``; the serializer
  then pops the ``None``. Django omits the key entirely; the port must never
  emit ``"url": null``.
* ``?expand=description_markdown`` — a declared write-only field: it is in
  ``self.fields`` (so the fallthrough adds it) but never rendered. Django gains
  ``"description_markdown": null`` on every row; the port must too.

Both backends run with a web base URL configured (the matrix workflow sets
``WEB_URL`` / ``CONTRACT_WEB_URL`` for each side), so the plain list carries an
absolute ``url`` and ``expand=url`` has something to remove.
"""

import pytest

from _harness import api, db, env

SLUG = db.WS_A_SLUG
PROJ = db.PROJ_A_ID

CYCLE_LIST = api.cycle_issues_url(SLUG, PROJ, db.CYCLE_ACTIVE_ID)
MODULE_LIST = api.module_issues_url(SLUG, PROJ, db.MODULE_ACTIVE_ID)

# The fixed seed rows: CYCLE_ACTIVE holds the started + completed issues,
# MODULE_ACTIVE holds the unstarted issue. sequence_ids are seeded too.
CYCLE_SEQUENCES = (103, 104)
MODULE_SEQUENCES = (102,)

# (label, list url, the sequence_ids of the seeded rows it returns)
CASES = [
    ("cycle", CYCLE_LIST, CYCLE_SEQUENCES),
    ("module", MODULE_LIST, MODULE_SEQUENCES),
]


def _rows(response):
    assert response.status_code == 200, response.text
    body = response.json()
    return body["results"]


def _expected_urls(sequences):
    return {f"{env.web_base()}/{SLUG}/browse/CTP-{seq}" for seq in sequences}


@pytest.mark.parametrize("label,url,sequences", CASES)
def test_expand_url_omits_key(admin_client, label, url, sequences):
    # Baseline: with a web base URL configured the plain list carries the
    # absolute url on every row, so the expand path has something to remove.
    baseline = _rows(admin_client.get(url))
    assert {row["url"] for row in baseline} == _expected_urls(sequences)

    expanded = _rows(admin_client.get(url, params={"expand": "url"}))
    assert len(expanded) == len(sequences)
    for row in expanded:
        # Django nulls url then pops the None, so the key is absent — not null.
        assert "url" not in row, row


@pytest.mark.parametrize("label,url,sequences", CASES)
def test_expand_description_markdown_adds_null(admin_client, label, url, sequences):
    # Baseline: the write-only field is never in the un-expanded body.
    baseline = _rows(admin_client.get(url))
    for row in baseline:
        assert "description_markdown" not in row

    expanded = _rows(
        admin_client.get(url, params={"expand": "description_markdown"})
    )
    assert len(expanded) == len(sequences)
    for row in expanded:
        # A declared write-only field is in self.fields, so the fallthrough
        # adds it as null to every row.
        assert "description_markdown" in row
        assert row["description_markdown"] is None
        # Unrelated expands leave url untouched.
        assert row["url"] in _expected_urls(sequences)
