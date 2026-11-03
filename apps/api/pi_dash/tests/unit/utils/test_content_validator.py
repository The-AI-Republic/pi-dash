# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Unit tests for :func:`pi_dash.utils.content_validator.validate_html_content`.

The sanitizer is the allowlist gate on the page save path: anything it strips
is lost from ``description_html`` on save/reload. These tests pin the page
body nodes (PAGE-040 embeds, PAGE-043 entity references) plus the hostile
markup that must still be removed.
"""

from pi_dash.utils.content_validator import validate_html_content


def clean(html):
    is_valid, error, clean_html = validate_html_content(html)
    assert is_valid, error
    return clean_html


class TestPageBodyNodes:
    def test_issue_embed_keeps_all_identifiers(self):
        html = (
            '<p><issue-embed-component id="emb-1" entity_identifier="ISS-2" '
            'project_identifier="P" workspace_identifier="acme" entity_name="issue">'
            "</issue-embed-component></p>"
        )

        result = clean(html)

        assert "<issue-embed-component" in result
        for attr in (
            'id="emb-1"',
            'entity_identifier="ISS-2"',
            'project_identifier="P"',
            'workspace_identifier="acme"',
            'entity_name="issue"',
        ):
            assert attr in result

    def test_entity_reference_keeps_all_attributes(self):
        html = (
            '<p>see <entity-reference-component entity_name="issue" '
            'entity_identifier="ISS-2" project_identifier="P" '
            'workspace_identifier="acme" project_id="p-1" data-label="Fix login">'
            "</entity-reference-component></p>"
        )

        result = clean(html)

        assert "<entity-reference-component" in result
        for attr in (
            'entity_name="issue"',
            'entity_identifier="ISS-2"',
            'project_identifier="P"',
            'workspace_identifier="acme"',
            'project_id="p-1"',
            'data-label="Fix login"',
        ):
            assert attr in result

    def test_entity_reference_without_optional_project_id(self):
        html = (
            '<p><entity-reference-component entity_name="issue" '
            'entity_identifier="ISS-2" project_identifier="P" '
            'workspace_identifier="acme" data-label="Fix login">'
            "</entity-reference-component></p>"
        )

        result = clean(html)

        assert "<entity-reference-component" in result
        assert 'data-label="Fix login"' in result

    def test_empty_entity_reference_node_survives(self):
        # The label lives only in data-label, never as text; a bare node must
        # still round-trip rather than vanish.
        html = "<p><entity-reference-component></entity-reference-component></p>"

        result = clean(html)

        assert "<entity-reference-component" in result

    def test_mention_component_still_kept(self):
        html = '<p><mention-component id="u-1" entity_identifier="u-1" entity_name="user"></mention-component></p>'

        result = clean(html)

        assert "<mention-component" in result
        assert 'entity_name="user"' in result


class TestHostileMarkupStillStripped:
    def test_script_tag_removed(self):
        result = clean("<p>hi</p><script>alert(1)</script>")

        assert "<script" not in result
        assert "alert(1)" not in result
        assert "hi" in result

    def test_event_handler_stripped_from_custom_tags(self):
        html = (
            '<p><issue-embed-component id="e" entity_identifier="ISS-2" '
            'onclick="alert(1)">x</issue-embed-component>'
            '<entity-reference-component entity_name="issue" '
            'onerror="alert(1)">y</entity-reference-component></p>'
        )

        result = clean(html)

        assert "<issue-embed-component" in result
        assert "<entity-reference-component" in result
        assert "onclick" not in result
        assert "onerror" not in result
        assert "alert(1)" not in result

    def test_javascript_url_stripped_from_links(self):
        result = clean('<p><a href="javascript:alert(1)">click</a></p>')

        assert "javascript:" not in result
        assert "click" in result

    def test_unknown_tag_stripped(self):
        result = clean("<p><marquee>scrolling</marquee></p>")

        assert "<marquee" not in result
