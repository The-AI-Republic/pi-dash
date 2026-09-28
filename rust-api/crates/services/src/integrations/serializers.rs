//! Git integration serializers (D-05, stage 5).
//!
//! Ports `apps/api/pi_dash/integrations/git/services.py:232-296`:
//! `serialize_repository` (232-247), `serialize_remote_repository`
//! (248-263), `serialize_provider_account` (264-279), `serialize_binding`
//! (280-296).
//!
//! JSON contract: every serializer emits its keys in the exact Python
//! `dict` insertion order below (serde emits struct fields in declaration
//! order even without the workspace `preserve_order` feature, which only
//! affects maps). `None` renders `null`, matching Python's `None` values.
//! Datetimes render through [`isoformat`], which matches Django
//! `datetime.isoformat()` (`+00:00` suffix, microseconds iff nonzero).
//!
//! Fixture: `rust-api/fixtures/integrations/services/serializers.golden.json`
//! (field lists per serializer, with source lines).

use chrono::{DateTime, SecondsFormat, Utc};
use serde::{Deserialize, Serialize};

use pidash_db::integrations::git_models::{
    git_provider_account, git_provider_account::GitProviderAccount, git_repository::GitRepository,
    git_repository_binding::GitRepositoryBinding,
};
use pidash_types::integrations::RemoteRepository;

/// Django `datetime.isoformat()` for a timezone-aware timestamp.
///
/// Renders `YYYY-MM-DDTHH:MM:SS+00:00`, with fractional seconds iff
/// nonzero (Postgres timestamps carry microseconds, so six digits, exactly
/// like Python). `serialize_*` take `Option<DateTime<Utc>>` and render
/// `None` as `null`, matching `… if x else None`.
pub fn isoformat(moment: &DateTime<Utc>) -> String {
    moment.to_rfc3339_opts(SecondsFormat::AutoSi, false)
}

/// `serialize_repository` (`services.py:232-245`).
///
/// `"id"` is the repository `external_id`; `"private"` renames
/// `is_private`. Key order is the Python dict order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SerializedRepository {
    pub id: String,
    pub provider: String,
    pub host_url: String,
    pub namespace: String,
    pub name: String,
    pub full_name: String,
    pub web_url: String,
    pub clone_url_http: String,
    pub clone_url_ssh: String,
    pub default_branch: String,
    pub r#private: bool,
}

/// `serialize_remote_repository` (`services.py:248-261`).
///
/// Same shape as [`SerializedRepository`], but `"host_url"` is the
/// caller-supplied host (the account's stored URL), NOT
/// `remote.web_url`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SerializedRemoteRepository {
    pub id: String,
    pub provider: String,
    pub host_url: String,
    pub namespace: String,
    pub name: String,
    pub full_name: String,
    pub web_url: String,
    pub clone_url_http: String,
    pub clone_url_ssh: String,
    pub default_branch: String,
    pub r#private: bool,
}

/// `serialize_provider_account` (`services.py:264-277`).
///
/// `"id"` stringifies the UUID; `"verified_at"` is ISO-8601 or `null`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SerializedProviderAccount {
    pub id: String,
    pub provider: String,
    pub host_url: String,
    pub auth_type: String,
    pub external_account_id: String,
    pub external_account_login: String,
    pub display_name: String,
    pub capabilities: serde_json::Value,
    pub status: String,
    pub verified_at: Option<String>,
    pub last_check_error: String,
}

/// `serialize_binding` (`services.py:280-294`).
///
/// `"bound"` is always `true` and always first; `"provider"` and
/// `"host_url"` come from the joined repository, `"degraded"` is
/// `account.status != 'connected'`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SerializedBinding {
    pub bound: bool,
    pub id: String,
    pub provider: String,
    pub provider_account_id: String,
    pub host_url: String,
    pub repository: SerializedRepository,
    pub is_sync_enabled: bool,
    pub clone_auth_mode: String,
    pub last_synced_at: Option<String>,
    pub last_sync_error: String,
    pub degraded: bool,
    pub degraded_reason: String,
}

/// Build [`SerializedRepository`] from a `git_repositories` row.
pub fn serialize_repository(repo: &GitRepository) -> SerializedRepository {
    SerializedRepository {
        id: repo.external_id.clone(),
        provider: repo.provider.clone(),
        host_url: repo.host_url.clone(),
        namespace: repo.namespace.clone(),
        name: repo.name.clone(),
        full_name: repo.full_name.clone(),
        web_url: repo.web_url.clone(),
        clone_url_http: repo.clone_url_http.clone(),
        clone_url_ssh: repo.clone_url_ssh.clone(),
        default_branch: repo.default_branch.clone(),
        r#private: repo.is_private,
    }
}

/// Build [`SerializedRemoteRepository`] from a provider DTO.
///
/// `host_url` is caller-supplied (`services.py:252`); it is NOT derived
/// from the remote.
pub fn serialize_remote_repository(
    repo: &RemoteRepository,
    host_url: &str,
) -> SerializedRemoteRepository {
    SerializedRemoteRepository {
        id: repo.external_id.clone(),
        provider: repo.provider.clone(),
        host_url: host_url.to_owned(),
        namespace: repo.namespace.clone(),
        name: repo.name.clone(),
        full_name: repo.full_name.clone(),
        web_url: repo.web_url.clone(),
        clone_url_http: repo.clone_url_http.clone(),
        clone_url_ssh: repo.clone_url_ssh.clone(),
        default_branch: repo.default_branch.clone(),
        r#private: repo.is_private,
    }
}

/// Build [`SerializedProviderAccount`] from a `git_provider_accounts` row.
pub fn serialize_provider_account(account: &GitProviderAccount) -> SerializedProviderAccount {
    SerializedProviderAccount {
        id: account.id.to_string(),
        provider: account.provider.clone(),
        host_url: account.host_url.clone(),
        auth_type: account.auth_type.clone(),
        external_account_id: account.external_account_id.clone(),
        external_account_login: account.external_account_login.clone(),
        display_name: account.display_name.clone(),
        capabilities: account.capabilities.clone(),
        status: account.status.clone(),
        verified_at: account.verified_at.as_ref().map(isoformat),
        last_check_error: account.last_check_error.clone(),
    }
}

/// Build [`SerializedBinding`] from a binding row plus its
/// `select_related` repository and provider-account rows.
pub fn serialize_binding(
    binding: &GitRepositoryBinding,
    repository: &GitRepository,
    account: &GitProviderAccount,
) -> SerializedBinding {
    SerializedBinding {
        bound: true,
        id: binding.id.to_string(),
        provider: repository.provider.clone(),
        provider_account_id: binding.provider_account_id.to_string(),
        host_url: repository.host_url.clone(),
        repository: serialize_repository(repository),
        is_sync_enabled: binding.is_sync_enabled,
        clone_auth_mode: binding.clone_auth_mode.clone(),
        last_synced_at: binding.last_synced_at.as_ref().map(isoformat),
        last_sync_error: binding.last_sync_error.clone(),
        degraded: account.status != git_provider_account::STATUS_CONNECTED,
        degraded_reason: account.last_check_error.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use serde_json::{json, Value};

    fn golden() -> Value {
        let path = format!(
            "{}/../../fixtures/integrations/services/serializers.golden.json",
            env!("CARGO_MANIFEST_DIR")
        );
        serde_json::from_str(&std::fs::read_to_string(&path).expect("golden exists"))
            .expect("golden parses")
    }

    /// Top-level JSON key order of a struct's serialization.
    ///
    /// Read off the serialized string, not a `serde_json::Value`: the
    /// workspace `serde_json` has no `preserve_order` feature, so `Value`
    /// objects iterate alphabetically while struct serialization always
    /// emits declaration order.
    fn serialized_keys<T: serde::Serialize>(value: &T) -> Vec<String> {
        let rendered = serde_json::to_string(value).expect("serializes");
        let mut keys = Vec::new();
        let mut depth = 0usize;
        let mut chars = rendered.chars().peekable();
        while let Some(c) = chars.next() {
            match c {
                '{' => {
                    depth += 1;
                }
                '}' => {
                    depth -= 1;
                }
                '"' if depth == 1 => {
                    let mut key = String::new();
                    while let Some(&next) = chars.peek() {
                        chars.next();
                        if next == '"' {
                            break;
                        }
                        key.push(next);
                    }
                    if chars.peek() == Some(&':') {
                        keys.push(key);
                    }
                }
                _ => {}
            }
        }
        keys
    }

    fn stamp() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2024, 1, 2, 3, 4, 5).unwrap()
    }

    fn stamp_micros() -> DateTime<Utc> {
        stamp() + chrono::Duration::microseconds(123_456)
    }

    fn account() -> GitProviderAccount {
        GitProviderAccount {
            id: uuid::Uuid::parse_str("11111111-1111-1111-1111-111111111111").unwrap(),
            created_at: stamp(),
            updated_at: stamp(),
            created_by_id: None,
            updated_by_id: None,
            deleted_at: None,
            workspace_id: uuid::Uuid::nil(),
            provider: "github".into(),
            host_url: "https://github.com".into(),
            auth_type: "pat".into(),
            external_account_id: "42".into(),
            external_account_login: "octo".into(),
            display_name: "octo".into(),
            capabilities: json!({"read_repositories": true, "clone": false}),
            credential_config: json!({}),
            workspace_integration_id: None,
            status: "connected".into(),
            verified_at: Some(stamp()),
            last_check_error: String::new(),
            metadata: json!({}),
        }
    }

    fn repository() -> GitRepository {
        GitRepository {
            id: uuid::Uuid::nil(),
            created_at: stamp(),
            updated_at: stamp(),
            created_by_id: None,
            updated_by_id: None,
            deleted_at: None,
            provider: "github".into(),
            host_url: "https://github.com".into(),
            external_id: "123".into(),
            namespace: "acme".into(),
            name: "web".into(),
            full_name: "acme/web".into(),
            web_url: "https://github.com/acme/web".into(),
            clone_url_http: "https://github.com/acme/web.git".into(),
            clone_url_ssh: "git@github.com:acme/web.git".into(),
            default_branch: "main".into(),
            is_private: true,
            metadata: json!({}),
        }
    }

    fn binding() -> GitRepositoryBinding {
        GitRepositoryBinding {
            id: uuid::Uuid::parse_str("22222222-2222-2222-2222-222222222222").unwrap(),
            created_at: stamp(),
            updated_at: stamp(),
            created_by_id: None,
            updated_by_id: None,
            deleted_at: None,
            project_id: uuid::Uuid::nil(),
            workspace_id: uuid::Uuid::nil(),
            repository_id: uuid::Uuid::nil(),
            provider_account_id: account().id,
            actor_id: uuid::Uuid::nil(),
            is_sync_enabled: false,
            clone_auth_mode: "runner_managed".into(),
            last_synced_at: None,
            last_sync_error: String::new(),
            metadata: json!({"raw_url": "https://github.com/acme/web"}),
        }
    }

    #[test]
    fn golden_file_covers_all_four_serializers() {
        let gold = golden();
        for section in [
            "serialize_binding",
            "serialize_provider_account",
            "serialize_remote_repository",
            "serialize_repository",
        ] {
            assert!(gold.get(section).is_some(), "{section} present");
            assert!(
                gold[section].get("source").is_some(),
                "{section} traces its Python source"
            );
        }
    }

    #[test]
    fn repository_key_order_and_bytes_match_python() {
        let rendered = serde_json::to_string(&serialize_repository(&repository())).unwrap();
        assert_eq!(
            rendered,
            r#"{"id":"123","provider":"github","host_url":"https://github.com","namespace":"acme","name":"web","full_name":"acme/web","web_url":"https://github.com/acme/web","clone_url_http":"https://github.com/acme/web.git","clone_url_ssh":"git@github.com:acme/web.git","default_branch":"main","private":true}"#
        );
        assert_eq!(
            serialized_keys(&serialize_repository(&repository())),
            vec![
                "id",
                "provider",
                "host_url",
                "namespace",
                "name",
                "full_name",
                "web_url",
                "clone_url_http",
                "clone_url_ssh",
                "default_branch",
                "private"
            ]
        );
    }

    #[test]
    fn remote_repository_uses_caller_host_url() {
        let remote = RemoteRepository {
            provider: "gitlab".into(),
            external_id: "7".into(),
            namespace: "grp".into(),
            name: "svc".into(),
            full_name: "grp/svc".into(),
            web_url: "https://gitlab.example.com/grp/svc".into(),
            clone_url_http: String::new(),
            clone_url_ssh: String::new(),
            default_branch: String::new(),
            is_private: false,
            metadata: json!({}),
        };
        let rendered = serde_json::to_string(&serialize_remote_repository(
            &remote,
            "https://gitlab.example.com",
        ))
        .unwrap();
        // host_url is the caller-supplied value even though the remote
        // carries its own web_url (services.py:252).
        assert!(rendered.contains(r#""host_url":"https://gitlab.example.com""#));
        assert_eq!(
            serialized_keys(&serialize_remote_repository(&remote, "https://x.test")),
            vec![
                "id",
                "provider",
                "host_url",
                "namespace",
                "name",
                "full_name",
                "web_url",
                "clone_url_http",
                "clone_url_ssh",
                "default_branch",
                "private"
            ]
        );
    }

    #[test]
    fn provider_account_renders_iso_verified_at_or_null() {
        let rendered = serde_json::to_string(&serialize_provider_account(&account())).unwrap();
        assert!(rendered.starts_with(
            r#"{"id":"11111111-1111-1111-1111-111111111111","provider":"github","host_url":"https://github.com","auth_type":"pat","external_account_id":"42","external_account_login":"octo","display_name":"octo","capabilities":{"read_repositories":true,"clone":false},"status":"connected","verified_at":"2024-01-02T03:04:05+00:00","last_check_error":""}"#
        ));
        let mut revoked = account();
        revoked.verified_at = None;
        let rendered = serde_json::to_string(&serialize_provider_account(&revoked)).unwrap();
        assert!(rendered.contains(r#""verified_at":null"#));
        assert_eq!(
            serialized_keys(&serialize_provider_account(&account())),
            vec![
                "id",
                "provider",
                "host_url",
                "auth_type",
                "external_account_id",
                "external_account_login",
                "display_name",
                "capabilities",
                "status",
                "verified_at",
                "last_check_error"
            ]
        );
    }

    #[test]
    fn isoformat_matches_python_for_micros_and_zero() {
        assert_eq!(isoformat(&stamp()), "2024-01-02T03:04:05+00:00");
        assert_eq!(
            isoformat(&stamp_micros()),
            "2024-01-02T03:04:05.123456+00:00"
        );
    }

    #[test]
    fn binding_marks_degraded_off_connected_status() {
        let repo = repository();
        let rendered =
            serde_json::to_string(&serialize_binding(&binding(), &repo, &account())).unwrap();
        assert!(rendered.starts_with(r#"{"bound":true,"id":"22222222-2222-2222-2222-222222222222","provider":"github","provider_account_id":"11111111-1111-1111-1111-111111111111","host_url":"https://github.com","repository":{"#));
        assert!(rendered.contains(r#""degraded":false,"degraded_reason":"""#));

        let mut degraded = account();
        degraded.status = "degraded".into();
        degraded.last_check_error = "token expired".into();
        let rendered =
            serde_json::to_string(&serialize_binding(&binding(), &repo, &degraded)).unwrap();
        assert!(rendered.contains(r#""degraded":true,"degraded_reason":"token expired""#));

        // Any non-connected status degrades (services.py:292 `!=`).
        let mut revoked = account();
        revoked.status = "revoked".into();
        let rendered =
            serde_json::to_string(&serialize_binding(&binding(), &repo, &revoked)).unwrap();
        assert!(rendered.contains(r#""degraded":true"#));
    }
}
