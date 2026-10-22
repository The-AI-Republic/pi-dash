"""Django's per-request ``get_user`` check applies to every app route.

Changing a password leaves other sessions with a stale ``_auth_user_hash``;
Django flushes those sessions on their next request. A forged backend or
hash is likewise anonymous. These tests exercise routes that read the bare
``_auth_user_id`` (issues and pages), not ``/api/users/me/`` which verifies
the hash in its own resolver.
"""

import httpx
import pytest

from _harness.sessions import SESSION_COOKIE_NAME

from .conftest import (
    NEW_PASSWORD,
    PASSWORD,
    login_cookies,
    make_auth_project,
    tamper_session,
)


ROUTES = ("issues", "pages")


@pytest.fixture()
def app_world(db, user):
    world = make_auth_project(db, user)
    yield world
    with db.connect() as conn:
        with conn.cursor() as cur:
            cur.execute(
                "DELETE FROM project_pages WHERE project_id = %s", (world["project"]["id"],)
            )
            cur.execute("DELETE FROM pages WHERE id = %s", (world["page_id"],))
            cur.execute(
                "DELETE FROM project_members WHERE project_id = %s",
                (world["project"]["id"],),
            )
            cur.execute("DELETE FROM projects WHERE id = %s", (world["project"]["id"],))
            cur.execute(
                "DELETE FROM workspace_members WHERE workspace_id = %s",
                (world["workspace"]["id"],),
            )
            cur.execute("DELETE FROM workspaces WHERE id = %s", (world["workspace"]["id"],))
        conn.commit()


def _app_url(world: dict, route: str) -> str:
    project = world["project"]["id"]
    slug = world["workspace"]["slug"]
    return f"/api/workspaces/{slug}/projects/{project}/{route}/"


def _stale_client(base: str, cookies: dict) -> httpx.Client:
    return httpx.Client(
        base_url=base,
        cookies=cookies,
        timeout=10,
        follow_redirects=False,
    )


def _assert_denied(response: httpx.Response) -> None:
    assert response.status_code == 401, response.text[:300]


def _assert_flushed(response: httpx.Response, db, session_key: str) -> None:
    _assert_denied(response)
    deleted = [
        value
        for value in response.headers.get_list("set-cookie")
        if value.startswith(f"{SESSION_COOKIE_NAME}=")
    ]
    assert deleted, response.headers.get_list("set-cookie")
    assert "Max-Age=0" in deleted[0]
    with db.connect() as conn:
        with conn.cursor() as cur:
            cur.execute(
                "SELECT COUNT(*) FROM sessions WHERE session_key = %s", (session_key,)
            )
            assert cur.fetchone()[0] == 0


def _assert_session_kept(response: httpx.Response, db, session_key: str) -> None:
    # Django's get_user only flushes after a ModelBackend user was loaded
    # and its session hash failed. An unknown backend (or inactive user)
    # returns AnonymousUser without touching the row.
    _assert_denied(response)
    assert not [
        value
        for value in response.headers.get_list("set-cookie")
        if value.startswith(f"{SESSION_COOKIE_NAME}=")
    ]
    with db.connect() as conn:
        with conn.cursor() as cur:
            cur.execute(
                "SELECT COUNT(*) FROM sessions WHERE session_key = %s", (session_key,)
            )
            assert cur.fetchone()[0] == 1


@pytest.mark.parametrize("route", ROUTES)
def test_password_change_flushes_other_session(browser, base, db, user, app_world, route):
    first = login_cookies(user)
    second = login_cookies(user)
    second_key = second[SESSION_COOKIE_NAME]

    changer = browser(first)
    response = changer.post(
        "/auth/change-password/",
        json={"old_password": PASSWORD, "new_password": NEW_PASSWORD},
    )
    assert response.status_code == 200, response.text[:300]

    with _stale_client(base, second) as stale:
        _assert_flushed(stale.get(_app_url(app_world, route)), db, second_key)


@pytest.mark.parametrize("route", ROUTES)
def test_tampered_hash_is_flushed(base, db, user, app_world, route):
    cookies = login_cookies(user)
    session_key = cookies[SESSION_COOKIE_NAME]
    tamper_session(db, user, session_key, session_hash="bogus")

    with _stale_client(base, cookies) as stale:
        _assert_flushed(stale.get(_app_url(app_world, route)), db, session_key)


@pytest.mark.parametrize("route", ROUTES)
def test_non_model_backend_is_anonymous(base, db, user, app_world, route):
    cookies = login_cookies(user)
    session_key = cookies[SESSION_COOKIE_NAME]
    tamper_session(db, user, session_key, backend="tests.OtherBackend")

    with _stale_client(base, cookies) as stale:
        _assert_session_kept(stale.get(_app_url(app_world, route)), db, session_key)


@pytest.mark.parametrize("route", ROUTES)
def test_inactive_user_is_anonymous(base, db, user, app_world, route):
    cookies = login_cookies(user)
    session_key = cookies[SESSION_COOKIE_NAME]
    with db.connect() as conn:
        with conn.cursor() as cur:
            cur.execute("UPDATE users SET is_active = false WHERE id = %s", (user["id"],))
        conn.commit()

    with _stale_client(base, cookies) as stale:
        _assert_session_kept(stale.get(_app_url(app_world, route)), db, session_key)
