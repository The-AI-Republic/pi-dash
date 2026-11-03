# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""PIDASHCONV-1218 Rust replay: a state created after boot still ticks.

Same scenario and assertions as the Django oracle
(`tasks_ticker/test_ticker_live_state.py`), executed against the Rust worker.
The Django worker consumes Celery frames from `CELERY_BROKER_URL`; the Rust
worker (`pidash-api worker`) claims rows from `rust_job_queue` instead, so
this module seeds that queue with the identical Celery v2 payload (see
`_harness/rust_queue.py`) and waits on the same Postgres row.

Run with the Django worker STOPPED and the Rust worker running against the
same `DATABASE_URL`::

    export DATABASE_URL=postgresql://...   # your own scratch database
    pidash-api worker &                    # Rust worker; Django worker stopped
    pytest -p no:django tasks_ticker/test_rust_replay.py

Before the fix the Rust worker resolved ticking states from a map loaded once
at boot (`live_inputs::load_states`), so the state seeded here — created after
the worker started — was unknown and `fire_tick` returned without writing.
The claim fingerprint (`last_tick_at`) stays NULL, so this test fails on
rust-dev before the fix and passes after it, on both backends.
"""

from _harness import rust_queue

from . import _live_state

FIRE = _live_state.FIRE


def test_fire_tick_claims_state_created_after_boot(db_conn):
    """A due ticker on an In Progress issue created after boot is claimed."""
    _w, issue = _live_state.seed_state_after_boot("rust")
    ticker_id = str(_live_state.ticker(issue)["id"])

    rust_queue.publish(FIRE, args=[ticker_id])

    row = _live_state.wait_for_claim(issue)
    assert row["last_tick_at"] is not None
    assert row["used"] == 0
    assert _live_state.run_count(issue) == 1  # only the seeded prior run
