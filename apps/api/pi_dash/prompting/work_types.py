# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Work types — per-work-type prompt guidance plugged into stage recipes.

A **work type** describes *what kind of task* an issue is (``software``,
``general``, later custom types); the **stage** (In Progress / In Review /
In Test) describes *where in the workflow* it sits. The stage keeps selecting
the prompt kind, the recipe, ``phase_kind``, ticking, and the outcome guard —
the work type only decides which sections fill the recipe's named slots
(:class:`~pi_dash.prompting.recipes.Slot`).

On disk a work type is a folder::

    prompting/work_types/<key>/
        work_type.md   # metadata: key, title, description (not a section)
        context.md     # fills Slot("context")   — optional
        execute.md     # fills Slot("execute")   — optional
        review.md      # fills Slot("review")    — optional
        test.md        # fills Slot("test")      — optional

The slot files are ordinary registry sections with namespaced keys
(``software.execute``), so overrides, tiers, save-time validation, compiled
preview, and the manifest treat them like any other section. A work type may
leave a slot empty; a missing slot falls back to the ``general`` work type's
section for that slot when one exists.

See the design in issue PDASHOSS01-234 (supersedes
``.ai_design/prompt_section_system/design.md`` §9.5's deferred "work kind").
"""

from __future__ import annotations

from dataclasses import dataclass, field
from types import MappingProxyType
from typing import Mapping

from pi_dash.prompting import recipes, registry

#: Slot names a work type may fill — must match the ``Slot`` entries used in
#: ``recipes.RECIPES``.
SLOT_NAMES = ("context", "execute", "review", "test")

#: Built-in work-type keys.
WORK_TYPE_SOFTWARE = "software"
WORK_TYPE_GENERAL = "general"

#: The work type everything resolves to until per-project/per-issue selection
#: ships (and the safety net after it): today's behavior, unchanged.
DEFAULT_WORK_TYPE = WORK_TYPE_SOFTWARE


class WorkTypeError(Exception):
    """Raised when the on-disk work-type registry is malformed, or an unknown
    work-type key is requested."""


@dataclass(frozen=True)
class WorkType:
    """One work type: identity, metadata, and its slot → section-key map."""

    key: str
    title: str
    description: str
    #: slot name → registry section key (``"software.execute"``); only the
    #: slots this work type actually supplies are present.
    sections: Mapping[str, str] = field(default_factory=dict)


def _parse_meta(path, expected_key: str) -> dict[str, str]:
    """Parse a ``work_type.md`` metadata file (same tiny front-matter format
    as sections; the body, if any, is ignored)."""
    text = path.read_text(encoding="utf-8")
    parts = text.split("---", 2)
    if not text.startswith("---") or len(parts) < 3:
        raise WorkTypeError(f"work type {expected_key!r}: {path.name} is missing the '---' front-matter block")
    meta: dict[str, str] = {}
    for line in parts[1].strip().splitlines():
        line = line.strip()
        if not line or line.startswith("#"):
            continue
        if ":" not in line:
            raise WorkTypeError(f"work type {expected_key!r}: invalid front-matter line: {line!r}")
        field_name, _, value = line.partition(":")
        meta[field_name.strip()] = value.strip()
    missing = {"key", "title", "description"} - meta.keys()
    if missing:
        raise WorkTypeError(f"work type {expected_key!r}: work_type.md missing field(s): {', '.join(sorted(missing))}")
    if meta["key"] != expected_key:
        raise WorkTypeError(f"work type folder {expected_key!r} does not match its work_type.md key={meta['key']!r}")
    return meta


def _load_work_types() -> dict[str, WorkType]:
    if not registry.WORK_TYPES_DIR.is_dir():
        raise WorkTypeError(f"work types directory not found: {registry.WORK_TYPES_DIR}")
    out: dict[str, WorkType] = {}
    for wt_dir in sorted(p for p in registry.WORK_TYPES_DIR.iterdir() if p.is_dir()):
        key = wt_dir.name
        meta_path = wt_dir / registry.WORK_TYPE_META_FILENAME
        if not meta_path.is_file():
            raise WorkTypeError(f"work type {key!r} has no {registry.WORK_TYPE_META_FILENAME}")
        meta = _parse_meta(meta_path, key)
        sections: dict[str, str] = {}
        for path in sorted(wt_dir.glob("*.md")):
            if path.name == registry.WORK_TYPE_META_FILENAME:
                continue
            slot = path.stem
            if slot not in SLOT_NAMES:
                raise WorkTypeError(
                    f"work type {key!r} has section file {path.name!r} which is not a slot "
                    f"(expected one of {sorted(SLOT_NAMES)})"
                )
            section_key = f"{key}.{slot}"
            # The registry loaded the same file; fail loudly if it didn't.
            registry.get_section(section_key)
            sections[slot] = section_key
        out[key] = WorkType(
            key=key,
            title=meta["title"],
            description=meta["description"],
            sections=MappingProxyType(sections),
        )
    if DEFAULT_WORK_TYPE not in out:
        raise WorkTypeError(f"the default work type {DEFAULT_WORK_TYPE!r} is not defined on disk")
    return out


#: The loaded work types. Parsed once at import; malformed folders raise
#: immediately (fail-loud at startup rather than at first compose).
WORK_TYPES: dict[str, WorkType] = _load_work_types()


def get_work_type(key: str) -> WorkType:
    try:
        return WORK_TYPES[key]
    except KeyError as exc:
        raise WorkTypeError(f"unknown work type: {key!r}") from exc


def all_work_types() -> list[WorkType]:
    """Work types in stable (key-sorted) order."""
    return [WORK_TYPES[k] for k in sorted(WORK_TYPES)]


def section_key_for_slot(work_type: WorkType, slot_name: str) -> str | None:
    """The section key filling ``slot_name`` for ``work_type``, or ``None``.

    A work type that doesn't supply a slot falls back to the ``general`` work
    type's section for it (when ``general`` exists and supplies one) — so a
    custom type can define only what it specializes.
    """
    key = work_type.sections.get(slot_name)
    if key is None and work_type.key != WORK_TYPE_GENERAL:
        fallback = WORK_TYPES.get(WORK_TYPE_GENERAL)
        if fallback is not None:
            key = fallback.sections.get(slot_name)
    return key


def expand(recipe: tuple, work_type_key: str) -> tuple[str, ...]:
    """Resolve a recipe's :class:`~pi_dash.prompting.recipes.Slot` entries to
    ``work_type_key``'s section keys; unfilled slots are dropped.

    Recipes without slots (scheduler, cloud) pass through unchanged.
    """
    work_type = get_work_type(work_type_key)
    out: list[str] = []
    for entry in recipe:
        if isinstance(entry, recipes.Slot):
            section_key = section_key_for_slot(work_type, entry.name)
            if section_key is not None:
                out.append(section_key)
        else:
            out.append(entry)
    return tuple(out)


def effective_work_type(issue) -> str:
    """The work-type key that applies to ``issue``.

    ``issue.work_type`` (explicit per-issue override) → the project's
    ``default_work_type`` → :data:`DEFAULT_WORK_TYPE`. The attribute reads are
    defensive so the resolver already works while the model fields ship in a
    later part. An unknown stored key is treated as absent — the next rung of
    the chain applies — rather than failing the run: composing with the
    project's (or the default) guidance beats not running.
    """
    for key in (
        getattr(issue, "work_type", None),
        getattr(getattr(issue, "project", None), "default_work_type", None),
    ):
        if key and key in WORK_TYPES:
            return key
    return DEFAULT_WORK_TYPE
