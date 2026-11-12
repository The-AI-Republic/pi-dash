"""Fixtures for the auth-oauth contract suite (PIDASHCONV-102).

The suite pins the redirect behavior of the 16 OAuth routes in
``pi_dash/authentication/urls.py`` (google/github/gitlab/gitea, each with
initiate + callback, each in an app and a ``spaces/`` variant) against the
live backend. HTTP goes through httpx with ``follow_redirects=False`` so
the 302 ``Location`` targets stay observable; the one DB write is the
``Instance`` set-up marker below. Nothing here imports Django.

Hermeticity: the backend under test must boot with NO OAuth provider
credentials — no ``*_CLIENT_ID``/``*_SECRET``/``*_HOST`` env values and no
matching ``instance_configurations`` rows (asserted by
``oauth_unconfigured``). Every provider constructor then raises its
``*_NOT_CONFIGURED`` error before any outbound HTTP, and every callback
with an unknown ``state`` returns before touching the provider at all, so
the suite never contacts Google, GitHub, GitLab or Gitea.

``app_base`` / ``space_base`` are read off live initiate responses rather
than hardcoded so the suite tracks whatever ``WEB_URL`` / ``APP_BASE_URL``
/ ``SPACE_BASE_URL`` the backend booted with.
"""

import os
import sys
import uuid
from urllib.parse import urlsplit

sys.path.insert(0, os.path.join(os.path.dirname(__file__), ".."))

import httpx  # noqa: E402
import pytest  # noqa: E402

from _harness import config, db  # noqa: E402
from _harness.client import base_url, client  # noqa: E402,F401  (shared fixtures)

# OAuth config keys that would flip initiate/callback out of their
# deterministic unconfigured branches if present (DB rows or server env).
OAUTH_CONFIG_KEYS = (
    "GOOGLE_CLIENT_ID",
    "GOOGLE_CLIENT_SECRET",
    "GITHUB_CLIENT_ID",
    "GITHUB_CLIENT_SECRET",
    "GITHUB_ORGANIZATION_ID",
    "GITLAB_CLIENT_ID",
    "GITLAB_CLIENT_SECRET",
    "GITLAB_HOST",
    "GITEA_CLIENT_ID",
    "GITEA_CLIENT_SECRET",
    "GITEA_HOST",
)


@pytest.fixture(scope="session")
def ensure_instance():
    """The OAuth initiate views 302 to INSTANCE_NOT_CONFIGURED (5000) unless
    an ``Instance`` with ``is_setup_done`` exists. Insert the marker on a
    fresh database; never touch an existing row."""
    database_url = config.database_url()
    row = db.fetchone(database_url, "SELECT is_setup_done FROM instances LIMIT 1")
    if row is None:
        tag = uuid.uuid4().hex[:8]
        db.execute(
            database_url,
            """INSERT INTO instances
               (id, instance_name, instance_id, current_version, edition,
                domain, last_checked_at, is_telemetry_enabled,
                is_support_required, is_setup_done, is_signup_screen_visited,
                is_verified, is_test, is_current_version_deprecated,
                created_at, updated_at)
               VALUES (%s,%s,%s,%s,%s,%s,now(),false,false,true,false,false,false,false,now(),now())""",
            (
                str(uuid.uuid4()),
                f"contract-{tag}",
                f"contract-{tag}",
                "0.0.0-contract",
                "PI_DASH_COMMUNITY",
                "",
            ),
        )
    else:
        assert row["is_setup_done"], (
            "instances.is_setup_done is false: OAuth initiate would 302 with "
            "INSTANCE_NOT_CONFIGURED (5000) instead of the provider codes. "
            "Finish instance setup on this database first."
        )
    return True


@pytest.fixture(scope="session")
def oauth_unconfigured(ensure_instance):
    """Fail fast if any OAuth provider is configured: provider credentials
    (DB ``instance_configurations`` rows or server env) would move initiate
    off its deterministic ``*_NOT_CONFIGURED`` redirect and callback off
    its deterministic provider-error redirect."""
    database_url = config.database_url()
    rows = db.fetchall(
        database_url,
        "SELECT key FROM instance_configurations WHERE key = ANY(%s)",
        (list(OAUTH_CONFIG_KEYS),),
    )
    assert not rows, (
        "OAuth provider configuration rows present "
        f"{sorted(r['key'] for r in rows)}: remove them so every provider "
        "is deterministically unconfigured for this suite."
    )
    return True


def _origin(location: str) -> str:
    parts = urlsplit(location)
    return f"{parts.scheme}://{parts.netloc}"


@pytest.fixture(scope="session")
def app_base(oauth_unconfigured):
    """scheme://host the app (non-space) OAuth redirects are built on."""
    with httpx.Client(base_url=config.base_url(), timeout=10, follow_redirects=False) as c:
        resp = c.get("/auth/google/")
    assert resp.status_code == 302, f"expected 302, got {resp.status_code}: {resp.text[:200]}"
    return _origin(resp.headers["location"])


@pytest.fixture(scope="session")
def space_base(oauth_unconfigured):
    """scheme://host the spaces/ OAuth redirects are built on."""
    with httpx.Client(base_url=config.base_url(), timeout=10, follow_redirects=False) as c:
        resp = c.get("/auth/spaces/google/")
    assert resp.status_code == 302, f"expected 302, got {resp.status_code}: {resp.text[:200]}"
    return _origin(resp.headers["location"])
