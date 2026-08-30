# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Validation, merge and read semantics for ``Profile.settings``.

Deliberately separate from the ``ee.settings.user_settings`` seam: a build
overlays that module to declare its own namespaces, and a whole-file
replacement cannot import from the file it replaces. Keeping the logic here
means an overlay declares a schema and inherits all of this unchanged.
"""

from __future__ import annotations

from typing import Any


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


def validate_settings_patch(patch: Any) -> dict[str, dict[str, Any]]:
    """Validate a client-supplied ``settings`` payload against the schema.

    Returns the accepted patch. Raises :class:`ValueError` with a human-readable
    message on anything unrecognised — unknown namespace, unknown key, or a
    payload that is not ``{namespace: {key: value}}``.

    This is a closed allow-list on purpose. ``Profile.settings`` is writable
    through the profile API by every authenticated user, so accepting arbitrary
    content would make it an unbounded per-user JSON store rather than a
    settings field.
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
        accepted[namespace] = dict(values)

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
