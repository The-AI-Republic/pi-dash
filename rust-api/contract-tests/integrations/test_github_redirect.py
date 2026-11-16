# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Renamed-repo redirects: 301/307 followed on the live Django worker (PIDASHCONV-1472).

Bug: the Rust job GitHub clients built reqwest with ``Policy::none()`` and
mapped any non-2xx to ``Transport``, so GitHub's 301 (GET) / 307 (POST) for a
renamed or transferred repo became a General error — ``last_sync_error`` set,
retry with backoff forever, completion comments never posted. Django's
``GithubClient._request`` uses ``requests.request``, which follows redirects
and keeps ``Authorization`` on same-host hops, so Django syncs through
renames. The fix (``Policy::limited(10)`` in both clients) is pinned here at
the worker level, on both backends (Rust replay: ``test_rust_replay_redirect.py``).

How the stub reaches each worker: neither backend's GitHub sync path takes
an api-base override in production (both hardcode https://api.github.com),
so each leg uses its own mechanism over one shared route table
(:func:`redirect_routes`):

- Django leg (this module): the worker boots with ``HTTPS_PROXY`` pointing
  at :class:`_harness.sinks.MitmProxy` (fixed ``GITHUB_REDIRECT_MITM_PORT``)
  and ``REQUESTS_CA_BUNDLE`` at a bundle this module writes (test CA minted
  per run + system roots) to ``GITHUB_REDIRECT_CA_BUNDLE``. The proxy MITMs
  api.github.com and serves the 301/307 routes in-tunnel; anything else is
  blind-relayed. No Python file changed.
- Rust leg (replay): the worker boots with ``PIDASH_GITHUB_API_BASE``
  pointing at a plain-HTTP :class:`_harness.sinks.RedirectStub` (fixed
  ``GITHUB_REDIRECT_STUB_PORT``), which rustls never has to verify. That
  variable is the only product-code addition, unset in production.

Stack (dedicated CI job; no shared worker, no beat): fresh Postgres (this
job's ``DATABASE_URL``), the job broker (``CELERY_BROKER_URL``), one Django
celery worker with the proxy env above. Every test self-seeds its chain
(per-test workspace) and deletes worker-mirrored rows on teardown, so the
module is hermetic. Running it against a shared worker that was not booted
with the proxy env skips every test (loop-membership gate: the proxy must
see a fresh CONNECT for api.github.com after each publish).
"""

from __future__ import annotations

import json
import os
import uuid

import pytest

from _harness import broker_redis as broker
from _harness import db
from _harness import seed as seed_helpers
from _harness import sinks

from . import seed

SYNC_ONE = "pi_dash.bgtasks.git_sync_task.sync_one_binding"
POST_COMMENT = "pi_dash.bgtasks.git_sync_task.post_completion_comment"
LEGACY_SYNC_ONE = "pi_dash.bgtasks.github_sync_task.sync_one_repo"
LEGACY_POST_COMMENT = "pi_dash.bgtasks.github_sync_task.post_completion_comment"

OLD_OWNER = "redirect-oldowner"
OLD_NAME = "redirect-oldrepo"
ISSUE_NUMBER = 7
COMMENT_ID = 4242
PLAIN_PAT = "redirect-probe-token-1472"

STUB_PORT = int(os.environ.get("GITHUB_REDIRECT_STUB_PORT", "14721"))
MITM_PORT = int(os.environ.get("GITHUB_REDIRECT_MITM_PORT", "14722"))
CA_BUNDLE_PATH = os.environ.get(
    "GITHUB_REDIRECT_CA_BUNDLE", "/tmp/pidash-1472-ca-bundle.pem"
)

ISSUE_PAYLOAD = {
    "id": 110000007,
    "number": ISSUE_NUMBER,
    "title": "Redirect probe issue",
    "body": "A renamed repo still syncs.",
    "state": "open",
    "user": {"login": "redirect-octocat"},
    "html_url": f"https://github.com/{OLD_OWNER}/{OLD_NAME}/issues/{ISSUE_NUMBER}",
    "created_at": "2026-01-02T03:04:05Z",
    "updated_at": "2026-01-03T04:05:06Z",
}

COMMENT_PAYLOAD = {
    "id": COMMENT_ID,
    "body": "This issue has been completed in Pi Dash.",
    "user": {"login": "redirect-octocat"},
    "html_url": f"https://github.com/{OLD_OWNER}/{OLD_NAME}/issues/{ISSUE_NUMBER}#issuecomment-{COMMENT_ID}",
    "created_at": "2026-01-04T05:06:07Z",
    "updated_at": "2026-01-04T05:06:07Z",
}


def redirect_routes() -> dict:
    """Shared stub routes: old-repo paths 301/307 to /repositories/1/... .

    ``{base}`` formats to the serving leg's own base (the MITM's
    https://api.github.com or the plain stub's http://127.0.0.1:port), so
    each client follows same-host Locations. Comment listings answer []
    (mirroring no comments still syncs); the redirect hop itself is what
    the tests pin.
    """
    old_issues = f"/repos/{OLD_OWNER}/{OLD_NAME}/issues"
    return {
        ("GET", old_issues): (
            301,
            b"",
            {"Location": "{base}/repositories/1/issues?state=open"},
        ),
        ("GET", "/repositories/1/issues"): (200, [ISSUE_PAYLOAD]),
        ("GET", f"{old_issues}/{ISSUE_NUMBER}/comments"): (
            301,
            b"",
            {"Location": "{base}/repositories/1/issues/7/comments?per_page=100"},
        ),
        ("GET", "/repositories/1/issues/7/comments"): (200, []),
        ("GET", f"{old_issues}/comments"): (
            301,
            b"",
            {"Location": "{base}/repositories/1/issues/comments?per_page=100"},
        ),
        ("GET", "/repositories/1/issues/comments"): (200, []),
        ("POST", f"{old_issues}/{ISSUE_NUMBER}/comments"): (
            307,
            b"",
            {"Location": "{base}/repositories/1/issues/7/comments"},
        ),
        ("POST", "/repositories/1/issues/7/comments"): (201, COMMENT_PAYLOAD),
    }


def _secret_key() -> str:
    value = os.environ.get("SECRET_KEY")
    if not value:
        raise pytest.skip("SECRET_KEY is not set — must match the worker's")
    return value


@pytest.fixture(scope="module")
def redirect_stack(tmp_path_factory):
    """MITM proxy on the fixed port + freshly minted CA bundle.

    The bundle publishes atomically to the fixed path the worker was
    booted with; the worker reads it per connection, and makes no
    api.github.com request before the first test publishes.
    """
    bundle, cert, key = sinks.make_ca_bundle(tmp_path_factory.mktemp("mitm-ca"))
    os.replace(bundle, CA_BUNDLE_PATH)
    with sinks.MitmProxy(
        redirect_routes(), tls_cert=cert, tls_key=key, port=MITM_PORT
    ) as proxy:
        yield proxy


@pytest.fixture()
def redirect_case(database_url, db_conn):
    """Per-test self-seeded chain + scope; deletes mirrors on teardown."""
    tag = f"redir{uuid.uuid4().hex[:8]}"
    scope = seed.Scope(database_url)
    built = seed_helpers.issue_chain(db_conn, tag)
    for table, row in (
        ("users", built["owner"]),
        ("workspaces", built["workspace"]),
        ("projects", built["project"]),
        ("states", built["state"]),
        ("issues", built["issue"]),
    ):
        scope.track(table, str(row["id"]))
    workspace_id = str(built["workspace"]["id"])
    yield {
        "scope": scope,
        "workspace_id": workspace_id,
        "anchor": {
            "workspace": built["workspace"],
            "projects": [built["project"]],
            "actor": built["owner"],
        },
        "issue": built["issue"],
    }
    seed.cleanup_workspace_mirrors(database_url, workspace_id)
    scope.cleanup()


def _cipher_token() -> str:
    token = seed.encrypt_token_for_worker(PLAIN_PAT, _secret_key())
    assert token and token != PLAIN_PAT
    return token


def _mitm_connects(proxy) -> int:
    return sum(
        1
        for entry in proxy.snapshot()
        if entry["method"] == "CONNECT"
        and entry["path"].split(":")[0].lower() == "api.github.com"
    )


def _publish_and_settle(
    broker_url: str,
    database_url: str,
    proxy,
    task: str,
    args: list,
    sql: str,
    params: tuple,
    what: str,
) -> dict:
    """Publish one job, wait for a terminal row state, gate on the proxy.

    The row settles on success (synced_at / comment id) or on recorded
    error; the loop-membership gate then skips when the consuming worker
    never came through this module's MITM (a shared worker without the
    proxy env), instead of failing on its direct-to-GitHub 401.
    """
    before = _mitm_connects(proxy)
    _ = broker_url  # broker_redis resolves CELERY_BROKER_URL from env
    broker.publish_task(task, args=tuple(args))
    row = db.wait_for(database_url, sql, params)
    assert row is not None, f"{task} never settled ({what})"
    if _mitm_connects(proxy) <= before:
        raise pytest.skip(
            "Django worker is not routed through the redirect MITM — boot it"
            f" with HTTPS_PROXY=http://127.0.0.1:{MITM_PORT} and"
            f" REQUESTS_CA_BUNDLE={CA_BUNDLE_PATH}"
        )
    return row


def hits_matching(entries: list[dict], method: str, prefix: str) -> list[dict]:
    """Hits with ``method`` whose path starts with ``prefix`` (superset match).

    Transport-agnostic over MITM (Django leg) and RedirectStub (Rust leg)
    snapshots: CONNECT entries never match a GET/POST method, so no flag
    filtering is needed. Superset (not exact-count) matching keeps the
    assertions safe under idempotent duplicate deliveries.
    """
    return [
        entry
        for entry in entries
        if entry["method"] == method and entry["path"].startswith(prefix)
    ]


def assert_sync_binding_ok(database_url: str, binding_id: str) -> None:
    row = db.fetchone(
        database_url,
        "SELECT last_sync_error, last_synced_at FROM git_repository_bindings"
        " WHERE id = %s",
        (binding_id,),
    )
    assert row["last_sync_error"] == "", row
    assert row["last_synced_at"] is not None, row
    mirror = db.fetchone(
        database_url,
        "SELECT external_iid, remote_state FROM git_issue_syncs"
        " WHERE binding_id = %s AND external_iid = %s",
        (binding_id, str(ISSUE_NUMBER)),
    )
    assert mirror is not None, "no mirrored issue for the renamed repo"
    assert mirror["remote_state"] == "open", mirror
    issue = db.fetchone(
        database_url,
        "SELECT name FROM issues WHERE project_id ="
        " (SELECT project_id FROM git_repository_bindings WHERE id = %s)"
        " AND external_source = 'github' AND external_id = %s",
        (binding_id, str(ISSUE_NUMBER)),
    )
    assert issue is not None, "no mirrored Issue row"
    assert issue["name"] == f"[github_{ISSUE_NUMBER}] Redirect probe issue", issue


def assert_comment_posted(
    database_url: str, sync_id: str, *, table: str, expect_id
) -> None:
    row = db.fetchone(
        database_url, f"SELECT metadata FROM {table} WHERE id = %s", (sync_id,)
    )
    assert row is not None
    assert row["metadata"].get("completion_comment_id") == expect_id, row["metadata"]
    assert "completion_comment_error" not in row["metadata"], row["metadata"]


def assert_post_reached_stub(entries: list[dict], *, body_marker: str) -> None:
    posts = hits_matching(entries, "POST", "/repositories/1/issues/7/comments")
    assert posts, "stub never saw the followed comment POST"
    bodies = [json.loads(hit["body"] or b"{}") for hit in posts]
    assert any(body_marker in (item.get("body") or "") for item in bodies), bodies


def seed_old_binding(database_url: str, redirect_case: dict) -> str:
    """Enabled binding whose repo still carries the pre-rename full_name."""
    anchor = redirect_case["anchor"]
    scope = redirect_case["scope"]
    account_id = seed.provider_account(
        database_url, anchor, scope, token=_cipher_token()
    )
    repo_id = seed.repository(database_url, scope, suffix="redir")
    db.execute(
        database_url,
        "UPDATE git_repositories SET namespace = %s, name = %s, full_name = %s"
        " WHERE id = %s",
        (OLD_OWNER, OLD_NAME, f"{OLD_OWNER}/{OLD_NAME}", repo_id),
    )
    return seed.binding(
        database_url, anchor, scope, account_id=account_id, repository_id=repo_id
    )


def seed_old_legacy_sync(database_url: str, redirect_case: dict) -> str:
    """Enabled legacy repo sync pointing at the pre-rename owner/name."""
    scope = redirect_case["scope"]
    built = redirect_case["anchor"]
    workspace_id = str(built["workspace"]["id"])
    project_id = str(built["projects"][0]["id"])
    actor_id = str(built["actor"]["id"])
    wi_id = seed.legacy_workspace_integration(
        database_url,
        scope,
        workspace_id=workspace_id,
        actor_id=actor_id,
        token_cipher=_cipher_token(),
    )
    return seed.legacy_repository_sync(
        database_url,
        scope,
        workspace_id=workspace_id,
        project_id=project_id,
        actor_id=actor_id,
        wi_id=wi_id,
        owner=OLD_OWNER,
        name=OLD_NAME,
    )["sync_id"]


def assert_sync_repo_ok(database_url: str, sync_id: str, project_id: str) -> None:
    row = db.fetchone(
        database_url,
        "SELECT last_sync_error, last_synced_at FROM github_repository_syncs"
        " WHERE id = %s",
        (sync_id,),
    )
    assert row["last_sync_error"] == "", row
    assert row["last_synced_at"] is not None, row
    mirror = db.fetchone(
        database_url,
        "SELECT repo_issue_id FROM github_issue_syncs"
        " WHERE repository_sync_id = %s AND repo_issue_id = %s",
        (sync_id, ISSUE_NUMBER),
    )
    assert mirror is not None, "no mirrored legacy issue for the renamed repo"
    issue = db.fetchone(
        database_url,
        "SELECT name FROM issues WHERE project_id = %s"
        " AND external_source = 'github' AND external_id = %s",
        (project_id, str(ISSUE_NUMBER)),
    )
    assert issue is not None, "no mirrored Issue row"
    assert issue["name"] == f"[github_{ISSUE_NUMBER}] Redirect probe issue", issue


def test_sync_one_binding_follows_rename_redirects(
    database_url, broker_url, redirect_stack, redirect_case
):
    binding_id = seed_old_binding(database_url, redirect_case)
    _publish_and_settle(
        broker_url,
        database_url,
        redirect_stack,
        SYNC_ONE,
        [binding_id],
        "SELECT last_sync_error, last_synced_at FROM git_repository_bindings"
        " WHERE id = %s AND (last_synced_at IS NOT NULL OR last_sync_error <> '')",
        (binding_id,),
        "sync_one_binding",
    )
    assert_sync_binding_ok(database_url, binding_id)
    entries = redirect_stack.snapshot()
    assert hits_matching(entries, "GET", f"/repos/{OLD_OWNER}/{OLD_NAME}/issues"), (
        "stub never saw the old-path issues listing"
    )
    assert hits_matching(entries, "GET", "/repositories/1/issues"), (
        "stub never saw the followed listing"
    )


def test_post_completion_comment_follows_307(
    database_url, broker_url, redirect_stack, redirect_case
):
    binding_id = seed_old_binding(database_url, redirect_case)
    sync_id = seed.git_issue_sync(
        database_url,
        redirect_case["anchor"],
        redirect_case["scope"],
        binding_id=binding_id,
        issue_id=str(redirect_case["issue"]["id"]),
        external_iid=str(ISSUE_NUMBER),
    )
    _publish_and_settle(
        broker_url,
        database_url,
        redirect_stack,
        POST_COMMENT,
        [sync_id],
        "SELECT metadata FROM git_issue_syncs WHERE id = %s"
        " AND (metadata ? 'completion_comment_id'"
        " OR metadata ? 'completion_comment_error')",
        (sync_id,),
        "post_completion_comment",
    )
    assert_comment_posted(
        database_url, sync_id, table="git_issue_syncs", expect_id=str(COMMENT_ID)
    )
    assert_post_reached_stub(
        redirect_stack.snapshot(), body_marker="completed in Pi Dash"
    )


def test_legacy_sync_one_repo_follows_rename_redirects(
    database_url, broker_url, redirect_stack, redirect_case
):
    sync_id = seed_old_legacy_sync(database_url, redirect_case)
    project_id = str(redirect_case["anchor"]["projects"][0]["id"])
    _publish_and_settle(
        broker_url,
        database_url,
        redirect_stack,
        LEGACY_SYNC_ONE,
        [sync_id],
        "SELECT last_sync_error, last_synced_at FROM github_repository_syncs"
        " WHERE id = %s AND (last_synced_at IS NOT NULL OR last_sync_error <> '')",
        (sync_id,),
        "legacy sync_one_repo",
    )
    assert_sync_repo_ok(database_url, sync_id, project_id)
    entries = redirect_stack.snapshot()
    assert hits_matching(entries, "GET", f"/repos/{OLD_OWNER}/{OLD_NAME}/issues"), (
        "stub never saw the old-path issues listing"
    )
    assert hits_matching(entries, "GET", "/repositories/1/issues"), (
        "stub never saw the followed listing"
    )


def test_legacy_post_completion_comment_follows_307(
    database_url, broker_url, redirect_stack, redirect_case
):
    sync_id = seed_old_legacy_sync(database_url, redirect_case)
    issue_sync_id = seed.legacy_issue_sync(
        database_url,
        redirect_case["scope"],
        sync_id=sync_id,
        issue_id=str(redirect_case["issue"]["id"]),
        project_id=str(redirect_case["anchor"]["projects"][0]["id"]),
        workspace_id=redirect_case["workspace_id"],
        repo_issue_id=ISSUE_NUMBER,
    )
    _publish_and_settle(
        broker_url,
        database_url,
        redirect_stack,
        LEGACY_POST_COMMENT,
        [issue_sync_id],
        "SELECT metadata FROM github_issue_syncs WHERE id = %s"
        " AND (metadata ? 'completion_comment_id'"
        " OR metadata ? 'completion_comment_error')",
        (issue_sync_id,),
        "legacy post_completion_comment",
    )
    # Legacy stores the payload id verbatim (JSON number), unlike the new
    # path's str() (github_sync_task.py:387 vs git_sync_task.py:321).
    assert_comment_posted(
        database_url, issue_sync_id, table="github_issue_syncs", expect_id=COMMENT_ID
    )
    assert_post_reached_stub(
        redirect_stack.snapshot(), body_marker="completed in Pi Dash"
    )
