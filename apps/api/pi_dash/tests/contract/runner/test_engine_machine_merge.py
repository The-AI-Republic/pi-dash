# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Machine rename, daemon PATCH, lists, replace prompt and merge (PIDESKAPP-122).

``.ai_design/built_engine_as_runner/design.md`` §4.10, §11 and the merge row
of §13. Shares fixtures and helpers with ``test_engine_identity``.
"""

from __future__ import annotations

from datetime import timedelta

import pytest
from django.test import override_settings
from django.urls import reverse
from django.utils import timezone

from pi_dash.db.models import Issue, Project, State, User
from pi_dash.runner.models import (
    AgentRun,
    AgentRunStatus,
    DevMachine,
    EngineCredential,
    EngineSignInState,
    MachineSession,
    MachineToken,
    Pod,
    Runner,
    RunnerProvisioning,
    RunnerStatus,
)
from pi_dash.runner.services.engine_credentials import issue_engine_credential

from .conftest import _client, _mint_mt
from .test_engine_identity import ENABLED, _create_runner, _sign_in

pytestmark = [pytest.mark.contract, pytest.mark.usefixtures("_isolate")]


# --------------------------------------------------------------------------
# Rename, daemon PATCH, lists (§4.10, §11)
# --------------------------------------------------------------------------


def test_owner_renames_machine_through_web_patch(db, web, workspace, machine, mt):
    url = reverse("dev-machine-delete", kwargs={"machine_id": machine.id})
    resp = web.patch(url, {"workspace": str(workspace.id), "label": "  Studio   Mac "}, format="json")
    assert resp.status_code == 200, resp.data
    assert resp.data["label"] == "Studio Mac"
    reset = web.patch(url, {"workspace": str(workspace.id), "label": ""}, format="json")
    assert reset.data["label"] == "rich-mbp"
    assert web.patch(url, {"workspace": str(workspace.id), "label": 5}, format="json").status_code == 400
    assert web.patch(url, {"workspace": str(workspace.id), "host_label": "x"}, format="json").status_code == 400


def test_daemon_patch_pauses_engine_with_machine_token(db, create_user, workspace, machine, mt):
    url = reverse("runner:daemon-dev-machine", kwargs={"dev_machine_id": machine.id})
    resp = _client(bearer=mt).patch(url, {"engine_paused_by_owner": True, "label": "Desk"}, format="json")
    assert resp.status_code == 200, resp.data
    machine.refresh_from_db()
    assert machine.engine_paused_by_owner is True and machine.label == "Desk"
    # Only the token's own machine, and only those fields.
    other = DevMachine.objects.create(owner=create_user, host_label="other")
    other_url = reverse("runner:daemon-dev-machine", kwargs={"dev_machine_id": other.id})
    assert _client(bearer=mt).patch(other_url, {"engine_paused_by_owner": True}, format="json").status_code == 403
    assert _client(bearer=mt).patch(url, {"engine_team_runs": True}, format="json").status_code == 400
    assert _client(bearer=mt).patch(url, {"engine_paused_by_owner": "yes"}, format="json").status_code == 400


@override_settings(**ENABLED)
def test_runner_list_includes_engine_runners_with_machine_fields(db, web, create_user, workspace, machine, mt, lane):
    _sign_in(create_user, machine)
    assert _create_runner(mt, project="DEF", agent_kind="engine").status_code == 201
    assert _create_runner(mt, project="DEF", agent_kind="codex", name="mine").status_code == 201
    resp = web.get(reverse("runner-list"), {"workspace": str(workspace.id)})
    assert resp.status_code == 200
    by_name = {r["name"]: r for r in resp.data}
    engine = by_name["builtin-rich-mbp-test-workspace-def"]
    assert engine["is_engine"] is True and engine["provisioning"] == "desktop_bundled"
    assert engine["pod_dispatch"] == "pinned_only"
    assert engine["dispatch_paused"] is False
    assert engine["engine_paused"] is False
    assert engine["machine_display_name"] == "Rich MBP"
    assert str(engine["dev_machine"]) == str(machine.id)
    assert engine["dev_machine_detail"]["engine_sign_in_state"] == EngineSignInState.ACTIVE
    assert by_name["mine"]["is_engine"] is False

    machines = web.get(reverse("dev-machine-list"), {"workspace": str(workspace.id)}).data
    row = next(m for m in machines if str(m["id"]) == str(machine.id))
    assert row["engine_sign_in_state"] == EngineSignInState.ACTIVE
    assert row["engine_credential_expires_at"]
    assert row["engine_paused"] is False
    assert "engine_state" in row and "merged_into" in row


# --------------------------------------------------------------------------
# Replace prompt (§4.10)
# --------------------------------------------------------------------------


def test_replace_candidates(db, web, create_user, workspace, machine, mt):
    new = DevMachine.objects.create(owner=create_user, host_label="rich-mbp", label="new")
    new_mt = _mint_mt(create_user, workspace, new)
    online = DevMachine.objects.create(owner=create_user, host_label="rich-mbp")
    MachineSession.objects.create(dev_machine=online, last_seen_at=timezone.now())
    DevMachine.objects.create(owner=create_user, host_label="elsewhere")
    stranger = User.objects.create(email="s3@example.com", username="s3")
    DevMachine.objects.create(owner=stranger, host_label="rich-mbp")

    url = reverse("runner:dev-machine-replace-candidates", kwargs={"dev_machine_id": new.id})
    resp = _client(bearer=new_mt).get(url)
    assert resp.status_code == 200, resp.data
    assert [c["id"] for c in resp.data["candidates"]] == [str(machine.id)]
    assert web.get(url).data["candidates"][0]["id"] == str(machine.id)
    other_url = reverse("runner:dev-machine-replace-candidates", kwargs={"dev_machine_id": machine.id})
    assert _client(bearer=new_mt).get(other_url).status_code == 403


# --------------------------------------------------------------------------
# Merge (§13)
# --------------------------------------------------------------------------


@pytest.fixture
def merge_setup(db, create_user, workspace, machine):
    """Source machine (old) with an engine on DEF in pod A, an issue and
    queued runs pinned to it, plus an own-agent runner; target (new) with its
    own engine on DEF in pod B."""
    project = Project.objects.get(workspace=workspace, identifier="DEF")
    pod_a = Pod.default_for_project(project)
    pod_b = Pod.objects.create(workspace=workspace, project=project, name="DEF_b", created_by=create_user)
    target = DevMachine.objects.create(owner=create_user, host_label="rich-mbp", label="new")
    source = machine
    mt_source = _mint_mt(create_user, workspace, source)
    loser = Runner.objects.create(
        owner=create_user,
        workspace=workspace,
        pod=pod_a,
        dev_machine=source,
        name="desktop-old-DEF",
        provisioning=RunnerProvisioning.DESKTOP_BUNDLED,
    )
    own = Runner.objects.create(owner=create_user, workspace=workspace, pod=pod_a, dev_machine=source, name="my-claude")
    survivor = Runner.objects.create(
        owner=create_user,
        workspace=workspace,
        pod=pod_b,
        dev_machine=target,
        name="builtin-rich-mbp-test-workspace-def",
        provisioning=RunnerProvisioning.DESKTOP_BUNDLED,
    )
    state = State.objects.create(name="Todo", group="unstarted", project=project)
    issue = Issue.objects.create(workspace=workspace, project=project, state=state, name="Task")
    Issue.all_objects.filter(pk=issue.pk).update(assigned_runner=loser, assigned_pod=pod_a)
    own_issue = Issue.objects.create(workspace=workspace, project=project, state=state, name="Own")
    Issue.all_objects.filter(pk=own_issue.pk).update(assigned_runner=own, assigned_pod=pod_a)

    def run(**kw):
        return AgentRun.objects.create(
            workspace=workspace,
            owner=create_user,
            created_by=create_user,
            pod=pod_a,
            prompt="p",
            status=AgentRunStatus.QUEUED,
            **kw,
        )

    strict = run(work_item=issue, pinned_runner=loser, pin_strict=True)
    loose = run(pinned_runner=loser, pin_strict=False)
    own_strict = run(work_item=own_issue, pinned_runner=own, pin_strict=True)
    issue_engine_credential(user=create_user, dev_machine=source)
    return {
        "source": source,
        "target": target,
        "loser": loser,
        "own": own,
        "survivor": survivor,
        "pod_a": pod_a,
        "pod_b": pod_b,
        "issue": issue,
        "own_issue": own_issue,
        "strict": strict,
        "loose": loose,
        "own_strict": own_strict,
        "mt_source": mt_source,
    }


def _merge(web, source, target):
    return web.post(
        reverse("runner:dev-machine-merge-into", kwargs={"dev_machine_id": source.id}),
        # The web client's body shape; ``target_dev_machine_id`` is accepted too.
        {"target": str(target.id)},
        format="json",
    )


def test_merge_moves_runners_and_resolves_collision_across_pods(db, web, merge_setup):
    s = merge_setup
    resp = _merge(web, s["source"], s["target"])
    assert resp.status_code == 200, resp.data
    assert resp.data["collisions"] == [
        {"revoked_runner_id": str(s["loser"].id), "survivor_runner_id": str(s["survivor"].id)}
    ]
    assert resp.data["moved_runner_ids"] == [str(s["own"].id)]

    # The target's engine survives; the source's is revoked after its work moved.
    s["loser"].refresh_from_db()
    assert s["loser"].revoked_at is not None and s["loser"].revoked_reason == "dev_machine_merged"
    s["issue"].refresh_from_db()
    assert s["issue"].assigned_runner_id == s["survivor"].id
    assert s["issue"].assigned_pod_id == s["pod_b"].id
    s["strict"].refresh_from_db()
    assert s["strict"].status == AgentRunStatus.QUEUED
    assert s["strict"].pinned_runner_id == s["survivor"].id and s["strict"].pod_id == s["pod_b"].id
    assert s["strict"].pin_strict is True
    s["loose"].refresh_from_db()
    assert s["loose"].pinned_runner_id is None and s["loose"].pod_id == s["pod_a"].id

    # The own-agent runner moved with its id, pod, pins and queued runs.
    s["own"].refresh_from_db()
    assert s["own"].dev_machine_id == s["target"].id and s["own"].revoked_at is None
    assert s["own"].pod_id == s["pod_a"].id
    s["own_strict"].refresh_from_db()
    assert s["own_strict"].pinned_runner_id == s["own"].id and s["own_strict"].status == AgentRunStatus.QUEUED
    s["own_issue"].refresh_from_db()
    assert s["own_issue"].assigned_runner_id == s["own"].id

    # Source revoked, its credentials too.
    s["source"].refresh_from_db()
    assert s["source"].revoked_at is not None and s["source"].merged_into_id == s["target"].id
    assert not MachineToken.objects.filter(dev_machine=s["source"], revoked_at__isnull=True).exists()
    assert EngineCredential.objects.get(dev_machine=s["source"]).revoked_reason == "machine_merged"
    assert _client(bearer=s["mt_source"]).get("/api/v1/runner/engine/profile/").status_code == 401

    # Retry into the same target: the same answer, nothing changes.
    again = _merge(web, s["source"], s["target"])
    assert again.status_code == 200
    assert again.data == resp.data
    # Into a different target: refused.
    elsewhere = DevMachine.objects.create(owner=s["source"].owner, host_label="z")
    refused = _merge(web, s["source"], elsewhere)
    assert refused.status_code == 409
    assert refused.data["error"] == "already_merged_into_another_machine"


def test_merge_renames_moved_engine_runner_to_target_scheme(db, web, create_user, workspace, workspace_b, machine):
    target = DevMachine.objects.create(owner=create_user, host_label="new-host")
    project_b = Project.objects.get(workspace=workspace_b, identifier="DEF")
    moved = Runner.objects.create(
        owner=create_user,
        workspace=workspace_b,
        pod=Pod.default_for_project(project_b),
        dev_machine=machine,
        name="desktop-desktop-123-DEF",
        provisioning=RunnerProvisioning.DESKTOP_BUNDLED,
        status=RunnerStatus.ONLINE,
    )
    resp = _merge(web, machine, target)
    assert resp.status_code == 200, resp.data
    moved.refresh_from_db()
    assert moved.dev_machine_id == target.id
    assert moved.name == "builtin-new-host-second-ws-def"
    assert moved.status == RunnerStatus.OFFLINE


def test_merge_refusals(db, web, merge_setup):
    s = merge_setup
    live = MachineSession.objects.create(dev_machine=s["source"], last_seen_at=timezone.now())
    assert _merge(web, s["source"], s["target"]).data["error"] == "source_machine_has_live_session"
    MachineSession.objects.filter(pk=live.pk).update(revoked_at=timezone.now())
    assert _merge(web, s["source"], s["source"]).status_code == 400
    stranger = User.objects.create(email="s4@example.com", username="s4")
    theirs = DevMachine.objects.create(owner=stranger, host_label="t")
    assert _merge(web, s["source"], theirs).status_code == 404
    DevMachine.objects.filter(pk=s["target"].pk).update(revoked_at=timezone.now())
    assert _merge(web, s["source"], s["target"]).data["error"] == "target_machine_not_live"
    # Nothing moved.
    s["loser"].refresh_from_db()
    assert s["loser"].revoked_at is None


def test_merge_stale_session_does_not_block(db, web, merge_setup):
    s = merge_setup
    MachineSession.objects.create(dev_machine=s["source"], last_seen_at=timezone.now() - timedelta(minutes=10))
    assert _merge(web, s["source"], s["target"]).status_code == 200
    assert not MachineSession.objects.filter(dev_machine=s["source"], revoked_at__isnull=True).exists()


def test_legacy_machine_availability_follows_desktop_endpoints(db, client, create_user, workspace, lane):
    """§9: a legacy desktop machine takes ``profile_unavailable`` /
    ``engine_disabled`` from the desktop-session profile endpoint (and
    recovers there), and never needs an engine credential."""
    from pi_dash.ee.authentication.desktop import DESKTOP_CLIENT, DESKTOP_SESSION_KEY

    legacy = DevMachine.objects.create(
        owner=create_user, host_label="lap", provisioning=RunnerProvisioning.DESKTOP_BUNDLED
    )
    client.force_login(create_user)
    session = client.session
    session[DESKTOP_SESSION_KEY] = DESKTOP_CLIENT
    session.save()

    with override_settings(MANAGED_RUNNER_ENABLED=False):
        assert client.get(reverse("ai-assistant-agent-profile")).status_code == 200
    legacy.refresh_from_db()
    assert legacy.engine_auto_pause_reason == "engine_disabled"
    with override_settings(MANAGED_RUNNER_ENABLED=True):
        lane["available"] = False
        client.get(reverse("ai-assistant-agent-profile"))
        legacy.refresh_from_db()
        assert legacy.engine_auto_pause_reason == "profile_unavailable"
        lane["available"] = True
        client.get(reverse("ai-assistant-agent-profile"))
        resp = client.post(reverse("ai-assistant-agent-token"), {}, content_type="application/json")
        assert resp.status_code == 200
    legacy.refresh_from_db()
    assert legacy.engine_auto_pause_reason == ""
    assert legacy.engine_sign_in_state == EngineSignInState.NEVER


def _merge_with_token(token, source, target):
    return _client(bearer=token).post(
        reverse("runner:dev-machine-merge-into", kwargs={"dev_machine_id": source.id}),
        {"target_dev_machine_id": str(target.id)},
        format="json",
    )


def test_merge_with_a_machine_token_only_into_its_own_machine(db, create_user, workspace, merge_setup):
    s = merge_setup
    # The old machine's token cannot pull itself into another machine.
    assert _merge_with_token(s["mt_source"], s["source"], s["target"]).status_code == 403
    # Nor can a third machine's token merge into the target.
    third = DevMachine.objects.create(owner=create_user, host_label="third")
    assert _merge_with_token(_mint_mt(create_user, workspace, third), s["source"], s["target"]).status_code == 403
    s["loser"].refresh_from_db()
    assert s["loser"].revoked_at is None
    # The target's own token (`pidash auth login` on the computer it now
    # serves) may: the CLI's "Replace it".
    resp = _merge_with_token(_mint_mt(create_user, workspace, s["target"]), s["source"], s["target"])
    assert resp.status_code == 200, resp.data
    s["own"].refresh_from_db()
    assert s["own"].dev_machine_id == s["target"].id


def test_merge_needs_authentication(db, merge_setup):
    s = merge_setup
    resp = _client().post(
        reverse("runner:dev-machine-merge-into", kwargs={"dev_machine_id": s["source"].id}),
        {"target_dev_machine_id": str(s["target"].id)},
        format="json",
    )
    assert resp.status_code in (401, 403)
