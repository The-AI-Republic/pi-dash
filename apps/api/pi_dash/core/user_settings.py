# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Validation, merge and read semantics for ``Profile.settings``.

Deliberately separate from the ``ee.settings.user_settings`` seam: a build
overlays that module to declare its own namespaces. Keeping the OSS-owned
schema and the composition logic here means an overlay can extend the public
declaration instead of replacing it, while inheriting all validation, merge,
and read behavior unchanged.
"""

from __future__ import annotations

import json
from typing import Any

#: Ceiling on one PATCH's accepted payload, encoded as JSON.
#:
#: Namespaces and keys are a closed allow-list, so the stored bag is bounded by
#: (declared keys x this) — capping the patch caps the bag. Generous for a
#: settings field, far too small to be worth using as storage.
MAX_SETTINGS_PATCH_BYTES = 4096


def base_settings_schema() -> dict[str, dict[str, Any]]:
    """Settings namespaces owned by OSS Pi Dash.

    The public build currently declares none. This function nevertheless lives
    outside the overlayable ``ee`` seam so downstream builds can always import
    the OSS declaration, merge their own namespaces into it, and automatically
    inherit public settings added later.
    """
    return {}


def extend_settings_schema(
    base: dict[str, dict[str, Any]], extension: dict[str, dict[str, Any]]
) -> dict[str, dict[str, Any]]:
    """Return ``base`` plus an independently owned schema ``extension``.

    Namespaces may be shared, but a key may have only one owner. Silently
    overriding a public key from a private build would make its validation and
    default depend on packaging order, so collisions fail when the schema is
    assembled rather than changing user-facing behavior implicitly.

    Neither input is mutated.
    """
    merged = {namespace: dict(values) for namespace, values in base.items()}
    for namespace, values in extension.items():
        existing = merged.setdefault(namespace, {})
        collisions = sorted(set(existing) & set(values))
        if collisions:
            raise ValueError(
                f"settings schema collision(s) in {namespace}: {', '.join(collisions)}"
            )
        existing.update(values)
    return merged


def _schema() -> dict[str, dict[str, Any]]:
    # Imported per call, not at module scope: the seam is overlayable and a
    # module-scope import would bind CE's version before the overlay applies.
    from pi_dash.ee.settings.user_settings import known_settings_schema

    return known_settings_schema()


def default_for(namespace: str, key: str) -> Any:
    """The declared default for one key, or ``None`` when it is not declared."""
    return _schema().get(namespace, {}).get(key)


def get_setting(profile, namespace: str, key: str) -> Any:
    """Read one setting, falling back to its declared default.

    Callers use this instead of indexing ``profile.settings`` so the default
    lives in one place and a missing or partial bag is never a KeyError.
    """
    stored = (profile.settings or {}).get(namespace) or {}
    if key in stored:
        return stored[key]
    return default_for(namespace, key)


def _check_value(namespace: str, key: str, value: Any, default: Any) -> None:
    """Reject a value whose type does not match the key's declared default.

    The schema declares ``{namespace: {key: default}}``, so the default doubles
    as the type declaration. Without this the allow-list would cover key
    *names* only, leaving a declared boolean free to hold a megabyte of text
    and ``get_setting`` free to return a type its caller never expects.

    ``bool`` is checked before ``int`` because it is a subclass of it — without
    the ordering, ``True`` would satisfy an int-typed key and vice versa.
    """
    if default is None:
        # Nothing to infer a type from; allow scalars, refuse containers.
        if isinstance(value, (dict, list)):
            raise ValueError(f"settings.{namespace}.{key} must be a scalar")
        return
    if isinstance(default, bool):
        ok = isinstance(value, bool)
    elif isinstance(default, int):
        ok = isinstance(value, int) and not isinstance(value, bool)
    elif isinstance(default, float):
        ok = isinstance(value, (int, float)) and not isinstance(value, bool)
    else:
        ok = isinstance(value, type(default))
    if not ok:
        raise ValueError(f"settings.{namespace}.{key} must be of type {type(default).__name__}")


def validate_settings_patch(patch: Any) -> dict[str, dict[str, Any]]:
    """Validate a client-supplied ``settings`` payload against the schema.

    Returns the accepted patch. Raises :class:`ValueError` with a human-readable
    message on anything unrecognised — unknown namespace, unknown key, or a
    payload that is not ``{namespace: {key: value}}``.

    This is a closed allow-list on purpose. ``Profile.settings`` is writable
    through the profile API by every authenticated user, so accepting arbitrary
    content would make it an unbounded per-user JSON store rather than a
    settings field. Names, types and total size are all part of that: an
    allow-list of key names alone still admits arbitrary values under them.
    """
    if not isinstance(patch, dict):
        raise ValueError("settings must be an object")

    schema = _schema()
    accepted: dict[str, dict[str, Any]] = {}

    for namespace, values in patch.items():
        if namespace not in schema:
            raise ValueError(f"unknown settings namespace: {namespace}")
        if not isinstance(values, dict):
            raise ValueError(f"settings.{namespace} must be an object")
        unknown = sorted(set(values) - set(schema[namespace]))
        if unknown:
            raise ValueError(f"unknown settings key(s) in {namespace}: {', '.join(unknown)}")
        for key, value in values.items():
            _check_value(namespace, key, value, schema[namespace][key])
        accepted[namespace] = dict(values)

    try:
        encoded = len(json.dumps(accepted).encode())
    except (TypeError, ValueError) as exc:
        # A value the field itself could not store; reject rather than fail on
        # the way to the database.
        raise ValueError("settings must be JSON-serialisable") from exc
    if encoded > MAX_SETTINGS_PATCH_BYTES:
        raise ValueError(f"settings payload is too large ({encoded} > {MAX_SETTINGS_PATCH_BYTES} bytes)")

    return accepted


def merge_settings(stored: Any, patch: dict[str, dict[str, Any]]) -> dict[str, dict[str, Any]]:
    """Merge a validated patch into the stored bag, per namespace.

    Merged rather than replaced so a client patching one namespace does not
    drop another's values — two concurrent PATCHes from different surfaces
    would otherwise silently undo each other.
    """
    merged = {k: dict(v) for k, v in (stored or {}).items() if isinstance(v, dict)}
    for namespace, values in patch.items():
        merged.setdefault(namespace, {}).update(values)
    return merged
