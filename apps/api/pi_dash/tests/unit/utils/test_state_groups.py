# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Regression coverage for lifecycle-wide state-group integrations."""

import pytest

from pi_dash.api.views.issue import IssueAdvancedSearchEndpoint
from pi_dash.db.models.state import DEFAULT_STATES, StateGroup
from pi_dash.space.utils.grouper import issue_group_values as space_issue_group_values
from pi_dash.utils.constants import (
    ACTIVE_STATE_GROUPS,
    CLOSED_STATE_GROUPS,
    OPEN_STATE_GROUPS,
    STATE_GROUP_ORDER,
)
from pi_dash.utils.filters.converters import LegacyToRichFiltersConverter
from pi_dash.utils.grouper import issue_group_values
from pi_dash.utils.issue_filters import filter_issue_state_type
from pi_dash.utils.order_queryset import STATE_ORDER


pytestmark = pytest.mark.unit


def test_test_group_is_in_the_canonical_lifecycle_order():
    assert STATE_GROUP_ORDER == (
        "backlog",
        "unstarted",
        "started",
        "review",
        "test",
        "completed",
        "cancelled",
    )
    assert tuple(group.value for group in StateGroup if group is not StateGroup.TRIAGE) == STATE_GROUP_ORDER
    assert tuple(state["group"] for state in DEFAULT_STATES if state["group"] != StateGroup.TRIAGE) == STATE_GROUP_ORDER


def test_test_group_is_open_and_active_but_not_closed():
    assert StateGroup.TEST in OPEN_STATE_GROUPS
    assert StateGroup.TEST in ACTIVE_STATE_GROUPS
    assert StateGroup.TEST not in CLOSED_STATE_GROUPS


@pytest.mark.parametrize("method", ["GET", "POST"])
def test_legacy_issue_type_filters_keep_test_issues_visible(method):
    all_filter = {}
    filter_issue_state_type({"type": "all"}, all_filter, method)
    assert all_filter["state__group__in"] == list(STATE_GROUP_ORDER)

    active_filter = {}
    filter_issue_state_type({"type": "active"}, active_filter, method)
    assert active_filter["state__group__in"] == list(ACTIVE_STATE_GROUPS)


def test_state_group_sorting_grouping_and_conversion_share_the_canonical_order():
    assert STATE_ORDER == list(STATE_GROUP_ORDER)
    assert issue_group_values("state__group", "workspace") == list(STATE_GROUP_ORDER)
    assert space_issue_group_values("state__group", "workspace") == list(STATE_GROUP_ORDER)
    assert LegacyToRichFiltersConverter.DEFAULT_VALID_CHOICES["state_group"] == list(STATE_GROUP_ORDER)


def test_advanced_search_treats_test_issues_as_open():
    assert IssueAdvancedSearchEndpoint._OPEN_STATE_GROUPS == OPEN_STATE_GROUPS
    assert IssueAdvancedSearchEndpoint._CLOSED_STATE_GROUPS == CLOSED_STATE_GROUPS
