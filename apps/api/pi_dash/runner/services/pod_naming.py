# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Pod-name validation helpers.

See ``.ai_design/n_runners_in_same_machine/new_pod_project_relationship/design.md``
§6.3 for the full naming rules. In short:

- Default pod (auto-created):  ``{project.identifier}_pod_<n>``
- User-created pod:            ``{project.identifier}_<custom_suffix>``

The ``{project.identifier}_`` prefix is mandatory and server-enforced
on every create / rename call. User-supplied suffixes can't match
``pod_\\d+`` (reserved for system auto-generation).
"""

from __future__ import annotations

import re
from typing import Optional

# Suffixes that match this pattern are reserved for the auto-default-pod
# naming scheme. Letting users supply them creates obvious confusion
# ("did I name this pod_1 or did the system?").
_RESERVED_USER_SUFFIX_RE = re.compile(r"^pod_\d+$")
_USER_SUFFIX_CHARSET_RE = re.compile(r"^[A-Za-z0-9._-]+$")

POD_NAME_MAX_LENGTH = 128
USER_SUFFIX_MAX_LENGTH = 96


def required_prefix(project_identifier: str) -> str:
    """Return the mandatory pod-name prefix for ``project_identifier``."""
    return f"{project_identifier}_"


def validate_user_pod_name(
    name: str, project_identifier: str
) -> Optional[str]:
    """Validate a user-supplied pod name. Returns an error message or None.

    Used by the POST /api/runners/pods/ create endpoint and the rename
    PATCH. The name must:

    - Begin with ``{project_identifier}_``.
    - Be ≤ ``POD_NAME_MAX_LENGTH`` characters total.
    - Have a non-empty suffix after the prefix, ≤ ``USER_SUFFIX_MAX_LENGTH``
      chars, drawing from ``[A-Za-z0-9._-]``.
    - Not collide with the auto-default suffix pattern ``pod_\\d+``.
    """
    if not isinstance(name, str) or not name:
        return "name is required"
    prefix = required_prefix(project_identifier)
    if not name.startswith(prefix):
        return f"name must start with {prefix!r}"
    if len(name) > POD_NAME_MAX_LENGTH:
        return f"name must be at most {POD_NAME_MAX_LENGTH} characters"
    suffix = name[len(prefix) :]
    if not suffix:
        return "name suffix (after the project prefix) cannot be empty"
    if len(suffix) > USER_SUFFIX_MAX_LENGTH:
        return (
            f"name suffix must be at most {USER_SUFFIX_MAX_LENGTH} characters"
        )
    if not _USER_SUFFIX_CHARSET_RE.match(suffix):
        return (
            "name suffix may only contain letters, digits, '.', '_', '-'"
        )
    if _RESERVED_USER_SUFFIX_RE.match(suffix):
        return (
            "suffixes matching 'pod_<digits>' are reserved for "
            "auto-generated default pods"
        )
    return None


def is_auto_default_name(name: str, project_identifier: str) -> bool:
    """True iff ``name`` is the system-default-pod naming pattern."""
    prefix = required_prefix(project_identifier)
    if not name.startswith(prefix):
        return False
    suffix = name[len(prefix) :]
    return bool(_RESERVED_USER_SUFFIX_RE.match(suffix))
