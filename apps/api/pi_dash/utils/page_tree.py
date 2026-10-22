# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Rules for nesting a page under a parent page, shared by the public API
(``api/views/page.py``) and the web app's page views (``app/views/page``)."""

# Module imports
from pi_dash.db.models import Page


def parent_error(visible_pages, parent_id, page=None):
    """Return why ``parent_id`` cannot be the parent, or ``None`` if it can.

    ``visible_pages`` is the queryset of pages the caller can see in the
    project. The parent must be one of them and not archived, and — when
    ``page`` is being moved — not the page itself or one of its descendants.
    """
    if parent_id is None:
        return None
    parent = visible_pages.filter(pk=parent_id).first()
    if parent is None:
        return "Parent page not found in this project"
    if parent.archived_at is not None:
        return "Parent page is archived"
    if page is not None:
        ancestor_id, seen = parent.id, set()
        while ancestor_id is not None and ancestor_id not in seen:
            if ancestor_id == page.id:
                return "A page cannot be nested under itself"
            seen.add(ancestor_id)
            ancestor_id = Page.objects.filter(pk=ancestor_id).values_list("parent_id", flat=True).first()
    return None
