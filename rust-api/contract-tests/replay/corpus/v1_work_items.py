# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Corpus: api-v1 work-item write flows (PIDASHCONV-823).

Built from the ``v1_work_items`` contract suite (PIDASHCONV-76):
test_work_items.py (CRUD), test_subresources.py (links, comments,
relations, workpad, PR links), test_labels_pages.py (labels, pages),
test_actions.py (move, re-tick, wait, run-ai).
"""

from psycopg.rows import dict_row

from ..session import Request, Session

# Fixed move-target project id (seeded by _seed_move_target, addressed
# as {target_project} in request templates).
TARGET_PROJECT_ID = "aaaaaaaa-0000-4000-8000-000000000001"

_WS = "/api/v1/workspaces/{ws}/projects/{project}"
_WI = _WS + "/work-items/{wi}/"
_WI_NEW = _WS + "/work-items/{newwi}/"


def _seed_move_target(conn) -> None:
    """Seed a second project in the world workspace (fixed id).

    Column list mirrors ``_harness.db.create_project`` (same values);
    only the id is fixed so requests can address it.
    """
    from _harness import db as db_helpers

    with conn.cursor(row_factory=dict_row) as cur:
        cur.execute("SELECT id FROM workspaces WHERE slug = 'replay-ws'")
        ws_id = cur.fetchone()["id"]
        cur.execute(
            "SELECT id FROM users WHERE email = 'replay-admin@example.com'"
        )
        admin_id = cur.fetchone()["id"]
    with conn.cursor() as cur:
        cur.execute(
            """
            INSERT INTO projects (id, name, description, network, identifier,
                workspace_id, created_by_id, cycle_view, module_view,
                issue_views_view, page_view, intake_view, archive_in, close_in,
                logo_props, is_time_tracking_enabled, is_issue_type_enabled,
                guest_view_all_features, timezone, members_can_edit_states,
                repo_url, base_branch, agent_default_interval_seconds,
                agent_default_max_ticks, agent_ticking_enabled, is_default,
                agent_review_default_interval_seconds, default_agent_executor,
                agent_test_default_interval_seconds, created_at, updated_at)
            VALUES (%s, 'Replay Move Target', '', 0, 'RPJMOVE',
                %s, %s, true, true,
                true, true, false, 0, 0,
                '{}', true, true,
                false, 'UTC', false,
                '', '', 0,
                0, false, false,
                0, '', 0, now(), now())
            """,
            (TARGET_PROJECT_ID, str(ws_id), str(admin_id)),
        )
        cur.execute(
            """
            INSERT INTO project_identifiers (name, project_id, workspace_id,
                created_by_id, created_at, updated_at)
            VALUES ('RPJMOVE', %s, %s, %s, now(), now())
            """,
            (TARGET_PROJECT_ID, str(ws_id), str(admin_id)),
        )
    conn.commit()
    db_helpers.add_project_member(conn, str(ws_id), TARGET_PROJECT_ID, str(admin_id), db_helpers.ADMIN)
    db_helpers.create_state(
        conn, str(ws_id), TARGET_PROJECT_ID, "replay-move",
        name="Move Backlog", group="backlog", default=True,
        created_by_id=str(admin_id),
    )


PR_PAYLOAD = {
    "repo_owner": "acme",
    "repo_name": "web",
    "pr_number": 424242,
    "url": "https://github.com/acme/web/pull/424242",
    "title": "Fix it",
    "state": "open",
    "merged": False,
    "draft": False,
}

SESSIONS = (
    Session(
        name="v1_work_items_lifecycle",
        domain="v1_work_items",
        source="v1_work_items/test_work_items.py, test_subresources.py, "
        "test_labels_pages.py, test_actions.py (PIDASHCONV-76)",
        auth_users=("api_key",),
        seed=_seed_move_target,
        static_context={"target_project": TARGET_PROJECT_ID},
        payload_time_paths={
            "pi_dash.bgtasks.issue_activities_task.issue_activity": ["kwargs/epoch"],
        },
        # Known divergence: Rust never enqueues the API-token audit log
        # (TokenLogLayer sinks to tracing only). Fails if Rust starts
        # emitting (remove the pin) or Django stops (investigate).
        known_absent_jobs={
            "pi_dash.bgtasks.logger_task.process_logs": "PIDASHCONV-868",
        },
        # soft_delete_related_objects("db", "<model>", <row id>): the row
        # id is backend-local (never in a response); each side's value
        # must exist in its own table.
        payload_row_refs={
            "pi_dash.bgtasks.deletion_task.soft_delete_related_objects": [
                ("args/2", "issue_relations"),
                ("args/2", "git_code_review_links"),
                ("args/2", "issues"),
                ("args/2", "issue_comments"),
                ("args/2", "issue_links"),
                ("args/2", "labels"),
            ],
        },
        # PR-link create also writes the legacy link row and stashes its
        # id in the new row's metadata (github_pr_links.py:_ensure...).
        db_cell_refs=[
            ("git_code_review_links", "metadata", "github_pull_request_links",
             "legacy_github_pull_request_link_id"),
        ],
        requests=(
            Request(
                "POST", _WS + "/work-items/",
                route="/api/v1/workspaces/{slug}/projects/{project_id}/work-items/",
                json={"name": "Replay v1", "description_html": "<p>body</p>", "priority": "high"},
                auth="api_key", capture={"wi": "id"}, expect_status=201,
            ),
            Request(
                "PATCH", _WI,
                route="/api/v1/workspaces/{slug}/projects/{project_id}/work-items/{pk}/",
                json={"name": "Replay v1b", "priority": "low"},
                auth="api_key", expect_status=200,
            ),
            Request(
                "POST", _WS + "/work-items/",
                route="/api/v1/workspaces/{slug}/projects/{project_id}/work-items/",
                json={"name": "Sibling"},
                auth="api_key", capture={"sib": "id"}, expect_status=201,
            ),
            Request(
                "POST", _WI + "relations/relate/",
                route="/api/v1/workspaces/{slug}/projects/{project_id}/work-items/{pk}/relations/relate/",
                json={"relation_type": "relates_to", "issues": ["{sib}"]},
                auth="api_key", expect_status=200,
            ),
            Request(
                "POST", _WI + "relations/unrelate/",
                route="/api/v1/workspaces/{slug}/projects/{project_id}/work-items/{pk}/relations/unrelate/",
                json={"relation_type": "relates_to", "issues": ["{sib}"]},
                auth="api_key", expect_status=200,
            ),
            Request(
                "POST", _WI + "comments/",
                route="/api/v1/workspaces/{slug}/projects/{project_id}/work-items/{pk}/comments/",
                json={"comment_html": "<p>hi</p>", "access": "EXTERNAL"},
                auth="api_key", capture={"wcomment": "id", "wcomment_desc": "description"},
                expect_status=201,
            ),
            Request(
                "PATCH", _WI + "comments/{wcomment}/",
                route="/api/v1/workspaces/{slug}/projects/{project_id}/work-items/{pk}/comments/{comment_id}/",
                json={"comment_html": "<p>v2</p>"},
                auth="api_key", expect_status=200,
            ),
            Request(
                "POST", _WI + "links/",
                route="/api/v1/workspaces/{slug}/projects/{project_id}/work-items/{pk}/links/",
                json={"title": "Spec", "url": "https://example.com/spec"},
                auth="api_key", capture={"link": "id"}, expect_status=201,
            ),
            Request(
                "PATCH", _WI + "links/{link}/",
                route="/api/v1/workspaces/{slug}/projects/{project_id}/work-items/{pk}/links/{link_id}/",
                json={"title": "New"},
                auth="api_key", expect_status=200,
            ),
            Request(
                "PATCH", _WI + "workpad/",
                route="/api/v1/workspaces/{slug}/projects/{project_id}/work-items/{pk}/workpad/",
                json={"body": "# notes"},
                auth="api_key", expect_status=200,
            ),
            Request(
                "POST", _WI + "github/pull-requests/",
                route="/api/v1/workspaces/{slug}/projects/{project_id}/work-items/{pk}/github/pull-requests/",
                json=dict(PR_PAYLOAD),
                auth="api_key", capture={"pr": "id"}, expect_status=201,
            ),
            Request(
                "POST", _WI + "github/pull-requests/",
                route="/api/v1/workspaces/{slug}/projects/{project_id}/work-items/{pk}/github/pull-requests/",
                json=dict(PR_PAYLOAD),
                auth="api_key", expect_status=200,
            ),
            Request(
                "POST", _WI + "move/",
                route="/api/v1/workspaces/{slug}/projects/{project_id}/work-items/{pk}/move/",
                json={},
                auth="api_key", expect_status=400,
            ),
            Request(
                "POST", _WI + "wait/",
                route="/api/v1/workspaces/{slug}/projects/{project_id}/work-items/{pk}/wait/",
                json={},
                auth="api_key", expect_status=200,
            ),
            Request(
                "POST", _WI + "re-tick/",
                route="/api/v1/workspaces/{slug}/projects/{project_id}/work-items/{pk}/re-tick/",
                json={},
                auth="api_key", capture={"tick_run": "run_id"}, expect_status=200,
            ),
            Request(
                "POST", _WI + "run-ai/",
                route="/api/v1/workspaces/{slug}/projects/{project_id}/work-items/{pk}/run-ai/",
                json={"prompt": "do it"},
                auth="api_key", expect_status=409,
            ),
            Request(
                "POST", _WS + "/labels/",
                route="/api/v1/workspaces/{slug}/projects/{project_id}/labels/",
                json={"name": "Bug", "color": "#ff0000", "description": "defects"},
                auth="api_key", capture={"label": "id"}, expect_status=201,
            ),
            Request(
                "PATCH", _WS + "/labels/{label}/",
                route="/api/v1/workspaces/{slug}/projects/{project_id}/labels/{pk}/",
                json={"name": "New"},
                auth="api_key", expect_status=200,
            ),
            Request(
                "POST", _WS + "/pages/",
                route="/api/v1/workspaces/{slug}/projects/{project_id}/pages/",
                json={"name": "Nope", "description_markdown": "# hi"},
                auth="api_key", expect_status=503,
            ),
            Request(
                "DELETE", _WI + "github/pull-requests/{pr}/",
                route="/api/v1/workspaces/{slug}/projects/{project_id}/work-items/{pk}/github/pull-requests/{link_id}/",
                auth="api_key", expect_status=204,
            ),
            Request(
                "DELETE", _WI + "links/{link}/",
                route="/api/v1/workspaces/{slug}/projects/{project_id}/work-items/{pk}/links/{link_id}/",
                auth="api_key", expect_status=204,
            ),
            Request(
                "DELETE", _WI + "comments/{wcomment}/",
                route="/api/v1/workspaces/{slug}/projects/{project_id}/work-items/{pk}/comments/{comment_id}/",
                auth="api_key", expect_status=204,
            ),
            Request(
                "POST", _WI + "move/",
                route="/api/v1/workspaces/{slug}/projects/{project_id}/work-items/{pk}/move/",
                json={"project": "{target_project}"},
                auth="api_key", expect_status=200,
            ),
            Request(
                "DELETE", _WS + "/labels/{label}/",
                route="/api/v1/workspaces/{slug}/projects/{project_id}/labels/{pk}/",
                auth="api_key", expect_status=204,
            ),
            Request(
                "DELETE", _WS + "/work-items/{sib}/",
                route="/api/v1/workspaces/{slug}/projects/{project_id}/work-items/{pk}/",
                auth="api_key", expect_status=204,
            ),
            Request(
                "DELETE",
                "/api/v1/workspaces/{ws}/projects/{target_project}/work-items/{wi}/",
                route="/api/v1/workspaces/{slug}/projects/{project_id}/work-items/{pk}/",
                auth="api_key", expect_status=204,
            ),
        ),
    ),
)
