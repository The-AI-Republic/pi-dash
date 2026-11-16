"""Contract tests: workspace webhooks (CRUD, secret regenerate, logs).

Paths (``app/urls/webhook.py``):
  GET    /api/workspaces/<slug>/webhooks/
  POST   /api/workspaces/<slug>/webhooks/
  GET    /api/workspaces/<slug>/webhooks/<id>/
  PATCH  /api/workspaces/<slug>/webhooks/<id>/
  DELETE /api/workspaces/<slug>/webhooks/<id>/
  POST   /api/workspaces/<slug>/webhooks/<id>/regenerate/
  GET    /api/workspaces/<slug>/webhook-logs/<webhook_id>/

Creation validates the URL (scheme allowlist, local-IP block, DNS) without
calling it, so ``POST`` succeeds offline against a public test host. Reads
and writes run against SQL-seeded rows. Unknown ids answer
``404 {"error": "The required object does not exist."}`` via the shared
``BaseAPIView`` handler.
"""

from __future__ import annotations

import pytest

from .conftest import ANON, DENIED, ws_url

pytestmark = pytest.mark.contract

NIL = "00000000-0000-0000-0000-000000000000"
NOT_FOUND = {"error": "The required object does not exist."}

# NOTE: the view passes a restricted ``fields=(...)`` list to the serializer,
# but the response carries the full shape anyway (the restriction is silently
# ignored server-side). The contract pins what the server actually sends —
# including ``secret_key`` on reads.
LIST_FIELDS = {
    "id",
    "url",
    "created_at",
    "updated_at",
    "deleted_at",
    "is_active",
    "secret_key",
    "project",
    "issue",
    "module",
    "cycle",
    "issue_comment",
    "is_internal",
    "version",
    "created_by",
    "updated_by",
    "workspace",
}


def hooks(world):
    return ws_url(world, "webhooks")


def test_list_empty(admin, world):
    response = admin.get(hooks(world))
    assert response.status_code == 200
    assert response.json() == []


def test_list_shape(admin, world, seeder):
    hook = seeder.create_webhook(world["workspace"]["id"])
    response = admin.get(hooks(world))
    assert response.status_code == 200
    [item] = response.json()
    assert set(item) == LIST_FIELDS
    assert item["id"] == hook["id"]
    assert item["url"] == hook["url"]
    assert item["is_active"] is True
    assert item["issue"] is True
    assert item["project"] is False


def test_detail_shape(admin, world, seeder):
    hook = seeder.create_webhook(world["workspace"]["id"])
    response = admin.get(hooks(world) + f"{hook['id']}/")
    assert response.status_code == 200
    assert set(response.json()) == LIST_FIELDS
    assert response.json()["id"] == hook["id"]


def test_detail_unknown(admin, world):
    response = admin.get(hooks(world) + f"{NIL}/")
    assert response.status_code == 404
    assert response.json() == NOT_FOUND


def test_create_shape(admin, world, seeder, susers):
    url = f"https://example.com/hook/{seeder.tag}-create"
    response = admin.post(hooks(world), json={"url": url, "issue": True, "project": True})
    assert response.status_code == 201
    body = response.json()
    assert body["url"] == url
    assert body["is_active"] is True
    assert body["issue"] is True
    assert body["project"] is True
    assert body["version"] == "v1"
    assert body["workspace"] == world["workspace"]["id"]
    assert body["secret_key"].startswith("pi_dash_wh_")
    # created_by is the session actor, not the workspace owner.
    assert body["created_by"] == susers["tadmin"]["id"]
    assert set(body) == LIST_FIELDS


def test_create_duplicate_url_conflict(admin, world, seeder):
    url = f"https://example.com/hook/{seeder.tag}-dup"
    first = admin.post(hooks(world), json={"url": url})
    assert first.status_code == 201
    second = admin.post(hooks(world), json={"url": url})
    assert second.status_code == 409
    assert second.json() == {"error": "URL already exists for the workspace"}


def test_create_rejects_non_http_scheme(admin, world):
    response = admin.post(hooks(world), json={"url": "not-a-url"})
    assert response.status_code == 400
    assert response.json() == {
        "url": ["Invalid schema. Only HTTP and HTTPS are allowed.", "Enter a valid URL."]
    }


def test_create_rejects_local_url(admin, world):
    response = admin.post(hooks(world), json={"url": "http://127.0.0.1/hook/x"})
    assert response.status_code == 400
    assert response.json() == {"url": ["Local URLs are not allowed."]}


def test_create_member_denied(member_client, world):
    response = member_client.post(hooks(world), json={"url": "https://example.com/hook/m"})
    assert response.status_code == 403
    assert response.json() == DENIED


def test_patch_toggle(admin, world, seeder):
    hook = seeder.create_webhook(world["workspace"]["id"])
    response = admin.patch(hooks(world) + f"{hook['id']}/", json={"is_active": False})
    assert response.status_code == 200
    assert set(response.json()) == LIST_FIELDS
    assert response.json()["is_active"] is False


def test_patch_unknown(admin, world):
    response = admin.patch(hooks(world) + f"{NIL}/", json={"is_active": False})
    assert response.status_code == 404
    assert response.json() == NOT_FOUND


def test_delete(admin, world, seeder):
    hook = seeder.create_webhook(world["workspace"]["id"])
    response = admin.delete(hooks(world) + f"{hook['id']}/")
    assert response.status_code == 204
    assert admin.get(hooks(world) + f"{hook['id']}/").json() == NOT_FOUND


def test_delete_unknown(admin, world):
    response = admin.delete(hooks(world) + f"{NIL}/")
    assert response.status_code == 404
    assert response.json() == NOT_FOUND


def test_delete_member_denied(member_client, world, seeder):
    hook = seeder.create_webhook(world["workspace"]["id"])
    response = member_client.delete(hooks(world) + f"{hook['id']}/")
    assert response.status_code == 403
    assert response.json() == DENIED


def test_regenerate_rotates_secret(admin, world, seeder):
    hook = seeder.create_webhook(world["workspace"]["id"])
    before = admin.get(hooks(world) + f"{hook['id']}/").json()
    assert before["is_active"] is True

    response = admin.post(hooks(world) + f"{hook['id']}/regenerate/")
    assert response.status_code == 200
    body = response.json()
    assert body["secret_key"].startswith("pi_dash_wh_")
    assert body["secret_key"] != before["secret_key"]

    row = seeder.db.fetchone("SELECT secret_key FROM webhooks WHERE id = %s", (hook["id"],))
    assert row["secret_key"] == body["secret_key"]


def test_regenerate_unknown(admin, world):
    response = admin.post(hooks(world) + f"{NIL}/regenerate/")
    assert response.status_code == 404
    assert response.json() == NOT_FOUND


def test_logs_empty(admin, world, seeder):
    hook = seeder.create_webhook(world["workspace"]["id"])
    response = admin.get(ws_url(world, "webhook-logs", hook["id"]))
    assert response.status_code == 200
    assert response.json() == []


def test_logs_shape(admin, world, seeder):
    hook = seeder.create_webhook(world["workspace"]["id"])
    seeder.create_webhook_log(world["workspace"]["id"], hook["id"])
    response = admin.get(ws_url(world, "webhook-logs", hook["id"]))
    assert response.status_code == 200
    [item] = response.json()
    assert item["event_type"] == "push"
    assert item["request_method"] == "POST"
    assert item["retry_count"] == 0
    assert item["webhook"] == hook["id"]
    assert set(item) == {
        "id",
        "created_at",
        "updated_at",
        "event_type",
        "request_method",
        "request_headers",
        "request_body",
        "response_status",
        "response_headers",
        "response_body",
        "retry_count",
        "created_by",
        "updated_by",
        "webhook",
        "workspace",
        "deleted_at",
    }


def test_logs_unknown_webhook_empty(admin, world):
    response = admin.get(ws_url(world, "webhook-logs", NIL))
    assert response.status_code == 200
    assert response.json() == []


def test_logs_cross_tenant_denied(other_admin, world, seeder):
    hook = seeder.create_webhook(world["workspace"]["id"])
    seeder.create_webhook_log(world["workspace"]["id"], hook["id"])
    response = other_admin.get(ws_url(world, "webhook-logs", hook["id"]))
    assert response.status_code == 403
    assert response.json() == DENIED


def test_anonymous_rejected(anon, world, seeder):
    hook = seeder.create_webhook(world["workspace"]["id"])
    for response in (
        anon.get(hooks(world)),
        anon.post(hooks(world), json={"url": "https://example.com/hook/anon"}),
        anon.get(hooks(world) + f"{hook['id']}/"),
        anon.get(ws_url(world, "webhook-logs", hook["id"])),
    ):
        assert response.status_code == 401
        assert response.json() == ANON


def test_outsider_denied(outsider_client, world):
    response = outsider_client.get(hooks(world))
    assert response.status_code == 403
    assert response.json() == DENIED
