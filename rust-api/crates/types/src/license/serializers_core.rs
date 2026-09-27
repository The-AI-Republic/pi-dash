//! D-01 core serializers: instance / configuration / admin (output shapes).
//!
//! Port of `apps/api/pi_dash/license/api/serializers/`:
//!
//! * `base.py:8-9` (`BaseSerializer`, `id = PrimaryKeyRelatedField(read_only=True)`)
//! * `instance.py:11-17` (`InstanceSerializer`)
//! * `configuration.py:11-30` (`InstanceConfigurationSerializer`)
//! * `admin.py:12-42` (`InstanceAdminMeSerializer`, `InstanceAdminSerializer`)
//!
//! These are pure output shapes: each `to_representation` takes a row borrowed
//! from the caller and returns a `serde::Serialize` view whose field order is
//! the live DRF wire order (captured by executing the real serializers under
//! `pi_dash.settings.test`; see the wire-order note below). Datetimes cross
//! this boundary already rendered as DRF `iso-8601` strings (`+00:00` as `Z`,
//! microseconds only when nonzero) — rendering owns to the F-07 kernel
//! (`pidash_api::serializer::render_datetime`) at the DB edge, so formatting
//! here is a byte-exact passthrough. UUID and FK primary keys render as
//! strings (`PrimaryKeyRelatedField`, read-only); a null FK renders `null`.
//!
//! The decrypt step and the CONFIG registry live outside the types crate
//! (crate graph `types -> db -> services -> api`), so
//! [`instance_configuration_to_representation`] takes them as callbacks:
//! `decrypt` mirrors `decrypt_data(instance.value)` (`configuration.py:20`)
//! and `registry_source` mirrors `CONFIG.get(instance.key)` (`:25`). The
//! branch conditions themselves live here, exactly as in Python.
//!
//! Ported bugs (translate, don't redesign):
//!
//! * BUG-1 (`instance.py:12`): `primary_owner_details` (source
//!   `primary_owner`) is absent from every output. `Instance` has no
//!   `primary_owner` attribute, so DRF's `Field.get_attribute` raises and the
//!   nested field — not required, `allow_null` false — raises `SkipField`
//!   (DRF 3.15 `fields.py:450-456`). [`InstanceView`] therefore has no such
//!   key; see fixture `serializers/instance.golden.json`.
//! * BUG-me (`admin.py:15-32`): `Meta.fields` lists `is_email_verified` twice
//!   (`:23` and `:31`); the output object carries it once (dict semantics).
//!   [`AdminMeView`] declares it once, in first-occurrence position.
//!
//! Shape-only no-ops preserved as documentation, not code: `read_only_fields`
//! (`instance.py:17`, `admin.py:33,42`) constrain writes, of which this port
//! has none; the `email` entry of the instance `read_only_fields` names a
//! model field that does not exist, so no `email` key is emitted.

use serde::Serialize;

/// DRF wire order, recorded for the future handlers layer (captured by executing
/// the live serializers under `pi_dash.settings.test`):
///
/// * instance: `id, primary_owner_details(absent, BUG-1), created_at, updated_at,
///   deleted_at, instance_name, whitelist_emails, instance_id, current_version,
///   latest_version, edition, domain, last_checked_at, namespace,
///   is_telemetry_enabled, is_support_required, is_setup_done,
///   is_signup_screen_visited, is_verified, is_test,
///   is_current_version_deprecated, created_by, updated_by`
/// * configuration: `id, created_at, updated_at, deleted_at, key, value,
///   category, is_encrypted, created_by, updated_by`, then `to_representation`
///   appends `source, is_managed`
/// * admin: `id, user_detail, created_at, updated_at, deleted_at, role,
///   is_verified, created_by, updated_by, user, instance`
/// * lite user: `id, first_name, last_name, avatar, avatar_url, is_bot,
///   display_name, email, last_login_medium`
/// * admin-me: `Meta.fields` order with the duplicated `is_email_verified`
///   collapsed (`id, avatar, avatar_url, cover_image, date_joined,
///   display_name, email, first_name, last_name, is_active, is_bot,
///   is_email_verified, user_timezone, username, is_password_autoset`)
///
/// Note: the workspace `serde_json` does not enable `preserve_order`, so
/// objects serialize with alphabetically sorted keys — the same canonical
/// form the golden fixtures are stored in (`sort_keys`). Key order is
/// semantically irrelevant JSON; byte-identity is established in that
/// canonical form (see tests).
/// A database row for `Instance` (`license/models/instance.py:22-50` with
/// audit columns). Datetimes are pre-rendered DRF strings; ids are UUID
/// strings; nullable columns are `Option`.
#[derive(Debug, Clone, PartialEq)]
pub struct InstanceRow<'a> {
    pub id: &'a str,
    pub created_at: &'a str,
    pub updated_at: &'a str,
    pub deleted_at: Option<&'a str>,
    pub instance_name: &'a str,
    pub whitelist_emails: Option<&'a str>,
    pub instance_id: &'a str,
    pub current_version: &'a str,
    pub latest_version: Option<&'a str>,
    pub edition: &'a str,
    pub domain: &'a str,
    pub last_checked_at: &'a str,
    pub namespace: Option<&'a str>,
    pub is_telemetry_enabled: bool,
    pub is_support_required: bool,
    pub is_setup_done: bool,
    pub is_signup_screen_visited: bool,
    pub is_verified: bool,
    pub is_test: bool,
    pub is_current_version_deprecated: bool,
    pub created_by: Option<&'a str>,
    pub updated_by: Option<&'a str>,
}

/// `InstanceSerializer.to_representation` output (`instance.py:11-17`,
/// `fields = "__all__"`). No `email` key (no such model field) and no
/// `primary_owner_details` key (BUG-1: always skipped).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct InstanceView<'a> {
    pub id: &'a str,
    pub created_at: &'a str,
    pub updated_at: &'a str,
    pub deleted_at: Option<&'a str>,
    pub instance_name: &'a str,
    pub whitelist_emails: Option<&'a str>,
    pub instance_id: &'a str,
    pub current_version: &'a str,
    pub latest_version: Option<&'a str>,
    pub edition: &'a str,
    pub domain: &'a str,
    pub last_checked_at: &'a str,
    pub namespace: Option<&'a str>,
    pub is_telemetry_enabled: bool,
    pub is_support_required: bool,
    pub is_setup_done: bool,
    pub is_signup_screen_visited: bool,
    pub is_verified: bool,
    pub is_test: bool,
    pub is_current_version_deprecated: bool,
    pub created_by: Option<&'a str>,
    pub updated_by: Option<&'a str>,
}

/// Port of `InstanceSerializer` (`instance.py:11-17`). Field-for-field copy;
/// the BUG-1 key is absent because the view has no such field.
pub fn instance_to_representation<'a>(row: &'a InstanceRow<'a>) -> InstanceView<'a> {
    InstanceView {
        id: row.id,
        created_at: row.created_at,
        updated_at: row.updated_at,
        deleted_at: row.deleted_at,
        instance_name: row.instance_name,
        whitelist_emails: row.whitelist_emails,
        instance_id: row.instance_id,
        current_version: row.current_version,
        latest_version: row.latest_version,
        edition: row.edition,
        domain: row.domain,
        last_checked_at: row.last_checked_at,
        namespace: row.namespace,
        is_telemetry_enabled: row.is_telemetry_enabled,
        is_support_required: row.is_support_required,
        is_setup_done: row.is_setup_done,
        is_signup_screen_visited: row.is_signup_screen_visited,
        is_verified: row.is_verified,
        is_test: row.is_test,
        is_current_version_deprecated: row.is_current_version_deprecated,
        created_by: row.created_by,
        updated_by: row.updated_by,
    }
}

/// A database row for `InstanceConfiguration`
/// (`license/models/instance.py:72-83`). `value` is the stored ciphertext or
/// plaintext; `NULL` and `""` are distinct (`:75`).
#[derive(Debug, Clone, PartialEq)]
pub struct InstanceConfigurationRow<'a> {
    pub id: &'a str,
    pub created_at: &'a str,
    pub updated_at: &'a str,
    pub deleted_at: Option<&'a str>,
    pub key: &'a str,
    pub value: Option<&'a str>,
    pub category: &'a str,
    pub is_encrypted: bool,
    pub created_by: Option<&'a str>,
    pub updated_by: Option<&'a str>,
}

/// `InstanceConfigurationSerializer.to_representation` output
/// (`configuration.py:11-30`): the model fields plus `source` / `is_managed`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct InstanceConfigurationView<'a> {
    pub id: &'a str,
    pub created_at: &'a str,
    pub updated_at: &'a str,
    pub deleted_at: Option<&'a str>,
    pub key: &'a str,
    pub value: Option<String>,
    pub category: &'a str,
    pub is_encrypted: bool,
    pub created_by: Option<&'a str>,
    pub updated_by: Option<&'a str>,
    pub source: &'a str,
    pub is_managed: bool,
}

/// Port of `InstanceConfigurationSerializer.to_representation`
/// (`configuration.py:18-30`).
///
/// * Decrypt (`:19-20`): when `is_encrypted` and `value` is not `None`,
///   `value` is `decrypt(value)`. `None` stays `null` even when encrypted.
/// * Source (`:25-28`): `registry_source(key)` is the entry's `source`;
///   an unregistered key (`None`) reads `"db"`. `is_managed` is
///   `source == "env"`.
pub fn instance_configuration_to_representation<'a>(
    row: &'a InstanceConfigurationRow<'a>,
    decrypt: &dyn Fn(&str) -> String,
    registry_source: &dyn Fn(&str) -> Option<&'a str>,
) -> InstanceConfigurationView<'a> {
    let value = match (row.is_encrypted, row.value) {
        (true, Some(stored)) => Some(decrypt(stored)),
        _ => row.value.map(str::to_owned),
    };
    let source = registry_source(row.key).unwrap_or("db");
    InstanceConfigurationView {
        id: row.id,
        created_at: row.created_at,
        updated_at: row.updated_at,
        deleted_at: row.deleted_at,
        key: row.key,
        value,
        category: row.category,
        is_encrypted: row.is_encrypted,
        created_by: row.created_by,
        updated_by: row.updated_by,
        source,
        is_managed: source == "env",
    }
}

/// The nine `UserAdminLiteSerializer` fields
/// (`app/serializers/user.py:156-170`) for one nested user.
#[derive(Debug, Clone, PartialEq)]
pub struct LiteUserRow<'a> {
    pub id: &'a str,
    pub first_name: &'a str,
    pub last_name: &'a str,
    pub avatar: &'a str,
    pub avatar_url: Option<&'a str>,
    pub is_bot: bool,
    pub display_name: &'a str,
    pub email: &'a str,
    pub last_login_medium: Option<&'a str>,
}

/// Nested `user_detail` rendering (`admin.py:37`, `source="user"`).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct LiteUserView<'a> {
    pub id: &'a str,
    pub first_name: &'a str,
    pub last_name: &'a str,
    pub avatar: &'a str,
    pub avatar_url: Option<&'a str>,
    pub is_bot: bool,
    pub display_name: &'a str,
    pub email: &'a str,
    pub last_login_medium: Option<&'a str>,
}

fn lite_user_to_representation<'a>(user: &'a LiteUserRow<'a>) -> LiteUserView<'a> {
    LiteUserView {
        id: user.id,
        first_name: user.first_name,
        last_name: user.last_name,
        avatar: user.avatar,
        avatar_url: user.avatar_url,
        is_bot: user.is_bot,
        display_name: user.display_name,
        email: user.email,
        last_login_medium: user.last_login_medium,
    }
}

/// A database row for `InstanceAdmin` (`license/models/instance.py:53-69`)
/// with its `user` prefetched. `user` is `None` when the FK is null
/// (`SET_NULL`); `user_id` is the raw FK string rendered by DRF.
#[derive(Debug, Clone, PartialEq)]
pub struct InstanceAdminRow<'a> {
    pub id: &'a str,
    pub user: Option<LiteUserRow<'a>>,
    pub created_at: &'a str,
    pub updated_at: &'a str,
    pub deleted_at: Option<&'a str>,
    pub role: i32,
    pub is_verified: bool,
    pub created_by: Option<&'a str>,
    pub updated_by: Option<&'a str>,
    pub user_id: Option<&'a str>,
    pub instance_id: &'a str,
}

/// `InstanceAdminSerializer.to_representation` output (`admin.py:36-42`,
/// `fields = "__all__"`). Raw FKs render as string PKs (`user`, `instance`);
/// `user_detail` nests the lite rendering of the same `user`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct InstanceAdminView<'a> {
    pub id: &'a str,
    pub user_detail: Option<LiteUserView<'a>>,
    pub created_at: &'a str,
    pub updated_at: &'a str,
    pub deleted_at: Option<&'a str>,
    pub role: i32,
    pub is_verified: bool,
    pub created_by: Option<&'a str>,
    pub updated_by: Option<&'a str>,
    pub user: Option<&'a str>,
    pub instance: &'a str,
}

/// Port of `InstanceAdminSerializer` (`admin.py:36-42`).
pub fn instance_admin_to_representation<'a>(
    row: &'a InstanceAdminRow<'a>,
) -> InstanceAdminView<'a> {
    InstanceAdminView {
        id: row.id,
        user_detail: row.user.as_ref().map(lite_user_to_representation),
        created_at: row.created_at,
        updated_at: row.updated_at,
        deleted_at: row.deleted_at,
        role: row.role,
        is_verified: row.is_verified,
        created_by: row.created_by,
        updated_by: row.updated_by,
        user: row.user_id,
        instance: row.instance_id,
    }
}

/// A `User` row projected onto the sixteen `Meta.fields` entries of
/// `InstanceAdminMeSerializer` (`admin.py:15-32`).
#[derive(Debug, Clone, PartialEq)]
pub struct AdminMeRow<'a> {
    pub id: &'a str,
    pub avatar: &'a str,
    pub avatar_url: Option<&'a str>,
    pub cover_image: Option<&'a str>,
    pub date_joined: &'a str,
    pub display_name: &'a str,
    pub email: &'a str,
    pub first_name: &'a str,
    pub last_name: &'a str,
    pub is_active: bool,
    pub is_bot: bool,
    pub is_email_verified: bool,
    pub user_timezone: &'a str,
    pub username: &'a str,
    pub is_password_autoset: bool,
}

/// `InstanceAdminMeSerializer.to_representation` output (`admin.py:12-33`,
/// all `read_only`). The duplicated `is_email_verified` entry appears once.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct AdminMeView<'a> {
    pub id: &'a str,
    pub avatar: &'a str,
    pub avatar_url: Option<&'a str>,
    pub cover_image: Option<&'a str>,
    pub date_joined: &'a str,
    pub display_name: &'a str,
    pub email: &'a str,
    pub first_name: &'a str,
    pub last_name: &'a str,
    pub is_active: bool,
    pub is_bot: bool,
    pub is_email_verified: bool,
    pub user_timezone: &'a str,
    pub username: &'a str,
    pub is_password_autoset: bool,
}

/// Port of `InstanceAdminMeSerializer` (`admin.py:12-33`).
pub fn instance_admin_me_to_representation<'a>(row: &'a AdminMeRow<'a>) -> AdminMeView<'a> {
    AdminMeView {
        id: row.id,
        avatar: row.avatar,
        avatar_url: row.avatar_url,
        cover_image: row.cover_image,
        date_joined: row.date_joined,
        display_name: row.display_name,
        email: row.email,
        first_name: row.first_name,
        last_name: row.last_name,
        is_active: row.is_active,
        is_bot: row.is_bot,
        is_email_verified: row.is_email_verified,
        user_timezone: row.user_timezone,
        username: row.username,
        is_password_autoset: row.is_password_autoset,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    fn fixture(name: &str) -> Value {
        let path = format!(
            "{}/../../fixtures/license/serializers/{name}.golden.json",
            env!("CARGO_MANIFEST_DIR")
        );
        serde_json::from_str(&std::fs::read_to_string(&path).expect("golden exists"))
            .expect("golden parses")
    }

    fn req<'a>(obj: &'a Value, key: &str) -> &'a str {
        obj.get(key).and_then(Value::as_str).unwrap_or_else(|| {
            panic!("golden input lacks required string key {key}");
        })
    }

    fn opt(obj: &Value, key: &str) -> Option<String> {
        match obj.get(key) {
            None | Some(Value::Null) => None,
            Some(Value::String(s)) => Some(s.clone()),
            Some(other) => panic!("golden key {key} is not a string/null: {other}"),
        }
    }

    fn boolean(obj: &Value, key: &str) -> bool {
        obj.get(key).and_then(Value::as_bool).unwrap_or_else(|| {
            panic!("golden input lacks required bool key {key}");
        })
    }

    /// Field-for-field equality with the golden output plus byte-identical
    /// replay: under this workspace's `serde_json` (sorted keys, no
    /// `preserve_order`) `to_string` is deterministic, so string equality is
    /// byte equality of the canonical form the goldens are stored in.
    fn assert_replay(produced: &Value, expected: &Value) {
        assert_eq!(
            produced, expected,
            "field-for-field mismatch against golden output"
        );
        assert_eq!(
            serde_json::to_string(produced).expect("serializes"),
            serde_json::to_string(expected).expect("serializes"),
            "byte-identical replay mismatch"
        );
    }

    #[test]
    fn instance_replays_golden() {
        // Fixture serializers/instance.golden.json: input row -> exact output.
        // Proves BUG-1 on the Rust side: neither `email` nor
        // `primary_owner_details` appears in the output.
        let golden = fixture("instance");
        let input = golden.get("input_row").expect("input_row");
        let created_by = opt(input, "created_by");
        let updated_by = opt(input, "updated_by");
        let deleted_at = opt(input, "deleted_at");
        let whitelist_emails = opt(input, "whitelist_emails");
        let latest_version = opt(input, "latest_version");
        let namespace = opt(input, "namespace");
        let row = InstanceRow {
            id: req(input, "id"),
            created_at: req(input, "created_at"),
            updated_at: req(input, "updated_at"),
            deleted_at: deleted_at.as_deref(),
            instance_name: req(input, "instance_name"),
            whitelist_emails: whitelist_emails.as_deref(),
            instance_id: req(input, "instance_id"),
            current_version: req(input, "current_version"),
            latest_version: latest_version.as_deref(),
            edition: req(input, "edition"),
            domain: req(input, "domain"),
            last_checked_at: req(input, "last_checked_at"),
            namespace: namespace.as_deref(),
            is_telemetry_enabled: boolean(input, "is_telemetry_enabled"),
            is_support_required: boolean(input, "is_support_required"),
            is_setup_done: boolean(input, "is_setup_done"),
            is_signup_screen_visited: boolean(input, "is_signup_screen_visited"),
            is_verified: boolean(input, "is_verified"),
            is_test: boolean(input, "is_test"),
            is_current_version_deprecated: boolean(input, "is_current_version_deprecated"),
            created_by: created_by.as_deref(),
            updated_by: updated_by.as_deref(),
        };
        let produced = serde_json::to_value(instance_to_representation(&row)).expect("serializes");
        assert_replay(&produced, golden.get("output").expect("output"));
        let map = produced.as_object().expect("object");
        assert!(
            !map.contains_key("email"),
            "no email key: no such model field"
        );
        assert!(
            !map.contains_key("primary_owner_details"),
            "BUG-1: key stays absent"
        );
    }

    #[test]
    fn instance_configuration_plain_case_replays_golden() {
        // Fixture serializers/instance_configuration.golden.json, plain case:
        // not encrypted, so `decrypt` must not be consulted; EMAIL_HOST is
        // db-sourced per config/registry.py.
        let golden = fixture("instance_configuration");
        let case = golden
            .get("cases")
            .and_then(Value::as_array)
            .and_then(|cases| cases.first())
            .expect("one case");
        let input = case.get("input_row").expect("input_row");
        // The case input omits audit columns (they ride the DB row); the
        // executed values survive in the case output.
        let case_output = case.get("output").expect("output");
        let created_by = opt(case_output, "created_by");
        let updated_by = opt(case_output, "updated_by");
        let deleted_at = opt(case_output, "deleted_at");
        let value = opt(input, "value");
        let row = InstanceConfigurationRow {
            id: req(input, "id"),
            created_at: req(case_output, "created_at"),
            updated_at: req(case_output, "updated_at"),
            deleted_at: deleted_at.as_deref(),
            key: req(input, "key"),
            value: value.as_deref(),
            category: req(input, "category"),
            is_encrypted: boolean(input, "is_encrypted"),
            created_by: created_by.as_deref(),
            updated_by: updated_by.as_deref(),
        };
        let decrypt = |_: &str| -> String {
            panic!("decrypt must not run when is_encrypted is false");
        };
        let registry_source = |key: &str| -> Option<&str> {
            assert_eq!(key, "EMAIL_HOST");
            Some("db")
        };
        let produced = serde_json::to_value(instance_configuration_to_representation(
            &row,
            &decrypt,
            &registry_source,
        ))
        .expect("serializes");
        assert_replay(&produced, case.get("output").expect("output"));
    }

    #[test]
    fn instance_configuration_decrypt_branch() {
        // configuration.py:19-20: decrypt runs iff is_encrypted and value is
        // not None. No golden vector covers this; the branch is proved with
        // stub callbacks.
        let row = InstanceConfigurationRow {
            id: "44444444-4444-4444-4444-444444444444",
            created_at: "2026-01-15T12:30:45Z",
            updated_at: "2026-01-15T12:30:45Z",
            deleted_at: None,
            key: "SECRET_KEY",
            value: Some("ciphertext"),
            category: "GENERAL",
            is_encrypted: true,
            created_by: None,
            updated_by: None,
        };
        let decrypt = |stored: &str| -> String {
            assert_eq!(stored, "ciphertext");
            "plaintext".to_owned()
        };
        let unregistered = |_: &str| -> Option<&str> { None };
        let view = instance_configuration_to_representation(&row, &decrypt, &unregistered);
        assert_eq!(view.value.as_deref(), Some("plaintext"));
        assert_eq!(view.source, "db");
        assert!(!view.is_managed);

        // Encrypted but null stays null without touching decrypt
        // (configuration.py:19 guards `value is not None`).
        let null_row = InstanceConfigurationRow { value: None, ..row };
        let never = |_: &str| -> String {
            panic!("decrypt must not run for a null value");
        };
        let view = instance_configuration_to_representation(&null_row, &never, &unregistered);
        assert_eq!(view.value, None);
    }

    #[test]
    fn instance_configuration_env_source_is_managed() {
        // configuration.py:25-28 with a registry entry whose source is env
        // (e.g. GITHUB_APP_NAME per config/registry.py:57).
        let row = InstanceConfigurationRow {
            id: "44444444-4444-4444-4444-444444444444",
            created_at: "2026-01-15T12:30:45Z",
            updated_at: "2026-01-15T12:30:45Z",
            deleted_at: None,
            key: "GITHUB_APP_NAME",
            value: Some("my-app"),
            category: "GITHUB",
            is_encrypted: false,
            created_by: None,
            updated_by: None,
        };
        let never = |_: &str| -> String {
            panic!("decrypt must not run when is_encrypted is false");
        };
        let env_source = |key: &str| -> Option<&str> {
            assert_eq!(key, "GITHUB_APP_NAME");
            Some("env")
        };
        let view = instance_configuration_to_representation(&row, &never, &env_source);
        assert_eq!(view.value.as_deref(), Some("my-app"));
        assert_eq!(view.source, "env");
        assert!(view.is_managed);
    }

    #[test]
    fn instance_admin_replays_golden() {
        // Fixture serializers/instance_admin.golden.json: raw FKs render as
        // string PKs while `user_detail` nests the same user lite.
        let golden = fixture("instance_admin");
        let input = golden.get("input_row").expect("input_row");
        let nested = input.get("user").expect("nested user");
        let avatar_url = opt(nested, "avatar_url");
        let last_login_medium = opt(nested, "last_login_medium");
        let user = LiteUserRow {
            id: req(nested, "id"),
            first_name: req(nested, "first_name"),
            last_name: req(nested, "last_name"),
            avatar: req(nested, "avatar"),
            avatar_url: avatar_url.as_deref(),
            is_bot: nested
                .get("is_bot")
                .and_then(Value::as_bool)
                .expect("is_bot"),
            display_name: req(nested, "display_name"),
            email: req(nested, "email"),
            last_login_medium: last_login_medium.as_deref(),
        };
        let user_id = req(nested, "id").to_owned();
        // The golden input_row omits audit datetimes (they ride the DB row,
        // not the posted body); the executed values survive in the output.
        let output = golden.get("output").expect("output");
        let created_by = opt(output, "created_by");
        let updated_by = opt(output, "updated_by");
        let deleted_at = opt(output, "deleted_at");
        let row = InstanceAdminRow {
            id: req(input, "id"),
            user: Some(user),
            created_at: req(output, "created_at"),
            updated_at: req(output, "updated_at"),
            deleted_at: deleted_at.as_deref(),
            role: input.get("role").and_then(Value::as_i64).expect("role") as i32,
            is_verified: boolean(input, "is_verified"),
            created_by: created_by.as_deref(),
            updated_by: updated_by.as_deref(),
            user_id: Some(&user_id),
            instance_id: req(input, "instance_id"),
        };
        let produced =
            serde_json::to_value(instance_admin_to_representation(&row)).expect("serializes");
        assert_replay(&produced, output);
        // The nested detail is exactly the nine lite fields.
        assert_eq!(
            produced.get("user_detail").expect("user_detail present"),
            output.get("user_detail").expect("golden user_detail"),
            "nested lite-user mismatch"
        );
    }

    #[test]
    fn instance_admin_me_replays_golden() {
        // Fixture serializers/instance_admin_me.golden.json: the golden
        // input_row is a partial projection (only what the fixture author
        // fed in); every output field is a `User` attribute, so the row is
        // rebuilt from the executed output and the replay proves the exact
        // 15-key shape, the collapsed duplicate, and the Meta order.
        let golden = fixture("instance_admin_me");
        let output = golden.get("output").expect("output");
        let avatar_url = opt(output, "avatar_url");
        let cover_image = opt(output, "cover_image");
        let row = AdminMeRow {
            id: req(output, "id"),
            avatar: req(output, "avatar"),
            avatar_url: avatar_url.as_deref(),
            cover_image: cover_image.as_deref(),
            date_joined: req(output, "date_joined"),
            display_name: req(output, "display_name"),
            email: req(output, "email"),
            first_name: req(output, "first_name"),
            last_name: req(output, "last_name"),
            is_active: boolean(output, "is_active"),
            is_bot: boolean(output, "is_bot"),
            is_email_verified: boolean(output, "is_email_verified"),
            user_timezone: req(output, "user_timezone"),
            username: req(output, "username"),
            is_password_autoset: boolean(output, "is_password_autoset"),
        };
        let produced =
            serde_json::to_value(instance_admin_me_to_representation(&row)).expect("serializes");
        assert_replay(&produced, output);
        // The duplicated Meta.fields entry (admin.py:23 and :31) collapses.
        let count = produced
            .as_object()
            .expect("object")
            .keys()
            .filter(|key| key.as_str() == "is_email_verified")
            .count();
        assert_eq!(count, 1, "duplicated field emitted once");
    }
}
