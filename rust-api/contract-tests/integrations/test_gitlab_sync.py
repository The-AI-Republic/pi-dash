"""GitLab sync wiring: allowed-host gate (PIDASHCONV-1470), Django oracle leg.

Bug: the Rust git-sync jobs built the GitLab adapter with
``GitLabAdapter::new`` (passthrough token, default config), so stored
ciphertext went out as ``PRIVATE-TOKEN`` and self-hosted hosts were refused
before any call. These worker-level tests pin the *host half* on the live
Django worker; the Rust replay leg mirrors them in ``test_rust_replay.py``.

Why an unresolvable host instead of a stub: both clients enforce HTTPS and
verify TLS (Rust's reqwest trusts only Mozilla webpki roots with no test-CA
override — see ``_harness/sinks.py:make_test_ca``), so no HTTPS stub can
answer 200 to the Rust worker, and the token travels inside TLS where no
black-box observer can see it. The ``.invalid`` self-host (RFC 2606) keeps
both gates fully exercised with no stub, proxy, or fixed port: a worker
that passes the gate fails fast at DNS (retry path, account stays
connected); a worker that refuses the gate records ``GitLab host is not
allowed`` and degrades the account. The token-decrypt half is pinned by
Rust unit tests over a recording fake transport (see the fix commit) plus
the Django probe evidence in the issue's workpad.

Worker env required (both backends): GITLAB_ALLOWED_HOSTS must include
``seed.GITLAB_SELFHOST``; SECRET_KEY must match the pytest process env so
the seeded ciphertext decrypts. The Django worker runs on the redis
broker (kombu wire format, identical Celery v2); no beat (a beat fan-out
would add noise). No HTTP server is needed (BASE_URL unused).
"""

from __future__ import annotations

import os
import uuid

import pytest

from _harness import broker_redis as broker
from _harness import db
from _harness import seed as seed_helpers

from . import seed

SYNC_ONE = "pi_dash.bgtasks.git_sync_task.sync_one_binding"
POST_COMMENT = "pi_dash.bgtasks.git_sync_task.post_completion_comment"

PLAIN_PAT = "contract-test-gitlab-pat-1470"


def _secret_key() -> str:
    value = os.environ.get("SECRET_KEY")
    if not value:
        raise pytest.skip("SECRET_KEY is not set — must match the worker's")
    return value


@pytest.fixture()
def gl_scope(database_url):
    """Teardown tracker needing no borrowed anchor rows (self-seeded chain)."""
    scope = seed.Scope(database_url)
    yield scope
    scope.cleanup()


@pytest.fixture()
def chain(db_conn, gl_scope):
    """Self-seeded anchor: user -> workspace -> project -> state -> issue."""
    built = seed_helpers.issue_chain(db_conn, f"glsync{uuid.uuid4().hex[:8]}")
    for table, row in (
        ("users", built["owner"]),
        ("workspaces", built["workspace"]),
        ("projects", built["project"]),
        ("states", built["state"]),
        ("issues", built["issue"]),
    ):
        gl_scope.track(table, str(row["id"]))
    return {
        "workspace": built["workspace"],
        "projects": [built["project"]],
        "actor": built["owner"],
        "state": built["state"],
        "issue": built["issue"],
    }


@pytest.fixture()
def gitlab_binding(database_url, chain, gl_scope) -> dict:
    """Enabled GitLab binding with a Fernet-encrypted token (production shape)."""
    token = seed.encrypt_token_for_worker(PLAIN_PAT, _secret_key())
    assert token and token != PLAIN_PAT
    return seed.gitlab_binding(database_url, chain, gl_scope, token=token)


def _publish_and_drain(task: str, args) -> str:
    before = broker.queue_depth()
    task_id = broker.publish_task(task, args=tuple(args))
    db.wait_for_condition(
        lambda: broker.queue_depth() <= before or None,
        what=f"Django worker consumed {task}",
    )
    return task_id


def test_sync_one_allowed_selfhosted_passes_gate(
    database_url, chain, gl_scope, gitlab_binding
):
    binding_id = gitlab_binding["binding_id"]
    account_id = gitlab_binding["account_id"]

    _publish_and_drain(SYNC_ONE, [binding_id])

    # The attempt must reach the transport (DNS fails there): a recorded,
    # non-empty error with no gate text, and the account stays connected.
    row = db.wait_for(
        database_url,
        "SELECT last_sync_error, last_synced_at FROM git_repository_bindings"
        " WHERE id = %s AND last_sync_error <> ''",
        (binding_id,),
    )
    assert row is not None, "enabled GitLab binding shows no sync attempt"
    assert "not allowed" not in row["last_sync_error"], row["last_sync_error"]
    assert "must use HTTPS" not in row["last_sync_error"], row["last_sync_error"]
    assert "gitlab-allowed.invalid" in row["last_sync_error"], row["last_sync_error"]
    assert row["last_synced_at"] is None

    account = db.fetchone(
        database_url,
        "SELECT status, last_check_error FROM git_provider_accounts WHERE id = %s",
        (account_id,),
    )
    assert account["status"] == "connected", account
    assert account["last_check_error"] == "", account

    # DNS failed before any listing: no mirror rows, and no retry storm
    # followed within the test window (the 60s retry lands after teardown
    # and finds the binding gone).
    assert (
        db.fetchone(
            database_url,
            "SELECT count(*) AS n FROM git_issue_syncs WHERE binding_id = %s",
            (binding_id,),
        )["n"]
        == 0
    )


def test_post_completion_allowed_selfhosted_passes_gate(
    database_url, chain, gl_scope, gitlab_binding
):
    sync_id = seed.git_issue_sync(
        database_url,
        chain,
        gl_scope,
        binding_id=gitlab_binding["binding_id"],
        issue_id=str(chain["issue"]["id"]),
        external_iid="424242",
        metadata={},
        provider="gitlab",
    )
    _publish_and_drain(POST_COMMENT, [sync_id])

    row = db.wait_for(
        database_url,
        "SELECT metadata FROM git_issue_syncs WHERE id = %s"
        " AND metadata ? 'completion_comment_error'",
        (sync_id,),
    )
    assert row is not None, "expected completion_comment_error after DNS failure"
    error = row["metadata"]["completion_comment_error"]
    assert "not allowed" not in error, error
    assert "must use HTTPS" not in error, error
    assert "gitlab-allowed.invalid" in error, error
    assert error != ""
    assert "completion_comment_id" not in row["metadata"]
