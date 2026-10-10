# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Corpus: app-tier issues write flows (PIDASHCONV-823).

Built from the ``app_issues`` contract suite (PIDASHCONV-84):
test_issue_core.py (create/patch/put/delete), test_activity_comments.py
(comments, reactions, subscribe), test_sub_issues.py (assign),
test_archive.py (archive/unarchive).
"""

from ..session import Request, Session

_WS = "/api/workspaces/{ws}/projects/{project}"
_ISSUES = _WS + "/issues/"
_ISSUE = _WS + "/issues/{issue}/"
_COMMENTS = _ISSUE + "comments/"
_COMMENT = _COMMENTS + "{comment}/"

SESSIONS = (
    Session(
        name="app_issues_lifecycle",
        domain="app_issues",
        source="app_issues/test_issue_core.py, test_activity_comments.py, "
        "test_sub_issues.py, test_archive.py (PIDASHCONV-84)",
        auth_users=("admin", "member"),
        # issue_activity carries int(timezone.now()) as `epoch`: the two
        # backends' requests can straddle a second boundary, so the exact
        # second compares within the plausibly-now window instead.
        payload_time_paths={
            "pi_dash.bgtasks.issue_activities_task.issue_activity": ["kwargs/epoch"],
        },
        requests=(
            Request(
                "POST", _ISSUES,
                route="/api/workspaces/{slug}/projects/{project_id}/issues/",
                json={"name": "Replay parent", "priority": "high"},
                auth="admin", capture={"issue": "id"}, expect_status=201,
            ),
            Request(
                "PATCH", _ISSUE,
                route="/api/workspaces/{slug}/projects/{project_id}/issues/{pk}/",
                json={"priority": "urgent"},
                auth="admin", expect_status=204,
            ),
            Request(
                "PUT", _ISSUE,
                route="/api/workspaces/{slug}/projects/{project_id}/issues/{pk}/",
                json={"name": "Replay parent v2", "priority": "low"},
                auth="admin", expect_status=200,
            ),
            Request(
                "POST", _COMMENTS,
                route="/api/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/comments/",
                json={"comment_html": "<p>replay note</p>", "comment_stripped": "replay note"},
                auth="admin", capture={"comment": "id", "comment_desc": "description"},
                expect_status=201,
            ),
            Request(
                "PATCH", _COMMENT,
                route="/api/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/comments/{pk}/",
                json={"comment_html": "<p>replay note v2</p>", "comment_stripped": "replay note v2"},
                auth="admin", expect_status=200,
            ),
            Request(
                "POST", _WS + "/comments/{comment}/reactions/",
                route="/api/workspaces/{slug}/projects/{project_id}/comments/{pk}/reactions/",
                json={"reaction": "+1"},
                auth="member", capture={"comment_reaction": "id"},
                expect_status=201,
            ),
            Request(
                "POST", _ISSUE + "reactions/",
                route="/api/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/reactions/",
                json={"reaction": "rocket"},
                auth="member", capture={"issue_reaction": "id"},
                expect_status=201,
            ),
            Request(
                "POST", _ISSUE + "subscribe/",
                route="/api/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/subscribe/",
                auth="member", capture={"subscription": "id"},
                expect_status=201,
            ),
            Request(
                "POST", _ISSUES,
                route="/api/workspaces/{slug}/projects/{project_id}/issues/",
                json={"name": "Replay child"},
                auth="admin", capture={"child": "id"}, expect_status=201,
            ),
            Request(
                "POST", _ISSUE + "sub-issues/",
                route="/api/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/sub-issues/",
                json={"sub_issue_ids": ["{child}"]},
                auth="admin", expect_status=200,
            ),
            Request(
                "PATCH", _ISSUE,
                route="/api/workspaces/{slug}/projects/{project_id}/issues/{pk}/",
                json={"state_id": "{state_completed}"},
                auth="admin", expect_status=204,
            ),
            Request(
                "POST", _ISSUE + "archive/",
                route="/api/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/archive/",
                auth="admin", expect_status=200,
            ),
            Request(
                "DELETE", _ISSUE + "archive/",
                route="/api/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/archive/",
                auth="admin", expect_status=204,
            ),
            Request(
                "DELETE", _ISSUE + "subscribe/",
                route="/api/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/subscribe/",
                auth="member", expect_status=204,
            ),
            Request(
                "DELETE", _WS + "/comments/{comment}/reactions/+1/",
                route="/api/workspaces/{slug}/projects/{project_id}/comments/{pk}/reactions/{reaction}/",
                auth="member", expect_status=204,
            ),
            Request(
                "DELETE", _ISSUE + "reactions/rocket/",
                route="/api/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/reactions/{reaction}/",
                auth="member", expect_status=204,
            ),
            Request(
                "DELETE", _COMMENT,
                route="/api/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/comments/{pk}/",
                auth="admin", expect_status=204,
            ),
            Request(
                "DELETE", _WS + "/issues/{child}/",
                route="/api/workspaces/{slug}/projects/{project_id}/issues/{pk}/",
                auth="admin", expect_status=204,
            ),
            Request(
                "DELETE", _ISSUE,
                route="/api/workspaces/{slug}/projects/{project_id}/issues/{pk}/",
                auth="admin", expect_status=204,
            ),
        ),
    ),
)
