"""Contract suite: OAuth invite soft-delete semantics.

PIDASHCONV-1089 pins consumed-invite soft deletion. PIDASHCONV-1358 adds
the signup gate: with ``ENABLE_SIGNUP=0``, a soft-deleted workspace invite
must not count as an invitation for GitLab or Gitea signup.

Django ends ``process_workspace_project_invitations`` with queryset
``.delete()`` on both invite sets (``workspace_project_join.py:90-91``),
which soft-deletes (``SoftDeletionQuerySet.delete`` stamps ``deleted_at``).
The Rust google and gitea callbacks instead hard-deleted the rows; github
and gitlab already soft-delete. This module pins the soft-delete on both
backends: after an app login with accepted workspace invites, the invite
rows are still present with ``deleted_at`` set.

Harness choice (the issue leaves this to the fix run): gitea and gitlab
accept host overrides, so their success tests point ``GITEA_HOST`` /
``GITLAB_HOST`` at loopback stubs serving the token + userinfo endpoints
and drive the full callback path over plain HTTP on both backends. Google
hardcodes its token/userinfo URLs on both backends (the same
sealed-provider finding as PIDASHCONV-1083 for github), so google has no
HTTP success test here: its net is the kernel-level harness
(``google_kernel_*`` live tests in ``oauth_google.rs`` driving
``process_invitations`` directly) plus the error-branch pins below.

Scope notes: PIDASHCONV-1686 retired the ported PB-1008 bug (Django
main sets ``project_id`` on the ``ProjectMember`` bulk), so accepted
project invites join on the success path instead of 500ing first: the
gitea success world below stays workspace-only (the two-workspace module
covers gitea project joins), while the gitlab success test seeds one
accepted project invite and pins its join plus soft-delete. Provider and
``ENABLE_SIGNUP`` ``instance_configurations`` rows are inserted
function-scoped and deleted in teardown, so the suite's boot-time
``oauth_unconfigured`` hermeticity holds for every other module (tests run
serially; no fixture can observe the rows mid-test). No test here contacts
a real provider.
"""

import os
import sys
import uuid
from urllib.parse import parse_qs, urlsplit

sys.path.insert(0, os.path.join(os.path.dirname(__file__), ".."))

import httpx  # noqa: E402
import pytest  # noqa: E402

from _harness import config, db  # noqa: E402
from _harness.client import SESSION_COOKIE  # noqa: E402
from _harness.db import Database  # noqa: E402
from _harness.http import forge_session_data  # noqa: E402
from _harness.sinks import RoutingStub  # noqa: E402

GITEA_KEYS = ("GITEA_CLIENT_ID", "GITEA_CLIENT_SECRET", "GITEA_HOST")
GITLAB_KEYS = ("GITLAB_CLIENT_ID", "GITLAB_CLIENT_SECRET", "GITLAB_HOST")
ENABLE_SIGNUP_KEY = ("ENABLE_SIGNUP",)
SESSION_HOST = "http://oauth-state.test"


def _forge_oauth_session(database_url, state):
    """Forge a ``sessions`` row carrying an OAuth ``state`` + ``host`` (the
    fixed ``CONTRACT_SECRET_KEY`` signs it exactly like Django). The
    success redirect is ``urljoin(host, path)``, so ``host`` must be set or
    the callback 500s after authenticating. Returns the session key; the
    caller deletes the row."""
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


def _delete_session(database_url, session_key):
    db.execute(
        database_url, "DELETE FROM sessions WHERE session_key = %s", (session_key,)
    )


def _delete_config(database_url, keys):
    db.execute(
        database_url,
        "DELETE FROM instance_configurations WHERE key = ANY(%s)",
        (list(keys),),
    )


def _delete_gitea_config(database_url):
    _delete_config(database_url, GITEA_KEYS)


def _delete_world(store, emails, workspace_ids, project_ids=()):
    """Best-effort world teardown (accounts/profiles/members/invites →
    projects → workspaces → users); tolerates partially seeded worlds."""
    with store.connect() as conn:
        with conn.cursor() as cur:
            cur.execute(
                "DELETE FROM accounts WHERE user_id IN"
                " (SELECT id FROM users WHERE email = ANY(%s))",
                (emails,),
            )
            cur.execute(
                "DELETE FROM profiles WHERE user_id IN"
                " (SELECT id FROM users WHERE email = ANY(%s))",
                (emails,),
            )
            cur.execute(
                "DELETE FROM workspace_members WHERE member_id IN"
                " (SELECT id FROM users WHERE email = ANY(%s))",
                (emails,),
            )
            cur.execute(
                "DELETE FROM project_members WHERE member_id IN"
                " (SELECT id FROM users WHERE email = ANY(%s))",
                (emails,),
            )
            cur.execute(
                "DELETE FROM workspace_member_invites WHERE email = ANY(%s)",
                (emails,),
            )
            cur.execute(
                "DELETE FROM project_member_invites WHERE email = ANY(%s)",
                (emails,),
            )
            # Django's post_save receiver auto-creates these for the
            # callback-created user (no-op on Rust, which runs no signals).
            cur.execute(
                "DELETE FROM user_notification_preferences WHERE user_id IN"
                " (SELECT id FROM users WHERE email = ANY(%s))",
                (emails,),
            )
            for project_id in project_ids:
                cur.execute("DELETE FROM projects WHERE id = %s", (project_id,))
            for workspace_id in workspace_ids:
                cur.execute("DELETE FROM workspaces WHERE id = %s", (workspace_id,))
            cur.execute("DELETE FROM users WHERE email = ANY(%s)", (emails,))
        conn.commit()


@pytest.fixture()
def gitea_stub_world():
    """Workspace + accepted invite for a fresh email, behind a stub Gitea.

    The stub serves the token exchange plus userinfo (email included, so
    neither backend calls ``/emails``) and the emails fallback. Yields the
    world with the stub running and the ``GITEA_*`` config rows in place;
    teardown removes the config rows first (suite hermeticity), then the
    world. Decoys: an unaccepted same-email invite (in a second workspace —
    live invites are unique per (email, workspace)) and an accepted
    other-email invite, both of which must stay live.
    """
    database_url = config.database_url()
    store = Database(database_url)
    tag = uuid.uuid4().hex[:8]
    owner_email = f"gh1089-own-{tag}@example.test"
    email = f"gh1089-{tag}@example.test"
    other = f"gh1089-other-{tag}@example.test"
    emails = [email, other, owner_email]
    world = {"tag": tag, "email": email, "other": other, "emails": emails}
    try:
        owner = store.make_user(owner_email)
        workspace = store.make_workspace(f"gh1089-{tag}", f"gh1089-{tag}", owner["id"])
        decoy_workspace = store.make_workspace(
            f"gh1089-{tag}-u", f"gh1089-{tag}-u", owner["id"]
        )
        store.make_invite(workspace["id"], email, accepted=True, token=f"tok-1089-{tag}")
        store.make_invite(
            decoy_workspace["id"], email, accepted=False, token=f"tok-1089-{tag}-u"
        )
        store.make_invite(
            workspace["id"], other, accepted=True, token=f"tok-1089-{tag}-o"
        )
    except Exception:
        _delete_world(store, emails, [])
        raise
    world["owner"] = owner
    world["workspace"] = workspace
    world["workspace_ids"] = [workspace["id"], decoy_workspace["id"]]
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
                "login": f"gh1089-{tag}",
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
            store.make_config(
                "GITEA_CLIENT_ID", "contract-id", category="AUTHENTICATION"
            )
            store.make_config(
                "GITEA_CLIENT_SECRET", "contract-secret", category="AUTHENTICATION"
            )
            store.make_config("GITEA_HOST", stub.url, category="AUTHENTICATION")
        except Exception:
            _delete_gitea_config(database_url)
            _delete_world(store, emails, world["workspace_ids"])
            raise
        try:
            world["stub"] = stub
            yield world
        finally:
            _delete_gitea_config(database_url)
            _delete_world(store, emails, world["workspace_ids"])


@pytest.fixture(params=["gitlab", "gitea"])
def signup_disabled_world(request):
    """A fresh email with only a soft-deleted workspace invite, plus a
    loopback provider stub and ``ENABLE_SIGNUP=0``.

    Both provider callbacks reach ``__check_signup`` only after the token
    exchange and userinfo fetch, so the stub must answer those endpoints.
    The invite is stamped deleted through raw SQL: it is the revoked or
    already-consumed shape that Django's ``SoftDeletionManager`` excludes.
    """
    provider = request.param
    database_url = config.database_url()
    store = Database(database_url)
    tag = uuid.uuid4().hex[:8]
    owner_email = f"signup-{provider}-own-{tag}@example.test"
    email = f"signup-{provider}-{tag}@example.test"
    emails = [email, owner_email]
    owner = store.make_user(owner_email)
    workspace = store.make_workspace(
        f"signup-{provider}-{tag}", f"signup-{provider}-{tag}", owner["id"]
    )
    invite = store.make_invite(
        workspace["id"], email, token=f"tok-signup-{provider}-{tag}"
    )
    db.execute(
        database_url,
        "UPDATE workspace_member_invites SET deleted_at = now() WHERE id = %s",
        (invite["id"],),
    )

    if provider == "gitlab":
        config_keys = GITLAB_KEYS
        callback_path = "/auth/gitlab/callback/"
        token_path = "/oauth/token"
        userinfo_path = "/api/v4/user"
        routes = {
            ("POST", token_path): (
                200,
                {"access_token": f"stub-at-{tag}", "token_type": "bearer"},
            ),
            ("GET", userinfo_path): (
                200,
                {
                    "id": 424242,
                    "email": email,
                    "name": "Contract User",
                    "username": f"signup-{tag}",
                    "avatar_url": None,
                },
            ),
        }
    else:
        config_keys = GITEA_KEYS
        callback_path = "/auth/gitea/callback/"
        token_path = "/login/oauth/access_token"
        userinfo_path = "/api/v1/user"
        routes = {
            ("POST", token_path): (
                200,
                {"access_token": f"stub-at-{tag}", "token_type": "bearer"},
            ),
            ("GET", userinfo_path): (
                200,
                {
                    "id": 424242,
                    "email": email,
                    "login": f"signup-{tag}",
                    "full_name": "Contract User",
                    "avatar_url": None,
                },
            ),
            ("GET", "/api/v1/user/emails"): (
                200,
                [{"email": email, "primary": True, "verified": True}],
            ),
        }

    world = {
        "provider": provider,
        "email": email,
        "callback_path": callback_path,
        "token_path": token_path,
        "userinfo_path": userinfo_path,
        "workspace": workspace,
        "emails": emails,
        "workspace_ids": [workspace["id"]],
    }
    with RoutingStub(routes) as stub:
        _delete_config(database_url, config_keys)
        _delete_config(database_url, ENABLE_SIGNUP_KEY)
        try:
            store.make_config(
                config_keys[0], "contract-id", category="AUTHENTICATION"
            )
            store.make_config(
                config_keys[1], "contract-secret", category="AUTHENTICATION"
            )
            store.make_config(config_keys[2], stub.url, category="AUTHENTICATION")
            store.make_config("ENABLE_SIGNUP", "0")
            world["stub"] = stub
            yield world
        finally:
            _delete_config(database_url, config_keys)
            _delete_config(database_url, ENABLE_SIGNUP_KEY)
            _delete_world(store, emails, world["workspace_ids"])


def test_signup_disabled_ignores_soft_deleted_invite(
    base_url: str, signup_disabled_world
):
    """A revoked invite does not satisfy the signup gate.

    Django's manager filters ``deleted_at IS NULL``, so the callback must
    answer the provider error redirect (5015, email payload) and leave the
    user table untouched. Before this fix Rust counted the soft-deleted
    row as an invite, created the user, and logged them in.
    """
    database_url = config.database_url()
    world = signup_disabled_world
    email = world["email"]
    state = f"signup-{world['provider']}-state-{uuid.uuid4().hex[:8]}"
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
                world["callback_path"],
                params={"code": "contract-stub-code", "state": state},
            )
        finally:
            authed.close()
    finally:
        _delete_session(database_url, session_key)

    assert resp.status_code == 302, (
        f"expected 302, got {resp.status_code}: {resp.text[:300]}"
    )
    user = db.fetchone(
        database_url, "SELECT id FROM users WHERE email = %s", (email,)
    )
    assert user is None, "soft-deleted invite must not let the callback create a user"
    location = resp.headers["location"]
    query = parse_qs(urlsplit(location).query)
    assert query.get("error_code") == ["5015"], location
    assert query.get("email") == [email], location

    hits = {(r["method"], urlsplit(r["path"]).path) for r in world["stub"].requests}
    assert ("POST", world["token_path"]) in hits, world["stub"].requests
    assert ("GET", world["userinfo_path"]) in hits, world["stub"].requests


def test_app_gitea_login_soft_deletes_consumed_invites(
    base_url: str, gitea_stub_world
):
    """Full app-gitea login: membership lands, consumed invite soft-deletes.

    Runs against both backends. Pre-fix Rust hard-deletes the accepted
    row, so the row-presence assertion fails; post-fix both backends keep
    it with ``deleted_at`` set, and the decoys (unaccepted same-email,
    accepted other-email) stay live.
    """
    database_url = config.database_url()
    world = gitea_stub_world
    email, workspace = world["email"], world["workspace"]
    state = f"gh1089-state-{world['tag']}"
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
        _delete_session(database_url, session_key)

    assert resp.status_code == 302, f"expected 302, got {resp.status_code}: {resp.text[:300]}"
    location = resp.headers["location"]
    assert location.startswith(SESSION_HOST), location
    assert "error_code" not in parse_qs(urlsplit(location).query), location

    hits = {(r["method"], urlsplit(r["path"]).path) for r in world["stub"].requests}
    assert ("POST", "/login/oauth/access_token") in hits, world["stub"].requests
    assert ("GET", "/api/v1/user") in hits, world["stub"].requests

    user = db.fetchone(
        database_url, "SELECT id FROM users WHERE email = %s", (email,)
    )
    assert user is not None, "callback created the user"
    members = db.fetchone(
        database_url,
        "SELECT COUNT(*) AS n FROM workspace_members"
        " WHERE workspace_id = %s AND member_id = %s",
        (workspace["id"], user["id"]),
    )["n"]
    assert members == 1, "accepted invite lands a membership row"

    # Invite rows are read WITHOUT a deleted_at filter ("including
    # deleted"): the consumed row must still be there, stamped.
    rows = db.fetchall(
        database_url,
        "SELECT accepted, deleted_at FROM workspace_member_invites WHERE email = %s",
        (email,),
    )
    assert len(rows) == 2, rows
    consumed = [r for r in rows if r["accepted"]]
    untouched = [r for r in rows if not r["accepted"]]
    assert len(consumed) == 1 and len(untouched) == 1, rows
    assert consumed[0]["deleted_at"] is not None, "consumed invite soft-deletes"
    assert untouched[0]["deleted_at"] is None, "unaccepted invite stays live"

    decoy = db.fetchone(
        database_url,
        "SELECT deleted_at FROM workspace_member_invites WHERE email = %s",
        (world["other"],),
    )
    assert decoy is not None and decoy["deleted_at"] is None, "other-email invite stays live"

    projects = db.fetchone(
        database_url,
        "SELECT COUNT(*) AS n FROM project_member_invites WHERE email = %s",
        (email,),
    )["n"]
    assert projects == 0, "no project invites seeded, none consumed"


def test_app_gitlab_login_joins_project_invite(base_url: str):
    """Full app-gitlab login with an accepted project invite: 302 + join.

    Runs against both backends (PIDASHCONV-1686, PB-1008 retired). The
    callback creates the user, lands the workspace-member row (role
    clamped to 5/15) and the project-member row carrying the invite's
    ``project_id``, and soft-deletes the invite. FAILS on pre-fix Rust
    (500, partial writes).
    """
    database_url = config.database_url()
    store = Database(database_url)
    tag = uuid.uuid4().hex[:8]
    owner_email = f"gl1686-own-{tag}@example.test"
    email = f"gl1686-{tag}@example.test"
    emails = [email, owner_email]
    owner = store.make_user(owner_email)
    workspace = store.make_workspace(f"gl1686-{tag}", f"gl1686-{tag}", owner["id"])
    project = store.make_project(workspace["id"], f"l6{tag}"[-8:])
    iid = str(uuid.uuid4())
    db.execute(
        database_url,
        "INSERT INTO project_member_invites (id, created_at, updated_at,"
        " project_id, workspace_id, email, accepted, token, role)"
        " VALUES (%s, now(), now(), %s, %s, %s, true, %s, %s)",
        (iid, project["id"], workspace["id"], email, f"tok-{iid[:8]}", 20),
    )
    routes = {
        ("POST", "/oauth/token"): (
            200,
            {"access_token": f"stub-at-{tag}", "token_type": "bearer"},
        ),
        ("GET", "/api/v4/user"): (
            200,
            {
                "id": 424242,
                "email": email,
                "name": "Contract User",
                "username": f"gl1686-{tag}",
                "avatar_url": None,
            },
        ),
    }
    with RoutingStub(routes) as stub:
        _delete_config(database_url, GITLAB_KEYS)
        try:
            store.make_config(
                "GITLAB_CLIENT_ID", "contract-id", category="AUTHENTICATION"
            )
            store.make_config(
                "GITLAB_CLIENT_SECRET", "contract-secret", category="AUTHENTICATION"
            )
            store.make_config("GITLAB_HOST", stub.url, category="AUTHENTICATION")
        except Exception:
            _delete_config(database_url, GITLAB_KEYS)
            _delete_world(store, emails, [workspace["id"]], [project["id"]])
            raise
        try:
            state = f"gl1686-state-{tag}"
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
                        "/auth/gitlab/callback/",
                        params={"code": "contract-stub-code", "state": state},
                    )
                finally:
                    authed.close()
            finally:
                _delete_session(database_url, session_key)
            user = db.fetchone(
                database_url, "SELECT id FROM users WHERE email = %s", (email,)
            )
            members = (
                db.fetchall(
                    database_url,
                    "SELECT workspace_id::text AS ws, role FROM workspace_members"
                    " WHERE member_id = %s",
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
            invite = db.fetchone(
                database_url,
                "SELECT accepted, deleted_at FROM project_member_invites WHERE id = %s",
                (iid,),
            )
            hits = {(r["method"], urlsplit(r["path"]).path) for r in stub.requests}
        finally:
            _delete_config(database_url, GITLAB_KEYS)
            _delete_world(store, emails, [workspace["id"]], [project["id"]])

    assert resp.status_code == 302, (
        f"expected 302, got {resp.status_code}: {resp.text[:300]}"
    )
    assert ("POST", "/oauth/token") in hits, stub.requests
    assert ("GET", "/api/v4/user") in hits, stub.requests
    assert user is not None, "callback created the user"
    assert [(m["ws"], m["role"]) for m in members] == [(workspace["id"], 15)], (
        f"invite workspace joined (role 20 clamps to 15): {members}"
    )
    assert [(m["pj"], m["ws"], m["role"]) for m in project_rows] == [
        (project["id"], workspace["id"], 15)
    ], f"invite project joined with its project_id: {project_rows}"
    assert invite is not None and invite["accepted"] is True, invite
    assert invite["deleted_at"] is not None, "consumed invite soft-deletes"


@pytest.mark.parametrize(
    "path,error_code",
    [
        ("/auth/google/callback/", "5115"),
        ("/auth/gitea/callback/", "5123"),
    ],
)
def test_error_branches_leave_invites_untouched(
    base_url: str, path: str, error_code: str
):
    """Guard branches consume nothing: the missing-code branch answers its
    provider-error redirect and the accepted invite stays live (present,
    ``deleted_at`` NULL, no membership, no user). Runs on both backends;
    green before and after the fix — it pins the consumption boundary the
    success test crosses."""
    database_url = config.database_url()
    store = Database(database_url)
    tag = uuid.uuid4().hex[:8]
    owner = store.make_user(f"gh1089-own-{tag}@example.test")
    email = f"gh1089-{tag}@example.test"
    workspace = store.make_workspace(f"gh1089-{tag}", f"gh1089-{tag}", owner["id"])
    store.make_invite(workspace["id"], email, accepted=True, token=f"tok-1089-{tag}")
    state = f"gh1089-state-{tag}"
    emails = [email, owner["email"]]
    session_key = _forge_oauth_session(database_url, state)
    try:
        authed = httpx.Client(
            base_url=base_url,
            cookies={SESSION_COOKIE: session_key},
            timeout=10,
            follow_redirects=False,
        )
        try:
            resp = authed.get(path, params={"state": state})
        finally:
            authed.close()
        # Read the post-state before teardown deletes the world.
        invite = db.fetchone(
            database_url,
            "SELECT deleted_at FROM workspace_member_invites WHERE email = %s",
            (email,),
        )
        user = db.fetchone(
            database_url, "SELECT id FROM users WHERE email = %s", (email,)
        )
    finally:
        _delete_session(database_url, session_key)
        _delete_world(store, emails, [workspace["id"]])

    assert resp.status_code == 302, f"expected 302, got {resp.status_code}: {resp.text[:200]}"
    query = parse_qs(urlsplit(resp.headers["location"]).query)
    assert query.get("error_code") == [error_code], resp.headers["location"]

    assert invite is not None and invite["deleted_at"] is None, invite
    assert user is None, "no user created on the error branch"
