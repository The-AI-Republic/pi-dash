"""``runner_gone`` auth code (PDASHOSS01-306).

Once the caller has proven who it is — a MachineToken that exists, is
unrevoked, sits on an unrevoked dev machine and whose user is still a
workspace member, or a legacy JWT whose signature and expiry verify — a
runner id with no row answers ``401 {"detail": "runner_gone"}`` instead of
``runner_not_found``. Every failure before that point keeps its own code,
a revoked runner still answers ``runner_revoked``, and a non-UUID id (only
reachable via ``X-Runner-Id``) answers ``runner_not_found``.
"""

from __future__ import annotations

import uuid

from _harness.http import bearer_client

from .conftest import DAEMON


def _open_session(client, runner_id):
    return client.post(f"{DAEMON}/runners/{runner_id}/sessions/", json={})


def _assert_auth_failed(response, code):
    assert response.status_code == 401, response.text
    assert response.json() == {"detail": code}
    assert response.headers["www-authenticate"] == "Bearer"


def test_verified_machine_token_missing_runner_is_runner_gone(settings, machine_flow):
    with bearer_client(settings.base_url, machine_flow["machine_token"]) as client:
        response = _open_session(client, uuid.uuid4())
    _assert_auth_failed(response, "runner_gone")


def test_verified_machine_token_missing_runner_via_header_is_runner_gone(settings, machine_flow):
    # Runs URLs carry no runner id: the identity travels in X-Runner-Id.
    with bearer_client(
        settings.base_url,
        machine_flow["machine_token"],
        headers={"X-Runner-Id": str(uuid.uuid4())},
    ) as client:
        response = client.post(f"{DAEMON}/runs/{uuid.uuid4()}/accept/", json={})
    _assert_auth_failed(response, "runner_gone")


def test_machine_token_non_uuid_runner_header_is_runner_not_found(settings, machine_flow):
    with bearer_client(
        settings.base_url,
        machine_flow["machine_token"],
        headers={"X-Runner-Id": "not-a-uuid"},
    ) as client:
        response = client.post(f"{DAEMON}/runs/{uuid.uuid4()}/accept/", json={})
    _assert_auth_failed(response, "runner_not_found")


def test_unknown_machine_token_keeps_its_code(settings, machine_flow):
    with bearer_client(settings.base_url, "mt_" + uuid.uuid4().hex) as client:
        response = _open_session(client, uuid.uuid4())
    _assert_auth_failed(response, "machine_token_invalid")


def test_revoked_machine_token_keeps_its_code(settings, seeder, daemon_world, machine_flow):
    seeder.db.execute(
        "UPDATE machine_token SET revoked_at = now() WHERE user_id = %s AND host_label = %s",
        (daemon_world["owner"]["id"], machine_flow["host_label"]),
    )
    with bearer_client(settings.base_url, machine_flow["machine_token"]) as client:
        response = _open_session(client, uuid.uuid4())
    _assert_auth_failed(response, "machine_token_revoked")


def test_non_member_machine_token_keeps_its_code(settings, seeder, daemon_world, machine_flow):
    seeder.db.execute(
        "UPDATE workspace_members SET is_active = false WHERE workspace_id = %s AND member_id = %s",
        (daemon_world["workspace"]["id"], daemon_world["owner"]["id"]),
    )
    with bearer_client(settings.base_url, machine_flow["machine_token"]) as client:
        response = _open_session(client, uuid.uuid4())
    _assert_auth_failed(response, "membership_revoked")


def test_revoked_runner_keeps_runner_revoked(settings, seeder, machine_flow):
    seeder.db.execute(
        "UPDATE runner SET revoked_at = now(), revoked_reason = 'manual_revoke' WHERE id = %s",
        (machine_flow["runner_id"],),
    )
    with bearer_client(settings.base_url, machine_flow["machine_token"]) as client:
        response = _open_session(client, machine_flow["runner_id"])
    _assert_auth_failed(response, "runner_revoked")


def test_verified_jwt_for_deleted_runner_is_runner_gone(settings, seeder, enrolled):
    seeder.db.execute("DELETE FROM runner WHERE id = %s", (enrolled["runner_id"],))
    with bearer_client(settings.base_url, enrolled["access_token"]) as client:
        response = _open_session(client, enrolled["runner_id"])
    _assert_auth_failed(response, "runner_gone")
