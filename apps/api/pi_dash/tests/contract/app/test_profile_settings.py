# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""``Profile.settings`` over the profile API.

The unit tests cover the seam's logic; these cover the thing that actually
protects the deployment — that the endpoint refuses undeclared content. With
``ProfileSerializer.fields = "__all__"`` the field is exposed the moment it
exists, so "the endpoint validates" is a property worth pinning rather than
assuming.
"""

from __future__ import annotations

import pytest

from pi_dash.db.models import Profile
from pi_dash.ee.settings import user_settings

pytestmark = [pytest.mark.django_db, pytest.mark.contract]

URL = "/api/users/me/profile/"


@pytest.fixture
def profile(create_user):
    return Profile.objects.get_or_create(user=create_user)[0]


def _declare(monkeypatch, schema):
    monkeypatch.setattr(user_settings, "known_settings_schema", lambda: schema)


def test_settings_defaults_to_an_empty_bag(session_client, profile):
    res = session_client.get(URL)
    assert res.status_code == 200
    assert res.data["settings"] == {}


def test_an_undeclared_namespace_is_rejected(session_client, profile):
    # CE declares nothing, so this is every write.
    res = session_client.patch(URL, {"settings": {"anything": {"k": "v"}}}, format="json")
    assert res.status_code == 400
    assert "settings" in res.data
    profile.refresh_from_db()
    assert profile.settings == {}


def test_a_declared_setting_is_stored(session_client, profile, monkeypatch):
    _declare(monkeypatch, {"openhub": {"apps_enabled": False}})
    res = session_client.patch(URL, {"settings": {"openhub": {"apps_enabled": True}}}, format="json")
    assert res.status_code == 200, res.data
    assert res.data["settings"] == {"openhub": {"apps_enabled": True}}
    profile.refresh_from_db()
    assert profile.settings == {"openhub": {"apps_enabled": True}}


def test_an_undeclared_key_is_rejected_and_stores_nothing(session_client, profile, monkeypatch):
    # Partial acceptance would be the worst outcome: the caller believes the
    # whole patch applied.
    _declare(monkeypatch, {"openhub": {"apps_enabled": False}})
    res = session_client.patch(
        URL, {"settings": {"openhub": {"apps_enabled": True, "smuggled": "x"}}}, format="json"
    )
    assert res.status_code == 400
    profile.refresh_from_db()
    assert profile.settings == {}


def test_patching_one_namespace_preserves_another(session_client, profile, monkeypatch):
    _declare(monkeypatch, {"a": {"x": 1}, "b": {"y": 2}})
    session_client.patch(URL, {"settings": {"a": {"x": 10}}}, format="json")
    res = session_client.patch(URL, {"settings": {"b": {"y": 20}}}, format="json")
    assert res.status_code == 200
    assert res.data["settings"] == {"a": {"x": 10}, "b": {"y": 20}}


def test_an_unrelated_patch_leaves_settings_untouched(session_client, profile, monkeypatch):
    _declare(monkeypatch, {"openhub": {"apps_enabled": False}})
    session_client.patch(URL, {"settings": {"openhub": {"apps_enabled": True}}}, format="json")

    res = session_client.patch(URL, {"language": "fr"}, format="json")
    assert res.status_code == 200
    assert res.data["settings"] == {"openhub": {"apps_enabled": True}}


def test_settings_cannot_be_written_through_the_serializer_directly(profile, monkeypatch):
    # The serializer marks the field read-only so any future endpoint reusing
    # it cannot expose an unvalidated whole-field write.
    from pi_dash.app.serializers.user import ProfileSerializer

    _declare(monkeypatch, {"openhub": {"apps_enabled": False}})
    serializer = ProfileSerializer(profile, data={"settings": {"junk": {"k": 1}}}, partial=True)
    assert serializer.is_valid(), serializer.errors
    serializer.save()
    profile.refresh_from_db()
    assert profile.settings == {}


def test_another_users_settings_are_not_reachable(session_client, profile, create_bot_user, monkeypatch):
    _declare(monkeypatch, {"openhub": {"apps_enabled": False}})
    other = Profile.objects.get_or_create(user=create_bot_user)[0]
    other.settings = {"openhub": {"apps_enabled": True}}
    other.save(update_fields=["settings"])

    # The endpoint is "me"-scoped: writes land on the caller's own row.
    session_client.patch(URL, {"settings": {"openhub": {"apps_enabled": False}}}, format="json")
    other.refresh_from_db()
    assert other.settings == {"openhub": {"apps_enabled": True}}


def test_the_merge_reads_under_a_row_lock(session_client, profile, monkeypatch):
    """Concurrency guard.

    The merge is a read-modify-write, so it must re-read the row under a lock
    rather than merging onto whatever was fetched at the top of the request —
    otherwise two requests patching different namespaces both merge onto the
    same stale bag and the second drops the first's namespace, which is the
    exact loss per-namespace merging exists to prevent.

    Asserted by observing the locked re-read, because the race itself cannot be
    reproduced deterministically in a single-threaded test.
    """
    _declare(monkeypatch, {"a": {"x": 1}, "b": {"y": 2}})

    locked_reads = []
    original = Profile.objects.select_for_update

    def spy(*args, **kwargs):
        locked_reads.append(True)
        return original(*args, **kwargs)

    monkeypatch.setattr(Profile.objects, "select_for_update", spy)

    res = session_client.patch(URL, {"settings": {"a": {"x": 10}}}, format="json")
    assert res.status_code == 200, res.data
    assert locked_reads, "settings merge must re-read the profile under select_for_update"


def test_a_concurrent_namespace_write_is_not_lost(session_client, profile, monkeypatch):
    """The property the lock protects, simulated at the seam it guards.

    Another writer commits between this request's initial fetch and its merge.
    Because the merge re-reads under the lock, that writer's namespace survives.
    """
    _declare(monkeypatch, {"a": {"x": 1}, "b": {"y": 2}})

    original_get = Profile.objects.select_for_update

    def racing_get(*args, **kwargs):
        # Simulate the other request landing first.
        Profile.objects.filter(pk=profile.pk).update(settings={"b": {"y": 99}})
        return original_get(*args, **kwargs)

    monkeypatch.setattr(Profile.objects, "select_for_update", racing_get)

    res = session_client.patch(URL, {"settings": {"a": {"x": 10}}}, format="json")
    assert res.status_code == 200, res.data
    assert res.data["settings"] == {"b": {"y": 99}, "a": {"x": 10}}
