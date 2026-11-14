# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Work-type neutrality of core stage sections (PDASHOSS01-234).

The core sections of the three issue-stage recipes (In Progress / In Review /
In Test) are shared by every work type, so they must not carry software/git/PR
wording — that guidance belongs in work-type sections (``software.execute``…).

This module is the single definition of "software wording". It backs both the
unit test that keeps the shipped defaults neutral and
``revalidate_section_overrides``, which flags existing workspace/user overrides
of core sections that still carry the pre-work-type git/PR steps (they would
otherwise leak into ``general`` prompts and double up in ``software`` ones).
"""

from __future__ import annotations

import re
from typing import List, Optional

from pi_dash.prompting import recipes

#: Wording that marks text as software/git/PR-specific; matched
#: case-insensitively.
NEUTRALITY_BANNED: List[str] = [
    r"\bgit\b",
    r"\bgithub\b",
    r"\bgitlab\b",
    r"\bbranch(es|ed|ing)?\b",
    r"\bcommit(s|ted|ting)?\b",
    r"\bpush(es|ed|ing)?\b",
    r"pull request",
    r"merge request",
    r"code review",
    r"\brepo\b",
    r"\brepositor(y|ies)\b",
    r"\bcodebase\b",
    r"CLAUDE\.md",
    r"AGENTS\.md",
    r"\.ai_design",
    r"\bgh\b",
    r"\bglab\b",
    r"\brebase(s|d)?\b",
    r"\bcheckout\b",
    # Bare "merge" only: "merged set" (labels) is not a git reference.
    r"\bmerge\b",
    r"\bpipeline(s)?\b",
    r"\bdiff(s)?\b",
]

#: Acronyms matched case-sensitively (``PR`` yes, "pr" in prose no).
NEUTRALITY_BANNED_CASE_SENSITIVE: List[str] = [
    r"\bPRs?\b",
    r"\bMRs?\b",
    r"\bCI\b",
]

_COMPILED = [re.compile(p, re.IGNORECASE) for p in NEUTRALITY_BANNED] + [
    re.compile(p) for p in NEUTRALITY_BANNED_CASE_SENSITIVE
]
_PATTERNS = NEUTRALITY_BANNED + NEUTRALITY_BANNED_CASE_SENSITIVE

#: The issue-stage kinds whose recipes are composed with a work type.
STAGE_KINDS = (recipes.KIND_CODING_TASK, recipes.KIND_REVIEW, recipes.KIND_TEST)


def violations(text: str) -> List[str]:
    """The banned *patterns* ``text`` matches, in list order."""
    return [p for p, rx in zip(_PATTERNS, _COMPILED) if rx.search(text)]


def matched_terms(text: str) -> List[str]:
    """The first matched word for each banned pattern, as written in ``text``
    (pattern order, duplicates dropped) — the human-readable form of
    :func:`violations`."""
    terms: List[str] = []
    for rx in _COMPILED:
        m = rx.search(text)
        if m is not None and m.group(0) not in terms:
            terms.append(m.group(0))
    return terms


def core_stage_section_keys() -> frozenset:
    """Core (non-slot) section keys of the three stage recipes."""
    keys = set()
    for kind in STAGE_KINDS:
        for entry in recipes.RECIPES[kind]:
            if isinstance(entry, str):
                keys.add(entry)
    return frozenset(keys)


def override_neutrality_issue(section_key: str, body: str) -> Optional[str]:
    """Why an override body of ``section_key`` breaks core neutrality, or
    ``None`` when it doesn't.

    Only core stage sections are checked; work-type sections
    (``software.execute``) are where software wording belongs, and sections
    outside the stage recipes are not composed with a work type.
    """
    if section_key not in core_stage_section_keys():
        return None
    terms = matched_terms(body)
    if not terms:
        return None
    listed = ", ".join(f"'{t}'" for t in terms)
    return (
        f"override for core section {section_key!r} carries work-type-specific wording ({listed}); "
        "core sections are shared by every work type, so move that guidance into a work-type section "
        "(e.g. 'software.execute')"
    )
