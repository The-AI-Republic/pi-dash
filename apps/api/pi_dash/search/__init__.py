# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Search domain: FTS building blocks for each indexable model.

Submodules:
    issue   — Issue (name + description_stripped) full-text search.

Callers should import from the submodule (``pi_dash.search.issue``) for
clarity, but the most common names are re-exported here for convenience.
"""

from .issue import (
    ISSUE_COMMENT_SEARCH_VECTOR,
    ISSUE_FTS_CONFIG,
    ISSUE_SEARCH_VECTOR,
    issue_search_queryset,
    search_issues,
)

__all__ = [
    "ISSUE_COMMENT_SEARCH_VECTOR",
    "ISSUE_FTS_CONFIG",
    "ISSUE_SEARCH_VECTOR",
    "issue_search_queryset",
    "search_issues",
]
