# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Engine credential, engine endpoints, machine identity and merge (PIDESKAPP-122).

Contract tests for ``.ai_design/built_engine_as_runner/design.md`` §4.3, §4.6,
§4.9, §4.10, §5, §8 (automatic pause reasons), §11 and the §13 server rows
for merge, daemon-credential expiry and settings. Daemon traffic uses bearer
tokens on a fresh ``APIClient`` per caller (``session_client`` forces
authentication on its client, which would mask bearer auth).
"""

from __future__ import annotations

import json
import logging
from datetime import timedelta

import pytest
from django.db import connection
from django.test import override_settings
from django.urls import reverse
from django.utils import timezone

from pi_dash.db.models import CLIDeviceCode, Project, User, WorkspaceMember
from pi_dash.runner.models import (
    DevMachine,
    EngineAutoPauseReason,
    EngineCredential,
    EngineSignInState,
    MachineSession,
    MachineToken,
    Pod,
    Runner,
    RunnerProvisioning,
)
from pi_dash.runner.services import machine_outbox, tokens
from pi_dash.runner.services.engine_credentials import issue_engine_credential
from pi_dash.runner.services.machine_identity import RUNNER_NAME_MAX, engine_runner_base_name

from .conftest import _client, _mint_mt

pytestmark = [pytest.mark.contract, pytest.mark.usefixtures("_isolate")]

ENABLED = {"MANAGED_RUNNER_ENABLED": True}


# --------------------------------------------------------------------------
# Helpers (fixtures live in this directory's conftest)
# --------------------------------------------------------------------------


class GatewayDown(Exception):
    pass


class SessionRevoked(Exception):
    pass


class WalletEmptyError(Exception):
    pass


def _age_poll(device_code):
    CLIDeviceCode.objects.filter(device_code=device_code).update(last_polled_at=timezone.now() - timedelta(seconds=30))


def _start(scope=None, **extra):
    body = dict(extra)
    if scope:
        body["scope"] = scope
    resp = _client().post("/api/v1/auth/device/start/", body, format="json")
    assert resp.status_code == 200, resp.data
    return resp.data


def _approve(web, user_code, **extra):
    return web.post("/api/v1/auth/device/approve/", {"user_code": user_code, **extra}, format="json")


def _poll(device_code, bearer=None):
    _age_poll(device_code)
    return _client(bearer=bearer).post("/api/v1/auth/device/token/", {"device_code": device_code}, format="json")


def _cli_login(web, machine_id, *, workspace_slug="test-workspace", interactive=True, host_label="rich-mbp"):
    """``pidash auth login``: device grant, then the machine-token exchange."""
    start = _start()
    assert _approve(web, start["user_code"], interactive=interactive).status_code == 200
    access = _poll(start["device_code"]).data["access_token"]
    return _client(api_key=access).post(
        "/api/v1/auth/machine-token/",
        {"workspace_slug": workspace_slug, "dev_machine_id": str(machine_id), "host_label": host_label},
        format="json",
    )


def _engine_login(web, mt_raw, *, interactive=True, dev_machine_id=None):
    """``pidash login --engine``: an engine grant redeemed with the machine token."""
    start = _start("engine")
    extra = {"interactive": interactive}
    if dev_machine_id is not None:
        extra["dev_machine_id"] = str(dev_machine_id)
    approve = _approve(web, start["user_code"], **extra)
    if approve.status_code != 200:
        return approve
    return _poll(start["device_code"], bearer=mt_raw)


def _sign_in(user, machine):
    return issue_engine_credential(user=user, dev_machine=machine)


def _set_expiry(machine, when):
    """Move the machine's live engine credential to expire at ``when``."""
    EngineCredential.objects.filter(dev_machine=machine, revoked_at__isnull=True).update(expires_at=when)
    DevMachine.objects.filter(pk=machine.pk).update(engine_credential_expires_at=when)
    machine.refresh_from_db()


def _make_expiring(machine):
    _set_expiry(machine, timezone.now() + timedelta(days=1))
    assert machine.current_engine_sign_in_state() == EngineSignInState.EXPIRING


# --------------------------------------------------------------------------
# Device flow: login issues the engine credential (§4.3)
# --------------------------------------------------------------------------


@override_settings(**ENABLED)
def test_login_issues_engine_credential_with_machine_token(db, web, create_user, workspace, lane):
    machine_id = "6f1c7d1e-1111-4a2b-9c3d-000000000001"
    resp = _cli_login(web, machine_id)
    assert resp.status_code == 201, resp.data
    assert resp.data["machine_token"].startswith("mt_")
    cred = resp.data["engine_credential"]
    assert cred["token"].startswith("ec_")
    assert resp.data["engine_sign_in_state"] == EngineSignInState.ACTIVE

    machine = DevMachine.objects.get(pk=machine_id)
    row = EngineCredential.objects.get(dev_machine=machine, revoked_at__isnull=True)
    # Hashed at rest; 14 days fixed from issue.
    assert row.token_hash != cred["token"] and cred["token"] not in row.token_hash
    assert abs((row.expires_at - row.created_at) - timedelta(days=14)) < timedelta(seconds=5)
    assert machine.engine_sign_in_state == EngineSignInState.ACTIVE
    assert machine.engine_credential_expires_at == row.expires_at


@override_settings(MANAGED_RUNNER_ENABLED=False)
def test_login_on_instance_without_engine_issues_no_engine_credential(db, web, workspace, lane):
    machine_id = "6f1c7d1e-1111-4a2b-9c3d-000000000002"
    resp = _cli_login(web, machine_id)
    assert resp.status_code == 201, resp.data
    assert "engine_credential" not in resp.data
    assert resp.data["engine_sign_in_state"] == EngineSignInState.NEVER
    assert not EngineCredential.objects.exists()


@override_settings(**ENABLED)
def test_relogin_rotates_engine_credential(db, web, workspace, lane):
    machine_id = "6f1c7d1e-1111-4a2b-9c3d-000000000003"
    first = _cli_login(web, machine_id).data["engine_credential"]["token"]
    second = _cli_login(web, machine_id).data["engine_credential"]["token"]
    assert first != second
    assert EngineCredential.objects.filter(dev_machine_id=machine_id, revoked_at__isnull=True).count() == 1
    assert EngineCredential.objects.get(revoked_at__isnull=False).revoked_reason == "superseded"


@override_settings(**ENABLED)
def test_machine_token_exchange_with_machine_token_mints_no_engine_credential(
    db, create_user, workspace, workspace_b, machine, mt, lane
):
    # Adding a workspace with the existing daemon credential: no person
    # approved anything, so no engine credential.
    resp = _client(api_key=mt).post(
        "/api/v1/auth/machine-token/",
        {"workspace_slug": "second-ws", "dev_machine_id": str(machine.id), "host_label": "rich-mbp"},
        format="json",
    )
    assert resp.status_code == 201, resp.data
    assert "engine_credential" not in resp.data
    assert not EngineCredential.objects.exists()


@override_settings(**ENABLED)
def test_approval_takes_explicit_workspace_slug(db, web, workspace, workspace_b):
    start = _start()
    resp = _approve(web, start["user_code"], workspace_slug="second-ws")
    assert resp.status_code == 200, resp.data
    assert resp.data["workspace_slug"] == "second-ws"
    assert CLIDeviceCode.objects.get(device_code=start["device_code"]).workspace_id == workspace_b.id
    other = _start()
    assert _approve(web, other["user_code"], workspace_slug="nope").status_code == 404


def test_approval_without_workspace_keeps_most_recent(db, web, workspace, workspace_b):
    start = _start()
    resp = _approve(web, start["user_code"])
    assert resp.status_code == 200
    assert resp.data["workspace_slug"] == "second-ws"  # most recent membership


# --------------------------------------------------------------------------
# Device flow: engine-scoped grants (§4.3, §4.6)
# --------------------------------------------------------------------------


@override_settings(**ENABLED)
def test_engine_grant_requires_the_machine_token(db, web, machine, mt, lane):
    start = _start("engine")
    assert start["scope"] == "engine"
    assert _approve(web, start["user_code"], interactive=True).status_code == 200
    # The approved code alone is not enough.
    bare = _poll(start["device_code"])
    assert bare.status_code == 401
    assert bare.data["error"] == "machine_token_required"
    assert not EngineCredential.objects.exists()
    # A bogus bearer is not enough either.
    bad = _poll(start["device_code"], bearer="mt_not-a-real-token")
    assert bad.status_code == 401
    ok = _poll(start["device_code"], bearer=mt)
    assert ok.status_code == 200, ok.data
    assert ok.data["engine_credential"]["token"].startswith("ec_")
    assert ok.data["engine_sign_in_state"] == EngineSignInState.ACTIVE
    # Bound to the token's machine.
    assert EngineCredential.objects.get().dev_machine_id == machine.id


@override_settings(**ENABLED)
def test_engine_grant_pending_until_approved(db, mt, lane):
    start = _start("engine")
    resp = _poll(start["device_code"], bearer=mt)
    assert resp.status_code == 400
    assert resp.data["error"] == "authorization_pending"


@override_settings(**ENABLED)
def test_engine_grant_refuses_machine_of_another_user(db, web, workspace, lane):
    stranger = User.objects.create(email="stranger@example.com", username="stranger")
    WorkspaceMember.objects.create(workspace=workspace, member=stranger, role=15)
    their_machine = DevMachine.objects.create(owner=stranger, host_label="theirs")
    their_mt = _mint_mt(stranger, workspace, their_machine)
    # create_user approves; the stranger's daemon tries to redeem.
    resp = _engine_login(web, their_mt)
    assert resp.status_code == 403, resp.data
    assert not EngineCredential.objects.exists()


@override_settings(**ENABLED)
def test_engine_grant_named_machine_must_match_token(db, web, create_user, workspace, machine, lane):
    other = DevMachine.objects.create(owner=create_user, host_label="other")
    other_mt = _mint_mt(create_user, workspace, other)
    start = _start("engine", dev_machine_id=str(machine.id))
    assert _approve(web, start["user_code"], interactive=True).status_code == 200
    resp = _poll(start["device_code"], bearer=other_mt)
    assert resp.status_code == 403
    assert resp.data["error"] == "dev_machine_mismatch"


@override_settings(**ENABLED)
def test_silent_approval_needs_own_machine(db, web, workspace, machine, lane):
    stranger = User.objects.create(email="s2@example.com", username="s2")
    theirs = DevMachine.objects.create(owner=stranger, host_label="x")
    other = _start("engine")
    resp = _approve(web, other["user_code"], interactive=False, dev_machine_id=str(theirs.id))
    assert resp.status_code == 404


@override_settings(**ENABLED)
def test_silent_approval_refused_after_sign_out_and_interactive_clears_it(db, web, create_user, machine, mt, lane):
    _sign_in(create_user, machine)
    # ``pidash auth logout --engine``.
    out = _client(bearer=mt).post("/api/v1/runner/engine/credential/revoke/", {}, format="json")
    assert out.status_code == 200, out.data
    assert out.data["engine_sign_in_state"] == EngineSignInState.SIGNED_OUT
    machine.refresh_from_db()
    assert machine.engine_sign_in_state == EngineSignInState.SIGNED_OUT
    assert machine.engine_auto_pause_reason == EngineAutoPauseReason.ENGINE_SIGN_IN_REQUIRED

    # The app's silent renewal is refused.
    start = _start("engine")
    silent = _approve(web, start["user_code"], interactive=False, dev_machine_id=str(machine.id))
    assert silent.status_code == 409
    assert silent.data["error"] == "engine_silent_renewal_refused"
    assert silent.data["engine_sign_in_state"] == EngineSignInState.SIGNED_OUT

    # An explicit browser sign-in clears it.
    resp = _engine_login(web, mt, interactive=True)
    assert resp.status_code == 200, resp.data
    machine.refresh_from_db()
    assert machine.current_engine_sign_in_state() == EngineSignInState.ACTIVE
    assert machine.engine_auto_pause_reason == ""


@override_settings(**ENABLED)
def test_silent_approval_renews_an_expiring_engine(db, web, create_user, machine, mt, lane):
    _sign_in(create_user, machine)
    soon = timezone.now() + timedelta(days=1)
    EngineCredential.objects.update(expires_at=soon)
    DevMachine.objects.filter(pk=machine.pk).update(engine_credential_expires_at=soon)
    machine.refresh_from_db()
    assert machine.current_engine_sign_in_state() == EngineSignInState.EXPIRING
    resp = _engine_login(web, mt, interactive=False, dev_machine_id=machine.id)
    assert resp.status_code == 200, resp.data
    assert resp.data["engine_sign_in_state"] == EngineSignInState.ACTIVE


@override_settings(**ENABLED)
def test_silent_approval_without_machine_is_checked_at_redemption(db, web, create_user, workspace, machine, mt, lane):
    """The canonical silent body ``{user_code, workspace_slug, interactive:
    false}`` names no machine; the redeeming token's machine is checked."""
    _sign_in(create_user, machine)
    _make_expiring(machine)
    start = _start("engine")
    body = {"workspace_slug": "test-workspace", "interactive": False}
    assert _approve(web, start["user_code"], **body).status_code == 200
    ok = _poll(start["device_code"], bearer=mt)
    assert ok.status_code == 200, ok.data

    # Now ``active`` again: a further silent renewal is not due.
    early = _start("engine")
    assert _approve(web, early["user_code"], **body).status_code == 200
    refused_early = _poll(early["device_code"], bearer=mt)
    assert refused_early.status_code == 409
    assert refused_early.data["engine_sign_in_state"] == EngineSignInState.ACTIVE

    _client(bearer=mt).post("/api/v1/runner/engine/credential/revoke/", {}, format="json")
    again = _start("engine")
    assert _approve(web, again["user_code"], **body).status_code == 200
    refused = _poll(again["device_code"], bearer=mt)
    assert refused.status_code == 409
    assert refused.data["error"] == "engine_silent_renewal_refused"


@override_settings(**ENABLED)
def test_signed_out_between_silent_approval_and_redemption_is_refused(db, web, create_user, machine, mt, lane):
    _sign_in(create_user, machine)
    _make_expiring(machine)
    start = _start("engine")
    assert _approve(web, start["user_code"], interactive=False, dev_machine_id=str(machine.id)).status_code == 200
    _client(bearer=mt).post("/api/v1/runner/engine/credential/revoke/", {}, format="json")
    resp = _poll(start["device_code"], bearer=mt)
    assert resp.status_code == 409
    assert resp.data["error"] == "engine_silent_renewal_refused"


@override_settings(**ENABLED)
def test_silent_cli_login_after_sign_out_issues_no_engine_credential(db, web, create_user, workspace, machine, lane):
    _sign_in(create_user, machine)
    from pi_dash.runner.services.engine_credentials import revoke_engine_credential

    revoke_engine_credential(machine, reason="signed_out")
    silent = _cli_login(web, machine.id, interactive=False)
    assert silent.status_code == 201
    assert "engine_credential" not in silent.data
    assert silent.data["engine_sign_in_state"] == EngineSignInState.SIGNED_OUT
    explicit = _cli_login(web, machine.id, interactive=True)
    assert explicit.data["engine_credential"]["token"].startswith("ec_")


def test_device_start_rejects_unknown_scope(db):
    resp = _client().post("/api/v1/auth/device/start/", {"scope": "root"}, format="json")
    assert resp.status_code == 400


# --------------------------------------------------------------------------
# Engine profile, model token, revoke (§4.3, §8)
# --------------------------------------------------------------------------


@override_settings(**ENABLED)
def test_profile_endpoint(db, create_user, machine, mt, lane):
    _sign_in(create_user, machine)
    resp = _client(bearer=mt).get("/api/v1/runner/engine/profile/")
    assert resp.status_code == 200, resp.data
    for key in ("available", "lane", "base_url", "model", "reason_code", "engine_sign_in_state"):
        assert key in resp.data
    assert resp.data["available"] is True
    assert resp.data["base_url"] == "https://gw.example.com/v1"
    assert resp.data["engine_sign_in_state"] == EngineSignInState.ACTIVE
    assert _client().get("/api/v1/runner/engine/profile/").status_code in (401, 403)


def test_profile_endpoint_kill_switch_pauses_and_recovers(db, machine, mt, lane):
    with override_settings(MANAGED_RUNNER_ENABLED=False):
        resp = _client(bearer=mt).get("/api/v1/runner/engine/profile/")
    assert resp.data["available"] is False
    assert resp.data["reason_code"] == "engine_disabled"
    machine.refresh_from_db()
    assert machine.engine_auto_pause_reason == "engine_disabled"
    with override_settings(MANAGED_RUNNER_ENABLED=True):
        lane["available"] = False
        resp = _client(bearer=mt).get("/api/v1/runner/engine/profile/")
        assert resp.data["reason_code"] == "llm_config_missing"
        machine.refresh_from_db()
        assert machine.engine_auto_pause_reason == "profile_unavailable"
        lane["available"] = True
        _client(bearer=mt).get("/api/v1/runner/engine/profile/")
    machine.refresh_from_db()
    assert machine.engine_auto_pause_reason == ""


@override_settings(**ENABLED)
def test_model_token_success_is_audited_without_the_token(db, create_user, machine, lane, caplog):
    issued = _sign_in(create_user, machine)
    DevMachine.objects.filter(pk=machine.pk).update(engine_auto_pause_reason="gateway_session_revoked")
    caplog.set_level(logging.INFO)
    resp = _client(bearer=issued.raw).post(
        "/api/v1/runner/engine/model-token/", {}, format="json", HTTP_X_REQUEST_ID="req-123"
    )
    assert resp.status_code == 200, resp.data
    assert resp.data["token"] == "gw-secret-token"
    assert resp.data["expires_at"]
    machine.refresh_from_db()
    assert machine.engine_auto_pause_reason == ""
    audit = [r.getMessage() for r in caplog.records if "model_token_issued" in r.getMessage()]
    assert audit and "req-123" in audit[0] and str(machine.id) in audit[0] and str(create_user.id) in audit[0]
    assert all("gw-secret-token" not in r.getMessage() and issued.raw not in r.getMessage() for r in caplog.records)
    assert EngineCredential.objects.get().last_used_at is not None


@override_settings(**ENABLED)
def test_model_token_accepts_engine_credential_only(db, mt, lane):
    resp = _client(bearer=mt).post("/api/v1/runner/engine/model-token/", {}, format="json")
    assert resp.status_code == 401
    assert resp.data["reason_code"] == "engine_sign_in_required"
    assert _client().post("/api/v1/runner/engine/model-token/", {}, format="json").status_code == 401


@override_settings(**ENABLED)
@pytest.mark.parametrize("kind", ["expired", "revoked"])
def test_model_token_dead_credential_pauses_with_sign_in_required(db, create_user, machine, lane, kind):
    issued = _sign_in(create_user, machine)
    if kind == "expired":
        EngineCredential.objects.update(expires_at=timezone.now() - timedelta(seconds=1))
    else:
        EngineCredential.objects.update(revoked_at=timezone.now(), revoked_reason="admin")
    resp = _client(bearer=issued.raw).post("/api/v1/runner/engine/model-token/", {}, format="json")
    assert resp.status_code == 401
    assert resp.data["reason_code"] == "engine_sign_in_required"
    machine.refresh_from_db()
    assert machine.engine_auto_pause_reason == "engine_sign_in_required"


@pytest.mark.parametrize(
    "setup, reason, http",
    [
        ("gateway", "gateway_session_revoked", 409),
        ("wallet", "wallet_empty", 402),
        ("profile", "profile_unavailable", 409),
        ("disabled", "engine_disabled", 409),
    ],
)
def test_model_token_reason_mapping(db, create_user, machine, lane, setup, reason, http):
    with override_settings(MANAGED_RUNNER_ENABLED=True):
        issued = _sign_in(create_user, machine)
    enabled = setup != "disabled"
    if setup == "gateway":
        lane["raise"] = SessionRevoked()
    elif setup == "wallet":
        lane["raise"] = WalletEmptyError()
    elif setup == "profile":
        lane["available"] = False
    with override_settings(MANAGED_RUNNER_ENABLED=enabled):
        resp = _client(bearer=issued.raw).post("/api/v1/runner/engine/model-token/", {}, format="json")
    assert resp.status_code == http, resp.data
    assert resp.data["reason_code"] == reason
    machine.refresh_from_db()
    assert machine.engine_auto_pause_reason == reason
    # Recovery: a successful token clears it (wallet_empty is the daemon's to clear).
    lane["raise"] = None
    lane["available"] = True
    with override_settings(MANAGED_RUNNER_ENABLED=True):
        ok = _client(bearer=issued.raw).post("/api/v1/runner/engine/model-token/", {}, format="json")
    assert ok.status_code == 200
    machine.refresh_from_db()
    assert machine.engine_auto_pause_reason == ("wallet_empty" if reason == "wallet_empty" else "")


@override_settings(**ENABLED)
def test_model_token_transient_failure_does_not_pause(db, create_user, machine, lane):
    issued = _sign_in(create_user, machine)
    lane["raise"] = GatewayDown()
    resp = _client(bearer=issued.raw).post("/api/v1/runner/engine/model-token/", {}, format="json")
    assert resp.status_code == 503
    machine.refresh_from_db()
    assert machine.engine_auto_pause_reason == ""


@override_settings(**ENABLED)
def test_engine_credential_revoke_is_idempotent(db, create_user, machine, mt, lane):
    issued = _sign_in(create_user, machine)
    first = _client(bearer=mt).post("/api/v1/runner/engine/credential/revoke/", {}, format="json")
    second = _client(bearer=mt).post("/api/v1/runner/engine/credential/revoke/", {}, format="json")
    assert first.status_code == second.status_code == 200
    assert first.data["revoked"] is True and second.data["revoked"] is False
    assert EngineCredential.objects.get().revoked_reason == "signed_out"
    assert _client(bearer=issued.raw).post("/api/v1/runner/engine/model-token/", {}, format="json").status_code == 401


@override_settings(**ENABLED)
def test_one_live_engine_credential_per_machine_is_enforced_by_the_database(db, create_user, machine, lane):
    from django.db import IntegrityError, transaction

    _sign_in(create_user, machine)
    with pytest.raises(IntegrityError), transaction.atomic():
        EngineCredential.objects.create(
            user=create_user, dev_machine=machine, token_hash="x" * 64, expires_at=timezone.now()
        )


@override_settings(**ENABLED)
def test_remove_this_computer_revokes_the_engine_credential(db, web, create_user, workspace, machine, mt, lane):
    issued = _sign_in(create_user, machine)
    resp = web.post(
        reverse("dev-machine-revoke", kwargs={"machine_id": machine.id}),
        {"workspace": str(workspace.id)},
        format="json",
    )
    assert resp.status_code == 200, resp.data
    assert resp.data["engine_sign_in_state"] == EngineSignInState.REVOKED
    assert EngineCredential.objects.get().revoked_reason == "machine_removed"
    assert _client(bearer=issued.raw).post("/api/v1/runner/engine/model-token/", {}, format="json").status_code == 401


@override_settings(**ENABLED)
def test_web_sign_out_the_engine(db, web, create_user, workspace, workspace_b, machine, mt, lane):
    _sign_in(create_user, machine)
    url = f"/api/runners/dev-machines/{machine.id}/engine-sign-out/"
    assert reverse("dev-machine-engine-sign-out", kwargs={"machine_id": machine.id}) == url
    # A workspace the machine is not attached to does not see it.
    assert web.post(url, {"workspace": str(workspace_b.id)}, format="json").status_code == 404
    resp = web.post(url, {"workspace": str(workspace.id)}, format="json")
    assert resp.status_code == 200, resp.data
    assert resp.data["engine_sign_in_state"] == EngineSignInState.SIGNED_OUT
    assert EngineCredential.objects.get().revoked_reason == "signed_out"
    machine.refresh_from_db()
    assert machine.engine_sign_in_state == EngineSignInState.SIGNED_OUT


@override_settings(**ENABLED)
def test_account_sign_out_everywhere_revokes_engine_credentials(db, create_user, machine, lane):
    from pi_dash.runner.services.engine_credentials import revoke_engine_credentials_for_user

    _sign_in(create_user, machine)
    assert revoke_engine_credentials_for_user(create_user) == 1
    machine.refresh_from_db()
    assert machine.engine_sign_in_state == EngineSignInState.SIGNED_OUT


# --------------------------------------------------------------------------
# Heartbeat engine_state (§8)
# --------------------------------------------------------------------------


def _open_session(bearer, machine):
    resp = _client(bearer=bearer).post(
        reverse("runner:machine-session-open", kwargs={"dev_machine_id": machine.id}), {}, format="json"
    )
    assert resp.status_code == 201, resp.data
    return resp.data["session_id"]


def _session_poll(bearer, machine, sid, body=None):
    return _client(bearer=bearer).post(
        reverse("runner:machine-session-poll", kwargs={"dev_machine_id": machine.id, "sid": sid}),
        body or {},
        format="json",
    )


@override_settings(**ENABLED)
def test_heartbeat_engine_state_drives_automatic_pause(db, create_user, machine, mt, lane):
    _sign_in(create_user, machine)
    sid = _open_session(mt, machine)
    resp = _session_poll(
        mt,
        machine,
        sid,
        {"engine_state": {"state": "unavailable", "reason_code": "wallet_empty", "sign_in_state": "active"}},
    )
    assert resp.status_code == 200, resp.content
    machine.refresh_from_db()
    assert machine.engine_state["reason_code"] == "wallet_empty"
    assert machine.engine_state["sign_in_state"] == "active"
    assert machine.engine_state_reported_at is not None
    assert machine.engine_auto_pause_reason == "wallet_empty"

    _session_poll(mt, machine, sid, {"engine_state": {"state": "available"}})
    machine.refresh_from_db()
    assert machine.engine_auto_pause_reason == ""

    # Daemon-local codes are stored, not applied.
    _session_poll(mt, machine, sid, {"engine_state": {"state": "unavailable", "reason_code": "engine_not_installed"}})
    machine.refresh_from_db()
    assert machine.engine_state["reason_code"] == "engine_not_installed"
    assert machine.engine_auto_pause_reason == ""

    _session_poll(mt, machine, sid, {"engine_state": {"state": "signed_out"}})
    machine.refresh_from_db()
    assert machine.engine_auto_pause_reason == "engine_sign_in_required"


@override_settings(**ENABLED)
def test_expired_engine_credentials_pause_engines_on_the_sweep(db, create_user, machine, lane):
    from pi_dash.runner.tasks import pause_engines_with_expired_credentials

    _sign_in(create_user, machine)
    legacy = DevMachine.objects.create(
        owner=create_user, host_label="lap", provisioning=RunnerProvisioning.DESKTOP_BUNDLED
    )
    assert pause_engines_with_expired_credentials() == 0
    past = timezone.now() - timedelta(seconds=1)
    DevMachine.objects.filter(pk__in=[machine.pk, legacy.pk]).update(
        engine_sign_in_state="active", engine_credential_expires_at=past
    )
    assert pause_engines_with_expired_credentials() == 1
    machine.refresh_from_db()
    legacy.refresh_from_db()
    assert machine.current_engine_sign_in_state() == EngineSignInState.EXPIRED
    assert machine.engine_auto_pause_reason == "engine_sign_in_required"
    assert legacy.engine_auto_pause_reason == ""


def test_heartbeat_cannot_clear_sign_in_pause_without_live_credential(db, machine, mt):
    DevMachine.objects.filter(pk=machine.pk).update(engine_auto_pause_reason="engine_sign_in_required")
    sid = _open_session(mt, machine)
    _session_poll(mt, machine, sid, {"engine_state": {"state": "available"}})
    machine.refresh_from_db()
    assert machine.engine_auto_pause_reason == "engine_sign_in_required"


def test_legacy_machine_never_gets_sign_in_required(db, create_user, workspace):
    legacy = DevMachine.objects.create(
        owner=create_user, host_label="lap", provisioning=RunnerProvisioning.DESKTOP_BUNDLED
    )
    raw = _mint_mt(create_user, workspace, legacy)
    sid = _open_session(raw, legacy)
    _session_poll(raw, legacy, sid, {"engine_state": {"state": "signed_out"}})
    legacy.refresh_from_db()
    assert legacy.engine_auto_pause_reason == ""


# --------------------------------------------------------------------------
# MachineToken expiry (§4.3, §13)
# --------------------------------------------------------------------------


def test_machine_token_gets_sixty_day_idle_expiry_renewed_on_use(db, machine, mt):
    token = MachineToken.objects.get()
    assert abs(token.expires_at - (timezone.now() + timedelta(days=60))) < timedelta(minutes=1)
    MachineToken.objects.update(expires_at=timezone.now() + timedelta(days=1))
    assert _client(bearer=mt).get("/api/v1/runner/engine/profile/").status_code == 200
    token.refresh_from_db()
    assert token.expires_at > timezone.now() + timedelta(days=59)
    assert token.last_used_at is not None


def test_expired_machine_token_is_rejected_everywhere(db, machine, mt, workspace):
    MachineToken.objects.update(expires_at=timezone.now() - timedelta(seconds=1))
    resp = _client(bearer=mt).get("/api/v1/runner/engine/profile/")
    assert resp.status_code == 401
    assert "machine_token_expired" in str(resp.data)
    # X-Api-Key surface (CLI commands).
    assert _client(api_key=mt).get("/api/v1/auth/workspaces/").status_code in (401, 403)
    # Runner-plane surface.
    runner = Runner.objects.create(owner=machine.owner, workspace=workspace, dev_machine=machine, name="r1")
    resp = _client(bearer=mt).post(
        reverse("runner:runner-session-open", kwargs={"runner_id": runner.id}), {}, format="json"
    )
    assert resp.status_code == 401


def test_machine_token_expiry_data_migration(db, machine, mt):
    import importlib

    module = importlib.import_module("pi_dash.runner.migrations.0037_machine_token_expiry_data")
    used = timezone.now() - timedelta(days=10)
    token = MachineToken.objects.get()
    MachineToken.objects.filter(pk=token.pk).update(expires_at=None, last_used_at=used)
    never = MachineToken.objects.create(
        user=machine.owner, workspace=token.workspace, dev_machine=None, host_label="other", token_hash="h2"
    )
    MachineToken.objects.filter(pk=never.pk).update(expires_at=None, last_used_at=None)
    with connection.cursor() as cursor:
        cursor.execute(module.FORWARD_SQL)
    token.refresh_from_db()
    never.refresh_from_db()
    assert abs(token.expires_at - (used + timedelta(days=60))) < timedelta(seconds=1)
    assert abs(never.expires_at - (timezone.now() + timedelta(days=60))) < timedelta(minutes=1)


# --------------------------------------------------------------------------
# One machine session across workspaces (§4.9)
# --------------------------------------------------------------------------


def test_one_machine_session_serves_every_workspace(db, web, create_user, workspace, workspace_b, machine, _isolate):
    mt_a = _mint_mt(create_user, workspace, machine)
    mt_b = _mint_mt(create_user, workspace_b, machine)
    sid = _open_session(mt_a, machine)
    MachineSession.objects.filter(pk=sid).update(last_seen_at=timezone.now())
    # Poll with workspace B's token.
    assert _session_poll(mt_b, machine, sid).status_code == 200

    # Machine control for workspace B is delivered on the session opened with A.
    resp = web.post(
        reverse("dev-machine-create-runner", kwargs={"machine_id": machine.id}),
        {"workspace": str(workspace_b.id), "project": "DEF", "agent": "codex"},
        format="json",
    )
    assert resp.status_code == 202, resp.data
    entries = _isolate.streams[machine_outbox.stream_key(machine.id)]
    payload = json.loads(entries[-1][1]["payload"])
    assert payload["workspace_slug"] == "second-ws"
    # And the daemon reports the result with workspace B's token.
    result = _client(bearer=mt_b).post(
        reverse(
            "runner:machine-command-result",
            kwargs={"dev_machine_id": machine.id, "request_id": resp.data["request_id"]},
        ),
        {"status": "ok", "runner_id": "", "runner_name": "x"},
        format="json",
    )
    assert result.status_code == 204

    # Revoking the token the session was opened with ends the session.
    MachineToken.objects.filter(token_hash=tokens.hash_token(mt_a)).update(revoked_at=timezone.now())
    evicted = _session_poll(mt_b, machine, sid)
    assert evicted.status_code == 409
    assert evicted.json()["reason"] == "machine_token_revoked"
    # The daemon re-opens with B's token.
    sid2 = _open_session(mt_b, machine)
    assert _session_poll(mt_b, machine, sid2).status_code == 200


def test_revoking_another_workspace_token_keeps_the_session(db, create_user, workspace, workspace_b, machine):
    mt_a = _mint_mt(create_user, workspace, machine)
    mt_b = _mint_mt(create_user, workspace_b, machine)
    sid = _open_session(mt_a, machine)
    MachineToken.objects.filter(token_hash=tokens.hash_token(mt_b)).update(revoked_at=timezone.now())
    assert _session_poll(mt_a, machine, sid).status_code == 200


# --------------------------------------------------------------------------
# Engine runners: uniqueness, naming, provisioning, legacy (§5)
# --------------------------------------------------------------------------


def _create_runner(mt_raw, **body):
    return _client(api_key=mt_raw).post("/api/v1/runner/runners/", body, format="json")


@override_settings(**ENABLED)
def test_engine_runner_requires_engine_credential_and_profile(db, create_user, machine, mt, lane):
    resp = _create_runner(mt, project="DEF", agent_kind="engine")
    assert resp.status_code == 409
    assert resp.data["reason_code"] == "engine_sign_in_required"
    _sign_in(create_user, machine)
    lane["available"] = False
    resp = _create_runner(mt, project="DEF", agent_kind="engine")
    assert resp.status_code == 409
    assert resp.data["reason_code"] == "profile_unavailable"
    with override_settings(MANAGED_RUNNER_ENABLED=False):
        lane["available"] = True
        resp = _create_runner(mt, project="DEF", agent_kind="engine")
        assert resp.data["reason_code"] == "engine_disabled"
    assert not Runner.objects.exists()


@override_settings(**ENABLED)
def test_engine_runner_is_unique_per_machine_and_project(db, create_user, workspace, machine, mt, lane):
    _sign_in(create_user, machine)
    first = _create_runner(mt, project="DEF", agent_kind="engine", name="ignored-name")
    assert first.status_code == 201, first.data
    assert first.data["provisioning"] == RunnerProvisioning.DESKTOP_BUNDLED
    assert first.data["pod_dispatch"] == "pinned_only"
    assert first.data["runner_name"] == "builtin-rich-mbp-test-workspace-def"
    # Re-adding (even naming another pod) returns the same row.
    other_pod = Pod.objects.create(
        workspace=workspace, project=Project.objects.get(workspace=workspace, identifier="DEF"), name="DEF_b"
    )
    again = _create_runner(mt, project="DEF", agent_kind="engine", pod=other_pod.name)
    assert again.status_code == 201
    assert again.data["runner_id"] == first.data["runner_id"]
    assert Runner.objects.filter(provisioning=RunnerProvisioning.DESKTOP_BUNDLED).count() == 1

    # No per-user cap: a second computer on the same account enrols its own.
    laptop = DevMachine.objects.create(owner=create_user, host_label="rich-laptop")
    laptop_mt = _mint_mt(create_user, workspace, laptop)
    _sign_in(create_user, laptop)
    second = _create_runner(laptop_mt, project="DEF", agent_kind="engine")
    assert second.status_code == 201, second.data
    assert second.data["runner_id"] != first.data["runner_id"]


@override_settings(**ENABLED)
def test_own_agent_runner_is_manual_on_the_same_machine(db, create_user, machine, mt, lane):
    _sign_in(create_user, machine)
    _create_runner(mt, project="DEF", agent_kind="engine")
    own = _create_runner(mt, project="DEF", agent_kind="claude-code", name="my-claude")
    assert own.status_code == 201, own.data
    assert own.data["provisioning"] == RunnerProvisioning.MANUAL
    assert own.data["runner_name"] == "my-claude"
    assert _create_runner(mt, project="DEF", name="plain").data["provisioning"] == RunnerProvisioning.MANUAL


@override_settings(**ENABLED)
def test_engine_names_differ_across_workspaces_with_the_same_project_identifier(
    db, create_user, workspace, workspace_b, machine, lane
):
    _sign_in(create_user, machine)
    mt_a = _mint_mt(create_user, workspace, machine)
    mt_b = _mint_mt(create_user, workspace_b, machine)
    a = _create_runner(mt_a, project="DEF", agent_kind="engine")
    b = _create_runner(mt_b, project="DEF", agent_kind="engine")
    assert a.status_code == b.status_code == 201, (a.data, b.data)
    assert a.data["runner_name"] == "builtin-rich-mbp-test-workspace-def"
    assert b.data["runner_name"] == "builtin-rich-mbp-second-ws-def"


@override_settings(**ENABLED)
def test_engine_name_clash_gets_a_numeric_suffix(db, create_user, workspace, machine, mt, lane):
    _sign_in(create_user, machine)
    pod = Pod.default_for_project(Project.objects.get(workspace=workspace, identifier="DEF"))
    # Some other live runner of this machine already has the name.
    Runner.objects.create(
        owner=create_user, workspace=workspace, pod=pod, dev_machine=machine, name="builtin-rich-mbp-test-workspace-def"
    )
    resp = _create_runner(mt, project="DEF", agent_kind="engine")
    assert resp.status_code == 201, resp.data
    assert resp.data["runner_name"] == "builtin-rich-mbp-test-workspace-def-2"


def test_long_engine_names_are_truncated_with_a_hash(db, create_user, workspace):
    machine = DevMachine.objects.create(owner=create_user, host_label="h" * 200)
    project = Project.objects.get(workspace=workspace, identifier="DEF")
    name = engine_runner_base_name(machine, workspace, project)
    assert len(name) == RUNNER_NAME_MAX
    assert name.startswith("builtin-hhh")
    import hashlib

    assert name.endswith(hashlib.sha256(str(project.pk).encode()).hexdigest()[:8])


def test_legacy_desktop_enrolment_still_works_without_engine_credential(db, create_user, workspace):
    legacy = DevMachine.objects.create(
        owner=create_user, host_label="lap", provisioning=RunnerProvisioning.DESKTOP_BUNDLED
    )
    raw = _mint_mt(create_user, workspace, legacy)
    # ``__managed enroll`` sends no agent kind and its own name.
    resp = _create_runner(raw, project="DEF", name="desktop-desktop-abc-DEF")
    assert resp.status_code == 201, resp.data
    assert resp.data["provisioning"] == RunnerProvisioning.DESKTOP_BUNDLED
    assert resp.data["runner_name"] == "desktop-desktop-abc-DEF"
    again = _create_runner(raw, project="DEF", name="desktop-desktop-abc-DEF")
    assert again.data["runner_id"] == resp.data["runner_id"]
    assert not EngineCredential.objects.exists()


def test_cap_setting_is_gone():
    from django.conf import settings

    from pi_dash.config import registry

    assert not hasattr(settings, "MANAGED_RUNNER_MAX_PER_USER_PROJECT")
    assert "MANAGED_RUNNER_MAX_PER_USER_PROJECT" not in open(registry.__file__).read()


# --------------------------------------------------------------------------
# Silent renewal is an allow-list: expiring / expired only (§4.3 table)
# --------------------------------------------------------------------------


@override_settings(**ENABLED)
def test_silent_engine_approval_refused_for_never_machine(db, web, machine, mt, lane):
    assert machine.current_engine_sign_in_state() == EngineSignInState.NEVER
    resp = _engine_login(web, mt, interactive=False, dev_machine_id=machine.id)
    assert resp.status_code == 409
    assert resp.data["error"] == "engine_silent_renewal_refused"
    assert resp.data["engine_sign_in_state"] == EngineSignInState.NEVER
    assert not EngineCredential.objects.exists()
    # "Turn on the built-in engine" is an explicit sign-in.
    explicit = _engine_login(web, mt, interactive=True, dev_machine_id=machine.id)
    assert explicit.status_code == 200, explicit.data
    assert explicit.data["engine_credential"]["token"].startswith("ec_")


@override_settings(**ENABLED)
def test_silent_engine_approval_refused_for_active_machine(db, web, create_user, machine, mt, lane):
    issued = _sign_in(create_user, machine)
    resp = _engine_login(web, mt, interactive=False, dev_machine_id=machine.id)
    assert resp.status_code == 409
    assert resp.data["engine_sign_in_state"] == EngineSignInState.ACTIVE
    # The live credential is untouched: nothing extends the 14 days.
    live = EngineCredential.objects.get(revoked_at__isnull=True)
    assert live.pk == issued.credential.pk


@override_settings(**ENABLED)
def test_silent_engine_approval_renews_an_expired_engine(db, web, create_user, machine, mt, lane):
    _sign_in(create_user, machine)
    _set_expiry(machine, timezone.now() - timedelta(minutes=1))
    assert machine.current_engine_sign_in_state() == EngineSignInState.EXPIRED
    resp = _engine_login(web, mt, interactive=False, dev_machine_id=machine.id)
    assert resp.status_code == 200, resp.data
    assert resp.data["engine_sign_in_state"] == EngineSignInState.ACTIVE


@override_settings(**ENABLED)
def test_silent_cli_login_issues_engine_only_when_due(db, web, create_user, workspace, lane):
    machine_id = "6f1c7d1e-1111-4a2b-9c3d-0000000000a1"
    # ``never``: a silent (app) sign-in does not turn the engine on.
    first = _cli_login(web, machine_id, interactive=False)
    assert first.status_code == 201, first.data
    assert "engine_credential" not in first.data
    assert first.data["engine_sign_in_state"] == EngineSignInState.NEVER
    # A person approving turns it on.
    on = _cli_login(web, machine_id, interactive=True)
    token = on.data["engine_credential"]["token"]
    # ``active``: a silent sign-in leaves the live credential alone.
    again = _cli_login(web, machine_id, interactive=False)
    assert "engine_credential" not in again.data
    assert EngineCredential.objects.get(revoked_at__isnull=True).token_hash == tokens.hash_token(token)
    # ``expiring``: renewed silently.
    machine = DevMachine.objects.get(pk=machine_id)
    _make_expiring(machine)
    renewed = _cli_login(web, machine_id, interactive=False)
    assert renewed.data["engine_credential"]["token"].startswith("ec_")


@override_settings(**ENABLED)
def test_silent_issue_rechecks_state_on_locked_row(db, create_user, machine):
    """A sign-out committed after the caller's unlocked check wins."""
    from pi_dash.runner.services.engine_credentials import EngineCredentialError, revoke_engine_credential

    _sign_in(create_user, machine)
    _make_expiring(machine)
    stale = DevMachine.objects.get(pk=machine.pk)  # the caller's earlier read
    revoke_engine_credential(DevMachine.objects.get(pk=machine.pk), reason="signed_out")
    with pytest.raises(EngineCredentialError) as err:
        issue_engine_credential(user=create_user, dev_machine=stale, silent=True)
    assert err.value.code == "engine_silent_renewal_refused"
    machine.refresh_from_db()
    assert machine.engine_sign_in_state == EngineSignInState.SIGNED_OUT
    assert not EngineCredential.objects.filter(revoked_at__isnull=True).exists()


@override_settings(**ENABLED)
def test_expiry_sweep_skips_machine_renewed_after_the_query(db, create_user, machine, lane, monkeypatch):
    from pi_dash.runner.services import engine_state

    _sign_in(create_user, machine)
    _set_expiry(machine, timezone.now() - timedelta(seconds=1))
    real_filter = DevMachine.objects.select_for_update

    def renew_then_lock(*args, **kwargs):
        # The renewal commits between the sweep's query and its lock.
        DevMachine.objects.filter(pk=machine.pk).update(
            engine_credential_expires_at=timezone.now() + timedelta(days=14)
        )
        return real_filter(*args, **kwargs)

    monkeypatch.setattr(DevMachine.objects, "select_for_update", renew_then_lock)
    assert engine_state.pause_engines_with_expired_credentials() == 0
    machine.refresh_from_db()
    assert machine.engine_auto_pause_reason == ""


def test_heartbeat_does_not_replace_engine_disabled(db, machine, mt):
    DevMachine.objects.filter(pk=machine.pk).update(engine_auto_pause_reason="engine_disabled")
    sid = _open_session(mt, machine)
    _session_poll(mt, machine, sid, {"engine_state": {"state": "signed_out"}})
    machine.refresh_from_db()
    assert machine.engine_auto_pause_reason == "engine_disabled"
    _session_poll(
        mt, machine, sid, {"engine_state": {"state": "unavailable", "reason_code": "engine_sign_in_required"}}
    )
    machine.refresh_from_db()
    assert machine.engine_auto_pause_reason == "engine_disabled"


def test_a_report_decided_on_a_stale_read_does_not_replace_engine_disabled(db, machine):
    """PIDESKAPP-122 review: the instance switches the engine off between the
    report's unlocked read and its write; the locked re-check keeps it."""
    from pi_dash.runner.services import engine_state

    stale = DevMachine.objects.get(pk=machine.pk)  # read before the switch-off
    DevMachine.objects.filter(pk=machine.pk).update(engine_auto_pause_reason="engine_disabled")
    assert engine_state._apply(stale, "engine_sign_in_required", keep=("engine_disabled",)) is False
    machine.refresh_from_db()
    assert machine.engine_auto_pause_reason == "engine_disabled"

    stale.engine_auto_pause_reason = "wallet_empty"  # the healthy report saw a clearable reason
    assert engine_state._clear(stale, {"wallet_empty"}) is False
    machine.refresh_from_db()
    assert machine.engine_auto_pause_reason == "engine_disabled"


@override_settings(**ENABLED)
def test_silent_cli_login_reissues_a_credential_the_daemon_lost(db, web, create_user, workspace, lane):
    """PIDESKAPP-122 review: the server says ``active`` but the daemon reports
    it holds no credential (keychain wiped, reinstall). A silent sign-in
    re-issues it instead of leaving the engine off until ``expiring``."""
    machine_id = "6f1c7d1e-1111-4a2b-9c3d-0000000000a2"
    on = _cli_login(web, machine_id, interactive=True)
    assert on.data["engine_credential"]["token"].startswith("ec_")
    machine = DevMachine.objects.get(pk=machine_id)
    DevMachine.objects.filter(pk=machine.pk).update(
        engine_state={"state": "signed_out", "sign_in_state": "never"}
    )
    again = _cli_login(web, machine_id, interactive=False)
    assert again.data["engine_credential"]["token"].startswith("ec_")
    assert EngineCredential.objects.filter(dev_machine_id=machine_id, revoked_at__isnull=True).count() == 1

    # A daemon that reports its own live credential keeps it.
    DevMachine.objects.filter(pk=machine.pk).update(
        engine_state={"state": "available", "sign_in_state": "active"}
    )
    kept = _cli_login(web, machine_id, interactive=False)
    assert "engine_credential" not in kept.data
