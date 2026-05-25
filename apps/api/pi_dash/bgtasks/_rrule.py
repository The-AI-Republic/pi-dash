# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""RRULE helpers for the project scheduler.

Two responsibilities:

1. ``cron_to_rrule(cron_expr)`` — convert a 5-field cron string into the
   ``(rrule, hint_dtstart_minute, hint_dtstart_hour)`` triple used by the
   migration. Only the cron flavors we actually have in production are
   handled — see the docstring for the supported grammar.

2. ``validate_rrule_string(rrule_str)`` — run our own constraints on an
   RRULE string before persisting it. FREQ=SECONDLY is rejected (DoS
   vector against the Beat tick); effective fire rate must be >= 1 min.

3. ``next_fire_from_rrule(...)`` — return the next datetime an RRULE
   bundle is due after ``now`` (UTC). Honors ``dtstart``, ``tzid``,
   ``rrule``, ``rdates``, ``exdates``. Returns ``None`` on parse error.

This module is intentionally Django-free so the migration can import it
without setting up the apps registry.
"""

from __future__ import annotations

import logging
from dataclasses import dataclass
from datetime import datetime, timedelta, timezone as dt_timezone
from typing import Iterable, Optional

from dateutil import rrule as dateutil_rrule
from dateutil.rrule import rrulestr

logger = logging.getLogger(__name__)


# RFC 5545 day-of-week codes for cron DOW (0=Sunday in cron, mapped to SU).
_DOW_TO_BYDAY = ["SU", "MO", "TU", "WE", "TH", "FR", "SA"]

# FREQ enums we allow. SECONDLY is rejected.
_ALLOWED_FREQS = {"MINUTELY", "HOURLY", "DAILY", "WEEKLY", "MONTHLY", "YEARLY"}


class CronConversionError(ValueError):
    """Raised when a cron expression can't be losslessly converted to RRULE."""


class RRuleValidationError(ValueError):
    """Raised when an RRULE string violates Pi Dash's constraints."""


# ---------------------------------------------------------------------- cron parsing


@dataclass(frozen=True)
class _CronField:
    """One parsed cron field: either ``None`` (= ``*``) or a sorted list of ints."""
    values: Optional[tuple[int, ...]]
    # True if the field came in as ``*/N`` (interval semantics). Used so we can
    # emit ``INTERVAL=N`` for FREQ=MINUTELY instead of a huge BYMINUTE list.
    is_interval: bool = False
    interval: Optional[int] = None


def _parse_cron_field(spec: str, *, lo: int, hi: int) -> _CronField:
    """Parse one cron field. Supports ``*``, ``N``, ``N-M``, ``*/N``,
    ``N-M/K``, and comma-separated lists thereof."""
    spec = spec.strip()
    if not spec:
        raise CronConversionError(f"empty cron field")
    if spec == "*":
        return _CronField(values=None)

    # */N — interval form. Keep the interval info distinct so the FREQ picker
    # below can emit INTERVAL=N instead of a long BYMINUTE list.
    if spec.startswith("*/"):
        try:
            interval = int(spec[2:])
        except ValueError:
            raise CronConversionError(f"bad interval: {spec!r}")
        if interval <= 0:
            raise CronConversionError(f"interval must be positive: {spec!r}")
        # Materialize the value list so callers can still see "which values
        # would fire" if they want it; also it's needed when this isn't the
        # field driving FREQ.
        values = tuple(range(lo, hi + 1, interval))
        return _CronField(values=values, is_interval=True, interval=interval)

    values: list[int] = []
    for piece in spec.split(","):
        piece = piece.strip()
        step = 1
        if "/" in piece:
            piece, step_s = piece.split("/", 1)
            try:
                step = int(step_s)
            except ValueError:
                raise CronConversionError(f"bad step: {step_s!r}")
            if step <= 0:
                raise CronConversionError(f"step must be positive: {step_s!r}")
        if piece == "*":
            start, end = lo, hi
        elif "-" in piece:
            try:
                start_s, end_s = piece.split("-", 1)
                start, end = int(start_s), int(end_s)
            except ValueError:
                raise CronConversionError(f"bad range: {piece!r}")
        else:
            try:
                start = end = int(piece)
            except ValueError:
                raise CronConversionError(f"bad value: {piece!r}")
        if start < lo or end > hi or start > end:
            raise CronConversionError(
                f"value out of range [{lo}..{hi}]: {piece!r}"
            )
        values.extend(range(start, end + 1, step))
    # Dedup + sort. cron is set-valued; "1,1,2" is just "1,2".
    return _CronField(values=tuple(sorted(set(values))))


def cron_to_rrule(cron_expr: str) -> str:
    """Convert a 5-field cron expression to an RFC 5545 RRULE string.

    Supports the cron grammar we actually have in production: ``*``,
    ``N``, ``N-M``, ``*/N``, ``N-M/K``, and comma-separated lists.

    Raises ``CronConversionError`` for:
    - Wrong field count (must be 5)
    - DOM and DOW both constrained — Vixie semantics are OR which RRULE
      can't express as a single rule. Manual disambiguation required.
    - Unsupported grammar (e.g. day names like ``MON``, the ``@yearly``
      shortcuts; rare in our data).
    """
    parts = cron_expr.split()
    if len(parts) != 5:
        raise CronConversionError(
            f"cron must have exactly 5 fields, got {len(parts)}: {cron_expr!r}"
        )
    minute = _parse_cron_field(parts[0], lo=0, hi=59)
    hour = _parse_cron_field(parts[1], lo=0, hi=23)
    dom = _parse_cron_field(parts[2], lo=1, hi=31)
    month = _parse_cron_field(parts[3], lo=1, hi=12)
    # cron DOW: 0 or 7 = Sunday. Normalize 7 → 0 before parsing.
    dow_spec = parts[4].replace("7", "0") if parts[4] != "*" else parts[4]
    dow = _parse_cron_field(dow_spec, lo=0, hi=6)

    dom_constrained = dom.values is not None
    dow_constrained = dow.values is not None
    if dom_constrained and dow_constrained:
        raise CronConversionError(
            f"cron has both day-of-month and day-of-week set; Vixie OR semantics "
            f"can't be expressed in a single RRULE: {cron_expr!r}"
        )

    # Determine FREQ.
    if dow_constrained:
        freq = "WEEKLY"
    elif dom_constrained:
        freq = "MONTHLY"
    elif month.values is not None:
        # Month set but DOM unset = "every day in this month every year" —
        # ambiguous and uncommon, emit YEARLY and let the BY* clauses define it.
        freq = "YEARLY"
    elif hour.values is not None or (minute.values is not None and not minute.is_interval):
        # Specific hour(s) and/or specific minute(s) → DAILY or HOURLY.
        if hour.values is None:
            freq = "HOURLY"
        else:
            freq = "DAILY"
    else:
        freq = "MINUTELY"

    parts_out: list[str] = [f"FREQ={freq}"]

    # INTERVAL — for MINUTELY/HOURLY we honor the */N form on the driving
    # field. For other FREQs the driving cadence comes from the BY* clauses.
    if freq == "MINUTELY":
        if minute.is_interval and minute.interval:
            parts_out.append(f"INTERVAL={minute.interval}")
        elif minute.values is not None and len(minute.values) > 1:
            parts_out.append(
                "BYMINUTE=" + ",".join(str(v) for v in minute.values)
            )
        # else: minute=*, fires every minute, INTERVAL defaults to 1
    elif freq == "HOURLY":
        if hour.is_interval and hour.interval:
            parts_out.append(f"INTERVAL={hour.interval}")
        if minute.values is not None:
            parts_out.append(
                "BYMINUTE=" + ",".join(str(v) for v in minute.values)
            )
    else:
        # DAILY/WEEKLY/MONTHLY/YEARLY all use BYHOUR/BYMINUTE for time.
        if hour.values is not None:
            parts_out.append("BYHOUR=" + ",".join(str(v) for v in hour.values))
        if minute.values is not None:
            parts_out.append("BYMINUTE=" + ",".join(str(v) for v in minute.values))

    if freq == "WEEKLY" and dow.values is not None:
        parts_out.append(
            "BYDAY=" + ",".join(_DOW_TO_BYDAY[d] for d in dow.values)
        )
    if freq in ("MONTHLY", "YEARLY") and dom.values is not None:
        parts_out.append("BYMONTHDAY=" + ",".join(str(v) for v in dom.values))
    if month.values is not None and freq != "MONTHLY":
        parts_out.append("BYMONTH=" + ",".join(str(v) for v in month.values))

    return ";".join(parts_out)


# ---------------------------------------------------------------------- validation


def validate_rrule_string(rrule_str: str, *, dtstart: Optional[datetime] = None) -> None:
    """Raise ``RRuleValidationError`` if the RRULE violates our constraints.

    Constraints:
    - Parses cleanly under ``dateutil.rrule.rrulestr``
    - FREQ is one of MINUTELY/HOURLY/DAILY/WEEKLY/MONTHLY/YEARLY
    - Effective fire rate is >= 1 minute (i.e. ``INTERVAL >= 1`` for MINUTELY)

    ``rrule_str`` is optional in our model; callers pass ``""`` for
    single-shot bindings (which fire once at ``dtstart``). This function
    accepts the empty string as valid (single-shot is fine).
    """
    if not rrule_str:
        return  # empty = single-shot, fine.

    anchor = dtstart or datetime.now(tz=dt_timezone.utc)
    try:
        # rrulestr can accept either a bare "FREQ=..." or a full "RRULE:FREQ=..."
        # form. dateutil handles both, but the bare form needs dtstart=.
        parsed = rrulestr(rrule_str, dtstart=anchor)
    except (ValueError, TypeError) as e:
        raise RRuleValidationError(f"invalid RRULE: {e}") from e

    freq_value = getattr(parsed, "_freq", None)
    if freq_value is None:
        raise RRuleValidationError("RRULE is missing FREQ")

    # dateutil's _freq is an int; map back to the name via dateutil's constants.
    freq_name_map = {
        dateutil_rrule.YEARLY: "YEARLY",
        dateutil_rrule.MONTHLY: "MONTHLY",
        dateutil_rrule.WEEKLY: "WEEKLY",
        dateutil_rrule.DAILY: "DAILY",
        dateutil_rrule.HOURLY: "HOURLY",
        dateutil_rrule.MINUTELY: "MINUTELY",
        dateutil_rrule.SECONDLY: "SECONDLY",
    }
    freq_name = freq_name_map.get(freq_value)
    if freq_name not in _ALLOWED_FREQS:
        raise RRuleValidationError(
            f"FREQ={freq_name} is not allowed (allowed: {sorted(_ALLOWED_FREQS)})"
        )

    interval = getattr(parsed, "_interval", 1) or 1
    if interval < 1:
        raise RRuleValidationError(f"INTERVAL must be >= 1, got {interval}")


# ---------------------------------------------------------------------- expansion


def next_fire_from_rrule(
    *,
    dtstart: datetime,
    rrule_str: str,
    tzid: str = "UTC",
    rdates: Optional[Iterable[datetime]] = None,
    exdates: Optional[Iterable[datetime]] = None,
    now: Optional[datetime] = None,
) -> Optional[datetime]:
    """Return the next datetime the RRULE bundle is due strictly after ``now``.

    Returns ``None`` on parse error (caller treats as a configuration error
    and disables the binding).

    Semantics:
    - ``dtstart`` is the series anchor. If ``rrule_str`` is empty, the rule
      fires once at ``dtstart`` (and only if it's after ``now``).
    - ``rdates`` are additional one-off firings appended to the series.
    - ``exdates`` are firings to skip.
    - Returned datetime is timezone-aware UTC.

    The ``tzid`` is currently informational at this layer — dateutil expands
    ``dtstart`` as it is (we pass tz-aware UTC). Wall-clock-aware DST
    semantics are deferred to a later PR; for now ``tzid`` is stored and
    surfaced in the API but doesn't drive expansion.
    """
    base = now or datetime.now(tz=dt_timezone.utc)
    if base.tzinfo is None:
        base = base.replace(tzinfo=dt_timezone.utc)
    if dtstart.tzinfo is None:
        dtstart = dtstart.replace(tzinfo=dt_timezone.utc)

    try:
        if rrule_str:
            rule = rrulestr(rrule_str, dtstart=dtstart)
            # rdates/exdates handling: dateutil's rrulestr doesn't directly
            # accept rdates/exdates parameters from a single string, so we
            # build an rruleset if extras are present.
            if rdates or exdates:
                rset = dateutil_rrule.rruleset()
                rset.rrule(rule)
                for d in rdates or ():
                    if d.tzinfo is None:
                        d = d.replace(tzinfo=dt_timezone.utc)
                    rset.rdate(d)
                for d in exdates or ():
                    if d.tzinfo is None:
                        d = d.replace(tzinfo=dt_timezone.utc)
                    rset.exdate(d)
                nxt = rset.after(base, inc=False)
            else:
                nxt = rule.after(base, inc=False)
        else:
            # Single-shot: fire at dtstart if it's still ahead.
            if dtstart > base and dtstart not in (exdates or ()):
                nxt = dtstart
            else:
                # Check rdates for an explicit one-off after now.
                ahead = sorted(d for d in (rdates or ()) if d > base)
                nxt = ahead[0] if ahead else None
        if nxt is None:
            return None
        if nxt.tzinfo is None:
            nxt = nxt.replace(tzinfo=dt_timezone.utc)
        return nxt.astimezone(dt_timezone.utc)
    except (ValueError, TypeError) as e:
        logger.warning("scheduler.rrule_parse: bad rrule=%r err=%s", rrule_str, e)
        return None


def occurrences_between(
    *,
    dtstart: datetime,
    rrule_str: str,
    tzid: str = "UTC",
    rdates: Optional[Iterable[datetime]] = None,
    exdates: Optional[Iterable[datetime]] = None,
    window_start: datetime,
    window_end: datetime,
    cap: int = 5000,
) -> tuple[list[datetime], bool]:
    """Expand the RRULE bundle into all occurrences in ``[window_start, window_end]``.

    Returns ``(occurrences, has_more)``. ``has_more`` is True if the cap was
    hit; callers should surface "narrow the date range" to the user.

    Used by the occurrences endpoint (PR2).
    """
    if dtstart.tzinfo is None:
        dtstart = dtstart.replace(tzinfo=dt_timezone.utc)
    if window_start.tzinfo is None:
        window_start = window_start.replace(tzinfo=dt_timezone.utc)
    if window_end.tzinfo is None:
        window_end = window_end.replace(tzinfo=dt_timezone.utc)

    out: list[datetime] = []
    try:
        if rrule_str:
            rule = rrulestr(rrule_str, dtstart=dtstart)
            if rdates or exdates:
                rset = dateutil_rrule.rruleset()
                rset.rrule(rule)
                for d in rdates or ():
                    if d.tzinfo is None:
                        d = d.replace(tzinfo=dt_timezone.utc)
                    rset.rdate(d)
                for d in exdates or ():
                    if d.tzinfo is None:
                        d = d.replace(tzinfo=dt_timezone.utc)
                    rset.exdate(d)
                iterator = rset.between(window_start, window_end, inc=True)
            else:
                iterator = rule.between(window_start, window_end, inc=True)
            for occ in iterator:
                if occ.tzinfo is None:
                    occ = occ.replace(tzinfo=dt_timezone.utc)
                out.append(occ.astimezone(dt_timezone.utc))
                if len(out) >= cap:
                    return out, True
        else:
            # Single-shot
            if window_start <= dtstart <= window_end and dtstart not in (exdates or ()):
                out.append(dtstart)
            for d in rdates or ():
                if d.tzinfo is None:
                    d = d.replace(tzinfo=dt_timezone.utc)
                if window_start <= d <= window_end and d not in (exdates or ()):
                    out.append(d)
            out.sort()
    except (ValueError, TypeError) as e:
        logger.warning("scheduler.rrule_expand: bad rrule=%r err=%s", rrule_str, e)
        return [], False
    return out, False
