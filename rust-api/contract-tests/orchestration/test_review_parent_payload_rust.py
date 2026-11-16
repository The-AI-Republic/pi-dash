# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""PIDASHCONV-1232 Rust replay: review continuation parent payload.

Same scenario and assertions as the Django oracle
(``orchestration/test_review_parent_payload.py``), executed against the Rust
worker. The Django worker consumes Celery frames from ``CELERY_BROKER_URL``;
the Rust worker (``pidash-api worker``) claims rows from ``rust_job_queue``
instead, so this module seeds that queue with the identical Celery v2
payload (see ``_harness/rust_queue.py``) and waits on the same Postgres row.

Run with the Django worker STOPPED and the Rust worker running against the
same ``DATABASE_URL``::

    export DATABASE_URL=postgresql://...   # your own scratch database
    pidash-api worker &                    # Rust worker; Django worker stopped
    pytest -p no:django orchestration/test_review_parent_payload_rust.py

Before the fix ``resolve_parent_payload`` falls through to the ticker's
resume-parent payload whenever the direct payload is missing or falsy, so
R3's prompt carries R1's stale payload and this test fails. After the fix
the ticker is consulted only when R3 has no parent run, and R3's prompt
carries the fallback line — on both backends.
"""

from _harness import rust_queue

from . import _review_parent_payload as scenario

FIRE = scenario.FIRE


def test_tick_parents_r3_off_r2_with_fallback_payload(db_conn, broker_url):
    """R3's prompt shows the fallback line, not R1's stale payload."""
    _w, issue, _r1, r2 = scenario.seed_review_continuation("rust")

    rust_queue.publish(FIRE, args=[scenario.ticker_id(issue)])

    r3 = scenario.wait_for_continuation(issue)
    scenario.assert_review_parent_payload(r3, r2)
