# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Tiptap HTML ⇄ markdown.

Pages (and work-item descriptions) are stored as Tiptap-flavoured HTML in
``description_html``. Agents read and write markdown far better than HTML, so
every agent-facing surface — the ``/api/v1/`` page endpoints, the ``pidash
page`` CLI, and the downstream MCP page tool — goes through this module:

* :func:`html_to_markdown` renders stored HTML for reads.
* :func:`markdown_to_html` turns an agent's markdown into HTML the editor's
  Tiptap/ProseMirror schema parses back into the right nodes, for writes.

Why ``markdownify`` (HTML → markdown): it is MIT-licensed, pure Python, and
built on ``beautifulsoup4``, which this project already depends on — so it
adds one small wheel and no new parser. It is also a plain subclassable
converter (``convert_<tag>`` method per tag), which is what lets us teach it
the Tiptap-only nodes below without forking it.

Why ``markdown-it-py`` (markdown → HTML): it is a CommonMark-compliant parser
(MIT) with GFM tables/strikethrough built in and task lists from
``mdit-py-plugins``, so we never hand-roll markdown parsing. We render straight
from its token stream with a custom renderer rather than post-processing its
stock HTML with BeautifulSoup: the Tiptap shapes differ structurally from stock
HTML (task items need a ``<label>``/``<div>`` wrapper, list items and table
cells need a ``<p>`` even in tight lists, horizontal rules are a wrapper
``<div>``), and emitting them directly from tokens is exact and cheap, whereas
reshaping a second DOM would be a lossy guess at what the parser meant. Raw
HTML inside the markdown is *not* passed through (``html=False``): markdown
is the input format, so ``<script>`` in the text arrives as escaped text.

Contract for callers:

* Both helpers are **pure** — no database access, no request state — so the
  API serializer, the CLI and the MCP connector can all import them.
* :func:`html_to_markdown` **never raises**. Malformed or surprising HTML
  degrades to the plain stripped text rather than failing a read request.
  Nodes with no markdown equivalent (mentions, embedded work items, images)
  degrade to readable text or a link. They are never dropped silently.
* :func:`markdown_to_html` runs its output through the same sanitizer as every
  other HTML write (:func:`~pi_dash.utils.content_validator.validate_html_content`)
  and raises :class:`ValueError` when that rejects it (e.g. the 10MB cap), which
  callers turn into a 400.
"""

from __future__ import annotations

from urllib.parse import urlsplit

from markdown_it import MarkdownIt
from markdown_it.common.utils import escapeHtml
from markdown_it.renderer import RendererHTML
from markdown_it.token import Token
from markdownify import MarkdownConverter
from mdit_py_plugins.tasklists import tasklists_plugin

from pi_dash.utils.content_validator import SAFE_PROTOCOLS, validate_html_content
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
        # Mirror TaskItem's parseHTML: `data-checked=""` counts as checked.
        # The live server's DOM shim (zeed-dom) serialises a checked item as
        # a bare `data-checked` attribute rather than `data-checked="true"`.
        checked = el.get("data-checked")
        is_checked = checked is not None and str(checked).lower() in ("", "true")
        marker = "[x] " if is_checked else "[ ] "
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


# ---------------------------------------------------------------------------
# markdown → Tiptap HTML
# ---------------------------------------------------------------------------
#
# Every shape below mirrors the `renderHTML` of the matching editor extension
# (packages/editor/src/core/extensions/ and the @tiptap packages) so the live
# server's `generateJSON(html, extensions)` lands on the same nodes the editor
# itself would have produced.

#: What the tasklists plugin injects as the first inline child of a task item.
_TASK_CHECKBOX_MARKER = 'class="task-list-item-checkbox"'

#: Tiptap stores an empty document as a single empty paragraph.
EMPTY_DOCUMENT_HTML = "<p></p>"


def _is_safe_url(url: str) -> bool:
    """Relative URLs and the sanitizer's allowed schemes only."""
    try:
        scheme = urlsplit(url).scheme.lower()
    except ValueError:
        return False
    return not scheme or scheme in SAFE_PROTOCOLS


def _is_task_item(tokens: list[Token], item_idx: int) -> bool:
    """True when the tasklists plugin turned this ``list_item_open`` into a
    todo (it prepends an ``html_inline`` checkbox to the first paragraph)."""
    if item_idx + 2 >= len(tokens):
        return False
    inline = tokens[item_idx + 2]
    return (
        tokens[item_idx + 1].type == "paragraph_open"
        and inline.type == "inline"
        and bool(inline.children)
        and inline.children[0].type == "html_inline"
        and _TASK_CHECKBOX_MARKER in inline.children[0].content
    )


def _resolve_task_lists(tokens: list[Token]) -> None:
    """Decide, per list, whether it becomes a Tiptap ``taskList``.

    Tiptap's ``taskList`` only holds ``taskItem`` children, so a bullet list
    becomes one only when *every* item is a todo. In a mixed or ordered list
    the ``[ ]``/``[x]`` marker is put back as literal text instead of silently
    dropping the checkbox state. Open and close tokens share one ``meta`` dict
    so the renderer sees the decision on both ends.
    """
    list_stack: list[tuple[Token, list[int]]] = []
    item_stack: list[Token] = []
    for idx, token in enumerate(tokens):
        if token.type in ("bullet_list_open", "ordered_list_open"):
            list_stack.append((token, []))
        elif token.type in ("bullet_list_close", "ordered_list_close"):
            list_open, items = list_stack.pop()
            token.meta = list_open.meta
            flags = [_is_task_item(tokens, i) for i in items]
            as_task_list = list_open.type == "bullet_list_open" and bool(items) and all(flags)
            list_open.meta["task_list"] = as_task_list
            for item_idx, is_task in zip(items, flags):
                if not is_task:
                    continue
                children = tokens[item_idx + 2].children
                checkbox = children.pop(0)
                checked = 'checked="checked"' in checkbox.content
                if as_task_list:
                    tokens[item_idx].meta["task_item"] = True
                    tokens[item_idx].meta["checked"] = checked
                    # The plugin strips `[ ]` but leaves the separating space.
                    if children and children[0].type == "text":
                        children[0].content = children[0].content.lstrip()
                else:
                    marker = Token("text", "", 0)
                    marker.content = "[x]" if checked else "[ ]"
                    children.insert(0, marker)
        elif token.type == "list_item_open":
            list_stack[-1][1].append(idx)
            item_stack.append(token)
        elif token.type == "list_item_close":
            token.meta = item_stack.pop().meta


class TiptapHTMLRenderer(RendererHTML):
    """Render markdown-it tokens as the HTML Tiptap's editor emits.

    Output is compact (no inter-block newlines) and carries only the
    attributes the editor's parse rules read; stock markdown-it attributes
    (alignment styles, plugin classes) are dropped.
    """

    # -- generic block/inline tags --------------------------------------

    def renderToken(self, tokens, idx, options, env):
        token = tokens[idx]
        if token.hidden or not token.tag:
            return ""
        if token.nesting == -1:
            return f"</{token.tag}>"
        return f"<{token.tag}>"

    def text(self, tokens, idx, options, env):
        return escapeHtml(tokens[idx].content)

    def softbreak(self, tokens, idx, options, env):
        # ProseMirror collapses a newline in a paragraph to a space anyway.
        return " "

    def hardbreak(self, tokens, idx, options, env):
        return "<br>"

    # Raw HTML is disabled at parse time (``html=False``); escape defensively
    # in case a plugin ever emits one.
    def html_inline(self, tokens, idx, options, env):
        return escapeHtml(tokens[idx].content)

    def html_block(self, tokens, idx, options, env):
        return f"<p>{escapeHtml(tokens[idx].content.strip())}</p>"

    # -- paragraphs: a lone image is hoisted out (Tiptap images are blocks) --

    @staticmethod
    def _lone_image(inline: Token | None) -> bool:
        return (
            inline is not None
            and inline.type == "inline"
            and inline.level == 1
            and len(inline.children or []) == 1
            and inline.children[0].type == "image"
            and _is_safe_url(inline.children[0].attrGet("src") or "")
        )

    def paragraph_open(self, tokens, idx, options, env):
        if tokens[idx].level == 0 and self._lone_image(tokens[idx + 1]):
            return ""
        return "<p>"

    def paragraph_close(self, tokens, idx, options, env):
        if tokens[idx].level == 0 and self._lone_image(tokens[idx - 1]):
            return ""
        return "</p>"

    # -- horizontal rule: CustomHorizontalRule renders a wrapper div ------

    def hr(self, tokens, idx, options, env):
        return '<div data-type="horizontalRule"><div></div></div>'

    # -- lists ----------------------------------------------------------

    def bullet_list_open(self, tokens, idx, options, env):
        if tokens[idx].meta.get("task_list"):
            return '<ul data-type="taskList">'
        return "<ul>"

    def ordered_list_open(self, tokens, idx, options, env):
        start = tokens[idx].attrGet("start")
        if start is not None and str(start) != "1":
            return f'<ol start="{escapeHtml(str(start))}">'
        return "<ol>"

    def list_item_open(self, tokens, idx, options, env):
        meta = tokens[idx].meta
        if not meta.get("task_item"):
            return "<li>"
        checked = meta.get("checked", False)
        checkbox = '<input type="checkbox" checked="checked">' if checked else '<input type="checkbox">'
        return (
            f'<li data-type="taskItem" data-checked="{"true" if checked else "false"}">'
            f"<label>{checkbox}<span></span></label><div>"
        )

    def list_item_close(self, tokens, idx, options, env):
        return "</div></li>" if tokens[idx].meta.get("task_item") else "</li>"

    # -- code -----------------------------------------------------------

    def fence(self, tokens, idx, options, env):
        token = tokens[idx]
        language = token.info.strip().split(maxsplit=1)[0] if token.info.strip() else ""
        # markdown-it keeps the newline before the closing fence; the editor
        # does not store one.
        body = escapeHtml(token.content.removesuffix("\n"))
        if language:
            return f'<pre><code class="language-{escapeHtml(language)}">{body}</code></pre>'
        return f"<pre><code>{body}</code></pre>"

    def code_block(self, tokens, idx, options, env):
        return f"<pre><code>{escapeHtml(tokens[idx].content.removesuffix(chr(10)))}</code></pre>"

    # -- tables: one <tbody>, header row as <th>, cell content in <p> ----

    def table_open(self, tokens, idx, options, env):
        return "<table><tbody>"

    def table_close(self, tokens, idx, options, env):
        return "</tbody></table>"

    def thead_open(self, tokens, idx, options, env):
        return ""

    thead_close = tbody_open = tbody_close = thead_open

    def th_open(self, tokens, idx, options, env):
        return "<th><p>"

    def th_close(self, tokens, idx, options, env):
        return "</p></th>"

    def td_open(self, tokens, idx, options, env):
        return "<td><p>"

    def td_close(self, tokens, idx, options, env):
        return "</p></td>"

    # -- links and images -----------------------------------------------

    def link_open(self, tokens, idx, options, env):
        token = tokens[idx]
        href = token.attrGet("href") or ""
        attrs = f' href="{escapeHtml(str(href))}"' if _is_safe_url(str(href)) else ""
        title = token.attrGet("title")
        if title:
            attrs += f' title="{escapeHtml(str(title))}"'
        return f"<a{attrs}>"

    def image(self, tokens, idx, options, env):
        token = tokens[idx]
        src = str(token.attrGet("src") or "")
        alt = self.renderInlineAsText(token.children or [], options, env)
        if not _is_safe_url(src) or not src:
            # The sanitizer would strip the src and the editor's `img[src]`
            # rule would then drop the node; keep the alt text instead.
            return escapeHtml(alt)
        attrs = f' src="{escapeHtml(src)}"'
        if alt:
            attrs += f' alt="{escapeHtml(alt)}"'
        title = token.attrGet("title")
        if title:
            attrs += f' title="{escapeHtml(str(title))}"'
        return f"<img{attrs}>"


def _build_markdown_parser() -> MarkdownIt:
    md = MarkdownIt("commonmark", {"html": False}, renderer_cls=TiptapHTMLRenderer)
    md.enable(["table", "strikethrough"])
    md.use(tasklists_plugin)
    return md


_MARKDOWN_PARSER = _build_markdown_parser()


def markdown_to_html(markdown: str | None) -> str:
    """Render agent-written markdown as Tiptap ``description_html``.

    Returns ``"<p></p>"`` (Tiptap's empty document) for empty input. The HTML
    is sanitised with :func:`validate_html_content`; raises ``ValueError``
    when the sanitizer rejects it (e.g. over the 10MB limit).
    """
    if not markdown or not markdown.strip():
        return EMPTY_DOCUMENT_HTML

    env: dict = {}
    tokens = _MARKDOWN_PARSER.parse(markdown, env)
    for token in tokens:
        # Tiptap list items and table cells always wrap text in a paragraph,
        # so render tight-list paragraphs too.
        if token.type in ("paragraph_open", "paragraph_close"):
            token.hidden = False
    _resolve_task_lists(tokens)
    html = _MARKDOWN_PARSER.renderer.render(tokens, _MARKDOWN_PARSER.options, env)

    if not html:
        return EMPTY_DOCUMENT_HTML
    is_valid, error, clean_html = validate_html_content(html)
    if not is_valid:
        raise ValueError(error or "Invalid HTML content")
    return clean_html or EMPTY_DOCUMENT_HTML
