# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Sandboxed Jinja2 renderer.

Templates are workspace-admin-editable; rendering them in the default Jinja
environment would let any admin pivot to an RCE via attribute traversal. The
`SandboxedEnvironment` denies access to Python internals, and we additionally
disallow filesystem loaders (everything is `from_string`).
"""

from __future__ import annotations

from typing import Any, Dict

from jinja2 import StrictUndefined, TemplateError, TemplateSyntaxError
from jinja2.sandbox import SandboxedEnvironment


class PromptRenderError(Exception):
    """Raised when a prompt template fails to render."""


class PromptSyntaxError(PromptRenderError):
    """Raised when a prompt template has invalid Jinja syntax.

    Distinct from :class:`PromptRenderError` so callers that only care about
    syntax (e.g. save-time validation) can ignore runtime issues like missing
    context variables.
    """


_ENV = SandboxedEnvironment(
    autoescape=False,
    trim_blocks=False,
    lstrip_blocks=False,
    undefined=StrictUndefined,
)


def render(body: str, context: Dict[str, Any]) -> str:
    """Render ``body`` with ``context`` and return the resulting string.

    Wraps any Jinja error in :class:`PromptRenderError` so call sites can fail
    an :class:`AgentRun` cleanly instead of bubbling a 500.
    """
    try:
        template = _ENV.from_string(body)
        return template.render(**context)
    except TemplateError as exc:
        raise PromptRenderError(str(exc)) from exc


def validate_syntax(body: str) -> None:
    """Raise :class:`PromptSyntaxError` iff ``body`` is not a valid Jinja
    template. Does not execute the template or require any context — useful
    for save-time validation where missing runtime variables are expected.
    """
    try:
        _ENV.parse(body)
    except TemplateSyntaxError as exc:
        raise PromptSyntaxError(str(exc)) from exc
