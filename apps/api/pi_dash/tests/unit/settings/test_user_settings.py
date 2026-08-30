# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""The namespaced user-settings seam.

The property worth protecting: ``Profile.settings`` is writable through the
profile API by every authenticated user, so it must accept *only* what the
running build declares. An unvalidated bag is an unbounded per-user JSON store,
not a settings field.
"""

from __future__ import annotations

import pytest

from pi_dash.ee.settings import user_settings

pytestmark = pytest.mark.unit


def _declare(monkeypatch, schema):
    monkeypatch.setattr(user_settings, "known_settings_schema", lambda: schema)


class _Profile:
    def __init__(self, settings=None):
        self.settings = settings


# --------------------------------------------------------------------------- #
# CE declares nothing
# --------------------------------------------------------------------------- #


def test_ce_declares_no_namespaces():
    # The bag exists but nothing may go in it until a build says otherwise.
    assert user_settings.known_settings_schema() == {}


def test_every_write_is_refused_when_nothing_is_declared():
    with pytest.raises(ValueError):
        user_settings.validate_settings_patch({"openhub": {"apps_enabled": True}})


def test_an_empty_patch_is_accepted():
    assert user_settings.validate_settings_patch({}) == {}


# --------------------------------------------------------------------------- #
# Validation is a closed allow-list
# --------------------------------------------------------------------------- #


def test_a_declared_namespace_and_key_round_trips(monkeypatch):
    _declare(monkeypatch, {"openhub": {"apps_enabled": False}})
    assert user_settings.validate_settings_patch({"openhub": {"apps_enabled": True}}) == {
        "openhub": {"apps_enabled": True}
    }


def test_an_undeclared_namespace_is_refused(monkeypatch):
    _declare(monkeypatch, {"openhub": {"apps_enabled": False}})
    with pytest.raises(ValueError, match="unknown settings namespace"):
        user_settings.validate_settings_patch({"attacker": {"blob": "x" * 100}})


def test_an_undeclared_key_inside_a_known_namespace_is_refused(monkeypatch):
    # The namespace being legitimate must not make it an open bucket.
    _declare(monkeypatch, {"openhub": {"apps_enabled": False}})
    with pytest.raises(ValueError, match="unknown settings key"):
        user_settings.validate_settings_patch({"openhub": {"apps_enabled": True, "junk": 1}})


@pytest.mark.parametrize("bad", ["a string", 42, ["a", "list"], None])
def test_a_non_object_payload_is_refused(bad):
    with pytest.raises(ValueError, match="must be an object"):
        user_settings.validate_settings_patch(bad)


def test_a_non_object_namespace_value_is_refused(monkeypatch):
    _declare(monkeypatch, {"openhub": {"apps_enabled": False}})
    with pytest.raises(ValueError, match="must be an object"):
        user_settings.validate_settings_patch({"openhub": "yes"})


# --------------------------------------------------------------------------- #
# Merge semantics
# --------------------------------------------------------------------------- #


def test_patching_one_namespace_leaves_the_others_intact():
    # The failure this guards: two surfaces patching different namespaces and
    # silently dropping each other's values.
    stored = {"openhub": {"apps_enabled": True}, "other": {"k": 1}}
    merged = user_settings.merge_settings(stored, {"other": {"k": 2}})
    assert merged == {"openhub": {"apps_enabled": True}, "other": {"k": 2}}


def test_merging_into_an_empty_bag_works():
    assert user_settings.merge_settings(None, {"openhub": {"apps_enabled": True}}) == {
        "openhub": {"apps_enabled": True}
    }


def test_merge_does_not_mutate_the_stored_bag():
    stored = {"openhub": {"apps_enabled": False}}
    user_settings.merge_settings(stored, {"openhub": {"apps_enabled": True}})
    assert stored == {"openhub": {"apps_enabled": False}}


def test_a_corrupt_namespace_value_is_dropped_rather_than_crashing():
    # Hand-edited or legacy rows must not break every subsequent write.
    merged = user_settings.merge_settings({"bad": "not-a-dict"}, {"openhub": {"apps_enabled": True}})
    assert merged == {"openhub": {"apps_enabled": True}}


# --------------------------------------------------------------------------- #
# Reads fall back to declared defaults
# --------------------------------------------------------------------------- #


def test_an_unset_key_reads_as_its_declared_default(monkeypatch):
    _declare(monkeypatch, {"openhub": {"apps_enabled": False}})
    assert user_settings.get_setting(_Profile({}), "openhub", "apps_enabled") is False


def test_a_stored_value_wins_over_the_default(monkeypatch):
    _declare(monkeypatch, {"openhub": {"apps_enabled": False}})
    profile = _Profile({"openhub": {"apps_enabled": True}})
    assert user_settings.get_setting(profile, "openhub", "apps_enabled") is True


def test_a_stored_false_is_not_mistaken_for_unset(monkeypatch):
    # `in` rather than truthiness: an explicit False must not fall back to a
    # default that happens to be True.
    _declare(monkeypatch, {"openhub": {"apps_enabled": True}})
    profile = _Profile({"openhub": {"apps_enabled": False}})
    assert user_settings.get_setting(profile, "openhub", "apps_enabled") is False


def test_reading_an_undeclared_key_is_none_not_an_error(monkeypatch):
    _declare(monkeypatch, {"openhub": {"apps_enabled": False}})
    assert user_settings.get_setting(_Profile({}), "nope", "whatever") is None


def test_a_missing_bag_reads_as_defaults(monkeypatch):
    _declare(monkeypatch, {"openhub": {"apps_enabled": False}})
    assert user_settings.get_setting(_Profile(None), "openhub", "apps_enabled") is False
