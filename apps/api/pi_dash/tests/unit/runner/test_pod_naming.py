# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Tests for the pod-name validator (``services/pod_naming.py``).

Reflects the rules in
``.ai_design/n_runners_in_same_machine/new_pod_project_relationship/design.md``
§6.3.
"""

from __future__ import annotations

import pytest

from pi_dash.runner.services.pod_naming import (
    is_auto_default_name,
    required_prefix,
    validate_user_pod_name,
)


@pytest.mark.unit
def test_required_prefix_is_identifier_underscore():
    assert required_prefix("WEB") == "WEB_"
    assert required_prefix("MY-PROJ") == "MY-PROJ_"


@pytest.mark.unit
@pytest.mark.parametrize(
    "name",
    [
        "WEB_beefy",
        "WEB_us-east",
        "WEB_main.next",
        "WEB_a",  # one-char suffix is OK
    ],
)
def test_valid_user_names(name):
    assert validate_user_pod_name(name, "WEB") is None


@pytest.mark.unit
@pytest.mark.parametrize(
    "name,reason_substr",
    [
        ("", "required"),
        ("WEB_", "cannot be empty"),
        ("api_beefy", "must start with 'WEB_'"),
        ("WEB_$$$", "letters, digits"),
        ("WEB_pod_1", "reserved"),  # auto-default suffix collision
        ("WEB_pod_42", "reserved"),
        ("WEB_" + "x" * 200, "must be at most"),  # too long
    ],
)
def test_invalid_user_names(name, reason_substr):
    err = validate_user_pod_name(name, "WEB")
    assert err is not None
    assert reason_substr in err


@pytest.mark.unit
@pytest.mark.parametrize(
    "name,is_auto",
    [
        ("WEB_pod_1", True),
        ("WEB_pod_2", True),
        ("WEB_pod_99", True),
        ("WEB_pod_", False),  # missing digits
        ("WEB_beefy", False),
        ("WEB_pod_1a", False),  # extra suffix
        ("API_pod_1", False),  # right shape, wrong prefix for "WEB"
    ],
)
def test_is_auto_default_name(name, is_auto):
    assert is_auto_default_name(name, "WEB") is is_auto
