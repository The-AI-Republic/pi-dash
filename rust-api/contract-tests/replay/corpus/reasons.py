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

MISSING_FIX = {}

OWNED_UNCOVERED = {
    ("/api/v1/auth/desktop-handoff/", "POST"): (
        "desktop_unified_signin §5: the handoff issue landed without a replay "
        "session (a code is single-use and lives 60s, so a recorded replay "
        "cannot redeem it); covered by Django's "
        "tests/contract/runner/test_desktop_handoff.py and the services "
        "live-db test instead."
    ),
    ("/api/v1/auth/desktop-handoff/redeem/", "POST"): (
        "desktop_unified_signin §5: the handoff redeem landed without a replay "
        "session (single-use 60s code + app-held verifier); covered by "
        "Django's tests/contract/runner/test_desktop_handoff.py and the "
        "services live-db test instead."
    ),
    ("/auth/desktop-handoff/redeem/", "POST"): (
        "desktop_unified_signin §5: the redeem path the desktop app posts to, "
        "same handler as /api/v1/auth/desktop-handoff/redeem/; covered by "
        "Django's test_redeem_also_answers_on_the_auth_path instead."
    ),
    ("/api/v1/runner/dev-machines/{}/", "PATCH"): (
        "NEWPIDASH-52: daemon machine PATCH landed without a replay session; "
        "covered by runner/test_engine_machine.py contract tests on both "
        "halves instead. Delete this entry when batch 4 adds the runner "
        "corpus session."
    ),
    ("/api/v1/runner/dev-machines/{}/merge-into/", "POST"): (
        "NEWPIDASH-52: daemon merge-into landed without a replay session; "
        "covered by runner/test_engine_machine.py contract tests on both "
        "halves instead. Delete this entry when batch 4 adds the runner "
        "corpus session."
    ),
}
