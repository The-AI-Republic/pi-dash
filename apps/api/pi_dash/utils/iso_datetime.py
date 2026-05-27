# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""ISO 8601 datetime helpers shared by the scheduler endpoints, the Beat
fire path, and the serializer. Pulls the parsing logic out of the three
near-identical copies that grew during the iCal recurrence rollout.
"""

from __future__ import annotations

import logging
from datetime import datetime, timezone as dt_timezone
from typing import Iterable, Optional

logger = logging.getLogger(__name__)


def parse_iso_utc(value: Optional[str]) -> Optional[datetime]:
    """Parse an ISO 8601 datetime string, returning a tz-aware UTC datetime.

    Accepts both ``...Z`` and ``...+00:00`` tz suffixes. Returns ``None`` on
    empty input or parse failure.
    """
    if not value:
        return None
    try:
        d = datetime.fromisoformat(value.replace("Z", "+00:00"))
    except ValueError:
        return None
    if d.tzinfo is None:
        d = d.replace(tzinfo=dt_timezone.utc)
    return d.astimezone(dt_timezone.utc)


def coerce_iso_datetimes(raw: Optional[Iterable]) -> list[datetime]:
    """Coerce a JSONField list of ISO strings (or datetimes) into tz-aware UTC datetimes.

    Items that don't parse are skipped with a warning. Used by the scheduler
    fire path and the occurrences endpoint to decode binding.rdates / .exdates.
    """
    out: list[datetime] = []
    for item in raw or ():
        if isinstance(item, datetime):
            d = item
        elif isinstance(item, str):
            d = parse_iso_utc(item)
            if d is None:
                logger.warning("scheduler.bad_isodate value=%r — skipping", item)
                continue
        else:
            logger.warning("scheduler.unexpected_rdate_type type=%s — skipping", type(item).__name__)
            continue
        if d.tzinfo is None:
            d = d.replace(tzinfo=dt_timezone.utc)
        out.append(d)
    return out
