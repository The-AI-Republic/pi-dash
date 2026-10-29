"""Raw-SQL seeding for the integrations oracle (no Django imports).

Anchor rows (workspace / project / actor) are borrowed read-only from the
target DB; every row this module inserts is tracked in a Scope and removed
in reverse-FK order on teardown, so reruns are hermetic.
"""
from __future__ import annotations

import base64
import hashlib
import random

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


def github_app_account(
    database_url: str,
    anchor: dict,
    scope: Scope,
    *,
    installation_id: int = 424242,
    suffix: str = "app",
) -> str:
    """Provider account whose credential takes the installation branch.

    `auth_type` is `github_app` with an `installation_id`, so the adapter
    mints an installation token (`GithubClient.for_installation`) instead
    of using a PAT. The mint reads the app id/slug from
    `instance_configurations` on Django (PIDASHCONV-1471).
    """
    return _insert(
        database_url,
        scope,
        "git_provider_accounts",
        ["workspace_id", "provider", "host_url", "auth_type", "external_account_id",
         "external_account_login", "display_name", "capabilities", "credential_config",
         "status", "last_check_error", "metadata"],
        (
            str(anchor["workspace"]["id"]),
            "github",
            "https://github.com",
            "github_app",
            f"ct-app-{suffix}",
            "ct-app",
            "contract-test github app",
            Jsonb({}),
            Jsonb({"installation_id": installation_id}),
            "connected",
            "",
            Jsonb({}),
        ),
    )


def github_app_binding(
    database_url: str,
    anchor: dict,
    scope: Scope,
    *,
    installation_id: int = 424242,
    suffix: str = "app",
) -> dict:
    """One enabled binding on a github_app account (installation credential)."""
    account_id = github_app_account(
        database_url, anchor, scope, installation_id=installation_id, suffix=suffix
    )
    repo_id = repository(database_url, scope, suffix=f"ct-app-{suffix}")
    binding_id = binding(
        database_url, anchor, scope, account_id=account_id, repository_id=repo_id
    )
    return {"account_id": account_id, "repository_id": repo_id, "binding_id": binding_id}


def delete_binding_triple(database_url: str, seeded: dict) -> None:
    """Delete one seeded binding + account + repo (reverse-FK order).

    Only one active binding per project is allowed, so two-phase tests
    remove the first phase's triple before seeding the second. Scope
    teardown re-deletes the same ids harmlessly afterwards.
    """
    db.execute(
        database_url,
        "DELETE FROM git_repository_bindings WHERE id = %s",
        (seeded["binding_id"],),
    )
    db.execute(
        database_url,
        "DELETE FROM git_provider_accounts WHERE id = %s",
        (seeded["account_id"],),
    )
    db.execute(
        database_url,
        "DELETE FROM git_repositories WHERE id = %s",
        (seeded["repository_id"],),
    )


def save_instance_config(database_url: str, keys: list[str]) -> dict:
    """Snapshot `instance_configurations` rows so a test can restore them.

    Returns `{key: row-dict | None}` (`None` = no live row; soft-deleted
    rows count as absent, like the config readers see them).
    """
    rows = {
        row["key"]: row
        for row in db.fetchall(
            database_url,
            "SELECT key, value, category, is_encrypted FROM instance_configurations"
            " WHERE key = ANY(%s) AND deleted_at IS NULL",
            (keys,),
        )
    }
    return {key: rows.get(key) for key in keys}


def set_instance_config(database_url: str, values: dict[str, str]) -> None:
    """Upsert `instance_configurations` rows (test values, never encrypted)."""
    for key, value in values.items():
        db.execute(
            database_url,
            "INSERT INTO instance_configurations"
            " (id, key, value, category, is_encrypted, created_at, updated_at)"
            " VALUES (%s, %s, %s, 'GITHUB', false, now(), now())"
            " ON CONFLICT (key) DO UPDATE SET value = EXCLUDED.value,"
            " deleted_at = NULL, updated_at = now()",
            (db.new_uuid(), key, value),
        )


def clear_instance_config(database_url: str, keys: list[str]) -> None:
    db.execute(
        database_url,
        "DELETE FROM instance_configurations WHERE key = ANY(%s)",
        (keys,),
    )


def restore_instance_config(database_url: str, saved: dict) -> None:
    """Undo `set_instance_config`: restore originals, drop test-only rows."""
    for key, row in saved.items():
        if row is None:
            db.execute(
                database_url,
                "DELETE FROM instance_configurations WHERE key = %s",
                (key,),
            )
        else:
            db.execute(
                database_url,
                "UPDATE instance_configurations SET value = %s, category = %s,"
                " is_encrypted = %s, updated_at = now() WHERE key = %s",
                (row["value"], row["category"], row["is_encrypted"], key),
            )


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


# --- Legacy (github_sync_task) rows for the PIDASHCONV-1472 redirect tests.
#
# Column lists mirror _harness/seed.py's create_workspace_integration /
# create_github_repo_sync / create_github_issue_sync; the legacy token has
# no plaintext fallback (github_sync_task._resolve_token decrypts only),
# so callers pass encrypt_token_for_worker() ciphertext.


def ensure_github_integration(database_url: str) -> str:
    """Get-or-create the shared ``integrations`` row (provider=github).

    Never tracked: like _harness/seed.py's ensure_github_integration, the
    shared row is left behind (deleting it would steal another suite's
    row on a shared database).
    """
    row = db.fetchone(
        database_url, "SELECT id FROM integrations WHERE provider = 'github'"
    )
    if row is not None:
        return str(row["id"])
    return db.fetchone(
        database_url,
        "INSERT INTO integrations (id, title, provider, network,"
        " description, author, webhook_url, webhook_secret, redirect_url,"
        " metadata, verified, created_at, updated_at)"
        " VALUES (%s, 'GitHub', 'github', 2,"
        " '{\"summary\": \"Mirror GitHub issues into Pi Dash projects.\"}',"
        " '', '', '', '', '{}', true, now(), now()) RETURNING id",
        (db.new_uuid(),),
    )["id"]


def legacy_api_token(
    database_url: str, scope: Scope, *, user_id: str, workspace_id: str
) -> str:
    """Minimal ``api_tokens`` row for a workspace_integrations FK."""
    return _insert(
        database_url,
        scope,
        "api_tokens",
        ["token", "label", "description", "is_active", "user_type", "user_id",
         "workspace_id", "is_service", "allowed_rate_limit"],
        (
            f"pi_dash_api_{db.new_uuid().replace('-', '')}",
            "redirect-probe",
            "seeded for contract tests",
            True,
            0,
            user_id,
            workspace_id,
            False,
            "100000/min",
        ),
    )


def legacy_workspace_integration(
    database_url: str,
    scope: Scope,
    *,
    workspace_id: str,
    actor_id: str,
    token_cipher: str,
) -> str:
    """``workspace_integrations`` row carrying the legacy credential."""
    integration_id = ensure_github_integration(database_url)
    token_id = legacy_api_token(
        database_url, scope, user_id=actor_id, workspace_id=workspace_id
    )
    return _insert(
        database_url,
        scope,
        "workspace_integrations",
        ["metadata", "config", "actor_id", "api_token_id", "integration_id",
         "workspace_id"],
        (
            Jsonb({}),
            Jsonb(
                {
                    "auth_type": "pat",
                    "token": token_cipher,
                    "github_user_login": "redirect-octocat",
                }
            ),
            actor_id,
            token_id,
            integration_id,
            workspace_id,
        ),
    )


def legacy_repository_sync(
    database_url: str,
    scope: Scope,
    *,
    workspace_id: str,
    project_id: str,
    actor_id: str,
    wi_id: str,
    owner: str,
    name: str,
    enabled: bool = True,
) -> dict:
    """Bound ``github_repositories`` + ``github_repository_syncs`` pair."""
    repo_id = _insert(
        database_url,
        scope,
        "github_repositories",
        ["name", "url", "config", "repository_id", "owner", "project_id",
         "workspace_id"],
        (
            name,
            f"https://github.com/{owner}/{name}",
            Jsonb({}),
            random.randint(10_000_000, 99_999_999),
            owner,
            project_id,
            workspace_id,
        ),
    )
    sync_id = _insert(
        database_url,
        scope,
        "github_repository_syncs",
        ["credentials", "actor_id", "project_id", "repository_id",
         "workspace_id", "workspace_integration_id", "is_sync_enabled",
         "last_sync_error"],
        (Jsonb({}), actor_id, project_id, repo_id, workspace_id, wi_id, enabled, ""),
    )
    return {"sync_id": sync_id, "repo_id": repo_id}


def legacy_issue_sync(
    database_url: str,
    scope: Scope,
    *,
    sync_id: str,
    issue_id: str,
    project_id: str,
    workspace_id: str,
    repo_issue_id: int,
) -> str:
    """One ``github_issue_syncs`` row for legacy post_completion_comment."""
    row_id = db.fetchone(
        database_url,
        "INSERT INTO github_issue_syncs (id, created_at, updated_at,"
        " repository_sync_id, issue_id, project_id, workspace_id,"
        " repo_issue_id, github_issue_id, issue_url, metadata)"
        " VALUES (%s, now(), now(), %s, %s, %s, %s, %s, %s, %s, %s)"
        " RETURNING id",
        (
            db.new_uuid(),
            sync_id,
            issue_id,
            project_id,
            workspace_id,
            repo_issue_id,
            random.randint(10_000_000, 99_999_999),
            f"https://github.com/redirect-oldowner/redirect-oldrepo/issues/{repo_issue_id}",
            Jsonb({}),
        ),
    )["id"]
    return scope.track("github_issue_syncs", str(row_id))


def cleanup_workspace_mirrors(database_url: str, workspace_id: str) -> None:
    """Delete worker-mirrored rows of one (per-test unique) workspace.

    Successful syncs create Issue / sync / comment rows the per-test Scope
    never tracked; deleting them by workspace_id (reverse-FK order) runs
    before Scope teardown, which then removes the seeded rows (chain,
    bindings, sync triples) itself. Syncs into ticking defaults also arm
    the issue clock and may create entry runs (PIDASHCONV-1576), so the
    run/ticker rows go first — raw SQL has no ORM cascade to hide behind.
    """
    for sql, params in (
        (
            "DELETE FROM agent_run_event WHERE agent_run_id IN"
            " (SELECT id FROM agent_run WHERE workspace_id = %s)",
            (workspace_id,),
        ),
        ("DELETE FROM agent_run WHERE workspace_id = %s", (workspace_id,)),
        (
            "DELETE FROM issue_agent_ticker WHERE issue_id IN"
            " (SELECT id FROM issues WHERE workspace_id = %s)",
            (workspace_id,),
        ),
    ):
        try:
            db.execute(database_url, sql, params)
        except Exception:
            pass
    for table in (
        "git_comment_syncs",
        "github_comment_syncs",
        "issue_comments",
        "issue_sequences",
        "git_issue_syncs",
        "github_issue_syncs",
        "issues",
    ):
        try:
            db.execute(
                database_url,
                f"DELETE FROM {table} WHERE workspace_id = %s",
                (workspace_id,),
            )
        except Exception:
            pass
