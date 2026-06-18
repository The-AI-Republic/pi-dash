# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Contract tests for the ``pull_request`` branch of the GitHub App webhook.

The webhook refreshes the *display snapshot* of an attached
``GithubPullRequestLink`` — and only that. It never reads or writes the linked
issue's state. If no link matches the PR, the delivery is ``skipped``.
"""

import json
from uuid import uuid4
from unittest.mock import patch

import pytest
from django.urls import reverse
from django.utils import timezone
from rest_framework import status

from pi_dash.db.models import GithubPullRequestLink, GithubWebhookDelivery, Issue, Project, ProjectMember


pytestmark = [pytest.mark.contract, pytest.mark.django_db]


@pytest.fixture(autouse=True)
def _no_throttle(settings):
    settings.REST_FRAMEWORK = {**settings.REST_FRAMEWORK, "DEFAULT_THROTTLE_CLASSES": ()}


@pytest.fixture
def link(workspace, create_user):
    project = Project.objects.create(name="Hook P", identifier="HKP", workspace=workspace, created_by=create_user)
    ProjectMember.objects.create(project=project, member=create_user, role=20, is_active=True)
    issue = Issue.objects.create(name="i", project=project, workspace=workspace, created_by=create_user)
    return GithubPullRequestLink.objects.create(
        project=project, issue=issue, repo_owner="acme", repo_name="web", pr_number=42,
        url="https://github.com/acme/web/pull/42", state="open",
    )


def _pr_payload(*, action="closed", state="closed", merged=True, updated_at="2026-06-17T12:00:00Z",
                owner="Acme", name="Web", number=42, title="Make button blue", draft=False):
    return {
        "action": action,
        "pull_request": {
            "number": number, "title": title, "state": state, "draft": draft,
            "merged": merged, "merged_at": updated_at if merged else None, "updated_at": updated_at,
        },
        "repository": {"name": name, "owner": {"login": owner}},
    }


def _post(api_client, payload, event="pull_request"):
    with patch("pi_dash.app.views.integration.github.verify_webhook_signature", return_value=True):
        return api_client.post(
            reverse("github-app-webhook"),
            data=json.dumps(payload),
            content_type="application/json",
            HTTP_X_GITHUB_DELIVERY=str(uuid4()),
            HTTP_X_GITHUB_EVENT=event,
            HTTP_X_HUB_SIGNATURE_256="sha256=valid",
        )


def test_pull_request_refreshes_snapshot(api_client, link):
    issue_state_before = link.issue.state_id

    response = _post(api_client, _pr_payload())

    assert response.status_code == status.HTTP_202_ACCEPTED
    assert response.data == {"status": GithubWebhookDelivery.Status.PROCESSED}
    link.refresh_from_db()
    # case-insensitive repo match worked (payload owner "Acme" vs stored "acme")
    assert link.state == "closed"
    assert link.merged is True
    assert link.title == "Make button blue"
    assert link.pr_updated_at is not None
    # the issue's own state is never touched
    link.issue.refresh_from_db()
    assert link.issue.state_id == issue_state_before


def test_pull_request_with_no_link_is_skipped(api_client, link):
    response = _post(api_client, _pr_payload(number=999))

    assert response.status_code == status.HTTP_202_ACCEPTED
    assert response.data == {"status": GithubWebhookDelivery.Status.SKIPPED}
    link.refresh_from_db()
    assert link.state == "open"  # untouched


def test_pull_request_ignores_out_of_order_delivery(api_client, link):
    link.pr_updated_at = timezone.now()
    link.state = "open"
    link.save(update_fields=["pr_updated_at", "state"])

    # An older delivery must not regress the snapshot.
    response = _post(api_client, _pr_payload(state="closed", merged=True, updated_at="2020-01-01T00:00:00Z"))

    assert response.status_code == status.HTTP_202_ACCEPTED
    # It matched a real link (so it's processed, not skipped) but was not applied.
    assert response.data == {"status": GithubWebhookDelivery.Status.PROCESSED}
    link.refresh_from_db()
    assert link.state == "open"  # stale delivery ignored


def test_pull_request_draft_reflected(api_client, link):
    response = _post(api_client, _pr_payload(action="opened", state="open", merged=False, draft=True))

    assert response.data == {"status": GithubWebhookDelivery.Status.PROCESSED}
    link.refresh_from_db()
    assert link.draft is True
    assert link.merged is False
    assert link.state == "open"
