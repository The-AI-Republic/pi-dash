"""PIDASHCONV-1218 oracle (Django worker): a state created after boot still ticks.

`fire_tick` decides "is this a ticking state?" from the issue's *live* state
row on Django (`agent_ticker.py:99-104` `select_related("issue__state")`,
`:187` `is_ticking_state(issue.state)`). The scenario below seeds its project
and In Progress state at test time — after the worker started — and a due
ticker with a prior run, then fires `fire_tick` and asserts the claim.

Runs against the live Celery worker the worker-plane oracle job starts (see
`orchestration/test_ticker_tasks.py` for the same seed/assert shape). The Rust
half of this scenario is `test_rust_replay.py`.
"""

from _harness import celery_wire

from . import _live_state

FIRE = _live_state.FIRE


def test_fire_tick_claims_state_created_after_boot(db_conn, broker_url):
    """A due ticker on an In Progress issue created after boot is claimed."""
    _w, issue = _live_state.seed_state_after_boot("django")
    ticker_id = str(_live_state.ticker(issue)["id"])

    celery_wire.publish(FIRE, args=[ticker_id])

    row = _live_state.wait_for_claim(issue)
    # The claim rolled back its budget (no default pod to dispatch), which is
    # fine: last_tick_at is the fingerprint that the ticking-state gate
    # opened. Before the fix the Rust worker never reached this write.
    assert row["last_tick_at"] is not None
    assert row["used"] == 0
    assert _live_state.run_count(issue) == 1  # only the seeded prior run
