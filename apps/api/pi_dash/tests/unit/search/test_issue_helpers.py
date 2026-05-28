# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Unit tests for pi_dash.search.issue helpers.

These exercise pure Python paths only — composing ``Q`` objects and
parsing ``ts_headline`` output. The DB-backed behavior of
``issue_search_queryset`` is covered by the contract tests under
``tests/contract/api/test_issue_search.py`` (which need Postgres FTS).
"""

import pytest
from django.contrib.postgres.search import SearchQuery
from django.db.models import Q

from pi_dash.search.issue import (
    HEADLINE_START_SEL,
    HEADLINE_STOP_SEL,
    ISSUE_FTS_CONFIG,
    _SEQUENCE_ID_MAX,
    _build_search_filter,
    extract_snippet,
)


def _q_repr(q):
    """Render a ``Q`` object as a list of ``(field, value)`` pairs.

    Walks the tree depth-first so test assertions can pattern-match on
    *what* fields ended up in the filter without coupling to Django's
    internal node ordering.
    """
    if isinstance(q, Q):
        out = []
        for child in q.children:
            out.extend(_q_repr(child))
        return out
    return [q]


def _make_sq(query):
    return SearchQuery(query, search_type="websearch", config=ISSUE_FTS_CONFIG)


@pytest.mark.unit
class TestExtractSnippet:
    """``extract_snippet`` recognises a real ts_headline hit by its
    delimiter markers and returns plain text without them. Filler text
    (no markers) must produce an empty snippet so the agent doesn't see
    a misleading description prefix on a comment-only or title-only
    match.
    """

    def test_empty_input_returns_empty(self):
        assert extract_snippet("") == ""

    def test_none_input_returns_empty(self):
        assert extract_snippet(None) == ""

    def test_no_markers_returns_empty(self):
        # ts_headline filler when nothing matched in description_stripped.
        assert extract_snippet("Lorem ipsum dolor sit amet…") == ""

    def test_strips_markers_around_match(self):
        raw = f"see the {HEADLINE_START_SEL}migration{HEADLINE_STOP_SEL} runbook"
        assert extract_snippet(raw) == "see the migration runbook"

    def test_strips_multiple_markers(self):
        raw = (
            f"the {HEADLINE_START_SEL}migration{HEADLINE_STOP_SEL} "
            f"and {HEADLINE_START_SEL}rollback{HEADLINE_STOP_SEL} steps"
        )
        assert extract_snippet(raw) == "the migration and rollback steps"


@pytest.mark.unit
class TestBuildSearchFilter:
    """``_build_search_filter`` composes the OR-chain that downstream
    callers apply via ``.filter()``. We assert on the shape of the ``Q``
    tree rather than executing it.
    """

    def test_includes_fts_and_name_icontains(self):
        q = _build_search_filter("hello", _make_sq("hello"), include_comments=False)
        pairs = [p for p in _q_repr(q) if isinstance(p, tuple)]
        fields = {field for field, _ in pairs}
        assert "_fts" in fields
        assert "name__icontains" in fields
        assert "project__identifier__icontains" in fields

    def test_excludes_comment_subquery_by_default(self):
        q = _build_search_filter("hello", _make_sq("hello"), include_comments=False)
        fields = {field for field, _ in (p for p in _q_repr(q) if isinstance(p, tuple))}
        assert "id__in" not in fields

    def test_includes_comment_subquery_when_opted_in(self):
        q = _build_search_filter("hello", _make_sq("hello"), include_comments=True)
        fields = {field for field, _ in (p for p in _q_repr(q) if isinstance(p, tuple))}
        assert "id__in" in fields

    def test_extracts_sequence_id_when_query_short(self):
        # Query is ≤ 20 chars and contains a small integer → picked up.
        q = _build_search_filter("PROJ 42", _make_sq("PROJ 42"), include_comments=False)
        pairs = [p for p in _q_repr(q) if isinstance(p, tuple)]
        seq_pairs = [(f, v) for f, v in pairs if f == "sequence_id"]
        assert seq_pairs == [("sequence_id", 42)]

    def test_skips_sequence_id_when_query_long(self):
        # > 20 chars: numeric tokens are NOT extracted (pasted logs/stack
        # traces don't sequence-id-match).
        query = "a very long query with 42 inside but over twenty chars"
        q = _build_search_filter(query, _make_sq(query), include_comments=False)
        pairs = [p for p in _q_repr(q) if isinstance(p, tuple)]
        assert not any(f == "sequence_id" for f, _ in pairs)

    def test_skips_digit_token_that_would_overflow_int4(self):
        # Regression test for the int4 overflow bug: a digit token
        # greater than 2_147_483_647 (int4 max) used to crash Postgres
        # with "out of range for type integer" and 500 the endpoint.
        # The build filter must now silently skip it.
        overflow_token = str(_SEQUENCE_ID_MAX + 1)  # 2_147_483_648
        assert len(overflow_token) == 10  # still passes the 20-char gate
        q = _build_search_filter(overflow_token, _make_sq(overflow_token), include_comments=False)
        pairs = [p for p in _q_repr(q) if isinstance(p, tuple)]
        seq_pairs = [(f, v) for f, v in pairs if f == "sequence_id"]
        assert seq_pairs == []

    def test_skips_digit_token_with_more_than_ten_chars(self):
        # An 11-digit token never fits in int4; we short-circuit before
        # int() to avoid a needless parse, but the *behaviour* under
        # test is the same: no sequence_id branch is added.
        token = "12345678901"  # 11 digits
        q = _build_search_filter(token, _make_sq(token), include_comments=False)
        pairs = [p for p in _q_repr(q) if isinstance(p, tuple)]
        assert not any(f == "sequence_id" for f, _ in pairs)

    def test_accepts_int4_boundary(self):
        # The exact max is allowed — protects against off-by-one in the
        # bound check.
        token = str(_SEQUENCE_ID_MAX)
        q = _build_search_filter(token, _make_sq(token), include_comments=False)
        pairs = [p for p in _q_repr(q) if isinstance(p, tuple)]
        seq_pairs = [(f, v) for f, v in pairs if f == "sequence_id"]
        assert seq_pairs == [("sequence_id", _SEQUENCE_ID_MAX)]

    def test_extracts_multiple_sequence_ids(self):
        q = _build_search_filter("see 1 and 2", _make_sq("see 1 and 2"), include_comments=False)
        pairs = [p for p in _q_repr(q) if isinstance(p, tuple)]
        seq_values = sorted(v for f, v in pairs if f == "sequence_id")
        assert seq_values == [1, 2]
