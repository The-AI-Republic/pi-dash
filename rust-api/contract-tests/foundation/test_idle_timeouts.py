"""Backend idle-timeout safeguard (PIDASHCONV-1320).

Both backends must open every Postgres connection with Django's idle guards
(``apps/api/pi_dash/settings/common.py:188-212``):
``idle_in_transaction_session_timeout=60000`` (a transaction left idle 60s is
terminated, releasing its row locks) and ``idle_session_timeout=600000`` (a
plain-idle backend is reaped after 10min).

Oracle gap (verified by probe against PG16, see the issue workpad): Postgres
exposes no per-backend view of these settings — ``SHOW``/``current_setting``
are session-local, ``pg_stat_activity``'s 22 columns carry no GUC, and no
endpoint executes session SQL or holds a 60s+ idle transaction — so no
black-box HTTP+SQL assertion can read a backend-owned connection's values.
The fail-before/pass-after gate therefore lives in ``crates/db``
(``pool::tests::pooled_connections_carry_django_idle_timeouts`` fails pre-fix
with ``"0"`` and passes post-fix with ``"1min"``/``"10min"``); Django parity
is pinned by the immutable ``OPTIONS`` above plus the Django-shell
reproduction (``1min``/``10min`` through a Django-issued connection).

This file pins the load-bearing precondition instead: the shared database
ships the factory default (``0``, disabled) for both settings, so the
safeguard can only come from each backend's own connect options. If a future
environment sets these server-side, this test fails loudly and the threat
model — and the Rust gate's meaning — must be revisited.

Runs unchanged against Django and the Rust port (``DATABASE_URL``); the
assertion is backend-independent by design (see above).
"""

from __future__ import annotations

import pytest

pytestmark = pytest.mark.contract


def test_idle_timeouts_default_to_disabled_server_side(db_conn):
    """Factory defaults: the safeguard must come from the backends, not the server."""
    with db_conn.cursor() as cur:
        cur.execute("SHOW idle_in_transaction_session_timeout")
        in_txn = cur.fetchone()[0]
        cur.execute("SHOW idle_session_timeout")
        idle = cur.fetchone()[0]
    assert in_txn == "0", in_txn
    assert idle == "0", idle
