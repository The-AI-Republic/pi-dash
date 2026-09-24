"""Raw-SQL seeding for the integrations oracle (no Django imports).

Anchor rows (workspace / project / actor) are borrowed read-only from the
target DB; every row this module inserts is tracked in a Scope and removed
in reverse-FK order on teardown, so reruns are hermetic.
"""
from __future__ import annotations

from psycopg.types.json import Jsonb

from _harness import db


class Scope:
    def __init__(self, database_url: str) -> None:
        self.database_url = database_url
        self._rows: list[tuple[str, str]] = []  # (table, id)

    def track(self, table: str, row_id: str) -> str:
        self._rows.append((table, row_id))
        return row_id

    def cleanup(self) -> None:
        for table, row_id in reversed(self._rows):
            try:
                db.execute(self.database_url, f"DELETE FROM {table} WHERE id = %s", (row_id,))
            except Exception:
                pass
        self._rows.clear()


def find_anchor(database_url: str) -> dict:
    ws = db.fetchone(
        database_url,
        "SELECT id, slug FROM workspaces WHERE deleted_at IS NULL ORDER BY created_at LIMIT 1",
    )
    assert ws, "target DB has no workspace to anchor on"
    projects = db.fetchall(
        database_url,
        "SELECT id, workspace_id FROM projects WHERE workspace_id = %s"
        " AND deleted_at IS NULL ORDER BY created_at LIMIT 2",
        (ws["id"],),
    )
    assert len(projects) >= 1, "need a project in the anchor workspace"
    actor = db.fetchone(
        database_url,
        "SELECT id FROM users WHERE is_active IS TRUE ORDER BY created_at LIMIT 1",
    )
    assert actor, "target DB has no user to act as binding actor"
    state = db.fetchone(
        database_url,
        'SELECT id FROM states WHERE project_id = %s AND deleted_at IS NULL'
        ' ORDER BY ("default" IS TRUE) DESC, created_at LIMIT 1',
        (projects[0]["id"],),
    )
    return {"workspace": ws, "projects": projects, "actor": actor, "state": state}


def _insert(database_url: str, scope: Scope, table: str, columns: list[str], values: tuple) -> str:
    row_id = db.fetchone(
        database_url,
        f"INSERT INTO {table} (id, created_at, updated_at, {', '.join(columns)}) "
        f"VALUES (%s, now(), now(), {', '.join(['%s'] * len(columns))}) RETURNING id",
        (db.new_uuid(), *values),
    )["id"]
    return scope.track(table, str(row_id))


def provider_account(
    database_url: str,
    anchor: dict,
    scope: Scope,
    *,
    provider: str = "github",
    host_url: str = "https://github.com",
    token: str = "contract-test-invalid-token",
) -> str:
    return _insert(
        database_url,
        scope,
        "git_provider_accounts",
        ["workspace_id", "provider", "host_url", "auth_type", "external_account_id",
         "external_account_login", "display_name", "capabilities", "credential_config",
         "status", "last_check_error", "metadata"],
        (
            str(anchor["workspace"]["id"]),
            provider,
            host_url,
            "pat",
            "ct-1",
            "ct-bot",
            "contract-test account",
            Jsonb({}),
            Jsonb({"auth_type": "pat", "host_url": host_url, "token": token}),
            "connected",
            "",
            Jsonb({}),
        ),
    )


def repository(
    database_url: str,
    scope: Scope,
    *,
    provider: str = "github",
    host_url: str = "https://github.com",
    suffix: str = "ct",
) -> str:
    full_name = f"contract-test/{suffix}"
    return _insert(
        database_url,
        scope,
        "git_repositories",
        ["provider", "host_url", "external_id", "namespace", "name", "full_name",
         "web_url", "clone_url_http", "clone_url_ssh", "default_branch", "is_private",
         "metadata"],
        (
            provider,
            host_url,
            f"ct-{suffix}",
            "contract-test",
            suffix,
            full_name,
            f"{host_url}/{full_name}",
            "",
            "",
            "main",
            False,
            Jsonb({}),
        ),
    )


def binding(
    database_url: str,
    anchor: dict,
    scope: Scope,
    *,
    project_index: int = 0,
    account_id: str,
    repository_id: str,
    enabled: bool = True,
) -> str:
    project = anchor["projects"][project_index]
    return _insert(
        database_url,
        scope,
        "git_repository_bindings",
        ["repository_id", "provider_account_id", "actor_id", "workspace_id", "project_id",
         "is_sync_enabled", "clone_auth_mode", "last_sync_error", "metadata"],
        (
            repository_id,
            account_id,
            str(anchor["actor"]["id"]),
            str(project["workspace_id"]),
            str(project["id"]),
            enabled,
            "runner_managed",
            "",
            Jsonb({}),
        ),
    )


def github_binding(database_url: str, anchor: dict, scope: Scope, *, token: str) -> dict:
    account_id = provider_account(database_url, anchor, scope, token=token)
    repo_id = repository(database_url, scope)
    binding_id = binding(
        database_url, anchor, scope, account_id=account_id, repository_id=repo_id
    )
    return {"account_id": account_id, "repository_id": repo_id, "binding_id": binding_id}


def unknown_provider_binding(
    database_url: str, anchor: dict, scope: Scope, *, token: str = "contract-test-invalid-token"
) -> dict:
    """Binding whose repository provider has no registered adapter.

    `get_adapter` raises KeyError before any provider HTTP, so
    `sync_one_binding` lands on the generic-except branch and calls
    `self.retry` — the black-box transient path (error recorded + retry
    re-queued with countdown 60 * 2^retries, max_retries=3).
    """
    account_id = provider_account(database_url, anchor, scope, token=token)
    unique = db.new_uuid()[:8]
    repo_id = repository(
        database_url, scope, provider="contract-test-unknown", suffix=f"ct-unknown-{unique}"
    )
    binding_id = binding(
        database_url, anchor, scope, account_id=account_id, repository_id=repo_id
    )
    return {"account_id": account_id, "repository_id": repo_id, "binding_id": binding_id}


def find_borrowed_issue(database_url: str) -> dict:
    """Borrow one existing issue read-only (completion-comment sync rows hang off it)."""
    row = db.fetchone(
        database_url,
        "SELECT id, workspace_id, project_id FROM issues ORDER BY created_at DESC LIMIT 1",
    )
    assert row, "target DB has no issue to anchor completion rows on"
    return row


def git_issue_sync(
    database_url: str,
    anchor: dict,
    scope: Scope,
    *,
    binding_id: str,
    issue_id: str,
    external_iid: str = "ct-4242",
    metadata: dict | None = None,
) -> str:
    """Insert one GitIssueSync row for post_completion_comment probes."""
    project = anchor["projects"][0]
    row_id = db.fetchone(
        database_url,
        "INSERT INTO git_issue_syncs (id, created_at, updated_at, provider,"
        " external_id, external_iid, web_url, remote_state, metadata,"
        " binding_id, issue_id, project_id, workspace_id)"
        " VALUES (%s, now(), now(), 'github', %s, %s, %s, 'open', %s, %s, %s, %s, %s)"
        " RETURNING id",
        (
            db.new_uuid(),
            f"ct-ext-{external_iid}",
            external_iid,
            f"https://github.com/contract-test/ct/issues/{external_iid}",
            Jsonb(metadata or {}),
            binding_id,
            issue_id,
            str(project["id"]),
            str(project["workspace_id"]),
        ),
    )["id"]
    return scope.track("git_issue_syncs", str(row_id))
