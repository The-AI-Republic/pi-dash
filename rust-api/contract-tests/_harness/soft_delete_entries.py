# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Seeders for the soft-delete entry contract (PIDASHCONV-1293).

Shared by the Django oracle (``tasks_cleanup/test_cleanup_tasks.py``) and
the Rust replay (``tasks_cleanup/test_rust_replay.py``): one seeder per
model name the Rust API enqueues through
``soft_delete_related_objects``. Each seeder inserts a minimal live row
(plus its FK parents) and returns ``(pk, extra)`` where ``extra`` carries
row ids the cascade checks need (draft children, the ``agent_run``
pointing at a scheduler binding).

Every value mirrors the Django model defaults; ``insert_row`` fills
``id``/``created_at``/``updated_at``/``deleted_at`` and the
``seed_helpers`` chain builders auto-uniquify usernames and slugs, so
each case is re-run safe.
"""

from __future__ import annotations

import uuid
from datetime import datetime, timedelta, timezone

from . import seed as seed_helpers
from .db import insert_row


def _now(days: int = 0) -> str:
    return (datetime.now(timezone.utc) - timedelta(days=days)).isoformat()


def seed_draft_tree(conn, tag: str):
    """Draft issue + assignee/label/module/cycle children + a file asset."""
    chain = seed_helpers.issue_chain(conn, f"sde1293draft{tag}")
    member = seed_helpers.user(conn, f"sde1293-{tag}-member")
    label = insert_row(
        conn,
        "labels",
        {
            "workspace_id": str(chain["workspace"]["id"]),
            "project_id": str(chain["project"]["id"]),
            "name": "L1",
            "description": "",
            "color": "#ff0000",
            "sort_order": 65535,
        },
    )
    module = seed_helpers.module(conn, chain["workspace"]["id"], chain["project"]["id"])
    cycle = seed_helpers.cycle(
        conn, chain["workspace"]["id"], chain["project"]["id"], chain["owner"]["id"]
    )
    draft = insert_row(
        conn,
        "draft_issues",
        {
            "workspace_id": str(chain["workspace"]["id"]),
            "project_id": str(chain["project"]["id"]),
            "name": "entry draft",
            "description_json": {},
            "description_html": "<p></p>",
            "priority": "none",
            "sort_order": 65535,
        },
    )
    children = [
        (
            "draft_issue_assignees",
            insert_row(
                conn,
                "draft_issue_assignees",
                {
                    "workspace_id": str(chain["workspace"]["id"]),
                    "draft_issue_id": str(draft["id"]),
                    "assignee_id": str(member["id"]),
                },
            )["id"],
        ),
        (
            "draft_issue_labels",
            insert_row(
                conn,
                "draft_issue_labels",
                {
                    "workspace_id": str(chain["workspace"]["id"]),
                    "draft_issue_id": str(draft["id"]),
                    "label_id": str(label["id"]),
                },
            )["id"],
        ),
        (
            "draft_issue_modules",
            insert_row(
                conn,
                "draft_issue_modules",
                {
                    "workspace_id": str(chain["workspace"]["id"]),
                    "draft_issue_id": str(draft["id"]),
                    "module_id": str(module["id"]),
                },
            )["id"],
        ),
        (
            "draft_issue_cycles",
            insert_row(
                conn,
                "draft_issue_cycles",
                {
                    "workspace_id": str(chain["workspace"]["id"]),
                    "draft_issue_id": str(draft["id"]),
                    "cycle_id": str(cycle["id"]),
                },
            )["id"],
        ),
        (
            "file_assets",
            insert_row(
                conn,
                "file_assets",
                {
                    "asset": f"sde1293/{tag}.bin",
                    "is_uploaded": True,
                    "is_deleted": False,
                    "is_archived": False,
                    "size": 0,
                    "attributes": {},
                    "draft_issue_id": str(draft["id"]),
                },
            )["id"],
        ),
    ]
    return draft["id"], {"children": children}


def seed_scheduler(conn, tag: str):
    owner = seed_helpers.user(conn, f"sde1293-{tag}-owner")
    ws = seed_helpers.workspace(conn, f"sde1293sched{tag}", owner["id"])
    row = insert_row(
        conn,
        "schedulers",
        {
            "slug": f"sde1293-{tag}",
            "name": "entry scheduler",
            "description": "",
            "prompt": "Do the thing.",
            "source": "builtin",
            "is_enabled": True,
            "workspace_id": str(ws["id"]),
            "color": "",
        },
    )
    return row["id"], {}


def seed_scheduler_binding(conn, tag: str):
    """Scheduler binding with an `agent_run` pointing at it (SET_NULL)."""
    owner = seed_helpers.user(conn, f"sde1293-{tag}-owner")
    ws = seed_helpers.workspace(conn, f"sde1293bind{tag}", owner["id"])
    project = seed_helpers.project(conn, ws["id"])
    scheduler_id, _ = seed_scheduler(conn, f"{tag}b")
    binding = insert_row(
        conn,
        "scheduler_bindings",
        {
            "scheduler_id": str(scheduler_id),
            "dtstart": _now(days=1),
            "tzid": "UTC",
            "rrule": "FREQ=DAILY;BYHOUR=3;BYMINUTE=0",
            "rdates": [],
            "exdates": [],
            "outcome_mode": "issue",
            "extra_context": "",
            "enabled": True,
            "last_error": "",
        },
    )
    pod = insert_row(
        conn,
        "pod",
        {
            "workspace_id": str(ws["id"]),
            "project_id": str(project["id"]),
            "name": f"sde1293-pod-{tag}",
            "description": "",
            "is_default": False,
        },
    )
    run = insert_row(
        conn,
        "agent_run",
        {
            "status": "queued",
            "prompt": "entry probe",
            "run_config": {},
            "required_capabilities": [],
            "thread_id": uuid.uuid4().hex,
            "error": "",
            "workspace_id": str(ws["id"]),
            "pod_id": str(pod["id"]),
            "created_by_id": str(owner["id"]),
            "llm_model": "",
            "refusal_category": "",
            "failure_reason": "",
            "trigger": "manual",
            "executor_kind": "local_runner",
            "dispatch_attempts": 0,
            "cancel_reason": "",
            "error_code": "",
            "tool_plan": {},
            "phase_kind": "",
            "agent_metadata": {},
            "usage": {},
            "scheduler_binding_id": str(binding["id"]),
        },
    )
    return binding["id"], {"agent_run_id": run["id"]}


def seed_notification(conn, tag: str):
    owner = seed_helpers.user(conn, f"sde1293-{tag}-owner")
    ws = seed_helpers.workspace(conn, f"sde1293not{tag}", owner["id"])
    receiver = seed_helpers.user(conn, f"sde1293-{tag}-receiver")
    row = insert_row(
        conn,
        "notifications",
        {
            "entity_name": "issue",
            "title": "entry",
            "message_html": "<p>hi</p>",
            "sender": "system",
            "receiver_id": str(receiver["id"]),
            "workspace_id": str(ws["id"]),
        },
    )
    return row["id"], {}


def seed_workspace_user_link(conn, tag: str):
    owner = seed_helpers.user(conn, f"sde1293-{tag}-owner")
    ws = seed_helpers.workspace(conn, f"sde1293link{tag}", owner["id"])
    row = insert_row(
        conn,
        "workspace_user_links",
        {
            "url": "https://example.com/entry",
            "metadata": {},
            "owner_id": str(owner["id"]),
            "workspace_id": str(ws["id"]),
        },
    )
    return row["id"], {}


def seed_sticky(conn, tag: str):
    owner = seed_helpers.user(conn, f"sde1293-{tag}-owner")
    ws = seed_helpers.workspace(conn, f"sde1293sticky{tag}", owner["id"])
    row = insert_row(
        conn,
        "stickies",
        {
            "description": {},
            "description_html": "<p>entry</p>",
            "logo_props": {},
            "owner_id": str(owner["id"]),
            "workspace_id": str(ws["id"]),
            "sort_order": 65535,
        },
    )
    return row["id"], {}


def seed_github_repository_sync(conn, tag: str):
    owner = seed_helpers.user(conn, f"sde1293-{tag}-owner")
    ws = seed_helpers.workspace(conn, f"sde1293ghsync{tag}", owner["id"])
    project = seed_helpers.project(conn, ws["id"])
    integration = insert_row(
        conn,
        "integrations",
        {
            "title": "GitHub",
            "provider": f"github-{tag}",
            "network": 0,
            "description": {},
            "author": "",
            "webhook_url": "",
            "webhook_secret": "",
            "redirect_url": "",
            "metadata": {},
            "verified": False,
        },
    )
    token = insert_row(
        conn,
        "api_tokens",
        {
            "token": uuid.uuid4().hex,
            "label": "entry",
            "user_type": 0,
            "user_id": str(owner["id"]),
            "description": "",
            "is_active": True,
            "is_service": False,
            "allowed_rate_limit": "",
        },
    )
    wintegration = insert_row(
        conn,
        "workspace_integrations",
        {
            "metadata": {},
            "config": {},
            "actor_id": str(owner["id"]),
            "api_token_id": str(token["id"]),
            "integration_id": str(integration["id"]),
            "workspace_id": str(ws["id"]),
        },
    )
    repo = insert_row(
        conn,
        "github_repositories",
        {
            "name": "entry-repo",
            "config": {},
            "repository_id": 424242,
            "owner": "entry-owner",
            "project_id": str(project["id"]),
            "workspace_id": str(ws["id"]),
        },
    )
    row = insert_row(
        conn,
        "github_repository_syncs",
        {
            "credentials": {},
            "actor_id": str(owner["id"]),
            "project_id": str(project["id"]),
            "repository_id": str(repo["id"]),
            "workspace_id": str(ws["id"]),
            "workspace_integration_id": str(wintegration["id"]),
            "is_sync_enabled": True,
            "last_sync_error": "",
        },
    )
    return row["id"], {}


def seed_git_repository_binding(conn, tag: str):
    owner = seed_helpers.user(conn, f"sde1293-{tag}-owner")
    ws = seed_helpers.workspace(conn, f"sde1293gitbind{tag}", owner["id"])
    project = seed_helpers.project(conn, ws["id"])
    account = insert_row(
        conn,
        "git_provider_accounts",
        {
            "provider": "github",
            "host_url": "https://github.com",
            "auth_type": "token",
            "external_account_id": "4242",
            "external_account_login": "entry-owner",
            "display_name": "entry",
            "capabilities": {},
            "credential_config": {},
            "status": "active",
            "last_check_error": "",
            "metadata": {},
            "workspace_id": str(ws["id"]),
        },
    )
    repo = insert_row(
        conn,
        "git_repositories",
        {
            "provider": "github",
            "host_url": "https://github.com",
            "external_id": f"4242-{tag}",
            "namespace": "entry-owner",
            "name": "entry-repo",
            "full_name": f"entry-owner/entry-repo-{tag}",
            "web_url": "https://github.com/entry-owner/entry-repo",
            "clone_url_http": "https://github.com/entry-owner/entry-repo.git",
            "clone_url_ssh": "git@github.com:entry-owner/entry-repo.git",
            "default_branch": "main",
            "is_private": False,
            "metadata": {},
        },
    )
    row = insert_row(
        conn,
        "git_repository_bindings",
        {
            "is_sync_enabled": True,
            "clone_auth_mode": "token",
            "last_sync_error": "",
            "metadata": {},
            "actor_id": str(owner["id"]),
            "project_id": str(project["id"]),
            "provider_account_id": str(account["id"]),
            "repository_id": str(repo["id"]),
            "workspace_id": str(ws["id"]),
        },
    )
    return row["id"], {}


def seed_deploy_board(conn, tag: str):
    owner = seed_helpers.user(conn, f"sde1293-{tag}-owner")
    ws = seed_helpers.workspace(conn, f"sde1293deploy{tag}", owner["id"])
    row = insert_row(
        conn,
        "deploy_boards",
        {
            "anchor": f"entry-{tag}",
            "is_comments_enabled": True,
            "is_reactions_enabled": True,
            "is_votes_enabled": True,
            "view_props": {},
            "workspace_id": str(ws["id"]),
            "is_activity_enabled": True,
            "is_disabled": False,
        },
    )
    return row["id"], {}


def seed_comment_reaction(conn, tag: str):
    chain = seed_helpers.issue_chain(conn, f"sde1293react{tag}")
    actor = seed_helpers.user(conn, f"sde1293-{tag}-actor")
    # Inline (not `seed_helpers.issue_comment`): `attachments`/`labels`
    # are `varchar[]` columns, sent as `"{}"` strings like the replay
    # `_seed_comment`.
    comment = insert_row(
        conn,
        "issue_comments",
        {
            "workspace_id": str(chain["workspace"]["id"]),
            "project_id": str(chain["project"]["id"]),
            "issue_id": str(chain["issue"]["id"]),
            "comment_stripped": "hi",
            "comment_json": {},
            "comment_html": "<p>hi</p>",
            "attachments": "{}",
            "labels": "{}",
            "access": "INTERNAL",
            "speaker_type": "human",
            "speaker_label": "",
        },
    )
    row = insert_row(
        conn,
        "comment_reactions",
        {
            "reaction": "heart",
            "actor_id": str(actor["id"]),
            "comment_id": str(comment["id"]),
            "project_id": str(chain["project"]["id"]),
            "workspace_id": str(chain["workspace"]["id"]),
        },
    )
    return row["id"], {}


def seed_issue_vote(conn, tag: str):
    chain = seed_helpers.issue_chain(conn, f"sde1293vote{tag}")
    actor = seed_helpers.user(conn, f"sde1293-{tag}-actor")
    row = insert_row(
        conn,
        "issue_votes",
        {
            "vote": 1,
            "actor_id": str(actor["id"]),
            "issue_id": str(chain["issue"]["id"]),
            "project_id": str(chain["project"]["id"]),
            "workspace_id": str(chain["workspace"]["id"]),
        },
    )
    return row["id"], {}


def seed_module_link(conn, tag: str):
    chain = seed_helpers.issue_chain(conn, f"sde1293mlink{tag}")
    module = seed_helpers.module(conn, chain["workspace"]["id"], chain["project"]["id"])
    row = insert_row(
        conn,
        "module_links",
        {
            "url": "https://example.com/entry",
            "module_id": str(module["id"]),
            "project_id": str(chain["project"]["id"]),
            "workspace_id": str(chain["workspace"]["id"]),
            "metadata": {},
        },
    )
    return row["id"], {}


def seed_git_code_review_link(conn, tag: str):
    chain = seed_helpers.issue_chain(conn, f"sde1293review{tag}")
    row = insert_row(
        conn,
        "git_code_review_links",
        {
            "provider": "github",
            "host_url": "https://github.com",
            "namespace": "entry-owner",
            "repo_name": "entry-repo",
            "repo_external_id": "4242",
            "external_id": "4242",
            "external_iid": f"7-{tag}",
            "url": "https://github.com/entry-owner/entry-repo/pull/7",
            "title": "entry",
            "state": "open",
            "merged": False,
            "draft": False,
            "metadata": {},
            "issue_id": str(chain["issue"]["id"]),
            "project_id": str(chain["project"]["id"]),
            "workspace_id": str(chain["workspace"]["id"]),
        },
    )
    return row["id"], {}


def seed_github_pull_request_link(conn, tag: str):
    chain = seed_helpers.issue_chain(conn, f"sde1293prlink{tag}")
    row = insert_row(
        conn,
        "github_pull_request_links",
        {
            "repo_owner": "entry-owner",
            "repo_name": "entry-repo",
            "pr_number": int(tag, 16) % 2_000_000_000,
            "url": "https://github.com/entry-owner/entry-repo/pull/7",
            "title": "entry",
            "state": "open",
            "merged": False,
            "draft": False,
            "issue_id": str(chain["issue"]["id"]),
            "project_id": str(chain["project"]["id"]),
            "workspace_id": str(chain["workspace"]["id"]),
        },
    )
    return row["id"], {}


# (model_name, table, seeder): every model name the Rust API enqueues
# through `soft_delete_related_objects` that the entry map was missing
# (PIDASHCONV-1293). The draft case doubles as the cascade test (its
# `children` extra pins the CASCADE stamps); the schedulerbinding case
# pins the `agent_run.scheduler_binding_id` SET_NULL.
ENTRY_CASES = [
    ("draftissue", "draft_issues", seed_draft_tree),
    ("schedulerbinding", "scheduler_bindings", seed_scheduler_binding),
    ("scheduler", "schedulers", seed_scheduler),
    ("notification", "notifications", seed_notification),
    ("workspaceuserlink", "workspace_user_links", seed_workspace_user_link),
    ("sticky", "stickies", seed_sticky),
    ("githubrepositorysync", "github_repository_syncs", seed_github_repository_sync),
    ("gitrepositorybinding", "git_repository_bindings", seed_git_repository_binding),
    ("deployboard", "deploy_boards", seed_deploy_board),
    ("commentreaction", "comment_reactions", seed_comment_reaction),
    ("issuevote", "issue_votes", seed_issue_vote),
    ("modulelink", "module_links", seed_module_link),
    ("gitcodereviewlink", "git_code_review_links", seed_git_code_review_link),
    ("githubpullrequestlink", "github_pull_request_links", seed_github_pull_request_link),
]
