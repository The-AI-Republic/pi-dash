# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Tiptap HTML → markdown.

Pages (and work-item descriptions) are stored as Tiptap-flavoured HTML in
``description_html``. Agents read markdown far better than HTML, so every
agent-facing surface — the ``/api/v1/`` page endpoints, the ``pidash page``
CLI, and the downstream MCP page tool — renders the same markdown through
:func:`html_to_markdown`.

Why ``markdownify``: it is MIT-licensed, pure Python, and built on
``beautifulsoup4``, which this project already depends on — so it adds one
small wheel and no new parser. It is also a plain subclassable converter
(``convert_<tag>`` method per tag), which is what lets us teach it the
Tiptap-only nodes below without forking it.

Contract for callers:

* The helper is **pure** — no database access, no request state — so the API
  serializer, the CLI and the MCP connector can all import it.
* It **never raises**. Malformed or surprising HTML degrades to the plain
  stripped text rather than failing a read request.
* Nodes with no markdown equivalent (mentions, embedded work items, images)
  degrade to readable text or a link. They are never dropped silently.
"""

from __future__ import annotations

from markdownify import MarkdownConverter

from pi_dash.utils.html_processor import strip_tags

#: Tiptap renders a fenced code block's language as ``class="language-<lang>"``
#: on the inner ``<code>`` element.
_LANGUAGE_CLASS_PREFIX = "language-"


def _code_language(pre_el) -> str:
    """Read the fence language off a ``<pre><code class="language-x">`` block."""
    code_el = pre_el.find("code")
    if code_el is None:
        return ""
    for css_class in code_el.get("class") or []:
        if css_class.startswith(_LANGUAGE_CLASS_PREFIX):
            return css_class[len(_LANGUAGE_CLASS_PREFIX) :]
    return ""


def _first_attr(el, *names: str) -> str:
    """Return the first non-empty attribute among ``names``."""
    for name in names:
        value = el.get(name)
        if value:
            return str(value).strip()
    return ""


class TiptapMarkdownConverter(MarkdownConverter):
    """``markdownify`` taught to read Pi Dash's editor output.

    Beyond stock HTML it handles the three custom element names the editor
    emits (see ``packages/editor/src/core/extensions/``) plus Tiptap's task
    lists, which are ordinary ``<ul>``/``<li>`` carrying ``data-type``
    attributes rather than a distinct tag.
    """

    class Options(MarkdownConverter.DefaultOptions):
        # `#` headings, not the `===` underlined style, so deep headings
        # survive (setext has no h3+).
        heading_style = "ATX"
        # One bullet character at every depth; nesting is carried by indent.
        bullets = "-"
        # This markdown is read, not round-tripped. Backslash-escaping every
        # `_` and `*` mangles identifiers like `snake_case_name` for no gain.
        escape_asterisks = False
        escape_underscores = False
        escape_misc = False
        code_language_callback = staticmethod(_code_language)

    # -- Tiptap task lists ------------------------------------------------
    #
    # `<ul data-type="taskList"><li data-type="taskItem" data-checked="true">`
    # with the checkbox itself in a `<label><input type="checkbox"></label>`.

    def convert_li(self, el, text, parent_tags):
        converted = super().convert_li(el, text, parent_tags)
        if el.get("data-type") != "taskItem":
            return converted
        marker = "[x] " if str(el.get("data-checked", "")).lower() == "true" else "[ ] "
        # `converted` looks like `<indent>- <content>\n`; slot the checkbox
        # marker in right after the bullet so nesting indentation survives.
        bullet_end = converted.find("- ")
        if bullet_end == -1:
            return converted
        insert_at = bullet_end + 2
        return converted[:insert_at] + marker + converted[insert_at:]

    def convert_input(self, el, text, parent_tags):
        # The task-item checkbox is rendered by `convert_li` from
        # `data-checked`; emitting it again here would double it up.
        return ""

    def convert_div(self, el, text, parent_tags):
        # The editor serialises a horizontal rule as a wrapper
        # `<div data-type="horizontalRule">`, not as `<hr>`, so without this
        # the rule is dropped from the markdown entirely.
        if el.get("data-type") == "horizontalRule":
            return self.convert_hr(el, text, parent_tags)
        return super().convert_div(el, text, parent_tags)

    # -- Tiptap custom nodes ----------------------------------------------
    #
    # markdownify maps a tag name to `convert_<name>` with `-` replaced by
    # `_`, so `mention-component` dispatches to `convert_mention_component`.

    def convert_mention_component(self, el, text, parent_tags):
        """`@user_mention:<uuid>` — the display name needs a DB lookup this
        helper deliberately does not do, so the entity type and id stand in."""
        identifier = _first_attr(el, "entity_identifier", "id")
        entity = _first_attr(el, "entity_name") or "mention"
        return f"@{entity}:{identifier}" if identifier else f"@{entity}"

    def convert_image_component(self, el, text, parent_tags):
        src = _first_attr(el, "src")
        if src:
            return f"![]({src})"
        asset_id = _first_attr(el, "id")
        return f"[image{f' {asset_id}' if asset_id else ''}]"

    def convert_issue_embed_component(self, el, text, parent_tags):
        entity_id = _first_attr(el, "entity_identifier", "id")
        project = _first_attr(el, "project_identifier")
        workspace = _first_attr(el, "workspace_identifier")
        label = f"work item {entity_id}" if entity_id else "work item"
        if entity_id and project and workspace:
            return f"[{label}](/{workspace}/projects/{project}/issues/{entity_id})"
        return f"[{label}]"


def html_to_markdown(html: str | None) -> str:
    """Render Tiptap ``description_html`` as markdown.

    Returns an empty string for empty input. On any conversion failure falls
    back to the tag-stripped text so a read never fails because of one odd
    document.
    """
    if not html:
        return ""
    try:
        return TiptapMarkdownConverter().convert(html).strip()
    except Exception:
        try:
            return strip_tags(html).strip()
        except Exception:
            return ""
