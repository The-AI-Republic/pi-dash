"""Worker path: scan / fire / dispatch through a live Celery worker.

The suite publishes ``pi_dash.bgtasks.loop.scan_due_targets`` and
``fire_loop_target`` in Celery wire format to a dedicated queue
(``WORKER_QUEUE``, default ``ct17``) that a live worker consumes, then diffs
the database before/after:

- scan fans out a ``fire_loop_target`` message per eligible due target
  (asserted on the broker: task name + args), advances ineligible cursors
  with deterministic skip reasons, and reconciles missing edge targets;
- fire claims one target, advances its cursor, and dispatches a hidden loop
  thread + queued turn + prompt message (asserted in Postgres);
- the downstream ``assistant.run_turn`` message is asserted on the broker
  (task name + turn-id arg) and never executes: nothing consumes the default
  ``celery`` queue, and every test re-checks the turn is still queued.

``LOOP_RECONCILE_EVERY_MINUTES=1`` in the backend env keeps reconcile
deterministic (every tick instead of once per 15 wall-clock minutes).
"""

from . import seed_loop as seed_l
from .conftest import (
    FIRE_TASK,
    RUN_TURN_TASK,
    SCAN_TASK,
    poll,
    publish,
    purge_queue,
    queued_tasks,
)

def due_job(org, suffix=""):
    return seed_l.create_loop_job(org.conn, slug=f"wj-{org.tag}{suffix}")


def eligible_user(org, kind):
    """A workspace member with LLM credentials (passes every predicate)."""
    user = _new_member(org, kind)
    seed_l.configure_llm(org.conn, user_id=user["id"])
    return user


def _new_member(org, kind):
    from _harness import seed, sessions

    email = f"loop-w-{kind}-{org.tag}@ct.example.com"
    user = seed.create_user(
        org.conn, email=email, username=email,
        password_field=sessions.make_password_hash(f"pw-{org.tag}-{kind}"),
    )
    seed.add_workspace_member(
        org.conn, workspace_id=org.workspace["id"], user_id=user["id"], role=seed.MEMBER
    )
    return user


def ineligible_user(org, kind):
    """A workspace member with no LLM config (fails exactly one predicate)."""
    return _new_member(org, kind)


def due_target(org, job, user):
    return seed_l.create_target(
        org.conn, job_id=job["id"], workspace_id=org.workspace["id"],
        user_id=user["id"], next_run_at=seed_l.hours_ago(1),
    )


def fire_message_for(target_id):
    msgs = queued_tasks("celery", FIRE_TASK)
    return [m for m in msgs if (m["body"] or [[None]])[0] == [target_id]]


def test_scan_fans_out_only_eligible(org):
    purge_queue("celery")
    # Eligible-due rows from earlier runs never drain (their fires sit
    # unconsumed), so reschedule every stale due cursor: this scan then fans
    # out only the rows below, and the broker window stays tiny.
    org.conn.execute(
        "UPDATE loop_targets SET next_run_at=%s"
        " WHERE next_run_at IS NULL OR next_run_at <= now()",
        (seed_l.hours_from_now(2),),
    )
    job = due_job(org)
    good = eligible_user(org, "scan-good")
    bad = ineligible_user(org, "scan-bad")
    t_good = due_target(org, job, good)
    t_bad = due_target(org, job, bad)

    publish(SCAN_TASK)

    # The scan reconciles before fanning out (~a minute on a big database),
    # so the deadline must cover its end-to-end latency, not just the fan-out.
    found = poll(lambda: fire_message_for(t_good["id"]), timeout=180.0)
    assert found, "no fire_loop_target message for the eligible target"
    assert found[0]["headers"]["task"] == FIRE_TASK
    # The scan itself never touches the eligible row: the fire it queued sits
    # on the unconsumed default queue.
    row = org.conn.execute(
        "SELECT last_run_id, next_run_at FROM loop_targets WHERE id=%s",
        (t_good["id"],),
    ).fetchone()
    assert row[0] is None

    # The ineligible target is advanced in the same tick with its reason.
    def advanced():
        r = org.conn.execute(
            "SELECT last_skip_reason, next_run_at FROM loop_targets WHERE id=%s",
            (t_bad["id"],),
        ).fetchone()
        return r if r[0] else None

    reason, cursor = poll(advanced, timeout=60.0)
    assert reason == "llm_config_missing"
    assert str(cursor) > seed_l.hours_ago(0.5)


def test_scan_reconciles_missing_edge(org):
    # A member edge with no target row gets one, scheduled in the future (no
    # immediate burst); repeat scans never duplicate it.
    job = due_job(org, suffix="-rec")
    user = ineligible_user(org, "scan-rec")
    publish(SCAN_TASK)

    def created():
        return org.conn.execute(
            """SELECT id, next_run_at FROM loop_targets
            WHERE job_id=%s AND workspace_id=%s AND user_id=%s
            AND deleted_at IS NULL""",
            (job["id"], org.workspace["id"], user["id"]),
        ).fetchone()

    row = poll(created, timeout=180.0)
    assert row is not None
    assert str(row[1]) > seed_l.hours_ago(0.5)
    before = org.conn.execute(
        "SELECT count(*) FROM loop_targets WHERE job_id=%s AND deleted_at IS NULL",
        (job["id"],),
    ).fetchone()[0]
    publish(SCAN_TASK)
    poll(
        lambda: org.conn.execute(
            "SELECT last_skipped_at FROM loop_targets WHERE id=%s", (row[0],)
        ).fetchone()[0] is not None,
        timeout=180.0,
    )
    after = org.conn.execute(
        "SELECT count(*) FROM loop_targets WHERE job_id=%s AND deleted_at IS NULL",
        (job["id"],),
    ).fetchone()[0]
    assert after == before


def test_fire_happy_path_creates_hidden_turn(org):
    job = seed_l.create_loop_job(
        org.conn, slug=f"fh-{org.tag}", public_name="Fire Happy",
        prompt="DO THE THING",
    )
    user = eligible_user(org, "fire-good")
    target = due_target(org, job, user)
    purge_queue("celery")
    before_cursor = org.conn.execute(
        "SELECT next_run_at FROM loop_targets WHERE id=%s", (target["id"],)
    ).fetchone()[0]

    publish(FIRE_TASK, [target["id"]])

    def fired():
        r = org.conn.execute(
            "SELECT last_run_id, thread_id, last_skip_reason, next_run_at"
            " FROM loop_targets WHERE id=%s",
            (target["id"],),
        ).fetchone()
        return r if r[0] else None

    last_run_id, thread_id, skip_reason, cursor = poll(fired, timeout=60.0)
    assert skip_reason == ""
    assert str(cursor) > str(before_cursor)

    thread = org.conn.execute(
        "SELECT kind, title, active_turn_id, is_archived FROM assistant_thread WHERE id=%s",
        (str(thread_id),),
    ).fetchone()
    assert thread[0] == "loop"
    assert thread[1] == "Fire Happy"
    assert str(thread[2]) == str(last_run_id)
    assert thread[3] is False

    turn = org.conn.execute(
        "SELECT status, user_message_id FROM assistant_turn WHERE id=%s",
        (str(last_run_id),),
    ).fetchone()
    assert turn[0] == "queued"
    msg = org.conn.execute(
        "SELECT kind, display_content, status FROM assistant_message WHERE id=%s",
        (str(turn[1]),),
    ).fetchone()
    assert msg == ("user", "DO THE THING", "completed")

    # The downstream run is queued in Celery wire format with the turn id...
    def run_queued():
        msgs = queued_tasks("celery", RUN_TURN_TASK)
        return [m for m in msgs if (m["body"] or [[None]])[0] == [str(last_run_id)]]

    assert poll(run_queued, timeout=30.0), "no assistant.run_turn message queued"

    # ...and never executes: nothing consumes the default queue.
    import time as _time

    _time.sleep(5)
    assert org.conn.execute(
        "SELECT status FROM assistant_turn WHERE id=%s", (str(last_run_id),)
    ).fetchone()[0] == "queued"


def test_fire_ineligible_advances_without_turn(org):
    job = due_job(org, suffix="-inelig")
    user = ineligible_user(org, "fire-bad")
    target = due_target(org, job, user)
    turns_before = org.conn.execute("SELECT count(*) FROM assistant_turn").fetchone()[0]

    publish(FIRE_TASK, [target["id"]])

    def skipped():
        r = org.conn.execute(
            "SELECT last_skip_reason, next_run_at, last_run_id FROM loop_targets"
            " WHERE id=%s",
            (target["id"],),
        ).fetchone()
        return r if r[0] else None

    reason, cursor, last_run = poll(skipped, timeout=60.0)
    assert reason == "llm_config_missing"
    assert str(cursor) > seed_l.hours_ago(0.5)
    assert last_run is None
    # No thread, no turn, and no downstream run was queued for this target.
    assert org.conn.execute(
        "SELECT thread_id FROM loop_targets WHERE id=%s", (target["id"],)
    ).fetchone()[0] is None
    assert org.conn.execute("SELECT count(*) FROM assistant_turn").fetchone()[0] == turns_before


def test_fire_future_cursor_noop(org):
    job = due_job(org, suffix="-future")
    user = eligible_user(org, "fire-future")
    target = seed_l.create_target(
        org.conn, job_id=job["id"], workspace_id=org.workspace["id"],
        user_id=user["id"], next_run_at=seed_l.hours_from_now(1),
    )
    before = org.conn.execute(
        "SELECT next_run_at, last_skip_reason, last_run_id, thread_id FROM loop_targets"
        " WHERE id=%s",
        (target["id"],),
    ).fetchone()

    purge_queue("celery")
    publish(FIRE_TASK, [target["id"]])

    import time as _time

    _time.sleep(10)
    after = org.conn.execute(
        "SELECT next_run_at, last_skip_reason, last_run_id, thread_id FROM loop_targets"
        " WHERE id=%s",
        (target["id"],),
    ).fetchone()
    assert after == before
    assert not fire_message_for(target["id"])


def test_fire_skips_when_turn_active(org):
    job = due_job(org, suffix="-active")
    user = eligible_user(org, "fire-active")
    thread = seed_l.create_thread(
        org.conn, workspace_id=org.workspace["id"], user_id=user["id"]
    )
    running = seed_l.create_turn(org.conn, thread_id=thread["id"], status="running")
    org.conn.execute(
        "UPDATE assistant_thread SET active_turn_id=%s WHERE id=%s",
        (running["id"], thread["id"]),
    )
    target = due_target(org, job, user)
    org.conn.execute(
        "UPDATE loop_targets SET thread_id=%s WHERE id=%s", (thread["id"], target["id"])
    )

    publish(FIRE_TASK, [target["id"]])

    def skipped():
        r = org.conn.execute(
            "SELECT last_skip_reason FROM loop_targets WHERE id=%s", (target["id"],)
        ).fetchone()
        return r if r[0] else None

    assert poll(skipped, timeout=60.0)[0] == "turn_active"
    assert org.conn.execute(
        "SELECT count(*) FROM assistant_turn WHERE thread_id=%s", (thread["id"],)
    ).fetchone()[0] == 1


def test_fire_rotates_full_thread(org):
    job = due_job(org, suffix="-rotate")
    user = eligible_user(org, "fire-rotate")
    thread = seed_l.create_thread(
        org.conn, workspace_id=org.workspace["id"], user_id=user["id"]
    )
    # MAX_THREAD_MESSAGES (200) minus LOOP_ROTATION_HEADROOM (30) is the
    # rotation threshold: one message past it forces a fresh thread.
    for i in range(171):
        seed_l.create_message(org.conn, thread_id=thread["id"], seq=i)
    target = due_target(org, job, user)
    org.conn.execute(
        "UPDATE loop_targets SET thread_id=%s WHERE id=%s", (thread["id"], target["id"])
    )

    publish(FIRE_TASK, [target["id"]])

    def rotated():
        r = org.conn.execute(
            "SELECT thread_id, last_run_id FROM loop_targets WHERE id=%s",
            (target["id"],),
        ).fetchone()
        return r if r[0] and str(r[0]) != thread["id"] else None

    fresh_id, turn_id = poll(rotated, timeout=60.0)
    old = org.conn.execute(
        "SELECT is_archived FROM assistant_thread WHERE id=%s", (thread["id"],)
    ).fetchone()
    assert old[0] is True
    fresh = org.conn.execute(
        "SELECT kind, active_turn_id FROM assistant_thread WHERE id=%s",
        (str(fresh_id),),
    ).fetchone()
    assert fresh[0] == "loop"
    assert str(fresh[1]) == str(turn_id)
