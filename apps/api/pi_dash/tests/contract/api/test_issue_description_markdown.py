# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Work-item create/patch accept the description as ``description_markdown``.

``pidash issue create|patch`` send the raw markdown and the v1 endpoints
convert it with the shared markdown -> Tiptap HTML converter the page
endpoints use (:func:`pi_dash.utils.markdown_converter.markdown_to_html`), so a
checklist in an issue body is stored as a checklist. The node shapes
themselves are pinned in ``tests/unit/utils/test_markdown_to_html.py``; these
tests pin that the issue endpoints route through that converter, keep
accepting ``description_html`` and the legacy ``description`` key, and apply
the documented precedence.
"""

from unittest import mock

import pytest
from rest_framework import status as http_status

from pi_dash.db.models import Issue, ProjectMember
from pi_dash.tests.unit.utils.test_markdown_to_html import task_item
from pi_dash.utils.markdown_converter import markdown_to_html

# One body carrying every structure the issue calls out: a heading, a nested
# list, a task list, a fenced code block with a language, and a table.
RICH_MARKDOWN = """# Cold start

- parent
  - child

Checklist:

- [ ] fixtures
- [x] prerequisites

```rust
fn main() {}
```

| page | updated_at |
| --- | --- |
| wiki-1 | 2026-09-21 |
"""


@pytest.fixture(autouse=True)
def _no_celery():
    """Activity fan-out runs over Celery; the tests inspect what it is sent."""
    with (
        mock.patch("pi_dash.api.views.issue.issue_activity.delay") as issue_activity,
        mock.patch("pi_dash.api.views.issue.model_activity.delay"),
    ):
        yield issue_activity


@pytest.fixture
def member_project(db, project, create_user):
    ProjectMember.objects.get_or_create(project=project, member=create_user, defaults={"role": 20, "is_active": True})
    return project


def _list_url(workspace, project):
    return f"/api/v1/workspaces/{workspace.slug}/projects/{project.id}/work-items/"


def _detail_url(workspace, project, issue_id):
    return f"/api/v1/workspaces/{workspace.slug}/projects/{project.id}/work-items/{issue_id}/"


def _create(client, workspace, project, **body):
    return client.post(_list_url(workspace, project), {"name": "Issue", **body}, format="json")


def _stored_html(response):
    return Issue.objects.get(pk=response.data["id"]).description_html


def assert_rich_structure(html):
    """The Tiptap node shapes the editor needs to render each structure."""
    assert "<h1>Cold start</h1>" in html
    # Nested bullet list: the child list sits inside the parent item.
    assert "<ul><li><p>parent</p><ul><li><p>child</p></li></ul></li></ul>" in html
    # Task list: taskList/taskItem nodes, checked state preserved.
    assert '<ul data-type="taskList">' in html
    assert task_item(False, "<p>fixtures</p>") in html
    assert task_item(True, "<p>prerequisites</p>") in html
    # Fenced code keeps its language for the code-block extension.
    assert '<pre><code class="language-rust">fn main() {}</code></pre>' in html
    assert "<table>" in html
    assert "<th><p>page</p></th>" in html
    assert "<td><p>wiki-1</p></td>" in html


@pytest.mark.contract
@pytest.mark.django_db
class TestCreate:
    def test_markdown_is_converted_with_the_shared_converter(self, api_key_client, workspace, member_project):
        response = _create(api_key_client, workspace, member_project, description_markdown=RICH_MARKDOWN)

        assert response.status_code == http_status.HTTP_201_CREATED, response.data
        html = _stored_html(response)
        assert html == markdown_to_html(RICH_MARKDOWN)
        assert_rich_structure(html)
        # The response reflects the stored HTML; the markdown key is write-only.
        assert response.data["description_html"] == html
        assert "description_markdown" not in response.data

    def test_activity_records_the_converted_html(self, api_key_client, workspace, member_project, _no_celery):
        response = _create(api_key_client, workspace, member_project, description_markdown="# Title")

        assert response.status_code == http_status.HTTP_201_CREATED, response.data
        requested = _no_celery.call_args.kwargs["requested_data"]
        assert "<h1>Title</h1>" in requested
        assert "description_markdown" not in requested

    def test_description_html_is_still_accepted(self, api_key_client, workspace, member_project):
        response = _create(api_key_client, workspace, member_project, description_html="<p>raw <b>html</b></p>")

        assert response.status_code == http_status.HTTP_201_CREATED, response.data
        assert _stored_html(response) == "<p>raw <b>html</b></p>"

    def test_legacy_description_is_converted(self, api_key_client, workspace, member_project):
        response = _create(api_key_client, workspace, member_project, description="## Legacy\n\n- [ ] item")

        assert response.status_code == http_status.HTTP_201_CREATED, response.data
        html = _stored_html(response)
        assert "<h2>Legacy</h2>" in html
        assert task_item(False, "<p>item</p>") in html

    def test_markdown_wins_over_html_and_legacy_description(self, api_key_client, workspace, member_project):
        response = _create(
            api_key_client,
            workspace,
            member_project,
            description_markdown="# From markdown",
            description_html="<p>from html</p>",
            description="from legacy",
        )

        assert response.status_code == http_status.HTTP_201_CREATED, response.data
        assert _stored_html(response) == "<h1>From markdown</h1>"

    def test_html_wins_over_legacy_description(self, api_key_client, workspace, member_project):
        response = _create(
            api_key_client, workspace, member_project, description_html="<p>from html</p>", description="legacy"
        )

        assert response.status_code == http_status.HTTP_201_CREATED, response.data
        assert _stored_html(response) == "<p>from html</p>"

    def test_raw_html_in_markdown_is_escaped(self, api_key_client, workspace, member_project):
        response = _create(api_key_client, workspace, member_project, description_markdown="<script>alert(1)</script>")

        assert response.status_code == http_status.HTTP_201_CREATED, response.data
        assert "<script" not in _stored_html(response)

    def test_non_string_markdown_is_400(self, api_key_client, workspace, member_project):
        response = _create(api_key_client, workspace, member_project, description_markdown={"x": 1})

        assert response.status_code == http_status.HTTP_400_BAD_REQUEST
        assert "description_markdown" in response.data


@pytest.mark.contract
@pytest.mark.django_db
class TestPatch:
    @pytest.fixture
    def issue(self, api_key_client, workspace, member_project):
        response = _create(api_key_client, workspace, member_project, description_html="<p>old</p>")
        assert response.status_code == http_status.HTTP_201_CREATED, response.data
        return Issue.objects.get(pk=response.data["id"])

    def _patch(self, client, workspace, project, issue, **body):
        return client.patch(_detail_url(workspace, project, issue.id), body, format="json")

    def test_markdown_is_converted_with_the_shared_converter(self, api_key_client, workspace, member_project, issue):
        response = self._patch(api_key_client, workspace, member_project, issue, description_markdown=RICH_MARKDOWN)

        assert response.status_code == http_status.HTTP_200_OK, response.data
        issue.refresh_from_db()
        assert issue.description_html == markdown_to_html(RICH_MARKDOWN)
        assert_rich_structure(issue.description_html)
        assert "Cold start" in issue.description_stripped

    def test_activity_records_the_description_change(
        self, api_key_client, workspace, member_project, issue, _no_celery
    ):
        _no_celery.reset_mock()
        response = self._patch(api_key_client, workspace, member_project, issue, description_markdown="new")

        assert response.status_code == http_status.HTTP_200_OK, response.data
        requested = _no_celery.call_args.kwargs["requested_data"]
        # issue_activities_task tracks a description change off this key.
        assert '"description_html": "<p>new</p>"' in requested

    def test_description_html_is_still_accepted(self, api_key_client, workspace, member_project, issue):
        response = self._patch(api_key_client, workspace, member_project, issue, description_html="<p>edited</p>")

        assert response.status_code == http_status.HTTP_200_OK, response.data
        issue.refresh_from_db()
        assert issue.description_html == "<p>edited</p>"

    def test_legacy_description_is_converted(self, api_key_client, workspace, member_project, issue):
        response = self._patch(api_key_client, workspace, member_project, issue, description="# Legacy")

        assert response.status_code == http_status.HTTP_200_OK, response.data
        issue.refresh_from_db()
        assert issue.description_html == "<h1>Legacy</h1>"

    def test_markdown_wins_over_html(self, api_key_client, workspace, member_project, issue):
        response = self._patch(
            api_key_client,
            workspace,
            member_project,
            issue,
            description_markdown="- [x] done",
            description_html="<p>ignored</p>",
        )

        assert response.status_code == http_status.HTTP_200_OK, response.data
        issue.refresh_from_db()
        assert issue.description_html == markdown_to_html("- [x] done")
        assert "ignored" not in issue.description_html

    def test_empty_markdown_clears_the_description(self, api_key_client, workspace, member_project, issue):
        response = self._patch(api_key_client, workspace, member_project, issue, description_markdown="")

        assert response.status_code == http_status.HTTP_200_OK, response.data
        issue.refresh_from_db()
        assert issue.description_html == "<p></p>"

    def test_patch_without_description_keys_leaves_it_alone(self, api_key_client, workspace, member_project, issue):
        response = self._patch(api_key_client, workspace, member_project, issue, name="Renamed")

        assert response.status_code == http_status.HTTP_200_OK, response.data
        issue.refresh_from_db()
        assert issue.description_html == "<p>old</p>"
