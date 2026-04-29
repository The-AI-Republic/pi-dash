# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Builtin scheduler definitions and the per-workspace upsert helper.

The list ``BUILTINS`` is the single source of truth. The seed migration
and the ``Workspace.post_save`` signal both call
:func:`ensure_builtin_schedulers` so existing and new workspaces both get
the catalog without divergent code paths.

See ``.ai_design/project_scheduler/design.md`` §6.6.
"""

from __future__ import annotations

from dataclasses import dataclass
from typing import Iterable, List


@dataclass(frozen=True)
class BuiltinScheduler:
    """One row to upsert into the ``Scheduler`` table for every workspace."""

    slug: str
    name: str
    description: str
    prompt: str


SECURITY_AUDIT_PROMPT = """\
Scan this project's source code for potential security vulnerabilities
(injection, auth bypass, secret leakage, unsafe deserialization, SSRF,
insecure defaults).

For each finding, create a Pi Dash issue using the `pi-dash` CLI:
    pi-dash issue create \\
      --title "[security] <short summary>" \\
      --description "<file path, line range, vulnerable snippet,
                     severity (high|medium|low), and suggested fix>"

Before creating an issue, list existing open issues with the
"[security]" title prefix and skip any finding that already has a
corresponding open issue (de-dupe by file + rule, not by exact title).
"""


BUILTINS: List[BuiltinScheduler] = [
    BuiltinScheduler(
        slug="security-audit",
        name="Security Audit",
        description=(
            "Scans the project for common security vulnerabilities and "
            "files Pi Dash issues for any new findings."
        ),
        prompt=SECURITY_AUDIT_PROMPT,
    ),
]


def ensure_builtin_schedulers(workspace, *, builtins: Iterable[BuiltinScheduler] | None = None) -> int:
    """Idempotent upsert of every BUILTINS entry for ``workspace``.

    Returns the number of rows touched (created or updated). Safe to call
    concurrently — relies on the ``(workspace, slug)`` conditional unique
    constraint to make racing inserts collapse to update-or-create.

    Importable from migrations (does not import the model module at
    file-import time; resolves it via ``apps.get_model`` if needed).
    """
    # Lazy import keeps this module safe to import at migration time.
    from pi_dash.db.models.scheduler import Scheduler, SchedulerSource

    if builtins is None:
        builtins = BUILTINS

    touched = 0
    for builtin in builtins:
        # Manually match on active (non-deleted) rows only — Django's
        # update_or_create takes model fields, not filter lookups, so we
        # can't pass deleted_at__isnull to it. The conditional unique
        # constraint covers the racing-insert case.
        existing = Scheduler.objects.filter(
            workspace=workspace,
            slug=builtin.slug,
            deleted_at__isnull=True,
        ).first()
        if existing is not None:
            existing.name = builtin.name
            existing.description = builtin.description
            existing.prompt = builtin.prompt
            existing.source = SchedulerSource.BUILTIN
            existing.save(
                update_fields=["name", "description", "prompt", "source", "updated_at"]
            )
        else:
            Scheduler.objects.create(
                workspace=workspace,
                slug=builtin.slug,
                name=builtin.name,
                description=builtin.description,
                prompt=builtin.prompt,
                source=SchedulerSource.BUILTIN,
            )
        touched += 1
    return touched
