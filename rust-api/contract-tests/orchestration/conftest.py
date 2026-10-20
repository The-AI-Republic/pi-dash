"""Orchestration-engine oracle (domain D-12).

No routes of its own: signals, state transitions, workpad, phase machine.
Stimuli are the surfaces that drive the engine — issue state PATCHes (with
and without X-Pi-Dash-Run-Id), the wait / re-tick endpoints, the workpad and
relations endpoints, and Celery tasks published in wire format. Asserts read
the engine's tables (issue_agent_ticker, agent_run, issues, issue_comments).
"""

import base64
import hashlib

import httpx
import pytest

from _harness import api, db, env


def _backend_up() -> bool:
    try:
        r = httpx.get(env.BASE_URL + "/api/v1/workspaces/nope/projects/nope/work-items/nope/", timeout=5)
        return r.status_code in (401, 403, 404)
    except Exception:
        return False


def pytest_configure(config):
    config.addinivalue_line("markers", "contract: backend behavior contract tests")


@pytest.fixture(scope="session", autouse=True)
def _backend():
    if not _backend_up():
        pytest.skip(
            f"backend not reachable at {env.BASE_URL}; boot Django per "
            "rust-api/contract-tests/README.md and set BASE_URL/DATABASE_URL/CELERY_BROKER_URL",
            allow_module_level=True,
        )


@pytest.fixture
def api_client():
    from _harness import world

    w = world.build("oc")
    return w, api.Api(w.api_key)


# -- web (app-tier) client ------------------------------------------------
#
# The web issue PATCH lives at /api/workspaces/<slug>/projects/<pid>/issues/
# and authenticates with the ``session-id`` cookie, not ``X-Api-Key``. Forge
# a DB session exactly like the app_issues suite (stdlib HMAC keyed by the
# pinned test SECRET_KEY); the seeded user's password field is overwritten to
# a known pbkdf2 hash so the forged ``_auth_user_hash`` verifies.

_WEB_PW_SALT = "cts1216seed"
_WEB_DK = hashlib.pbkdf2_hmac("sha256", b"cts1216-password", _WEB_PW_SALT.encode(), 10000)
WEB_PASSWORD_FIELD = "pbkdf2_sha256$10000$%s$%s" % (
    _WEB_PW_SALT,
    base64.b64encode(_WEB_DK).decode(),
)


@pytest.fixture
def web_client():
    from _harness import world
    from _harness.auth import authed_client, forge_session_key, get_secret_key

    w = world.build("ocw")
    with db.connect() as conn:
        with conn.cursor() as cur:
            cur.execute(
                "UPDATE users SET password = %s WHERE id = %s",
                (WEB_PASSWORD_FIELD, w.user_id),
            )
        session_key = forge_session_key(
            conn,
            user_id=w.user_id,
            password_field=WEB_PASSWORD_FIELD,
            secret=get_secret_key(),
        )
    client = authed_client(env.BASE_URL, session_key)
    try:
        yield w, client
    finally:
        client.close()
