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


FABLE_AUDIT_PROMPT = """\
You are performing a scheduled, read-only security and correctness audit of this
repository. Run autonomously to completion — no human is available to answer
questions, so make reasonable assumptions and note them rather than stopping.

Scope: the entire repository at the current working directory — application code,
configuration, infrastructure-as-code, dependency manifests, and CI/CD
definitions. This is authorized defensive review of the project owner's own code.

Find:
1. Security vulnerabilities — injection (SQL, command, XSS, SSRF, path traversal,
   template, deserialization); broken authn/authz (missing checks, IDOR,
   privilege escalation, weak sessions); secrets committed or logged;
   cryptographic misuse; insecure configuration (permissive CORS, verbose prod
   errors, open cloud resources, unauthenticated endpoints); vulnerable or
   outdated dependencies with known CVEs (read manifests + lockfiles; if a
   scanner such as npm audit / pip-audit / osv-scanner / trivy is available, run
   it and fold in the results); unsafe file handling and redirects.
2. Correctness bugs — logic errors that could cause wrong behavior, data loss,
   crashes, race conditions, or resource leaks, traced through real data flow
   (not lint-level nits).

Method: work at high thoroughness. Trace data from untrusted inputs to sensitive
sinks and reason about WHY each finding is exploitable or wrong, not just whether
a pattern matches. Read the actual code paths. Where deterministic tooling exists
in the repo (SAST, dependency scanners, type checkers, the test suite), run it
and incorporate the output. Do NOT modify code, commit, or open PRs — read-only.

Report EVERY issue you find, including uncertain or low-severity ones; do not
filter while finding. For each finding capture: a short specific title, the file
path and line range, category (security|correctness) and a specific subtype,
severity (high|medium|low), confidence (high|medium|low), a 1-3 sentence
explanation of the exploit path or failure mode, and a concrete fix.

For each NEW finding, create a Pi Dash issue with the `pi-dash` CLI. Use the
title prefix "[fable-security]" for security findings and "[fable-bug]" for
correctness findings (a dedicated namespace so this audit does not collide with
the basic "[security]" Security Audit scheduler):
    pi-dash issue create \\
      --title "[fable-security] <short summary>" \\
      --description "<file path + line range, category/subtype, severity,
                     confidence, explanation, and suggested fix>"

Before creating an issue, list existing open issues with the "[fable-security]"
or "[fable-bug]" title prefix and skip any finding that already has a
corresponding open issue (de-dupe by file + subtype, not by exact title). Never
refile duplicates.

End with a one-line summary: issues filed, duplicates skipped, breakdown by
severity. If there are no new findings, file nothing and report "No new findings".
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
    BuiltinScheduler(
        slug="fable-security-audit",
        name="Claude Mythos/Fable System Security Audit",
        description=(
            "Comprehensive Fable-powered audit: scans the project for security "
            "vulnerabilities and correctness bugs and files Pi Dash issues for "
            "new findings."
        ),
        prompt=FABLE_AUDIT_PROMPT,
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
    from django.db import IntegrityError, transaction

    from pi_dash.db.models.scheduler import Scheduler, SchedulerSource

    if builtins is None:
        builtins = BUILTINS

    def _apply_defaults(row: Scheduler, builtin: BuiltinScheduler) -> None:
        row.name = builtin.name
        row.description = builtin.description
        row.prompt = builtin.prompt
        row.source = SchedulerSource.BUILTIN
        row.save(update_fields=["name", "description", "prompt", "source", "updated_at"])

    touched = 0
    for builtin in builtins:
        # Manually match on active (non-deleted) rows — Django's
        # update_or_create takes model fields, not filter lookups, so we
        # can't pass deleted_at__isnull to it. Two callers (migration +
        # post_save signal) can race on first deploy; the conditional
        # unique constraint will reject the second create with
        # IntegrityError, so wrap the create in a savepoint and retry as
        # an update on conflict.
        existing = Scheduler.objects.filter(
            workspace=workspace,
            slug=builtin.slug,
            deleted_at__isnull=True,
        ).first()
        if existing is not None:
            _apply_defaults(existing, builtin)
        else:
            try:
                with transaction.atomic():
                    Scheduler.objects.create(
                        workspace=workspace,
                        slug=builtin.slug,
                        name=builtin.name,
                        description=builtin.description,
                        prompt=builtin.prompt,
                        source=SchedulerSource.BUILTIN,
                    )
            except IntegrityError:
                # The other caller created it between our SELECT and
                # INSERT. Re-fetch and apply defaults so both callers
                # converge to the same state.
                winner = Scheduler.objects.filter(
                    workspace=workspace,
                    slug=builtin.slug,
                    deleted_at__isnull=True,
                ).first()
                if winner is not None:
                    _apply_defaults(winner, builtin)
        touched += 1
    return touched
