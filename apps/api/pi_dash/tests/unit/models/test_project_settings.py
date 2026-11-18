# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""``Project.settings`` — the Rust-owned namespaced bag (PDASHOSS01-375).

Django stores the bag without interpreting it; the point of these tests
is the legacy contract: ``Project`` create/update paths that never name
``settings`` keep working and see ``{}``, and a save that doesn't touch
the field leaves whatever is stored alone.
"""

import pytest

from pi_dash.db.models import Project


@pytest.mark.unit
@pytest.mark.django_db
class TestProjectSettings:
    def test_create_without_settings_defaults_to_empty_bag(self, workspace, create_user):
        project = Project.objects.create(name="Bare Project", identifier="BAR", workspace=workspace)
        project.refresh_from_db()
        assert project.settings == {}

    def test_explicit_settings_are_stored_verbatim(self, workspace, create_user):
        project = Project.objects.create(
            name="Set Project",
            identifier="SET",
            workspace=workspace,
            settings={"issue_run_policy": {"enabled": True}},
        )
        project.refresh_from_db()
        assert project.settings == {"issue_run_policy": {"enabled": True}}

    def test_update_without_settings_leaves_it_untouched(self, workspace, create_user):
        project = Project.objects.create(
            name="Keep Project",
            identifier="KEP",
            workspace=workspace,
            settings={"some": {"nested": [1, 2, 3]}},
        )
        project.name = "Renamed Project"
        project.save()
        project.refresh_from_db()
        assert project.settings == {"some": {"nested": [1, 2, 3]}}
