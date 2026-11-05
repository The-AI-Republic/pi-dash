# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""PIDESKAPP-122 follow-ups (``built_engine_as_runner/design.md`` §5, §13, §4.3).

- ``0038_rename_engine_runners``: existing engine runners get the
  server-assigned ``builtin-<machine>-<workspace>-<project>`` names,
  deterministically and idempotently.
- ``auth/revoke/`` signs the machine's engine out server-side when it
  revokes the machine's last live machine token (the CLI's full ``pidash
  auth logout``), not on ``pidash auth logout --workspace X``.
"""

from __future__ import annotations

import importlib
from datetime import timedelta

import pytest
from django.apps import apps as django_apps
from django.test import override_settings
from django.urls import reverse
from django.utils import timezone

from pi_dash.db.models import APIToken, CLIDeviceCode, Project
from pi_dash.runner.models import (
    DevMachine,
    EngineAutoPauseReason,
    EngineCredential,
    EngineSignInState,
    MachineToken,
    Pod,
    Runner,
    RunnerProvisioning,
)
from pi_dash.runner.services.engine_credentials import issue_engine_credential

from .conftest import _client, _mint_mt

pytestmark = [pytest.mark.contract, pytest.mark.usefixtures("_isolate")]

ENABLED = {"MANAGED_RUNNER_ENABLED": True}

rename_engine_runners = importlib.import_module(
    "pi_dash.runner.migrations.0038_rename_engine_runners"
).rename_engine_runners


def _engine(user, workspace, machine, name, *, project="DEF", age_days=0, **extra):
    pod = Pod.default_for_project(Project.objects.get(workspace=workspace, identifier=project))
    runner = Runner.objects.create(
        owner=user,
        workspace=workspace,
        pod=pod,
        dev_machine=machine,
        name=name,
        provisioning=RunnerProvisioning.DESKTOP_BUNDLED,
        **extra,
    )
    # Fix the visiting order (oldest first) independently of insert timing.
    Runner.objects.filter(pk=runner.pk).update(created_at=timezone.now() - timedelta(days=age_days))
    return runner


def _names(*runners):
    return [Runner.objects.get(pk=r.pk).name for r in runners]


# --------------------------------------------------------------------------
# 0038: rename existing engine runners (§5, §13)
# --------------------------------------------------------------------------


def test_rename_distinguishes_workspaces_with_the_same_project_identifier(
    db, create_user, workspace, workspace_b, machine
):
    # The legacy names collide across workspaces (PIDESKAPP-53): both are
    # ``desktop-desktop-<device>-DEF``, legal only because the pods differ.
    old = "desktop-desktop-0f1e2d3c-DEF"
    a = _engine(create_user, workspace, machine, old, age_days=2)
    b = _engine(create_user, workspace_b, machine, old, age_days=1)

    assert rename_engine_runners(django_apps) == 2
    assert _names(a, b) == ["builtin-rich-mbp-test-workspace-def", "builtin-rich-mbp-second-ws-def"]


def test_rename_clash_on_one_machine_gets_numeric_suffixes_oldest_first(db, create_user, workspace, machine):
    older = _engine(create_user, workspace, machine, "desktop-desktop-aaaa-DEF", age_days=3)
    newer = _engine(create_user, workspace, machine, "desktop-desktop-bbbb-DEF", age_days=1)
    assert rename_engine_runners(django_apps) == 2
    assert _names(older, newer) == ["builtin-rich-mbp-test-workspace-def", "builtin-rich-mbp-test-workspace-def-2"]


def test_rename_avoids_names_held_by_other_runners_and_skips_non_engines(db, create_user, workspace, machine):
    pod = Pod.default_for_project(Project.objects.get(workspace=workspace, identifier="DEF"))
    # An own-agent runner on the same machine already holds the bare name.
    manual = Runner.objects.create(
        owner=create_user, workspace=workspace, pod=pod, dev_machine=machine, name="builtin-rich-mbp-test-workspace-def"
    )
    revoked = _engine(create_user, workspace, machine, "desktop-desktop-dead-DEF", age_days=5)
    Runner.objects.filter(pk=revoked.pk).update(status="revoked", revoked_at=timezone.now())
    live = _engine(create_user, workspace, machine, "desktop-desktop-cafe-DEF", age_days=1)

    assert rename_engine_runners(django_apps) == 1
    assert _names(manual, revoked, live) == [
        "builtin-rich-mbp-test-workspace-def",
        "desktop-desktop-dead-DEF",
        "builtin-rich-mbp-test-workspace-def-2",
    ]


def test_rename_is_idempotent(db, create_user, workspace, workspace_b, machine):
    other = DevMachine.objects.create(owner=create_user, host_label="Rich's Laptop")
    runners = [
        _engine(create_user, workspace, machine, "desktop-desktop-1-DEF", age_days=4),
        _engine(create_user, workspace, machine, "desktop-desktop-2-DEF", age_days=3),
        _engine(create_user, workspace_b, machine, "desktop-desktop-1-DEF", age_days=2),
        _engine(create_user, workspace, other, "desktop-desktop-3-DEF", age_days=1),
    ]
    assert rename_engine_runners(django_apps) == 4
    first = _names(*runners)
    assert first == [
        "builtin-rich-mbp-test-workspace-def",
        "builtin-rich-mbp-test-workspace-def-2",
        "builtin-rich-mbp-second-ws-def",
        "builtin-rich-s-laptop-test-workspace-def",
    ]
    # A second pass (re-applied migration, or a retry after a crash) changes nothing.
    assert rename_engine_runners(django_apps) == 0
    assert _names(*runners) == first


# --------------------------------------------------------------------------
# auth/revoke/: full logout signs the engine out (§4.3)
# --------------------------------------------------------------------------


def _logout(token):
    return _client(api_key=token).post("/api/v1/auth/revoke/", {}, format="json")


@override_settings(**ENABLED)
def test_full_logout_signs_the_engine_out_and_blocks_silent_approval(
    db, web, create_user, workspace, machine, mt, lane
):
    issued = issue_engine_credential(user=create_user, dev_machine=machine)

    resp = _logout(mt)
    assert resp.status_code == 200, resp.data

    assert MachineToken.objects.get().revoked_at is not None
    credential = EngineCredential.objects.get()
    assert credential.revoked_at is not None
    assert credential.revoked_reason == "signed_out"
    machine.refresh_from_db()
    assert machine.engine_sign_in_state == EngineSignInState.SIGNED_OUT
    assert machine.engine_credential_expires_at is None
    assert machine.engine_auto_pause_reason == EngineAutoPauseReason.ENGINE_SIGN_IN_REQUIRED
    # The engine credential no longer obtains model tokens.
    assert _client(bearer=issued.raw).post("/api/v1/runner/engine/model-token/", {}, format="json").status_code == 401

    # The app's silent renewal is refused: only an explicit sign-in clears it.
    start = _client().post("/api/v1/auth/device/start/", {"scope": "engine"}, format="json").data
    silent = web.post(
        "/api/v1/auth/device/approve/",
        {"user_code": start["user_code"], "interactive": False, "dev_machine_id": str(machine.id)},
        format="json",
    )
    assert silent.status_code == 409, silent.data
    assert silent.data["error"] == "engine_silent_renewal_refused"
    assert silent.data["engine_sign_in_state"] == EngineSignInState.SIGNED_OUT


@override_settings(**ENABLED)
def test_full_logout_without_engine_marks_it_signed_out_and_is_idempotent(db, create_user, workspace, machine, lane):
    mt_raw = _mint_mt(create_user, workspace, machine)
    assert _logout(mt_raw).status_code == 200
    machine.refresh_from_db()
    # Never signed in: still recorded as signed out, so silent renewal is refused.
    assert machine.engine_sign_in_state == EngineSignInState.SIGNED_OUT
    assert not EngineCredential.objects.exists()
    # The revoked token is now rejected (DRF answers 403: no auth header
    # challenge); the CLI clears its local token either way.
    assert _logout(mt_raw).status_code in (401, 403)
    machine.refresh_from_db()
    assert machine.engine_sign_in_state == EngineSignInState.SIGNED_OUT


@override_settings(**ENABLED)
def test_full_logout_only_signs_out_the_token_machine(db, create_user, workspace, machine, mt, lane):
    other = DevMachine.objects.create(owner=create_user, host_label="desk")
    issue_engine_credential(user=create_user, dev_machine=machine)
    issue_engine_credential(user=create_user, dev_machine=other)
    assert _logout(mt).status_code == 200
    machine.refresh_from_db()
    other.refresh_from_db()
    assert machine.engine_sign_in_state == EngineSignInState.SIGNED_OUT
    assert other.engine_sign_in_state == EngineSignInState.ACTIVE
    assert EngineCredential.objects.get(dev_machine=other).revoked_at is None


@override_settings(**ENABLED)
def test_personal_api_token_logout_leaves_engines_alone(db, create_user, workspace, machine, lane):
    issue_engine_credential(user=create_user, dev_machine=machine)
    token = APIToken.objects.create(user=create_user, label="cli", workspace=workspace)
    assert _logout(token.token).status_code == 200
    token.refresh_from_db()
    assert token.is_active is False
    machine.refresh_from_db()
    assert machine.engine_sign_in_state == EngineSignInState.ACTIVE


def _device_start(scope=None):
    body = {"scope": scope} if scope else {}
    resp = _client().post("/api/v1/auth/device/start/", body, format="json")
    assert resp.status_code == 200, resp.data
    return resp.data


def _device_poll(device_code, bearer=None):
    CLIDeviceCode.objects.filter(device_code=device_code).update(last_polled_at=timezone.now() - timedelta(seconds=30))
    return _client(bearer=bearer).post("/api/v1/auth/device/token/", {"device_code": device_code}, format="json")


@override_settings(**ENABLED)
def test_workspace_logout_leaves_the_engine_signed_in(db, create_user, workspace, workspace_b, machine, mt, lane):
    """``pidash auth logout --workspace second-ws``: the machine still has a
    live token for another workspace, so its engine keeps serving it."""
    issued = issue_engine_credential(user=create_user, dev_machine=machine)
    mt_b = _mint_mt(create_user, workspace_b, machine)

    assert _logout(mt_b).status_code == 200
    machine.refresh_from_db()
    assert machine.engine_sign_in_state == EngineSignInState.ACTIVE
    assert machine.engine_auto_pause_reason == ""
    assert EngineCredential.objects.get().revoked_at is None
    assert _client(bearer=issued.raw).post("/api/v1/runner/engine/model-token/", {}, format="json").status_code != 401

    # Signing out the last workspace too is a full logout: the engine goes.
    assert _logout(mt).status_code == 200
    machine.refresh_from_db()
    assert machine.engine_sign_in_state == EngineSignInState.SIGNED_OUT
    assert machine.engine_auto_pause_reason == EngineAutoPauseReason.ENGINE_SIGN_IN_REQUIRED


@override_settings(**ENABLED)
def test_expired_sibling_token_does_not_keep_the_engine_signed_in(
    db, create_user, workspace, workspace_b, machine, mt, lane
):
    issue_engine_credential(user=create_user, dev_machine=machine)
    _mint_mt(create_user, workspace_b, machine)
    MachineToken.objects.filter(workspace=workspace_b).update(expires_at=timezone.now() - timedelta(minutes=1))
    assert _logout(mt).status_code == 200
    machine.refresh_from_db()
    assert machine.engine_sign_in_state == EngineSignInState.SIGNED_OUT


@override_settings(**ENABLED)
def test_explicit_sign_in_after_full_logout_restores_the_engine(db, web, create_user, workspace, machine, mt, lane):
    issue_engine_credential(user=create_user, dev_machine=machine)
    assert _logout(mt).status_code == 200
    # Every machine token is gone, so the way back is ``pidash auth login``
    # with a person approving in the browser (interactive).
    start = _device_start()
    approve = web.post(
        "/api/v1/auth/device/approve/", {"user_code": start["user_code"], "interactive": True}, format="json"
    )
    assert approve.status_code == 200, approve.data
    access = _device_poll(start["device_code"]).data["access_token"]
    resp = _client(api_key=access).post(
        "/api/v1/auth/machine-token/",
        {"workspace_slug": "test-workspace", "dev_machine_id": str(machine.id), "host_label": "rich-mbp"},
        format="json",
    )
    assert resp.status_code == 201, resp.data
    assert resp.data["engine_credential"]["token"].startswith("ec_")
    machine.refresh_from_db()
    assert machine.current_engine_sign_in_state() == EngineSignInState.ACTIVE
    assert machine.engine_auto_pause_reason == ""


@override_settings(**ENABLED)
def test_grant_approved_before_full_logout_cannot_be_redeemed_after(
    db, web, create_user, workspace, machine, mt, lane
):
    issue_engine_credential(user=create_user, dev_machine=machine)
    soon = timezone.now() + timedelta(days=1)
    EngineCredential.objects.update(expires_at=soon)
    DevMachine.objects.filter(pk=machine.pk).update(engine_credential_expires_at=soon)
    # The app approves a silent renewal while the engine is expiring ...
    start = _device_start("engine")
    approve = web.post(
        "/api/v1/auth/device/approve/",
        {"user_code": start["user_code"], "interactive": False, "dev_machine_id": str(machine.id)},
        format="json",
    )
    assert approve.status_code == 200, approve.data
    # ... then the user logs out fully before the daemon redeems it.
    assert _logout(mt).status_code == 200

    # The old machine token no longer redeems anything.
    stale = _device_poll(start["device_code"], bearer=mt)
    assert stale.status_code in (401, 403), stale.data
    # Nor does a fresh machine token: the engine is signed out, and a silent
    # grant cannot clear that.
    fresh = _mint_mt(create_user, workspace, machine)
    refused = _device_poll(start["device_code"], bearer=fresh)
    assert refused.status_code == 409, refused.data
    assert refused.data["error"] == "engine_silent_renewal_refused"
    assert not EngineCredential.objects.filter(revoked_at__isnull=True).exists()


# --------------------------------------------------------------------------
# GET/PATCH dev-machines/<id>/: the daemon's view of its machine (§5, §8)
# --------------------------------------------------------------------------


def _machine_url(machine):
    return reverse("runner:daemon-dev-machine", kwargs={"dev_machine_id": machine.id})


@override_settings(**ENABLED)
def test_daemon_machine_get_lists_engine_fields_and_the_token_workspace_runner_rows(
    db, create_user, workspace, workspace_b, machine, mt, lane
):
    issue_engine_credential(user=create_user, dev_machine=machine)
    DevMachine.objects.filter(pk=machine.pk).update(
        engine_paused_by_owner=True, engine_auto_pause_reason=EngineAutoPauseReason.WALLET_EMPTY
    )
    engine = _engine(create_user, workspace, machine, "builtin-rich-mbp-test-workspace-def", age_days=3)
    Runner.objects.filter(pk=engine.pk).update(dev_metadata={"working_dir": "/home/rich/work/def"})
    gone = _engine(create_user, workspace_b, machine, "builtin-rich-mbp-second-ws-def", age_days=2)
    Runner.objects.filter(pk=gone.pk).update(status="revoked", revoked_at=timezone.now(), revoked_reason="user_revoke")
    pod = Pod.default_for_project(Project.objects.get(workspace=workspace, identifier="DEF"))
    own = Runner.objects.create(
        owner=create_user,
        workspace=workspace,
        pod=pod,
        dev_machine=machine,
        name="my-claude",
        capabilities=["agent:claude_code"],
    )
    Runner.objects.filter(pk=own.pk).update(created_at=timezone.now() - timedelta(days=1))
    # Another machine's runner is not listed.
    other = DevMachine.objects.create(owner=create_user, host_label="desk")
    _engine(create_user, workspace, other, "builtin-desk-test-workspace-def")

    # Any of the machine's workspace tokens works; the machine fields are the
    # same for both, the runner rows are the token's workspace only.
    mt_b = _mint_mt(create_user, workspace_b, machine)
    bodies = {}
    for key, token in (("a", mt), ("b", mt_b)):
        resp = _client(bearer=token).get(_machine_url(machine))
        assert resp.status_code == 200, resp.data
        body = resp.data
        assert body["id"] == str(machine.id)
        assert body["engine_paused_by_owner"] is True
        assert body["engine_auto_pause_reason"] == EngineAutoPauseReason.WALLET_EMPTY
        assert body["engine_sign_in_state"] == EngineSignInState.ACTIVE
        assert body["engine_credential_expires_at"] is not None
        bodies[key] = body
    assert {k: v for k, v in bodies["a"].items() if k != "runners"} == {
        k: v for k, v in bodies["b"].items() if k != "runners"
    }

    # Workspace A's token: its engine and own-agent rows, not B's revoked row.
    rows = bodies["a"]["runners"]
    assert [r["id"] for r in rows] == [str(engine.id), str(own.id)]
    first, mine = rows
    assert first == {
        "id": str(engine.id),
        "name": "builtin-rich-mbp-test-workspace-def",
        "workspace_slug": "test-workspace",
        "project_id": str(pod.project_id),
        "project_identifier": "DEF",
        "pod_id": str(pod.id),
        "agent_kind": "engine",
        "provisioning": RunnerProvisioning.DESKTOP_BUNDLED,
        "status": engine.status,
        "revoked_at": None,
        "revoked_reason": "",
        "working_dir": "/home/rich/work/def",
    }
    assert mine["agent_kind"] == "claude_code"
    assert mine["provisioning"] == RunnerProvisioning.MANUAL

    # Workspace B's token: only B's row, revoked included.
    (revoked,) = bodies["b"]["runners"]
    assert revoked["id"] == str(gone.id)
    assert revoked["workspace_slug"] == "second-ws"
    assert revoked["revoked_at"] is not None and revoked["status"] == "revoked"
    assert revoked["revoked_reason"] == "user_revoke"
    assert revoked["working_dir"] == ""


def test_daemon_machine_get_is_restricted_to_the_token_machine(db, create_user, workspace, machine, mt):
    other = DevMachine.objects.create(owner=create_user, host_label="desk")
    assert _client(bearer=mt).get(_machine_url(other)).status_code == 403
    # A machine token not bound to a computer.
    unbound = _mint_mt(create_user, workspace, other)
    MachineToken.objects.filter(dev_machine=other).update(dev_machine=None)
    assert _client(bearer=unbound).get(_machine_url(other)).status_code == 403
    # No credential at all.
    assert _client().get(_machine_url(machine)).status_code in (401, 403)
    # A personal API token is not a daemon credential.
    api_token = APIToken.objects.create(user=create_user, label="cli", workspace=workspace)
    assert _client(api_key=api_token.token).get(_machine_url(machine)).status_code in (401, 403)


def test_daemon_machine_patch_answers_the_same_body(db, create_user, workspace, machine, mt):
    engine = _engine(create_user, workspace, machine, "builtin-rich-mbp-test-workspace-def")
    resp = _client(bearer=mt).patch(_machine_url(machine), {"engine_paused_by_owner": True}, format="json")
    assert resp.status_code == 200, resp.data
    assert resp.data["engine_paused_by_owner"] is True
    assert resp.data["engine_auto_pause_reason"] == ""
    assert resp.data["engine_sign_in_state"] == EngineSignInState.NEVER
    assert [r["id"] for r in resp.data["runners"]] == [str(engine.id)]
    got = _client(bearer=mt).get(_machine_url(machine))
    assert got.data == resp.data
