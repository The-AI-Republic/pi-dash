# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Tests for the fragment assembler.

Covers the assembly contract directly (ordering, required section headers,
glob scope) so regressions in fragment layout surface without needing the
full composer + Issue fixture stack.
"""

from __future__ import annotations

import pytest

from pi_dash.prompting.fragments import FRAGMENTS_DIR, assemble, fragment_paths
from pi_dash.prompting.renderer import render


@pytest.mark.unit
def test_fragment_paths_sorted_and_nonempty():
    paths = fragment_paths()
    assert paths, "no fragments discovered"
    names = [p.name for p in paths]
    assert names == sorted(names), f"fragments not in lexical order: {names}"


@pytest.mark.unit
def test_fragment_paths_glob_rejects_non_numeric_prefix(tmp_path, monkeypatch):
    # A stray README.md in the fragments directory must not get concatenated
    # into the production prompt. Simulate it by pointing FRAGMENTS_DIR at a
    # tempdir mirror with one valid and one invalid filename.
    (tmp_path / "01_ok.md").write_text("valid", encoding="utf-8")
    (tmp_path / "README.md").write_text("should be ignored", encoding="utf-8")
    (tmp_path / "NOTES.md").write_text("should be ignored", encoding="utf-8")

    from pi_dash.prompting import fragments as fragments_mod

    monkeypatch.setattr(fragments_mod, "FRAGMENTS_DIR", tmp_path)
    discovered = [p.name for p in fragments_mod.fragment_paths()]
    assert discovered == ["01_ok.md"]


@pytest.mark.unit
def test_assemble_orders_sections_and_includes_cli_section():
    body = assemble()

    # Section headers in expected order.
    expected_order = [
        "## Session framing",
        "## Pi Dash CLI",
        "## Default posture",
        "## Autonomy / escalation model",
        "## State routing",
        "## Step 1 — Workpad setup",
        "## Step 2 — Implementation and validation",
        "## Blocking the run",
        "## Guardrails",
        "## Workpad template",
        "## Ending the run",
    ]
    positions = [body.find(h) for h in expected_order]
    assert all(p >= 0 for p in positions), (
        f"missing section headers: "
        f"{[h for h, p in zip(expected_order, positions) if p < 0]}"
    )
    assert positions == sorted(positions), (
        f"section headers out of order: "
        f"{list(zip(expected_order, positions))}"
    )


@pytest.mark.unit
def test_assemble_contains_expanded_pidash_cli_subsections():
    body = assemble()
    # Subsections added when the CLI fragment was expanded — a regression
    # here means somebody dropped or reordered CLI guidance.
    for marker in (
        "### Environment",
        "### Output contract",
        "### Not for you",
        "### Typical recipes",
        "pidash comment update <identifier> <comment-id>",
    ):
        assert marker in body, f"missing CLI subsection marker: {marker!r}"


@pytest.mark.unit
def test_assemble_preserves_unindented_jinja_block_tags():
    # Renderer runs with lstrip_blocks=False, so leading whitespace on
    # {% if %} / {% endif %} / {% else %} / {% endfor %} lines bleeds into
    # rendered output. Block tags must start at column 0.
    body = assemble()
    for line_no, line in enumerate(body.splitlines(), start=1):
        stripped = line.lstrip()
        if stripped.startswith("{%") and stripped != line:
            # Allow the nested `{% if repo.base_branch %}...{% endif %}` on
            # the "Resolve the base branch" bullet, which is an inline
            # expression, not a block tag on its own line.
            assert "{% if repo.base_branch %}" in line, (
                f"fragment line {line_no} has indented block tag: {line!r}"
            )


@pytest.mark.unit
def test_fragments_directory_exists_on_disk():
    assert FRAGMENTS_DIR.is_dir(), f"fragments dir missing: {FRAGMENTS_DIR}"


def _base_ctx(*, parent=None, work_branch=None, base_branch="trunk"):
    """Minimal context shape needed to render the assembled fragment body.

    Mirrors the keys produced by ``prompting.context.build_context`` that are
    referenced by the workpad-setup and implementation fragments.
    """
    return {
        "issue": {
            "identifier": "TP-7",
            "title": "Make button blue",
            "priority": "high",
            "state": "Todo",
            "state_group": "started",
            "description": "",
        },
        "project": {"identifier": "TP", "name": "Test Project"},
        "repo": {
            "url": "git@github.com:acme/web.git",
            "base_branch": base_branch,
            "work_branch": work_branch,
        },
        "parent": parent,
        "run": {"id": "00000000", "attempt": 1, "turn_number": 1},
        "workspace": {"slug": "test-workspace"},
    }


@pytest.mark.unit
def test_assemble_renders_independent_issue_uses_project_base():
    body = render(assemble(), _base_ctx())
    # No parent → independent path: BASE comes from project base branch.
    assert "BASE=trunk" in body
    # And the PR-base prose in Step 2 matches.
    assert "`trunk`" in body


@pytest.mark.unit
def test_assemble_renders_parent_with_work_branch_uses_parent_branch():
    parent = {
        "identifier": "TP-1",
        "title": "Umbrella",
        "work_branch": "pi-dash/tp-1",
    }
    body = render(assemble(), _base_ctx(parent=parent))
    # Parent w/ branch → BASE points at the parent's branch.
    assert "BASE=pi-dash/tp-1" in body
    # PR-base prose in Step 2 references the parent's branch, not the project base.
    assert "`pi-dash/tp-1`" in body


@pytest.mark.unit
def test_assemble_renders_parent_without_work_branch_falls_back_to_project_base():
    parent = {"identifier": "TP-1", "title": "Umbrella", "work_branch": None}
    body = render(assemble(), _base_ctx(parent=parent))
    # Parent w/o branch → fall back to project base.
    assert "BASE=trunk" in body
    # And the fallback note must be present so the agent records it.
    assert "Fall back to the project base branch" in body


@pytest.mark.unit
def test_assemble_renders_existing_work_branch_skips_base_resolution():
    body = render(assemble(), _base_ctx(work_branch="feat/pinned"))
    # repo.work_branch path → checkout existing branch directly, no BASE= resolution.
    assert "git checkout feat/pinned" in body
    assert "BASE=" not in body
