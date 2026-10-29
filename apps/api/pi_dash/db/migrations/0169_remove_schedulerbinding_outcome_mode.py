# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""PDASHOSS01-281: drop ``SchedulerBinding.outcome_mode``.

The platform no longer appends a work-mode directive to a scheduler run's
task; the scheduler prompt and the binding's extra context say what a run
does. Existing installs must keep behaving the same, so before the column
goes each binding's directive is folded into its ``extra_context``:

- ``apply_fix`` / ``fix_and_review``: always.
- ``create_issue``: unless the scheduler prompt already carries a concrete
  filing command (contains ``issue create``, case-insensitive) — both builtin
  prompts do, and repeating the directive there adds nothing.

The builtin prompts also named the CLI ``pi-dash``; the agent has ``pidash``.
Their filing command carried no ``--project`` either — the create-issue
directive supplied it — so it is added, or the CLI falls back to the
workspace default project rather than the one the scheduler is installed on.
"""

from __future__ import annotations

import logging

from django.db import migrations

logger = logging.getLogger(__name__)

#: Frozen copy of the directives the dispatcher appended up to this migration.
DIRECTIVES: dict[str, str] = {
    "create_issue": (
        "## Work mode: create issues\n\n"
        "For each distinct finding, file a Pi Dash issue with the `pidash` CLI:\n"
        "    pidash issue create --project <PROJ> --title \"<short summary>\" \\\n"
        "        --description \"<file path, line range, evidence, severity, "
        'suggested fix>"\n'
        "Before creating an issue, list existing open issues and skip any "
        "finding that already has a corresponding open issue (de-dupe by file "
        "+ root cause, not by exact title). Do NOT modify code."
    ),
    "apply_fix": (
        "## Work mode: apply fix\n\n"
        "For each finding you are confident about, implement the fix and open a "
        "pull request for human review — do NOT merge it. Keep one PR per "
        "logical fix where practical. If a fix is risky, ambiguous, or larger "
        "than a focused change, do NOT force it: create a Pi Dash issue "
        "describing the finding instead (same form as create-issue mode)."
    ),
    "fix_and_review": (
        "## Work mode: file issue and delegate fix\n\n"
        "Do NOT modify code or open a pull request in this run — the fix is "
        "delegated to the issue agent. For each distinct finding, do ALL of "
        "the following:\n"
        "1. File a Pi Dash issue with the `pidash` CLI (de-dupe against existing "
        "open issues by file + root cause, as in create-issue mode), and note "
        "the issue identifier it returns. Write the description so an AI agent "
        "can implement the fix without re-investigating: file path(s) and line "
        "range, the evidence you observed, root cause, severity, a concrete "
        "suggested fix, and how to validate it:\n"
        "    pidash issue create --project <PROJ> --title \"<short summary>\" \\\n"
        "        --description \"<agent-ready technical details>\"\n"
        "2. Move the issue to In Progress — this automatically delegates it to "
        "the coding agent, which implements the fix and opens a pull request "
        "for human review:\n"
        "    pidash issue patch <IDENT> --state \"In Progress\"\n"
        "If a finding is risky, ambiguous, or larger than a focused change, "
        "still file the issue but leave it in its default state (do NOT move "
        "it to In Progress) and describe the open questions in the issue "
        "description instead."
    ),
}

DEFAULT_MODE = "create_issue"
BUILTIN_SLUGS = ("security-audit", "fable-security-audit")
CLI_RENAMES = (
    ("`pi-dash` CLI", "`pidash` CLI"),
    ("    pi-dash issue create", "    pidash issue create"),
    (
        "    pidash issue create \\\n      --title",
        "    pidash issue create \\\n      --project <this project's identifier> \\\n      --title",
    ),
)


def refreshed_builtin_prompt(prompt: str) -> str:
    """A stored builtin prompt with the CLI name and ``--project`` fixed."""
    for old, new in CLI_RENAMES:
        prompt = prompt.replace(old, new)
    return prompt


def prompt_files_issues(prompt: str) -> bool:
    return "issue create" in (prompt or "").lower()


def migrated_extra_context(mode: str, prompt: str, extra_context: str) -> str:
    """``extra_context`` that reproduces the pre-migration task body."""
    # An unknown mode dispatched the create-issue directive.
    directive = DIRECTIVES.get(mode, DIRECTIVES[DEFAULT_MODE])
    if directive is DIRECTIVES[DEFAULT_MODE] and prompt_files_issues(prompt):
        return extra_context or ""
    return "\n\n".join(p for p in ((extra_context or "").strip(), directive) if p)


def split_extra_context(extra_context: str) -> tuple[str, str]:
    """Reverse of :func:`migrated_extra_context`: ``(mode, extra_context)``."""
    text = extra_context or ""
    for mode, directive in DIRECTIVES.items():
        if text.endswith(directive):
            return mode, text[: -len(directive)].rstrip()
    return DEFAULT_MODE, text


def fold_directive_into_extra_context(apps, schema_editor):
    Scheduler = apps.get_model("db", "Scheduler")
    SchedulerBinding = apps.get_model("db", "SchedulerBinding")

    for scheduler in Scheduler.objects.filter(source="builtin", slug__in=BUILTIN_SLUGS):
        prompt = refreshed_builtin_prompt(scheduler.prompt)
        if prompt != scheduler.prompt:
            scheduler.prompt = prompt
            scheduler.save(update_fields=["prompt"])

    counts: dict[str, int] = {}
    rows = SchedulerBinding.objects.select_related("scheduler").only(
        "id", "outcome_mode", "extra_context", "scheduler__prompt"
    )
    for binding in rows.iterator():
        new = migrated_extra_context(binding.outcome_mode, binding.scheduler.prompt, binding.extra_context)
        changed = new != (binding.extra_context or "")
        key = f"{binding.outcome_mode}:{'appended' if changed else 'unchanged'}"
        counts[key] = counts.get(key, 0) + 1
        if changed:
            SchedulerBinding.objects.filter(pk=binding.pk).update(extra_context=new)
    logger.info("0169 scheduler outcome_mode fold: %s", dict(sorted(counts.items())) or "no bindings")


def restore_outcome_mode(apps, schema_editor):
    SchedulerBinding = apps.get_model("db", "SchedulerBinding")
    for binding in SchedulerBinding.objects.only("id", "extra_context").iterator():
        mode, extra_context = split_extra_context(binding.extra_context)
        if mode != DEFAULT_MODE or extra_context != (binding.extra_context or ""):
            SchedulerBinding.objects.filter(pk=binding.pk).update(outcome_mode=mode, extra_context=extra_context)


class Migration(migrations.Migration):
    dependencies = [
        ("db", "0168_ticker_stop_signal_disarm_reason"),
    ]

    operations = [
        migrations.RunPython(fold_directive_into_extra_context, restore_outcome_mode),
        migrations.RemoveField(
            model_name="schedulerbinding",
            name="outcome_mode",
        ),
    ]
