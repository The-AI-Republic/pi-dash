# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Loop serialization helpers — user-facing and admin-facing shapes.

The user-facing shape (:func:`public_job_payload`) is a deliberate **whitelist**:
``prompt``, ``min_role``, the admin ``name``, and anything that says "loop" must
never reach a normal user. See design §9.1.
"""

from __future__ import annotations

from pi_dash.db.models import LoopJob

_FREQ_LABELS = {
    "MINUTELY": "every few minutes",
    "HOURLY": "hourly",
    "DAILY": "daily",
    "WEEKLY": "weekly",
    "MONTHLY": "monthly",
    "YEARLY": "yearly",
}


def interval_label(rrule: str) -> str:
    """Plain-language cadence from an RRULE FREQ, so the client never parses
    RRULEs. Falls back to "periodically" for anything unrecognized."""
    for part in (rrule or "").split(";"):
        if part.startswith("FREQ="):
            return _FREQ_LABELS.get(part[len("FREQ="):].upper(), "periodically")
    return "periodically"


def public_job_payload(job: LoopJob, *, enabled: bool) -> dict:
    """User-facing job card. Whitelisted keys only."""
    return {
        "slug": job.slug,
        "name": job.public_name,
        "description": job.public_description,
        "interval_label": interval_label(job.rrule),
        "enabled": enabled,
    }
