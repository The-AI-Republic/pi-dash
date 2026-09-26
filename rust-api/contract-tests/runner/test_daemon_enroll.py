"""Daemon enrollment + refresh + self-revoke + machine-token redeem.

Covers ``runners/enroll/``, ``runners/`` (CLI create), ``runners/<id>/refresh/``,
``runners/<id>/`` (self-revoke) and ``machine-tokens/`` (redeem) — the full
credential lifecycle: one-time enrollment token → refresh/access pair →
rotation → self-teardown, plus the ``pidash auth login`` ticket path.
"""

from __future__ import annotations

import secrets
import uuid

import pytest

from _harness.http import anonymous_client, api_key_client, bearer_client

from .conftest import DAEMON

pytestmark = pytest.mark.contract


def _enrollment_raw(tag: str) -> str:
    return f"apd_en_{tag}{secrets.token_hex(8)}"


# -- runners/enroll/ -----------------------------------------------------

def test_daemon_enroll_shape(settings, seeder, daemon_world):
    raw = _enrollment_raw(seeder.tag)
    host_label = f"enroll-shape-{seeder.tag}"[:200]
    seeded = seeder.enroll_runner(
        daemon_world["owner"]["id"], daemon_world["workspace"]["id"], daemon_world["pod"]["id"], raw
    )
    with anonymous_client(settings.base_url) as client:
        response = client.post(
            f"{DAEMON}/runners/enroll/",
            json={"enrollment_token": raw, "host_label": host_label},
        )
    assert response.status_code == 201, response.text
    body = response.json()
    assert body["runner_id"] == seeded["id"]
    assert body["runner_name"] == seeded["name"]
    assert body["refresh_token"].startswith("rt_")
    assert body["access_token"]
    assert body["access_token_expires_at"]
    assert body["refresh_token_generation"] == 1
    assert body["workspace_slug"] == daemon_world["workspace"]["slug"]
    assert body["pod_slug"] == daemon_world["pod"]["name"]
    assert body["project_identifier"] == daemon_world["project"]["identifier"]
    assert body["long_poll_interval_secs"] == 25
    assert body["protocol_version"] == 4
    assert body["machine_token_minted"] is True
    assert body["machine_token"].startswith("mt_")
    # Teardown rows the endpoint minted (dev machine + machine token).
    seeder.db.execute(
        "DELETE FROM machine_token WHERE user_id = %s AND host_label = %s",
        (daemon_world["owner"]["id"], host_label),
    )
    seeder.db.execute(
        "UPDATE runner SET dev_machine_id = NULL WHERE id = %s", (seeded["id"],)
    )
    seeder.db.execute(
        "DELETE FROM dev_machine WHERE owner_id = %s AND host_label = %s",
        (daemon_world["owner"]["id"], host_label),
    )


def test_daemon_enroll_rejects_unknown_token(anon_client):
    response = anon_client.post(
        f"{DAEMON}/runners/enroll/",
        json={"enrollment_token": "apd_en_doesnotexist000000", "host_label": "h"},
    )
    assert response.status_code == 401
    assert response.json() == {"error": "invalid_or_expired_enrollment_token"}


def test_daemon_enroll_token_single_use(settings, seeder, daemon_world):
    raw = _enrollment_raw(seeder.tag)
    seeder.enroll_runner(
        daemon_world["owner"]["id"], daemon_world["workspace"]["id"], daemon_world["pod"]["id"], raw
    )
    with anonymous_client(settings.base_url) as client:
        first = client.post(
            f"{DAEMON}/runners/enroll/", json={"enrollment_token": raw, "host_label": "h1"}
        )
        assert first.status_code == 201
        second = client.post(
            f"{DAEMON}/runners/enroll/", json={"enrollment_token": raw, "host_label": "h2"}
        )
    # The success path clears the stored hash, so a replayed token looks
    # unknown (401) rather than already-used (409 pins the revoked/enrolled
    # row states, which a hash lookup can no longer reach).
    assert second.status_code == 401
    assert second.json() == {"error": "invalid_or_expired_enrollment_token"}
    # Teardown rows the endpoint minted.
    seeder.db.execute(
        "DELETE FROM machine_token WHERE user_id = %s AND host_label IN ('h1','h2')",
        (daemon_world["owner"]["id"],),
    )
    seeder.db.execute(
        "UPDATE runner SET dev_machine_id = NULL WHERE owner_id = %s AND host_label = 'h1'",
        (daemon_world["owner"]["id"],),
    )
    seeder.db.execute(
        "DELETE FROM dev_machine WHERE owner_id = %s AND host_label IN ('h1','h2')",
        (daemon_world["owner"]["id"],),
    )


def test_daemon_enroll_validates_body(anon_client):
    response = anon_client.post(f"{DAEMON}/runners/enroll/", json={})
    assert response.status_code == 400


# -- runners/ (CLI create, X-Api-Key) -------------------------------------

def test_daemon_runner_create_shape(settings, daemon_world, seeder):
    host_label = f"create-shape-{seeder.tag}"[:200]
    with api_key_client(settings.base_url, daemon_world["api_token"]["token"]) as client:
        response = client.post(
            f"{DAEMON}/runners/",
            json={
                "project": daemon_world["project"]["identifier"],
                "workspace_slug": daemon_world["workspace"]["slug"],
                "host_label": host_label,
                "name": f"cli-{seeder.tag}",
            },
        )
    assert response.status_code == 201, response.text
    body = response.json()
    assert uuid.UUID(body["runner_id"])
    assert body["runner_name"] == f"cli-{seeder.tag}"
    assert body["workspace_slug"] == daemon_world["workspace"]["slug"]
    assert body["pod_slug"] == daemon_world["pod"]["name"]
    assert body["project_identifier"] == daemon_world["project"]["identifier"]
    assert body["long_poll_interval_secs"] == 25
    assert body["protocol_version"] == 4
    # APIToken callers rotate into an mt_ token (machine_token_minted=True).
    assert body["machine_token_minted"] is True
    assert body["machine_token"].startswith("mt_")
    # Teardown rows the endpoint created (the seeder never saw them).
    seeder.db.execute(
        "DELETE FROM machine_token WHERE user_id = %s AND host_label = %s",
        (daemon_world["owner"]["id"], host_label),
    )
    seeder.db.execute("DELETE FROM runner WHERE id = %s", (body["runner_id"],))
    seeder.db.execute(
        "DELETE FROM dev_machine WHERE owner_id = %s AND host_label = %s",
        (daemon_world["owner"]["id"], host_label),
    )


def test_daemon_runner_create_requires_auth(anon_client, daemon_world):
    response = anon_client.post(
        f"{DAEMON}/runners/", json={"project": daemon_world["project"]["identifier"]}
    )
    assert response.status_code in (401, 403)


def test_daemon_runner_create_validates_project(settings, daemon_world):
    with api_key_client(settings.base_url, daemon_world["api_token"]["token"]) as client:
        missing = client.post(
            f"{DAEMON}/runners/",
            json={"workspace_slug": daemon_world["workspace"]["slug"]},
        )
        assert missing.status_code == 400
        assert missing.json() == {"error": "project is required"}
        unknown = client.post(
            f"{DAEMON}/runners/",
            json={"project": "NOPE", "workspace_slug": daemon_world["workspace"]["slug"]},
        )
        assert unknown.status_code == 404
        assert unknown.json() == {"error": "project_not_found"}


def test_daemon_runner_create_rejects_invalid_name(settings, daemon_world):
    with api_key_client(settings.base_url, daemon_world["api_token"]["token"]) as client:
        response = client.post(
            f"{DAEMON}/runners/",
            json={
                "project": daemon_world["project"]["identifier"],
                "workspace_slug": daemon_world["workspace"]["slug"],
                "name": "not a valid name!",
            },
        )
    assert response.status_code == 400
    assert response.json()["error"] == "invalid_runner_name"


# -- runners/<id>/refresh/ ------------------------------------------------

def test_daemon_refresh_rotates_pair(settings, enrolled):
    with bearer_client(settings.base_url, enrolled["refresh_token"]) as client:
        response = client.post(f"{DAEMON}/runners/{enrolled['runner_id']}/refresh/")
    assert response.status_code == 200, response.text
    body = response.json()
    assert body["refresh_token"].startswith("rt_")
    assert body["refresh_token"] != enrolled["refresh_token"]
    assert body["access_token"]
    assert body["access_token_expires_at"]
    assert body["refresh_token_generation"] == 2


def test_daemon_refresh_rejects_bogus_token(settings, enrolled):
    with bearer_client(settings.base_url, "rt_bogus000000000000000000") as client:
        response = client.post(f"{DAEMON}/runners/{enrolled['runner_id']}/refresh/")
    assert response.status_code == 401


def test_daemon_refresh_replay_is_detected(settings, enrolled):
    """Re-presenting the rotated-out refresh token revokes the runner."""
    with bearer_client(settings.base_url, enrolled["refresh_token"]) as client:
        rotated = client.post(f"{DAEMON}/runners/{enrolled['runner_id']}/refresh/")
        assert rotated.status_code == 200
        replay = client.post(f"{DAEMON}/runners/{enrolled['runner_id']}/refresh/")
    assert replay.status_code == 401
    assert replay.json() == {"error": "refresh_token_replayed"}


# -- runners/<id>/ (self-revoke) ------------------------------------------

def test_daemon_self_revoke_tears_down(settings, seeder, runner_client, enrolled):
    response = runner_client.delete(f"{DAEMON}/runners/{enrolled['runner_id']}/")
    assert response.status_code == 204
    assert (
        seeder.db.fetchval("SELECT COUNT(*) FROM runner WHERE id = %s", (enrolled["runner_id"],))
        == 0
    )


def test_daemon_self_revoke_rejects_cross_runner(settings, enrolled, seeder, daemon_world):
    other = seeder.enroll_runner(
        daemon_world["owner"]["id"],
        daemon_world["workspace"]["id"],
        daemon_world["pod"]["id"],
        _enrollment_raw(seeder.tag),
    )
    with bearer_client(settings.base_url, enrolled["access_token"]) as client:
        response = client.delete(f"{DAEMON}/runners/{other['id']}/")
    # The JWT auth class itself enforces URL identity before the view runs,
    # so the mismatch surfaces as 401 + detail rather than the view's 403.
    assert response.status_code == 401
    assert response.json() == {"detail": "runner_id_mismatch"}


# -- machine-tokens/ (redeem) ----------------------------------------------

def test_daemon_machine_token_redeem_validates_ticket(anon_client):
    missing = anon_client.post(f"{DAEMON}/machine-tokens/", json={})
    assert missing.status_code == 400
    assert missing.json() == {"error": "ticket is required"}
    bogus = anon_client.post(f"{DAEMON}/machine-tokens/", json={"ticket": "nope"})
    assert bogus.status_code == 401
    assert bogus.json() == {"error": "invalid_or_expired_ticket"}


def test_daemon_machine_token_redeem_full_circle(settings, user_client, daemon_world, anon_client, seeder):
    """Web UI mints a ticket (session auth), the CLI redeems it here."""
    ticket_response = user_client.post(
        f"/api/runners/machine-tokens/{daemon_world['workspace']['id']}/tickets/",
        json={"host_label": "redeem-circle"},
    )
    assert ticket_response.status_code == 201, ticket_response.text
    ticket = ticket_response.json()["ticket"]
    redeem = anon_client.post(f"{DAEMON}/machine-tokens/", json={"ticket": ticket})
    assert redeem.status_code == 201, redeem.text
    body = redeem.json()
    assert body["machine_token"].startswith("mt_")
    assert body["workspace_slug"] == daemon_world["workspace"]["slug"]
    # Single use: the ticket is consumed on read.
    again = anon_client.post(f"{DAEMON}/machine-tokens/", json={"ticket": ticket})
    assert again.status_code == 401
    # Teardown the redeemed token row.
    seeder.db.execute(
        "DELETE FROM machine_token WHERE user_id = %s AND host_label = 'redeem-circle'",
        (daemon_world["owner"]["id"],),
    )
