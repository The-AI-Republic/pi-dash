# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""GitHub project-invite join succeeds like Django (PIDASHCONV-1686, PB-1008 retired).

Django ``process_workspace_project_invitations``
(``authentication/utils/workspace_project_join.py:76-87``) sets
``project_id`` on the ``ProjectMember`` bulk (Django main, NEWFRONT-151),
so sign-up/sign-in carrying accepted project invites succeeds: the user
joins every invite workspace (role clamped to 5/15) *and* every invite
project, the invites are consumed, and login completes. PB-1008 (the
port that omitted ``project_id`` and 500d) is retired; every provider
port plus email and magic bind the invite's ``project_id``.

Why this file is a kernel-level harness instead of an end-to-end github
login: the github token/userinfo URLs are hardcoded ``https://`` on both
backends, the Rust ``reqwest`` client is built without system-proxy
support (``crates/api/Cargo.toml``: ``default-features = false``), and no
test seam exists on either side — so no provider stub can feed both
backends without changing shipped behavior, which translate-don't-redesign
forbids. The stage-1 suite already records the success branch as
hermetically untestable (see "Known oracle limits" in
``test_permissions.py``). This file pins the join from four sides instead:

1. ``test_project_invite_signin_succeeds_with_member_rows`` drives the
   *shared* invite-join kernel live on both backends through the
   provider-free email sign-in path (``EmailProvider`` passes the same
   ``callback=post_user_auth_workflow``): 302, session cookie set, the
   workspace-member row (role clamped to 5/15), the project-member row
   carrying the invite's ``project_id``, the invite consumed, and
   ``last_login`` stamped. It FAILS on the pre-fix code (500, partial
   writes) and passes after, in both backend runs.
2. ``test_github_project_members_insert_binds_project_id`` is the
   regression test: it reads the shipped ``oauth_github.rs``, extracts the
   ``project_members`` call block, and asserts the site binds the invite's
   ``project_id`` (``Some(..)``, never ``None``). It FAILS on the pre-fix
   code and passes after, in both backend runs.
3. ``test_django_join_sets_project_id_on_project_members`` pins the
   Django side of the contract: ``workspace_project_join.py`` must set
   ``project_id=`` on ``ProjectMember``. It FAILS on a checkout whose
   Django predates the main fix (a maintainer sync merges main into
   rust-dev first).
4. ``test_project_members_project_id_is_not_null`` proves the schema
   premise live against the backend's own database: a complete row with a
   real ``project_id`` inserts fine, the same row with NULL raises a NOT
   NULL violation naming ``project_id``.

Together: the kernel joins live (1), the github copy binds the id (2),
Django sets it (3), and the column still rejects NULL (4) — so the
github callback succeeds exactly where Django does.
"""

import os
import re
import sys
import uuid
from pathlib import Path

sys.path.insert(0, os.path.join(os.path.dirname(__file__), ".."))

import httpx  # noqa: E402
import pytest  # noqa: E402

from _harness import config, db, factory  # noqa: E402
from _harness.client import SESSION_COOKIE  # noqa: E402
from _harness.db import Database  # noqa: E402

REPO_ROOT = Path(__file__).resolve().parents[3]
GITHUB_RS = REPO_ROOT / "rust-api/crates/api/src/auth_oauth/oauth_github.rs"
JOIN_PY = REPO_ROOT / "apps/api/pi_dash/authentication/utils/workspace_project_join.py"


def _seed_project_invite(database_url, workspace_id, project_id, email, *, role):
    iid = str(uuid.uuid4())
    db.execute(
        database_url,
        "INSERT INTO project_member_invites (id, created_at, updated_at,"
        " project_id, workspace_id, email, accepted, token, role)"
        " VALUES (%s, now(), now(), %s, %s, %s, true, %s, %s)",
        (iid, project_id, workspace_id, email, f"tok-{iid[:8]}", role),
    )
    return iid


def _signin_post(client: httpx.Client, email: str, password: str) -> httpx.Response:
    """CSRF-primed POST to /auth/sign-in/; returns the raw response."""
    primed = client.get("/auth/get-csrf-token/")
    assert primed.status_code == 200, f"CSRF priming failed: {primed.status_code}"
    token = primed.json()["csrf_token"]
    csrf_cookie = primed.cookies.get("csrftoken")
    assert csrf_cookie, "no csrftoken cookie from /auth/get-csrf-token/"
    import urllib.parse

    body = urllib.parse.urlencode(
        {"email": email, "password": password, "csrfmiddlewaretoken": token}
    )
    return client.post(
        "/auth/sign-in/",
        content=body,
        headers={
            "Cookie": f"csrftoken={csrf_cookie}",
            "Referer": f"{config.base_url()}/",
            "Content-Type": "application/x-www-form-urlencoded",
        },
    )


def test_project_invite_signin_succeeds_with_member_rows(
    client: httpx.Client, ensure_instance
):
    """Accepted project invite: 302 on both, both member rows, invite consumed.

    Drives the shared ``process_workspace_project_invitations`` kernel live
    through email sign-in (same callback Django wires into every OAuth
    provider): the project-arm ``workspace_members`` row commits, the
    ``project_members`` row carries the invite's ``project_id`` (role
    clamped to 5/15 on both), the invite is consumed, and ``login()``
    stamps ``last_login``.
    """
    database_url = config.database_url()
    database = Database(database_url)
    tag = f"gh1084{uuid.uuid4().hex[:8]}"
    email = f"{tag}@example.com"
    user = factory.create_user(email)
    workspace = database.make_workspace(f"{tag}-ws", f"{tag}-ws", user["id"])
    project = database.make_project(workspace["id"], f"gh{tag[-8:]}")
    # role 20 is outside [5, 15], so the kernel clamps it to 15.
    invite_id = _seed_project_invite(
        database_url, workspace["id"], project["id"], email, role=20
    )
    try:
        res = _signin_post(client, email, user["password"])
        assert res.status_code == 302, f"expected 302, got {res.status_code}: {res.text[:300]}"
        assert any(
            raw.startswith(SESSION_COOKIE + "=")
            for raw in res.headers.get_list("set-cookie")
        ), f"no session cookie: {res.headers.get_list('set-cookie')!r}"

        members = db.fetchall(
            database_url,
            "SELECT workspace_id::text AS ws, role FROM workspace_members WHERE member_id = %s",
            (user["id"],),
        )
        assert [(m["ws"], m["role"]) for m in members] == [(workspace["id"], 15)], members
        project_rows = db.fetchall(
            database_url,
            "SELECT project_id::text AS pj, workspace_id::text AS ws, role"
            " FROM project_members WHERE member_id = %s",
            (user["id"],),
        )
        assert [(m["pj"], m["ws"], m["role"]) for m in project_rows] == [
            (project["id"], workspace["id"], 15)
        ], project_rows
        # No live accepted invite remains on either backend. (Django
        # soft-deletes the row while Rust-email hard-deletes it -- the
        # hardness divergence test_signin_invite_join.py already records
        # as outside the join scope.)
        live = db.fetchone(
            database_url,
            "SELECT COUNT(*) AS n FROM project_member_invites"
            " WHERE id = %s AND accepted AND deleted_at IS NULL",
            (invite_id,),
        )
        assert live["n"] == 0, live
        stamps = db.fetchone(
            database_url,
            "SELECT last_login FROM users WHERE id = %s",
            (user["id"],),
        )
        assert stamps["last_login"] is not None, stamps
    finally:
        db.execute(
            database_url,
            "DELETE FROM project_member_invites WHERE id = %s",
            (invite_id,),
        )
        db.execute(
            database_url,
            "DELETE FROM project_members WHERE member_id = %s",
            (user["id"],),
        )
        db.execute(database_url, "DELETE FROM projects WHERE id = %s", (project["id"],))
        factory.cleanup_run(tag)


def test_project_members_project_id_is_not_null():
    """Schema premise of PB-1008, live: NULL project_id violates NOT NULL."""
    database_url = config.database_url()
    database = Database(database_url)
    tag = f"gh1084{uuid.uuid4().hex[:8]}"
    email = f"{tag}@example.com"
    user = factory.create_user(email)
    workspace = database.make_workspace(f"{tag}-ws", f"{tag}-ws", user["id"])
    project = database.make_project(workspace["id"], f"gh{tag[-8:]}")
    row_id = str(uuid.uuid4())
    columns = (
        "id, created_at, updated_at, project_id, workspace_id, member_id,"
        " comment, role, view_props, default_props, preferences,"
        " sort_order, is_active, created_by_id, updated_by_id, deleted_at"
    )
    params = (
        row_id,
        workspace["id"],
        user["id"],
        15,
        "{}",
        "{}",
        "{}",
    )
    try:
        # A complete row with a real project_id inserts fine: the harness
        # values are otherwise valid, isolating project_id below.
        db.execute(
            database_url,
            f"INSERT INTO project_members ({columns})"
            " VALUES (%s, now(), now(), %s, %s, %s,"
            " NULL, %s, %s, %s, %s, 65535, TRUE, NULL, NULL, NULL)"
            " ON CONFLICT DO NOTHING",
            (params[0], project["id"], *params[1:]),
        )
        db.execute(database_url, "DELETE FROM project_members WHERE id = %s", (row_id,))
        # The same row with NULL project_id must raise the NOT NULL
        # violation — the statement Django's bulk and the Rust ports issue.
        with pytest.raises(Exception) as excinfo:
            db.execute(
                database_url,
                f"INSERT INTO project_members ({columns})"
                " VALUES (%s, now(), now(), NULL, %s, %s,"
                " NULL, %s, %s, %s, %s, 65535, TRUE, NULL, NULL, NULL)"
                " ON CONFLICT DO NOTHING",
                params,
            )
        assert "project_id" in str(excinfo.value), str(excinfo.value)[:300]
    finally:
        db.execute(database_url, "DELETE FROM project_members WHERE id = %s", (row_id,))
        db.execute(database_url, "DELETE FROM projects WHERE id = %s", (project["id"],))
        factory.cleanup_run(tag)


def _github_insert_block() -> str:
    # ORM-07 moved the INSERT into `domain::projects::members::add`;
    # the pin now covers the call site's `NewProjectMember` literal.
    assert GITHUB_RS.is_file(), (
        f"harness: {GITHUB_RS} moved — update PIDASHCONV-1084 test paths"
    )
    text = GITHUB_RS.read_text()
    marker = "domain::projects::members::add("
    at = text.find(marker)
    assert at != -1, (
        "harness: no domain::projects::members::add call in oauth_github.rs"
        " — update the PIDASHCONV-1084 harness"
    )
    end = text.find(".await?;", at)
    assert end != -1, "harness: could not delimit the domain call block"
    return text[at : end + 2]


def test_github_project_members_insert_binds_project_id():
    """Regression test: the github INSERT must carry the invite's project_id.

    The call site must pass ``project_id: Some(..)`` (never ``None`` —
    the retired PB-1008 shape 500d). FAILS on pre-fix code.
    """
    block = _github_insert_block()
    assert "project_id: None" not in block, (
        "PIDASHCONV-1686: oauth_github.rs still passes project_id: None —"
        " bind the invite's project_id (PB-1008 is retired)"
    )
    assert re.search(r"project_id:\s*Some\(", block) is not None, (
        "PIDASHCONV-1686: oauth_github.rs must pass project_id: Some(..)"
        " into project_members"
    )


def test_django_join_sets_project_id_on_project_members():
    """Django pin: ProjectMember(...) sets project_id (PB-1008 retired)."""
    assert JOIN_PY.is_file(), f"harness: {JOIN_PY} moved — update test paths"
    text = JOIN_PY.read_text()
    start = text.find("ProjectMember(\n")
    assert start != -1, "harness: ProjectMember( constructor not found"
    end = text.find("for project_member_invite in", start)
    assert end != -1, "harness: invite loop not found after ProjectMember("
    block = text[start:end]
    assert "workspace_id=" in block, "harness: extracted the wrong block"
    assert re.search(r"\bproject_id\s*=", block) is not None, (
        "PIDASHCONV-1686: workspace_project_join.py sets no project_id="
        " on ProjectMember — the checkout's Django predates the main fix;"
        " a maintainer sync merges main into rust-dev first"
    )
