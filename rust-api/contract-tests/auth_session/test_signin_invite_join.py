# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""POST /auth/sign-in/ with accepted invites — post-auth write sequence.

PIDASHCONV-1075: the email sign-in tail must replay Django's statement
order — the project arm as two bulk statements (all ``workspace_members``
rows committed before the ported ``project_id`` IntegrityError), the
``last_login`` stamp after the invitation workflow, invite processing in
``-created_at`` order, and the activation mail published before the
``is_active`` save.

Run like the rest of the suite (see ``conftest.py``), plus the broker
URLs (``AMQP_URL`` redis, ``RABBITMQ_*``) the Django server publishes
``.delay()`` side effects to. PIDASHCONV-1108 removed the Rust AMQP
publisher, so the Rust server enqueues the same side effects onto the
``rust_job_queue`` Postgres table instead of publishing to a broker.
Each publish test below collects from both planes — the broker drains
(Django) and the queue rows (Rust, id order) — and asserts on the
union; exactly one plane fires per backend run.
"""

import os
import time
import uuid

import pytest

TRACK_EVENT_TASK = "pi_dash.bgtasks.event_tracking_task.track_event"
ACTIVATION_TASK = "pi_dash.bgtasks.user_activation_email_task.user_activation_email"


def _redis_url() -> str:
    try:
        return os.environ["AMQP_URL"]
    except KeyError:
        raise AssertionError("AMQP_URL is required (redis broker the Django server publishes to)")


def _rabbit_params():
    import pika

    return pika.ConnectionParameters(
        host=os.environ.get("RABBITMQ_HOST", "localhost"),
        port=int(os.environ.get("RABBITMQ_PORT", "5672")),
        virtual_host=os.environ.get("RABBITMQ_VHOST", "/"),
        credentials=pika.PlainCredentials(
            os.environ.get("RABBITMQ_USER", "guest"),
            os.environ.get("RABBITMQ_PASSWORD", "guest"),
        ),
        socket_timeout=5,
    )


def _drain_redis() -> list:
    """Consume every pending ``celery`` queue entry on the redis broker."""
    from kombu import Connection, Exchange, Queue

    out = []
    queue = Queue("celery", exchange=Exchange("celery", "direct", durable=True), routing_key="celery")
    with Connection(_redis_url()) as conn:
        simple = conn.SimpleQueue(queue)
        try:
            while True:
                try:
                    message = simple.get_nowait()
                except Exception:
                    # Drained (an empty/non-declared queue raises) — same
                    # shape as `_harness.broker_redis.purge`.
                    break
                try:
                    body = message.decode()
                    headers = message.headers or {}
                    out.append((headers.get("task"), body))
                finally:
                    try:
                        message.ack()
                    except Exception:
                        pass
        finally:
            simple.close()
    return out


def _drain_rabbit() -> list:
    """Consume every pending ``celery`` queue entry on the AMQP broker.

    An unreachable broker yields no messages instead of raising: neither
    backend publishes here (Django publishes to redis; since
    PIDASHCONV-1108 the Rust server enqueues to Postgres instead of
    publishing to AMQP), so a missing RabbitMQ must not fail either side.
    """
    import json

    import pika

    out = []
    try:
        connection = pika.BlockingConnection(_rabbit_params())
    except Exception:
        return out
    try:
        channel = connection.channel()
        try:
            # Non-passive declare (same durable shape every Celery
            # publisher uses): a missing queue simply starts empty.
            channel.queue_declare(queue="celery", durable=True, passive=False)
        except Exception:
            return out
        while True:
            method, properties, body = channel.basic_get("celery", auto_ack=True)
            if method is None:
                break
            headers = (properties.headers or {}) if properties else {}
            try:
                payload = json.loads(body)
            except ValueError:
                continue
            out.append((headers.get("task"), payload))
    finally:
        try:
            connection.close()
        except Exception:
            pass
    return out


def _purge_brokers() -> None:
    _drain_redis()
    _drain_rabbit()


def _drain_track_events(timeout: float = 10.0) -> list:
    """Poll both brokers until quiet; return ``track_event`` kwargs in arrival order."""
    deadline = time.time() + timeout
    seen: list = []
    idle_since = time.time()
    while time.time() - idle_since < 2.0 and time.time() < deadline:
        batch = _drain_redis() + _drain_rabbit()
        fresh = [body for task, body in batch if task == TRACK_EVENT_TASK]
        if fresh:
            seen.extend(fresh)
            idle_since = time.time()
        else:
            time.sleep(0.2)
    return [kwargs for _args, kwargs, _embed in seen]


def _drain_task_messages(task: str, timeout: float = 10.0) -> list:
    """Poll both brokers until quiet; return ``(args, kwargs)`` for one task."""
    deadline = time.time() + timeout
    seen: list = []
    idle_since = time.time()
    while time.time() - idle_since < 2.0 and time.time() < deadline:
        batch = _drain_redis() + _drain_rabbit()
        fresh = [body for t, body in batch if t == task]
        if fresh:
            seen.extend(fresh)
            idle_since = time.time()
        else:
            time.sleep(0.2)
    return [(args, kwargs) for args, kwargs, _embed in seen]


def _queue_rows(db, task: str, where: str = "", params: tuple = ()) -> list:
    """``(args, kwargs)`` rows the Rust plane enqueued for ``task`` (id order).

    The PIDASHCONV-1108 D3 pattern: the Rust server enqueues ``.delay()``
    side effects onto ``rust_job_queue`` instead of publishing to a
    broker. The Django plane never writes this table (on a Django-only
    run it may not exist at all), so a missing table yields no rows.
    ``auth_session`` is not a worker suite, so no worker drains the rows
    mid-run.
    """
    with db.connect() as conn:
        with conn.cursor() as cur:
            cur.execute("SELECT to_regclass('public.rust_job_queue') AS t")
            if cur.fetchone()[0] is None:
                return []
            cur.execute(
                "SELECT args, kwargs FROM rust_job_queue"
                f" WHERE task = %s{where} ORDER BY id",
                (task, *params),
            )
            return cur.fetchall()


def _queue_track_event_kwargs(db, *slugs: str) -> list:
    """``track_event`` kwargs rows the Rust plane enqueued (id order).

    Scoped to this test's workspace slugs (unique per test), so rows
    from other tests in the same run never leak in.
    """
    return [
        kwargs
        for _args, kwargs in _queue_rows(
            db, TRACK_EVENT_TASK, " AND kwargs ->> 'slug' = ANY(%s)", (list(slugs),)
        )
    ]


def _queue_task_calls(db, task: str, user_id: str) -> list:
    """``(args, kwargs)`` rows the Rust plane enqueued for ``task`` (id order).

    Scoped to this test's user id (``args[1]``, unique per test).
    """
    return _queue_rows(db, task, " AND args ->> 1 = %s", (user_id,))


@pytest.fixture()
def invite_world(db, user):
    """Two workspaces + projects owned by the user; all rows cleaned after.

    ``Database.reset()`` wipes the workspace tables before every test but
    knows nothing of the project tables, and the suite teardown deletes
    the ``as-`` users while our workspaces still reference them — so every
    seeded row is deleted here, project tables and workspace tables alike.
    """
    owner_id = user["id"]
    tag = uuid.uuid4().hex[:8]
    workspaces = [
        db.make_workspace(f"as-ws-{tag}-old", f"as-ws-{tag}-old", owner_id),
        db.make_workspace(f"as-ws-{tag}-new", f"as-ws-{tag}-new", owner_id),
    ]
    projects = [db.make_project(w["id"], f"as{tag}{i}") for i, w in enumerate(workspaces)]
    world = {"workspaces": workspaces, "projects": projects, "tag": tag}
    yield world
    ws_ids = [w["id"] for w in workspaces]
    with db.connect() as conn:
        with conn.cursor() as cur:
            cur.execute(
                "DELETE FROM project_member_invites WHERE workspace_id = ANY(%s)", (ws_ids,)
            )
            cur.execute("DELETE FROM project_members WHERE workspace_id = ANY(%s)", (ws_ids,))
            cur.execute(
                "DELETE FROM projects WHERE id = ANY(%s)", ([p["id"] for p in projects],)
            )
            cur.execute(
                "DELETE FROM workspace_member_invites WHERE workspace_id = ANY(%s)", (ws_ids,)
            )
            cur.execute("DELETE FROM workspace_members WHERE workspace_id = ANY(%s)", (ws_ids,))
            cur.execute("DELETE FROM workspaces WHERE id = ANY(%s)", (ws_ids,))
        conn.commit()


def _seed_project_invite(db, workspace_id, project_id, email, *, role, created_days_ago):
    iid = str(uuid.uuid4())
    with db.connect() as conn:
        with conn.cursor() as cur:
            cur.execute(
                "INSERT INTO project_member_invites (id, created_at, updated_at,"
                " project_id, workspace_id, email, accepted, token, role)"
                " VALUES (%s, now() - (%s || ' days')::interval, now(),"
                " %s, %s, %s, true, %s, %s)",
                (iid, created_days_ago, project_id, workspace_id, email, f"tok-{iid[:8]}", role),
            )
        conn.commit()
    return iid


def _seed_workspace_invite(db, workspace_id, email, *, role, created_days_ago):
    iid = str(uuid.uuid4())
    with db.connect() as conn:
        with conn.cursor() as cur:
            cur.execute(
                "INSERT INTO workspace_member_invites (id, created_at, updated_at,"
                " workspace_id, email, accepted, token, role)"
                " VALUES (%s, now() - (%s || ' days')::interval, now(),"
                " %s, %s, true, %s, %s)",
                (iid, created_days_ago, workspace_id, email, f"tok-{iid[:8]}", role),
            )
        conn.commit()
    return iid


def test_project_invites_commit_all_workspace_rows_before_ported_500(
    browser, db, user, invite_world
):
    """Two accepted project invites: 500 on both, N rows kept, no stamp.

    Django commits the whole project-arm ``workspace_members`` bulk before
    the ``project_members`` bulk raises the ported IntegrityError, and the
    ``last_login`` stamp (fired from ``login()`` after ``authenticate()``)
    never runs. ``save_user_data`` still runs first, so ``last_active``
    is stamped while ``last_login`` stays NULL.
    """
    email = user["email"]
    old_ws, new_ws = invite_world["workspaces"]
    old_proj, new_proj = invite_world["projects"]
    # Distinct roles to pin the `in [5, 15] else 15` mapping per row.
    _seed_project_invite(db, old_ws["id"], old_proj["id"], email, role=20, created_days_ago=2)
    _seed_project_invite(db, new_ws["id"], new_proj["id"], email, role=5, created_days_ago=1)

    res = browser().post("/auth/sign-in/", data={"email": email, "password": user["password"]})
    assert res.status_code == 500, res.text[:300]

    with db.connect() as conn:
        with conn.cursor() as cur:
            cur.execute(
                "SELECT workspace_id::text, role FROM workspace_members"
                " WHERE member_id = %s ORDER BY workspace_id::text",
                (user["id"],),
            )
            members = cur.fetchall()
            cur.execute("SELECT COUNT(*) FROM project_members WHERE member_id = %s", (user["id"],))
            project_rows = cur.fetchone()[0]
            cur.execute(
                "SELECT last_login, last_active FROM users WHERE id = %s", (user["id"],)
            )
            last_login, last_active = cur.fetchone()
            cur.execute(
                "SELECT COUNT(*) FROM project_member_invites WHERE email = %s AND accepted",
                (email,),
            )
            invites_left = cur.fetchone()[0]
    assert sorted((w, r) for w, r in members) == sorted(
        [(old_ws["id"], 15), (new_ws["id"], 5)]
    ), members
    assert project_rows == 0
    assert last_login is None
    assert last_active is not None
    # The invite deletes never run on the 500 path.
    assert invites_left == 2


def test_workspace_invites_publish_track_event_newest_first(browser, db, user, invite_world):
    """Two accepted workspace invites: 302 on both, newest ``track_event`` first.

    Both invite querysets iterate in model Meta ordering (``-created_at``),
    so the two ``track_event.delay`` publishes land newest-first. The older
    invite is seeded first so an unordered scan would emit it first and
    fail the assertion.
    """
    _purge_brokers()
    email = user["email"]
    old_ws, new_ws = invite_world["workspaces"]
    _seed_workspace_invite(db, old_ws["id"], email, role=15, created_days_ago=2)
    _seed_workspace_invite(db, new_ws["id"], email, role=20, created_days_ago=1)

    res = browser().post("/auth/sign-in/", data={"email": email, "password": user["password"]})
    assert res.status_code == 302, res.text[:300]

    track_events = _drain_track_events() + _queue_track_event_kwargs(
        db, new_ws["slug"], old_ws["slug"]
    )
    assert [kw.get("slug") for kw in track_events] == [new_ws["slug"], old_ws["slug"]]

    with db.connect() as conn:
        with conn.cursor() as cur:
            cur.execute(
                "SELECT workspace_id::text, role FROM workspace_members"
                " WHERE member_id = %s ORDER BY workspace_id::text",
                (user["id"],),
            )
            members = cur.fetchall()
            cur.execute(
                "SELECT COUNT(*) FROM workspace_member_invites"
                " WHERE email = %s AND accepted AND deleted_at IS NULL",
                (email,),
            )
            invites_live = cur.fetchone()[0]
            cur.execute("SELECT last_login FROM users WHERE id = %s", (user["id"],))
            last_login = cur.fetchone()[0]
    assert sorted((w, r) for w, r in members) == sorted(
        [(old_ws["id"], 15), (new_ws["id"], 20)]
    ), members
    # No live accepted invite remains on either backend. (Django
    # soft-deletes the rows while Rust hard-deletes them -- a hardness
    # divergence outside this issue's write-sequence scope.)
    assert invites_live == 0
    assert last_login is not None


def test_inactive_user_signin_publishes_activation_and_succeeds(browser, db, user):
    """Inactive user: the activation mail goes out, then the user activates.

    Pins the success path of the activation branch while the publish sits
    ahead of the ``is_active`` save (``base.py:230`` before ``:232-233``).
    """
    _purge_brokers()
    with db.connect() as conn:
        with conn.cursor() as cur:
            cur.execute("UPDATE users SET is_active = false WHERE id = %s", (user["id"],))
        conn.commit()

    res = browser().post(
        "/auth/sign-in/", data={"email": user["email"], "password": user["password"]}
    )
    assert res.status_code == 302, res.text[:300]

    activations = _drain_task_messages(ACTIVATION_TASK) + _queue_task_calls(
        db, ACTIVATION_TASK, user["id"]
    )
    # Kombu decodes the user-id arg back to UUID; the Rust queue row
    # carries the plain string -- compare stringified on both.
    assert [str(args[1]) for args, _kwargs in activations] == [user["id"]]

    with db.connect() as conn:
        with conn.cursor() as cur:
            cur.execute("SELECT is_active, last_login FROM users WHERE id = %s", (user["id"],))
            is_active, last_login = cur.fetchone()
    assert is_active is True
    assert last_login is not None
