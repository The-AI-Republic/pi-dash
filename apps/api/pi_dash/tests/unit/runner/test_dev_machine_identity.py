# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""One record per dev machine: fingerprint resolution, merging, the
``dev_machine_merged`` redirect, desktop sign-in handoff and the
``merge_duplicate_dev_machines`` command.

See ``pi_dash/runner/services/dev_machines.py`` and
``pi_dash/authentication/services/desktop_handoff.py``.
"""

from __future__ import annotations

import hashlib
from datetime import timedelta
from io import StringIO
from urllib.parse import parse_qs, urlparse
from uuid import uuid4

import pytest
from django.core.management import call_command
from django.db import transaction
from django.urls import reverse
from django.utils import timezone

from pi_dash.authentication.services import desktop_handoff
from pi_dash.authentication.services.cli_tokens import CLI_DEVICE_API_TOKEN_DESCRIPTION
from pi_dash.db.models import APIToken, User, Workspace, WorkspaceMember
from pi_dash.runner.models import (
    DesktopHandoffCode,
    DesktopHandoffKey,
    DevMachine,
    MachineSession,
    MachineToken,
    Runner,
)
from pi_dash.runner.services import tokens
from pi_dash.runner.services.dev_machines import (
    DevMachineOwnershipError,
    merge_dev_machines,
    resolve_dev_machine,
)
from pi_dash.tests.unit.runner.test_machine_sessions import _fake_redis  # noqa: F401  (autouse fixture)

pytestmark = pytest.mark.unit


def _fp(seed: str) -> str:
    return hashlib.sha256(f"pidash-machine-v1\0{seed}".encode()).hexdigest()


FP_A = _fp("machine-a")
FP_B = _fp("machine-b")


def _resolve(user, **kwargs):
    kwargs.setdefault("dev_machine_id", None)
    kwargs.setdefault("host_label", "host")
    kwargs.setdefault("fingerprint", "")
    with transaction.atomic():
        return resolve_dev_machine(user=user, **kwargs)


def _machine(owner, **kwargs):
    kwargs.setdefault("host_label", "host")
    return DevMachine.objects.create(owner=owner, **kwargs)


def _runner(owner, workspace, machine, name, **kwargs):
    return Runner.objects.create(owner=owner, workspace=workspace, dev_machine=machine, name=name, **kwargs)


def _token(user, workspace, machine, label="machine: host"):
    minted = tokens.mint_machine_token()
    row = MachineToken.objects.create(
        user=user,
        workspace=workspace,
        dev_machine=machine,
        host_label=machine.host_label,
        token_hash=minted.hashed,
        token_fingerprint=minted.fingerprint,
        label=label,
        is_service=True,
    )
    return minted.raw, row


def _other_user(email="other@example.com"):
    return User.objects.create(email=email, username=email.split("@")[0])


# -------------------- resolution rules --------------------


def test_rule1_foreign_id_raises_ownership_error(db, create_user):
    other = _machine(_other_user())
    with pytest.raises(DevMachineOwnershipError):
        _resolve(create_user, dev_machine_id=other.id)


def test_rule1_merged_id_follows_to_survivor(db, create_user):
    survivor = _machine(create_user, fingerprint=FP_A)
    old = _machine(create_user, merged_into=survivor, revoked_at=timezone.now())
    resolved = _resolve(create_user, dev_machine_id=old.id, fingerprint=FP_A)
    assert resolved.id == survivor.id


def test_rule3_known_id_with_empty_fingerprint_is_stamped_and_absorbs_duplicates(db, create_user, workspace):
    mine = _machine(create_user)
    dup = _machine(create_user, fingerprint=FP_A)
    _runner(create_user, workspace, dup, "dup_runner")
    resolved = _resolve(create_user, dev_machine_id=mine.id, fingerprint=FP_A, host_label="new-host")
    assert resolved.id == mine.id
    mine.refresh_from_db()
    dup.refresh_from_db()
    assert mine.fingerprint == FP_A
    assert mine.host_label == "new-host"
    assert dup.merged_into_id == mine.id
    assert dup.revoked_at is not None
    assert Runner.objects.get(name="dup_runner").dev_machine_id == mine.id


def test_rule3_copied_config_fingerprint_mismatch_ignores_the_id(db, create_user):
    original = _machine(create_user, fingerprint=FP_A)
    resolved = _resolve(create_user, dev_machine_id=original.id, fingerprint=FP_B)
    # Id was taken by the original computer, so a fresh record is created.
    assert resolved.id != original.id
    assert resolved.fingerprint == FP_B
    original.refresh_from_db()
    assert original.revoked_at is None and original.merged_into_id is None


def test_rule3_copied_config_lands_on_existing_candidate(db, create_user):
    original = _machine(create_user, fingerprint=FP_A)
    existing_b = _machine(create_user, fingerprint=FP_B)
    resolved = _resolve(create_user, dev_machine_id=original.id, fingerprint=FP_B)
    assert resolved.id == existing_b.id


def test_rule4_survivor_has_most_runners_then_oldest(db, create_user, workspace):
    from pi_dash.runner.services.dev_machines import _pick_survivor

    old = _machine(create_user)
    busy = _machine(create_user)
    young = _machine(create_user)
    _runner(create_user, workspace, busy, "busy_1")
    _runner(create_user, workspace, busy, "busy_2")
    _runner(create_user, workspace, young, "young_1")
    _runner(create_user, workspace, old, "old_revoked", revoked_at=timezone.now())
    assert _pick_survivor([old, busy, young]).id == busy.id
    # Revoked runners don't count; young has one live runner, old none.
    assert _pick_survivor([young, old]).id == young.id
    Runner.objects.filter(dev_machine=young).delete()
    # Tie on runners -> oldest.
    assert _pick_survivor([young, old]).id == old.id


def test_rule4_no_id_picks_candidate(db, create_user):
    existing = _machine(create_user, fingerprint=FP_A)
    resolved = _resolve(create_user, fingerprint=FP_A)
    assert resolved.id == existing.id


def test_rule5_creates_with_client_id_when_unused(db, create_user):
    wanted = uuid4()
    resolved = _resolve(create_user, dev_machine_id=wanted, fingerprint=FP_A)
    assert resolved.id == wanted
    assert resolved.fingerprint == FP_A


def test_rule5_revoked_id_gets_new_record(db, create_user):
    revoked = _machine(create_user, revoked_at=timezone.now())
    resolved = _resolve(create_user, dev_machine_id=revoked.id, fingerprint=FP_A)
    assert resolved.id != revoked.id


def test_legacy_no_id_no_fingerprint_reuses_host_label(db, create_user):
    existing = _machine(create_user, host_label="laptop")
    assert _resolve(create_user, host_label="laptop").id == existing.id


def test_unique_constraint_blocks_second_active_fingerprint(db, create_user):
    from django.db import IntegrityError

    _machine(create_user, fingerprint=FP_A)
    with pytest.raises(IntegrityError), transaction.atomic():
        _machine(create_user, fingerprint=FP_A)
    # Other owners and merged rows are unconstrained.
    _machine(_other_user(), fingerprint=FP_A)


# -------------------- merge --------------------


def test_merge_moves_runners_tokens_keys_and_revokes_sessions(db, create_user, workspace):
    second_ws = Workspace.objects.create(name="Second", slug="second-ws", owner=create_user)
    WorkspaceMember.objects.create(workspace=second_ws, member=create_user, role=20)

    survivor = _machine(create_user)
    other = _machine(create_user)
    runner = _runner(create_user, workspace, other, "other_runner")
    _, survivor_tok = _token(create_user, workspace, survivor)
    _, conflicting = _token(create_user, workspace, other)
    _, movable = _token(create_user, second_ws, other)
    session = MachineSession.objects.create(dev_machine=other)
    key = DesktopHandoffKey.objects.create(user=create_user, dev_machine=other, key_hash="x")
    older = _machine(create_user, merged_into=other, revoked_at=timezone.now())

    assert merge_dev_machines(survivor, [other]) == 1

    runner.refresh_from_db()
    conflicting.refresh_from_db()
    movable.refresh_from_db()
    survivor_tok.refresh_from_db()
    session.refresh_from_db()
    key.refresh_from_db()
    other.refresh_from_db()
    older.refresh_from_db()
    assert runner.dev_machine_id == survivor.id
    assert survivor_tok.revoked_at is None
    # One active token per (workspace, dev_machine): the duplicate is revoked
    # and stays on the old record.
    assert conflicting.revoked_at is not None and conflicting.dev_machine_id == other.id
    # No conflict in the second workspace: the token moves and keeps working.
    assert movable.revoked_at is None and movable.dev_machine_id == survivor.id
    assert session.revoked_at is not None and session.revoked_reason == "merged"
    assert key.dev_machine_id == survivor.id
    assert other.merged_into_id == survivor.id and other.revoked_at is not None
    # Chains stay flat.
    assert older.merged_into_id == survivor.id


# -------------------- endpoints: machine token exchange --------------------


def _device_token(user, workspace):
    return APIToken.objects.create(
        user=user,
        workspace=workspace,
        user_type=0,
        label="pidash CLI bridge",
        description=CLI_DEVICE_API_TOKEN_DESCRIPTION,
    )


def test_machine_token_exchange_returns_survivor_user_and_handoff_key(db, api_client, create_user, workspace):
    existing = _machine(create_user, fingerprint=FP_A, host_label="laptop")
    api_client.credentials(HTTP_X_API_KEY=_device_token(create_user, workspace).token)
    resp = api_client.post(
        "/api/v1/auth/machine-token/",
        {
            "workspace_slug": workspace.slug,
            "dev_machine_id": str(uuid4()),
            "host_label": "laptop",
            "machine_fingerprint": FP_A,
        },
        format="json",
    )
    assert resp.status_code == 201, resp.data
    assert resp.data["dev_machine_id"] == str(existing.id)
    assert resp.data["user"] == {
        "id": str(create_user.id),
        "email": create_user.email,
        "display_name": create_user.display_name,
    }
    assert resp.data["handoff_key"].startswith("dhk_")
    assert DesktopHandoffKey.objects.filter(dev_machine=existing, revoked_at__isnull=True).count() == 1


def test_second_interactive_login_revokes_previous_handoff_key(db, api_client, create_user, workspace):
    body = {
        "workspace_slug": workspace.slug,
        "dev_machine_id": str(uuid4()),
        "host_label": "laptop",
        "machine_fingerprint": FP_A,
    }
    api_client.credentials(HTTP_X_API_KEY=_device_token(create_user, workspace).token)
    first = api_client.post("/api/v1/auth/machine-token/", body, format="json").data
    api_client.credentials(HTTP_X_API_KEY=_device_token(create_user, workspace).token)
    second = api_client.post("/api/v1/auth/machine-token/", body, format="json").data
    assert first["handoff_key"] != second["handoff_key"]
    keys = DesktopHandoffKey.objects.filter(user=create_user)
    assert keys.count() == 2
    assert keys.filter(revoked_at__isnull=True).count() == 1


def test_machine_token_exchange_with_machine_token_has_no_handoff_key(db, api_client, create_user, workspace):
    machine = _machine(create_user, host_label="laptop")
    raw, _ = _token(create_user, workspace, machine)
    api_client.credentials(HTTP_X_API_KEY=raw)
    resp = api_client.post(
        "/api/v1/auth/machine-token/",
        {"workspace_slug": workspace.slug, "dev_machine_id": str(machine.id), "host_label": "laptop"},
        format="json",
    )
    assert resp.status_code == 201, resp.data
    assert "handoff_key" not in resp.data
    assert resp.data["user"]["id"] == str(create_user.id)


def test_machine_token_exchange_rejects_bad_fingerprint(db, api_client, create_user, workspace):
    api_client.credentials(HTTP_X_API_KEY=_device_token(create_user, workspace).token)
    resp = api_client.post(
        "/api/v1/auth/machine-token/",
        {
            "workspace_slug": workspace.slug,
            "dev_machine_id": str(uuid4()),
            "host_label": "laptop",
            "machine_fingerprint": "NOT-HEX",
        },
        format="json",
    )
    assert resp.status_code == 400
    assert resp.data == {"error": "invalid_machine_fingerprint"}


def test_machine_token_exchange_foreign_id_is_404(db, api_client, create_user, workspace):
    foreign = _machine(_other_user())
    api_client.credentials(HTTP_X_API_KEY=_device_token(create_user, workspace).token)
    resp = api_client.post(
        "/api/v1/auth/machine-token/",
        {"workspace_slug": workspace.slug, "dev_machine_id": str(foreign.id), "host_label": "laptop"},
        format="json",
    )
    assert resp.status_code == 404
    assert resp.data["error"] == "dev_machine_not_found"


def test_revoke_with_machine_token_revokes_handoff_keys(db, api_client, create_user, workspace):
    machine = _machine(create_user)
    raw, row = _token(create_user, workspace, machine)
    desktop_handoff.mint_handoff_key(user=create_user, dev_machine=machine)
    other_machine = _machine(create_user, host_label="other")
    desktop_handoff.mint_handoff_key(user=create_user, dev_machine=other_machine)
    api_client.credentials(HTTP_X_API_KEY=raw)
    resp = api_client.post("/api/v1/auth/revoke/", {}, format="json")
    assert resp.status_code == 200
    row.refresh_from_db()
    assert row.revoked_at is not None
    assert not DesktopHandoffKey.objects.filter(dev_machine=machine, revoked_at__isnull=True).exists()
    # Keys for other machines are untouched.
    assert DesktopHandoffKey.objects.filter(dev_machine=other_machine, revoked_at__isnull=True).exists()


# -------------------- endpoints: runner create / enroll --------------------


def test_runner_create_accepts_fingerprint_and_returns_survivor(db, api_key_client, create_user, workspace, project):
    existing = _machine(create_user, fingerprint=FP_A)
    resp = api_key_client.post(
        "/api/v1/runner/runners/",
        {
            "workspace_slug": workspace.slug,
            "project": project.identifier,
            "dev_machine_id": str(uuid4()),
            "host_label": "laptop",
            "machine_fingerprint": FP_A,
        },
        format="json",
    )
    assert resp.status_code == 201, resp.data
    assert resp.data["dev_machine_id"] == str(existing.id)


def test_runner_create_rejects_bad_fingerprint(db, api_key_client, workspace, project):
    resp = api_key_client.post(
        "/api/v1/runner/runners/",
        {"workspace_slug": workspace.slug, "project": project.identifier, "machine_fingerprint": "abc"},
        format="json",
    )
    assert resp.status_code == 400
    assert resp.data["error"] == "invalid_machine_fingerprint"


def test_runner_create_with_machine_token_stamps_fingerprint(db, api_client, create_user, workspace, project):
    machine = _machine(create_user)
    raw, _ = _token(create_user, workspace, machine)
    api_client.credentials(HTTP_X_API_KEY=raw)
    resp = api_client.post(
        "/api/v1/runner/runners/",
        {"project": project.identifier, "host_label": "host", "machine_fingerprint": FP_A},
        format="json",
    )
    assert resp.status_code == 201, resp.data
    assert resp.data["dev_machine_id"] == str(machine.id)
    machine.refresh_from_db()
    assert machine.fingerprint == FP_A


def test_runner_enroll_returns_dev_machine_id(db, api_client, create_user, workspace):
    enrollment = tokens.mint_enrollment_token()
    runner = Runner.objects.create(
        owner=create_user,
        workspace=workspace,
        name="legacy",
        enrollment_token_hash=enrollment.hashed,
    )
    existing = _machine(create_user, fingerprint=FP_A)
    resp = api_client.post(
        "/api/v1/runner/runners/enroll/",
        {"enrollment_token": enrollment.raw, "host_label": "host", "machine_fingerprint": FP_A},
        format="json",
    )
    assert resp.status_code == 201, resp.data
    assert resp.data["dev_machine_id"] == str(existing.id)
    runner.refresh_from_db()
    assert runner.dev_machine_id == existing.id


# -------------------- machine sessions: fingerprint, supervisor, 409 --------------------


def _bearer(client, raw):
    client.credentials(HTTP_AUTHORIZATION=f"Bearer {raw}")
    return client


def test_session_open_stamps_fingerprint_merges_and_stores_supervisor(db, api_client, create_user, workspace):
    machine = _machine(create_user)
    dup = _machine(create_user, fingerprint=FP_A)
    raw, _ = _token(create_user, workspace, machine)
    _bearer(api_client, raw)
    url = reverse("runner:machine-session-open", kwargs={"dev_machine_id": machine.id})
    resp = api_client.post(url, {"machine_fingerprint": FP_A, "supervisor": "desktop"}, format="json")
    assert resp.status_code == 201, resp.data
    machine.refresh_from_db()
    dup.refresh_from_db()
    assert machine.fingerprint == FP_A
    assert machine.supervisor == "desktop"
    assert dup.merged_into_id == machine.id


def test_session_open_ignores_unknown_supervisor_and_rejects_bad_fingerprint(db, api_client, create_user, workspace):
    machine = _machine(create_user, supervisor="service")
    raw, _ = _token(create_user, workspace, machine)
    _bearer(api_client, raw)
    url = reverse("runner:machine-session-open", kwargs={"dev_machine_id": machine.id})
    assert api_client.post(url, {"supervisor": "launchd"}, format="json").status_code == 201
    machine.refresh_from_db()
    assert machine.supervisor == "service"
    bad = api_client.post(url, {"machine_fingerprint": "xyz"}, format="json")
    assert bad.status_code == 400
    assert bad.data["error"] == "invalid_machine_fingerprint"


def test_session_open_with_different_fingerprint_does_not_merge(db, api_client, create_user, workspace):
    machine = _machine(create_user, fingerprint=FP_B)
    other = _machine(create_user, fingerprint=FP_A)
    raw, _ = _token(create_user, workspace, machine)
    _bearer(api_client, raw)
    url = reverse("runner:machine-session-open", kwargs={"dev_machine_id": machine.id})
    assert api_client.post(url, {"machine_fingerprint": FP_A}, format="json").status_code == 201
    other.refresh_from_db()
    assert other.merged_into_id is None


def test_merged_id_gets_409_with_survivor(db, api_client, create_user, workspace):
    survivor = _machine(create_user)
    old = _machine(create_user)
    raw, _ = _token(create_user, workspace, old)
    merge_dev_machines(survivor, [old])  # token moves to the survivor
    _bearer(api_client, raw)

    open_url = reverse("runner:machine-session-open", kwargs={"dev_machine_id": old.id})
    resp = api_client.post(open_url, {}, format="json")
    assert resp.status_code == 409
    assert resp.data == {"error": "dev_machine_merged", "dev_machine_id": str(survivor.id)}

    del_url = reverse("runner:machine-session-delete", kwargs={"dev_machine_id": old.id, "sid": uuid4()})
    assert api_client.delete(del_url).status_code == 409

    poll_url = reverse("runner:machine-session-poll", kwargs={"dev_machine_id": old.id, "sid": uuid4()})
    poll = api_client.post(poll_url, {}, format="json")
    assert poll.status_code == 409
    assert poll.json() == {"error": "dev_machine_merged", "dev_machine_id": str(survivor.id)}

    result_url = reverse(
        "runner:machine-command-result",
        kwargs={"dev_machine_id": old.id, "request_id": uuid4()},
    )
    result = api_client.post(result_url, {"status": "ok"}, format="json")
    assert result.status_code == 409
    assert result.data["dev_machine_id"] == str(survivor.id)


def test_unrelated_machine_still_403(db, api_client, create_user, workspace):
    mine = _machine(create_user)
    unrelated_survivor = _machine(create_user)
    unrelated = _machine(create_user)
    merge_dev_machines(unrelated_survivor, [unrelated])
    raw, _ = _token(create_user, workspace, mine)
    _bearer(api_client, raw)
    url = reverse("runner:machine-session-open", kwargs={"dev_machine_id": unrelated.id})
    resp = api_client.post(url, {}, format="json")
    assert resp.status_code == 403
    assert resp.data["error"] == "dev_machine_mismatch"


# -------------------- listing / delete --------------------


def test_dev_machine_list_hides_merged_rows_and_exposes_supervisor(db, session_client, create_user, workspace):
    survivor = _machine(create_user, supervisor="service")
    old = _machine(create_user)
    _token(create_user, workspace, survivor)
    _token(create_user, workspace, old)  # revoked on merge, stays on old record
    merge_dev_machines(survivor, [old])
    resp = session_client.get(reverse("dev-machine-list"), {"workspace": str(workspace.id)})
    assert resp.status_code == 200, resp.data
    ids = [row["id"] for row in resp.data]
    assert ids == [str(survivor.id)]
    assert resp.data[0]["supervisor"] == "service"


def test_delete_dev_machine_drops_merged_tombstones(db, create_user):
    from pi_dash.runner.services.runner_delete import delete_dev_machine

    survivor = _machine(create_user)
    old = _machine(create_user)
    merge_dev_machines(survivor, [old])
    delete_dev_machine(survivor, purge_local=False)
    assert not DevMachine.objects.filter(pk__in=[survivor.pk, old.pk]).exists()


# -------------------- desktop handoff --------------------


def test_handoff_code_flow_logs_in_once(db, api_client, client, create_user):
    machine = _machine(create_user, fingerprint=FP_A)
    key = desktop_handoff.mint_handoff_key(user=create_user, dev_machine=machine)
    api_client.credentials(HTTP_AUTHORIZATION=f"Bearer {key}")
    resp = api_client.post("/api/v1/auth/desktop-handoff/", {"machine_fingerprint": FP_A}, format="json")
    assert resp.status_code == 201, resp.data
    assert resp.data["expires_in"] == 60
    redeem = urlparse(resp.data["redeem_url"])
    assert redeem.scheme in ("http", "https") and redeem.netloc
    assert redeem.path == "/api/v1/auth/desktop-handoff/redeem/"
    assert parse_qs(redeem.query)["code"] == [resp.data["code"]]
    # Stored hashed only.
    assert not DesktopHandoffCode.objects.filter(code_hash=resp.data["code"]).exists()

    first = client.get(redeem.path, {"code": resp.data["code"]})
    assert first.status_code == 302
    assert urlparse(first["Location"]).path == "/"
    assert str(client.session["_auth_user_id"]) == str(create_user.id)

    client.logout()
    second = client.get(redeem.path, {"code": resp.data["code"]})
    assert second.status_code == 302
    assert "/sign-in?error=handoff_invalid" in second["Location"]
    assert "_auth_user_id" not in client.session


def test_handoff_code_expired(db, client, create_user):
    machine = _machine(create_user)
    key = desktop_handoff.mint_handoff_key(user=create_user, dev_machine=machine)
    code, ttl = desktop_handoff.create_handoff_code(raw_key=key, machine_fingerprint=None)
    assert ttl == 60
    DesktopHandoffCode.objects.update(expires_at=timezone.now() - timedelta(seconds=1))
    resp = client.get("/api/v1/auth/desktop-handoff/redeem/", {"code": code})
    assert resp.status_code == 302
    assert "error=handoff_invalid" in resp["Location"]
    assert desktop_handoff.consume_handoff_code(code) is None


def test_handoff_fingerprint_must_match(db, api_client, create_user):
    machine = _machine(create_user, fingerprint=FP_A)
    key = desktop_handoff.mint_handoff_key(user=create_user, dev_machine=machine)
    assert desktop_handoff.create_handoff_code(raw_key=key, machine_fingerprint=FP_B) is None
    assert desktop_handoff.create_handoff_code(raw_key=key, machine_fingerprint=None) is None
    api_client.credentials(HTTP_AUTHORIZATION=f"Bearer {key}")
    resp = api_client.post("/api/v1/auth/desktop-handoff/", {"machine_fingerprint": FP_B}, format="json")
    assert resp.status_code == 401
    # A machine with no fingerprint accepts any (or none).
    bare = _machine(create_user, host_label="bare")
    bare_key = desktop_handoff.mint_handoff_key(user=create_user, dev_machine=bare)
    assert desktop_handoff.create_handoff_code(raw_key=bare_key, machine_fingerprint=FP_B) is not None


def test_handoff_rejects_invalid_or_revoked_key(db, api_client, create_user):
    resp = api_client.post("/api/v1/auth/desktop-handoff/", {}, format="json")
    assert resp.status_code == 401
    api_client.credentials(HTTP_AUTHORIZATION="Bearer dhk_nope")
    assert api_client.post("/api/v1/auth/desktop-handoff/", {}, format="json").status_code == 401

    machine = _machine(create_user)
    key = desktop_handoff.mint_handoff_key(user=create_user, dev_machine=machine)
    assert desktop_handoff.revoke_handoff_keys(user=create_user) == 1
    api_client.credentials(HTTP_AUTHORIZATION=f"Bearer {key}")
    assert api_client.post("/api/v1/auth/desktop-handoff/", {}, format="json").status_code == 401


def test_handoff_code_dies_with_its_key(db, create_user):
    machine = _machine(create_user)
    key = desktop_handoff.mint_handoff_key(user=create_user, dev_machine=machine)
    code, _ = desktop_handoff.create_handoff_code(raw_key=key, machine_fingerprint=None)
    desktop_handoff.revoke_handoff_keys(user=create_user, dev_machine=machine)
    assert desktop_handoff.consume_handoff_code(code) is None


# -------------------- management command --------------------


def _drop_fingerprint_constraint():
    """Phase 1 cleans up rows from before the constraint existed; recreate
    that state (the test transaction rolls the DDL back)."""
    from django.db import connection

    with connection.cursor() as cur:
        cur.execute("DROP INDEX IF EXISTS dev_machine_one_active_per_owner_fingerprint")


def test_merge_command_dry_run_then_apply(db, create_user, workspace):
    _drop_fingerprint_constraint()
    stale = timezone.now() - timedelta(days=60)
    keeper = _machine(create_user, host_label="laptop")
    _runner(create_user, workspace, keeper, "keeper_runner")
    # Phase 1 group: same owner + fingerprint.
    fp_one = _machine(create_user, host_label="desk", fingerprint=FP_A)
    fp_two = _machine(create_user, host_label="desk", fingerprint=FP_A)
    # Phase 2 leftover: no fingerprint, no runners, stale, same host label.
    leftover = _machine(create_user, host_label="laptop", last_seen_at=stale)
    # Seen recently: kept.
    recent = _machine(create_user, host_label="laptop", last_seen_at=timezone.now())

    out = StringIO()
    call_command("merge_duplicate_dev_machines", stdout=out)
    text = out.getvalue()
    assert "WOULD MERGE" in text
    assert "same_fingerprint" in text and "stale_same_host_label" in text
    assert str(leftover.id) in text and str(fp_two.id) in text
    assert str(recent.id) not in text
    assert not DevMachine.objects.filter(merged_into__isnull=False).exists()

    out = StringIO()
    call_command("merge_duplicate_dev_machines", "--apply", stdout=out)
    assert "Merged 2 record(s)" in out.getvalue()
    fp_two.refresh_from_db()
    leftover.refresh_from_db()
    recent.refresh_from_db()
    assert fp_two.merged_into_id == fp_one.id  # tie on runners -> oldest
    assert fp_two.revoked_at is not None
    assert leftover.merged_into_id == keeper.id  # prefers the one with runners
    assert recent.merged_into_id is None

    out = StringIO()
    call_command("merge_duplicate_dev_machines", stdout=out)
    assert "No duplicate dev machines found." in out.getvalue()


def test_merge_command_owner_filter(db, create_user):
    other = _other_user()
    stale = timezone.now() - timedelta(days=60)
    _machine(other, host_label="box")
    _machine(other, host_label="box", last_seen_at=stale)
    out = StringIO()
    call_command("merge_duplicate_dev_machines", "--owner", create_user.email, stdout=out)
    assert "No duplicate dev machines found." in out.getvalue()
