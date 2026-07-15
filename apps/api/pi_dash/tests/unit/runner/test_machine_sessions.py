# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Machine-level control session endpoints.

Covers the open / delete / poll lifecycle plus the machine-token auth
boundary (a token bound to machine A must not drive machine B). Redis is
faked so no live server is needed; the async poll's Redis read is stubbed
to return no messages.
"""

from __future__ import annotations

import pytest
from django.urls import reverse
from django.utils import timezone

from pi_dash.runner.models import DevMachine, MachineSession, MachineToken
from pi_dash.runner.services import machine_outbox
from pi_dash.runner.services import tokens


class _FakeRedis:
    """Minimal Redis stand-in for the session endpoints' side effects."""

    def __init__(self):
        self.streams: dict[str, list] = {}
        self.kv: dict[str, str] = {}
        self._seq = 0

    def xgroup_create(self, **_kwargs):
        return True

    def xadd(self, key, fields, maxlen=None, approximate=None):
        self._seq += 1
        sid = f"{self._seq}-0".encode()
        self.streams.setdefault(key, []).append((sid, dict(fields)))
        return sid

    def xrange(self, key):
        return self.streams.get(key, [])

    def xack(self, *_args):
        return 0

    def delete(self, *keys):
        for k in keys:
            self.kv.pop(k, None)
            self.streams.pop(k, None)
        return 0

    def expire(self, *_args):
        return True

    def set(self, key, val, ex=None):
        self.kv[key] = val
        return True

    def exists(self, key):
        return 1 if key in self.kv else 0

    def publish(self, *_args):
        return 0


@pytest.fixture(autouse=True)
def _fake_redis(monkeypatch):
    """Route the machine outbox's Redis calls to a fake, and make the
    async poll read a no-op (returns no messages)."""
    client = _FakeRedis()
    monkeypatch.setattr(machine_outbox, "redis_instance", lambda: client)
    monkeypatch.setattr(
        "pi_dash.settings.redis.async_redis_instance", lambda: None
    )
    return client


@pytest.fixture
def dev_machine(create_user):
    return DevMachine.objects.create(owner=create_user, host_label="host-a")


@pytest.fixture
def machine_token(create_user, workspace, dev_machine):
    minted = tokens.mint_machine_token()
    MachineToken.objects.create(
        user=create_user,
        workspace=workspace,
        dev_machine=dev_machine,
        host_label="host-a",
        token_hash=minted.hashed,
        token_fingerprint=minted.fingerprint,
        label="machine: host-a",
        is_service=True,
    )
    return minted.raw


def _bearer(client, raw):
    client.credentials(HTTP_AUTHORIZATION=f"Bearer {raw}")
    return client


@pytest.mark.unit
def test_open_creates_machine_session(db, api_client, dev_machine, machine_token):
    _bearer(api_client, machine_token)
    url = reverse("runner:machine-session-open", kwargs={"dev_machine_id": dev_machine.id})
    resp = api_client.post(url, {}, format="json")
    assert resp.status_code == 201, resp.data
    assert resp.data["session_id"]
    assert resp.data["welcome"]["dev_machine_id"] == str(dev_machine.id)
    assert MachineSession.objects.filter(
        dev_machine=dev_machine, revoked_at__isnull=True
    ).count() == 1
    dev_machine.refresh_from_db()
    assert dev_machine.last_seen_at is not None


@pytest.mark.unit
def test_second_open_evicts_prior_session(db, api_client, dev_machine, machine_token):
    _bearer(api_client, machine_token)
    url = reverse("runner:machine-session-open", kwargs={"dev_machine_id": dev_machine.id})
    first = api_client.post(url, {}, format="json")
    second = api_client.post(url, {}, format="json")
    assert first.status_code == 201
    assert second.status_code == 201
    assert first.data["session_id"] != second.data["session_id"]
    # Only one active session; the prior is revoked.
    assert MachineSession.objects.filter(
        dev_machine=dev_machine, revoked_at__isnull=True
    ).count() == 1
    prior = MachineSession.objects.get(id=first.data["session_id"])
    assert prior.revoked_at is not None
    assert prior.revoked_reason == "evicted_by_new_session"


@pytest.mark.unit
def test_open_requires_auth(db, api_client, dev_machine):
    url = reverse("runner:machine-session-open", kwargs={"dev_machine_id": dev_machine.id})
    resp = api_client.post(url, {}, format="json")
    assert resp.status_code in (401, 403)
    assert not MachineSession.objects.filter(dev_machine=dev_machine).exists()


@pytest.mark.unit
def test_open_rejects_token_bound_to_other_machine(
    db, api_client, create_user, dev_machine, machine_token
):
    # A different machine owned by the same user, no token bound to it.
    other = DevMachine.objects.create(owner=create_user, host_label="host-b")
    _bearer(api_client, machine_token)  # token is bound to dev_machine (host-a)
    url = reverse("runner:machine-session-open", kwargs={"dev_machine_id": other.id})
    resp = api_client.post(url, {}, format="json")
    assert resp.status_code == 403
    assert not MachineSession.objects.filter(dev_machine=other).exists()


@pytest.mark.unit
def test_delete_revokes_session(db, api_client, dev_machine, machine_token):
    _bearer(api_client, machine_token)
    open_url = reverse("runner:machine-session-open", kwargs={"dev_machine_id": dev_machine.id})
    sid = api_client.post(open_url, {}, format="json").data["session_id"]
    del_url = reverse(
        "runner:machine-session-delete",
        kwargs={"dev_machine_id": dev_machine.id, "sid": sid},
    )
    resp = api_client.delete(del_url)
    assert resp.status_code == 204
    session = MachineSession.objects.get(id=sid)
    assert session.revoked_at is not None
    assert session.revoked_reason == "clean_shutdown"


@pytest.mark.unit
def test_poll_returns_messages_and_updates_presence(db, api_client, dev_machine, machine_token):
    _bearer(api_client, machine_token)
    open_url = reverse("runner:machine-session-open", kwargs={"dev_machine_id": dev_machine.id})
    sid = api_client.post(open_url, {}, format="json").data["session_id"]
    # Reset presence so we can prove the poll refreshes it.
    DevMachine.objects.filter(pk=dev_machine.id).update(last_seen_at=None)

    poll_url = reverse(
        "runner:machine-session-poll",
        kwargs={"dev_machine_id": dev_machine.id, "sid": sid},
    )
    resp = api_client.post(poll_url, {}, format="json")
    assert resp.status_code == 200, resp.content
    payload = resp.json()
    assert payload["messages"] == []
    assert "server_time" in payload
    dev_machine.refresh_from_db()
    assert dev_machine.last_seen_at is not None


@pytest.mark.unit
def test_poll_on_revoked_session_returns_409(db, api_client, dev_machine, machine_token):
    _bearer(api_client, machine_token)
    open_url = reverse("runner:machine-session-open", kwargs={"dev_machine_id": dev_machine.id})
    sid = api_client.post(open_url, {}, format="json").data["session_id"]
    MachineSession.objects.filter(id=sid).update(
        revoked_at=timezone.now(), revoked_reason="clean_shutdown"
    )
    poll_url = reverse(
        "runner:machine-session-poll",
        kwargs={"dev_machine_id": dev_machine.id, "sid": sid},
    )
    resp = api_client.post(poll_url, {}, format="json")
    assert resp.status_code == 409


@pytest.mark.unit
def test_poll_rejects_token_bound_to_other_machine(
    db, api_client, create_user, dev_machine, machine_token
):
    _bearer(api_client, machine_token)
    open_url = reverse("runner:machine-session-open", kwargs={"dev_machine_id": dev_machine.id})
    sid = api_client.post(open_url, {}, format="json").data["session_id"]
    other = DevMachine.objects.create(owner=create_user, host_label="host-c")
    poll_url = reverse(
        "runner:machine-session-poll",
        kwargs={"dev_machine_id": other.id, "sid": sid},
    )
    resp = api_client.post(poll_url, {}, format="json")
    assert resp.status_code == 403
