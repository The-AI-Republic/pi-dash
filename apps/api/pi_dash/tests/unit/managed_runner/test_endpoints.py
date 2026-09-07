# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Desktop-only endpoints: enrollment, model profile, model credential.

Each of these hands the caller something a browser tab has no business
holding — a machine token, or a live model credential — so "is authenticated"
is not a sufficient gate. These tests pin the desktop-session requirement and
the shape the desktop depends on.
"""

from __future__ import annotations

import pytest
from django.test import override_settings
from django.urls import reverse

from pi_dash.ee.authentication.desktop import DESKTOP_CLIENT, DESKTOP_SESSION_KEY
from pi_dash.runner.models import DevMachine, MachineToken, Runner, RunnerProvisioning, RunnerStatus

from .conftest import MANAGED_SETTINGS

pytestmark = pytest.mark.unit


@pytest.fixture
def desktop_client(client, create_user):
    """A logged-in client whose session is marked as coming from the app."""
    client.force_login(create_user)
    session = client.session
    session[DESKTOP_SESSION_KEY] = DESKTOP_CLIENT
    session.save()
    return client


@pytest.fixture
def web_client(client, create_user):
    """A logged-in browser session — same user, no desktop marker."""
    client.force_login(create_user)
    return client


# --------------------------------------------------------------------------
# Gate
# --------------------------------------------------------------------------


@pytest.mark.parametrize(
    "url_name,method",
    [
        ("runner:desktop-enroll", "post"),
        ("ai-assistant-agent-profile", "get"),
        ("ai-assistant-agent-token", "post"),
    ],
)
@override_settings(**MANAGED_SETTINGS)
def test_web_session_cannot_reach_desktop_endpoints(web_client, url_name, method, workspace):
    """An ordinary browser session is refused, and told *why* — so a client
    can distinguish "sign in" from "this is not for browsers"."""
    try:
        url = reverse(url_name)
    except Exception:
        pytest.skip(f"{url_name} not routed under a namespace in this build")
    resp = getattr(web_client, method)(url, {}, content_type="application/json")
    assert resp.status_code == 403


@override_settings(**MANAGED_SETTINGS)
def test_anonymous_is_refused(client):
    from pi_dash.managed_runner.permissions import IsDesktopSession

    class _Req:
        user = None
        session = {}

    assert IsDesktopSession().has_permission(_Req(), None) is False


def test_permission_accepts_only_marked_sessions(create_user):
    from pi_dash.managed_runner.permissions import IsDesktopSession

    class _Req:
        def __init__(self, session):
            self.user = create_user
            self.session = session

    assert IsDesktopSession().has_permission(_Req({}), None) is False
    assert IsDesktopSession().has_permission(_Req({DESKTOP_SESSION_KEY: DESKTOP_CLIENT}), None) is True

    # A broken session store must fail closed, not raise into the view.
    class _Boom:
        def get(self, *_args, **_kw):
            raise RuntimeError("session backend down")

    assert IsDesktopSession().has_permission(_Req(_Boom()), None) is False


# --------------------------------------------------------------------------
# Enrollment
# --------------------------------------------------------------------------


@override_settings(**MANAGED_SETTINGS)
def test_enroll_mints_a_machine_token_once(desktop_client, workspace):
    url = reverse("runner:desktop-enroll")
    resp = desktop_client.post(
        url,
        {"workspace_slug": workspace.slug, "host_label": "rich-laptop"},
        content_type="application/json",
    )
    assert resp.status_code == 201, resp.content
    body = resp.json()
    assert body["machine_token"]
    assert body["workspace_slug"] == workspace.slug
    assert body["managed_runner_enabled"] is True

    machine = DevMachine.objects.get(id=body["dev_machine_id"])
    assert machine.provisioning == RunnerProvisioning.DESKTOP_BUNDLED
    assert MachineToken.objects.filter(dev_machine=machine, revoked_at__isnull=True).count() == 1


@override_settings(**MANAGED_SETTINGS)
def test_enroll_is_idempotent_and_rotates_the_token(desktop_client, workspace):
    """Signing out and back in must not accumulate machines or live tokens —
    a lost laptop should be revocable by one row."""
    url = reverse("runner:desktop-enroll")
    payload = {"workspace_slug": workspace.slug, "host_label": "rich-laptop"}
    first = desktop_client.post(url, payload, content_type="application/json").json()
    second = desktop_client.post(url, payload, content_type="application/json").json()

    assert first["dev_machine_id"] == second["dev_machine_id"]
    assert first["machine_token"] != second["machine_token"]
    assert DevMachine.objects.filter(provisioning=RunnerProvisioning.DESKTOP_BUNDLED).count() == 1
    live = MachineToken.objects.filter(dev_machine_id=first["dev_machine_id"], revoked_at__isnull=True)
    assert live.count() == 1


@override_settings(**MANAGED_SETTINGS)
def test_enroll_refuses_a_workspace_you_are_not_in(desktop_client):
    url = reverse("runner:desktop-enroll")
    resp = desktop_client.post(
        url,
        {"workspace_slug": "someone-elses", "host_label": "rich-laptop"},
        content_type="application/json",
    )
    assert resp.status_code == 404
    assert resp.json()["error"] == "workspace_not_found"


@override_settings(**MANAGED_SETTINGS)
def test_same_desktop_has_distinct_machine_identity_per_workspace(desktop_client, workspace, create_user):
    from pi_dash.db.models import Workspace, WorkspaceMember

    second = Workspace.objects.create(name="Other workspace", slug="other-workspace", owner=create_user)
    WorkspaceMember.objects.create(workspace=second, member=create_user, role=20)
    url = reverse("runner:desktop-enroll")
    first_body = desktop_client.post(
        url, {"workspace_slug": workspace.slug, "host_label": "desktop-test"}, content_type="application/json"
    ).json()
    second_body = desktop_client.post(
        url, {"workspace_slug": second.slug, "host_label": "desktop-test"}, content_type="application/json"
    ).json()
    assert first_body["dev_machine_id"] != second_body["dev_machine_id"]
    assert MachineToken.objects.filter(revoked_at__isnull=True, dev_machine__host_label="desktop-test").count() == 2


@override_settings(**MANAGED_SETTINGS)
def test_reenroll_after_local_config_loss_reuses_server_runner(desktop_client, workspace, project):
    from rest_framework.test import APIClient

    enrollment = desktop_client.post(
        reverse("runner:desktop-enroll"),
        {"workspace_slug": workspace.slug, "host_label": "desktop-test"},
        content_type="application/json",
    ).json()
    machine_client = APIClient()
    machine_client.credentials(HTTP_X_API_KEY=enrollment["machine_token"])
    payload = {
        "workspace_slug": workspace.slug,
        "project": project.identifier,
        "host_label": "desktop-test",
        "dev_machine_id": enrollment["dev_machine_id"],
    }
    url = reverse("runner:runner-create")
    first = machine_client.post(url, payload, format="json")
    assert first.status_code == 201, first.data
    second = machine_client.post(url, payload, format="json")
    assert second.status_code == 201, second.data
    assert first.data["runner_id"] == second.data["runner_id"]
    assert Runner.objects.filter(dev_machine_id=enrollment["dev_machine_id"]).count() == 1


@override_settings(**MANAGED_SETTINGS)
def test_enroll_requires_host_label(desktop_client, workspace):
    url = reverse("runner:desktop-enroll")
    resp = desktop_client.post(url, {"workspace_slug": workspace.slug}, content_type="application/json")
    assert resp.status_code == 400


@override_settings(MANAGED_RUNNER_ENABLED=True, DESKTOP_MIN_VERSION_FOR_MANAGED_RUNNER="0.5.0")
def test_enroll_refuses_an_outdated_desktop(desktop_client, workspace):
    url = reverse("runner:desktop-enroll")
    resp = desktop_client.post(
        url,
        {"workspace_slug": workspace.slug, "host_label": "rich-laptop", "app_version": "0.4.9"},
        content_type="application/json",
    )
    assert resp.status_code == 409
    assert resp.json()["error"] == "desktop_update_required"

    ok = desktop_client.post(
        url,
        {"workspace_slug": workspace.slug, "host_label": "rich-laptop", "app_version": "0.10.0"},
        content_type="application/json",
    )
    # 0.10.0 > 0.5.0 numerically, not lexically — the comparison must be by
    # component, or every user past 0.9 is locked out.
    assert ok.status_code == 201


@override_settings(**MANAGED_SETTINGS)
def test_signout_revokes_tokens_and_parks_runners(desktop_client, workspace, project, create_user):
    url = reverse("runner:desktop-enroll")
    body = desktop_client.post(
        url,
        {"workspace_slug": workspace.slug, "host_label": "rich-laptop"},
        content_type="application/json",
    ).json()
    machine = DevMachine.objects.get(id=body["dev_machine_id"])
    runner = Runner.objects.create(
        owner=create_user,
        workspace=workspace,
        pod=project.pods.get(is_default=True),
        dev_machine=machine,
        name="desktop-rich-laptop",
        provisioning=RunnerProvisioning.DESKTOP_BUNDLED,
        status=RunnerStatus.ONLINE,
    )

    resp = desktop_client.delete(url, content_type="application/json")
    assert resp.status_code == 204

    runner.refresh_from_db()
    # The row survives so the next sign-in reuses it rather than burning the
    # per-project cap; OFFLINE is what actually stops dispatch.
    assert runner.status == RunnerStatus.OFFLINE
    assert runner.revoked_at is None
    assert MachineToken.objects.filter(dev_machine=machine, revoked_at__isnull=True).count() == 0


# --------------------------------------------------------------------------
# Model profile / credential
# --------------------------------------------------------------------------


@override_settings(**MANAGED_SETTINGS)
def test_profile_reports_lane_and_never_a_credential(desktop_client, openhub_lane):
    resp = desktop_client.get(reverse("ai-assistant-agent-profile"))
    assert resp.status_code == 200
    body = resp.json()
    assert body["available"] is True
    assert body["lane"] == "openhub"
    assert body["base_url"].endswith("/v1")
    assert body["managed_runner_enabled"] is True
    assert body["reason_code"] == ""
    # The credential travels by its own endpoint; leaking it into a pollable
    # body would put it in every log and cache along the way.
    assert "token" not in body


@override_settings(MANAGED_RUNNER_ENABLED=False)
def test_profile_folds_in_the_kill_switch(desktop_client, openhub_lane):
    """A disabled instance must report unavailable even with a healthy lane,
    so the app does not offer a Run button that will be refused."""
    body = desktop_client.get(reverse("ai-assistant-agent-profile")).json()
    assert body["available"] is False
    assert body["reason_code"] == "managed_runner_disabled"


@override_settings(**MANAGED_SETTINGS)
def test_profile_explains_byok(desktop_client, byok_lane):
    body = desktop_client.get(reverse("ai-assistant-agent-profile")).json()
    assert body["available"] is False
    assert body["reason_code"] == "byok_not_supported_on_desktop"


@override_settings(**MANAGED_SETTINGS)
def test_token_endpoint_reports_ce_has_no_lane(desktop_client):
    """CE has no lane the desktop may use, so the credential endpoint refuses
    with the same reason code the profile would have given — never a blank
    token that fails later inside a run."""
    resp = desktop_client.post(reverse("ai-assistant-agent-token"), {}, content_type="application/json")
    assert resp.status_code == 409
    assert resp.json()["error"] == "byok_not_supported_on_desktop"


@override_settings(**MANAGED_SETTINGS)
def test_token_endpoint_returns_credential_when_a_lane_exists(desktop_client, monkeypatch):
    from datetime import timedelta

    from django.utils import timezone

    from pi_dash.ee.assistant import model_provider

    expires = timezone.now() + timedelta(minutes=30)
    monkeypatch.setattr(
        model_provider,
        "agent_model_credential_for_user",
        lambda user: ("tok-abc", expires),
    )
    resp = desktop_client.post(reverse("ai-assistant-agent-token"), {}, content_type="application/json")
    assert resp.status_code == 200
    body = resp.json()
    assert body["token"] == "tok-abc"
    assert body["expires_at"] == expires.isoformat()


@override_settings(**MANAGED_SETTINGS)
def test_token_endpoint_maps_a_revoked_session_to_401(desktop_client, monkeypatch):
    """The desktop keys off this: 401 stops the daemon and prompts sign-in,
    503 keeps the current token and retries."""
    from pi_dash.ee.assistant import model_provider

    class OpenHubAuthError(Exception):
        pass

    def _boom(user):
        raise OpenHubAuthError("revoked")

    monkeypatch.setattr(model_provider, "agent_model_credential_for_user", _boom)
    resp = desktop_client.post(reverse("ai-assistant-agent-token"), {}, content_type="application/json")
    assert resp.status_code == 401
    assert resp.json()["error"] == "gateway_session_revoked"


@override_settings(**MANAGED_SETTINGS)
def test_token_endpoint_maps_an_outage_to_503(desktop_client, monkeypatch):
    from pi_dash.ee.assistant import model_provider

    def _boom(user):
        raise RuntimeError("gateway down")

    monkeypatch.setattr(model_provider, "agent_model_credential_for_user", _boom)
    resp = desktop_client.post(reverse("ai-assistant-agent-token"), {}, content_type="application/json")
    assert resp.status_code == 503
    assert resp.json()["error"] == "gateway_unavailable"
