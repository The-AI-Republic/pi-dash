# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

import pytest

from pi_dash.prompting.renderer import PromptRenderError, render


@pytest.mark.unit
def test_render_substitutes_variables():
    body = "Hello {{ name }} — {{ count }} items."
    out = render(body, {"name": "Rich", "count": 3})
    assert out == "Hello Rich — 3 items."


@pytest.mark.unit
def test_render_missing_variable_raises():
    with pytest.raises(PromptRenderError):
        render("Hi {{ who }}", {})


@pytest.mark.unit
def test_render_rejects_attribute_access_to_python_internals():
    # SandboxedEnvironment should refuse dunder traversal that would let a
    # template author reach the interpreter.
    body = "{{ obj.__class__.__mro__ }}"
    with pytest.raises(PromptRenderError):
        render(body, {"obj": object()})
