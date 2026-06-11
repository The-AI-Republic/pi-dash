# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Convert assistant-authored markdown to sanitized HTML for issue/comment bodies.

Uses the same minimal paragraph-per-blank-line rendering + nh3 sanitization as
the GitHub sync importer (``bgtasks/github_sync_task.py``); there is no Python
Tiptap converter, so ``description_json`` is stored empty. See
``.ai_design/integrate_ai_agent/02-backend.md`` §4.2.
"""

from __future__ import annotations

from html import escape

from pi_dash.utils.content_validator import validate_html_content


def markdown_to_html(body: str | None) -> str:
    if not body:
        return "<p></p>"
    paragraphs = [p.strip() for p in body.split("\n\n") if p.strip()]
    if not paragraphs:
        return "<p></p>"
    return "".join(f"<p>{escape(p).replace(chr(10), '<br/>')}</p>" for p in paragraphs)


def to_safe_html(body: str | None) -> str:
    """Return sanitized HTML for ``body``; falls back to ``<p></p>`` on failure."""
    html = markdown_to_html(body)
    result = validate_html_content(html)
    # validate_html_content returns (is_valid, error_message, clean_html)
    if isinstance(result, tuple) and len(result) == 3:
        is_valid, _error, clean = result
        if is_valid and clean:
            return clean
    return "<p></p>"
