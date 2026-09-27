# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Unit tests for :func:`pi_dash.utils.markdown_converter.markdown_to_html`.

Agents write page bodies as markdown; this helper turns them into the HTML the
live server feeds to Tiptap's ``generateJSON``. Each expected string below is
the shape the matching editor extension's ``renderHTML`` produces (minus the
editor's styling classes), so the parse lands on the right node. The
round-trip tests pin the contract with :func:`html_to_markdown`: what an agent
writes is what it reads back.
"""

import pytest

from pi_dash.utils.markdown_converter import html_to_markdown, markdown_to_html

UNCHECKED_BOX = '<label><input type="checkbox"><span></span></label>'
CHECKED_BOX = '<label><input type="checkbox" checked="checked"><span></span></label>'


def task_item(checked: bool, inner: str) -> str:
    box = CHECKED_BOX if checked else UNCHECKED_BOX
    flag = "true" if checked else "false"
    return f'<li data-type="taskItem" data-checked="{flag}">{box}<div>{inner}</div></li>'


class TestBlockNodes:
    def test_headings_h1_to_h6(self):
        md = "\n\n".join(f"{'#' * level} H{level}" for level in range(1, 7))

        assert markdown_to_html(md) == "".join(f"<h{n}>H{n}</h{n}>" for n in range(1, 7))

    def test_paragraphs(self):
        assert markdown_to_html("one\n\ntwo") == "<p>one</p><p>two</p>"

    def test_soft_break_collapses_to_a_space(self):
        assert markdown_to_html("one\ntwo") == "<p>one two</p>"

    def test_hard_break(self):
        assert markdown_to_html("one  \ntwo") == "<p>one<br>two</p>"
        assert markdown_to_html("one\\\ntwo") == "<p>one<br>two</p>"

    def test_horizontal_rule_uses_the_editor_wrapper_div(self):
        # CustomHorizontalRule parses (and renders) `div[data-type=horizontalRule]`.
        assert (
            markdown_to_html("a\n\n---\n\nb")
            == '<p>a</p><div data-type="horizontalRule"><div></div></div><p>b</p>'
        )

    def test_blockquote(self):
        assert markdown_to_html("> quoted") == "<blockquote><p>quoted</p></blockquote>"


class TestInlineMarks:
    def test_bold_italic_strike_code(self):
        html = markdown_to_html("**b** *i* ~~s~~ `c`")

        # Tiptap's Strike renders `<s>` (it parses `<del>` too).
        assert html == "<p><strong>b</strong> <em>i</em> <s>s</s> <code>c</code></p>"

    def test_link_keeps_href(self):
        html = markdown_to_html("[a link](https://example.com/x)")

        assert html == '<p><a href="https://example.com/x" rel="noopener noreferrer">a link</a></p>'

    def test_unsafe_link_scheme_is_not_linked(self):
        html = markdown_to_html("[bad](javascript:alert(1))")

        assert "<a" not in html
        assert "href" not in html

    def test_link_with_non_allowed_scheme_loses_href(self):
        assert "href" not in markdown_to_html("[f](ftp://example.com/x)")


class TestImages:
    def test_lone_image_is_a_block_img(self):
        # Tiptap's Image node is a block, so it is not wrapped in a paragraph.
        assert markdown_to_html("![a cat](https://cdn.example/x.png)") == (
            '<img src="https://cdn.example/x.png" alt="a cat">'
        )

    def test_inline_image_stays_in_its_paragraph(self):
        html = markdown_to_html("see ![a cat](https://cdn.example/x.png) here")

        assert html == '<p>see <img src="https://cdn.example/x.png" alt="a cat"> here</p>'

    def test_data_uri_image_degrades_to_alt_text(self):
        assert markdown_to_html("![a cat](data:image/png;base64,AAAA)") == "<p>a cat</p>"


class TestLists:
    def test_bullet_list_items_wrap_content_in_paragraphs(self):
        assert markdown_to_html("- one\n- two") == "<ul><li><p>one</p></li><li><p>two</p></li></ul>"

    def test_nested_bullet_list(self):
        html = markdown_to_html("- one\n  - nested\n- two")

        assert html == "<ul><li><p>one</p><ul><li><p>nested</p></li></ul></li><li><p>two</p></li></ul>"

    def test_ordered_list(self):
        assert markdown_to_html("1. a\n2. b") == "<ol><li><p>a</p></li><li><p>b</p></li></ol>"

    def test_ordered_list_start(self):
        assert markdown_to_html("3. a\n4. b") == '<ol start="3"><li><p>a</p></li><li><p>b</p></li></ol>'

    def test_ordered_list_nested_in_bullet_list(self):
        html = markdown_to_html("- one\n  1. first\n  2. second")

        assert html == "<ul><li><p>one</p><ol><li><p>first</p></li><li><p>second</p></li></ol></li></ul>"


class TestTaskLists:
    def test_task_list(self):
        html = markdown_to_html("- [ ] todo\n- [x] done")

        assert html == (
            '<ul data-type="taskList">'
            + task_item(False, "<p>todo</p>")
            + task_item(True, "<p>done</p>")
            + "</ul>"
        )

    def test_uppercase_x_is_checked(self):
        assert 'data-checked="true"' in markdown_to_html("- [X] done")

    def test_nested_task_list_lives_inside_the_item_div(self):
        html = markdown_to_html("- [x] parent\n  - [ ] child")

        assert html == (
            '<ul data-type="taskList">'
            + task_item(
                True,
                '<p>parent</p><ul data-type="taskList">' + task_item(False, "<p>child</p>") + "</ul>",
            )
            + "</ul>"
        )

    def test_task_item_with_leading_formatting(self):
        html = markdown_to_html("- [ ] **bold** rest")

        assert html == '<ul data-type="taskList">' + task_item(False, "<p><strong>bold</strong> rest</p>") + "</ul>"

    def test_mixed_list_keeps_markers_as_text(self):
        """Tiptap's taskList only holds taskItems, so a partly-checked list
        stays a bullet list and the marker is kept rather than dropped."""
        html = markdown_to_html("- [ ] todo\n- plain")

        assert html == "<ul><li><p>[ ] todo</p></li><li><p>plain</p></li></ul>"

    def test_ordered_task_markers_stay_text(self):
        assert markdown_to_html("1. [x] done") == "<ol><li><p>[x] done</p></li></ol>"


class TestCodeBlocks:
    def test_fenced_code_block_with_language(self):
        html = markdown_to_html("```python\ndef f():\n    return 1\n```")

        # code-block.ts reads the language off the first child's class.
        assert html == '<pre><code class="language-python">def f():\n    return 1</code></pre>'

    def test_fenced_code_block_without_language(self):
        assert markdown_to_html("```\nplain\n```") == "<pre><code>plain</code></pre>"

    def test_indented_code_block(self):
        assert markdown_to_html("    indented") == "<pre><code>indented</code></pre>"

    def test_code_content_is_escaped(self):
        html = markdown_to_html("```html\n<script>alert(1)</script>\n```")

        assert html == '<pre><code class="language-html">&lt;script&gt;alert(1)&lt;/script&gt;</code></pre>'

    def test_inline_code_is_escaped(self):
        assert markdown_to_html("`<b>`") == "<p><code>&lt;b&gt;</code></p>"


class TestTables:
    def test_gfm_table(self):
        html = markdown_to_html("| A | B |\n| --- | :-: |\n| 1 | 2 |")

        assert html == (
            "<table><tbody>"
            "<tr><th><p>A</p></th><th><p>B</p></th></tr>"
            "<tr><td><p>1</p></td><td><p>2</p></td></tr>"
            "</tbody></table>"
        )

    def test_table_with_only_a_header_row(self):
        assert markdown_to_html("| A |\n| --- |") == "<table><tbody><tr><th><p>A</p></th></tr></tbody></table>"

    def test_table_cell_inline_formatting(self):
        assert "<td><p><strong>x</strong></p></td>" in markdown_to_html("| A |\n| --- |\n| **x** |")


class TestEscaping:
    def test_raw_html_block_is_escaped(self):
        html = markdown_to_html("<script>alert(1)</script>")

        assert "<script" not in html
        assert "&lt;script&gt;alert(1)&lt;/script&gt;" in html

    def test_raw_inline_html_is_escaped(self):
        assert markdown_to_html("text <b>x</b>") == "<p>text &lt;b&gt;x&lt;/b&gt;</p>"

    def test_ampersand_is_escaped(self):
        assert markdown_to_html("a & b") == "<p>a &amp; b</p>"


class TestEdgeCases:
    @pytest.mark.parametrize("value", [None, "", "   ", "\n\n"])
    def test_empty_input_is_an_empty_document(self, value):
        assert markdown_to_html(value) == "<p></p>"

    def test_oversized_output_raises_value_error(self, monkeypatch):
        import pi_dash.utils.content_validator as validator

        monkeypatch.setattr(validator, "MAX_SIZE", 10)

        with pytest.raises(ValueError, match="maximum size"):
            markdown_to_html("this paragraph is longer than ten bytes")


# -- round trip ------------------------------------------------------------
#
# Written in the canonical form html_to_markdown emits (ATX headings, `-`
# bullets, a blank line before a nested list because list items carry a
# `<p>`), so the round trip is exact.

ROUND_TRIP_CASES = [
    "# H1",
    "###### H6",
    "plain paragraph",
    "one  \ntwo",
    "a\n\n---\n\nb",
    "> quoted",
    "**bold** *italic* ~~strike~~ `code`",
    "[a link](https://example.com/x)",
    "![a cat](https://cdn.example/x.png)",
    "- one\n\n  - nested\n- two",
    "1. first\n2. second",
    "3. three\n4. four",
    "- [ ] todo\n- [x] done",
    "- [x] parent\n\n  - [ ] child",
    "```python\ndef f():\n    return '<script>'\n```",
    "```\nplain\n```",
    "| A | B |\n| --- | --- |\n| 1 | 2 |",
    "<b>not html</b> & friends",
]

FULL_DOCUMENT = """# Title

## Sub *heading*

Para with **bold**, *italic*, ~~strike~~, `code` and a [link](https://example.com).<HARD_BREAK>Hard break line.

---

- one

  - nested
- two

3. three
4. four

- [ ] todo
- [x] done

  - [ ] sub task

```python
def f():
    return "<script>"
```

| A | B |
| --- | --- |
| 1 | 2 |

> quoted

![cat](https://cdn.example/x.png)""".replace(
    # Spelled out so an editor's trailing-whitespace cleanup can't eat it.
    "<HARD_BREAK>",
    "  \n",
)


class TestRoundTrip:
    @pytest.mark.parametrize("markdown", ROUND_TRIP_CASES)
    def test_node_round_trips(self, markdown):
        assert html_to_markdown(markdown_to_html(markdown)) == markdown

    def test_full_document_round_trips(self):
        assert html_to_markdown(markdown_to_html(FULL_DOCUMENT)) == FULL_DOCUMENT

    def test_tight_nested_list_normalises_to_the_same_html(self):
        """A tight nested list reads back with a blank line, which is only a
        markdown spelling difference: both render to identical HTML."""
        tight = "- one\n  - nested\n- two"

        read_back = html_to_markdown(markdown_to_html(tight))

        assert read_back == "- one\n\n  - nested\n- two"
        assert markdown_to_html(read_back) == markdown_to_html(tight)


class TestHtmlToMarkdownTaskState:
    """Regression: the live server's DOM shim serialises a checked task item
    as a bare ``data-checked`` attribute, which TaskItem's parseHTML treats as
    checked; html_to_markdown must agree."""

    def test_bare_data_checked_reads_as_checked(self):
        html = '<ul data-type="taskList"><li data-checked data-type="taskItem"><div><p>x</p></div></li></ul>'

        assert html_to_markdown(html) == "- [x] x"

    def test_missing_data_checked_reads_as_unchecked(self):
        html = '<ul data-type="taskList"><li data-type="taskItem"><div><p>x</p></div></li></ul>'

        assert html_to_markdown(html) == "- [ ] x"
