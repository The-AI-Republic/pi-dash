# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Seed helpers for the global ``test`` PromptTemplate row.

The ticking runtime composes the ``test`` prompt from sections (not this
DB row), but the row mirrors the ``review`` precedent for operator
visibility and is inserted by migration ``0005_test_template`` via
``seed_test_template`` semantics. These tests lock in the create /
skip / force-refresh behavior the migration relies on.
"""

from __future__ import annotations

import pytest

from pi_dash.prompting.models import PromptTemplate
from pi_dash.prompting.seed import (
    TEST_TEMPLATE_BODY,
    TEST_TEMPLATE_NAME,
    seed_test_template,
)


@pytest.mark.unit
def test_seed_test_template_creates_then_skips(db):
    # Fresh DB: the migration would have already created it under
    # post_migrate, so remove it to test the create path deterministically.
    PromptTemplate.objects.filter(
        workspace__isnull=True, name=TEST_TEMPLATE_NAME
    ).delete()

    assert seed_test_template() == "created"
    row = PromptTemplate.objects.get(
        workspace__isnull=True, name=TEST_TEMPLATE_NAME
    )
    assert row.body == TEST_TEMPLATE_BODY
    assert row.is_active is True

    # Idempotent: a second call without force is a no-op.
    assert seed_test_template() == "skipped"


@pytest.mark.unit
def test_seed_test_template_force_refreshes_changed_body(db):
    PromptTemplate.objects.filter(
        workspace__isnull=True, name=TEST_TEMPLATE_NAME
    ).delete()
    seed_test_template()
    row = PromptTemplate.objects.get(
        workspace__isnull=True, name=TEST_TEMPLATE_NAME
    )
    row.body = "stale body"
    row.save(update_fields=["body"])

    assert seed_test_template(force=True) == "refreshed"
    row.refresh_from_db()
    assert row.body == TEST_TEMPLATE_BODY
    assert row.version == 2
