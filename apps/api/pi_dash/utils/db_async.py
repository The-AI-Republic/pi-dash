# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Run ORM work from async code without parking idle Postgres connections.

``CONN_MAX_AGE`` is 0, so Django closes a connection only when
``close_old_connections`` runs *on the thread that opened it*. Under ASGI that
happens via ``response.close()``, which for a streaming response fires only
once the stream ends — and not at all when the client disconnects abruptly. A
bare ``sync_to_async`` ORM call in an async view therefore parks one idle
connection per open stream or long poll, for as long as it stays open
(observed in prod 2026-06-10 and 2026-09-23: ~94 and ~85 idle connections,
exhausting a Postgres host shared with home-page).

``db_sync_to_async`` closes around every call, so a connection is held only for
the query itself rather than for the life of the request.

It is Channels' ``database_sync_to_async`` with one fix: it never closes a
connection that is inside a transaction. ``close_old_connections`` is not
transaction-aware — inside an atomic block ``close()`` still drops the socket
and sets ``closed_in_transaction``/``needs_rollback``, leaving every later
query to fail with "the connection is closed". That breaks any caller that
wraps one of these helpers in ``transaction.atomic``, and it breaks every
``TestCase``, since Django runs each test inside an atomic block.
"""

from asgiref.sync import SyncToAsync
from django.db import connections


def close_idle_connections() -> None:
    """``close_old_connections``, but never on a connection in a transaction.

    Mid-transaction is never a safe moment to close: Postgres would roll the
    work back, and Django would mark the connection unusable rather than
    reopening it.
    """
    for conn in connections.all(initialized_only=True):
        if not conn.in_atomic_block:
            conn.close_if_unusable_or_obsolete()


class DbSyncToAsync(SyncToAsync):
    """``sync_to_async`` that hands the DB connection back when it is done."""

    def thread_handler(self, loop, *args, **kwargs):
        close_idle_connections()
        try:
            return super().thread_handler(loop, *args, **kwargs)
        finally:
            close_idle_connections()


# Lowercase alias so call sites read like ``sync_to_async``.
db_sync_to_async = DbSyncToAsync
