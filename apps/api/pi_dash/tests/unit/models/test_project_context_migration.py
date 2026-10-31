# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""The data step of ``db/0169_project_context``.

Built-in prompts stopped reading ``Project.description``; the backfill is what
keeps an existing project's prompt text unchanged across the upgrade.
"""

from __future__ import annotations

import importlib

import pytest
from django.apps import apps

from pi_dash.db.models import Project

migration = importlib.import_module("pi_dash.db.migrations.0169_project_context")


def _project(workspace, identifier, **fields):
    return Project.objects.create(name=identifier, identifier=identifier, workspace=workspace, **fields)


@pytest.mark.unit
def test_backfill_copies_description_into_context(db, workspace):
    described = _project(workspace, "DESC", description="Rulebook.\nNever push to main.")
    empty = _project(workspace, "EMPT")
    deleted = _project(workspace, "GONE", description="Archived rules.")
    deleted.delete()  # soft delete

    migration.backfill_context_from_description(apps, None)

    described.refresh_from_db()
    empty.refresh_from_db()
    assert described.context == "Rulebook.\nNever push to main."
    assert described.description == "Rulebook.\nNever push to main."  # left as is
    assert empty.context == ""
    assert Project.all_objects.get(pk=deleted.pk).context == "Archived rules."


@pytest.mark.unit
def test_backfill_does_not_overwrite_an_existing_context(db, workspace):
    project = _project(workspace, "KEEP", description="Summary.", context="Hand-written agent rules.")

    migration.backfill_context_from_description(apps, None)

    project.refresh_from_db()
    assert project.context == "Hand-written agent rules."
