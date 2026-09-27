# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Review and test intros (local + Cloud) carry the project block.

The project description is the only project-scoped prompt channel, so the
review and test phases must render it the same way ``intro.md`` does.
"""

from __future__ import annotations

import copy

import pytest

from pi_dash.prompting import registry
from pi_dash.prompting.composer import compose, compose_cloud
from pi_dash.prompting.renderer import render
from pi_dash.prompting.validation import sample_contexts

PROJECT_LINE = "This issue belongs to the Project: {{ project.name }} ({{ project.identifier }})"
DESCRIPTION = "Assume the code is wrong.\nReject stubs."


def _instructions(kind):
    return f"Project-level instructions in the description apply to this {kind} pass as well as to implementation."


def _local_block(kind):
    return (
        f"{PROJECT_LINE}\n"
        "{% if project.description %}\n"
        "{{ project.description }}\n"
        "\n"
        f"{_instructions(kind)}\n"
        "{% endif %}\n"
    )


def _cloud_block(kind):
    return (
        "\n"
        f"{PROJECT_LINE}\n"
        "{%- if project.description %}\n"
        "\n"
        "{{ project.description }}\n"
        "\n"
        f"{_instructions(kind)}\n"
        "{%- endif %}\n"
    )


# (section key, recipe kind, exact block added to the section body)
SECTIONS = [
    ("review-intro", "review", _local_block("review")),
    ("test-intro", "test", _local_block("test")),
    ("cloud-review-intro", "review", _cloud_block("review")),
    ("cloud-test-intro", "test", _cloud_block("test")),
]


def _ctx(kind, description):
    ctx = copy.deepcopy(sample_contexts(kind)[0])
    ctx["project"].update(name="Rust Port", identifier="RUST", description=description)
    return ctx


@pytest.mark.unit
@pytest.mark.parametrize("key,kind,block", SECTIONS)
def test_section_renders_project_line_and_description(key, kind, block):
    out = render(registry.get_section(key).default_body, _ctx(kind, DESCRIPTION))
    assert (f"This issue belongs to the Project: Rust Port (RUST)\n\n{DESCRIPTION}\n\n{_instructions(kind)}") in out


@pytest.mark.unit
@pytest.mark.parametrize("key,kind,block", SECTIONS)
def test_section_empty_description_renders_project_line_only(key, kind, block):
    out = render(registry.get_section(key).default_body, _ctx(kind, ""))
    assert "This issue belongs to the Project: Rust Port (RUST)" in out
    assert "Project-level instructions" not in out


@pytest.mark.unit
@pytest.mark.parametrize("description", [DESCRIPTION, ""])
@pytest.mark.parametrize("key,kind,block", SECTIONS)
def test_rest_of_section_is_unchanged(key, kind, block, description):
    """Removing the block's rendered text leaves exactly the pre-change render."""
    body = registry.get_section(key).default_body
    assert body.count(block) == 1
    before = body.replace(block, "")
    ctx = _ctx(kind, description)
    rendered_block = "This issue belongs to the Project: Rust Port (RUST)\n"
    if description:
        rendered_block += f"\n{description}\n\n{_instructions(kind)}\n"
    if key.startswith("cloud-"):
        # Appended after the intro paragraph, separated by one blank line.
        # (Jinja drops the body's single trailing newline, hence the rstrip.)
        assert render(body, ctx) == render(before, ctx).rstrip("\n") + "\n\n" + rendered_block.rstrip("\n")
    else:
        # Inserted directly above the "Issue:" line, followed by a blank line.
        anchor = "Issue: {{ issue.title }}\n"
        expected = render(before.replace(anchor, "@@BLOCK@@" + anchor), ctx)
        assert render(body, ctx) == expected.replace("@@BLOCK@@", rendered_block + "\n")


@pytest.mark.unit
@pytest.mark.parametrize("kind", ["review", "test"])
@pytest.mark.parametrize("description", [DESCRIPTION, ""])
def test_composed_local_prompt_places_block_above_issue_lines(kind, description):
    text = compose(kind, workspace=None, project=None, user=None, context=_ctx(kind, description)).text
    head = "This issue belongs to the Project: Rust Port (RUST)\n\n"
    if description:
        head += f"{description}\n\n{_instructions(kind)}\n\n"
    assert head + "Issue: " in text


@pytest.mark.unit
@pytest.mark.parametrize("kind", ["review", "test"])
@pytest.mark.parametrize("description", [DESCRIPTION, ""])
def test_composed_cloud_prompt_keeps_single_blank_line_after_block(kind, description):
    ctx = _ctx(kind, description)
    ctx["run"]["id"] = "run-1"
    ctx.setdefault("workpad_body", "")
    text = compose_cloud(kind, workspace=None, project=None, context=ctx).text
    tail = "This issue belongs to the Project: Rust Port (RUST)"
    if description:
        tail += f"\n\n{description}\n\n{_instructions(kind)}"
    assert tail + "\n\n# " in text or tail + "\n\n## " in text


@pytest.mark.unit
def test_implementation_intro_block_unchanged():
    # The review/test block copies intro.md's wording; intro.md itself stays put.
    body = registry.get_section("intro").default_body
    assert f"{PROJECT_LINE}\n{{% if project.description %}}\n{{{{ project.description }}}}\n{{% endif %}}\n" in body
    assert "Project-level instructions" not in body
