# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Sync-created/adopted mirrors must run the post_save legs (PIDASHCONV-1576).

Django runs ``Issue.save()`` on every sync write (both ``update_or_create``
arms of the legacy ``_upsert_issue``; the create arm of the provider-neutral
``_upsert_issue`` — its update arm is a signal-free ``filter().update``), so
both ``post_save`` receivers fire:

* ``orchestration.signals.fire_state_transition`` (ignores ``created``):
  a sync-create into a ticking project default (``None`` -> default) enters
  the bucket — ``scheduling.reconcile`` arms the issue ticker and the entry
  run is dispatched;
* ``bgtasks.github_signals.trigger_completion_comment`` (skips ``created``):
  a sync-update that adopts a completed-group default onto a stateless row
  (``None`` -> completed) enqueues ``post_completion_comment``. The adopt is
  in-memory only — ``update_or_create`` saves with ``update_fields`` that
  exclude ``state`` — so the row keeps ``state_id`` NULL while the receivers
  evaluate against the adopted default.

The Rust port inlined the save columns but called neither leg, so
sync-created issues in ticking defaults sat without a ticker or entry run
and sync-adopted issues into completed never got their comment-back. These
cases pin both legs at the worker level, on both backends (Rust replay:
``test_rust_replay_signal_legs.py``).

Stub mechanics follow PIDASHCONV-1472 (:mod:`test_github_redirect`): one
shared route table (plain 200s here — no renames), reached by the Django
worker through its MITM proxy and by the Rust worker through
``PIDASH_GITHUB_API_BASE``. Stack: dedicated CI job; fresh Postgres (this
job's ``DATABASE_URL``), the job broker (``CELERY_BROKER_URL``), one Django
celery worker booted with the proxy env below. Every test self-seeds its
chain (per-test workspace) and deletes worker-mirrored rows on teardown.
Running against a shared worker that was not booted with the proxy env
skips every test (loop-membership gate).
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
LEGACY_SYNC_ONE = "pi_dash.bgtasks.github_sync_task.sync_one_repo"

SIG_OWNER = "signal-leg-owner"
SIG_NAME = "signal-leg-repo"
ISSUE_NUMBER = 11
COMMENT_ID = 5757
PLAIN_PAT = "signal-leg-probe-token-1576"

STUB_PORT = int(os.environ.get("SIGNAL_LEGS_STUB_PORT", "15761"))
MITM_PORT = int(os.environ.get("SIGNAL_LEGS_MITM_PORT", "15762"))
CA_BUNDLE_PATH = os.environ.get(
    "SIGNAL_LEGS_CA_BUNDLE", "/tmp/pidash-1576-ca-bundle.pem"
)

ISSUE_PAYLOAD = {
    "id": 110000011,
    "number": ISSUE_NUMBER,
    "title": "Signal leg probe issue",
    "body": "A synced issue still enters the bucket.",
    "state": "open",
    "user": {"login": "signal-octocat"},
    "html_url": f"https://github.com/{SIG_OWNER}/{SIG_NAME}/issues/{ISSUE_NUMBER}",
    "created_at": "2026-01-02T03:04:05Z",
    "updated_at": "2026-01-03T04:05:06Z",
}

COMMENT_PAYLOAD = {
    "id": COMMENT_ID,
    "body": "This issue has been completed in Pi Dash.",
    "user": {"login": "signal-octocat"},
    "html_url": f"https://github.com/{SIG_OWNER}/{SIG_NAME}/issues/{ISSUE_NUMBER}#issuecomment-{COMMENT_ID}",
    "created_at": "2026-01-04T05:06:07Z",
    "updated_at": "2026-01-04T05:06:07Z",
}


def signal_routes() -> dict:
    """Shared stub routes: one open issue, no comments, comment POST 201."""
    old_issues = f"/repos/{SIG_OWNER}/{SIG_NAME}/issues"
    return {
        ("GET", old_issues): (200, [ISSUE_PAYLOAD]),
        ("GET", f"{old_issues}/{ISSUE_NUMBER}/comments"): (200, []),
        ("GET", f"{old_issues}/comments"): (200, []),
        ("POST", f"{old_issues}/{ISSUE_NUMBER}/comments"): (201, COMMENT_PAYLOAD),
    }


def _secret_key() -> str:
    value = os.environ.get("SECRET_KEY")
    if not value:
        raise pytest.skip("SECRET_KEY is not set — must match the worker's")
    return value


@pytest.fixture(scope="module")
def signal_stack(tmp_path_factory):
    """MITM proxy on the fixed port + freshly minted CA bundle."""
    bundle, cert, key = sinks.make_ca_bundle(tmp_path_factory.mktemp("mitm-ca"))
    os.replace(bundle, CA_BUNDLE_PATH)
    with sinks.MitmProxy(
        signal_routes(), tls_cert=cert, tls_key=key, port=MITM_PORT
    ) as proxy:
        yield proxy


@pytest.fixture()
def signal_case(database_url, db_conn):
    """Per-test self-seeded chain + scope; deletes mirrors on teardown."""
    tag = f"sig{uuid.uuid4().hex[:8]}"
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
        "conn": db_conn,
    }
    seed.cleanup_workspace_mirrors(database_url, workspace_id)
    scope.cleanup()


def _cipher_token() -> str:
    token = seed.encrypt_token_for_worker(PLAIN_PAT, _secret_key())
    assert token and token != PLAIN_PAT
    return token


def project_state(
    signal_case: dict, *, name: str, group: str, default: bool = True
) -> str:
    """One project state (tracked); the chain's backlog state stays put."""
    project = signal_case["anchor"]["projects"][0]
    row = db.insert_row(
        signal_case["conn"],
        "states",
        {
            "project_id": str(project["id"]),
            "workspace_id": str(project["workspace_id"]),
            "name": name,
            "description": "",
            "color": "#000000",
            "slug": "",
            "group": group,
            "sequence": 1000,
            "is_triage": False,
            "default": default,
        },
    )
    return signal_case["scope"].track("states", str(row["id"]))


def seed_binding(database_url: str, signal_case: dict) -> str:
    """Enabled provider-neutral binding whose repo matches the stub."""
    anchor = signal_case["anchor"]
    scope = signal_case["scope"]
    account_id = seed.provider_account(
        database_url, anchor, scope, token=_cipher_token()
    )
    repo_id = seed.repository(database_url, scope, suffix="sigleg")
    db.execute(
        database_url,
        "UPDATE git_repositories SET namespace = %s, name = %s, full_name = %s"
        " WHERE id = %s",
        (SIG_OWNER, SIG_NAME, f"{SIG_OWNER}/{SIG_NAME}", repo_id),
    )
    return seed.binding(
        database_url, anchor, scope, account_id=account_id, repository_id=repo_id
    )


def seed_legacy_sync(database_url: str, signal_case: dict) -> str:
    """Enabled legacy repo sync pointing at the stub owner/name."""
    scope = signal_case["scope"]
    built = signal_case["anchor"]
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
        owner=SIG_OWNER,
        name=SIG_NAME,
    )["sync_id"]


def seed_stateless_mirror(
    database_url: str, signal_case: dict, *, sync_id: str
) -> tuple[str, str]:
    """A legacy mirror row pair whose Issue carries no state (adopt target).

    Returns ``(issue_id, issue_sync_id)``. The Issue's upsert key
    (``external_source``/``external_id``) matches the stub listing, so the
    next ``sync_one_repo`` takes the update arm and ``Issue.save()`` adopts
    the project default.
    """
    project = signal_case["anchor"]["projects"][0]
    issue = db.insert_row(
        signal_case["conn"],
        "issues",
        {
            "workspace_id": str(project["workspace_id"]),
            "project_id": str(project["id"]),
            "state_id": None,
            "name": f"[github_{ISSUE_NUMBER}] Signal leg probe issue",
            "description_json": {},
            "description_html": "<p></p>",
            "priority": "none",
            "complexity_score": 0,
            "sequence_id": 7,
            "is_draft": False,
            "sort_order": 65535,
            "git_work_branch": "",
            "workpad": "",
            "external_source": "github",
            "external_id": str(ISSUE_NUMBER),
        },
    )
    issue_id = signal_case["scope"].track("issues", str(issue["id"]))
    sync_row_id = seed.legacy_issue_sync(
        database_url,
        signal_case["scope"],
        sync_id=sync_id,
        issue_id=issue_id,
        project_id=str(project["id"]),
        workspace_id=str(project["workspace_id"]),
        repo_issue_id=ISSUE_NUMBER,
    )
    return issue_id, sync_row_id


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
    """Publish one job, wait for a terminal row state, gate on the proxy."""
    before = _mitm_connects(proxy)
    _ = broker_url  # broker_redis resolves CELERY_BROKER_URL from env
    broker.publish_task(task, args=tuple(args))
    row = db.wait_for(database_url, sql, params)
    assert row is not None, f"{task} never settled ({what})"
    if _mitm_connects(proxy) <= before:
        raise pytest.skip(
            "Django worker is not routed through the signal-legs MITM — boot it"
            f" with HTTPS_PROXY=http://127.0.0.1:{MITM_PORT} and"
            f" REQUESTS_CA_BUNDLE={CA_BUNDLE_PATH}"
        )
    return row


def hits_matching(entries: list[dict], method: str, prefix: str) -> list[dict]:
    """Hits with ``method`` whose path starts with ``prefix`` (superset match)."""
    return [
        entry
        for entry in entries
        if entry["method"] == method and entry["path"].startswith(prefix)
    ]


def mirrored_issue_id(database_url: str, project_id: str) -> str | None:
    row = db.fetchone(
        database_url,
        "SELECT id FROM issues WHERE project_id = %s"
        " AND external_source = 'github' AND external_id = %s"
        " AND deleted_at IS NULL",
        (project_id, str(ISSUE_NUMBER)),
    )
    return str(row["id"]) if row else None


def assert_ticker_armed(database_url: str, issue_id: str) -> dict:
    """The entered-bucket leg armed the issue clock (reconcile wrote it)."""
    row = db.wait_for(
        database_url,
        "SELECT issue_id, enabled, disarm_reason FROM issue_agent_ticker"
        " WHERE issue_id = %s",
        (issue_id,),
    )
    assert row is not None, f"no ticker row for synced issue {issue_id}"
    assert row["enabled"] is True, row
    assert row["disarm_reason"] == "", row
    return row


def assert_no_completion_stamp(table_row: dict) -> None:
    metadata = table_row["metadata"] or {}
    assert "completion_comment_id" not in metadata, metadata
    assert "completion_comment_error" not in metadata, metadata


def assert_post_reached_stub(entries: list[dict], *, body_marker: str) -> None:
    posts = hits_matching(
        entries, "POST", f"/repos/{SIG_OWNER}/{SIG_NAME}/issues/{ISSUE_NUMBER}/comments"
    )
    assert posts, "stub never saw the completion comment POST"
    bodies = [json.loads(hit["body"] or b"{}") for hit in posts]
    assert any(body_marker in (item.get("body") or "") for item in bodies), bodies


def test_binding_sync_create_into_ticking_default_arms_ticker(
    database_url, broker_url, signal_stack, signal_case
):
    ticking_id = project_state(signal_case, name="In Progress", group="started")
    binding_id = seed_binding(database_url, signal_case)
    project_id = str(signal_case["anchor"]["projects"][0]["id"])
    _publish_and_settle(
        broker_url,
        database_url,
        signal_stack,
        SYNC_ONE,
        [binding_id],
        "SELECT last_sync_error, last_synced_at FROM git_repository_bindings"
        " WHERE id = %s AND (last_synced_at IS NOT NULL OR last_sync_error <> '')",
        (binding_id,),
        "sync_one_binding",
    )
    row = db.fetchone(
        database_url,
        "SELECT last_sync_error, last_synced_at FROM git_repository_bindings"
        " WHERE id = %s",
        (binding_id,),
    )
    assert row["last_sync_error"] == "", row
    assert row["last_synced_at"] is not None, row
    issue_id = mirrored_issue_id(database_url, project_id)
    assert issue_id is not None, "no mirrored Issue row"
    issue = db.fetchone(
        database_url, "SELECT state_id FROM issues WHERE id = %s", (issue_id,)
    )
    assert str(issue["state_id"]) == ticking_id, issue
    assert_ticker_armed(database_url, issue_id)
    sync_row = db.fetchone(
        database_url,
        "SELECT metadata FROM git_issue_syncs WHERE binding_id = %s"
        " AND external_iid = %s",
        (binding_id, str(ISSUE_NUMBER)),
    )
    assert sync_row is not None
    # Created, not transitioned: the completion leg must not fire.
    assert_no_completion_stamp(sync_row)


def test_legacy_sync_create_into_ticking_default_arms_ticker(
    database_url, broker_url, signal_stack, signal_case
):
    ticking_id = project_state(signal_case, name="In Progress", group="started")
    sync_id = seed_legacy_sync(database_url, signal_case)
    project_id = str(signal_case["anchor"]["projects"][0]["id"])
    _publish_and_settle(
        broker_url,
        database_url,
        signal_stack,
        LEGACY_SYNC_ONE,
        [sync_id],
        "SELECT last_sync_error, last_synced_at FROM github_repository_syncs"
        " WHERE id = %s AND (last_synced_at IS NOT NULL OR last_sync_error <> '')",
        (sync_id,),
        "legacy sync_one_repo",
    )
    row = db.fetchone(
        database_url,
        "SELECT last_sync_error, last_synced_at FROM github_repository_syncs"
        " WHERE id = %s",
        (sync_id,),
    )
    assert row["last_sync_error"] == "", row
    assert row["last_synced_at"] is not None, row
    issue_id = mirrored_issue_id(database_url, project_id)
    assert issue_id is not None, "no mirrored Issue row"
    issue = db.fetchone(
        database_url, "SELECT state_id FROM issues WHERE id = %s", (issue_id,)
    )
    assert str(issue["state_id"]) == ticking_id, issue
    assert_ticker_armed(database_url, issue_id)
    sync_row = db.fetchone(
        database_url,
        "SELECT metadata FROM github_issue_syncs WHERE repository_sync_id = %s"
        " AND repo_issue_id = %s",
        (sync_id, ISSUE_NUMBER),
    )
    assert sync_row is not None
    # Created, not transitioned: the completion leg must not fire.
    assert_no_completion_stamp(sync_row)


def _publish_legacy_and_wait_comment(
    database_url: str, proxy, sync_id: str, sync_row_id: str
) -> dict:
    """Publish one legacy sync; settle on the mirror's comment outcome + gate."""
    before = _mitm_connects(proxy)
    broker.publish_task(LEGACY_SYNC_ONE, args=(sync_id,))
    row = db.wait_for(
        database_url,
        "SELECT metadata FROM github_issue_syncs WHERE id = %s"
        " AND (metadata ? 'completion_comment_id'"
        " OR metadata ? 'completion_comment_error')",
        (sync_row_id,),
    )
    assert row is not None, "adopt never produced a completion comment outcome"
    if _mitm_connects(proxy) <= before:
        raise pytest.skip(
            "Django worker is not routed through the signal-legs MITM — boot it"
            f" with HTTPS_PROXY=http://127.0.0.1:{MITM_PORT} and"
            f" REQUESTS_CA_BUNDLE={CA_BUNDLE_PATH}"
        )
    return row


def assert_no_ticker(database_url: str, issue_id: str) -> None:
    row = db.fetchone(
        database_url,
        "SELECT issue_id FROM issue_agent_ticker WHERE issue_id = %s",
        (issue_id,),
    )
    assert row is None, f"unexpected ticker row for issue {issue_id}: {row}"


def test_legacy_sync_adopt_into_completed_enqueues_completion_comment(
    database_url, broker_url, signal_stack, signal_case
):
    # The adopt is in-memory only: Django 4.2 `update_or_create` saves the
    # update arm with `update_fields=set(defaults)+auto` (no `state`), so the
    # receivers evaluate prev=None vs current=adopted (completion enqueued)
    # while the row keeps state_id NULL.
    project_state(signal_case, name="Done", group="completed")
    sync_id = seed_legacy_sync(database_url, signal_case)
    issue_id, sync_row_id = seed_stateless_mirror(
        database_url, signal_case, sync_id=sync_id
    )
    # The adopt enqueues post_completion_comment, which the same worker runs.
    row = _publish_legacy_and_wait_comment(
        database_url, signal_stack, sync_id, sync_row_id
    )
    metadata = row["metadata"] or {}
    # Legacy stores the payload id verbatim (JSON number).
    assert metadata.get("completion_comment_id") == COMMENT_ID, metadata
    assert "completion_comment_error" not in metadata, metadata
    issue = db.fetchone(
        database_url, "SELECT state_id FROM issues WHERE id = %s", (issue_id,)
    )
    assert issue["state_id"] is None, issue
    # Completed is not a delegation trigger: the orchestration leg runs but
    # arms nothing.
    assert_no_ticker(database_url, issue_id)
    assert_post_reached_stub(
        signal_stack.snapshot(), body_marker="completed in Pi Dash"
    )


def test_legacy_sync_adopt_into_ticking_default_arms_ticker(
    database_url, broker_url, signal_stack, signal_case
):
    # Same in-memory adopt as the completed case, but the adopted default is
    # ticking: the orchestration leg arms the clock while the completion leg
    # skips (non-completed). The row still keeps state_id NULL.
    project_state(signal_case, name="In Progress", group="started")
    sync_id = seed_legacy_sync(database_url, signal_case)
    issue_id, sync_row_id = seed_stateless_mirror(
        database_url, signal_case, sync_id=sync_id
    )
    _publish_and_settle(
        broker_url,
        database_url,
        signal_stack,
        LEGACY_SYNC_ONE,
        [sync_id],
        "SELECT last_sync_error, last_synced_at FROM github_repository_syncs"
        " WHERE id = %s AND (last_synced_at IS NOT NULL OR last_sync_error <> '')",
        (sync_id,),
        "legacy sync_one_repo",
    )
    row = db.fetchone(
        database_url,
        "SELECT last_sync_error, last_synced_at FROM github_repository_syncs"
        " WHERE id = %s",
        (sync_id,),
    )
    assert row["last_sync_error"] == "", row
    assert row["last_synced_at"] is not None, row
    issue = db.fetchone(
        database_url, "SELECT state_id FROM issues WHERE id = %s", (issue_id,)
    )
    assert issue["state_id"] is None, issue
    assert_ticker_armed(database_url, issue_id)
    sync_row = db.fetchone(
        database_url,
        "SELECT metadata FROM github_issue_syncs WHERE id = %s",
        (sync_row_id,),
    )
    assert sync_row is not None
    assert_no_completion_stamp(sync_row)
