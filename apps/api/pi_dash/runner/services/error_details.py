# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Canonical failure shape for agent runs (PDASHOSS01-187).

``agent_run`` used to describe one concept — how the run went wrong — across
three columns: ``error_code``, ``error`` and ``refusal_category``. They are
written together at terminal time by one writer and none was ever a query
predicate, so they now live in one JSON bag, ``AgentRun.error_details``::

    {
        "code": "run_timeout",          # short machine reason
        "message": "worker was lost",   # operator-facing detail
        "refusal_category": "cyber",    # only when status == REFUSED
    }

Only keys with a value are present, so a run that ended cleanly carries
``{}`` rather than three empty strings. Readers go through
``AgentRun.error_code`` / ``.error`` / ``.refusal_category``, which return
``""`` for an absent key — the API keeps emitting the three flat keys with
exactly the values and empty-string defaults the columns produced.

``refusal_category`` is additionally a Postgres STORED generated column
(``runner/fields.py:JSONKeyTextField``), so grouping declines by category
stays a real-column query. See the note on the model field.
"""

from __future__ import annotations

from typing import Any, Dict, Mapping, Optional

#: Update keys the three columns used to accept, mapped to their key inside
#: ``error_details``. Callers keep passing the flat names.
FOLDED_KEYS = {
    "error_code": "code",
    "error": "message",
    "refusal_category": "refusal_category",
}


def merge_error_details(existing: Any, **fields: Any) -> Dict[str, Any]:
    """Return ``existing`` with ``fields`` applied, dropping emptied keys.

    ``fields`` are the flat names (``error_code`` / ``error`` /
    ``refusal_category``); a falsy value removes the key rather than storing
    an empty string, which is how a COMPLETED run clears a prior attempt's
    failure text. Keys not named are left alone.
    """
    merged = dict(existing) if isinstance(existing, Mapping) else {}
    for flat, key in FOLDED_KEYS.items():
        if flat not in fields:
            continue
        value = fields[flat]
        if value:
            merged[key] = value
        else:
            merged.pop(key, None)
    return merged


def split_folded_updates(values: Dict[str, Any]) -> Optional[Dict[str, Any]]:
    """Pop the folded flat keys out of an ``update()`` kwargs dict.

    Returns the popped ``{flat_name: value}`` mapping, or ``None`` when the
    caller named none of them. ``refusal_category`` in particular *must* be
    intercepted: it is a generated column now, and Postgres rejects a write
    to one.
    """
    popped = {flat: values.pop(flat) for flat in list(FOLDED_KEYS) if flat in values}
    return popped or None
