# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Fixtures for the engine identity / merge contract tests (PIDESKAPP-122).

Not autouse: ``test_engine_identity`` and ``test_engine_machine_merge`` opt
in with ``usefixtures("_isolate")``; other modules here are unaffected.
"""

from __future__ import annotations

from datetime import timedelta

import pytest
from django.utils import timezone
from rest_framework.test import APIClient

from pi_dash.db.models import Project, Workspace, WorkspaceMember
from pi_dash.runner.models import DevMachine, MachineToken
from pi_dash.runner.services import machine_outbox, tokens


class _FakeRedis:
    def __init__(self):
        self.streams: dict = {}
        self.kv: dict = {}
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

    def get(self, key):
        return self.kv.get(key)

    def exists(self, key):
        return 1 if key in self.kv else 0

    def publish(self, *_args):
        return 0


@pytest.fixture
def _isolate(monkeypatch):
    client = _FakeRedis()
    monkeypatch.setattr(machine_outbox, "redis_instance", lambda: client)
    monkeypatch.setattr("pi_dash.settings.redis.async_redis_instance", lambda: None)
    monkeypatch.setattr("pi_dash.runner.services.pubsub.send_to_runner", lambda *a, **k: None)
    # The device-flow endpoints are throttled per client address; every test
    # here calls them several times from the same test client.
    from django.core.cache import cache

    cache.clear()
    return client


@pytest.fixture
def lane(monkeypatch):
    """The cloud overlay's OpenHub lane, via the seam it replaces."""
    from pi_dash.ee.assistant import model_provider

    state = {"available": True, "raise": None}

    def _profile(user):
        if state["available"]:
            return model_provider.AgentModelProfile(
                available=True, lane="openhub", base_url="https://gw.example.com/v1", model="m-1"
            )
        return model_provider.AgentModelProfile(available=False, reason_code="llm_config_missing")

    def _credential(user):
        if state["raise"] is not None:
            raise state["raise"]
        if not state["available"]:
            from pi_dash.managed_runner.errors import ManagedRunnerUnavailable

            raise ManagedRunnerUnavailable("llm_config_missing")
        return "gw-secret-token", timezone.now() + timedelta(minutes=10)

    monkeypatch.setattr(model_provider, "agent_model_profile_for_user", _profile)
    monkeypatch.setattr(model_provider, "agent_model_credential_for_user", _credential)
    return state


def _client(bearer=None, api_key=None, user=None):
    client = APIClient()
    if bearer:
        client.credentials(HTTP_AUTHORIZATION=f"Bearer {bearer}")
    if api_key:
        client.credentials(HTTP_X_API_KEY=api_key)
    if user is not None:
        client.force_authenticate(user=user)
    return client


@pytest.fixture
def web(create_user):
    return _client(user=create_user)


@pytest.fixture
def machine(create_user):
    return DevMachine.objects.create(owner=create_user, host_label="rich-mbp", label="Rich MBP")


def _mint_mt(user, workspace, machine):
    minted = tokens.mint_machine_token()
    MachineToken.objects.create(
        user=user,
        workspace=workspace,
        dev_machine=machine,
        host_label=machine.host_label,
        token_hash=minted.hashed,
        token_fingerprint=minted.fingerprint,
        is_service=True,
    )
    return minted.raw


@pytest.fixture
def mt(create_user, workspace, machine):
    return _mint_mt(create_user, workspace, machine)


@pytest.fixture
def workspace_b(create_user):
    ws = Workspace.objects.create(name="Second", owner=create_user, slug="second-ws")
    WorkspaceMember.objects.create(workspace=ws, member=create_user, role=20)
    # Same project identifier as the first workspace's default project.
    Project.objects.create(name="Second Default", identifier="DEF", workspace=ws, created_by=create_user)
    return ws
