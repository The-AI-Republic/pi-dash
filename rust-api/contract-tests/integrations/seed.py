"""Raw-SQL seeding for the integrations oracle (no Django imports).

Anchor rows (workspace / project / actor) are borrowed read-only from the
target DB; every row this module inserts is tracked in a Scope and removed
in reverse-FK order on teardown, so reruns are hermetic.
"""
from __future__ import annotations

import base64
import hashlib

from cryptography.fernet import Fernet
from psycopg.types.json import Jsonb

from _harness import db


# Fixed self-hosted GitLab name for the PIDASHCONV-1470 gate tests. The
# `.invalid` TLD (RFC 2606) never resolves, so a worker that passes the
# allowlist gate fails fast at DNS with no stub, proxy, or fixed port —
# and the must-use-HTTPS + allowlist gates stay fully exercised. Both
# workers under test must boot with this host in GITLAB_ALLOWED_HOSTS.
GITLAB_SELFHOST = "https://gitlab-allowed.invalid"


def encrypt_token_for_worker(plaintext: str, secret_key: str) -> str:
    """Fernet-encrypt a PAT exactly like Django's ``encrypt_data``.

    Mirrors ``apps/api/pi_dash/license/utils/encryption.py`` (``derive_key``
    = PBKDF2-HMAC-SHA256 over ``secret_key`` salted ``b"salt"`` with 100000
    rounds, urlsafe-b64; same KDF as ``Keyring::derive_fernet_key`` in
    ``pidash-db``). ``secret_key`` must equal the SECRET_KEY the worker
    under test boots with. Production always stores tokens encrypted
    (``integrations/git/services.py:90,104-108``), so gate tests seed the
    ciphertext form.
    """
    if not plaintext:
        return ""
    dk = hashlib.pbkdf2_hmac("sha256", secret_key.encode(), b"salt", 100000)
    return Fernet(base64.urlsafe_b64encode(dk)).encrypt(plaintext.encode()).decode()


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


def gitlab_binding(
    database_url: str,
    anchor: dict,
    scope: Scope,
    *,
    host_url: str = GITLAB_SELFHOST,
    token: str,
    suffix: str = "ct-gitlab",
) -> dict:
    """One enabled GitLab binding (PIDASHCONV-1470 gate tests).

    Pass an ``encrypt_token_for_worker()`` ciphertext as ``token`` to mirror
    production storage. ``host_url`` defaults to the fixed unresolvable
    self-host the workers under test allow-list at boot.
    """
    account_id = provider_account(
        database_url, anchor, scope, provider="gitlab", host_url=host_url, token=token
    )
    repo_id = repository(
        database_url, scope, provider="gitlab", host_url=host_url, suffix=suffix
    )
    binding_id = binding(
        database_url, anchor, scope, account_id=account_id, repository_id=repo_id
    )
    return {"account_id": account_id, "repository_id": repo_id, "binding_id": binding_id}


def unknown_provider_binding(
    database_url: str, anchor: dict, scope: Scope, *, token: str = "contract-test-invalid-token"
) -> dict:
    """Binding whose repository provider has no registered adapter.

    `get_adapter` runs before the guarded `try` in `sync_one_binding`
    (git_sync_task.py), so the KeyError escapes as a silent task failure:
    nothing is recorded on the binding and `self.retry` never runs. The
    Rust port must keep that ordering (translate, don't redesign).
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
    provider: str = "github",
) -> str:
    """Insert one GitIssueSync row for post_completion_comment probes."""
    project = anchor["projects"][0]
    row_id = db.fetchone(
        database_url,
        "INSERT INTO git_issue_syncs (id, created_at, updated_at, provider,"
        " external_id, external_iid, web_url, remote_state, metadata,"
        " binding_id, issue_id, project_id, workspace_id)"
        " VALUES (%s, now(), now(), %s, %s, %s, %s, 'open', %s, %s, %s, %s, %s)"
        " RETURNING id",
        (
            db.new_uuid(),
            provider,
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
