# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""CE seam declaring which namespaced user settings exist.

Some preferences only exist in a particular build — a hosted deployment may
offer an integration the open-source product has no code for. Adding a column
per such preference would put deployment-specific concepts into the shared
schema, so ``Profile.settings`` is a generic ``{namespace: {key: value}}`` bag
and *this module* is the only thing that says what may live in it.

Open source declares no namespaces, so the bag stays empty and the API rejects
every write to it. A downstream build replaces this file to declare its own,
and inherits validation, defaults and the API surface without changing any
model. The semantics live in ``pi_dash.core.user_settings`` precisely so a
replacement of this file does not have to reimplement them.

The declaration is values-with-defaults rather than a schema language: it
answers "which keys exist and what do they mean when unset", which is all the
endpoint and the readers need.
"""

from __future__ import annotations

from typing import Any

# Re-exported so callers import one module regardless of which build declared
# the schema.
from pi_dash.core.user_settings import (  # noqa: F401
    default_for,
    get_setting,
    merge_settings,
    validate_settings_patch,
)


def known_settings_schema() -> dict[str, dict[str, Any]]:
    """Namespaces this build recognises: ``{namespace: {key: default}}``.

    CE declares none. Anything absent here is rejected on write and ignored on
    read, so an overlay that stops declaring a namespace does not start serving
    values it no longer understands.
    """
    return {}
