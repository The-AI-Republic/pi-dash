# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Issue full-text search building blocks.

All callers should go through ``issue_search_queryset`` so the
``SearchVector`` expressions stay byte-for-byte identical to the ones
backing ``issues_fts_idx`` and ``issue_comments_fts_idx`` (see the
``indexes`` declarations on ``Issue.Meta`` and ``IssueComment.Meta``).
Diverging the expression would silently drop the index from the plan.

Coverage:
    * Issue.name + Issue.description_stripped — primary index.
    * IssueComment.comment_stripped — OR-joined via subquery so the
      agent's "find why this was decided" queries match resolution
      discussion, not just title/description.
"""

# Python imports
import re

# Django imports
from django.contrib.postgres.search import (
    SearchHeadline,
    SearchQuery,
    SearchRank,
    SearchVector,
)
from django.db.models import Q

# Module imports
from pi_dash.db.models import IssueComment


ISSUE_FTS_CONFIG = "english"

# Must match the expression in ``Issue.Meta.indexes['issues_fts_idx']``.
ISSUE_SEARCH_VECTOR = SearchVector(
    "name", "description_stripped", config=ISSUE_FTS_CONFIG
)

# Must match the expression in
# ``IssueComment.Meta.indexes['issue_comments_fts_idx']``.
ISSUE_COMMENT_SEARCH_VECTOR = SearchVector(
    "comment_stripped", config=ISSUE_FTS_CONFIG
)


def _matching_comment_issue_ids(search_query):
    """Subquery of ``issue_id``s whose comments match ``search_query``.

    Uses ``IssueComment.objects`` (a SoftDeletionManager), so soft-deleted
    comments are automatically excluded.
    """
    return (
        IssueComment.objects.annotate(_cfts=ISSUE_COMMENT_SEARCH_VECTOR)
        .filter(_cfts=search_query)
        .values("issue_id")
    )


def _build_search_filter(query, search_query):
    """Compose the FTS predicate with the legacy numeric / project-code
    fallbacks ``search_issues`` has always supported, plus a comment-side
    match via subquery."""
    q = Q(_fts=search_query) | Q(id__in=_matching_comment_issue_ids(search_query))
    if len(query) <= 20:
        for sequence_id in re.findall(r"\b\d+\b", query):
            q |= Q(sequence_id=sequence_id)
    q |= Q(project__identifier__icontains=query)
    return q


def issue_search_queryset(queryset, query, *, with_rank=False, with_headline=False):
    """Annotate and filter ``queryset`` for issue search.

    Args:
        queryset: a base Issue queryset (caller applies workspace/project
            scoping and permission filters first).
        query: the user-supplied search string. Empty / falsy returns the
            queryset unchanged so callers can compose conditionally.
        with_rank: annotate ``_rank`` (SearchRank against the issue's own
            vector, title+description). Comment-only matches will rank 0
            — caller should secondary-sort by recency.
        with_headline: annotate ``_headline`` — a short snippet around the
            match from ``description_stripped``. Plain text, no markup.
            Empty when the match was in a comment only.

    Annotations always added when query is non-empty:
        ``_fts`` (SearchVector) — exists so the lookup
        ``Q(_fts=SearchQuery(...))`` resolves against an annotation and
        the planner can match the index expression.
    """
    if not query:
        return queryset

    search_query = SearchQuery(
        query, search_type="websearch", config=ISSUE_FTS_CONFIG
    )

    annotations = {"_fts": ISSUE_SEARCH_VECTOR}
    if with_rank:
        annotations["_rank"] = SearchRank(ISSUE_SEARCH_VECTOR, search_query)
    if with_headline:
        annotations["_headline"] = SearchHeadline(
            "description_stripped",
            search_query,
            config=ISSUE_FTS_CONFIG,
            start_sel="",
            stop_sel="",
            max_words=20,
            min_words=10,
            short_word=3,
            highlight_all=False,
        )

    return queryset.annotate(**annotations).filter(
        _build_search_filter(query, search_query)
    )


def search_issues(query, queryset):
    """Backward-compatible wrapper used by the app-tier search views."""
    return issue_search_queryset(queryset, query).distinct()
