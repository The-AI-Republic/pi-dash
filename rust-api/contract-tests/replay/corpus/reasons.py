# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Reasons for corpus-uncovered write pairs (PIDASHCONV-823).

Keys are ``(route shape, METHOD)`` with ``{}`` params (the
``shape_key`` form from ``parity/route_inventory.py``). ``coverage.py
check`` fails when a write pair is neither covered by a session nor
listed here, and fails when a listed pair becomes covered or vanishes
(stale reason — delete the entry).

Buckets:

- ``MISSING_FIX``: the pair has no Rust route/method (parity MISSING);
  the reason names the fix issue. The check verifies the pair is
  actually missing live, so a landed fix fails loudly here until the
  pair is covered or moved.
- ``EXCLUDED``: the pair is owned/proxied but not replayable (live
  third-party provider, single-use flow, ...), each with its evidence.
- ``OWNED_UNCOVERED``: owned pairs with no corpus session yet. Entries
  are deleted as corpus batches land; the batch plan lives in the
  header comment below.

Proxied pairs need no entry: the check verifies them live against
``routes --json`` and reports the uniform proxied-by-design reason.
``__debug__`` pairs likewise (dev-only toolbar).
"""

# Batch plan (batch 1 landed with the runner; later batches delete rows):
# - batch 2: v1_work_items, app_project, app_cycles, app_modules
# - batch 3: auth_session, auth_device, auth_magic, space, license
# - batch 4: assistant, loop, prompting, runner, runner_web
# - batch 5: remaining app_* domains + v1_projects + v1_cycles_modules

EXCLUDED = {}

MISSING_FIX = {
    ('/api/instances/admins/', 'DELETE'): "MISSING parity gap G2 (no Rust route); fix PIDASHCONV-827 (Rust license fix: instance-admin session/auth endpoints)",
    ('/api/instances/admins/{}/', 'POST'): "MISSING parity gap G2 (no Rust route); fix PIDASHCONV-827 (Rust license fix: instance-admin session/auth endpoints)",
}

OWNED_UNCOVERED = {
}
