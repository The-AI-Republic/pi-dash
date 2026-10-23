"""Instance-admin runner list (``GET /api/instances/runners/``, PDASHOSS01-209).

The support view required by ``.ai_design/managed_runner/design.md`` §15.1 /
§23 item 8: cross-workspace, read-only, behind ``InstanceAdminPermission``,
showing per-runner ``provisioning``, ``runner_version`` and
``dev_metadata.codex_version``. Pins the gate (401 anonymous / app cookie,
403 non-admin), the exact row bytes (``isoformat()`` datetimes), the
``provisioning`` / ``status`` / ``workspace`` filters, the 400 bodies for
unknown or empty choice values, ``page`` parsing, the method fallthrough,
and the truthy-non-dict ``dev_metadata`` 500.

The list is instance-wide, so every positive test scopes itself with
``?workspace=<this test's slug>``.
"""

from __future__ import annotations

import json

import pytest

from _harness import sessions
from _harness.http import anonymous_client, api_client

ADMIN_RUNNERS = "/api/instances/runners/"

pytestmark = pytest.mark.contract

UNAUTHENTICATED = {"detail": "Authentication credentials were not provided."}
FORBIDDEN = {"detail": "You do not have permission to perform this action."}


def _cookie_client(settings, cookies: dict):
    client = api_client(settings.base_url)
    client.headers["Cookie"] = "; ".join(f"{k}={v}" for k, v in cookies.items())
    return client


def _password_field(seeder, user_id: str) -> str:
    return seeder.db.fetchval("SELECT password FROM users WHERE id = %s", (user_id,))


@pytest.fixture()
def admin_world(settings, seeder, daemon_world):
    """``daemon_world`` plus an instance-admin user and admin/app sessions."""
    admin = seeder.create_user()
    seeder.create_instance_admin(admin["id"])
    owner = daemon_world["owner"]
    session_rows = []

    def login(user_id: str, *, admin_cookie: bool) -> dict:
        login_fn = sessions.login_admin if admin_cookie else sessions.login
        cookies = login_fn(seeder.db, user_id, _password_field(seeder, user_id))
        session_rows.append(next(iter(cookies.values())))
        return cookies

    world = dict(daemon_world)
    world["admin"] = admin
    world["admin_cookies"] = login(admin["id"], admin_cookie=True)
    world["admin_app_cookies"] = login(admin["id"], admin_cookie=False)
    world["member_cookies"] = login(owner["id"], admin_cookie=True)
    yield world
    for key in session_rows:
        seeder.db.execute("DELETE FROM sessions WHERE session_key = %s", (key,))


@pytest.fixture()
def admin_client(settings, admin_world):
    with _cookie_client(settings, admin_world["admin_cookies"]) as client:
        yield client


def _scoped(world, **params):
    return {"workspace": world["workspace"]["slug"], **params}


# -- gate -------------------------------------------------------------------


def test_anonymous_is_401(settings, admin_world):
    with anonymous_client(settings.base_url) as client:
        response = client.get(ADMIN_RUNNERS)
    assert response.status_code == 401
    assert response.json() == UNAUTHENTICATED


def test_non_instance_admin_is_403(settings, admin_world):
    with _cookie_client(settings, admin_world["member_cookies"]) as client:
        response = client.get(ADMIN_RUNNERS)
    assert response.status_code == 403
    assert response.json() == FORBIDDEN


def test_app_cookie_is_anonymous_on_instances_path(settings, admin_world):
    # `instances` paths read `admin-session-id`; the app `session-id` cookie
    # of the very same instance admin is anonymous here.
    with _cookie_client(settings, admin_world["admin_app_cookies"]) as client:
        response = client.get(ADMIN_RUNNERS)
    assert response.status_code == 401
    assert response.json() == UNAUTHENTICATED


def test_gate_runs_before_filter_validation(settings, admin_world):
    with _cookie_client(settings, admin_world["member_cookies"]) as client:
        response = client.get(ADMIN_RUNNERS, params={"provisioning": "bogus"})
    assert response.status_code == 403


# -- payload ----------------------------------------------------------------


def test_admin_sees_exact_rows(seeder, admin_world, admin_client):
    world = admin_world
    bundled = seeder.create_runner(
        world["owner"]["id"], world["workspace"]["id"], world["pod"]["id"],
        name="bundled",
        provisioning="desktop_bundled",
        status="online",
        runner_version="1.2.3",
        host_label="mac-mini",
        dev_metadata={"codex_version": "rust-v0.153.4", "other": 1},
        created_at="2026-10-02T03:04:05.000450+00:00",
        last_heartbeat_at="2026-10-03T00:00:00+00:00",
    )
    manual = seeder.create_runner(
        world["owner"]["id"], world["workspace"]["id"], world["pod"]["id"],
        name="manual",
        status="revoked",
        created_at="2026-10-01T00:00:00+00:00",
        revoked_at="2026-10-04T05:06:07.100000+00:00",
    )

    response = admin_client.get(ADMIN_RUNNERS, params=_scoped(world))
    assert response.status_code == 200, response.text
    assert response.headers["content-type"].startswith("application/json")
    assert response.json() == {
        "page": 1,
        "total": 2,
        "results": [
            {
                "id": bundled["id"],
                "name": "bundled",
                "workspace_slug": world["workspace"]["slug"],
                "owner_email": world["owner"]["email"],
                "host_label": "mac-mini",
                "provisioning": "desktop_bundled",
                "status": "online",
                "runner_version": "1.2.3",
                "codex_version": "rust-v0.153.4",
                "last_heartbeat_at": "2026-10-03T00:00:00+00:00",
                "created_at": "2026-10-02T03:04:05.000450+00:00",
                "revoked_at": None,
            },
            {
                "id": manual["id"],
                "name": "manual",
                "workspace_slug": world["workspace"]["slug"],
                "owner_email": world["owner"]["email"],
                "host_label": "",
                "provisioning": "manual",
                "status": "revoked",
                "runner_version": "",
                # No codex_version reported: null (the UI renders "—").
                "codex_version": None,
                "last_heartbeat_at": None,
                "created_at": "2026-10-01T00:00:00+00:00",
                "revoked_at": "2026-10-04T05:06:07.100000+00:00",
            },
        ],
    }
    # Key order is the view's `_row` order.
    assert list(response.json()["results"][0]) == [
        "id", "name", "workspace_slug", "owner_email", "host_label",
        "provisioning", "status", "runner_version", "codex_version",
        "last_heartbeat_at", "created_at", "revoked_at",
    ]
    assert list(response.json()) == ["page", "total", "results"]


@pytest.mark.parametrize("dev_metadata", [{}, [], "", 0, False, None, {"codex_version": None}])
def test_falsy_or_missing_codex_version_is_null(seeder, admin_world, admin_client, dev_metadata):
    world = admin_world
    seeder.db.execute(
        "UPDATE runner SET dev_metadata = %s::jsonb WHERE id = %s",
        (
            "null" if dev_metadata is None else json.dumps(dev_metadata),
            seeder.create_runner(
                world["owner"]["id"], world["workspace"]["id"], world["pod"]["id"]
            )["id"],
        ),
    )
    response = admin_client.get(ADMIN_RUNNERS, params=_scoped(world))
    assert response.status_code == 200, response.text
    assert response.json()["results"][0]["codex_version"] is None


def test_truthy_non_dict_dev_metadata_is_500(seeder, admin_world, admin_client):
    # Ported bug: `(dev_metadata or {}).get(...)` raises AttributeError on a
    # truthy list, which the base view answers with its generic 500.
    world = admin_world
    seeder.create_runner(
        world["owner"]["id"], world["workspace"]["id"], world["pod"]["id"],
        dev_metadata=["x"],
    )
    response = admin_client.get(ADMIN_RUNNERS, params=_scoped(world))
    assert response.status_code == 500
    assert response.json() == {"error": "Something went wrong please try again later"}


# -- filters ----------------------------------------------------------------


def test_provisioning_filter(seeder, admin_world, admin_client):
    world = admin_world
    args = (world["owner"]["id"], world["workspace"]["id"], world["pod"]["id"])
    seeder.create_runner(*args, name="bundled", provisioning="desktop_bundled")
    seeder.create_runner(*args, name="manual")

    response = admin_client.get(
        ADMIN_RUNNERS, params=_scoped(world, provisioning="desktop_bundled")
    )
    assert response.status_code == 200, response.text
    body = response.json()
    assert [r["name"] for r in body["results"]] == ["bundled"]
    assert body["total"] == 1

    response = admin_client.get(ADMIN_RUNNERS, params=_scoped(world, provisioning="manual"))
    assert [r["name"] for r in response.json()["results"]] == ["manual"]


def test_status_filter(seeder, admin_world, admin_client):
    world = admin_world
    args = (world["owner"]["id"], world["workspace"]["id"], world["pod"]["id"])
    seeder.create_runner(*args, name="up", status="online")
    seeder.create_runner(*args, name="down", status="offline")
    response = admin_client.get(ADMIN_RUNNERS, params=_scoped(world, status="online"))
    assert response.status_code == 200, response.text
    assert [r["name"] for r in response.json()["results"]] == ["up"]


def test_workspace_filter(seeder, admin_world, admin_client):
    world = admin_world
    seeder.create_runner(
        world["owner"]["id"], world["workspace"]["id"], world["pod"]["id"], name="in-ws"
    )
    response = admin_client.get(ADMIN_RUNNERS, params=_scoped(world))
    assert [r["name"] for r in response.json()["results"]] == ["in-ws"]
    response = admin_client.get(ADMIN_RUNNERS, params={"workspace": "no-such-ws-pd209"})
    assert response.json() == {"page": 1, "total": 0, "results": []}


def test_empty_workspace_is_no_filter(seeder, admin_world, admin_client):
    world = admin_world
    seeder.create_runner(
        world["owner"]["id"], world["workspace"]["id"], world["pod"]["id"], name="any"
    )
    response = admin_client.get(ADMIN_RUNNERS + "?workspace=")
    assert response.status_code == 200, response.text
    assert response.json()["total"] >= 1


@pytest.mark.parametrize(
    "query, body",
    [
        (
            "provisioning=bogus",
            {"error": "invalid_provisioning", "detail": ["desktop_bundled", "manual"]},
        ),
        (
            "provisioning=",
            {"error": "invalid_provisioning", "detail": ["desktop_bundled", "manual"]},
        ),
        (
            "status=bogus",
            {"error": "invalid_status", "detail": ["busy", "offline", "online", "revoked"]},
        ),
        (
            "status=",
            {"error": "invalid_status", "detail": ["busy", "offline", "online", "revoked"]},
        ),
        # provisioning is validated first.
        (
            "status=bogus&provisioning=bogus",
            {"error": "invalid_provisioning", "detail": ["desktop_bundled", "manual"]},
        ),
        # QueryDict.get: the last value wins.
        (
            "provisioning=manual&provisioning=bogus",
            {"error": "invalid_provisioning", "detail": ["desktop_bundled", "manual"]},
        ),
    ],
)
def test_invalid_choice_is_400(admin_world, admin_client, query, body):
    response = admin_client.get(f"{ADMIN_RUNNERS}?{query}")
    assert response.status_code == 400
    assert response.json() == body


# -- pagination -------------------------------------------------------------


@pytest.mark.parametrize(
    "raw, page", [("abc", 1), ("", 1), ("0", 1), ("-4", 1), ("2.0", 1), (" 2 ", 2), ("1_0", 10)]
)
def test_page_param_parsing(seeder, admin_world, admin_client, raw, page):
    world = admin_world
    seeder.create_runner(world["owner"]["id"], world["workspace"]["id"], world["pod"]["id"])
    response = admin_client.get(ADMIN_RUNNERS, params=_scoped(world, page=raw))
    assert response.status_code == 200, response.text
    body = response.json()
    assert body["page"] == page
    assert body["total"] == 1
    assert len(body["results"]) == (1 if page == 1 else 0)


def test_page_size_is_fifty_newest_first(seeder, admin_world, admin_client):
    world = admin_world
    args = (world["owner"]["id"], world["workspace"]["id"], world["pod"]["id"])
    for i in range(51):
        seeder.create_runner(
            *args, name=f"r{i:02d}", created_at=f"2026-01-01T00:{i:02d}:00+00:00"
        )
    first = admin_client.get(ADMIN_RUNNERS, params=_scoped(world)).json()
    assert first["total"] == 51
    assert len(first["results"]) == 50
    assert first["results"][0]["name"] == "r50"
    second = admin_client.get(ADMIN_RUNNERS, params=_scoped(world, page=2)).json()
    assert [r["name"] for r in second["results"]] == ["r00"]
    assert second["page"] == 2


# -- other methods ----------------------------------------------------------


@pytest.mark.parametrize("method", ["POST", "PUT", "PATCH", "DELETE"])
def test_other_methods_are_405_for_admin(admin_world, admin_client, method):
    response = admin_client.request(method, ADMIN_RUNNERS)
    assert response.status_code == 405
    assert response.json() == {"detail": f'Method "{method}" not allowed.'}
