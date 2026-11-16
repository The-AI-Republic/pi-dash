"""Daemon self-delete (``DELETE /api/v1/runner/runners/<rid>/``) is all-or-nothing.

Django runs ``Runner.objects.filter(pk=…).delete()`` — the whole collector
cascade — inside one transaction (``Collector.delete`` wraps every
cascade/SET_NULL/delete statement in ``transaction.atomic``, Django 4.2
``db/models/deletion.py``), so a failure after the first statement rolls the
lot back.

Rust used to run the same sequence as ~ten autocommit statements on the pool
(``runner_enroll/teardown.rs`` ``delete_collected_runner``). When the final
runner-row delete failed, the earlier statements were already committed: chat
events/approvals/messages/sessions gone and agent runs detached while the
runner row survived. That is data loss Django's single transaction rules out.

This test forces the last statement (``DELETE FROM runner``) to fail with a
scratch-DB ``BEFORE DELETE`` trigger created and dropped by the test, calls the
daemon self-delete with the runner's own token, and asserts the whole cascade is
still intact and the response is 500 on both backends.
"""

from __future__ import annotations

import uuid

import pytest

from .conftest import DAEMON

pytestmark = pytest.mark.contract


def _install_blocking_trigger(db, tag: str) -> str:
    """A BEFORE DELETE trigger on ``runner`` that raises, created per test.

    Returns the function name so the test can drop both objects afterwards.
    """
    fn = f"ct_selfdel_{tag}"
    db.execute(
        f"CREATE OR REPLACE FUNCTION {fn}() RETURNS trigger AS $$ "
        f"BEGIN RAISE EXCEPTION 'contract: forced runner delete failure'; "
        f"END $$ LANGUAGE plpgsql"
    )
    db.execute(
        f"CREATE TRIGGER {fn} BEFORE DELETE ON runner "
        f"FOR EACH ROW EXECUTE FUNCTION {fn}()"
    )
    return fn


def _drop_blocking_trigger(db, fn: str) -> None:
    db.execute(f"DROP TRIGGER IF EXISTS {fn} ON runner")
    db.execute(f"DROP FUNCTION IF EXISTS {fn}()")


def test_daemon_self_delete_rolls_back_on_runner_delete_failure(
    settings, seeder, enrolled, runner_client, daemon_world
):
    rid = enrolled["runner_id"]
    workspace_id = daemon_world["workspace"]["id"]
    pod_id = daemon_world["pod"]["id"]
    owner_id = daemon_world["owner"]["id"]

    issue = seeder.create_issue(workspace_id, daemon_world["project"]["id"])
    session = seeder.create_chat_session(workspace_id, rid, pod_id, owner_id)
    message = seeder.create_chat_message(session["id"], role="user", content="keep me")
    run = seeder.create_agent_run(workspace_id, owner_id, pod_id, issue["id"])
    seeder.db.execute(
        "UPDATE agent_run SET runner_id = %s WHERE id = %s", (rid, run["id"])
    )

    runner_session_id = str(uuid.uuid4())
    seeder.db.execute(
        "INSERT INTO runner_session "
        "(id, protocol_version, created_at, last_seen_at, revoked_at, revoked_reason, runner_id) "
        "VALUES (%s, 1, now(), now(), NULL, '', %s)",
        (runner_session_id, rid),
    )
    seeder.db.execute(
        "INSERT INTO agent_chat_event "
        "(seq, source_key, kind, payload, created_at, message_id, session_id) "
        "VALUES (1, %s, 'raw', '{}'::jsonb, now(), %s, %s)",
        (f"evt-{seeder.tag}", message["id"], session["id"]),
    )
    approval_id = str(uuid.uuid4())
    seeder.db.execute(
        "INSERT INTO agent_chat_approval "
        "(id, local_approval_id, kind, payload, reason, status, decision_source, requested_at, session_id) "
        "VALUES (%s, %s, 'command', '{}'::jsonb, '', 'pending', 'agent', now(), %s)",
        (approval_id, f"appr-{seeder.tag}", session["id"]),
    )

    fn = _install_blocking_trigger(seeder.db, seeder.tag)
    try:
        response = runner_client.delete(f"{DAEMON}/runners/{rid}/")

        assert response.status_code == 500, response.text

        # All-or-nothing: the runner and its cascade are untouched.
        assert (
            seeder.db.fetchval("SELECT COUNT(*) FROM runner WHERE id = %s", (rid,))
            == 1
        )
        assert (
            seeder.db.fetchval(
                "SELECT COUNT(*) FROM agent_chat_session WHERE id = %s",
                (session["id"],),
            )
            == 1
        )
        assert (
            seeder.db.fetchval(
                "SELECT COUNT(*) FROM agent_chat_message WHERE id = %s",
                (message["id"],),
            )
            == 1
        )
        assert (
            seeder.db.fetchval(
                "SELECT COUNT(*) FROM agent_chat_event WHERE session_id = %s",
                (session["id"],),
            )
            == 1
        )
        assert (
            seeder.db.fetchval(
                "SELECT COUNT(*) FROM agent_chat_approval WHERE id = %s",
                (approval_id,),
            )
            == 1
        )
        assert (
            seeder.db.fetchval(
                "SELECT COUNT(*) FROM runner_session WHERE id = %s",
                (runner_session_id,),
            )
            == 1
        )
        assert str(
            seeder.db.fetchval(
                "SELECT runner_id FROM agent_run WHERE id = %s", (run["id"],)
            )
        ) == str(rid)
    finally:
        _drop_blocking_trigger(seeder.db, fn)
        seeder.db.execute(
            "DELETE FROM agent_chat_event WHERE session_id = %s", (session["id"],)
        )
        seeder.db.execute(
            "DELETE FROM agent_chat_approval WHERE session_id = %s", (session["id"],)
        )
        seeder.db.execute(
            "DELETE FROM runner_session WHERE id = %s", (runner_session_id,)
        )
