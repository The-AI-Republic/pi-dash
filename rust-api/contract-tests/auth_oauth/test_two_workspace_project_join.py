# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Two-workspace project-invite join keeps every row (PIDASHCONV-1359, -1686).

Django ``process_workspace_project_invitations``
(``authentication/utils/workspace_project_join.py:59-87``) bulk-inserts
``workspace_members`` for EVERY accepted project invite first, and only
then runs the ``project_members`` bulk — which sets ``project_id`` on
every row (Django main, NEWFRONT-151; PB-1008 retired), so the join
succeeds: both workspace rows, both project rows, both invites
consumed. There is no ``ATOMIC_REQUESTS``.

PIDASHCONV-1359: the Rust github and gitea copies ran both inserts per
invite in one loop, so ``?`` aborted after the first invite's
workspace; the fix splits both copies into workspace-first /
project-second phases, which this module's order pins keep. Google,
gitlab and email already insert all workspace rows first.

Harness choice (github is sealed end to end, like PIDASHCONV-1083/1084:
Django posts the exchange to hardcoded ``https://github.com`` /
``https://api.github.com`` URLs and Rust posts to the same hardcoded
consts over reqwest without system-proxy support, so no provider stub
can drive the github callback on either backend). Gitea accepts a host
override, so it gets the full HTTP callback test; github gets a
source-order pin here plus the kernel-level live test
(``github_kernel_two_project_invites_join_both_workspaces`` in
``oauth_github.rs``, driving ``complete_login_or_signup`` directly with
a fabricated ``GithubAuth``):

1. ``test_app_gitea_two_project_invites_join_both_workspaces`` drives
   the full app-gitea callback over plain HTTP on both backends (the
   1089 stub pattern): 302, both workspace-member rows (role clamped to
   5/15), both project-member rows carrying their invite's
   ``project_id``, both invites soft-deleted. FAILS on pre-fix Rust
   (500, partial writes), passes on Django and fixed Rust.
2. ``test_email_signin_two_project_invites_join_both_workspaces``
   drives the same kernel through email sign-in on both backends: 302
   plus both row sets. It is the kernel oracle pinning the Django
   premise — FAILS on pre-fix Rust, passes on Django and fixed Rust.
3. ``test_github_project_arm_inserts_all_workspaces_first`` and
   ``test_gitea_project_arm_inserts_all_workspaces_first`` pin the
   fixed statement order in the shipped sources: the project-invite
   section must run two ``&project_invites`` loops with the
   ``workspace_members`` INSERT before the second loop and the
   ``project_members`` INSERT inside it. FAIL on the pre-fix single
   loop, in both backend runs.
"""

import os
import re
import sys
import uuid
from pathlib import Path
from urllib.parse import urlsplit

sys.path.insert(0, os.path.join(os.path.dirname(__file__), ".."))

import httpx  # noqa: E402
import pytest  # noqa: E402

from _harness import config, db  # noqa: E402
from _harness.client import SESSION_COOKIE  # noqa: E402
from _harness.db import Database  # noqa: E402
from _harness import factory  # noqa: E402
from _harness.http import forge_session_data  # noqa: E402
from _harness.sinks import RoutingStub  # noqa: E402

REPO_ROOT = Path(__file__).resolve().parents[3]
GITHUB_RS = REPO_ROOT / "rust-api/crates/api/src/auth_oauth/oauth_github.rs"
GITEA_RS = REPO_ROOT / "rust-api/crates/api/src/auth_oauth/oauth_gitea.rs"

GITEA_KEYS = ("GITEA_CLIENT_ID", "GITEA_CLIENT_SECRET", "GITEA_HOST")
SESSION_HOST = "http://oauth-state.test"


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


def _forge_oauth_session(database_url, state):
    """Forge a ``sessions`` row carrying OAuth ``state`` + ``host`` (the
    fixed ``CONTRACT_SECRET_KEY`` signs it exactly like Django)."""
    secret = os.environ[config.CONTRACT_SECRET_KEY]
    payload = {"host": SESSION_HOST, "state": state}
    session_key = uuid.uuid4().hex
    db.execute(
        database_url,
        "INSERT INTO sessions (session_key, session_data, expire_date)"
        " VALUES (%s, %s, now() + make_interval(days => 7))",
        (session_key, forge_session_data(payload, secret)),
    )
    return session_key


def _delete_gitea_config(database_url):
    db.execute(
        database_url,
        "DELETE FROM instance_configurations WHERE key = ANY(%s)",
        (list(GITEA_KEYS),),
    )


def _delete_login_world(database_url, email, owner_email, workspace_ids, project_ids, invite_ids):
    """Best-effort teardown of a callback-created world; tolerates partial
    seeds (the pre-fix 500 branch never reaches the account upsert)."""
    for iid in invite_ids:
        db.execute(
            database_url, "DELETE FROM project_member_invites WHERE id = %s", (iid,)
        )
    for table in (
        "accounts",
        "profiles",
        "workspace_members",
        "project_members",
        "user_notification_preferences",
    ):
        # Django's post_save receivers auto-create profile/notification
        # rows for the callback-created user (no-op on Rust).
        column = (
            "member_id" if table in ("workspace_members", "project_members") else "user_id"
        )
        db.execute(
            database_url,
            f"DELETE FROM {table} WHERE {column} IN"
            " (SELECT id FROM users WHERE email = %s)",
            (email,),
        )
    for pid in project_ids:
        db.execute(database_url, "DELETE FROM projects WHERE id = %s", (pid,))
    for wid in workspace_ids:
        db.execute(database_url, "DELETE FROM workspaces WHERE id = %s", (wid,))
    db.execute(database_url, "DELETE FROM users WHERE email = %s", (email,))
    db.execute(database_url, "DELETE FROM users WHERE email = %s", (owner_email,))


def test_app_gitea_two_project_invites_join_both_workspaces(base_url: str):
    """Full app-gitea callback: 302 joins BOTH workspaces AND projects.

    Runs against both backends. Both backends write both
    workspace-member rows (role clamped to 5/15) and both
    project-member rows carrying their invite's ``project_id``, and
    soft-delete both invites. FAILS on pre-fix Rust (500, partial
    writes).
    """
    database_url = config.database_url()
    store = Database(database_url)
    tag = uuid.uuid4().hex[:8]
    email = f"ge1359-{tag}@example.test"
    owner_email = f"ge1359-own-{tag}@example.test"
    owner = store.make_user(owner_email)
    workspace_ids, project_ids, invite_ids = [], [], []
    try:
        for i, role in ((1, 5), (2, 20)):
            ws = store.make_workspace(
                f"ge1359-{tag}-w{i}", f"ge1359-{tag}-w{i}", owner["id"]
            )
            proj = store.make_project(ws["id"], f"g9{tag}{i}"[-8:])
            iid = _seed_project_invite(database_url, ws["id"], proj["id"], email, role=role)
            workspace_ids.append(ws["id"])
            project_ids.append(proj["id"])
            invite_ids.append(iid)
    except Exception:
        _delete_login_world(database_url, email, owner_email, workspace_ids, project_ids, invite_ids)
        raise
    routes = {
        ("POST", "/login/oauth/access_token"): (
            200,
            {"access_token": f"stub-at-{tag}", "token_type": "bearer"},
        ),
        ("GET", "/api/v1/user"): (
            200,
            {
                "id": 424242,
                "email": email,
                "login": f"ge1359-{tag}",
                "full_name": "Contract User",
                "avatar_url": None,
            },
        ),
        ("GET", "/api/v1/user/emails"): (
            200,
            [{"email": email, "primary": True, "verified": True}],
        ),
    }
    with RoutingStub(routes) as stub:
        # Reap before sow: a crashed earlier run must not leak config rows
        # into this one (or, via teardown, into sibling modules).
        _delete_gitea_config(database_url)
        try:
            store.make_config("GITEA_CLIENT_ID", "contract-id", category="AUTHENTICATION")
            store.make_config(
                "GITEA_CLIENT_SECRET", "contract-secret", category="AUTHENTICATION"
            )
            store.make_config("GITEA_HOST", stub.url, category="AUTHENTICATION")
        except Exception:
            _delete_gitea_config(database_url)
            _delete_login_world(
                database_url, email, owner_email, workspace_ids, project_ids, invite_ids
            )
            raise
        try:
            state = f"ge1359-state-{tag}"
            session_key = _forge_oauth_session(database_url, state)
            try:
                authed = httpx.Client(
                    base_url=base_url,
                    cookies={SESSION_COOKIE: session_key},
                    timeout=30,
                    follow_redirects=False,
                )
                try:
                    resp = authed.get(
                        "/auth/gitea/callback/",
                        params={"code": "contract-stub-code", "state": state},
                    )
                finally:
                    authed.close()
            finally:
                db.execute(
                    database_url,
                    "DELETE FROM sessions WHERE session_key = %s",
                    (session_key,),
                )
            # Read the post-state before teardown deletes the world.
            user = db.fetchone(
                database_url, "SELECT id FROM users WHERE email = %s", (email,)
            )
            members = (
                db.fetchall(
                    database_url,
                    "SELECT workspace_id::text AS ws, role FROM workspace_members"
                    " WHERE member_id = %s ORDER BY ws",
                    (user["id"],),
                )
                if user is not None
                else []
            )
            project_rows = (
                db.fetchall(
                    database_url,
                    "SELECT project_id::text AS pj, workspace_id::text AS ws, role"
                    " FROM project_members WHERE member_id = %s",
                    (user["id"],),
                )
                if user is not None
                else []
            )
            invites = db.fetchall(
                database_url,
                "SELECT workspace_id::text AS ws, accepted, deleted_at"
                " FROM project_member_invites WHERE email = %s ORDER BY ws",
                (email,),
            )
            hits = {(r["method"], urlsplit(r["path"]).path) for r in stub.requests}
        finally:
            _delete_gitea_config(database_url)
            _delete_login_world(
                database_url, email, owner_email, workspace_ids, project_ids, invite_ids
            )

    assert resp.status_code == 302, (
        f"expected 302, got {resp.status_code}: {resp.text[:300]}"
    )
    assert ("POST", "/login/oauth/access_token") in hits, stub.requests
    assert ("GET", "/api/v1/user") in hits, stub.requests
    assert user is not None, "callback created the user"
    assert sorted((m["ws"], m["role"]) for m in members) == sorted(
        [(workspace_ids[0], 5), (workspace_ids[1], 15)]
    ), f"both invite workspaces joined (role 20 clamps to 15): {members}"
    assert sorted((m["pj"], m["ws"], m["role"]) for m in project_rows) == sorted(
        [(project_ids[0], workspace_ids[0], 5), (project_ids[1], workspace_ids[1], 15)]
    ), f"both invite projects joined with their project_ids: {project_rows}"
    assert sorted(i["ws"] for i in invites) == sorted(workspace_ids), invites
    assert all(i["accepted"] is True for i in invites), invites
    assert all(i["deleted_at"] is not None for i in invites), (
        f"both invites soft-deleted: {invites}"
    )


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


def test_email_signin_two_project_invites_join_both_workspaces(
    client: httpx.Client, ensure_instance
):
    """Kernel oracle: email sign-in joins BOTH workspaces AND projects.

    Drives the shared ``process_workspace_project_invitations`` kernel
    live on both backends (the same callback every OAuth provider wires
    in): the project-arm ``workspace_members`` rows for both invites
    commit, both ``project_members`` rows carry their invite's
    ``project_id``, the invites are consumed, and ``login()`` stamps
    ``last_login``. FAILS on pre-fix Rust (500, partial writes).
    """
    database_url = config.database_url()
    database = Database(database_url)
    tag = f"ge1359{uuid.uuid4().hex[:8]}"
    email = f"{tag}@example.com"
    user = factory.create_user(email)
    workspace_ids, project_ids, invite_ids = [], [], []
    for i, role in ((1, 5), (2, 20)):
        ws = database.make_workspace(f"{tag}-w{i}", f"{tag}-w{i}", user["id"])
        proj = database.make_project(ws["id"], f"e9{tag[-6:]}{i}")
        iid = _seed_project_invite(database_url, ws["id"], proj["id"], email, role=role)
        workspace_ids.append(ws["id"])
        project_ids.append(proj["id"])
        invite_ids.append(iid)
    try:
        res = _signin_post(client, email, user["password"])
        assert res.status_code == 302, (
            f"expected 302, got {res.status_code}: {res.text[:300]}"
        )
        assert any(
            raw.startswith(SESSION_COOKIE + "=")
            for raw in res.headers.get_list("set-cookie")
        ), f"no session cookie: {res.headers.get_list('set-cookie')!r}"

        members = db.fetchall(
            database_url,
            "SELECT workspace_id::text AS ws, role FROM workspace_members"
            " WHERE member_id = %s",
            (user["id"],),
        )
        assert sorted((m["ws"], m["role"]) for m in members) == sorted(
            [(workspace_ids[0], 5), (workspace_ids[1], 15)]
        ), f"both invite workspaces joined (role 20 clamps to 15): {members}"
        project_rows = db.fetchall(
            database_url,
            "SELECT project_id::text AS pj, workspace_id::text AS ws, role"
            " FROM project_members WHERE member_id = %s",
            (user["id"],),
        )
        assert sorted((m["pj"], m["ws"], m["role"]) for m in project_rows) == sorted(
            [
                (project_ids[0], workspace_ids[0], 5),
                (project_ids[1], workspace_ids[1], 15),
            ]
        ), f"both invite projects joined with their project_ids: {project_rows}"
        # No live accepted invite remains on either backend. (Django
        # soft-deletes the rows while Rust-email hard-deletes them --
        # the hardness divergence test_signin_invite_join.py already
        # records as outside the join scope.)
        live = db.fetchone(
            database_url,
            "SELECT COUNT(*) AS n FROM project_member_invites"
            " WHERE id = ANY(%s) AND accepted AND deleted_at IS NULL",
            (invite_ids,),
        )
        assert live["n"] == 0, live
        stamps = db.fetchone(
            database_url,
            "SELECT last_login FROM users WHERE id = %s",
            (user["id"],),
        )
        assert stamps["last_login"] is not None, stamps
    finally:
        for iid in invite_ids:
            db.execute(
                database_url, "DELETE FROM project_member_invites WHERE id = %s", (iid,)
            )
        db.execute(
            database_url,
            "DELETE FROM project_members WHERE member_id = %s",
            (user["id"],),
        )
        for pid in project_ids:
            db.execute(database_url, "DELETE FROM projects WHERE id = %s", (pid,))
        factory.cleanup_run(tag)


def _project_arm_block(path: Path, fetch_marker: str, end_marker: str) -> str:
    assert path.is_file(), f"harness: {path} moved — update PIDASHCONV-1359 test paths"
    text = path.read_text()
    start = text.find(fetch_marker)
    assert start != -1, f"harness: {fetch_marker!r} not found in {path.name}"
    end = text.find(end_marker, start)
    assert end != -1, f"harness: {end_marker!r} not found after the fetch in {path.name}"
    return text[start:end]


def _assert_workspace_phases_first(path: Path, fetch_marker: str, end_marker: str) -> None:
    """The project-invite section must run two ``&project_invites`` loops:
    the ``workspace_members`` INSERT before the second loop starts, the
    ``project_members`` INSERT after. A single loop (pre-fix) aborts on
    ``?`` after the first invite's workspace."""
    block = _project_arm_block(path, fetch_marker, end_marker)
    loops = [m.start() for m in re.finditer(r"for\s+[^\n{]*\bin\s+&project_invites", block)]
    assert len(loops) == 2, (
        f"PIDASHCONV-1359: {path.name} runs {len(loops)} project-invite loop(s),"
        " want 2 (all workspace_members rows first, then the project_members"
        " rows) — Django commits every invite's workspace row before the"
        " project_members bulk"
    )
    # Match the write statements, not bare table names (comments name the
    # tables too). ORM-07 moved the workspace write into
    # `domain::workspaces::members` and the project write into
    # `domain::projects::members`, so the pin matches the domain calls.
    ws_m = re.search(r"members::(add|bulk_add)\b", block)
    proj_m = re.search(r"projects::members::(add|bulk_add)\b", block)
    assert ws_m is not None and proj_m is not None, (
        "harness: workspace/project write statements not found in the block"
    )
    ws_at, proj_at = ws_m.start(), proj_m.start()
    assert ws_at < loops[1], (
        f"PIDASHCONV-1359: {path.name} writes workspace_members inside/after the"
        " second loop — all workspace rows must commit before any project row"
    )
    assert proj_at > loops[1], (
        f"PIDASHCONV-1359: {path.name} writes project_members before the second"
        " loop — the project bulk must run after every workspace row"
    )


def test_github_project_arm_inserts_all_workspaces_first():
    """Regression pin: the github copy inserts every workspace row before
    the first project row (Django's bulk order). FAILS on the pre-fix
    single loop; the live behavioral net is
    ``github_kernel_two_project_invites_join_both_workspaces``."""
    _assert_workspace_phases_first(
        GITHUB_RS,
        "let project_invites: Vec<(Uuid, Uuid, i16, Option<Uuid>)>",
        "// Both invite sets soft-delete",
    )


def test_gitea_project_arm_inserts_all_workspaces_first():
    """Regression pin: the gitea copy inserts every workspace row before
    the first project row (Django's bulk order). FAILS on the pre-fix
    single loop; the live behavioral net is the HTTP test above plus
    ``gitea_kernel_two_project_invites_join_both_workspaces``."""
    _assert_workspace_phases_first(
        GITEA_RS,
        "let project_invites: Vec<ProjectInviteRow>",
        "// Both invite sets soft-delete",
    )
