# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Scheduler slug derivation (PDASHOSS01-319).

A scheduler's slug is always derived from its name on the server, so every
client (web, CLI, MCP) gets the same result. The rule must match
``deriveSchedulerSlug`` in
``apps/web/core/components/schedulers/definition-helpers.ts`` (and the Rust
port); ``slug_vectors.json`` next to this module holds the shared test
vectors all of them are checked against.
"""

from __future__ import annotations

import re

# ``Scheduler.slug`` is a ``CharField(max_length=64)``.
SCHEDULER_SLUG_MAX_LENGTH = 64

_NON_SLUG_RUN_RE = re.compile(r"[^a-z0-9]+")


def derive_scheduler_slug(name: str) -> str:
    """Lowercase ``name``, turn every run of characters other than
    ``a-z``/``0-9`` into one dash, trim leading/trailing dashes, and cap at
    64 characters (re-trimming a dash the cut leaves at the end).

    Returns ``""`` when the name has no ASCII letter or digit.
    """
    slug = _NON_SLUG_RUN_RE.sub("-", name.lower()).strip("-")
    return slug[:SCHEDULER_SLUG_MAX_LENGTH].rstrip("-")
