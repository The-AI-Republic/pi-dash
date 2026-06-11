# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Prompt composition.

Compose-time assembly: a prompt *kind* (coding-task / review / scheduler) names
a recipe (ordered section keys); each section resolves through the override
chain (user → workspace → registry default); the resolved bodies are
concatenated into one template body and rendered once via the sandboxed Jinja
environment.

Caller-facing entrypoints:

- ``build_first_turn(issue, run)`` — issue-scoped runs (coding-task / review).
- ``build_scheduler_turn(binding, run)`` — project-scoped scheduler runs.
- ``compose(...)`` / ``compile_template(...)`` — used by the visibility/preview
  endpoints to assemble without creating a run.

See ``.ai_design/prompt_section_system/design.md`` §6, §7.
"""

from __future__ import annotations

from dataclasses import dataclass
from typing import Any, Dict, List, Optional

from pi_dash.prompting import recipes, registry
from pi_dash.prompting.renderer import PromptRenderError, render

#: Manifest/source labels.
SOURCE_DEFAULT = "default"
SOURCE_WORKSPACE = "workspace"


@dataclass(frozen=True)
class ResolvedSection:
    """A section after override resolution, ready to assemble."""

    key: str
    title: str
    customizable: str
    body: str
    source: str  # "default" | "workspace" | "user:<id>"
    version: int  # 0 for the registry default


@dataclass(frozen=True)
class ManifestEntry:
    """One section's provenance + position in the assembled template body."""

    section_key: str
    source: str
    version: int
    line_start: int
    line_end: int

    def to_dict(self) -> Dict[str, Any]:
        return {
            "section_key": self.section_key,
            "source": self.source,
            "version": self.version,
            "line_start": self.line_start,
            "line_end": self.line_end,
        }


@dataclass(frozen=True)
class ComposedPrompt:
    """Result of composing a kind: rendered text + provenance + raw template."""

    text: str
    manifest: List[ManifestEntry]
    template_body: str
    resolved: List[ResolvedSection]

    @property
    def manifest_dicts(self) -> List[Dict[str, Any]]:
        return [e.to_dict() for e in self.manifest]


def effective_customizability(section: registry.PromptSection, workspace) -> str:
    """Return the customizability that actually applies for ``workspace``.

    Returns the registry flag in v1. The indirection is the §9.2 seam: a
    per-workspace admin-lock tier (open / workspace-only / locked) lands as a
    change to this one function without touching the resolver or callers.
    """
    return section.customizable


def _active_override(workspace, user, section_key: str):
    """Fetch the active override row for a scope, or ``None``.

    ``user is None`` selects the workspace-level row (``user IS NULL``).
    """
    from pi_dash.prompting.models import PromptSectionOverride

    qs = PromptSectionOverride.objects.filter(
        workspace=workspace, section_key=section_key, is_active=True
    )
    qs = qs.filter(user__isnull=True) if user is None else qs.filter(user=user)
    return qs.order_by("-updated_at").first()


def resolve_section(key: str, *, workspace, project, user) -> ResolvedSection:
    """Resolve one section's body via the precedence chain.

    user override → workspace override → registry default. Locked sections
    skip the chain entirely. ``project`` is accepted (every call site has one)
    and ignored in v1 — the §9.4 seam for a future project-level rung.
    """
    section = registry.get_section(key)
    default = ResolvedSection(
        key=section.key,
        title=section.title,
        customizable=section.customizable,
        body=section.default_body,
        source=SOURCE_DEFAULT,
        version=0,
    )
    if effective_customizability(section, workspace) == registry.CUSTOMIZABLE_LOCKED:
        return default
    if workspace is None:
        # No workspace context (e.g. a global preview): defaults only.
        return default

    if user is not None:
        row = _active_override(workspace, user, key)
        if row is not None:
            return ResolvedSection(
                key=section.key,
                title=section.title,
                customizable=section.customizable,
                body=row.body,
                source=f"user:{user.id}",
                version=row.version,
            )
    row = _active_override(workspace, None, key)
    if row is not None:
        return ResolvedSection(
            key=section.key,
            title=section.title,
            customizable=section.customizable,
            body=row.body,
            source=SOURCE_WORKSPACE,
            version=row.version,
        )
    return default


def _assemble(resolved: List[ResolvedSection]) -> tuple[str, List[ManifestEntry]]:
    """Concatenate resolved sections, tracking each one's line range.

    Mirrors the legacy ``fragments.assemble``: strip each body, join with a
    blank line, append a trailing newline. The line ranges index the assembled
    (pre-render) template so a Jinja error lineno maps back to its section.
    """
    parts = [r.body.strip() for r in resolved]
    template_body = "\n\n".join(parts) + "\n"
    manifest: List[ManifestEntry] = []
    line = 1
    for r, part in zip(resolved, parts):
        n_lines = part.count("\n") + 1
        line_start = line
        line_end = line + n_lines - 1
        manifest.append(
            ManifestEntry(
                section_key=r.key,
                source=r.source,
                version=r.version,
                line_start=line_start,
                line_end=line_end,
            )
        )
        # "\n\n" join inserts exactly one blank line between parts.
        line = line_end + 2
    return template_body, manifest


def _attributed_render_error(
    exc: PromptRenderError, manifest: List[ManifestEntry]
) -> PromptRenderError:
    """Re-wrap a render failure with section/source attribution (§6.3)."""
    cause = exc.__cause__
    lineno = getattr(cause, "lineno", None) or getattr(exc, "lineno", None)
    culprit: Optional[ManifestEntry] = None
    if lineno:
        for entry in manifest:
            if entry.line_start <= lineno <= entry.line_end:
                culprit = entry
                break
    detail = str(exc)
    if culprit is not None:
        return PromptRenderError(
            f"section '{culprit.section_key}' (source={culprit.source}, "
            f"v{culprit.version}) failed to render: {detail}"
        )
    overridden = [e for e in manifest if e.source != SOURCE_DEFAULT]
    if overridden:
        srcs = ", ".join(f"{e.section_key}({e.source})" for e in overridden)
        return PromptRenderError(
            f"prompt render failed (active overrides: {srcs}): {detail}"
        )
    return PromptRenderError(detail)


def compose(
    kind: str, *, workspace, project, user, context: Dict[str, Any]
) -> ComposedPrompt:
    """Resolve, assemble, and render the recipe for ``kind``.

    Raises :class:`PromptRenderError` (attributed to the failing section) when
    rendering fails — the caller fails the run cleanly, never a 500.
    """
    recipe = recipes.recipe_for(kind)
    resolved = [
        resolve_section(key, workspace=workspace, project=project, user=user)
        for key in recipe
    ]
    template_body, manifest = _assemble(resolved)
    try:
        text = render(template_body, context)
    except PromptRenderError as exc:
        raise _attributed_render_error(exc, manifest) from exc
    return ComposedPrompt(
        text=text, manifest=manifest, template_body=template_body, resolved=resolved
    )


def compile_template(kind: str, *, workspace, project, user) -> ComposedPrompt:
    """Assemble the recipe for ``kind`` **without rendering** (Jinja markers
    intact). Powers the "see the final template" view (§7.2). Returns a
    :class:`ComposedPrompt` whose ``text`` is the raw assembled body.
    """
    recipe = recipes.recipe_for(kind)
    resolved = [
        resolve_section(key, workspace=workspace, project=project, user=user)
        for key in recipe
    ]
    template_body, manifest = _assemble(resolved)
    return ComposedPrompt(
        text=template_body,
        manifest=manifest,
        template_body=template_body,
        resolved=resolved,
    )


# ----------------------------------------------------------------------
# Run-facing entrypoints
# ----------------------------------------------------------------------


def _user_for_run(run) -> Optional[Any]:
    """The user whose overrides apply to ``run`` (§9.1).

    Human-triggered runs resolve with ``run.created_by``; automatic runs
    (tick, scheduler beat, system-bot) resolve workspace + defaults only.
    """
    from pi_dash.runner.models import run_is_human_triggered

    return run.created_by if run_is_human_triggered(run) else None


def build_first_turn(issue, run) -> str:
    """Render the prompt for ``run`` executing ``issue``.

    Selects the recipe by the issue's state via the phase registry, resolves
    overrides for the run's triggering user, renders, and stamps the
    composition manifest onto ``run.prompt_manifest`` (the caller must include
    ``prompt_manifest`` in its ``save(update_fields=...)``).
    """
    from pi_dash.orchestration.agent_phases import template_name_for

    template_name = template_name_for(issue.state)
    kind = recipes.kind_for(template_name)
    context = build_first_turn_context(issue, run)
    composed = compose(
        kind,
        workspace=issue.workspace,
        project=issue.project,
        user=_user_for_run(run),
        context=context,
    )
    run.prompt_manifest = composed.manifest_dicts
    return composed.text


def build_first_turn_context(issue, run) -> Dict[str, Any]:
    """Issue context for ``build_first_turn`` — thin wrapper kept so callers
    don't import ``context`` directly."""
    from pi_dash.prompting.context import build_context

    return build_context(issue, run)


def build_scheduler_turn(binding, run) -> str:
    """Render the project-scoped prompt for a scheduler ``binding`` run.

    Scheduler runs are always automatic — no user overrides apply. Stamps the
    manifest onto ``run.prompt_manifest`` like ``build_first_turn``.
    """
    from pi_dash.prompting.context import build_scheduler_context

    context = build_scheduler_context(binding, run)
    project = binding.project if binding.project_id is not None else None
    workspace = binding.workspace if binding.workspace_id is not None else None
    composed = compose(
        recipes.KIND_SCHEDULER,
        workspace=workspace,
        project=project,
        user=None,
        context=context,
    )
    run.prompt_manifest = composed.manifest_dicts
    return composed.text
