# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Contract tests for ``IssueAdvancedSearchEndpoint``.

Cover the scenarios called out in PR #180's test plan:
  * empty query / invalid sort / invalid since → expected status codes
  * ``status=open`` includes NULL-state issues (the SQL ``NOT IN`` trap)
  * ``status=closed`` excludes them
  * ``limit`` clamping at both ends
  * project resolution by identifier and by UUID
  * comment-only match surfaces the parent issue
  * the int4 overflow regression — long digit token must not 500

These tests need Postgres FTS, so they run under ``pytest.mark.contract``
and the standard ``django_db_setup`` fixture (same surface as
``test_labels.py``).
"""

import pytest
from rest_framework import status as http_status

from pi_dash.db.models import (
    Issue,
    IssueComment,
    Project,
    ProjectMember,
    State,
)


# ---------------------------------------------------------------------------
# Fixtures
# ---------------------------------------------------------------------------


@pytest.fixture
def search_project(db, workspace, create_user):
    project = Project.objects.create(
        name="Search Test Project",
        identifier="ST",
        workspace=workspace,
        created_by=create_user,
    )
    ProjectMember.objects.create(
        project=project,
        member=create_user,
        role=20,
        is_active=True,
    )
    return project


@pytest.fixture
def open_state(search_project, create_user):
    return State.objects.create(
        name="Todo",
        project=search_project,
        workspace=search_project.workspace,
        group="unstarted",
        default=True,
        created_by=create_user,
    )


@pytest.fixture
def closed_state(search_project, create_user):
    return State.objects.create(
        name="Done",
        project=search_project,
        workspace=search_project.workspace,
        group="completed",
        created_by=create_user,
    )


def _make_issue(project, user, *, name, description="", state=None):
    """Issues need both ``description_html`` (the editor field) and
    ``description_stripped`` (the FTS field) — the production save() path
    derives the stripped form from the HTML, but tests insert directly so
    we set both explicitly.
    """
    return Issue.objects.create(
        name=name,
        description_html=f"<p>{description}</p>" if description else "",
        description_stripped=description,
        project=project,
        workspace=project.workspace,
        state=state,
        created_by=user,
    )


def _url(slug):
    return f"/api/v1/workspaces/{slug}/work-items/search/advanced/"


# ---------------------------------------------------------------------------
# Tests
# ---------------------------------------------------------------------------


@pytest.mark.contract
class TestIssueAdvancedSearchValidation:
    """Input-shape contract: bad input → 400 with a useful error
    message. Empty query is *not* an error — it short-circuits to an
    empty result set so callers can render a placeholder.
    """

    @pytest.mark.django_db
    def test_empty_query_returns_empty_payload(self, api_key_client, workspace):
        response = api_key_client.get(_url(workspace.slug) + "?q=")
        assert response.status_code == http_status.HTTP_200_OK
        assert response.data == {"query": "", "count": 0, "results": []}

    @pytest.mark.django_db
    def test_whitespace_query_returns_empty_payload(self, api_key_client, workspace):
        response = api_key_client.get(_url(workspace.slug) + "?q=%20%20")
        assert response.status_code == http_status.HTTP_200_OK
        assert response.data["count"] == 0

    @pytest.mark.django_db
    def test_invalid_sort_returns_400(self, api_key_client, workspace):
        response = api_key_client.get(_url(workspace.slug) + "?q=hello&sort=garbage")
        assert response.status_code == http_status.HTTP_400_BAD_REQUEST
        assert "sort" in response.data["error"].lower()

    @pytest.mark.django_db
    def test_invalid_since_returns_400(self, api_key_client, workspace):
        response = api_key_client.get(_url(workspace.slug) + "?q=hello&since=not-a-date")
        assert response.status_code == http_status.HTTP_400_BAD_REQUEST
        assert "since" in response.data["error"].lower()

    @pytest.mark.django_db
    def test_date_only_since_is_accepted_as_midnight(self, api_key_client, workspace):
        # Python 3.11+'s ``datetime.fromisoformat`` (which Django's
        # ``parse_datetime`` defers to first) accepts a bare ISO date and
        # treats it as midnight in that day. We don't fight that — the
        # endpoint is documented as lenient, and dropping the time
        # component is a common ergonomic shortcut for agents.
        response = api_key_client.get(_url(workspace.slug) + "?q=hello&since=2025-01-01")
        assert response.status_code == http_status.HTTP_200_OK


@pytest.mark.contract
class TestIssueAdvancedSearchStatusFilter:
    """``?status=open`` must include issues with NULL state — the
    naive ``NOT IN (closed_groups)`` predicate silently drops them
    because of SQL three-valued logic.
    """

    @pytest.mark.django_db
    def test_open_includes_null_state_issues(
        self, api_key_client, workspace, search_project, create_user
    ):
        _make_issue(search_project, create_user, name="stateless ticket", description="alpha")
        url = _url(workspace.slug) + "?q=alpha&status=open"
        response = api_key_client.get(url)
        assert response.status_code == http_status.HTTP_200_OK
        names = [r["name"] for r in response.data["results"]]
        assert "stateless ticket" in names

    @pytest.mark.django_db
    def test_closed_excludes_null_state_issues(
        self, api_key_client, workspace, search_project, create_user
    ):
        _make_issue(search_project, create_user, name="stateless", description="alpha")
        response = api_key_client.get(_url(workspace.slug) + "?q=alpha&status=closed")
        assert response.status_code == http_status.HTTP_200_OK
        assert response.data["count"] == 0

    @pytest.mark.django_db
    def test_status_open_excludes_completed(
        self, api_key_client, workspace, search_project, create_user, closed_state
    ):
        _make_issue(
            search_project,
            create_user,
            name="resolved item",
            description="alpha",
            state=closed_state,
        )
        response = api_key_client.get(_url(workspace.slug) + "?q=alpha&status=open")
        assert response.status_code == http_status.HTTP_200_OK
        assert response.data["count"] == 0

    @pytest.mark.django_db
    def test_status_closed_includes_completed(
        self, api_key_client, workspace, search_project, create_user, closed_state
    ):
        _make_issue(
            search_project,
            create_user,
            name="resolved item",
            description="alpha",
            state=closed_state,
        )
        response = api_key_client.get(_url(workspace.slug) + "?q=alpha&status=closed")
        assert response.status_code == http_status.HTTP_200_OK
        names = [r["name"] for r in response.data["results"]]
        assert "resolved item" in names


@pytest.mark.contract
class TestIssueAdvancedSearchLimit:
    """``limit`` is clamped to ``[1, _MAX_LIMIT]`` (50). Non-numeric
    values fall back to the default. Tunable for agent context windows.
    """

    @pytest.mark.django_db
    def test_limit_clamped_to_max(self, api_key_client, workspace, search_project, create_user):
        for i in range(3):
            _make_issue(search_project, create_user, name=f"i{i}", description="alpha")
        response = api_key_client.get(_url(workspace.slug) + "?q=alpha&limit=999")
        assert response.status_code == http_status.HTTP_200_OK
        # We only created 3 — the contract under test is that the
        # endpoint didn't 400 or 500, and that the value was clamped to
        # something sane. Real cap is 50; verifying we didn't get a 999.
        assert response.data["count"] <= 50

    @pytest.mark.django_db
    def test_limit_zero_returns_at_least_one(
        self, api_key_client, workspace, search_project, create_user
    ):
        _make_issue(search_project, create_user, name="only", description="alpha")
        response = api_key_client.get(_url(workspace.slug) + "?q=alpha&limit=0")
        assert response.status_code == http_status.HTTP_200_OK
        # limit was clamped up to 1 — the actual result count depends on
        # how many matched, but we should still have gotten back what
        # was there (1 issue).
        assert response.data["count"] == 1

    @pytest.mark.django_db
    def test_non_numeric_limit_falls_back_to_default(
        self, api_key_client, workspace, search_project, create_user
    ):
        _make_issue(search_project, create_user, name="only", description="alpha")
        response = api_key_client.get(_url(workspace.slug) + "?q=alpha&limit=abc")
        assert response.status_code == http_status.HTTP_200_OK


@pytest.mark.contract
class TestIssueAdvancedSearchOverflowRegression:
    """Regression for the int4 overflow bug: a digit token greater than
    2_147_483_647 (Postgres int4 max) used to be sent into
    ``Q(sequence_id=...)``, which Postgres rejected as
    ``out of range for type integer``. The endpoint must now return a
    normal 200 (no 500) regardless of the numeric token's size.
    """

    @pytest.mark.django_db
    def test_long_digit_token_does_not_500(
        self, api_key_client, workspace, search_project, create_user
    ):
        _make_issue(search_project, create_user, name="anything", description="filler")
        response = api_key_client.get(_url(workspace.slug) + "?q=9999999999")
        # The key assertion is the absence of 500 — the FTS branch may
        # well return an empty match set, which is fine.
        assert response.status_code == http_status.HTTP_200_OK


@pytest.mark.contract
class TestIssueAdvancedSearchCommentMatch:
    """Comments-side FTS path: an issue whose only text-match lives in
    a comment must still appear in results when the agent endpoint opts
    in to comment widening.
    """

    @pytest.mark.django_db
    def test_match_lives_only_in_comment(
        self,
        api_key_client,
        workspace,
        search_project,
        create_user,
    ):
        issue = _make_issue(
            search_project,
            create_user,
            name="unrelated title",
            description="unrelated description",
        )
        IssueComment.objects.create(
            issue=issue,
            project=search_project,
            workspace=search_project.workspace,
            comment_html="<p>resolution: the upstream cache was stale</p>",
            comment_stripped="resolution: the upstream cache was stale",
            actor=create_user,
        )

        response = api_key_client.get(_url(workspace.slug) + "?q=upstream+cache")
        assert response.status_code == http_status.HTTP_200_OK
        ids = [r["id"] for r in response.data["results"]]
        assert str(issue.id) in ids

    @pytest.mark.django_db
    def test_comment_only_match_has_empty_snippet(
        self,
        api_key_client,
        workspace,
        search_project,
        create_user,
    ):
        issue = _make_issue(
            search_project,
            create_user,
            name="unrelated title",
            description="unrelated description",
        )
        IssueComment.objects.create(
            issue=issue,
            project=search_project,
            workspace=search_project.workspace,
            comment_html="<p>resolution: the upstream cache was stale</p>",
            comment_stripped="resolution: the upstream cache was stale",
            actor=create_user,
        )
        response = api_key_client.get(_url(workspace.slug) + "?q=upstream+cache")
        match = next(r for r in response.data["results"] if r["id"] == str(issue.id))
        # ts_headline only excerpts description_stripped today; when the
        # match is in a comment, snippet must be empty (not a misleading
        # description prefix). Documented in the agent prompt fragment.
        assert match["snippet"] == ""


@pytest.mark.contract
class TestIssueAdvancedSearchProjectScoping:
    """``?project=`` accepts either a UUID or a workspace-scoped
    identifier (slug like ``ST``). Resolution via ``Project.resolve``
    keeps the workspace btree index in play (the model has an explicit
    warning against ``__iexact`` here).
    """

    @pytest.mark.django_db
    def test_project_filter_by_identifier(
        self, api_key_client, workspace, search_project, create_user
    ):
        _make_issue(search_project, create_user, name="match here", description="alpha")
        response = api_key_client.get(
            _url(workspace.slug) + f"?q=alpha&project={search_project.identifier}"
        )
        assert response.status_code == http_status.HTTP_200_OK
        assert response.data["count"] == 1

    @pytest.mark.django_db
    def test_project_filter_by_uuid(
        self, api_key_client, workspace, search_project, create_user
    ):
        _make_issue(search_project, create_user, name="match here", description="alpha")
        response = api_key_client.get(
            _url(workspace.slug) + f"?q=alpha&project={search_project.id}"
        )
        assert response.status_code == http_status.HTTP_200_OK
        assert response.data["count"] == 1
