# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Unit tests for :func:`pi_dash.utils.markdown_converter.html_to_markdown`.

The helper is the single server-side renderer behind the ``/api/v1/`` page
detail endpoint, ``pidash page get`` and the downstream MCP page tool, so its
output is a contract: one node type per test, plus the two properties the
agent-facing surfaces rely on — unknown nodes degrade instead of vanishing,
and nothing raises.
"""

from pi_dash.utils.markdown_converter import html_to_markdown


class TestBlockNodes:
    def test_headings_use_atx_so_deep_levels_survive(self):
        html = "<h1>One</h1><h2>Two</h2><h3>Three</h3>"

        assert html_to_markdown(html) == "# One\n\n## Two\n\n### Three"

    def test_paragraph_and_inline_formatting(self):
        html = "<p>plain <strong>bold</strong> <em>italic</em> <code>code</code> <del>gone</del></p>"

        assert html_to_markdown(html) == "plain **bold** *italic* `code` ~~gone~~"

    def test_blockquote(self):
        assert html_to_markdown("<blockquote><p>quoted</p></blockquote>") == "> quoted"

    def test_link_keeps_href(self):
        html = '<p><a href="https://example.com/x">a link</a></p>'

        assert html_to_markdown(html) == "[a link](https://example.com/x)"

    def test_horizontal_rule(self):
        assert html_to_markdown("<p>a</p><hr /><p>b</p>") == "a\n\n---\n\nb"

    def test_tiptap_horizontal_rule_div_is_kept(self):
        # The editor emits a wrapper div rather than `<hr>`; the rule has a
        # markdown equivalent and must not be dropped.
        html = '<p>a</p><div class="py-4" data-type="horizontalRule"><div></div></div><p>b</p>'

        assert html_to_markdown(html) == "a\n\n---\n\nb"

    def test_plain_div_is_not_turned_into_a_rule(self):
        assert html_to_markdown("<div><p>a</p></div>") == "a"


class TestLists:
    def test_nested_unordered_list_is_indented(self):
        html = "<ul><li><p>one</p><ul><li><p>nested</p></li></ul></li><li><p>two</p></li></ul>"

        assert html_to_markdown(html) == "- one\n\n  - nested\n- two"

    def test_ordered_list_numbers_sequentially(self):
        html = "<ol><li><p>first</p></li><li><p>second</p></li></ol>"

        assert html_to_markdown(html) == "1. first\n2. second"

    def test_task_list_renders_checkbox_state(self):
        """Tiptap task lists are `<ul>`/`<li>` carrying `data-type`, with the
        checked flag on `data-checked` rather than the `<input>`."""
        html = (
            '<ul data-type="taskList">'
            '<li data-checked="true" data-type="taskItem">'
            '<label><input type="checkbox" checked="checked"><span></span></label>'
            "<div><p>done thing</p></div></li>"
            '<li data-checked="false" data-type="taskItem">'
            '<label><input type="checkbox"><span></span></label>'
            "<div><p>todo thing</p></div></li>"
            "</ul>"
        )

        assert html_to_markdown(html) == "- [x] done thing\n- [ ] todo thing"

    def test_task_list_checkbox_input_is_not_duplicated(self):
        html = (
            '<ul data-type="taskList"><li data-checked="true" data-type="taskItem">'
            '<label><input type="checkbox" checked="checked"></label><div><p>x</p></div>'
            "</li></ul>"
        )

        assert html_to_markdown(html).count("[x]") == 1


class TestCodeAndTables:
    def test_fenced_code_block_keeps_its_language(self):
        html = '<pre><code class="language-python">def f():\n    return 1</code></pre>'

        assert html_to_markdown(html) == "```python\ndef f():\n    return 1\n```"

    def test_code_block_without_language_still_fences(self):
        html = "<pre><code>plain</code></pre>"

        assert html_to_markdown(html) == "```\nplain\n```"

    def test_table_renders_header_and_body_rows(self):
        html = (
            "<table><tbody>"
            "<tr><th><p>Col A</p></th><th><p>Col B</p></th></tr>"
            "<tr><td><p>1</p></td><td><p>2</p></td></tr>"
            "</tbody></table>"
        )

        assert html_to_markdown(html) == "| Col A | Col B |\n| --- | --- |\n| 1 | 2 |"


class TestNodesWithNoMarkdownEquivalent:
    """These must degrade to readable text or a link — never disappear."""

    def test_mention_renders_entity_type_and_id(self):
        html = (
            '<p>Hi <mention-component id="m1" entity_identifier="8f3c" '
            'entity_name="user_mention"></mention-component>!</p>'
        )

        assert html_to_markdown(html) == "Hi @user_mention:8f3c!"

    def test_mention_without_identifier_falls_back_to_entity_name(self):
        html = '<p><mention-component entity_name="user_mention"></mention-component></p>'

        assert html_to_markdown(html) == "@user_mention"

    def test_image_component_becomes_an_image_link(self):
        html = '<image-component id="img1" src="https://cdn.example/x.png"></image-component>'

        assert html_to_markdown(html) == "![](https://cdn.example/x.png)"

    def test_image_component_without_src_still_reports_the_asset(self):
        html = '<image-component id="img1"></image-component>'

        assert html_to_markdown(html) == "[image img1]"

    def test_plain_img_tag_is_converted(self):
        html = '<p><img src="https://cdn.example/y.png" alt="a cat" /></p>'

        assert html_to_markdown(html) == "![a cat](https://cdn.example/y.png)"

    def test_work_item_embed_becomes_a_link(self):
        html = (
            '<issue-embed-component entity_identifier="iss" project_identifier="proj" '
            'workspace_identifier="acme"></issue-embed-component>'
        )

        assert html_to_markdown(html) == "[work item iss](/acme/projects/proj/issues/iss)"

    def test_work_item_embed_without_routing_attrs_degrades_to_text(self):
        html = '<issue-embed-component entity_identifier="iss"></issue-embed-component>'

        assert html_to_markdown(html) == "[work item iss]"

    def test_unknown_node_keeps_its_text(self):
        html = "<p>before</p><some-future-node>fallback text</some-future-node><p>after</p>"

        result = html_to_markdown(html)

        assert "fallback text" in result
        assert "some-future-node" not in result


class TestRobustness:
    def test_empty_inputs_return_empty_string(self):
        assert html_to_markdown("") == ""
        assert html_to_markdown(None) == ""
        assert html_to_markdown("<p></p>") == ""

    def test_unclosed_tags_do_not_raise(self):
        assert "dangling" in html_to_markdown("<p>dangling<strong>bold")

    def test_identifiers_are_not_backslash_escaped(self):
        """Escaping every `_`/`*` mangles code-ish prose for the agent reading
        it; this markdown is read, not round-tripped."""
        result = html_to_markdown("<p>snake_case_name and 2*3</p>")

        assert result == "snake_case_name and 2*3"

    def test_conversion_failure_falls_back_to_stripped_text(self, monkeypatch):
        import pi_dash.utils.markdown_converter as mod

        def boom(self, html):
            raise RuntimeError("converter exploded")

        monkeypatch.setattr(mod.TiptapMarkdownConverter, "convert", boom)

        assert html_to_markdown("<p>still readable</p>") == "still readable"
