# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Per-backend expectations for deliberately-diverged (DIFF) contract cases.

An "Old bug fix" changes the new system on purpose where the old system is
wrong. The contract case that pinned the old behaviour must then pin *both*
behaviours: the old behaviour against Django, the fixed behaviour against
Rust — naming the DIFF id the divergence is recorded under in
``.ai_design/validation/differences.md``. This module is that marker::

    from _harness.diffs import expect

    def test_space_unknown_uuid_user(browser, user):
        res = browser.post(space_reset_url(user), data={...})
        assert res.status_code == expect("DIFF-4", django=500, rust=302)

Backend detection: :func:`current_backend` reads ``PIDASH_CONTRACT_BACKEND``
(``django`` when unset or empty, ``rust`` otherwise; anything else fails the
test). The Rust-matrix CI (``.github/workflows/rust-api-contract-rust.yml``)
sets it explicitly per half — ``django`` for the oracle run, ``rust`` for
the Rust run — so each half checks its own arm. Django-only workflows and
local Django runs rely on the default. New code branches on
:func:`current_backend` / :func:`is_rust` instead of reading the variable
ad hoc.

Rules for a DIFF case (see ``rust-api/contract-tests/README.md``):

- the ``django=`` arm pins the old system's *real* behaviour — prove it
  against live Django before converting the case;
- the case must still fail if either backend drifts from its pinned arm
  (each arm is a fixed expectation, never a pass-on-both);
- never delete the case and never weaken it to pass on both.
"""

from __future__ import annotations

import os
import re
from typing import TypeVar

#: Environment variable selecting the backend under test.
BACKEND_ENV_VAR = "PIDASH_CONTRACT_BACKEND"

#: Backend values. ``django`` is the default (Django-only runs never set it).
DJANGO = "django"
RUST = "rust"

_DIFF_ID_RE = re.compile(r"^DIFF-\d+$")

T = TypeVar("T")


def current_backend() -> str:
    """Return ``"django"`` or ``"rust"`` for the backend under test.

    Reads :data:`BACKEND_ENV_VAR`; unset or blank means ``"django"``.
    Any other value raises :class:`RuntimeError` so a misconfigured run
    fails loudly instead of silently checking the wrong arm.
    """
    raw = os.environ.get(BACKEND_ENV_VAR)
    if raw is None or not raw.strip():
        return DJANGO
    value = raw.strip()
    if value not in (DJANGO, RUST):
        raise RuntimeError(
            f"{BACKEND_ENV_VAR} must be {DJANGO!r} or {RUST!r}, "
            f"got {raw!r} (unset means {DJANGO!r})"
        )
    return value


def is_rust() -> bool:
    """True when the suite runs against the Rust backend."""
    return current_backend() == RUST


def is_django() -> bool:
    """True when the suite runs against the Django backend (the default)."""
    return current_backend() == DJANGO


def expect(diff_id: str, *, django: T, rust: T) -> T:
    """Return the pinned expectation for the backend under test.

    ``diff_id`` names the differences-ledger row (``"DIFF-<n>"``) this
    divergence is recorded under; anything else raises :class:`ValueError`
    so a typo'd id fails the test instead of silently passing. ``django``
    and ``rust`` are keyword-only so the arms cannot be swapped by
    position.
    """
    if not isinstance(diff_id, str) or not _DIFF_ID_RE.match(diff_id):
        raise ValueError(
            f"diff_id must look like 'DIFF-<n>' (e.g. 'DIFF-4'), got {diff_id!r}"
        )
    return rust if is_rust() else django
