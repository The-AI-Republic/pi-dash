"""Concurrent first-time toggles on the auto-pm preference upsert.

Django's ``QuerySet.update_or_create`` is atomic: it takes a
``select_for_update()`` on the lookup, creates inside a savepoint, and on
``IntegrityError`` re-reads the row the other writer just created and updates
it. Two concurrent first toggles therefore both answer 200 and leave exactly
one live row. The Rust port originally did a bare SELECT followed by INSERT,
so the loser of the race on ``loop_pref_unique_user_*_when_active`` got a 500.

Each round seeds a brand-new user (the race only exists while no live
preference row exists) and fires the two opposite toggles after a barrier so
their handlers overlap.
"""

from __future__ import annotations

import threading
import uuid
from concurrent.futures import ThreadPoolExecutor

from _harness import http, seed, sessions

from . import seed_loop as seed_l

SETTINGS = "/api/users/me/auto-pm/"
ROUNDS = 24


def _new_user_client(conn, tag: str):
    user = seed.create_user(
        conn,
        email=f"race-{tag}@ct.example.com",
        username=f"race-{tag}",
        password_field=sessions.make_password_hash(f"pw-{tag}"),
    )
    jar = sessions.login(conn, user["id"], user["password_field"])
    return user, jar


def _fire_together(jar: dict, url: str) -> list:
    """Two PATCHes released at the same instant, each on its own client."""
    barrier = threading.Barrier(2)

    def fire(enabled: bool):
        client = http.make_client(jar)
        try:
            barrier.wait(timeout=10)
            return client.patch(url, json={"enabled": enabled})
        finally:
            client.close()

    with ThreadPoolExecutor(max_workers=2) as pool:
        return list(pool.map(fire, (False, True)))


def _assert_all_200(responses):
    for r in responses:
        assert r.status_code == 200, r.text


def test_concurrent_first_master_toggle(org):
    for i in range(ROUNDS):
        tag = f"{org.tag}-m{i}"
        user, jar = _new_user_client(org.conn, tag)
        responses = _fire_together(jar, SETTINGS)
        _assert_all_200(responses)
        n = org.conn.execute(
            "SELECT count(*) FROM loop_user_preferences"
            " WHERE user_id=%s AND job_id IS NULL AND deleted_at IS NULL",
            (user["id"],),
        ).fetchone()[0]
        assert n == 1, f"round {i}: {n} live master rows"


def test_concurrent_first_job_toggle(org):
    job = seed_l.create_loop_job(
        org.conn, slug=f"race-{org.tag}", public_name="Race Job"
    )
    url = f"{SETTINGS}jobs/{job['slug']}/"
    for i in range(ROUNDS):
        tag = f"{org.tag}-j{i}"
        user, jar = _new_user_client(org.conn, tag)
        responses = _fire_together(jar, url)
        _assert_all_200(responses)
        n = org.conn.execute(
            "SELECT count(*) FROM loop_user_preferences"
            " WHERE user_id=%s AND job_id=%s AND deleted_at IS NULL",
            (user["id"], job["id"]),
        ).fetchone()[0]
        assert n == 1, f"round {i}: {n} live job rows"
