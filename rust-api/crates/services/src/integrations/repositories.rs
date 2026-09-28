//! Git repositories and bindings (D-05, stage 5).
//!
//! Ports the repository half of `apps/api/pi_dash/integrations/git/services.py`:
//! `upsert_repository` (203-227), `canonical_clone_url` (228-231),
//! `bind_repository` (297-352), `get_binding` (353-360),
//! `set_binding_sync_enabled` (361-373), `unbind_repository` (374-380),
//! `list_account_repositories` (381-388). The four serializers live in
//! [`super::serializers`]; the account helpers and the [`GitStore`][super::accounts::GitStore]
//! seam live in [`super::accounts`].
//!
//! Every SQL statement lives here as a `*_SQL` const (or builder, where
//! the `SET` clause is dynamic) so tests assert the exact store calls.
//! `apply_bind` executes the whole binding section inside one
//! transaction, matching `with transaction.atomic()`
//! (`services.py:324`); `upsert_repository` runs outside it, exactly as
//! in Python (`services.py:316` precedes the `with` block).
//!
//! Ported bugs / inherited semantics (translate, don't redesign):
//!
//! * `upsert_repository` strips only trailing slashes from `host_url`
//!   (`host_url.rstrip("/")`, `services.py:206`) — no whitespace trim,
//!   no scheme prepend, unlike the account lookup path.
//! * `update_or_create` is find-then-write with no race retry: a
//!   concurrent insert surfaces as a store error, like Django's
//!   `IntegrityError`.
//! * `bind_repository` hard-deletes the existing project binding
//!   filtered by project only (no workspace predicate,
//!   `services.py:325`) and hard-deletes every `GithubRepositorySync`
//!   row for the project even for non-GitHub remotes (`services.py:328`).
//! * `bind_repository` overwrites `project.repo_url` whenever it differs
//!   and fills `project.base_branch` only when the remote has a branch
//!   and the project has none (`services.py:341-349`); an empty update
//!   list skips the project write entirely.
//! * `set_binding_sync_enabled` mirrors `is_sync_enabled` onto
//!   `GithubRepositorySync` rows only when the bound repository's
//!   provider is `"github"` (`services.py:367`); other providers leave
//!   the sync table untouched.
//! * `unbind_repository` deletes the binding only when one exists but
//!   always deletes the project's `GithubRepositorySync` rows
//!   (`services.py:374-378`).
//! * `list_account_repositories` passes `page` straight through to the
//!   adapter (default `1` is the caller's convention,
//!   `services.py:381`).

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use pidash_db::integrations::git_models::{
    git_provider_account::GitProviderAccount, git_repository::GitRepository,
    git_repository_binding, git_repository_binding::GitRepositoryBinding,
};
use pidash_db::RequestContext;
use pidash_types::integrations::registry::resolve_adapter_key;
use pidash_types::integrations::{
    GitProviderError, ParsedRepository, RemoteRepository, UnknownProvider,
};

use super::accounts::{
    account_credential, ctx_actor_id, resolve_provider_account_repository, GitIntegrationError,
    GitStore, ProviderAdapter, ServiceError, StoreError,
};
use super::serializers::{serialize_remote_repository, SerializedRemoteRepository};

/// `upsert_repository` lookup by external id
/// (`services.py:204-209`, `external_id` truthy branch).
///
/// Parameters: `$1` provider, `$2` host URL (trailing-slash stripped),
/// `$3` external id.
pub const REPOSITORY_FIND_BY_EXTERNAL_SQL: &str = "SELECT id, created_at, updated_at, created_by_id, updated_by_id, deleted_at, provider, host_url, external_id, namespace, name, full_name, web_url, clone_url_http, clone_url_ssh, default_branch, is_private, metadata FROM git_repositories WHERE provider = $1 AND host_url = $2 AND external_id = $3 AND deleted_at IS NULL ORDER BY id ASC LIMIT 1";

/// `upsert_repository` lookup by full name (`services.py:210-211`,
/// falsy-`external_id` branch).
///
/// Parameters: `$1` provider, `$2` host URL, `$3` full name.
pub const REPOSITORY_FIND_BY_FULL_NAME_SQL: &str = "SELECT id, created_at, updated_at, created_by_id, updated_by_id, deleted_at, provider, host_url, external_id, namespace, name, full_name, web_url, clone_url_http, clone_url_ssh, default_branch, is_private, metadata FROM git_repositories WHERE provider = $1 AND host_url = $2 AND full_name = $3 AND deleted_at IS NULL ORDER BY id ASC LIMIT 1";

/// `update_or_create` update branch (`services.py:212-224`).
///
/// Parameters: `$1` id, `$2` external_id, `$3` namespace, `$4` name,
/// `$5` full_name, `$6` web_url, `$7` clone_url_http, `$8`
/// clone_url_ssh, `$9` default_branch, `$10` is_private, `$11` metadata
/// (jsonb), `$12` updated_at.
pub const REPOSITORY_UPDATE_SQL: &str = "UPDATE git_repositories SET external_id = $2, namespace = $3, name = $4, full_name = $5, web_url = $6, clone_url_http = $7, clone_url_ssh = $8, default_branch = $9, is_private = $10, metadata = $11, updated_at = $12 WHERE id = $1";

/// `update_or_create` create branch.
///
/// Parameters: `$1` id, `$2` created_at, `$3` updated_at, `$4`
/// provider, `$5` host URL, `$6` external_id, `$7` namespace, `$8`
/// name, `$9` full_name, `$10` web_url, `$11` clone_url_http, `$12`
/// clone_url_ssh, `$13` default_branch, `$14` is_private, `$15`
/// metadata (jsonb). Audit FKs stay NULL (no request actor on this
/// path, matching Django's `update_or_create` without user stamps).
pub const REPOSITORY_INSERT_SQL: &str = "INSERT INTO git_repositories (id, created_at, updated_at, provider, host_url, external_id, namespace, name, full_name, web_url, clone_url_http, clone_url_ssh, default_branch, is_private, metadata) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15)";

/// `get_binding` with `select_related("repository", "provider_account")`
/// (`services.py:353-358`).
///
/// Parameters: `$1` project id, `$2` workspace slug. The pool
/// implementation maps the `b_`/`r_`/`a_` columns onto the binding,
/// repository, and account rows of [`BindingView`].
pub const BINDING_GET_SQL: &str = "SELECT b.id AS b_id, b.created_at AS b_created_at, b.updated_at AS b_updated_at, b.created_by_id AS b_created_by_id, b.updated_by_id AS b_updated_by_id, b.deleted_at AS b_deleted_at, b.project_id AS b_project_id, b.workspace_id AS b_workspace_id, b.repository_id AS b_repository_id, b.provider_account_id AS b_provider_account_id, b.actor_id AS b_actor_id, b.is_sync_enabled AS b_is_sync_enabled, b.clone_auth_mode AS b_clone_auth_mode, b.last_synced_at AS b_last_synced_at, b.last_sync_error AS b_last_sync_error, b.metadata AS b_metadata, r.id AS r_id, r.created_at AS r_created_at, r.updated_at AS r_updated_at, r.created_by_id AS r_created_by_id, r.updated_by_id AS r_updated_by_id, r.deleted_at AS r_deleted_at, r.provider AS r_provider, r.host_url AS r_host_url, r.external_id AS r_external_id, r.namespace AS r_namespace, r.name AS r_name, r.full_name AS r_full_name, r.web_url AS r_web_url, r.clone_url_http AS r_clone_url_http, r.clone_url_ssh AS r_clone_url_ssh, r.default_branch AS r_default_branch, r.is_private AS r_is_private, r.metadata AS r_metadata, a.id AS a_id, a.created_at AS a_created_at, a.updated_at AS a_updated_at, a.created_by_id AS a_created_by_id, a.updated_by_id AS a_updated_by_id, a.deleted_at AS a_deleted_at, a.workspace_id AS a_workspace_id, a.provider AS a_provider, a.host_url AS a_host_url, a.auth_type AS a_auth_type, a.external_account_id AS a_external_account_id, a.external_account_login AS a_external_account_login, a.display_name AS a_display_name, a.capabilities AS a_capabilities, a.credential_config AS a_credential_config, a.workspace_integration_id AS a_workspace_integration_id, a.status AS a_status, a.verified_at AS a_verified_at, a.last_check_error AS a_last_check_error, a.metadata AS a_metadata FROM git_repository_bindings b JOIN git_repositories r ON r.id = b.repository_id JOIN git_provider_accounts a ON a.id = b.provider_account_id JOIN workspaces w ON w.id = b.workspace_id WHERE b.project_id = $1 AND w.slug = $2 AND b.deleted_at IS NULL ORDER BY b.id ASC LIMIT 1";

/// Delete the existing project binding inside `bind_repository`
/// (`services.py:325-327`): hard delete, filtered by project only.
pub const BINDING_DELETE_SQL: &str = "DELETE FROM git_repository_bindings WHERE project_id = $1";

/// Hard-delete one binding by id (`unbind_repository`,
/// `services.py:377`).
pub const BINDING_DELETE_ONE_SQL: &str = "DELETE FROM git_repository_bindings WHERE id = $1";

/// Insert the new binding (`services.py:329-340`).
///
/// Parameters: `$1` id, `$2` created_at, `$3` updated_at, `$4`
/// created_by id (nullable), `$5` updated_by id (nullable), `$6`
/// project id, `$7` workspace id, `$8` repository id, `$9` provider
/// account id, `$10` actor id, `$11` is_sync_enabled (always false on
/// bind), `$12` clone_auth_mode, `$13` metadata (jsonb, `{"raw_url":
/// …}`). `last_synced_at` stays NULL; `last_sync_error` keeps `""`.
pub const BINDING_INSERT_SQL: &str = "INSERT INTO git_repository_bindings (id, created_at, updated_at, created_by_id, updated_by_id, project_id, workspace_id, repository_id, provider_account_id, actor_id, is_sync_enabled, clone_auth_mode, metadata) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13)";

/// `set_binding_sync_enabled` binding update (`services.py:365-366`).
///
/// Parameters: `$1` binding id, `$2` enabled, `$3` updated_at.
pub const BINDING_SET_SYNC_SQL: &str =
    "UPDATE git_repository_bindings SET is_sync_enabled = $2, updated_at = $3 WHERE id = $1";

/// Hard-delete a project's `GithubRepositorySync` rows
/// (`services.py:328,378`).
pub const GITHUB_SYNC_DELETE_SQL: &str =
    "DELETE FROM github_repository_syncs WHERE project_id = $1";

/// `set_binding_sync_enabled` Github mirror (`services.py:368-370`).
///
/// Parameters: `$1` enabled, `$2` project id, `$3` workspace slug.
pub const GITHUB_SYNC_SET_ENABLED_SQL: &str = "UPDATE github_repository_syncs SET is_sync_enabled = $1 WHERE project_id = $2 AND workspace_id = (SELECT id FROM workspaces WHERE slug = $3)";

/// `get_object_or_404(Workspace, slug=…)` (`services.py:309`).
pub const WORKSPACE_ID_SQL: &str =
    "SELECT id FROM workspaces WHERE slug = $1 AND deleted_at IS NULL ORDER BY id ASC LIMIT 1";

/// `get_object_or_404(Project, pk=…, workspace=…)` (`services.py:310`).
pub const PROJECT_GET_SQL: &str = "SELECT id, workspace_id, repo_url, base_branch FROM projects WHERE id = $1 AND workspace_id = $2 AND deleted_at IS NULL ORDER BY id ASC LIMIT 1";

/// `update_or_create` lookup shape (`services.py:204-211`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RepositoryLookup {
    ByExternalId {
        provider: String,
        host_url: String,
        external_id: String,
    },
    ByFullName {
        provider: String,
        host_url: String,
        full_name: String,
    },
}

/// `update_or_create` defaults (`services.py:212-223`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepositoryDefaults {
    pub external_id: String,
    pub namespace: String,
    pub name: String,
    pub full_name: String,
    pub web_url: String,
    pub clone_url_http: String,
    pub clone_url_ssh: String,
    pub default_branch: String,
    pub is_private: bool,
    pub metadata: serde_json::Value,
}

/// Insert row for the `update_or_create` create branch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewRepository {
    pub id: Uuid,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub provider: String,
    pub host_url: String,
    pub defaults: RepositoryDefaults,
}

/// Project fields `bind_repository` reads and conditionally writes
/// (`services.py:342-349`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectRef {
    pub id: Uuid,
    pub workspace_id: Uuid,
    pub repo_url: String,
    pub base_branch: String,
}

/// One `apply_bind` transaction (`services.py:324-349`).
///
/// `repo_url_update` / `base_branch_update` are `Some` exactly when the
/// field joins `update_fields`; `None` skips the project write, as in
/// Python (`services.py:348-349`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BindPlan {
    pub project_id: Uuid,
    pub workspace_id: Uuid,
    pub repository_id: Uuid,
    pub provider_account_id: Uuid,
    pub actor_id: Option<Uuid>,
    pub clone_auth_mode: String,
    pub raw_url: String,
    pub repo_url_update: Option<String>,
    pub base_branch_update: Option<String>,
    pub now: DateTime<Utc>,
}

/// `get_binding` row triple (`select_related`, `services.py:356`).
///
/// `PartialEq` only: the `pidash-db` row structs it joins do not
/// implement `Eq`.
#[derive(Debug, Clone, PartialEq)]
pub struct BindingView {
    pub binding: GitRepositoryBinding,
    pub repository: GitRepository,
    pub account: GitProviderAccount,
}

/// `list_account_repositories` envelope (`services.py:384-388`).
///
/// Key order is the Python dict order (`repos`, `page`,
/// `has_next_page`); `page` passes straight through from the adapter.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepositoryList {
    pub repos: Vec<SerializedRemoteRepository>,
    pub page: i64,
    pub has_next_page: bool,
}

/// `upsert_repository` lookup half (`services.py:204-211`).
///
/// `host_url` keeps only the trailing-slash strip (`rstrip("/")`) —
/// deliberately not `normalize_host_url`.
pub fn plan_upsert_lookup(remote: &RemoteRepository, host_url: &str) -> RepositoryLookup {
    let host = host_url.trim_end_matches('/').to_owned();
    if remote.external_id.is_empty() {
        RepositoryLookup::ByFullName {
            provider: remote.provider.clone(),
            host_url: host,
            full_name: remote.full_name.clone(),
        }
    } else {
        RepositoryLookup::ByExternalId {
            provider: remote.provider.clone(),
            host_url: host,
            external_id: remote.external_id.clone(),
        }
    }
}

/// `upsert_repository` defaults half (`services.py:212-223`).
pub fn plan_upsert_defaults(remote: &RemoteRepository) -> RepositoryDefaults {
    RepositoryDefaults {
        external_id: remote.external_id.clone(),
        namespace: remote.namespace.clone(),
        name: remote.name.clone(),
        full_name: remote.full_name.clone(),
        web_url: remote.web_url.clone(),
        clone_url_http: remote.clone_url_http.clone(),
        clone_url_ssh: remote.clone_url_ssh.clone(),
        default_branch: remote.default_branch.clone(),
        is_private: remote.is_private,
        metadata: remote.metadata.clone(),
    }
}

/// `upsert_repository` (`services.py:203-225`): find by external id or
/// full name, then update defaults or insert.
pub async fn upsert_repository<S: GitStore>(
    store: &S,
    remote: &RemoteRepository,
    host_url: &str,
    now: DateTime<Utc>,
) -> Result<GitRepository, ServiceError> {
    let lookup = plan_upsert_lookup(remote, host_url);
    let found = match &lookup {
        RepositoryLookup::ByExternalId {
            provider,
            host_url,
            external_id,
        } => {
            store
                .find_repository_by_external(provider, host_url, external_id)
                .await?
        }
        RepositoryLookup::ByFullName {
            provider,
            host_url,
            full_name,
        } => {
            store
                .find_repository_by_full_name(provider, host_url, full_name)
                .await?
        }
    };
    let defaults = plan_upsert_defaults(remote);
    if let Some(repo) = found {
        return Ok(store.update_repository(repo.id, defaults, now).await?);
    }
    let (provider, host_url) = match &lookup {
        RepositoryLookup::ByExternalId {
            provider, host_url, ..
        }
        | RepositoryLookup::ByFullName {
            provider, host_url, ..
        } => (provider.clone(), host_url.clone()),
    };
    Ok(store
        .insert_repository(NewRepository {
            id: Uuid::new_v4(),
            created_at: now,
            updated_at: now,
            provider,
            host_url,
            defaults,
        })
        .await?)
}

/// `canonical_clone_url` (`services.py:228-229`).
///
/// First present of `clone_url_http`, `raw_url`, `web_url` (empty
/// strings count as missing, like Python `or`).
pub fn canonical_clone_url(remote: &RemoteRepository, raw_url: &str) -> String {
    super::accounts::first_present(&[&remote.clone_url_http, raw_url, &remote.web_url]).to_owned()
}

/// `parse_repository_url` first-match loop (`registry.py:29-42`).
///
/// Tries each adapter in slice order (callers pass github-then-gitlab
/// per `ADAPTER_PARSE_ORDER`) and returns the first parse hit.
pub fn parse_repository_url(
    adapters: &[&dyn ProviderAdapter],
    raw_url: &str,
) -> Option<ParsedRepository> {
    adapters
        .iter()
        .find_map(|adapter| adapter.parse_repo_url(raw_url))
}

/// `get_adapter` provider lookup (`registry.py:18-22`).
///
/// Case-insensitive over the adapter table (`(provider or
/// "").lower()`); unknown providers fail with
/// `UnknownProvider("Unsupported Git provider: {provider}")`, carrying
/// the original input. A known key missing from the table reports the
/// same error (the table, not the registry, is incomplete).
pub fn lookup_adapter<'a>(
    adapters: &[&'a dyn ProviderAdapter],
    provider: &str,
) -> Result<&'a dyn ProviderAdapter, UnknownProvider> {
    let lowered = provider.to_lowercase();
    if let Some(adapter) = adapters.iter().find(|adapter| adapter.key() == lowered) {
        return Ok(*adapter);
    }
    Err(match resolve_adapter_key(provider) {
        Err(unknown) => unknown,
        // Known key but absent slice: report against a guaranteed
        // unknown input ("" is never a provider key) so the error still
        // names the provider path.
        Ok(_) => match resolve_adapter_key("") {
            Err(unknown) => unknown,
            Ok(_) => unreachable!("empty string is never a provider key"),
        },
    })
}

/// `bind_repository` caller input (`services.py:297-304`).
pub struct BindRequest<'a> {
    pub workspace_slug: &'a str,
    pub project_id: Uuid,
    pub raw_url: &'a str,
    pub provider_account_id: Option<Uuid>,
}

/// `bind_repository` (`services.py:297-350`).
///
/// Parse → workspace → project → resolve → upsert (outside the
/// transaction) → single-transaction [`GitStore::apply_bind`]. Returns
/// the binding and the clone URL written onto the project.
pub async fn bind_repository<S: GitStore>(
    store: &S,
    ctx: &RequestContext,
    adapters: &[&dyn ProviderAdapter],
    request: BindRequest<'_>,
    now: DateTime<Utc>,
) -> Result<(GitRepositoryBinding, String), ServiceError> {
    let parsed = parse_repository_url(adapters, request.raw_url).ok_or_else(|| {
        GitIntegrationError::UnsupportedRepositoryUrl(
            "A supported GitHub or GitLab repository URL is required".to_owned(),
        )
    })?;
    let workspace_id = store
        .find_workspace_id(request.workspace_slug)
        .await?
        .ok_or(StoreError::NotFound("workspace"))?;
    let project = store
        .find_project(workspace_id, request.project_id)
        .await?
        .ok_or(StoreError::NotFound("project"))?;
    let adapter =
        lookup_adapter(adapters, &parsed.provider).map_err(ServiceError::UnknownProvider)?;
    let (account, remote) = resolve_provider_account_repository(
        store,
        adapter,
        workspace_id,
        &parsed,
        request.provider_account_id.as_ref(),
    )
    .await?;
    let repo = upsert_repository(store, &remote, &parsed.host_url, now).await?;
    let clone_url = canonical_clone_url(&remote, &parsed.clone_url);
    let clone_auth_mode = if remote.is_private {
        git_repository_binding::CLONE_AUTH_MODE_RUNNER_MANAGED
    } else {
        git_repository_binding::CLONE_AUTH_MODE_PUBLIC
    }
    .to_owned();
    let repo_url_update = if project.repo_url != clone_url {
        Some(clone_url.clone())
    } else {
        None
    };
    let base_branch_update = if !remote.default_branch.is_empty() && project.base_branch.is_empty()
    {
        Some(remote.default_branch.clone())
    } else {
        None
    };
    let binding = store
        .apply_bind(BindPlan {
            project_id: request.project_id,
            workspace_id,
            repository_id: repo.id,
            provider_account_id: account.id,
            actor_id: ctx_actor_id(ctx),
            clone_auth_mode,
            raw_url: request.raw_url.to_owned(),
            repo_url_update,
            base_branch_update,
            now,
        })
        .await?;
    Ok((binding, clone_url))
}

/// Project `repo_url`/`base_branch` write (`services.py:341-349`).
///
/// Returns the `UPDATE` statement, or `None` when `update_fields` is
/// empty (Django skips the save). Parameters: `$1` project id, then
/// one placeholder per `Some` field in order (`repo_url`,
/// `base_branch`), then `updated_at` last.
pub fn project_repo_update_sql(
    repo_url: Option<&str>,
    base_branch: Option<&str>,
) -> Option<String> {
    let mut sets = Vec::new();
    let mut placeholder = 2u8;
    if repo_url.is_some() {
        sets.push(format!("repo_url = ${placeholder}"));
        placeholder += 1;
    }
    if base_branch.is_some() {
        sets.push(format!("base_branch = ${placeholder}"));
        placeholder += 1;
    }
    if sets.is_empty() {
        return None;
    }
    sets.push(format!("updated_at = ${placeholder}"));
    Some(format!(
        "UPDATE projects SET {} WHERE id = $1",
        sets.join(", ")
    ))
}

/// `get_binding` (`services.py:353-358`).
pub async fn get_binding<S: GitStore>(
    store: &S,
    workspace_slug: &str,
    project_id: Uuid,
) -> Result<Option<BindingView>, ServiceError> {
    Ok(store.get_binding(project_id, workspace_slug).await?)
}

/// `set_binding_sync_enabled` (`services.py:361-371`).
///
/// Missing bindings raise `ProviderAccountNotFound("Repository is not
/// bound")`; the Github sync-table mirror runs only for `"github"`
/// repositories.
pub async fn set_binding_sync_enabled<S: GitStore>(
    store: &S,
    workspace_slug: &str,
    project_id: Uuid,
    enabled: bool,
    now: DateTime<Utc>,
) -> Result<GitRepositoryBinding, ServiceError> {
    let view = store
        .get_binding(project_id, workspace_slug)
        .await?
        .ok_or_else(|| {
            GitIntegrationError::ProviderAccountNotFound("Repository is not bound".to_owned())
        })?;
    let binding = store
        .set_binding_sync(view.binding.id, enabled, now)
        .await?;
    if view.repository.provider == "github" {
        store
            .set_github_syncs_enabled(project_id, workspace_slug, enabled)
            .await?;
    }
    Ok(binding)
}

/// `unbind_repository` (`services.py:374-378`).
pub async fn unbind_repository<S: GitStore>(
    store: &S,
    workspace_slug: &str,
    project_id: Uuid,
) -> Result<(), ServiceError> {
    if let Some(view) = store.get_binding(project_id, workspace_slug).await? {
        store.delete_binding(view.binding.id).await?;
    }
    store.delete_github_syncs_for_project(project_id).await?;
    Ok(())
}

/// `list_account_repositories` (`services.py:381-388`).
///
/// `page` passes straight through; every remote serializes against the
/// account's stored `host_url`.
pub fn list_account_repositories(
    adapter: &dyn ProviderAdapter,
    account: &GitProviderAccount,
    page: i64,
) -> Result<RepositoryList, GitProviderError> {
    let credential = account_credential(
        &account.credential_config,
        &account.auth_type,
        &account.host_url,
    );
    let repo_page = adapter.list_repositories(&credential, page)?;
    Ok(RepositoryList {
        repos: repo_page
            .repositories
            .iter()
            .map(|remote| serialize_remote_repository(remote, &account.host_url))
            .collect(),
        page: repo_page.page,
        has_next_page: repo_page.has_next_page,
    })
}

#[cfg(test)]
mod tests {
    use super::super::accounts::fakes::*;
    use super::super::accounts::{GitIntegrationError, GitStore, ServiceError, StoreError};
    use super::*;
    use chrono::TimeZone;
    use pidash_db::integrations::git_models::{
        git_provider_account::{self, GitProviderAccount},
        git_repository::GitRepository,
    };
    use pidash_types::integrations::{
        GitProviderCapabilities, ParsedRepository, RemoteRepository, RepositoryPage,
    };
    use serde_json::Value;
    use std::collections::HashMap;
    use std::sync::Mutex;

    /// Minimal URL parser stub: parses `https://{host}/{ns}/{name}[.git]`
    /// for one provider key.
    struct StaticParser {
        key: &'static str,
        host: &'static str,
    }

    impl ProviderAdapter for StaticParser {
        fn key(&self) -> &'static str {
            self.key
        }

        fn display_name(&self) -> &'static str {
            "Static"
        }

        fn parse_repo_url(&self, url: &str) -> Option<ParsedRepository> {
            let rest = url.strip_prefix(&format!("https://{}/", self.host))?;
            let mut parts = rest.split('/');
            let namespace = parts.next()?.to_owned();
            let name = parts.next()?.trim_end_matches(".git").to_owned();
            if namespace.is_empty() || name.is_empty() {
                return None;
            }
            Some(ParsedRepository {
                provider: self.key.to_owned(),
                host_url: format!("https://{}", self.host),
                namespace: namespace.clone(),
                name: name.clone(),
                full_name: format!("{namespace}/{name}"),
                clone_url: url.trim().to_owned(),
            })
        }

        fn verify_provider_account(&self, credential: &Value) -> Result<Value, GitProviderError> {
            Ok(credential.clone())
        }

        fn credential_capabilities(
            &self,
            _credential: &Value,
        ) -> Result<GitProviderCapabilities, GitProviderError> {
            Ok(GitProviderCapabilities {
                read_repositories: true,
                read_issues: true,
                write_comments: true,
                manage_webhooks: false,
                clone: false,
            })
        }

        fn list_repositories(
            &self,
            _credential: &Value,
            page: i64,
        ) -> Result<RepositoryPage, GitProviderError> {
            Ok(RepositoryPage {
                repositories: Vec::new(),
                page,
                has_next_page: false,
            })
        }

        fn get_repository(
            &self,
            _credential: &Value,
            parsed: &ParsedRepository,
        ) -> Result<RemoteRepository, GitProviderError> {
            Err(GitProviderError::NotFound(parsed.full_name.clone()))
        }
    }

    /// Full in-memory [`GitStore`] for the repository closure.
    struct FullFake {
        accounts: Mutex<Vec<GitProviderAccount>>,
        repos: Mutex<Vec<GitRepository>>,
        bindings: Mutex<Vec<GitRepositoryBinding>>,
        workspaces: HashMap<String, Uuid>,
        projects: Mutex<HashMap<Uuid, ProjectRef>>,
        github_syncs: Mutex<HashMap<Uuid, bool>>,
    }

    impl FullFake {
        fn workspace_slug() -> &'static str {
            "acme"
        }

        fn workspace_id() -> Uuid {
            Uuid::parse_str("aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa").unwrap()
        }

        fn project_id() -> Uuid {
            Uuid::parse_str("bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb").unwrap()
        }

        fn with_project() -> Self {
            let workspace_id = Self::workspace_id();
            let project_id = Self::project_id();
            Self {
                accounts: Mutex::new(Vec::new()),
                repos: Mutex::new(Vec::new()),
                bindings: Mutex::new(Vec::new()),
                workspaces: HashMap::from([(Self::workspace_slug().to_owned(), workspace_id)]),
                projects: Mutex::new(HashMap::from([(
                    project_id,
                    ProjectRef {
                        id: project_id,
                        workspace_id,
                        repo_url: String::new(),
                        base_branch: String::new(),
                    },
                )])),
                github_syncs: Mutex::new(HashMap::new()),
            }
        }

        fn scoped_accounts(
            &self,
            workspace_id: Uuid,
            provider: &str,
            host_url: &str,
        ) -> Vec<GitProviderAccount> {
            let mut scoped: Vec<GitProviderAccount> = self
                .accounts
                .lock()
                .unwrap()
                .iter()
                .filter(|account| {
                    account.workspace_id == workspace_id
                        && account.provider == provider
                        && account.host_url == host_url
                        && (account.status == git_provider_account::STATUS_CONNECTED
                            || account.status == git_provider_account::STATUS_DEGRADED)
                        && account.deleted_at.is_none()
                })
                .cloned()
                .collect();
            scoped.sort_by_key(|left| left.created_at);
            scoped
        }

        fn view_for(&self, project_id: Uuid, workspace_slug: &str) -> Option<BindingView> {
            let workspace_id = self.workspaces.get(workspace_slug)?;
            let binding = self
                .bindings
                .lock()
                .unwrap()
                .iter()
                .find(|binding| {
                    binding.project_id == project_id
                        && &binding.workspace_id == workspace_id
                        && binding.deleted_at.is_none()
                })
                .cloned()?;
            let repository = self
                .repos
                .lock()
                .unwrap()
                .iter()
                .find(|repo| repo.id == binding.repository_id)
                .cloned()?;
            let account = self
                .accounts
                .lock()
                .unwrap()
                .iter()
                .find(|account| account.id == binding.provider_account_id)
                .cloned()?;
            Some(BindingView {
                binding,
                repository,
                account,
            })
        }
    }

    impl GitStore for FullFake {
        async fn list_provider_accounts(
            &self,
            workspace_id: Uuid,
            provider: &str,
            host_url: &str,
        ) -> Result<Vec<GitProviderAccount>, StoreError> {
            Ok(self.scoped_accounts(workspace_id, provider, host_url))
        }

        async fn get_provider_account(
            &self,
            workspace_id: Uuid,
            provider: &str,
            host_url: &str,
            account_id: Uuid,
        ) -> Result<Option<GitProviderAccount>, StoreError> {
            Ok(self
                .scoped_accounts(workspace_id, provider, host_url)
                .into_iter()
                .find(|account| account.id == account_id))
        }

        async fn insert_provider_account(
            &self,
            row: super::super::accounts::NewProviderAccount,
        ) -> Result<GitProviderAccount, StoreError> {
            let account = GitProviderAccount {
                id: row.id,
                created_at: row.created_at,
                updated_at: row.updated_at,
                created_by_id: row.created_by_id,
                updated_by_id: row.updated_by_id,
                deleted_at: None,
                workspace_id: row.workspace_id,
                provider: row.provider,
                host_url: row.host_url,
                auth_type: row.auth_type,
                external_account_id: row.external_account_id,
                external_account_login: row.external_account_login,
                display_name: row.display_name,
                capabilities: row.capabilities,
                credential_config: row.credential_config,
                workspace_integration_id: None,
                status: row.status,
                verified_at: Some(row.verified_at),
                last_check_error: String::new(),
                metadata: row.metadata,
            };
            self.accounts.lock().unwrap().push(account.clone());
            Ok(account)
        }

        async fn find_repository_by_external(
            &self,
            provider: &str,
            host_url: &str,
            external_id: &str,
        ) -> Result<Option<GitRepository>, StoreError> {
            Ok(self
                .repos
                .lock()
                .unwrap()
                .iter()
                .find(|repo| {
                    repo.provider == provider
                        && repo.host_url == host_url
                        && repo.external_id == external_id
                        && repo.deleted_at.is_none()
                })
                .cloned())
        }

        async fn find_repository_by_full_name(
            &self,
            provider: &str,
            host_url: &str,
            full_name: &str,
        ) -> Result<Option<GitRepository>, StoreError> {
            Ok(self
                .repos
                .lock()
                .unwrap()
                .iter()
                .find(|repo| {
                    repo.provider == provider
                        && repo.host_url == host_url
                        && repo.full_name == full_name
                        && repo.deleted_at.is_none()
                })
                .cloned())
        }

        async fn update_repository(
            &self,
            id: Uuid,
            defaults: RepositoryDefaults,
            now: DateTime<Utc>,
        ) -> Result<GitRepository, StoreError> {
            let mut repos = self.repos.lock().unwrap();
            let repo = repos
                .iter_mut()
                .find(|repo| repo.id == id)
                .ok_or_else(|| StoreError::Db("missing repository".into()))?;
            repo.external_id = defaults.external_id;
            repo.namespace = defaults.namespace;
            repo.name = defaults.name;
            repo.full_name = defaults.full_name;
            repo.web_url = defaults.web_url;
            repo.clone_url_http = defaults.clone_url_http;
            repo.clone_url_ssh = defaults.clone_url_ssh;
            repo.default_branch = defaults.default_branch;
            repo.is_private = defaults.is_private;
            repo.metadata = defaults.metadata;
            repo.updated_at = now;
            Ok(repo.clone())
        }

        async fn insert_repository(&self, row: NewRepository) -> Result<GitRepository, StoreError> {
            let repo = GitRepository {
                id: row.id,
                created_at: row.created_at,
                updated_at: row.updated_at,
                created_by_id: None,
                updated_by_id: None,
                deleted_at: None,
                provider: row.provider,
                host_url: row.host_url,
                external_id: row.defaults.external_id,
                namespace: row.defaults.namespace,
                name: row.defaults.name,
                full_name: row.defaults.full_name,
                web_url: row.defaults.web_url,
                clone_url_http: row.defaults.clone_url_http,
                clone_url_ssh: row.defaults.clone_url_ssh,
                default_branch: row.defaults.default_branch,
                is_private: row.defaults.is_private,
                metadata: row.defaults.metadata,
            };
            self.repos.lock().unwrap().push(repo.clone());
            Ok(repo)
        }

        async fn find_workspace_id(&self, slug: &str) -> Result<Option<Uuid>, StoreError> {
            Ok(self.workspaces.get(slug).copied())
        }

        async fn find_project(
            &self,
            workspace_id: Uuid,
            project_id: Uuid,
        ) -> Result<Option<ProjectRef>, StoreError> {
            Ok(self
                .projects
                .lock()
                .unwrap()
                .get(&project_id)
                .filter(|project| project.workspace_id == workspace_id)
                .cloned())
        }

        async fn apply_bind(&self, plan: BindPlan) -> Result<GitRepositoryBinding, StoreError> {
            // One atomic section: the fake holds a single logical lock
            // scope here, mirroring transaction.atomic().
            self.bindings
                .lock()
                .unwrap()
                .retain(|binding| binding.project_id != plan.project_id);
            self.github_syncs.lock().unwrap().remove(&plan.project_id);
            if let Some(repo_url) = plan.repo_url_update.clone() {
                if let Some(project) = self.projects.lock().unwrap().get_mut(&plan.project_id) {
                    project.repo_url = repo_url;
                }
            }
            if let Some(base_branch) = plan.base_branch_update.clone() {
                if let Some(project) = self.projects.lock().unwrap().get_mut(&plan.project_id) {
                    project.base_branch = base_branch;
                }
            }
            let binding = GitRepositoryBinding {
                id: Uuid::new_v4(),
                created_at: plan.now,
                updated_at: plan.now,
                created_by_id: plan.actor_id,
                updated_by_id: plan.actor_id,
                deleted_at: None,
                project_id: plan.project_id,
                workspace_id: plan.workspace_id,
                repository_id: plan.repository_id,
                provider_account_id: plan.provider_account_id,
                actor_id: plan.actor_id.unwrap_or_else(Uuid::nil),
                is_sync_enabled: false,
                clone_auth_mode: plan.clone_auth_mode,
                last_synced_at: None,
                last_sync_error: String::new(),
                metadata: serde_json::json!({ "raw_url": plan.raw_url }),
            };
            self.bindings.lock().unwrap().push(binding.clone());
            Ok(binding)
        }

        async fn get_binding(
            &self,
            project_id: Uuid,
            workspace_slug: &str,
        ) -> Result<Option<BindingView>, StoreError> {
            Ok(self.view_for(project_id, workspace_slug))
        }

        async fn set_binding_sync(
            &self,
            binding_id: Uuid,
            enabled: bool,
            now: DateTime<Utc>,
        ) -> Result<GitRepositoryBinding, StoreError> {
            let mut bindings = self.bindings.lock().unwrap();
            let binding = bindings
                .iter_mut()
                .find(|binding| binding.id == binding_id)
                .ok_or_else(|| StoreError::Db("missing binding".into()))?;
            binding.is_sync_enabled = enabled;
            binding.updated_at = now;
            Ok(binding.clone())
        }

        async fn set_github_syncs_enabled(
            &self,
            project_id: Uuid,
            _workspace_slug: &str,
            enabled: bool,
        ) -> Result<u64, StoreError> {
            let mut syncs = self.github_syncs.lock().unwrap();
            if let Some(entry) = syncs.get_mut(&project_id) {
                *entry = enabled;
                return Ok(1);
            }
            Ok(0)
        }

        async fn delete_binding(&self, binding_id: Uuid) -> Result<(), StoreError> {
            self.bindings
                .lock()
                .unwrap()
                .retain(|binding| binding.id != binding_id);
            Ok(())
        }

        async fn delete_github_syncs_for_project(
            &self,
            project_id: Uuid,
        ) -> Result<u64, StoreError> {
            Ok(self
                .github_syncs
                .lock()
                .unwrap()
                .remove(&project_id)
                .map(|_| 1)
                .unwrap_or(0))
        }
    }

    fn remote() -> RemoteRepository {
        RemoteRepository {
            provider: "github".into(),
            external_id: "123".into(),
            namespace: "acme".into(),
            name: "web".into(),
            full_name: "acme/web".into(),
            web_url: "https://github.com/acme/web".into(),
            clone_url_http: "https://github.com/acme/web.git".into(),
            clone_url_ssh: "git@github.com:acme/web.git".into(),
            default_branch: "main".into(),
            is_private: true,
            metadata: serde_json::json!({}),
        }
    }

    fn ctx() -> RequestContext {
        use pidash_types::{UserId, WorkspaceId};
        RequestContext::new(
            WorkspaceId::from("ws"),
            Some(UserId::from("cccccccc-cccc-cccc-cccc-cccccccccccc")),
        )
    }

    fn stamp() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2024, 5, 1, 12, 0, 0).unwrap()
    }

    /// Adapter that both parses URLs (like the real provider adapters)
    /// and resolves through the probe visibility set (like
    /// `_RepositoryProbeAdapter` in `test_git_services.py`).
    struct ParseAndProbe {
        parser: StaticParser,
        probe: ProbeAdapter,
    }

    impl ParseAndProbe {
        fn github(visible: &[&str]) -> Self {
            Self {
                parser: StaticParser {
                    key: "github",
                    host: "github.com",
                },
                probe: ProbeAdapter::new(visible),
            }
        }
    }

    impl ProviderAdapter for ParseAndProbe {
        fn key(&self) -> &'static str {
            "github"
        }

        fn display_name(&self) -> &'static str {
            "GitHub"
        }

        fn parse_repo_url(&self, url: &str) -> Option<ParsedRepository> {
            self.parser.parse_repo_url(url)
        }

        fn verify_provider_account(&self, credential: &Value) -> Result<Value, GitProviderError> {
            Ok(credential.clone())
        }

        fn credential_capabilities(
            &self,
            credential: &Value,
        ) -> Result<GitProviderCapabilities, GitProviderError> {
            self.probe.credential_capabilities(credential)
        }

        fn list_repositories(
            &self,
            _credential: &Value,
            page: i64,
        ) -> Result<RepositoryPage, GitProviderError> {
            Ok(RepositoryPage {
                repositories: Vec::new(),
                page,
                has_next_page: false,
            })
        }

        fn get_repository(
            &self,
            credential: &Value,
            parsed: &ParsedRepository,
        ) -> Result<RemoteRepository, GitProviderError> {
            self.probe.get_repository(credential, parsed)
        }
    }

    #[test]
    fn upsert_lookup_branches_on_external_id_and_strips_host() {
        let with_id = plan_upsert_lookup(&remote(), "https://github.com//");
        assert_eq!(
            with_id,
            RepositoryLookup::ByExternalId {
                provider: "github".into(),
                host_url: "https://github.com".into(),
                external_id: "123".into(),
            }
        );
        let mut no_id = remote();
        no_id.external_id = String::new();
        assert_eq!(
            plan_upsert_lookup(&no_id, "https://github.com"),
            RepositoryLookup::ByFullName {
                provider: "github".into(),
                host_url: "https://github.com".into(),
                full_name: "acme/web".into(),
            }
        );
        // Only trailing slashes strip: no scheme prepend, no trim.
        let with_id = plan_upsert_lookup(&remote(), "github.com//");
        assert!(
            matches!(with_id, RepositoryLookup::ByExternalId { host_url, .. } if host_url == "github.com")
        );
    }

    #[test]
    fn upsert_sql_asserts_lookup_and_write_shapes() {
        assert!(REPOSITORY_FIND_BY_EXTERNAL_SQL.contains("AND external_id = $3"));
        assert!(REPOSITORY_FIND_BY_FULL_NAME_SQL.contains("AND full_name = $3"));
        for sql in [
            REPOSITORY_FIND_BY_EXTERNAL_SQL,
            REPOSITORY_FIND_BY_FULL_NAME_SQL,
        ] {
            assert!(sql.contains("FROM git_repositories"));
            assert!(sql.contains("deleted_at IS NULL"));
        }
        assert!(REPOSITORY_UPDATE_SQL.starts_with("UPDATE git_repositories SET external_id = $2"));
        assert!(REPOSITORY_UPDATE_SQL.contains("WHERE id = $1"));
        assert!(REPOSITORY_INSERT_SQL.starts_with("INSERT INTO git_repositories"));
        assert!(BINDING_GET_SQL.contains("JOIN git_repositories r ON r.id = b.repository_id"));
        assert!(BINDING_GET_SQL
            .contains("JOIN git_provider_accounts a ON a.id = b.provider_account_id"));
        // Bind deletes by project only (no workspace predicate, services.py:325).
        assert_eq!(
            BINDING_DELETE_SQL,
            "DELETE FROM git_repository_bindings WHERE project_id = $1"
        );
        assert_eq!(
            GITHUB_SYNC_DELETE_SQL,
            "DELETE FROM github_repository_syncs WHERE project_id = $1"
        );
        assert!(BINDING_INSERT_SQL.contains("clone_auth_mode"));
        assert!(BINDING_SET_SYNC_SQL.contains("is_sync_enabled = $2"));
        assert!(GITHUB_SYNC_SET_ENABLED_SQL
            .contains("workspace_id = (SELECT id FROM workspaces WHERE slug = $3)"));
    }

    #[test]
    fn project_repo_update_sql_skips_empty_field_lists() {
        assert_eq!(project_repo_update_sql(None, None), None);
        assert_eq!(
            project_repo_update_sql(Some("u"), None),
            Some("UPDATE projects SET repo_url = $2, updated_at = $3 WHERE id = $1".to_owned())
        );
        assert_eq!(
            project_repo_update_sql(Some("u"), Some("b")),
            Some(
                "UPDATE projects SET repo_url = $2, base_branch = $3, updated_at = $4 WHERE id = $1"
                    .to_owned()
            )
        );
    }

    #[test]
    fn canonical_clone_url_follows_or_chain() {
        let full = remote();
        assert_eq!(
            canonical_clone_url(&full, "https://github.com/acme/web"),
            "https://github.com/acme/web.git"
        );
        let mut no_http = remote();
        no_http.clone_url_http = String::new();
        assert_eq!(
            canonical_clone_url(&no_http, "https://github.com/acme/web"),
            "https://github.com/acme/web"
        );
        assert_eq!(
            canonical_clone_url(&no_http, ""),
            "https://github.com/acme/web"
        );
        let mut bare = no_http.clone();
        bare.web_url = String::new();
        assert_eq!(canonical_clone_url(&bare, ""), "");
    }

    #[test]
    fn parse_tries_adapters_in_order_and_misses() {
        let github = StaticParser {
            key: "github",
            host: "github.com",
        };
        let gitlab = StaticParser {
            key: "gitlab",
            host: "gitlab.example.com",
        };
        let adapters: &[&dyn ProviderAdapter] = &[&github, &gitlab];
        let parsed =
            parse_repository_url(adapters, "https://github.com/acme/web.git").expect("parses");
        assert_eq!(parsed.provider, "github");
        assert_eq!(parsed.full_name, "acme/web");
        assert!(parse_repository_url(adapters, "https://example.test/acme/web").is_none());

        // Lookup is case-insensitive ((provider or "").lower()).
        assert!(lookup_adapter(adapters, "GitHub").is_ok());
        let Err(missing) = lookup_adapter(adapters, "bitbucket") else {
            panic!("unknown provider must fail lookup")
        };
        assert_eq!(missing.message(), "Unsupported Git provider: bitbucket");
    }

    #[tokio::test]
    async fn upsert_creates_then_updates() {
        let store = FullFake::with_project();
        let created = upsert_repository(&store, &remote(), "https://github.com/", stamp())
            .await
            .expect("creates");
        assert_eq!(created.full_name, "acme/web");

        let mut changed = remote();
        changed.default_branch = "develop".into();
        let updated = upsert_repository(&store, &changed, "https://github.com", stamp())
            .await
            .expect("updates");
        assert_eq!(updated.id, created.id);
        assert_eq!(updated.default_branch, "develop");
        assert_eq!(store.repos.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn bind_rejects_unsupported_urls() {
        let store = FullFake::with_project();
        let github = ParseAndProbe::github(&["pat"]);
        let adapters: &[&dyn ProviderAdapter] = &[&github];
        let error = bind_repository(
            &store,
            &ctx(),
            adapters,
            BindRequest {
                workspace_slug: FullFake::workspace_slug(),
                project_id: FullFake::project_id(),
                raw_url: "https://example.test/acme/web",
                provider_account_id: None,
            },
            stamp(),
        )
        .await
        .expect_err("unsupported");
        assert!(matches!(
            error,
            ServiceError::Integration(GitIntegrationError::UnsupportedRepositoryUrl(_))
        ));
    }

    #[tokio::test]
    async fn bind_missing_workspace_or_project_is_not_found() {
        let store = FullFake::with_project();
        let github = ParseAndProbe::github(&["pat"]);
        let adapters: &[&dyn ProviderAdapter] = &[&github];
        let error = bind_repository(
            &store,
            &ctx(),
            adapters,
            BindRequest {
                workspace_slug: "missing",
                project_id: FullFake::project_id(),
                raw_url: "https://github.com/acme/web",
                provider_account_id: None,
            },
            stamp(),
        )
        .await
        .expect_err("no workspace");
        assert!(matches!(
            error,
            ServiceError::Store(StoreError::NotFound("workspace"))
        ));

        let error = bind_repository(
            &store,
            &ctx(),
            adapters,
            BindRequest {
                workspace_slug: FullFake::workspace_slug(),
                project_id: Uuid::new_v4(),
                raw_url: "https://github.com/acme/web",
                provider_account_id: None,
            },
            stamp(),
        )
        .await
        .expect_err("no project");
        assert!(matches!(
            error,
            ServiceError::Store(StoreError::NotFound("project"))
        ));
    }

    #[tokio::test]
    async fn bind_full_cycle_replaces_binding_and_clears_syncs() {
        let workspace_id = FullFake::workspace_id();
        let project_id = FullFake::project_id();
        let store = FullFake::with_project();
        let pat = probe_account(workspace_id, "pat", "pat", true, stamp());
        store.accounts.lock().unwrap().push(pat.clone());
        store.github_syncs.lock().unwrap().insert(project_id, true);

        let combo = ParseAndProbe::github(&["pat"]);
        let adapters: &[&dyn ProviderAdapter] = &[&combo];

        let request = || BindRequest {
            workspace_slug: FullFake::workspace_slug(),
            project_id,
            raw_url: "https://github.com/acme/web",
            provider_account_id: None,
        };
        let (first, clone_url) = bind_repository(&store, &ctx(), adapters, request(), stamp())
            .await
            .expect("binds");
        assert_eq!(clone_url, "https://github.com/acme/web.git");
        assert_eq!(first.provider_account_id, pat.id);
        assert_eq!(
            first.clone_auth_mode,
            git_repository_binding::CLONE_AUTH_MODE_RUNNER_MANAGED
        );
        assert!(!first.is_sync_enabled);
        // Project updated (repo_url differed, base_branch was empty).
        let project = store.projects.lock().unwrap()[&project_id].clone();
        assert_eq!(project.repo_url, "https://github.com/acme/web.git");
        assert_eq!(project.base_branch, "main");
        // Github sync rows for the project were hard-deleted on bind.
        assert!(!store.github_syncs.lock().unwrap().contains_key(&project_id));

        // Rebinding replaces the existing binding (one row per project).
        let (second, _) = bind_repository(&store, &ctx(), adapters, request(), stamp())
            .await
            .expect("rebinds");
        assert_ne!(first.id, second.id);
        assert_eq!(store.bindings.lock().unwrap().len(), 1);
        // Second bind writes nothing to the project (values unchanged).
        let project = store.projects.lock().unwrap()[&project_id].clone();
        assert_eq!(project.repo_url, "https://github.com/acme/web.git");
    }

    #[tokio::test]
    async fn get_set_and_unbind_cover_binding_lifecycle() {
        let workspace_id = FullFake::workspace_id();
        let project_id = FullFake::project_id();
        let store = FullFake::with_project();
        let pat = probe_account(workspace_id, "pat", "pat", true, stamp());
        store.accounts.lock().unwrap().push(pat.clone());

        // Missing binding → None; enabling sync → NotFound("Repository
        // is not bound").
        assert!(get_binding(&store, FullFake::workspace_slug(), project_id)
            .await
            .expect("reads")
            .is_none());
        let error = set_binding_sync_enabled(
            &store,
            FullFake::workspace_slug(),
            project_id,
            true,
            stamp(),
        )
        .await
        .expect_err("not bound");
        assert!(matches!(
            error,
            ServiceError::Integration(GitIntegrationError::ProviderAccountNotFound(_))
        ));

        // Seed a github binding + a github sync row.
        let repo = upsert_repository(&store, &remote(), "https://github.com", stamp())
            .await
            .expect("seeds repo");
        let binding = store
            .apply_bind(BindPlan {
                project_id,
                workspace_id,
                repository_id: repo.id,
                provider_account_id: pat.id,
                actor_id: None,
                clone_auth_mode: git_repository_binding::CLONE_AUTH_MODE_PUBLIC.into(),
                raw_url: "https://github.com/acme/web".into(),
                repo_url_update: None,
                base_branch_update: None,
                now: stamp(),
            })
            .await
            .expect("seeds binding");
        store.github_syncs.lock().unwrap().insert(project_id, false);

        let view = get_binding(&store, FullFake::workspace_slug(), project_id)
            .await
            .expect("reads")
            .expect("present");
        assert_eq!(view.binding.id, binding.id);
        assert_eq!(view.repository.id, repo.id);
        assert_eq!(view.account.id, pat.id);

        // Enabling sync flips the binding and mirrors onto github syncs.
        let updated = set_binding_sync_enabled(
            &store,
            FullFake::workspace_slug(),
            project_id,
            true,
            stamp(),
        )
        .await
        .expect("enables");
        assert!(updated.is_sync_enabled);
        assert!(store.github_syncs.lock().unwrap()[&project_id]);

        // Unbind removes the binding and the sync rows.
        unbind_repository(&store, FullFake::workspace_slug(), project_id)
            .await
            .expect("unbinds");
        assert!(get_binding(&store, FullFake::workspace_slug(), project_id)
            .await
            .expect("reads")
            .is_none());
        assert!(!store.github_syncs.lock().unwrap().contains_key(&project_id));

        // Unbinding again is a no-op (no binding), sync delete still runs.
        unbind_repository(&store, FullFake::workspace_slug(), project_id)
            .await
            .expect("idempotent");
    }

    #[tokio::test]
    async fn set_sync_skips_github_mirror_for_other_providers() {
        let workspace_id = FullFake::workspace_id();
        let project_id = FullFake::project_id();
        let store = FullFake::with_project();
        let account = probe_account(workspace_id, "group_token", "g", false, stamp());
        store.accounts.lock().unwrap().push(account.clone());
        let mut gitlab_remote = remote();
        gitlab_remote.provider = "gitlab".into();
        gitlab_remote.external_id = String::new();
        gitlab_remote.full_name = "grp/svc".into();
        let repo = upsert_repository(
            &store,
            &gitlab_remote,
            "https://gitlab.example.com",
            stamp(),
        )
        .await
        .expect("seeds repo");
        store
            .apply_bind(BindPlan {
                project_id,
                workspace_id,
                repository_id: repo.id,
                provider_account_id: account.id,
                actor_id: None,
                clone_auth_mode: git_repository_binding::CLONE_AUTH_MODE_PUBLIC.into(),
                raw_url: "https://gitlab.example.com/grp/svc".into(),
                repo_url_update: None,
                base_branch_update: None,
                now: stamp(),
            })
            .await
            .expect("seeds binding");
        store.github_syncs.lock().unwrap().insert(project_id, false);

        let updated = set_binding_sync_enabled(
            &store,
            FullFake::workspace_slug(),
            project_id,
            true,
            stamp(),
        )
        .await
        .expect("enables");
        assert!(updated.is_sync_enabled);
        // Non-github provider: the sync table is untouched.
        assert!(!store.github_syncs.lock().unwrap()[&project_id]);
    }

    #[test]
    fn list_repositories_serializes_against_account_host() {
        struct ListAdapter {
            remotes: Vec<RemoteRepository>,
        }

        impl ProviderAdapter for ListAdapter {
            fn key(&self) -> &'static str {
                "github"
            }

            fn display_name(&self) -> &'static str {
                "GitHub"
            }

            fn parse_repo_url(&self, _url: &str) -> Option<ParsedRepository> {
                None
            }

            fn verify_provider_account(
                &self,
                credential: &Value,
            ) -> Result<Value, GitProviderError> {
                Ok(credential.clone())
            }

            fn credential_capabilities(
                &self,
                _credential: &Value,
            ) -> Result<GitProviderCapabilities, GitProviderError> {
                Ok(GitProviderCapabilities {
                    read_repositories: true,
                    read_issues: true,
                    write_comments: false,
                    manage_webhooks: false,
                    clone: false,
                })
            }

            fn list_repositories(
                &self,
                _credential: &Value,
                page: i64,
            ) -> Result<RepositoryPage, GitProviderError> {
                Ok(RepositoryPage {
                    repositories: self.remotes.clone(),
                    page,
                    has_next_page: true,
                })
            }

            fn get_repository(
                &self,
                _credential: &Value,
                parsed: &ParsedRepository,
            ) -> Result<RemoteRepository, GitProviderError> {
                Err(GitProviderError::NotFound(parsed.full_name.clone()))
            }
        }

        let workspace_id = FullFake::workspace_id();
        let account = probe_account(workspace_id, "pat", "pat", true, stamp());
        let adapter = ListAdapter {
            remotes: vec![remote()],
        };
        let list = list_account_repositories(&adapter, &account, 3).expect("lists");
        // Page passes straight through (services.py:383,386-387).
        assert_eq!(list.page, 3);
        assert!(list.has_next_page);
        assert_eq!(list.repos.len(), 1);
        assert_eq!(list.repos[0].host_url, "https://github.com");
        assert_eq!(list.repos[0].id, "123");
        let rendered = serde_json::to_string(&list).unwrap();
        assert!(rendered.starts_with(r#"{"repos":[{"id":"123""#));
    }
}
