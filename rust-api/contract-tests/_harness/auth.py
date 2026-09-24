"""HTTP authentication over the public login flow (no Django internals).

Session cookies carry the ``Secure`` flag, so plain ``http://`` test runs
cannot rely on a cookie jar to round-trip them: the jar stores them but will
not send them back over cleartext. This module therefore manages cookies
manually — it extracts ``session-id`` from the login response and sends it
back as an explicit ``Cookie`` header, which works over both HTTP and HTTPS.

Flow (all public endpoints, identical on Django and the Rust port):
1. ``GET /auth/get-csrf-token/`` -> ``{"csrf_token": ...}`` + ``csrftoken`` cookie.
2. ``POST /auth/sign-in/`` (form) with the token -> 302 + ``session-id`` cookie.
"""
import urllib.parse

import httpx

SESSION_COOKIE = "session-id"


def login_session_cookie(base_url: str, email: str, password: str) -> str:
    probe = httpx.Client(base_url=base_url, timeout=30)
    r = probe.get("/auth/get-csrf-token/")
    r.raise_for_status()
    token = r.json()["csrf_token"]
    csrf_cookie = r.cookies.get("csrftoken")
    assert csrf_cookie, "no csrftoken cookie from /auth/get-csrf-token/"
    body = urllib.parse.urlencode(
        {"email": email, "password": password, "csrfmiddlewaretoken": token}
    )
    r2 = httpx.post(
        f"{base_url}/auth/sign-in/",
        content=body,
        headers={
            "Cookie": f"csrftoken={csrf_cookie}",
            "Referer": f"{base_url}/",
            "Content-Type": "application/x-www-form-urlencoded",
        },
        follow_redirects=False,
        timeout=30,
    )
    assert r2.status_code == 302, f"login failed: {r2.status_code} {r2.text[:200]}"
    session_cookie = None
    for raw in r2.headers.get_list("set-cookie"):
        if raw.startswith(SESSION_COOKIE + "="):
            session_cookie = raw.split(";", 1)[0].split("=", 1)[1]
    assert session_cookie, f"no {SESSION_COOKIE} cookie in login response"
    return session_cookie


def api_client(base_url: str, session_cookie: str | None) -> httpx.Client:
    """Client sending the session cookie explicitly (see module docstring)."""
    headers = {"Cookie": f"{SESSION_COOKIE}={session_cookie}"} if session_cookie else {}
    return httpx.Client(base_url=base_url, timeout=30, headers=headers)
