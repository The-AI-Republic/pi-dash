# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Corpus: app-tier issues write flows (PIDASHCONV-823).

Built from the ``app_issues`` contract suite (PIDASHCONV-84):
test_issue_core.py (create/patch/put/delete), test_activity_comments.py
(comments, reactions, subscribe), test_sub_issues.py (assign),
test_archive.py (archive/unarchive). The follow-up session below covers
the rest of the domain: test_labels.py, test_relations_links.py,
test_attachments.py, test_versions_move.py, the bulk legs of
test_archive.py, and the dates/user-properties legs of
test_issue_core.py.
"""

from psycopg.rows import dict_row

from ..session import Request, Session

_WS = "/api/workspaces/{ws}/projects/{project}"
_ISSUES = _WS + "/issues/"
_ISSUE = _WS + "/issues/{issue}/"
_COMMENTS = _ISSUE + "comments/"
_COMMENT = _COMMENTS + "{comment}/"

# Fixed ids seeded by _seed_followup (addressed via static_context).
PROJ2_ID = "cccccccc-0000-4000-8000-000000000001"
PROJ2_STATE_ID = "cccccccc-0000-4000-8000-000000000002"
ISSUE_A_ID = "cccccccc-0000-4000-8000-000000000003"
ASSET1_ID = "cccccccc-0000-4000-8000-000000000004"
ASSET2_ID = "cccccccc-0000-4000-8000-000000000005"
PR_LINK_ID = "cccccccc-0000-4000-8000-000000000006"
REVIEW1_ID = "cccccccc-0000-4000-8000-000000000007"
REVIEW2_ID = "cccccccc-0000-4000-8000-000000000008"


def _seed_followup(conn) -> None:
    """Anchor rows the follow-up session addresses by fixed id.

    Mirrors the ``app_issues/conftest.py`` SQL (same columns/values):
    a second project RPJ2 with a default backlog state (move target),
    an anchor issue on RPJ (seq 901, clear of API-created sequences),
    a v1-uploaded + a v2-pending file asset, and PR + code-review links
    (iid 7 shares the PR's repo/number for the cascade leg, iid 8 for
    the no-cascade leg).
    """
    with conn.cursor(row_factory=dict_row) as cur:
        cur.execute("SELECT id FROM workspaces WHERE slug = 'replay-ws'")
        ws_id = str(cur.fetchone()["id"])
        cur.execute("SELECT id FROM projects WHERE identifier = 'RPJ'")
        project_id = str(cur.fetchone()["id"])
        cur.execute(
            "SELECT id FROM states WHERE workspace_id = %s AND \"group\" = 'backlog'",
            (ws_id,),
        )
        backlog_id = str(cur.fetchone()["id"])
        cur.execute(
            "SELECT id FROM users WHERE email = 'replay-admin@example.com'"
        )
        admin_id = str(cur.fetchone()["id"])
    with conn.cursor() as cur:
        cur.execute(
            """
            INSERT INTO projects (created_at, updated_at, id, name, description, context,
                network, identifier, workspace_id, cycle_view, module_view,
                issue_views_view, page_view, intake_view, archive_in, close_in,
                logo_props, is_time_tracking_enabled, is_issue_type_enabled,
                guest_view_all_features, timezone, members_can_edit_states,
                repo_url, base_branch, agent_default_interval_seconds,
                agent_default_max_ticks, agent_ticking_enabled, is_default,
                agent_review_default_interval_seconds, default_agent_executor,
                agent_test_default_interval_seconds)
            VALUES (now(), now(), %s, 'Replay Project 2', '', '', 0, 'RPJ2', %s,
                true, true, true, true, true, 0, 0, '{}', true, true,
                true, 'UTC', true, '', '', 3600, 100, true, false, 10800,
                'muse', 10800)
            """,
            (PROJ2_ID, ws_id),
        )
        cur.execute(
            """
            INSERT INTO states (created_at, updated_at, id, name, description,
                color, slug, project_id, workspace_id, sequence, "group",
                "default", is_triage)
            VALUES (now(), now(), %s, 'RPJ2 Backlog', '', '#000000',
                'rpj2-backlog', %s, %s, 0.0, 'backlog', true, false)
            """,
            (PROJ2_STATE_ID, PROJ2_ID, ws_id),
        )
        cur.execute(
            """
            INSERT INTO issues (created_at, updated_at, id, name,
                description_json, priority, sequence_id, created_by_id, project_id,
                state_id, workspace_id, description_html, sort_order, point,
                completed_at, is_draft, git_work_branch, workpad, complexity_score)
            VALUES (now(), now(), %s, 'Replay anchor', '{}', 'high', 901, %s,
                %s, %s, %s, '', 0.0, null, null, false, '', '', 0)
            """,
            (ISSUE_A_ID, admin_id, project_id, backlog_id, ws_id),
        )
        for asset_id, name, uploaded, size in (
            (ASSET1_ID, "seed.txt", True, 12),
            (ASSET2_ID, "pending.bin", False, 10),
        ):
            cur.execute(
                """
                INSERT INTO file_assets (created_at, updated_at, id, attributes,
                    asset, created_by_id, updated_by_id, workspace_id, is_deleted,
                    is_archived, entity_type, is_uploaded, issue_id, project_id,
                    size, user_id)
                VALUES (now(), now(), %s, %s, %s, %s, %s, %s, false, false,
                    'ISSUE_ATTACHMENT', %s, %s, %s, %s, %s)
                """,
                (asset_id, '{"name": "%s"}' % name, name, admin_id, admin_id,
                 ws_id, uploaded, ISSUE_A_ID, project_id, size, admin_id),
            )
        cur.execute(
            """
            INSERT INTO github_pull_request_links (created_at, updated_at, id,
                repo_owner, repo_name, pr_number, url, title, state, merged, draft,
                created_by_id, issue_id, project_id, updated_by_id, workspace_id)
            VALUES (now(), now(), %s, 'octo', 'demo', 7,
                'https://github.com/octo/demo/pull/7', 'Seed PR', 'open',
                false, false, %s, %s, %s, %s, %s)
            """,
            (PR_LINK_ID, admin_id, ISSUE_A_ID, project_id, admin_id, ws_id),
        )
        for review_id, iid, url, title in (
            (REVIEW1_ID, "7", "https://github.com/octo/demo/pull/7", "Seed review"),
            (REVIEW2_ID, "8", "https://github.com/octo/demo/pull/8", "Seed review 2"),
        ):
            cur.execute(
                """
                INSERT INTO git_code_review_links (created_at, updated_at, id,
                    provider, host_url, namespace, repo_name, external_iid, url,
                    title, state, merged, draft, created_by_id, issue_id,
                    project_id, updated_by_id, workspace_id, external_id,
                    repo_external_id, metadata)
                VALUES (now(), now(), %s, 'github', 'https://github.com',
                    'octo', 'demo', %s, %s, %s, 'open', false, false,
                    %s, %s, %s, %s, %s, '', '', '{}')
                """,
                (review_id, iid, url, title, admin_id, ISSUE_A_ID,
                 project_id, admin_id, ws_id),
            )
    conn.commit()

    # Project memberships for RPJ2 (admin/member/guest, mirroring the
    # world seed) so the move legs resolve the target tenant-side.
    from _harness import db as db_helpers

    for key, role in (("admin", db_helpers.ADMIN),
                      ("member", db_helpers.MEMBER),
                      ("guest", db_helpers.GUEST)):
        with conn.cursor(row_factory=dict_row) as cur:
            cur.execute(
                "SELECT id FROM users WHERE email = %s",
                ("replay-%s@example.com" % key,),
            )
            user_id = str(cur.fetchone()["id"])
        db_helpers.add_project_member(conn, ws_id, PROJ2_ID, user_id, role)


_F = _WS  # follow-up legs address the same project base
_F2 = "/api/workspaces/{ws}/projects/{proj2}"
_V2 = "/api/assets/v2/workspaces/{ws}/projects/{project}/issues/{issue_a}/attachments"

_SOFT_DELETE_TASK = "pi_dash.bgtasks.deletion_task.soft_delete_related_objects"


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
    Session(
        name="app_issues_followup",
        domain="app_issues",
        source="app_issues/test_labels.py, test_relations_links.py, "
        "test_attachments.py, test_versions_move.py, test_archive.py "
        "(bulk legs), test_issue_core.py (dates/user-properties legs), "
        "test_activity_comments.py (comment PUT, subscribers) (PIDASHCONV-84)",
        auth_users=("admin", "member", "guest"),
        seed=_seed_followup,
        static_context={
            "proj2": PROJ2_ID,
            "proj2_state": PROJ2_STATE_ID,
            "issue_a": ISSUE_A_ID,
            "asset1": ASSET1_ID,
            "asset2": ASSET2_ID,
            "pr_link": PR_LINK_ID,
            "review1": REVIEW1_ID,
            "review2": REVIEW2_ID,
        },
        payload_time_paths={
            "pi_dash.bgtasks.issue_activities_task.issue_activity": ["kwargs/epoch"],
        },
        # BulkCreateIssueLabelsEndpoint mints color=f"#{random...}" per
        # row (app/views/issue/label.py:104), ignoring any input color:
        # minted per backend, so the column compares blanked (the
        # explicit-color single-create legs still pin color in the
        # response bodies).
        generated_columns={"labels": ["color"]},
        payload_row_refs={
            _SOFT_DELETE_TASK: [
                # remove-relation enqueues the issue_relation row pk,
                # which no response ever reveals (the create response
                # returns the related issue, not the bridge row).
                ("args/2", "issue_relations"),
            ],
        },
        requests=(
            # Labels: create -> duplicate-400 -> bulk -> patch -> put ->
            # member-denied x2 -> deletes.
            Request(
                "POST", _F + "/issue-labels/",
                route="/api/workspaces/{slug}/projects/{project_id}/issue-labels/",
                json={"name": "L-cycle", "color": "#123456"},
                auth="admin", capture={"label": "id"}, expect_status=201,
            ),
            Request(
                "POST", _F + "/issue-labels/",
                route="/api/workspaces/{slug}/projects/{project_id}/issue-labels/",
                json={"name": "L-cycle", "color": "#123456"},
                auth="admin", expect_status=400,
            ),
            Request(
                "POST", _F + "/bulk-create-labels/",
                route="/api/workspaces/{slug}/projects/{project_id}/bulk-create-labels/",
                json={"label_data": [{"name": "LB1"}, {"name": "LB2"}]},
                auth="admin", capture={"lb1": "labels/0/id", "lb2": "labels/1/id"},
                # Random colors minted per backend (see
                # generated_columns); presence-asserted here.
                body_blank_paths=["labels/0/color", "labels/1/color"],
                expect_status=201,
            ),
            Request(
                "PATCH", _F + "/issue-labels/{label}/",
                route="/api/workspaces/{slug}/projects/{project_id}/issue-labels/{pk}/",
                json={"description": "cycled"},
                auth="admin", expect_status=200,
            ),
            Request(
                "PUT", _F + "/issue-labels/{label}/",
                route="/api/workspaces/{slug}/projects/{project_id}/issue-labels/{pk}/",
                json={"name": "L-cycle", "color": "#00ff00"},
                auth="admin", expect_status=200,
            ),
            Request(
                "PATCH", _F + "/issue-labels/{label}/",
                route="/api/workspaces/{slug}/projects/{project_id}/issue-labels/{pk}/",
                json={"description": "x"},
                auth="member", expect_status=403,
            ),
            Request(
                "POST", _F + "/issue-labels/",
                route="/api/workspaces/{slug}/projects/{project_id}/issue-labels/",
                json={"name": "L-member", "color": "#123456"},
                auth="member", expect_status=403,
            ),
            Request(
                "DELETE", _F + "/issue-labels/{lb1}/",
                route="/api/workspaces/{slug}/projects/{project_id}/issue-labels/{pk}/",
                auth="admin", expect_status=204,
            ),
            Request(
                "DELETE", _F + "/issue-labels/{lb2}/",
                route="/api/workspaces/{slug}/projects/{project_id}/issue-labels/{pk}/",
                auth="admin", expect_status=204,
            ),
            Request(
                "DELETE", _F + "/issue-labels/{label}/",
                route="/api/workspaces/{slug}/projects/{project_id}/issue-labels/{pk}/",
                auth="admin", expect_status=204,
            ),
            # Relations: two API issues, requires-type-400, relates_to
            # round-trip, blocking-branch round-trip.
            Request(
                "POST", _ISSUES,
                route="/api/workspaces/{slug}/projects/{project_id}/issues/",
                json={"name": "Rel A"},
                auth="admin", capture={"rel_a": "id"}, expect_status=201,
            ),
            Request(
                "POST", _ISSUES,
                route="/api/workspaces/{slug}/projects/{project_id}/issues/",
                json={"name": "Rel B"},
                auth="admin", capture={"rel_b": "id"}, expect_status=201,
            ),
            Request(
                "POST", _F + "/issues/{rel_a}/issue-relation/",
                route="/api/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/issue-relation/",
                json={"issues": ["{rel_b}"]},
                auth="admin", expect_status=400,
            ),
            Request(
                "POST", _F + "/issues/{rel_a}/issue-relation/",
                route="/api/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/issue-relation/",
                json={"relation_type": "relates_to", "issues": ["{rel_b}"]},
                auth="admin", expect_status=201,
            ),
            Request(
                "POST", _F + "/issues/{rel_a}/remove-relation/",
                route="/api/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/remove-relation/",
                json={"related_issue": "{rel_b}"},
                auth="admin", expect_status=204,
            ),
            Request(
                "POST", _F + "/issues/{rel_a}/issue-relation/",
                route="/api/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/issue-relation/",
                json={"relation_type": "blocking", "issues": ["{rel_b}"]},
                auth="admin", expect_status=201,
            ),
            Request(
                "POST", _F + "/issues/{rel_a}/remove-relation/",
                route="/api/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/remove-relation/",
                json={"related_issue": "{rel_b}"},
                auth="admin", expect_status=204,
            ),
            # Links: post -> patch -> put -> delete.
            Request(
                "POST", _F + "/issues/{rel_a}/issue-links/",
                route="/api/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/issue-links/",
                json={"title": "docs", "url": "https://example.com/docs"},
                auth="admin", capture={"link": "id"}, expect_status=201,
            ),
            Request(
                "PATCH", _F + "/issues/{rel_a}/issue-links/{link}/",
                route="/api/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/issue-links/{pk}/",
                json={"title": "docs v2"},
                auth="admin", expect_status=200,
            ),
            Request(
                "PUT", _F + "/issues/{rel_a}/issue-links/{link}/",
                route="/api/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/issue-links/{pk}/",
                json={"title": "docs v3", "url": "https://example.com/docs"},
                auth="admin", expect_status=200,
            ),
            Request(
                "DELETE", _F + "/issues/{rel_a}/issue-links/{link}/",
                route="/api/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/issue-links/{pk}/",
                auth="admin", expect_status=204,
            ),
            # PR + code-review links: invalid-URL 400s cover the POSTs
            # (attaching needs the live GitHub API); the seeded rows
            # cover the DELETEs, review #8 without cascade first, then
            # PR #7 cascading review #7.
            Request(
                "POST", _F + "/issues/{issue_a}/github-pull-requests/",
                route="/api/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/github-pull-requests/",
                json={"url": "not-a-url"},
                auth="admin", expect_status=400,
            ),
            Request(
                "POST", _F + "/issues/{issue_a}/code-reviews/",
                route="/api/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/code-reviews/",
                json={"url": "not-a-url"},
                auth="admin", expect_status=400,
            ),
            Request(
                "DELETE", _F + "/issues/{issue_a}/code-reviews/{review2}/",
                route="/api/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/code-reviews/{pk}/",
                auth="admin", expect_status=204,
            ),
            Request(
                "DELETE", _F + "/issues/{issue_a}/github-pull-requests/{pr_link}/",
                route="/api/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/github-pull-requests/{pk}/",
                auth="admin", expect_status=204,
            ),
            # Comment PUT round-trip.
            Request(
                "POST", _F + "/issues/{rel_a}/comments/",
                route="/api/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/comments/",
                json={"comment_html": "<p>put me</p>", "comment_stripped": "put me"},
                auth="admin", capture={"cput": "id", "cput_desc": "description"},
                expect_status=201,
            ),
            Request(
                "PUT", _F + "/issues/{rel_a}/comments/{cput}/",
                route="/api/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/comments/{pk}/",
                json={"comment_html": "<p>put</p>", "comment_stripped": "put"},
                auth="admin", expect_status=200,
            ),
            Request(
                "DELETE", _F + "/issues/{rel_a}/comments/{cput}/",
                route="/api/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/comments/{pk}/",
                auth="admin", expect_status=204,
            ),
            # Subscribers: bare-id 400, explicit-nulls 201, destroy by
            # subscriber (user) id.
            Request(
                "POST", _F + "/issues/{rel_b}/issue-subscribers/",
                route="/api/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/issue-subscribers/",
                json={"subscriber": "{guest}"},
                auth="admin", expect_status=400,
            ),
            Request(
                "POST", _F + "/issues/{rel_b}/issue-subscribers/",
                route="/api/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/issue-subscribers/",
                json={"subscriber": "{guest}", "deleted_at": None},
                auth="admin", capture={"sub": "id"}, expect_status=201,
            ),
            Request(
                "DELETE", _F + "/issues/{rel_b}/issue-subscribers/{guest}/",
                route="/api/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/issue-subscribers/{pk}/",
                auth="admin", expect_status=204,
            ),
            # v1 attachments: empty-post 400, valid-file 500 (ported
            # S3Storage bug), missing-delete 404, seeded-delete 500 (the
            # row survives — the 500 fires before the delete).
            Request(
                "POST", _F + "/issues/{issue_a}/issue-attachments/",
                route="/api/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/issue-attachments/",
                auth="admin", expect_status=400,
            ),
            Request(
                "POST", _F + "/issues/{issue_a}/issue-attachments/",
                route="/api/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/issue-attachments/",
                files={"asset": ("probe.txt", b"hello probe", "text/plain")},
                auth="admin", expect_status=500,
            ),
            Request(
                "DELETE", _F + "/issues/{issue_a}/issue-attachments/00000000-0000-0000-0000-000000000000/",
                route="/api/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/issue-attachments/{pk}/",
                auth="admin", expect_status=404,
            ),
            Request(
                "DELETE", _F + "/issues/{issue_a}/issue-attachments/{asset1}/",
                route="/api/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/issue-attachments/{pk}/",
                auth="admin", expect_status=500,
            ),
            # v2 attachments: invalid-type 400 covers the POST (valid
            # types need a bucket to presign against), PATCH flips the
            # pending asset to uploaded, DELETE soft-deletes.
            Request(
                "POST", _V2 + "/",
                route="/api/assets/v2/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/attachments/",
                json={"name": "x.exe", "type": "application/x-msdownload", "size": 10},
                auth="admin", expect_status=400,
            ),
            Request(
                "PATCH", _V2 + "/{asset2}/",
                route="/api/assets/v2/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/attachments/{pk}/",
                auth="admin", expect_status=204,
            ),
            Request(
                "DELETE", _V2 + "/{asset1}/",
                route="/api/assets/v2/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/attachments/{pk}/",
                auth="admin", expect_status=204,
            ),
            # Bulk dates + user properties.
            Request(
                "POST", _F + "/issue-dates/",
                route="/api/workspaces/{slug}/projects/{project_id}/issue-dates/",
                json={"updates": [{"id": "{rel_a}", "start_date": "2026-01-01",
                                   "target_date": "2026-02-01"}]},
                auth="admin", expect_status=200,
            ),
            Request(
                "POST", _F + "/issue-dates/",
                route="/api/workspaces/{slug}/projects/{project_id}/issue-dates/",
                json={"updates": [{"id": "{rel_a}", "start_date": "2026-03-01",
                                   "target_date": "2026-02-01"}]},
                auth="admin", expect_status=400,
            ),
            Request(
                "GET", _F + "/user-properties/",
                route="/api/workspaces/{slug}/projects/{project_id}/user-properties/",
                auth="member", capture={"uprops": "id"}, expect_status=200,
            ),
            Request(
                "PATCH", _F + "/user-properties/",
                route="/api/workspaces/{slug}/projects/{project_id}/user-properties/",
                json={"display_properties": {"key": "priority"}},
                auth="member", expect_status=200,
            ),
            # Bulk archive: throwaways to completed, archive, open-state
            # 400, empty 400, unarchive, delete.
            Request(
                "POST", _ISSUES,
                route="/api/workspaces/{slug}/projects/{project_id}/issues/",
                json={"name": "Bulk a"},
                auth="admin", capture={"bulk_a": "id"}, expect_status=201,
            ),
            Request(
                "POST", _ISSUES,
                route="/api/workspaces/{slug}/projects/{project_id}/issues/",
                json={"name": "Bulk b"},
                auth="admin", capture={"bulk_b": "id"}, expect_status=201,
            ),
            Request(
                "PATCH", _F + "/issues/{bulk_a}/",
                route="/api/workspaces/{slug}/projects/{project_id}/issues/{pk}/",
                json={"state_id": "{state_completed}"},
                auth="admin", expect_status=204,
            ),
            Request(
                "PATCH", _F + "/issues/{bulk_b}/",
                route="/api/workspaces/{slug}/projects/{project_id}/issues/{pk}/",
                json={"state_id": "{state_completed}"},
                auth="admin", expect_status=204,
            ),
            Request(
                "POST", _F + "/bulk-archive-issues/",
                route="/api/workspaces/{slug}/projects/{project_id}/bulk-archive-issues/",
                json={"issue_ids": ["{bulk_a}", "{bulk_b}"]},
                auth="admin", expect_status=200,
            ),
            Request(
                "POST", _F + "/bulk-archive-issues/",
                route="/api/workspaces/{slug}/projects/{project_id}/bulk-archive-issues/",
                json={"issue_ids": ["{rel_a}"]},
                auth="admin", expect_status=400,
            ),
            Request(
                "POST", _F + "/bulk-archive-issues/",
                route="/api/workspaces/{slug}/projects/{project_id}/bulk-archive-issues/",
                json={"issue_ids": []},
                auth="admin", expect_status=400,
            ),
            Request(
                "DELETE", _F + "/issues/{bulk_a}/archive/",
                route="/api/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/archive/",
                auth="admin", expect_status=204,
            ),
            Request(
                "DELETE", _F + "/issues/{bulk_b}/archive/",
                route="/api/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/archive/",
                auth="admin", expect_status=204,
            ),
            Request(
                "DELETE", _F + "/issues/{bulk_a}/",
                route="/api/workspaces/{slug}/projects/{project_id}/issues/{pk}/",
                auth="admin", expect_status=204,
            ),
            Request(
                "DELETE", _F + "/issues/{bulk_b}/",
                route="/api/workspaces/{slug}/projects/{project_id}/issues/{pk}/",
                auth="admin", expect_status=204,
            ),
            # Bulk delete: throwaways, round-trip, empty 400, member 403.
            Request(
                "POST", _ISSUES,
                route="/api/workspaces/{slug}/projects/{project_id}/issues/",
                json={"name": "Gone a"},
                auth="admin", capture={"gone_a": "id"}, expect_status=201,
            ),
            Request(
                "POST", _ISSUES,
                route="/api/workspaces/{slug}/projects/{project_id}/issues/",
                json={"name": "Gone b"},
                auth="admin", capture={"gone_b": "id"}, expect_status=201,
            ),
            Request(
                "DELETE", _F + "/bulk-delete-issues/",
                route="/api/workspaces/{slug}/projects/{project_id}/bulk-delete-issues/",
                json={"issue_ids": ["{gone_a}", "{gone_b}"]},
                auth="admin", expect_status=200,
            ),
            Request(
                "DELETE", _F + "/bulk-delete-issues/",
                route="/api/workspaces/{slug}/projects/{project_id}/bulk-delete-issues/",
                json={"issue_ids": []},
                auth="admin", expect_status=400,
            ),
            Request(
                "DELETE", _F + "/bulk-delete-issues/",
                route="/api/workspaces/{slug}/projects/{project_id}/bulk-delete-issues/",
                json={"issue_ids": ["{rel_a}"]},
                auth="member", expect_status=403,
            ),
            # Move: RPJ -> RPJ2 -> RPJ, requires-target 400,
            # non-member-target 404, guest 403.
            Request(
                "POST", _F + "/work-items/{rel_b}/move/",
                route="/api/workspaces/{slug}/projects/{project_id}/work-items/{issue_id}/move/",
                json={"project": "RPJ2"},
                auth="admin", expect_status=200,
            ),
            Request(
                "POST", _F2 + "/work-items/{rel_b}/move/",
                route="/api/workspaces/{slug}/projects/{project_id}/work-items/{issue_id}/move/",
                json={"project": "RPJ"},
                auth="admin", expect_status=200,
            ),
            Request(
                "POST", _F + "/work-items/{rel_b}/move/",
                route="/api/workspaces/{slug}/projects/{project_id}/work-items/{issue_id}/move/",
                json={},
                auth="admin", expect_status=400,
            ),
            Request(
                "POST", _F + "/work-items/{rel_b}/move/",
                route="/api/workspaces/{slug}/projects/{project_id}/work-items/{issue_id}/move/",
                json={"project": "RPX"},
                auth="admin", expect_status=404,
            ),
            Request(
                "POST", _F + "/work-items/{rel_b}/move/",
                route="/api/workspaces/{slug}/projects/{project_id}/work-items/{issue_id}/move/",
                json={"project": "RPJ2"},
                auth="guest", expect_status=403,
            ),
            # Cleanup: the relation anchors.
            Request(
                "DELETE", _F + "/issues/{rel_a}/",
                route="/api/workspaces/{slug}/projects/{project_id}/issues/{pk}/",
                auth="admin", expect_status=204,
            ),
            Request(
                "DELETE", _F + "/issues/{rel_b}/",
                route="/api/workspaces/{slug}/projects/{project_id}/issues/{pk}/",
                auth="admin", expect_status=204,
            ),
        ),
    ),
)
