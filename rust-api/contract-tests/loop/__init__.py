"""Contract tests: loop auto-pm (D-03 oracle).

Covers the 5 loop URL patterns against a live backend:

- ``GET`` + ``PATCH`` ``/api/users/me/auto-pm/`` (own settings, master pause)
- ``PATCH`` ``/api/users/me/auto-pm/jobs/<slug>/`` (per-job opt-out)
- ``GET`` + ``POST`` ``/api/instances/loop/jobs/`` (instance-admin catalog)
- ``GET`` + ``PATCH`` + ``DELETE`` ``/api/instances/loop/jobs/<uuid>/``
- ``GET`` ``/api/instances/loop/jobs/<uuid>/targets/`` (run cursors)

plus the worker path (``test_worker.py``): the Celery tasks
``pi_dash.bgtasks.loop.scan_due_targets`` / ``fire_loop_target`` are published
in wire format to a dedicated queue, a live worker executes them, and the
suite diffs the database before/after while asserting the downstream
``assistant.run_turn`` message wire format through the broker management API
(peek-and-requeue: observed, never executed).

Auth uses two cookies with the same session payload format: ``session-id``
for app routes, ``admin-session-id`` for ``/api/instances/`` routes (see
``_harness.sessions.login_admin``).

Known Django quirks are pinned exactly and listed in the PR; the Rust port
reproduces them byte for byte.
"""
