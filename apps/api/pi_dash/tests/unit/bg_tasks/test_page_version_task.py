# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Regression tests for :func:`pi_dash.bgtasks.page_version_task.track_page_version`.

The task read ``page.description``, a field ``Page`` does not have, and its
catch-all ``except`` swallowed the ``AttributeError`` — so no page version
was ever recorded, for editor saves and API writes alike.
"""

import json

import pytest

from pi_dash.bgtasks.page_version_task import track_page_version
from pi_dash.db.models import Page, PageVersion


@pytest.fixture
def page(db, workspace, create_user):
    return Page.objects.create(
        name="Versioned",
        description_html="<p>new</p>",
        description_json={"type": "doc"},
        description_binary=b"\x01\x02\x03\x04",
        owned_by=create_user,
        workspace=workspace,
    )


@pytest.mark.unit
@pytest.mark.django_db
class TestTrackPageVersion:
    def test_changed_body_creates_a_version(self, page, create_user):
        track_page_version(page.id, json.dumps({"description_html": "<p>old</p>"}), create_user.id)

        version = PageVersion.objects.get(page=page)
        assert version.description_html == "<p>new</p>"
        assert version.description_json == {"type": "doc"}
        assert bytes(version.description_binary) == b"\x01\x02\x03\x04"
        assert version.owned_by_id == create_user.id

    def test_recent_version_by_the_same_user_is_updated_in_place(self, page, create_user):
        track_page_version(page.id, json.dumps({"description_html": "<p>old</p>"}), create_user.id)
        page.description_html = "<p>newer</p>"
        page.save()

        track_page_version(page.id, json.dumps({"description_html": "<p>new</p>"}), create_user.id)

        version = PageVersion.objects.get(page=page)
        assert version.description_html == "<p>newer</p>"

    def test_unchanged_body_creates_no_version(self, page, create_user):
        track_page_version(page.id, json.dumps({"description_html": "<p>new</p>"}), create_user.id)

        assert not PageVersion.objects.filter(page=page).exists()
