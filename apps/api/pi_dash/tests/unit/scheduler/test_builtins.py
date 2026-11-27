# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Tests for ``pi_dash.scheduler.builtins.ensure_builtin_schedulers``.

Covers idempotency and race-safe behavior — see Codex review #3.
"""

from __future__ import annotations

from unittest import mock

import pytest

from pi_dash.db.models import Scheduler
from pi_dash.scheduler.builtins import (
    BUILTINS,
    BuiltinScheduler,
    ensure_builtin_schedulers,
)


@pytest.mark.unit
def test_ensure_creates_one_row_per_builtin(workspace):
    # Prod catalog is empty after NEWPIDASH-256: no rows are seeded.
    ensure_builtin_schedulers(workspace)
    rows = Scheduler.objects.filter(workspace=workspace)
    assert rows.count() == len(BUILTINS) == 0
    slugs = set(rows.values_list("slug", flat=True))
    assert slugs == {b.slug for b in BUILTINS} == set()
    # The mechanics still hold with a test-only catalog entry — the
    # ensure loop is catalog-driven, not hardcoded to prod slugs.
    test_builtin = BuiltinScheduler(
        slug="test-only-builtin",
        name="Test Only",
        description="",
        prompt="prompt",
    )
    ensure_builtin_schedulers(workspace, builtins=[test_builtin])
    rows = Scheduler.objects.filter(workspace=workspace, slug="test-only-builtin")
    assert rows.count() == 1


@pytest.mark.unit
def test_ensure_is_idempotent(workspace):
    ensure_builtin_schedulers(workspace)
    first_count = Scheduler.objects.filter(workspace=workspace).count()
    # Calling again must not duplicate rows.
    ensure_builtin_schedulers(workspace)
    assert Scheduler.objects.filter(workspace=workspace).count() == first_count


@pytest.mark.unit
def test_ensure_updates_existing_row(workspace, create_user):
    """If a workspace already has a builtin row with stale prompt text,
    ensure_builtin_schedulers refreshes it."""
    # Prod catalog is empty after NEWPIDASH-256, so no rows are seeded
    # by the post_save signal. Exercise the refresh path with a
    # test-only catalog entry instead of relying on a prod slug.
    test_builtin = BuiltinScheduler(
        slug="test-refresh-builtin",
        name="Test Refresh",
        description="",
        prompt="original prompt",
    )
    ensure_builtin_schedulers(workspace, builtins=[test_builtin])
    stale = Scheduler.objects.get(workspace=workspace, slug="test-refresh-builtin")
    stale.name = "Stale name"
    stale.prompt = "Stale prompt"
    stale.save(update_fields=["name", "prompt", "updated_at"])

    refreshed = BuiltinScheduler(
        slug="test-refresh-builtin",
        name="Test Refresh Updated",
        description="updated",
        prompt="refreshed prompt",
    )
    ensure_builtin_schedulers(workspace, builtins=[refreshed])
    row = Scheduler.objects.get(workspace=workspace, slug="test-refresh-builtin")
    assert row.name == "Test Refresh Updated"
    assert row.prompt == "refreshed prompt"
    assert row.source == "builtin"


@pytest.mark.unit
def test_ensure_handles_concurrent_create_via_integrity_error(workspace):
    """Codex review #3: when two callers race, the IntegrityError on the
    losing insert must be caught, the winning row re-fetched, and
    defaults applied — not propagated as an exception."""
    from pi_dash.scheduler.builtins import ensure_builtin_schedulers as helper

    test_builtin = BuiltinScheduler(
        slug="race-test",
        name="Race Test",
        description="",
        prompt="prompt v1",
    )

    # First call creates the row normally.
    helper(workspace, builtins=[test_builtin])
    assert Scheduler.objects.filter(workspace=workspace, slug="race-test").exists()

    # Now simulate a race: monkey-patch .first() to return None even though
    # the row exists, forcing the helper down the create() path. The DB's
    # conditional unique constraint will reject the create with
    # IntegrityError; the helper must catch it and update the existing row.
    real_filter = Scheduler.objects.filter
    call_count = {"n": 0}

    def fake_filter(*args, **kwargs):
        qs = real_filter(*args, **kwargs)
        # First filter call inside the helper looks for an existing row;
        # subsequent calls (re-fetch on conflict) should behave normally.
        call_count["n"] += 1
        if call_count["n"] == 1:
            mocked_qs = mock.MagicMock(wraps=qs)
            mocked_qs.first.return_value = None
            return mocked_qs
        return qs

    refreshed_builtin = BuiltinScheduler(
        slug="race-test",
        name="Race Test Updated",
        description="",
        prompt="prompt v2",
    )

    with mock.patch.object(Scheduler.objects, "filter", side_effect=fake_filter):
        # Should NOT raise IntegrityError — helper catches it and updates
        # via the re-fetch path.
        helper(workspace, builtins=[refreshed_builtin])

    row = Scheduler.objects.get(workspace=workspace, slug="race-test")
    assert row.prompt == "prompt v2"
    assert row.name == "Race Test Updated"
