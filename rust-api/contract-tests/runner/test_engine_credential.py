# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Built-in engine credential domain (NEWPIDASH-50, #2910/#2912).

Black-box ports of the Django oracle cases in
``apps/api/pi_dash/tests/contract/runner/test_engine_identity.py``
(profile / model-token / revoke / heartbeat / legacy / machine-token
expiry / one-session / sign-out) that are observable over HTTP in CE:

- The suite env leaves ``MANAGED_RUNNER_ENABLED`` at its default ``false``
  on both halves, so the engine profile always reports
  ``engine_disabled`` and the model-token endpoint only pins the
  engine-disabled 409 arm plus the dead-credential 401 arms. The
  gateway / wallet / transient / successful-mint arms need the cloud
  overlay's OpenHub lane (Django ``lane`` monkeypatch) and are covered
  by the Rust unit tests instead — there is no gateway HTTP to stub in
  CE (the seam is in-process and always unavailable; see module notes
  in the originating issue's workpad).

Engine credentials are seeded straight into Postgres (hash with
``_harness/tokens.py``, the same as machine tokens) because credential
issuance belongs to the device-flow issue (NEWPIDASH-51); the machine's
stored sign-in fields are set to what ``issue_engine_credential`` would
have written (state ``active`` + the credential's expiry).

Workspace fixtures come from the runner suite's ``conftest.py``; every
test uses its own world, so throttle counters (keyed by credential
hash) and rows never collide between tests.
"""

from __future__ import annotations

import os
import secrets
import uuid

import pytest

from _harness import rust_queue
from _harness.celery import Broker, get_broker_url
from _harness.db import wait_for_condition
from _harness.http import api_key_client, bearer_client
from _harness.tokens import fingerprint, hash_token
from _harness.web import web_delete, web_post

from .conftest import DAEMON
from .test_daemon_sessions import _machine_ids, _post_poll

pytestmark = pytest.mark.contract

ENGINE = "/api/v1/runner/engine"
WEB = "/api/runners"

PROFILE_KEYS = [
    "available",
    "lane",
    "base_url",
    "model",
    "reason_code",
    "pause_reason",
    "top_up_url",
    "engine_sign_in_state",
    "engine_credential_expires_at",
]

SIGN_IN_AGAIN_DETAIL = "Sign the built-in engine in again (`pidash login --engine`)."


# -- seeding helpers -------------------------------------------------------


def _mint_engine_credential(
    seeder,
    user_id: str,
    machine_id: str,
    raw: str,
    *,
    days: float = 14,
    revoked_reason: str | None = None,
    state: str = "active",
):
    """Insert one ``engine_credential`` row and set the machine as issuance
    would (``engine_sign_in_state`` + ``engine_credential_expires_at``)."""
    cred_id = str(uuid.uuid4())
    seeder.db.execute(
        """INSERT INTO engine_credential
           (id, user_id, dev_machine_id, token_hash, fingerprint,
            created_at, expires_at, revoked_at, revoked_reason)
           VALUES (%s,%s,%s,%s,%s, now(), now() + (%s || ' days')::interval,
                   %s, %s)""",
        (
            cred_id,
            user_id,
            machine_id,
            hash_token(raw, seeder.secret_key),
            fingerprint(raw),
            str(days),
            "now()" if revoked_reason else None,
            revoked_reason or "",
        ),
    )
    seeder.track.add("engine_credential", cred_id)
    seeder.db.execute(
        "UPDATE dev_machine SET engine_sign_in_state = %s,"
        " engine_credential_expires_at = now() + (%s || ' days')::interval"
        " WHERE id = %s",
        (state, str(days), machine_id),
    )
    return {"id": cred_id, "raw": raw}


def _credential_row(seeder, cred_id: str) -> dict:
    return seeder.db.fetchone(
        "SELECT id, user_id, dev_machine_id, token_hash, fingerprint,"
        " last_used_at, revoked_at, revoked_reason, expires_at"
        " FROM engine_credential WHERE id = %s",
        (cred_id,),
    )


def _machine_engine_row(seeder, machine_id: str) -> dict:
    return seeder.db.fetchone(
        "SELECT engine_sign_in_state, engine_credential_expires_at,"
        " engine_state, engine_state_reported_at, engine_auto_pause_reason"
        " FROM dev_machine WHERE id = %s",
        (machine_id,),
    )


def _set_pause(seeder, machine_id: str, reason: str):
    seeder.db.execute(
        "UPDATE dev_machine SET engine_auto_pause_reason = %s WHERE id = %s",
        (reason, machine_id),
    )


def _machine_id(user_client, daemon_world, machine_flow) -> str:
    response = user_client.get(
        f"{WEB}/dev-machines/", params={"workspace": daemon_world["workspace"]["id"]}
    )
    assert response.status_code == 200, response.text
    for machine in response.json():
        if machine["host_label"] == machine_flow["host_label"]:
            return machine["id"]
    raise AssertionError(f"flow machine {machine_flow['host_label']} missing from list")


def _open_machine_session(client, machine_id: str) -> str:
    response = client.post(f"{DAEMON}/dev-machines/{machine_id}/sessions/", json={})
    assert response.status_code == 201, response.text
    return response.json()["session_id"]


def _poll_machine(client, machine_id: str, session_id: str, payload: dict):
    # ``_post_poll`` absorbs the runserver event-loop 500 flake (see its
    # docstring); the shape assertions below pin the real envelope.
    return _post_poll(
        client,
        f"{DAEMON}/dev-machines/{machine_id}/sessions/{session_id}/poll",
        payload,
    )


# -- profile ---------------------------------------------------------------


def test_profile_endpoint_shape(settings, seeder, daemon_world, machine_flow):
    machine_id = _machine_ids(seeder, machine_flow)
    with bearer_client(settings.base_url, machine_flow["machine_token"]) as client:
        response = client.get(f"{ENGINE}/profile/")
    assert response.status_code == 200, response.text
    assert response.headers["allow"] == "GET, HEAD, OPTIONS"
    body = response.json()
    # Django dict order; the instance kill switch is off in this env.
    assert list(body.keys()) == PROFILE_KEYS, body
    assert body["available"] is False
    assert body["reason_code"] == "engine_disabled"
    assert body["pause_reason"] == "engine_disabled"
    assert body["lane"] == ""
    assert body["base_url"] == ""
    assert body["model"] == ""
    assert body["top_up_url"] == ""
    assert body["engine_sign_in_state"] == "never"
    assert body["engine_credential_expires_at"] is None
    # The profile outcome pauses the machine (a new-path machine).
    assert _machine_engine_row(seeder, machine_id)["engine_auto_pause_reason"] == "engine_disabled"


def test_profile_endpoint_reports_sign_in_state(settings, seeder, daemon_world, machine_flow):
    machine_id = _machine_ids(seeder, machine_flow)
    raw = "ec_" + secrets.token_urlsafe(32)
    _mint_engine_credential(seeder, daemon_world["owner"]["id"], machine_id, raw)
    with bearer_client(settings.base_url, machine_flow["machine_token"]) as client:
        body = client.get(f"{ENGINE}/profile/").json()
    assert body["engine_sign_in_state"] == "active"
    assert body["engine_credential_expires_at"] is not None
    assert body["available"] is False  # kill switch off: still engine_disabled
    assert body["reason_code"] == "engine_disabled"


def test_profile_endpoint_expiring_and_expired_states(settings, seeder, daemon_world, machine_flow):
    machine_id = _machine_ids(seeder, machine_flow)
    raw = "ec_" + secrets.token_urlsafe(32)
    _mint_engine_credential(seeder, daemon_world["owner"]["id"], machine_id, raw, days=1)
    with bearer_client(settings.base_url, machine_flow["machine_token"]) as client:
        assert client.get(f"{ENGINE}/profile/").json()["engine_sign_in_state"] == "expiring"
    seeder.db.execute(
        "UPDATE engine_credential SET expires_at = now() - interval '1 minute'"
        " WHERE dev_machine_id = %s",
        (machine_id,),
    )
    seeder.db.execute(
        "UPDATE dev_machine SET engine_credential_expires_at = now() - interval '1 minute'"
        " WHERE id = %s",
        (machine_id,),
    )
    with bearer_client(settings.base_url, machine_flow["machine_token"]) as client:
        assert client.get(f"{ENGINE}/profile/").json()["engine_sign_in_state"] == "expired"


def test_profile_endpoint_requires_a_machine_bound_token(
    settings, seeder, daemon_world, machine_flow, anon_client
):
    # No Bearer [REDACTED]: machine-token auth returns None, so the view
    # itself refuses with 403 dev_machine_required (never 401 — nothing
    # raised AuthenticationFailed).
    no_token = anon_client.get(f"{ENGINE}/profile/")
    assert no_token.status_code == 403, no_token.text
    assert no_token.json() == {
        "error": "dev_machine_required",
        "detail": "This machine token is not bound to a computer.",
    }
    # An unknown mt_ value fails the resolver: DRF's 401 with the code.
    with bearer_client(settings.base_url, "mt_" + uuid.uuid4().hex) as client:
        unknown = client.get(f"{ENGINE}/profile/")
    assert unknown.status_code == 401, unknown.text
    assert unknown.json() == {"detail": "machine_token_invalid"}
    # A known token bound to no machine: the view's own 403.
    unbound = seeder.create_machine_token(
        daemon_world["owner"]["id"], daemon_world["workspace"]["id"],
        "mt_" + secrets.token_urlsafe(24),
    )
    with bearer_client(settings.base_url, unbound["token"]) as client:
        response = client.get(f"{ENGINE}/profile/")
    assert response.status_code == 403, response.text
    assert response.json() == {
        "error": "dev_machine_required",
        "detail": "This machine token is not bound to a computer.",
    }


def test_profile_endpoint_legacy_machine_skips_the_pause_apply(
    settings, seeder, daemon_world,
):
    """A desktop-bundled machine never gets the pause applied from the
    profile outcome, even though the body computes the kill-switch
    reason."""
    machine = seeder.create_dev_machine(
        daemon_world["owner"]["id"], provisioning="desktop_bundled"
    )
    raw = "mt_" + secrets.token_urlsafe(24)
    seeder.create_machine_token(
        daemon_world["owner"]["id"], daemon_world["workspace"]["id"],
        raw, dev_machine_id=machine["id"],
    )
    _set_pause(seeder, machine["id"], "wallet_empty")
    with bearer_client(settings.base_url, raw) as client:
        response = client.get(f"{ENGINE}/profile/")
    assert response.status_code == 200, response.text
    assert response.json()["pause_reason"] == "engine_disabled"
    # The stored pause is untouched on a legacy machine.
    assert _machine_engine_row(seeder, machine["id"])["engine_auto_pause_reason"] == "wallet_empty"


# -- model-token -----------------------------------------------------------


def test_model_token_accepts_an_engine_credential_only(
    settings, seeder, daemon_world, machine_flow, anon_client
):
    # An mt_ Bearer [REDACTED] an engine credential: 401 immediately.
    with bearer_client(settings.base_url, machine_flow["machine_token"]) as client:
        response = client.post(f"{ENGINE}/model-token/", json={})
    assert response.status_code == 401, response.text
    assert response.json() == {
        "reason_code": "engine_sign_in_required",
        "error": "engine_sign_in_required",
        "detail": SIGN_IN_AGAIN_DETAIL,
    }
    # No Bearer [REDACTED]: same 401 (there are no DRF auth classes on
    # this endpoint; the credential is the auth).
    assert anon_client.post(f"{ENGINE}/model-token/", json={}).status_code == 401
    # An unknown ec_ value: same shape.
    with bearer_client(settings.base_url, "ec_" + secrets.token_urlsafe(32)) as client:
        unknown = client.post(f"{ENGINE}/model-token/", json={})
    assert unknown.status_code == 401
    assert unknown.json()["reason_code"] == "engine_sign_in_required"


def test_model_token_disabled_engine_pauses(settings, seeder, daemon_world, machine_flow):
    """The kill switch off (this env's default): a live credential gets
    409 engine_disabled with the "turned off" detail, and the machine is
    paused. This is the reason→status row CE can reach black-box; the
    gateway/wallet/transient arms need the overlay lane."""
    machine_id = _machine_ids(seeder, machine_flow)
    raw = "ec_" + secrets.token_urlsafe(32)
    _mint_engine_credential(seeder, daemon_world["owner"]["id"], machine_id, raw)
    with bearer_client(settings.base_url, raw) as client:
        response = client.post(f"{ENGINE}/model-token/", json={})
    assert response.status_code == 409, response.text
    assert response.json() == {
        "reason_code": "engine_disabled",
        "detail": "The engine is turned off.",
    }
    assert _machine_engine_row(seeder, machine_id)["engine_auto_pause_reason"] == "engine_disabled"
    # The credential still counts as used.
    row = _credential_row(seeder, _credential_id_for(seeder, machine_id))
    assert row["last_used_at"] is not None


def _credential_id_for(seeder, machine_id: str) -> str:
    return seeder.db.fetchval(
        "SELECT id FROM engine_credential WHERE dev_machine_id = %s AND revoked_at IS NULL",
        (machine_id,),
    )


@pytest.mark.parametrize("kind", ["expired", "revoked"])
def test_model_token_dead_credential_pauses_with_sign_in_required(
    settings, seeder, daemon_world, machine_flow, kind,
):
    machine_id = _machine_ids(seeder, machine_flow)
    raw = "ec_" + secrets.token_urlsafe(32)
    _mint_engine_credential(
        seeder, daemon_world["owner"]["id"], machine_id, raw,
        revoked_reason=None if kind == "expired" else "admin",
    )
    if kind == "expired":
        seeder.db.execute(
            "UPDATE engine_credential SET expires_at = now() - interval '1 second'"
            " WHERE dev_machine_id = %s",
            (machine_id,),
        )
    with bearer_client(settings.base_url, raw) as client:
        response = client.post(f"{ENGINE}/model-token/", json={})
    assert response.status_code == 401, response.text
    assert response.json() == {
        "reason_code": "engine_sign_in_required",
        "error": "engine_sign_in_required",
        "detail": SIGN_IN_AGAIN_DETAIL,
    }
    assert (
        _machine_engine_row(seeder, machine_id)["engine_auto_pause_reason"]
        == "engine_sign_in_required"
    )


def test_model_token_superseded_credential_does_not_pause_the_replacement(
    settings, seeder, daemon_world, machine_flow,
):
    """An old (superseded) credential is still 401 — but its machine's
    healthy replacement keeps running, so nothing is paused."""
    machine_id = _machine_ids(seeder, machine_flow)
    old_raw = "ec_" + secrets.token_urlsafe(32)
    new_raw = "ec_" + secrets.token_urlsafe(32)
    _mint_engine_credential(
        seeder, daemon_world["owner"]["id"], machine_id, old_raw,
        revoked_reason="superseded",
    )
    _mint_engine_credential(seeder, daemon_world["owner"]["id"], machine_id, new_raw)
    with bearer_client(settings.base_url, old_raw) as client:
        stale = client.post(f"{ENGINE}/model-token/", json={})
    assert stale.status_code == 401
    assert stale.json()["reason_code"] == "engine_sign_in_required"
    assert _machine_engine_row(seeder, machine_id)["engine_auto_pause_reason"] == ""
    # The live replacement still reaches the engine ("engine_disabled"
    # here, never "engine_sign_in_required").
    with bearer_client(settings.base_url, new_raw) as client:
        alive = client.post(f"{ENGINE}/model-token/", json={})
    assert alive.status_code == 409
    assert alive.json()["reason_code"] == "engine_disabled"


def test_model_token_is_throttled_per_credential(
    settings, seeder, daemon_world, machine_flow,
):
    """30/minute per engine credential (DRF per-presenter bucket); the
    31st request answers the DRF Throttled rewrite."""
    machine_id = _machine_ids(seeder, machine_flow)
    raw = "ec_" + secrets.token_urlsafe(32)
    _mint_engine_credential(seeder, daemon_world["owner"]["id"], machine_id, raw)
    with bearer_client(settings.base_url, raw) as client:
        for _ in range(30):
            assert client.post(f"{ENGINE}/model-token/", json={}).status_code == 409
        throttled = client.post(f"{ENGINE}/model-token/", json={})
    assert throttled.status_code == 429, throttled.text
    assert throttled.json() == {
        "error_code": 5900,
        "error_message": "RATE_LIMIT_EXCEEDED",
    }


# -- credential revoke -----------------------------------------------------


def test_engine_credential_revoke_is_idempotent(
    settings, seeder, daemon_world, machine_flow,
):
    machine_id = _machine_ids(seeder, machine_flow)
    raw = "ec_" + secrets.token_urlsafe(32)
    cred = _mint_engine_credential(seeder, daemon_world["owner"]["id"], machine_id, raw)
    with bearer_client(settings.base_url, machine_flow["machine_token"]) as client:
        first = client.post(f"{ENGINE}/credential/revoke/", json={})
        second = client.post(f"{ENGINE}/credential/revoke/", json={})
    assert first.status_code == second.status_code == 200, (first.text, second.text)
    assert first.json()["ok"] is True and first.json()["revoked"] is True
    assert second.json()["ok"] is True and second.json()["revoked"] is False
    assert first.json()["engine_sign_in_state"] == "signed_out"
    assert first.json()["engine_credential_expires_at"] is None
    row = _credential_row(seeder, cred["id"])
    assert row["revoked_reason"] == "signed_out"
    assert row["revoked_at"] is not None
    # The revoked credential no longer serves.
    with bearer_client(settings.base_url, raw) as client:
        assert client.post(f"{ENGINE}/model-token/", json={}).status_code == 401


def test_engine_credential_revoke_requires_a_machine_bound_token(
    settings, seeder, daemon_world, machine_flow, anon_client
):
    # No Bearer [REDACTED]: machine-token auth returns None, so the view
    # itself refuses with 403 dev_machine_required.
    no_token = anon_client.post(f"{ENGINE}/credential/revoke/", json={})
    assert no_token.status_code == 403, no_token.text
    assert no_token.json() == {
        "error": "dev_machine_required",
        "detail": "This machine token is not bound to a computer.",
    }
    unbound = seeder.create_machine_token(
        daemon_world["owner"]["id"], daemon_world["workspace"]["id"],
        "mt_" + secrets.token_urlsafe(24),
    )
    with bearer_client(settings.base_url, unbound["token"]) as client:
        response = client.post(f"{ENGINE}/credential/revoke/", json={})
    assert response.status_code == 403, response.text
    assert response.json() == {
        "error": "dev_machine_required",
        "detail": "This machine token is not bound to a computer.",
    }


def test_one_live_engine_credential_per_machine_is_enforced_by_the_database(
    settings, seeder, daemon_world, machine_flow,
):
    """The partial unique index ``engine_credential_one_live_per_machine``
    (one unrevoked row per dev_machine) is the law: the service relies
    on it, so a second live row must fail at the database."""
    import psycopg

    machine_id = _machine_ids(seeder, machine_flow)
    raw = "ec_" + secrets.token_urlsafe(32)
    _mint_engine_credential(seeder, daemon_world["owner"]["id"], machine_id, raw)
    second = "ec_" + secrets.token_urlsafe(32)
    with psycopg.connect(settings.database_url) as conn:
        with conn.cursor() as cur:
            with pytest.raises(psycopg.errors.UniqueViolation):
                cur.execute(
                    """INSERT INTO engine_credential
                       (id, user_id, dev_machine_id, token_hash, fingerprint,
                        created_at, expires_at, revoked_reason)
                       VALUES (%s,%s,%s,%s,%s, now(), now() + interval '14 days', '')""",
                    (
                        str(uuid.uuid4()),
                        daemon_world["owner"]["id"],
                        machine_id,
                        hash_token(second, seeder.secret_key),
                        fingerprint(second),
                    ),
                )
        conn.rollback()


# -- web sign-out + machine removal cascades -------------------------------


def test_remove_this_computer_revokes_the_engine_credential(
    settings, seeder, daemon_world, machine_flow, user_client,
):
    """Machine revoke (``POST dev-machines/{id}/revoke/``) revokes the
    engine credential with reason ``machine_removed`` (state
    ``revoked``)."""
    machine_id = _machine_id(user_client, daemon_world, machine_flow)
    raw = "ec_" + secrets.token_urlsafe(32)
    cred = _mint_engine_credential(seeder, daemon_world["owner"]["id"], machine_id, raw)
    response = web_post(
        user_client,
        f"{WEB}/dev-machines/{machine_id}/revoke/",
        {"workspace": daemon_world["workspace"]["id"]},
    )
    assert response.status_code == 200, response.text
    row = _credential_row(seeder, cred["id"])
    assert row["revoked_reason"] == "machine_removed"
    machine = _machine_engine_row(seeder, machine_id)
    assert machine["engine_sign_in_state"] == "revoked"
    with bearer_client(settings.base_url, raw) as client:
        assert client.post(f"{ENGINE}/model-token/", json={}).status_code == 401


def test_machine_delete_revokes_the_engine_credential(
    settings, seeder, daemon_world, machine_flow, user_client,
):
    """Machine delete cascades the credential rows with the machine (the
    revoke runs first, so no live credential survives the delete)."""
    machine_id = _machine_id(user_client, daemon_world, machine_flow)
    raw = "ec_" + secrets.token_urlsafe(32)
    _mint_engine_credential(seeder, daemon_world["owner"]["id"], machine_id, raw)
    response = web_delete(
        user_client,
        f"{WEB}/dev-machines/{machine_id}/",
        params={"workspace": daemon_world["workspace"]["id"]},
    )
    assert response.status_code == 204, response.text
    assert (
        seeder.db.fetchval(
            "SELECT COUNT(*) FROM engine_credential WHERE dev_machine_id = %s",
            (machine_id,),
        )
        == 0
    )
    assert (
        seeder.db.fetchval("SELECT COUNT(*) FROM dev_machine WHERE id = %s", (machine_id,))
        == 0
    )


def test_web_sign_out_the_engine(settings, seeder, daemon_world, machine_flow, user_client):
    machine_id = _machine_id(user_client, daemon_world, machine_flow)
    raw = "ec_" + secrets.token_urlsafe(32)
    cred = _mint_engine_credential(seeder, daemon_world["owner"]["id"], machine_id, raw)
    other_ws = seeder.create_workspace(daemon_world["owner"]["id"])
    seeder.create_member(other_ws["id"], daemon_world["owner"]["id"])
    # A workspace the machine is not attached to does not see it.
    foreign = web_post(
        user_client,
        f"{WEB}/dev-machines/{machine_id}/engine-sign-out/",
        {"workspace": other_ws["id"]},
    )
    assert foreign.status_code == 404, foreign.text
    assert foreign.json() == {"error": "not found"}
    response = web_post(
        user_client,
        f"{WEB}/dev-machines/{machine_id}/engine-sign-out/",
        {"workspace": daemon_world["workspace"]["id"]},
    )
    assert response.status_code == 200, response.text
    body = response.json()
    assert body["engine_sign_in_state"] == "signed_out"
    row = _credential_row(seeder, cred["id"])
    assert row["revoked_reason"] == "signed_out"
    assert _machine_engine_row(seeder, machine_id)["engine_sign_in_state"] == "signed_out"
    with bearer_client(settings.base_url, raw) as client:
        assert client.post(f"{ENGINE}/model-token/", json={}).status_code == 401


def test_web_sign_out_the_engine_denies_a_foreign_machine(
    settings, seeder, daemon_world, user_client,
):
    """The machine must be the caller's: another owner's machine is a
    404, never a revoke."""
    other_owner = seeder.create_user()
    other_ws = seeder.create_workspace(other_owner["id"])
    seeder.create_member(other_ws["id"], other_owner["id"])
    foreign_machine = seeder.create_dev_machine(other_owner["id"])
    raw = "ec_" + secrets.token_urlsafe(32)
    cred = _mint_engine_credential(seeder, other_owner["id"], foreign_machine["id"], raw)
    response = web_post(
        user_client,
        f"{WEB}/dev-machines/{foreign_machine['id']}/engine-sign-out/",
        {"workspace": daemon_world["workspace"]["id"]},
    )
    assert response.status_code == 404, response.text
    assert _credential_row(seeder, cred["id"])["revoked_at"] is None


# -- heartbeat engine_state (machine-session poll) -------------------------


def test_heartbeat_engine_state_drives_automatic_pause(
    settings, seeder, daemon_world, machine_flow,
):
    machine_id = _machine_ids(seeder, machine_flow)
    raw = "ec_" + secrets.token_urlsafe(32)
    _mint_engine_credential(seeder, daemon_world["owner"]["id"], machine_id, raw)
    with bearer_client(settings.base_url, machine_flow["machine_token"], timeout=40.0) as client:
        session_id = _open_machine_session(client, machine_id)
        response = _poll_machine(
            client, machine_id, session_id,
            {"engine_state": {
                "state": "unavailable",
                "reason_code": "wallet_empty",
                "sign_in_state": "active",
            }},
        )
        assert response.status_code == 200, response.text
        machine = _machine_engine_row(seeder, machine_id)
        assert machine["engine_state"]["reason_code"] == "wallet_empty"
        assert machine["engine_state"]["sign_in_state"] == "active"
        assert machine["engine_state_reported_at"] is not None
        assert machine["engine_auto_pause_reason"] == "wallet_empty"

        _poll_machine(
            client, machine_id, session_id,
            {"engine_state": {"state": "available"}},
        )
        assert _machine_engine_row(seeder, machine_id)["engine_auto_pause_reason"] == ""

        # Daemon-local codes are stored, not applied.
        _poll_machine(
            client, machine_id, session_id,
            {"engine_state": {"state": "unavailable", "reason_code": "engine_not_installed"}},
        )
        machine = _machine_engine_row(seeder, machine_id)
        assert machine["engine_state"]["reason_code"] == "engine_not_installed"
        assert machine["engine_auto_pause_reason"] == ""

        _poll_machine(
            client, machine_id, session_id,
            {"engine_state": {"state": "signed_out"}},
        )
        assert (
            _machine_engine_row(seeder, machine_id)["engine_auto_pause_reason"]
            == "engine_sign_in_required"
        )


def test_heartbeat_cannot_clear_sign_in_pause_without_live_credential(
    settings, seeder, daemon_world, machine_flow,
):
    """A healthy heartbeat does not clear the sign-in pause when no live
    credential exists: the engine would still need signing in."""
    machine_id = _machine_ids(seeder, machine_flow)
    _set_pause(seeder, machine_id, "engine_sign_in_required")
    with bearer_client(settings.base_url, machine_flow["machine_token"], timeout=40.0) as client:
        session_id = _open_machine_session(client, machine_id)
        response = _poll_machine(
            client, machine_id, session_id,
            {"engine_state": {"state": "available"}},
        )
        assert response.status_code == 200, response.text
    assert (
        _machine_engine_row(seeder, machine_id)["engine_auto_pause_reason"]
        == "engine_sign_in_required"
    )


def test_heartbeat_does_not_replace_engine_disabled(
    settings, seeder, daemon_world, machine_flow,
):
    """``engine_disabled`` (the instance kill switch) is never replaced
    by a daemon report."""
    machine_id = _machine_ids(seeder, machine_flow)
    _set_pause(seeder, machine_id, "engine_disabled")
    with bearer_client(settings.base_url, machine_flow["machine_token"], timeout=40.0) as client:
        session_id = _open_machine_session(client, machine_id)
        _poll_machine(
            client, machine_id, session_id,
            {"engine_state": {"state": "signed_out"}},
        )
        _poll_machine(
            client, machine_id, session_id,
            {"engine_state": {"state": "unavailable", "reason_code": "engine_sign_in_required"}},
        )
    assert _machine_engine_row(seeder, machine_id)["engine_auto_pause_reason"] == "engine_disabled"


def test_legacy_machine_never_gets_sign_in_required(settings, seeder, daemon_world):
    """A desktop-bundled machine reports ``signed_out`` but never
    receives the sign-in pause (it has no engine credential)."""
    machine = seeder.create_dev_machine(
        daemon_world["owner"]["id"], provisioning="desktop_bundled"
    )
    raw = "mt_" + secrets.token_urlsafe(24)
    seeder.create_machine_token(
        daemon_world["owner"]["id"], daemon_world["workspace"]["id"],
        raw, dev_machine_id=machine["id"],
    )
    with bearer_client(settings.base_url, raw, timeout=40.0) as client:
        session_id = _open_machine_session(client, machine["id"])
        response = _poll_machine(
            client, machine["id"], session_id,
            {"engine_state": {"state": "signed_out"}},
        )
        assert response.status_code == 200, response.text
    assert _machine_engine_row(seeder, machine["id"])["engine_auto_pause_reason"] == ""


def test_heartbeat_malformed_engine_state_never_fails_the_poll(
    settings, seeder, daemon_world, machine_flow,
):
    """A non-dict engine_state is ignored (logged), never a 500."""
    machine_id = _machine_ids(seeder, machine_flow)
    with bearer_client(settings.base_url, machine_flow["machine_token"], timeout=40.0) as client:
        session_id = _open_machine_session(client, machine_id)
        response = _poll_machine(client, machine_id, session_id, {"engine_state": "x"})
        assert response.status_code == 200, response.text
    assert _machine_engine_row(seeder, machine_id)["engine_state_reported_at"] is None


# -- machine-token 60-day idle expiry --------------------------------------


def test_machine_token_gets_sixty_day_idle_expiry_renewed_on_use(
    settings, seeder, daemon_world, machine_flow,
):
    token = seeder.db.fetchone(
        "SELECT id, expires_at, last_used_at FROM machine_token WHERE user_id = %s",
        (daemon_world["owner"]["id"],),
    )
    assert token is not None
    # A freshly minted token expires ~60 days out.
    assert token["expires_at"] is not None
    from datetime import datetime, timedelta, timezone

    assert token["expires_at"] > datetime.now(timezone.utc) + timedelta(days=59)
    assert token["expires_at"] < datetime.now(timezone.utc) + timedelta(days=61)
    # Use near the idle boundary renews it ~60 days out.
    seeder.db.execute(
        "UPDATE machine_token SET expires_at = now() + interval '1 day' WHERE id = %s",
        (token["id"],),
    )
    with bearer_client(settings.base_url, machine_flow["machine_token"]) as client:
        response = client.get(f"{ENGINE}/profile/")
    assert response.status_code == 200, response.text
    refreshed = seeder.db.fetchone(
        "SELECT expires_at, last_used_at FROM machine_token WHERE id = %s",
        (token["id"],),
    )
    assert refreshed["expires_at"] > datetime.now(timezone.utc) + timedelta(days=59)
    assert refreshed["last_used_at"] is not None


def test_expired_machine_token_is_rejected_on_the_engine_paths(
    settings, seeder, daemon_world, machine_flow,
):
    """An idle-expired token is dead on the engine path (401 with the
    specific code) and on the X-Api-Key surface (the generic 403 that
    keeps its message), and on runner-session open."""
    seeder.db.execute(
        "UPDATE machine_token SET expires_at = now() - interval '1 second'"
        " WHERE user_id = %s AND host_label = %s",
        (daemon_world["owner"]["id"], machine_flow["host_label"]),
    )
    with bearer_client(settings.base_url, machine_flow["machine_token"]) as client:
        response = client.get(f"{ENGINE}/profile/")
    assert response.status_code == 401, response.text
    assert "machine_token_expired" in response.text
    # X-Api-Key surface: the specific code is swallowed into the generic
    # invalid-token 403 (APIKeyAuthentication keeps its message).
    with api_key_client(settings.base_url, machine_flow["machine_token"]) as client:
        workspaces = client.get("/api/v1/auth/workspaces/")
    assert workspaces.status_code == 403, workspaces.text
    assert workspaces.json() == {"detail": "Given API token is not valid"}
    # Runner-plane surface.
    with bearer_client(settings.base_url, machine_flow["machine_token"]) as client:
        response = client.post(
            f"{DAEMON}/runners/{machine_flow['runner_id']}/sessions/", json={},
        )
    assert response.status_code == 401, response.text
    assert "machine_token_expired" in response.text


# -- machine sessions bound to the opening token ----------------------------


def test_machine_session_open_stores_the_opening_token(
    settings, seeder, daemon_world, machine_flow,
):
    """The session row records which machine token opened it (the
    eviction check on poll compares against this)."""
    machine_id = _machine_ids(seeder, machine_flow)
    with bearer_client(settings.base_url, machine_flow["machine_token"], timeout=40.0) as client:
        session_id = _open_machine_session(client, machine_id)
    session = seeder.db.fetchone(
        "SELECT machine_token_id FROM machine_session WHERE id = %s",
        (session_id,),
    )
    token_row = seeder.db.fetchone(
        "SELECT id FROM machine_token WHERE user_id = %s AND host_label = %s",
        (daemon_world["owner"]["id"], machine_flow["host_label"]),
    )
    assert str(session["machine_token_id"]) == str(token_row["id"])


# -- expiry sweep (WORKER; test_offline_runner_sweeps.py style) ------------
#
# ``runner.pause_engines_with_expired_credentials`` (beat ``*/5``): an
# expired active credential pauses its engine; a renewed credential
# (expiry in the future at decision time) is left alone; a legacy
# desktop machine never gets the sign-in pause. The Django half
# publishes Celery wire to the running solo worker; the Rust half
# seeds ``rust_job_queue`` for ``pidash-api worker``.


PAUSE_TASK = "runner.pause_engines_with_expired_credentials"
TIMEOUT = 60.0

SWEEP_BACKEND = os.environ.get("PIDASH_CONTRACT_BACKEND", "django")


@pytest.fixture()
def sweep_broker():
    if SWEEP_BACKEND == "rust":
        yield None
        return
    broker = Broker(get_broker_url())
    yield broker
    broker.close()


def _fire_sweep(broker):
    """Publish one sweep invocation; block until the worker settles it."""
    if SWEEP_BACKEND == "rust":
        celery_id = rust_queue.publish(PAUSE_TASK, [], {})
        rust_queue.wait_for_settled(celery_id, what=f"{PAUSE_TASK} settled")
        return
    broker.publish(PAUSE_TASK, [])
    # No broker-side settle signal on the Django path: the tests poll
    # the row effects below.


def _expire(engine_machine_id, seeder):
    seeder.db.execute(
        "UPDATE engine_credential SET expires_at = now() - interval '1 minute'"
        " WHERE dev_machine_id = %s",
        (engine_machine_id,),
    )
    seeder.db.execute(
        "UPDATE dev_machine SET engine_sign_in_state = 'active',"
        " engine_credential_expires_at = now() - interval '1 minute'"
        " WHERE id = %s",
        (engine_machine_id,),
    )


def test_expired_engine_credentials_pause_engines_on_the_sweep(
    seeder, daemon_world, machine_flow, sweep_broker,
):
    machine_id = _machine_ids(seeder, machine_flow)
    raw = "ec_" + secrets.token_urlsafe(32)
    _mint_engine_credential(seeder, daemon_world["owner"]["id"], machine_id, raw)
    legacy = seeder.create_dev_machine(
        daemon_world["owner"]["id"], provisioning="desktop_bundled"
    )
    _expire(machine_id, seeder)
    _expire(legacy["id"], seeder)

    _fire_sweep(sweep_broker)

    def paused():
        reason = seeder.db.fetchval(
            "SELECT engine_auto_pause_reason FROM dev_machine WHERE id = %s",
            (machine_id,),
        )
        return reason if reason == "engine_sign_in_required" else None

    wait_for_condition(paused, timeout=TIMEOUT, what="engine paused")
    assert _machine_engine_row(seeder, machine_id)["engine_sign_in_state"] == "active"
    #: the sweep pauses; it does not rewrite the stored sign-in state.
    assert _machine_engine_row(seeder, legacy["id"])["engine_auto_pause_reason"] == ""


def test_sweep_leaves_a_renewed_credential_untouched(
    seeder, daemon_world, machine_flow, sweep_broker,
):
    """A credential whose expiry is in the future at decision time
    (renewed / still valid) is not swept: no pause, even though the
    machine is active."""
    import time

    machine_id = _machine_ids(seeder, machine_flow)
    raw = "ec_" + secrets.token_urlsafe(32)
    _mint_engine_credential(seeder, daemon_world["owner"]["id"], machine_id, raw, days=14)
    _fire_sweep(sweep_broker)
    time.sleep(5)
    assert _machine_engine_row(seeder, machine_id)["engine_auto_pause_reason"] == ""



