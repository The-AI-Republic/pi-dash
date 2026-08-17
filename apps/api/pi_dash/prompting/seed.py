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

Issue: {{ issue.title }}
Issue Description: {{ issue.description }}
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


TEST_TEMPLATE_NAME = "test"

#: Polymorphic test prompt. At runtime the agent picks the right test
#: kind from ``parent_done_payload`` (and working-tree inspection) and
#: runs the cycle for that kind, then reports results as a structured
#: issue comment. See ``.ai_design/create_test_state/design.md`` §4.7 / §5.
TEST_TEMPLATE_BODY = """\
You are testing the work product of a previous implementation pass.
"Testing" means different things depending on what was produced.

Issue: {{ issue.title }}
Issue Description: {{ issue.description }}
Recent activity:
{{ comments_section }}
Latest implementation run output (read this carefully — it is your
authoritative record of what was produced, including any acceptance
criteria, pr_url, or design_doc_paths it reported):
{{ parent_done_payload }}

Step 1 — Decide what kind of testing this is.
Inspect parent_done_payload, the issue description, and the working
tree. Choose ONE:
  (a) AUTOMATED — the issue produced code (a PR / feature branch).
      Check out the branch and run the repo's own gates: pnpm check
      (lint + types), the targeted unit/integration tests, pnpm
      build. Add missing tests for the changed surface.
  (b) UI / EXPLORATORY — a frontend change whose value is visual /
      interactive. Launch the app, drive the changed flow, check the
      acceptance criteria by observation. If you cannot boot the app
      or drive a browser in this environment, say so and emit
      `blocked` — do not report a false pass.
  (c) OPS / INFRA — a config / deploy artifact. Dry-run, validate
      config, health-check, confirm idempotency.
  (d) DESIGN — a design / doc deliverable. Verify it against its
      acceptance criteria: internal consistency, open questions
      resolved, testability of what it proposes.
  (e) NON_TECHNICAL / GENERIC — none of the above. Verify the
      deliverable against the stated acceptance criteria, one by
      one, and report a pass/fail assessment for a human to confirm.

If the acceptance criteria are ambiguous or absent, derive a plan
from the description, STATE YOUR ASSUMPTIONS in your comment, and
test against them — or emit `paused` to ask if the deliverable is
high-stakes.

Step 2 — Run the test cycle (uniform across kinds):
  i.   Derive a test plan from the acceptance criteria.
  ii.  Set up the environment for the chosen kind.
  iii. Execute.
  iv.  Collect evidence (test output, coverage, logs, screenshots).
  v.   Validate your findings — re-run / confirm; never report a
       hallucinated failure.
  vi.  Post a STRUCTURED results comment to the pidash issue:
        - Kind detected and what was tested (scope).
        - Method — commands run / flows exercised.
        - Result — pass/fail per acceptance criterion, with evidence.
        - Defects found — and whether you auto-fixed (pushed to the
          PR branch) or it needs a human / a follow-up issue.
  vii. Optionally push trivial fixes to the PR branch, or file a
       follow-up issue for real defects.

Step 3 — Emit a done-signal.
- `completed` = all tests pass / acceptance criteria met.
- `blocked`   = real defects found that need a human/dev, OR the
                test couldn't be run (missing env / creds / tooling).
- `paused`    = acceptance criteria ambiguous — ask the human.
- `noop`      = nothing changed since the last test pass.
"""


def read_review_body() -> str:
    """Return the review template body."""
    return REVIEW_TEMPLATE_BODY


def read_test_body() -> str:
    """Return the test template body."""
    return TEST_TEMPLATE_BODY


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


def seed_test_template(force: bool = False) -> str:
    """Create or (if ``force``) refresh the global ``test``
    PromptTemplate row. Returns ``"created"`` / ``"refreshed"`` /
    ``"skipped"``.
    """
    from pi_dash.prompting.models import PromptTemplate

    body = read_test_body()
    existing = (
        PromptTemplate.objects.filter(
            workspace__isnull=True, name=TEST_TEMPLATE_NAME
        )
        .order_by("-updated_at")
        .first()
    )
    if existing is None:
        PromptTemplate.objects.create(
            workspace=None,
            name=TEST_TEMPLATE_NAME,
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
        # Also seed the review + test templates — same lifecycle, same gate.
        seed_review_template(force=False)
        seed_test_template(force=False)
    except Exception as exc:  # noqa: BLE001
        # Seeding is best-effort during migrate; failures should not abort the
        # migrate command. Operators can re-run via management command.
        if verbosity:
            print(f"[prompting] default template seed skipped: {exc}")
