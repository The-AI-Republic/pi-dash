"""Contract tests: GitHub App install flow + inbound webhook receiver.

Paths (``app/urls/integration.py``):
  GET  /api/users/me/integrations/github/app/
  POST /api/users/me/integrations/github/app/install/
  POST /api/users/me/integrations/github/app/refresh/
  GET  /api/integrations/github/app/callback/      (AllowAny; redirects)
  POST /api/integrations/github/app/webhook/       (AllowAny; HMAC-signed)

The db-sourced App identity keys are seeded by the suite; the secrets come
from the server environment (``GITHUB_APP_WEBHOOK_SECRET`` etc. — inert
dummies in CI). Signatures are computed with stdlib hmac, so the whole
delivery state machine runs offline: no test calls api.github.com.
"""

from __future__ import annotations

import hashlib
import hmac
import json
import os
import uuid
from datetime import datetime
from urllib.parse import parse_qs, urlparse

import pytest

from .conftest import ANON, login_as, ws_url

pytestmark = pytest.mark.contract

WEBHOOK_SECRET = os.environ.get("GITHUB_APP_WEBHOOK_SECRET", "contract91-webhook-secret")
APP_SLUG = "contract-91-app-slug"


def sign(body: bytes) -> str:
    return "sha256=" + hmac.new(WEBHOOK_SECRET.encode(), body, hashlib.sha256).hexdigest()


def webhook_post(client, payload: dict, *, event: str, delivery_id=None, raw=None, signature=None):
    body = raw if raw is not None else json.dumps(payload).encode()
    headers = {
        "Content-Type": "application/json",
        "X-Hub-Signature-256": signature if signature is not None else sign(body),
        "X-GitHub-Event": event,
    }
    if delivery_id is not None:
        headers["X-GitHub-Delivery"] = delivery_id
    return client.post("/api/integrations/github/app/webhook/", content=body, headers=headers)


def redirect_params(response):
    assert response.status_code == 302
    query = parse_qs(urlparse(response.headers["location"]).query)
    return {key: values[0] for key, values in query.items()}


def test_app_status_shape(admin, world):
    response = admin.get("/api/users/me/integrations/github/app/")
    assert response.status_code == 200
    body = response.json()
    assert body["configured"] is True
    assert body["app_slug"] == APP_SLUG
    entry = next(item for item in body["workspaces"] if item["slug"] == world["workspace"]["slug"])
    assert entry["id"] == world["workspace"]["id"]
    assert entry["github_app"] == {"connected": False}


def test_app_status_excludes_member_only_workspaces(seeder, settings, other_world):
    member = seeder.create_user()
    # NOTE (rebase onto rust-dev tip): member factories take the workspace first.
    seeder.create_workspace_member(other_world["workspace"]["id"], member["id"], role=15)
    client = login_as(settings.base_url, member)
    try:
        body = client.get("/api/users/me/integrations/github/app/").json()
    finally:
        client.close()
    assert body["workspaces"] == []
    assert all(item["slug"] != other_world["workspace"]["slug"] for item in body["workspaces"])


def test_app_status_seeded_installation_shape(admin, world, seeder):
    wi = seeder.create_workspace_integration(world["workspace"]["id"], world["owner"]["id"])
    seeder.create_github_app_installation(wi["id"])
    body = admin.get("/api/users/me/integrations/github/app/").json()
    entry = next(item for item in body["workspaces"] if item["slug"] == world["workspace"]["slug"])
    assert entry["github_app"] == {
        "connected": True,
        "installation_id": entry["github_app"]["installation_id"],
        "account_login": "contract-octocat",
        "account_type": "Organization",
        "repository_selection": "selected",
        "repository_count": 3,
        "permissions": {},
        "events": [],
        "installed_at": None,
        "suspended_at": None,
        "verified_at": entry["github_app"]["verified_at"],
        "last_checked_at": entry["github_app"]["last_checked_at"],
        "last_check_error": "",
    }
    assert isinstance(entry["github_app"]["installation_id"], int)


def test_install_start_missing_slug(admin):
    response = admin.post("/api/users/me/integrations/github/app/install/", json={})
    assert response.status_code == 400
    assert response.json() == {"error": "workspace_slug is required"}


def test_install_start_unknown_workspace(admin):
    response = admin.post(
        "/api/users/me/integrations/github/app/install/",
        json={"workspace_slug": "no-such-workspace"},
    )
    assert response.status_code == 404
    assert response.json() == {"detail": "No Workspace matches the given query."}


def test_install_start_member_denied(member_client, world):
    response = member_client.post(
        "/api/users/me/integrations/github/app/install/",
        json={"workspace_slug": world["workspace"]["slug"]},
    )
    assert response.status_code == 403
    assert response.json() == {"error": "You must be a workspace admin to install the GitHub App"}


def test_install_start_admin_creates_session(admin, world):
    response = admin.post(
        "/api/users/me/integrations/github/app/install/",
        json={"workspace_slug": world["workspace"]["slug"]},
    )
    assert response.status_code == 201
    body = response.json()
    assert set(body) == {"state", "expires_at", "install_url"}
    datetime.fromisoformat(body["expires_at"])
    assert body["install_url"].startswith(f"https://github.com/apps/{APP_SLUG}/installations/new?")
    assert f"state={body['state']}" in body["install_url"]


def test_refresh_missing_slug(admin):
    response = admin.post("/api/users/me/integrations/github/app/refresh/", json={})
    assert response.status_code == 400
    assert response.json() == {"error": "workspace_slug is required"}


def test_refresh_unknown_workspace(admin):
    response = admin.post(
        "/api/users/me/integrations/github/app/refresh/",
        json={"workspace_slug": "no-such-workspace"},
    )
    assert response.status_code == 404


def test_refresh_member_denied(member_client, world):
    response = member_client.post(
        "/api/users/me/integrations/github/app/refresh/",
        json={"workspace_slug": world["workspace"]["slug"]},
    )
    assert response.status_code == 403
    assert response.json() == {"error": "You must be a workspace admin to refresh this connection"}


def test_refresh_not_installed(admin, world):
    response = admin.post(
        "/api/users/me/integrations/github/app/refresh/",
        json={"workspace_slug": world["workspace"]["slug"]},
    )
    assert response.status_code == 404
    assert response.json() == {"error": "GitHub App is not installed for this workspace"}


def test_refresh_live_check_failure_maps_to_502(admin, world, seeder):
    wi = seeder.create_workspace_integration(world["workspace"]["id"], world["owner"]["id"])
    seeder.create_github_app_installation(wi["id"])
    response = admin.post(
        "/api/users/me/integrations/github/app/refresh/",
        json={"workspace_slug": world["workspace"]["slug"]},
    )
    assert response.status_code == 502
    assert isinstance(response.json()["error"], str)


def test_callback_anonymous_redirects_login(no_redirect):
    params = redirect_params(no_redirect.get("/api/integrations/github/app/callback/"))
    assert params == {"github_app": "error", "error": "login_required"}


def test_callback_unknown_state(admin_nr):
    params = redirect_params(
        admin_nr.get("/api/integrations/github/app/callback/?state=does-not-exist")
    )
    assert params == {"github_app": "error", "error": "unknown_state"}


def test_callback_missing_code_then_replay_marks_failed(admin_nr, world):
    start = admin_nr.post(
        "/api/users/me/integrations/github/app/install/",
        json={"workspace_slug": world["workspace"]["slug"]},
    )
    assert start.status_code == 201
    state = start.json()["state"]

    params = redirect_params(
        admin_nr.get(
            f"/api/integrations/github/app/callback/?state={state}&installation_id=424242"
        )
    )
    assert params == {"github_app": "error", "error": "missing_oauth_code"}

    replay = redirect_params(
        admin_nr.get(
            f"/api/integrations/github/app/callback/?state={state}&installation_id=424242"
        )
    )
    assert replay == {"github_app": "failed"}


def test_callback_live_code_verification_failure(admin_nr, world):
    start = admin_nr.post(
        "/api/users/me/integrations/github/app/install/",
        json={"workspace_slug": world["workspace"]["slug"]},
    )
    state = start.json()["state"]
    params = redirect_params(
        admin_nr.get(
            f"/api/integrations/github/app/callback/"
            f"?state={state}&installation_id=424242&code=dummy-oauth-code"
        )
    )
    assert params == {"github_app": "error", "error": "github_verification_failed"}


def test_webhook_bad_signature(admin):
    body = json.dumps({"zen": "x"}).encode()
    response = webhook_post(admin, {}, event="ping", raw=body, signature="sha256=dead")
    assert response.status_code == 401
    assert response.json() == {"error": "Invalid signature"}


def test_webhook_missing_delivery(admin):
    response = admin.post(
        "/api/integrations/github/app/webhook/",
        content=json.dumps({"zen": "x"}).encode(),
        headers={
            "Content-Type": "application/json",
            "X-Hub-Signature-256": sign(json.dumps({"zen": "x"}).encode()),
            "X-GitHub-Event": "ping",
        },
    )
    assert response.status_code == 400
    assert response.json() == {"error": "Missing X-GitHub-Delivery"}


def test_webhook_invalid_delivery_uuid(admin):
    response = webhook_post(admin, {"zen": "x"}, event="ping", delivery_id="not-a-uuid")
    assert response.status_code == 400
    assert response.json() == {"error": "Invalid X-GitHub-Delivery"}


def test_webhook_invalid_json(admin):
    raw = b"{not json"
    response = webhook_post(
        admin, {}, event="ping", raw=raw, delivery_id=str(uuid.uuid4())
    )
    assert response.status_code == 400
    assert response.json() == {"error": "Invalid JSON"}


def test_webhook_ping_processed_and_idempotent(admin):
    delivery_id = str(uuid.uuid4())
    first = webhook_post(admin, {"zen": "contract"}, event="ping", delivery_id=delivery_id)
    assert first.status_code == 202
    assert first.json() == {"status": "processed"}
    second = webhook_post(admin, {"zen": "contract"}, event="ping", delivery_id=delivery_id)
    assert second.status_code == 202
    assert second.json() == {"status": "processed"}


def test_webhook_pull_request_without_link_skipped(admin):
    response = webhook_post(
        admin,
        {"action": "opened", "pull_request": {"number": 1}, "repository": {}},
        event="pull_request",
        delivery_id=str(uuid.uuid4()),
    )
    assert response.status_code == 202
    assert response.json() == {"status": "skipped"}


def test_webhook_installation_unknown_skipped(admin):
    response = webhook_post(
        admin,
        {"action": "created", "installation": {"id": 987654321}},
        event="installation",
        delivery_id=str(uuid.uuid4()),
    )
    assert response.status_code == 202
    assert response.json() == {"status": "skipped"}


def test_webhook_unknown_event_skipped(admin):
    response = webhook_post(
        admin, {"action": "x"}, event="star", delivery_id=str(uuid.uuid4())
    )
    assert response.status_code == 202
    assert response.json() == {"status": "skipped"}


def test_webhook_missing_config_409(admin, seeder, db):
    # The secrets stay in the server environment; deleting the db-sourced
    # identity keys trips the 409 with exactly the db keys listed.
    db.execute("DELETE FROM instance_configurations WHERE key LIKE 'GITHUB_APP\\_%%'")
    try:
        body = json.dumps({"zen": "x"}).encode()
        response = admin.post(
            "/api/integrations/github/app/webhook/",
            content=body,
            headers={
                "Content-Type": "application/json",
                "X-Hub-Signature-256": sign(body),
                "X-GitHub-Delivery": str(uuid.uuid4()),
                "X-GitHub-Event": "ping",
            },
        )
        assert response.status_code == 409
        assert response.json() == {"error": "GitHub App config missing: app_id, app_slug"}

        install = admin.post(
            "/api/users/me/integrations/github/app/install/",
            json={"workspace_slug": "x"},
        )
        assert install.status_code == 409
        assert install.json() == {
            "error": "GitHub App config missing: app_id, app_slug, client_id"
        }
    finally:
        seeder.ensure_github_app_config()


def test_webhook_anonymous_without_signature(anon):
    response = anon.post(
        "/api/integrations/github/app/webhook/",
        content=b"{}",
        headers={"Content-Type": "application/json"},
    )
    assert response.status_code == 401
    assert response.json() == {"error": "Invalid signature"}


def test_anonymous_session_endpoints_reject(anon, world):
    for response in (
        anon.get(ws_url(world, "integrations", "github")),
        anon.get("/api/users/me/integrations/github/app/"),
    ):
        assert response.status_code == 401
        assert response.json() == ANON
