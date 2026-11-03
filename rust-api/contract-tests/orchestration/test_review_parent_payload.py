# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""PIDASHCONV-1232 oracle (Django worker): review continuation parent payload.

Seeds R1 (completed, done payload) / R2 (failed, NULL payload) on an
In Review issue whose ticker resume parent is R1, fires ``fire_tick``, and
asserts the continuation run R3 (parent R2) carries the fallback payload
block — Django renders ``(no parent run done payload available)`` when the
run's own parent has no payload, and never falls back to the ticker.

Runs against the live Celery worker the worker-plane oracle job starts (see
``orchestration/test_ticker_tasks.py`` for the same seed/publish shape). The
Rust half of this scenario is ``test_review_parent_payload_rust.py``.
"""

from _harness import celery_wire

from . import _review_parent_payload as scenario

FIRE = scenario.FIRE


def test_tick_parents_r3_off_r2_with_fallback_payload(db_conn, broker_url):
    """R3's prompt shows the fallback line, not R1's stale payload."""
    _w, issue, _r1, r2 = scenario.seed_review_continuation("django")

    celery_wire.publish(FIRE, args=[scenario.ticker_id(issue)])

    r3 = scenario.wait_for_continuation(issue)
    scenario.assert_review_parent_payload(r3, r2)
