# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Default template seed.

The global default `PromptTemplate` (workspace=NULL) is the runtime source of
truth, but the *initial* body is composed from the ordered fragments in
``apps/api/pi_dash/prompting/fragments/`` so it can evolve in code review a
section at a time. At migrate time we insert the row if it is missing.
Operators who want to re-sync the default after editing fragments call the
``reseed_default_template`` management command — we never silently clobber a
workspace row.

The ``review`` template (used by the In Review phase, see
``.ai_design/create_review_state/design.md`` §5) lives here as a single
body string for v1. It is polymorphic: at runtime the agent decides
which kind of review (CODE / DESIGN / DESIGN_THEN_CODE / GENERIC)
applies based on what the prior implementation run produced.
"""

from __future__ import annotations

import os

from pi_dash.prompting.fragments import assemble


REVIEW_TEMPLATE_NAME = "review"

#: Polymorphic review prompt. At runtime the agent picks the right
#: review kind from ``parent_done_payload`` (and working-tree
#: inspection) and runs the cycle for that kind. See design §4.7 / §5.
REVIEW_TEMPLATE_BODY = """\
You are reviewing the work product of a previous implementation pass.
"Review" can mean different things depending on what was produced.

Issue: {{ issue.name }}
Description: {{ issue.description_stripped }}
Recent activity:
{{ comments_section }}
Latest implementation run output (read this carefully —
it is your authoritative record of what was produced):
{{ parent_done_payload }}

Step 1 — Decide what kind of review this is.
Inspect parent_done_payload, the issue description, and the working
tree. Choose ONE:
  (a) CODE — the issue produced a GitHub PR (look for a `pr_url` in
      done_payload, or a feature branch ahead of main).
  (b) DESIGN — the issue produced a design / planning document
      (look for paths under `.ai_design/`, paths in
      `done_payload.design_doc_paths`, or markdown artifacts
      referenced as outputs).
  (c) DESIGN_THEN_CODE — both a design doc AND a PR exist. Review
      the design first, then the code.
  (d) GENERIC — none of the above. Review the work product against
      the issue description and leave a summary on the pidash issue.

If you cannot decide, ask the human via `paused`.

Step 2 — Run the cycle for the chosen kind. All cycles share this
shape:
  i.   Find issues with the work product.
  ii.  Validate your findings (no hallucinations) — re-read the
       artifact, confirm each issue is real, drop any that aren't.
  iii. Read existing reviewer comments (in the PR, in the doc, or
       on the pidash issue depending on kind) and reconcile your
       findings against them.
  iv.  Comment on the validated issues at the appropriate surface:
       - CODE: comments on the GitHub PR (use `gh` CLI).
       - DESIGN: inline comments on the doc, or a structured
         comment on the pidash issue if the doc has no comment
         surface.
       - DESIGN_THEN_CODE: design comments first, then PR comments.
       - GENERIC: a structured comment on the pidash issue.
  v.   If you can fix a confirmed issue and the kind permits it,
       apply the fix and resolve the corresponding comment:
       - CODE: edit, commit, push to the PR branch, resolve the
         PR comment thread.
       - DESIGN: edit the doc and resolve / strike the inline
         comment.
       - GENERIC: usually does NOT auto-apply — leave the summary
         and let the human act.
  vi.  Post a summary back to the pidash issue as a comment:
       confirmed issues found, what you fixed automatically, what
       still needs human action.

Step 3 — Emit a done-signal.
- `completed` = approved, no further automatic ticking needed
  (the review pass is satisfied).
- `blocked` = real issues found that you couldn't auto-fix and
  need human attention.
- `paused` = clarifying question for the human.
- `noop` = nothing has changed since your last review pass.
"""


def read_review_body() -> str:
    """Return the review template body."""
    return REVIEW_TEMPLATE_BODY


def read_default_body() -> str:
    """Return the default prompt body, composed from fragments."""
    return assemble()


def seed_default_template(force: bool = False) -> str:
    """Create or (if ``force``) refresh the global default PromptTemplate.

    Returns one of: ``"created"``, ``"refreshed"``, ``"skipped"``.
    """
    from pi_dash.prompting.models import PromptTemplate

    body = read_default_body()
    existing = (
        PromptTemplate.objects.filter(
            workspace__isnull=True, name=PromptTemplate.DEFAULT_NAME
        )
        .order_by("-updated_at")
        .first()
    )
    if existing is None:
        PromptTemplate.objects.create(
            workspace=None,
            name=PromptTemplate.DEFAULT_NAME,
            body=body,
            is_active=True,
            version=1,
        )
        return "created"

    if force and existing.body != body:
        existing.body = body
        existing.version = (existing.version or 0) + 1
        existing.is_active = True
        existing.save(update_fields=["body", "version", "is_active", "updated_at"])
        return "refreshed"
    return "skipped"


def seed_review_template(force: bool = False) -> str:
    """Create or (if ``force``) refresh the global ``review``
    PromptTemplate row. Returns ``"created"`` / ``"refreshed"`` /
    ``"skipped"``.
    """
    from pi_dash.prompting.models import PromptTemplate

    body = read_review_body()
    existing = (
        PromptTemplate.objects.filter(
            workspace__isnull=True, name=REVIEW_TEMPLATE_NAME
        )
        .order_by("-updated_at")
        .first()
    )
    if existing is None:
        PromptTemplate.objects.create(
            workspace=None,
            name=REVIEW_TEMPLATE_NAME,
            body=body,
            is_active=True,
            version=1,
        )
        return "created"

    if force and existing.body != body:
        existing.body = body
        existing.version = (existing.version or 0) + 1
        existing.is_active = True
        existing.save(update_fields=["body", "version", "is_active", "updated_at"])
        return "refreshed"
    return "skipped"


def seed_default_template_on_migrate(
    sender=None, app_config=None, verbosity=1, using=None, **kwargs
) -> None:
    """`post_migrate` receiver. Only runs from the prompting app config."""
    # Running under unrelated apps is fine — post_migrate fires once per app —
    # but we gate on our own app to avoid creating multiple rows.
    if app_config is not None and app_config.label != "prompting":
        return
    if os.environ.get("PI_DASH_SKIP_PROMPT_SEED") == "1":
        return
    try:
        seed_default_template(force=False)
        # Also seed the review template — same lifecycle, same gate.
        seed_review_template(force=False)
    except Exception as exc:  # noqa: BLE001
        # Seeding is best-effort during migrate; failures should not abort the
        # migrate command. Operators can re-run via management command.
        if verbosity:
            print(f"[prompting] default template seed skipped: {exc}")
