# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Save-time override validation (design §6.3).

An override body cannot be saved unless it (1) parses as Jinja and (2) renders
cleanly as part of the *full* prompt — for **every kind whose recipe contains
the section** — against two synthetic contexts: one fully populated and one
minimal (all optionals empty). This catches typo'd variables, kind-mismatched
variables (a shared section referencing ``issue.*`` while also used by the
scheduler kind), missing-``{% if %}`` traps, and cross-section interactions
before they can fail a real run.

The sample contexts mirror the keys emitted by ``context.build_context`` /
``context.build_scheduler_context``; ``tests/unit/prompting/test_validation.py``
asserts they stay in sync with the builders.
"""

from __future__ import annotations

from typing import Any, Dict, List

from pi_dash.prompting import recipes, registry
from pi_dash.prompting.composer import ResolvedSection, _assemble, resolve_section
from pi_dash.prompting.renderer import (
    PromptRenderError,
    PromptSyntaxError,
    render,
    validate_syntax,
)

#: Mirrors ``registry.MAX_SECTION_BODY_LENGTH`` — restated for the validator's
#: own error message.
MAX_BODY_LENGTH = registry.MAX_SECTION_BODY_LENGTH


class OverrideValidationError(Exception):
    """Raised when a candidate override body fails save-time validation."""


def _issue_sample(kind: str, *, populated: bool) -> Dict[str, Any]:
    """Issue-context sample matching ``context.build_context`` keys."""
    if populated:
        return {
            "issue": {
                "id": "00000000-0000-0000-0000-000000000001",
                "identifier": "SAMPLE-1",
                "title": "Sample issue title",
                "description": "Sample description body.",
                "state": "In Progress",
                "state_group": "started",
                "priority": "medium",
                "labels": ["backend", "bug"],
                "assignees": ["Sample Assignee"],
                "url": "/sample/projects/p/issues/i",
                "target_date": "2026-01-01",
                "project_states": [
                    {"name": "In Progress", "group": "started", "description": "Active"},
                    {"name": "Done", "group": "completed", "description": "Finished"},
                ],
            },
            "workspace": {"slug": "sample-ws", "name": "Sample Workspace"},
            "project": {
                "id": "00000000-0000-0000-0000-000000000002",
                "identifier": "SAMPLE",
                "name": "Sample Project",
                "description": "Sample project description.",
            },
            "repo": {
                "url": "https://example.com/repo.git",
                "base_branch": "main",
                "work_branch": "pi-dash/sample-1",
            },
            "parent": {
                "identifier": "SAMPLE-0",
                "title": "Parent issue",
                "work_branch": "pi-dash/sample-0",
                "description": "Parent description.",
                "comments_count": 3,
            },
            "lineage": [
                {"identifier": "SAMPLE-1", "title": "Sample issue title"},
                {"identifier": "SAMPLE-0", "title": "Parent issue"},
                {"identifier": "SAMPLE-root", "title": "Root issue"},
            ],
            "run": {
                "id": "00000000-0000-0000-0000-000000000003",
                "kind": kind,
                "attempt": 2,
                "turn_number": 1,
            },
            "comments_section": "### Comment 1 — Human: Sample at 2026-01-01\n\nHello.",
            "parent_done_payload": '{\n  "pr_url": "https://example.com/pr/1"\n}',
            "workpad_body": "### Phase\n- investigating",
        }
    # Minimal: every key present, all optionals empty/None.
    return {
        "issue": {
            "id": "00000000-0000-0000-0000-000000000001",
            "identifier": "SAMPLE-1",
            "title": "",
            "description": "",
            "state": "",
            "state_group": "",
            "priority": "none",
            "labels": [],
            "assignees": [],
            "url": "",
            "target_date": None,
            "project_states": [],
        },
        "workspace": {"slug": "sample-ws", "name": ""},
        "project": {"id": "", "identifier": "SAMPLE", "name": "", "description": ""},
        "repo": {"url": None, "base_branch": None, "work_branch": None},
        "parent": None,
        "lineage": None,
        "run": {
            "id": "00000000-0000-0000-0000-000000000003",
            "kind": kind,
            "attempt": 1,
            "turn_number": 1,
        },
        "comments_section": "(no comments on this issue yet)",
        "parent_done_payload": "(no parent run done payload available)",
        "workpad_body": "",
    }


def _scheduler_sample(*, populated: bool) -> Dict[str, Any]:
    """Scheduler-context sample matching ``context.build_scheduler_context``."""
    if populated:
        return {
            "workspace": {"slug": "sample-ws", "name": "Sample Workspace"},
            "project": {
                "id": "00000000-0000-0000-0000-000000000002",
                "identifier": "SAMPLE",
                "name": "Sample Project",
                "description": "Sample project description.",
            },
            "scheduler": {
                "slug": "nightly-audit",
                "name": "Nightly Audit",
                "description": "Scan for issues nightly.",
            },
            "run": {
                "id": "00000000-0000-0000-0000-000000000003",
                "kind": recipes.KIND_SCHEDULER,
                "attempt": 1,
                "turn_number": 1,
            },
            "scheduler_task_body": "Audit the codebase for TODOs.",
        }
    return {
        "workspace": {"slug": "sample-ws", "name": ""},
        "project": {"id": "", "identifier": "SAMPLE", "name": "", "description": ""},
        "scheduler": {"slug": "s", "name": "", "description": ""},
        "run": {
            "id": "00000000-0000-0000-0000-000000000003",
            "kind": recipes.KIND_SCHEDULER,
            "attempt": 1,
            "turn_number": 1,
        },
        "scheduler_task_body": "",
    }


def sample_contexts(kind: str) -> List[Dict[str, Any]]:
    """Return the (populated, minimal) sample contexts for ``kind``."""
    if kind == recipes.KIND_SCHEDULER:
        return [_scheduler_sample(populated=True), _scheduler_sample(populated=False)]
    return [_issue_sample(kind, populated=True), _issue_sample(kind, populated=False)]


def kinds_for_section(section_key: str) -> List[str]:
    """All recipe kinds whose ordered section list contains ``section_key``."""
    return [k for k, keys in recipes.RECIPES.items() if section_key in keys]


def _compose_with_candidate(
    kind: str, section_key: str, candidate_body: str, *, workspace, project, user
):
    """Assemble ``kind`` with ``section_key`` forced to ``candidate_body``.

    Other sections resolve normally (existing overrides + defaults) so the
    candidate is validated in the real assembled context, not in isolation.
    """
    section = registry.get_section(section_key)
    resolved: List[ResolvedSection] = []
    for key in recipes.recipe_for(kind):
        if key == section_key:
            resolved.append(
                ResolvedSection(
                    key=section.key,
                    title=section.title,
                    customizable=section.customizable,
                    body=candidate_body,
                    source="candidate",
                    version=0,
                )
            )
        else:
            resolved.append(
                resolve_section(key, workspace=workspace, project=project, user=user)
            )
    template_body, _manifest = _assemble(resolved)
    return template_body


def validate_override(
    section_key: str, candidate_body: str, *, workspace, project=None, user=None
) -> None:
    """Validate a candidate override body. Raises on the first failure.

    Steps (design §6.3):
    1. Length cap + Jinja syntax parse.
    2. For every kind containing the section: assemble the full prompt with the
       candidate slotted in and render it against the populated AND minimal
       sample contexts for that kind.
    """
    section = registry.get_section(section_key)
    if section.is_locked:
        raise OverrideValidationError(
            f"section {section_key!r} is locked and cannot be overridden"
        )
    if len(candidate_body) > MAX_BODY_LENGTH:
        raise OverrideValidationError(
            f"override body exceeds {MAX_BODY_LENGTH}-character limit "
            f"(got {len(candidate_body)} characters)"
        )
    try:
        validate_syntax(candidate_body)
    except PromptSyntaxError as exc:
        raise OverrideValidationError(f"invalid Jinja syntax: {exc}") from exc

    for kind in kinds_for_section(section_key):
        template_body = _compose_with_candidate(
            kind, section_key, candidate_body, workspace=workspace, project=project, user=user
        )
        for ctx in sample_contexts(kind):
            try:
                render(template_body, ctx)
            except PromptRenderError as exc:
                raise OverrideValidationError(
                    f"override for section {section_key!r} fails to render as part "
                    f"of the {kind!r} prompt: {exc}"
                ) from exc
