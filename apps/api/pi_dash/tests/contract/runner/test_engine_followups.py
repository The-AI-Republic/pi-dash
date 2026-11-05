# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""PIDESKAPP-122 follow-ups (``built_engine_as_runner/design.md`` §5, §13, §4.3).

- ``0038_rename_engine_runners``: existing engine runners get the
  server-assigned ``builtin-<machine>-<workspace>-<project>`` names,
  deterministically and idempotently.
- ``auth/revoke/`` (the CLI's full ``pidash auth logout``) also signs the
  machine's engine out server-side.
"""

from __future__ import annotations

import importlib
from datetime import timedelta

import pytest
from django.apps import apps as django_apps
from django.test import override_settings
from django.utils import timezone

from pi_dash.db.models import APIToken, Project
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
