# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""GitHub project-invite join must 500 like Django (PIDASHCONV-1084, PB-1008).

Django ``process_workspace_project_invitations``
(``authentication/utils/workspace_project_join.py:76-87``) builds
``ProjectMember`` rows without ``project`` while
``project_members.project_id`` is NOT NULL, so every app-callback success
with accepted project invites raises ``IntegrityError`` into a 500 *after*
the workspace-member rows commit: no invite delete, no login, no account
upsert. Google, gitlab and gitea port the 500; the github port selected
``project_id`` from the invite and inserted it, so github wrongly
succeeded. The fix drops ``project_id`` from the github
``project_members`` INSERT.

Why this file is a kernel-level harness instead of an end-to-end github
login: the github token/userinfo URLs are hardcoded ``https://`` on both
backends, the Rust ``reqwest`` client is built without system-proxy
support (``crates/api/Cargo.toml``: ``default-features = false``), and no
test seam exists on either side — so no provider stub can feed both
backends without changing shipped behavior, which translate-don't-redesign
forbids. The stage-1 suite already records the success branch as
hermetically untestable (see "Known oracle limits" in
``test_permissions.py``). The issue delegates the harness choice to the fix
run; this file pins the bug from four sides instead:

1. ``test_project_invite_signin_500s_with_partial_writes`` drives the
   *shared* invite-join kernel live on both backends through the
   provider-free email sign-in path (``EmailProvider`` passes the same
   ``callback=post_user_auth_workflow``): 500, no session cookie, the
   workspace-member row committed (role clamped to 5/15), zero
   project-member rows, the invite intact, ``last_login`` still NULL.
   Green before and after the fix — it is the kernel oracle.
2. ``test_github_project_members_insert_omits_project_id`` is the
   regression test: it reads the shipped ``oauth_github.rs``, extracts the
   ``project_members`` INSERT block, and asserts the statement cannot
   insert a real ``project_id`` (omitted column like google/gitlab, or an
   explicit NULL like gitea, with no ``project_id`` bind). It FAILS on the
   pre-fix code and passes after, in both backend runs.
3. ``test_django_join_builds_project_members_without_project`` pins the
   Django side of the contract: if ``workspace_project_join.py`` ever sets
   ``project=``, PB-1008's premise changed and every provider port must be
   revisited.
4. ``test_project_members_project_id_is_not_null`` proves the schema
   premise live against the backend's own database: a complete row with a
   real ``project_id`` inserts fine, the same row with NULL raises a NOT
   NULL violation naming ``project_id``.

Together: the kernel 500s live (1), the github copy carries the same
omission (2), Django still omits (3), and the column still rejects NULL
(4) — so the github callback must 500 exactly where Django does.
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


def test_project_invite_signin_500s_with_partial_writes(
    client: httpx.Client, ensure_instance
):
    """Accepted project invite: 500 on both, workspace row kept, invite intact.

    Drives the shared ``process_workspace_project_invitations`` kernel live
    through email sign-in (same callback Django wires into every OAuth
    provider): the project-arm ``workspace_members`` row commits, the
    ``project_members`` bulk raises the ported IntegrityError, and the
    invite deletes plus ``login()`` never run.
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
        assert res.status_code == 500, f"expected 500, got {res.status_code}: {res.text[:300]}"
        for raw in res.headers.get_list("set-cookie"):
            assert not raw.startswith(SESSION_COOKIE + "="), f"session cookie set: {raw!r}"

        members = db.fetchall(
            database_url,
            "SELECT workspace_id::text AS ws, role FROM workspace_members WHERE member_id = %s",
            (user["id"],),
        )
        assert [(m["ws"], m["role"]) for m in members] == [(workspace["id"], 15)], members
        project_rows = db.fetchone(
            database_url,
            "SELECT COUNT(*) AS n FROM project_members WHERE member_id = %s",
            (user["id"],),
        )
        assert project_rows["n"] == 0, project_rows
        invite = db.fetchone(
            database_url,
            "SELECT accepted, deleted_at FROM project_member_invites WHERE id = %s",
            (invite_id,),
        )
        assert invite is not None and invite["accepted"] is True, invite
        assert invite["deleted_at"] is None, invite
        stamps = db.fetchone(
            database_url,
            "SELECT last_login FROM users WHERE id = %s",
            (user["id"],),
        )
        assert stamps["last_login"] is None, stamps
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


def test_github_project_members_insert_omits_project_id():
    """Regression test: the github INSERT must not carry a real project_id.

    The call site must pass ``project_id: None`` (the explicit-NULL
    shape the issue sanctions for the per-row replays) and no
    ``.bind(project_id)`` may feed the write. FAILS on pre-fix code.
    """
    block = _github_insert_block()
    bind_has = re.search(r"\.bind\(\s*&?\s*\*?project_id\s*\)", block) is not None
    assert not bind_has, (
        "PB-1008: oauth_github.rs binds the invite's project_id into"
        " project_members — Django omits it and 500s; drop the bind"
    )
    assert "project_id: None" in block, (
        "PB-1008: oauth_github.rs must pass project_id: None — a real"
        " project_id would stop the ported 500"
    )


def test_django_join_builds_project_members_without_project():
    """Django pin: ProjectMember(...) sets no project (PB-1008 premise)."""
    assert JOIN_PY.is_file(), f"harness: {JOIN_PY} moved — update test paths"
    text = JOIN_PY.read_text()
    start = text.find("ProjectMember(\n")
    assert start != -1, "harness: ProjectMember( constructor not found"
    end = text.find("for project_member_invite in", start)
    assert end != -1, "harness: invite loop not found after ProjectMember("
    block = text[start:end]
    assert "workspace_id=" in block, "harness: extracted the wrong block"
    assert re.search(r"\bproject\s*=", block) is None, (
        "PB-1008 premise changed: workspace_project_join.py now sets"
        " project= on ProjectMember — revisit every provider port"
    )
