# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Prompt section registry.

The unit of prompt content is a **section**: a markdown + Jinja file under
``prompting/sections/`` carrying YAML-ish front-matter (``key``, ``title``,
``customizable``). Recipes (``prompting/recipes.py``) reference sections by
key and define their order per prompt kind; the composer
(``prompting/composer.py``) resolves and assembles them at compose time.

Default section bodies live in code and evolve through code review — exactly
like the fragments they replace. The DB only ever stores *overrides*
(``prompting.models.PromptSectionOverride``), never defaults, so there is no
seed/sync step.

See ``.ai_design/prompt_section_system/design.md`` §3.
"""

from __future__ import annotations

from dataclasses import dataclass
from pathlib import Path

SECTIONS_DIR = Path(__file__).resolve().parent / "sections"

#: Allowed values for a section's ``customizable`` front-matter field — the
#: three governance tiers (design §9.2):
#:
#: - ``locked``      — nobody edits the body; the registry default always wins
#:   (e.g. ``pidash-cli``, ``guardrails``).
#: - ``workspace``   — a workspace **admin** may set a workspace-level override,
#:   but individual members may **not** keep a personal override. The whole
#:   workspace (and all automatic runs) shares the admin's value.
#: - ``overridable`` — fully open: a member may keep a **personal** override for
#:   their own human-triggered runs, and an admin may set the workspace default.
CUSTOMIZABLE_LOCKED = "locked"
CUSTOMIZABLE_WORKSPACE = "workspace"
CUSTOMIZABLE_OVERRIDABLE = "overridable"
_VALID_CUSTOMIZABLE = frozenset(
    {CUSTOMIZABLE_LOCKED, CUSTOMIZABLE_WORKSPACE, CUSTOMIZABLE_OVERRIDABLE}
)


def tier_allows_workspace_override(tier: str) -> bool:
    """Whether a workspace admin may set a workspace-level override at ``tier``.

    Single source of truth for the tier→capability rule. Takes the *tier
    string* (not a section) so it applies equally to a section's declared tier
    and to the workspace-effective tier from ``effective_customizability``.
    """
    return tier in (CUSTOMIZABLE_WORKSPACE, CUSTOMIZABLE_OVERRIDABLE)


def tier_allows_personal_override(tier: str) -> bool:
    """Whether a member may keep a personal (user-scope) override at ``tier``."""
    return tier == CUSTOMIZABLE_OVERRIDABLE

#: Only files following the ``<key>.md`` convention are loaded. A stray
#: README.md / NOTES.md without valid front-matter raises at import time
#: rather than being silently skipped.
_SECTION_GLOB = "*.md"

#: Upper bound on an override body, mirroring the legacy template cap. Enforced
#: at the API boundary (serializer), restated here as the shared constant.
MAX_SECTION_BODY_LENGTH = 100_000


class PromptRegistryError(Exception):
    """Raised when the on-disk section registry is malformed."""


@dataclass(frozen=True)
class PromptSection:
    """One registry section: identity, metadata, and default body."""

    key: str
    title: str
    customizable: str
    default_body: str

    @property
    def is_locked(self) -> bool:
        return self.customizable == CUSTOMIZABLE_LOCKED

    @property
    def is_overridable(self) -> bool:
        """True only for the fully-open tier (members may personal-override)."""
        return self.customizable == CUSTOMIZABLE_OVERRIDABLE

    @property
    def allows_workspace_override(self) -> bool:
        """Whether a workspace admin may set a workspace-level override."""
        return tier_allows_workspace_override(self.customizable)

    @property
    def allows_personal_override(self) -> bool:
        """Whether a member may keep a personal (user-scope) override."""
        return tier_allows_personal_override(self.customizable)


def _parse_front_matter(path: Path) -> PromptSection:
    """Parse a ``<key>.md`` section file with a leading ``---`` front-matter
    block. Deliberately a tiny hand-rolled parser (key: value lines) so the
    registry has no YAML dependency and the format stays trivially auditable.
    """
    text = path.read_text(encoding="utf-8")
    if not text.startswith("---"):
        raise PromptRegistryError(
            f"section {path.name!r} is missing the leading '---' front-matter block"
        )
    # Split off the front-matter between the first two '---' fences.
    parts = text.split("---", 2)
    if len(parts) < 3:
        raise PromptRegistryError(
            f"section {path.name!r} has a malformed front-matter block "
            "(expected '---\\n<meta>\\n---\\n<body>')"
        )
    _, raw_meta, body = parts
    meta: dict[str, str] = {}
    for line in raw_meta.strip().splitlines():
        line = line.strip()
        if not line or line.startswith("#"):
            continue
        if ":" not in line:
            raise PromptRegistryError(
                f"section {path.name!r} has an invalid front-matter line: {line!r}"
            )
        field, _, value = line.partition(":")
        meta[field.strip()] = value.strip()

    missing = {"key", "title", "customizable"} - meta.keys()
    if missing:
        raise PromptRegistryError(
            f"section {path.name!r} is missing front-matter field(s): "
            f"{', '.join(sorted(missing))}"
        )
    customizable = meta["customizable"]
    if customizable not in _VALID_CUSTOMIZABLE:
        raise PromptRegistryError(
            f"section {path.name!r} has invalid customizable={customizable!r} "
            f"(expected one of {sorted(_VALID_CUSTOMIZABLE)})"
        )
    key = meta["key"]
    if path.stem != key:
        raise PromptRegistryError(
            f"section file {path.name!r} does not match its front-matter "
            f"key={key!r} (filename stem must equal the key)"
        )
    # Body: drop a single leading newline left by the closing '---\n', keep the
    # rest verbatim. Trailing whitespace is normalized to a single newline so
    # the composer's join is deterministic regardless of editor settings.
    body = body.lstrip("\n").rstrip("\n") + "\n"
    return PromptSection(
        key=key, title=meta["title"], customizable=customizable, default_body=body
    )


def _load_registry() -> dict[str, PromptSection]:
    if not SECTIONS_DIR.is_dir():
        raise PromptRegistryError(f"sections directory not found: {SECTIONS_DIR}")
    registry: dict[str, PromptSection] = {}
    for path in sorted(SECTIONS_DIR.glob(_SECTION_GLOB)):
        section = _parse_front_matter(path)
        if section.key in registry:
            raise PromptRegistryError(f"duplicate section key: {section.key!r}")
        registry[section.key] = section
    if not registry:
        raise PromptRegistryError(f"no sections found under {SECTIONS_DIR}")
    return registry


#: The loaded registry. Parsed once at import; malformed front-matter raises
#: immediately (fail-loud at startup rather than at first render).
REGISTRY: dict[str, PromptSection] = _load_registry()


def get_section(key: str) -> PromptSection:
    try:
        return REGISTRY[key]
    except KeyError as exc:
        raise PromptRegistryError(f"unknown section key: {key!r}") from exc


def all_sections() -> list[PromptSection]:
    """Registry sections in stable (key-sorted) order."""
    return [REGISTRY[k] for k in sorted(REGISTRY)]
