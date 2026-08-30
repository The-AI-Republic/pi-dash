# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""CE seam for namespaced per-user settings stored on ``Profile.settings``.

Some preferences only exist in a particular build — a hosted deployment may
offer an integration the open-source product has no code for. Adding a column
per such preference would put deployment-specific concepts into the shared
schema, so ``Profile.settings`` is a generic ``{namespace: {key: value}}`` bag
and *this module* is the only thing that says what may live in it.

Open source declares no namespaces, so the bag stays empty and the API rejects
every write to it. A downstream build overlays this module to declare its own
(see the cloud build's ``ee-overlay``), and gets validation, defaults, and the
API surface without changing any model.

The declaration is deliberately values-with-defaults rather than a schema
language: it answers "which keys exist and what do they mean when unset",
which is all the endpoint and the readers need.
"""

from __future__ import annotations

from typing import Any


def known_settings_schema() -> dict[str, dict[str, Any]]:
    """Namespaces this build recognises: ``{namespace: {key: default}}``.

    CE declares none. Anything absent here is rejected on write and ignored on
    read, so an overlay that stops declaring a namespace does not start serving
    values it no longer understands.
    """
    return {}


def default_for(namespace: str, key: str) -> Any:
    """The declared default for one key, or ``None`` when it is not declared."""
    return known_settings_schema().get(namespace, {}).get(key)


def get_setting(profile, namespace: str, key: str) -> Any:
    """Read one setting, falling back to its declared default.

    Callers use this instead of indexing ``profile.settings`` so the default
    lives in one place and a missing/partial bag is never a KeyError.
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

    schema = known_settings_schema()
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
