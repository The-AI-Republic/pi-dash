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
    * IssueComment.comment_stripped — optional OR via subquery (off by
      default; legacy endpoints pass ``include_comments=False`` to keep
      their pre-FTS contract; ``IssueAdvancedSearchEndpoint`` opts in).
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

# Headline delimiters used by ``SearchHeadline`` so the view can detect
# whether ``ts_headline`` actually highlighted anything — Postgres returns
# a leading-text excerpt with no markers when nothing matched, which is
# misleading. The view strips these before returning to the client.
HEADLINE_START_SEL = "<<"
HEADLINE_STOP_SEL = ">>"

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

    Known limitation: this does NOT filter ``IssueComment.access``, so a
    user who can see an issue (project member) but not its INTERNAL
    comments will still surface the issue via comment-text match. Fixing
    this requires threading the request user's project role through the
    search util to mirror the comment-list endpoint's visibility logic.
    Tracked separately.
    """
    return (
        IssueComment.objects.annotate(_cfts=ISSUE_COMMENT_SEARCH_VECTOR)
        .filter(_cfts=search_query)
        .values("issue_id")
    )


def _build_search_filter(query, search_query, include_comments):
    """Compose the FTS predicate.

    The OR-chain in order:
      1. FTS over ``name`` + ``description_stripped`` (stem-aware,
         token-based — the main path).
      2. ``Q(name__icontains=query)`` — substring fallback so partial-word
         lookups like ``auth`` → ``Authentication`` keep working after
         the FTS swap. Cheap because ``name`` is short.
      3. (optional) ``Q(id__in=<comments matching subquery>)`` — only
         enabled by callers that want to widen results to comment text.
      4. Legacy ``sequence_id`` exact-int branch (guarded on query
         length ≤ 20 to avoid scanning numeric tokens out of pasted
         logs/stack traces).
      5. Legacy ``project__identifier`` icontains for short codes.
    """
    q = Q(_fts=search_query) | Q(name__icontains=query)
    if include_comments:
        q |= Q(id__in=_matching_comment_issue_ids(search_query))
    if len(query) <= 20:
        for sequence_id in re.findall(r"\b\d+\b", query):
            q |= Q(sequence_id=sequence_id)
    q |= Q(project__identifier__icontains=query)
    return q


def issue_search_queryset(
    queryset,
    query,
    *,
    with_rank=False,
    with_headline=False,
    include_comments=False,
):
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
            match from ``description_stripped``, delimited by
            ``HEADLINE_START_SEL`` / ``HEADLINE_STOP_SEL`` so the caller
            can detect whether any tokens actually matched. Empty when
            the match was in a comment only.
        include_comments: OR-include issues whose comments match. Default
            False so the legacy ``search_issues`` / ``IssueSearchEndpoint``
            contract (title/sequence-id/project-code only) is preserved.
            New callers (the agent-oriented advanced endpoint) opt in.

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
            start_sel=HEADLINE_START_SEL,
            stop_sel=HEADLINE_STOP_SEL,
            max_words=20,
            min_words=10,
            short_word=3,
            highlight_all=False,
        )

    return queryset.annotate(**annotations).filter(
        _build_search_filter(query, search_query, include_comments)
    )


def extract_snippet(headline):
    """Convert a ``_headline`` annotation value to the public snippet.

    Returns ``""`` when ``ts_headline`` returned filler text rather than
    a real match (no markers present) or when the field was NULL.
    Otherwise strips the marker delimiters and returns plain text.
    """
    if not headline or HEADLINE_START_SEL not in headline:
        return ""
    return headline.replace(HEADLINE_START_SEL, "").replace(HEADLINE_STOP_SEL, "")


def search_issues(query, queryset):
    """Backward-compatible wrapper used by the app-tier search views.

    Preserves the pre-PR contract: title + sequence_id + project__identifier
    + name-substring fallback; **no** comment-text widening.
    """
    return issue_search_queryset(queryset, query, include_comments=False).distinct()
