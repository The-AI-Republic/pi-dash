"""Cloud-agent creation-rate admission over direct runs (PIDASHCONV-1487).

``apps/api/pi_dash/cloud_agent/admission.py`` throttles cloud creations to 6
per user per minute and 30 per workspace per minute out of the shared
django-redis cache (keys ``:1:cloud-agent:admission:<scope>:<id>:<bucket>``);
the 7th request in a bucket answers 429 ``run_quota_exceeded`` and a cache
outage answers 503 ``admission_unavailable`` (fail closed,
``runner/views/runs.py``). The Rust port kept the check but every live
caller passed a fresh empty in-memory cache, so the throttle never fired.

Regime: both backends run with ``CLOUD_AGENT_ENABLED=true`` (the dispatch
suite regime, see ``__init__.py``). The world is one member user with a
BYOK marker row, one workspace, one cloud-default project and one pod;
seven direct ``POST /api/runners/runs/`` creates (no ``work_item``, so the
active-run guardrail stays out of the way) must yield six 201s and one
429 with the exact Django body, plus ``:1:``-prefixed bucket rows in
Redis. Fails on pre-fix Rust (seven 201s, nothing in Redis).

The 503 arm has no live case here: the shared contract Redis cannot be
stopped or paused per-test without disturbing other suites, so the
fail-closed behavior (unusable client or refused command -> Err ->
``admission_unavailable``) is pinned by Rust unit tests instead.
"""

from __future__ import annotations

import time

import pytest

from _harness import redis as redis_harness
from _harness.auth import login_session
from _harness.http import api_client
from _harness.web import web_post

from .conftest import create_llm_config

pytestmark = pytest.mark.contract

WEB_RUNS = "/api/runners/runs/"
ALLOW_RUNS = "GET, POST, HEAD, OPTIONS"

USER_LIMIT = 6
REQUESTS = USER_LIMIT + 1


def _await_bucket_margin(min_remaining_seconds: float = 30.0) -> None:
    """Start with a full minute bucket ahead so the slow boundary cannot split it."""
    remaining = 60 - (time.time() % 60)
    if remaining < min_remaining_seconds:
        time.sleep(remaining + 0.5)


def _admission_key_pattern(scope: str, owner_id: str) -> str:
    return f":1:cloud-agent:admission:{scope}:{owner_id}:*"


def _read_bucket(rdb, scope: str, owner_id: str) -> dict[str, str]:
    keys = list(rdb.scan_iter(_admission_key_pattern(scope, owner_id)))
    return {key: rdb.get(key) for key in keys}


def _clear_buckets(rdb, scope: str, owner_id: str) -> None:
    for key in rdb.scan_iter(_admission_key_pattern(scope, owner_id)):
        rdb.delete(key)
    for key in rdb.scan_iter(f"cloud-agent:admission:{scope}:{owner_id}:*"):
        rdb.delete(key)


def test_direct_create_enforces_user_creation_rate(settings, seeder, world):
    """Six direct cloud creates succeed; the 7th in the bucket 429s (PIDASHCONV-1487)."""
    owner_id = world["owner"]["id"]
    workspace_id = world["workspace"]["id"]
    seeder.db.execute(
        "UPDATE projects SET default_agent_executor = 'cloud_agent' WHERE id = %s",
        (world["project"]["id"],),
    )
    create_llm_config(seeder, owner_id)
    rdb = redis_harness.client()
    _clear_buckets(rdb, "user", owner_id)
    _clear_buckets(rdb, "workspace", workspace_id)

    created_run_ids: list[str] = []
    try:
        with api_client(settings.base_url) as client:
            login_session(
                client,
                email=world["owner"]["email"],
                password=world["owner"]["password"],
            )
            _await_bucket_margin()
            responses = [
                web_post(
                    client,
                    WEB_RUNS,
                    {
                        "prompt": f"admission probe {index}",
                        "workspace": workspace_id,
                        "pod": world["pod"]["id"],
                    },
                )
                for index in range(REQUESTS)
            ]
        for index, response in enumerate(responses[:USER_LIMIT]):
            assert response.status_code == 201, (index, response.text)
            assert response.headers["allow"] == ALLOW_RUNS
            body = response.json()
            assert body["executor_kind"] == "cloud_agent", body
            created_run_ids.append(body["id"])
        refused = responses[USER_LIMIT]
        assert refused.status_code == 429, refused.text
        assert refused.headers["allow"] == ALLOW_RUNS
        payload = refused.json()
        assert payload["error"] == "Cloud Agent creation rate exceeded", payload
        assert payload["code"] == "run_quota_exceeded", payload
        assert 1 <= payload["retry_after_seconds"] <= 60, payload
        assert set(payload) == {"error", "code", "retry_after_seconds"}, payload

        user_buckets = _read_bucket(rdb, "user", owner_id)
        assert len(user_buckets) == 1, user_buckets
        assert next(iter(user_buckets.values())) == str(USER_LIMIT), user_buckets
        workspace_buckets = _read_bucket(rdb, "workspace", workspace_id)
        assert len(workspace_buckets) == 1, workspace_buckets
        assert next(iter(workspace_buckets.values())) == str(USER_LIMIT), workspace_buckets
        # The bare (unprefixed) Django key must never be written: Rust and
        # Django share one bucket per scope/id, not one each.
        bare_user = list(rdb.scan_iter(f"cloud-agent:admission:user:{owner_id}:*"))
        bare_workspace = list(
            rdb.scan_iter(f"cloud-agent:admission:workspace:{workspace_id}:*")
        )
        assert bare_user == [], bare_user
        assert bare_workspace == [], bare_workspace
    finally:
        _clear_buckets(rdb, "user", owner_id)
        _clear_buckets(rdb, "workspace", workspace_id)
        if created_run_ids:
            seeder.db.execute(
                "DELETE FROM agent_run_event WHERE agent_run_id = ANY(%s)",
                (created_run_ids,),
            )
            seeder.db.execute(
                "DELETE FROM agent_run WHERE id = ANY(%s)", (created_run_ids,)
            )
