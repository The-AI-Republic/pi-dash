# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Bounds on the per-project agent ticking policy.

The project settings form is the only writer of these fields, but the
web-app serializer exposes them through ``fields = "__all__"``, so the
bounds live on the serializer rather than in the form — a hand-crafted
PATCH has to be rejected too.

``-1`` is the infinite-pool sentinel (``IssueAgentTicker.INFINITE_MAX_TICKS``).
``0`` and other negatives are not: they would arm a clock that either never
fires or reads as infinite in one code path and as exhausted in another.
"""

import pytest
from rest_framework.exceptions import ValidationError

from pi_dash.app.serializers.project import ProjectSerializer


@pytest.mark.parametrize("max_ticks", [-1, 1, 10, 500])
def test_valid_max_ticks_pass_through(max_ticks):
    assert ProjectSerializer().validate_agent_default_max_ticks(max_ticks) == max_ticks


@pytest.mark.parametrize("max_ticks", [0, -2, -10])
def test_zero_and_other_negatives_are_rejected(max_ticks):
    with pytest.raises(ValidationError):
        ProjectSerializer().validate_agent_default_max_ticks(max_ticks)


@pytest.mark.parametrize(
    "validator",
    [
        "validate_agent_default_interval_seconds",
        "validate_agent_review_default_interval_seconds",
        "validate_agent_test_default_interval_seconds",
    ],
)
@pytest.mark.parametrize("interval", [60, 1800, 10800, 86400])
def test_cadence_at_or_above_the_scan_floor_passes(validator, interval):
    assert getattr(ProjectSerializer(), validator)(interval) == interval


@pytest.mark.parametrize(
    "validator",
    [
        "validate_agent_default_interval_seconds",
        "validate_agent_review_default_interval_seconds",
        "validate_agent_test_default_interval_seconds",
    ],
)
@pytest.mark.parametrize("interval", [59, 0, -1])
def test_cadence_below_the_scan_floor_is_rejected(validator, interval):
    """The ticker scans once a minute, so a sub-minute cadence cannot be served."""
    with pytest.raises(ValidationError):
        getattr(ProjectSerializer(), validator)(interval)
