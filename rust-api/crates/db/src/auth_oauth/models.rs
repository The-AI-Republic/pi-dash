//! SocialLoginConnection + CLIDeviceCode table models and code generators (D-17).
//!
//! Translation of the model slice PIDASHCONV-326 owns:
//!
//! * `apps/api/pi_dash/db/models/social_connection.py:1-43`
//!   (`SocialLoginConnection`, class 12-43; `Meta` 33-40; `__str__` 42-43);
//!   pk/audit shape `db/models/base.py:17-21` + `db/mixins.py`
//!   (`TimeAuditModel` / `UserAuditModel` / `SoftDeleteModel`).
//! * `apps/api/pi_dash/db/models/api.py:24-32` (`generate_device_code`,
//!   `generate_user_code`) and `:63-94` (`CLIDeviceCode`).
//! * `apps/api/pi_dash/db/models/api.py:35-60` (`APIToken`) is a
//!   read-only reference: only the columns the device flow touches are
//!   pinned ([`api_token_device_flow`]), so the queries layer
//!   (PIDASHCONV-327) builds against recorded names. The full `APIToken`
//!   port belongs to whichever later issue owns it.
//!
//! Fixture source of truth: `rust-api/fixtures/auth_oauth/`
//! `F1_social_login_connection.columns.json` (AUTHOAUTH-F1) and
//! `F2_cli_device_code.columns.json` (AUTHOAUTH-F2), recorded by
//! PIDASHCONV-324; the `#[cfg(test)]` suite asserts these consts equal
//! the fixture column lists field-for-field.
//!
//! # Column order
//!
//! Each `COLUMNS` const follows Django `_meta` field order (the order
//! recorded in the fixtures): the audit prefix (`id`, `created_at`,
//! `updated_at`, `created_by_id`, `updated_by_id`, `deleted_at`), then the
//! declared fields in source order with Django attnames (`user` ->
//! `user_id`, `workspace` -> `workspace_id`). Order is cosmetic for query
//! building; membership is the contract.
//!
//! # Django-level defaults
//!
//! Every column default below is application-level (Django); the live
//! tables carry no `column_default`, so Rust inserts must supply these
//! values explicitly — there is no DB fallback.
//!
//! # Ported quirks (translate, don't redesign)
//!
//! * `medium` declares `default=None` with no `null=True`
//!   (`social_connection.py:13-22`): the column is `NOT NULL` and every
//!   insert must supply a value. Ported as a non-optional `String` with
//!   [`social_login_connection::MEDIUM_HAS_DEFAULT`] set to `false`.
//! * `medium` choices store the *first* element (`"Google"`, `"Github"`,
//!   `"GitLab"`, `"Jira"`); the lowercase second element is display-only.
//!   [`social_login_connection::MEDIUM_CHOICES`] keeps that order.
//! * `CLIDeviceCode.user` and `.workspace` are nullable `CASCADE` FKs
//!   (`api.py:77-80`): the row exists before approval (no user yet) and a
//!   device code may be workspace-scoped or global.

/// Django-level FK delete behavior (ORM-emulated; the live FKs may show
/// `NO ACTION`, so Rust write paths replicate the nulling/cascading
/// explicitly).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OnDelete {
    /// `models.SET_NULL` — FK column must be nullable.
    SetNull,
    /// `models.CASCADE` — dependent rows are deleted with the parent.
    Cascade,
}

/// `social_login_connections` table (`social_connection.py:12-43`).
pub mod social_login_connection {
    use super::OnDelete;
    use serde::{Deserialize, Serialize};

    /// Django table name (`Meta.db_table`, `:36`).
    pub const TABLE: &str = "social_login_connections";

    /// Default `ORDER BY` (`Meta.ordering = ("-created_at",)`, `:37`).
    pub const ORDERING: &str = "-created_at";

    /// Verbose names (`Meta.verbose_name[_plural]`, `:34-35`).
    pub const VERBOSE_NAME: &str = "Social Login Connection";
    pub const VERBOSE_NAME_PLURAL: &str = "Social Login Connections";

    /// Columns in Django `_meta` field order (matches AUTHOAUTH-F1).
    /// FK columns use the Django attnames (`created_by_id`,
    /// `updated_by_id`, `user_id`).
    pub const COLUMNS: &[&str] = &[
        "id",
        "created_at",
        "updated_at",
        "created_by_id",
        "updated_by_id",
        "deleted_at",
        "medium",
        "last_login_at",
        "last_received_at",
        "user_id",
        "token_data",
        "extra_data",
    ];

    /// `medium` choices (`:14-21`): the first element is the stored value,
    /// the second is display-only.
    pub const MEDIUM_CHOICES: &[(&str, &str)] = &[
        ("Google", "google"),
        ("Github", "github"),
        ("GitLab", "gitlab"),
        ("Jira", "jira"),
    ];

    /// `medium` `max_length` (`:13`).
    pub const MEDIUM_MAX_LENGTH: usize = 20;

    /// `medium` declares `default=None` without `null=True`: `NOT NULL`
    /// with no usable default — inserts must supply a value.
    pub const MEDIUM_HAS_DEFAULT: bool = false;
    pub const MEDIUM_NULLABLE: bool = false;

    /// `last_login_at` / `last_received_at` (`:23-24`): nullable
    /// `DateTimeField`s whose application default is `timezone.now`.
    pub const LAST_LOGIN_AT_NULLABLE: bool = true;
    pub const LAST_RECEIVED_AT_NULLABLE: bool = true;

    /// `user` FK (`:25-29`): required `CASCADE` to the auth user model,
    /// `related_name="user_login_connections"`.
    pub const USER_ON_DELETE: OnDelete = OnDelete::Cascade;
    pub const USER_NULLABLE: bool = false;
    pub const USER_RELATED_NAME: &str = "user_login_connections";

    /// Audit FK delete behavior (`db/mixins.py:26-38`, `SET_NULL`).
    pub const CREATED_BY_ON_DELETE: OnDelete = OnDelete::SetNull;
    pub const UPDATED_BY_ON_DELETE: OnDelete = OnDelete::SetNull;

    /// `token_data` / `extra_data` (`:30-31`): nullable `JSONField`s.
    pub const TOKEN_DATA_NULLABLE: bool = true;
    pub const EXTRA_DATA_NULLABLE: bool = true;

    /// `__str__` (`:42-43`): `f"{self.medium} <{self.user.email}>"`.
    pub fn display_string(medium: &str, user_email: &str) -> String {
        format!("{medium} <{user_email}>")
    }

    /// One `SocialLoginConnection` row. Timestamps are `timestamptz`;
    /// `token_data` / `extra_data` keep `NULL` and JSON `null` as distinct
    /// stored states via `Option<Value>`.
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    pub struct SocialLoginConnection {
        pub id: uuid::Uuid,
        pub created_at: chrono::DateTime<chrono::Utc>,
        pub updated_at: chrono::DateTime<chrono::Utc>,
        pub created_by_id: Option<uuid::Uuid>,
        pub updated_by_id: Option<uuid::Uuid>,
        pub deleted_at: Option<chrono::DateTime<chrono::Utc>>,
        pub medium: String,
        pub last_login_at: Option<chrono::DateTime<chrono::Utc>>,
        pub last_received_at: Option<chrono::DateTime<chrono::Utc>>,
        pub user_id: uuid::Uuid,
        pub token_data: Option<serde_json::Value>,
        pub extra_data: Option<serde_json::Value>,
    }
}

/// `cli_device_codes` table + generators (`api.py:24-32,63-94`).
pub mod cli_device_code {
    use super::OnDelete;
    use serde::{Deserialize, Serialize};

    /// Django table name (`Meta.db_table`, `:90`).
    pub const TABLE: &str = "cli_device_codes";

    /// Default `ORDER BY` (`Meta.ordering = ("-created_at",)`, `:91`).
    pub const ORDERING: &str = "-created_at";

    /// Verbose names (`Meta.verbose_name[_plural]`, `:88-89`).
    pub const VERBOSE_NAME: &str = "CLI Device Code";
    pub const VERBOSE_NAME_PLURAL: &str = "CLI Device Codes";

    /// Columns in Django `_meta` field order (matches AUTHOAUTH-F2
    /// `cli_device_code_columns`).
    pub const COLUMNS: &[&str] = &[
        "id",
        "created_at",
        "updated_at",
        "created_by_id",
        "updated_by_id",
        "deleted_at",
        "device_code",
        "user_code",
        "user_id",
        "workspace_id",
        "approved",
        "denied",
        "consumed",
        "expires_at",
        "last_polled_at",
    ];

    /// `device_code` (`:75`): opaque code, unique + indexed,
    /// `max_length=64`, application default [`generate_device_code`].
    pub const DEVICE_CODE_MAX_LENGTH: usize = 64;
    /// `user_code` (`:76`): short human code, unique + indexed,
    /// `max_length=16`, application default [`generate_user_code`].
    pub const USER_CODE_MAX_LENGTH: usize = 16;
    /// Column-level uniques (`:75-76`, enforced soft-delete-unaware).
    pub const UNIQUE_COLUMNS: &[&str] = &["device_code", "user_code"];

    /// `user` FK (`:77`): nullable `CASCADE`, `related_name="device_codes"`.
    pub const USER_ON_DELETE: OnDelete = OnDelete::Cascade;
    pub const USER_NULLABLE: bool = true;
    pub const USER_RELATED_NAME: &str = "device_codes";

    /// `workspace` FK (`:78-80`): nullable + blank `CASCADE`,
    /// `related_name="device_codes"`.
    pub const WORKSPACE_ON_DELETE: OnDelete = OnDelete::Cascade;
    pub const WORKSPACE_NULLABLE: bool = true;
    pub const WORKSPACE_BLANK: bool = true;
    pub const WORKSPACE_RELATED_NAME: &str = "device_codes";

    /// Lifecycle flags (`:81-83`), all defaulting to `false`.
    pub const DEFAULT_APPROVED: bool = false;
    pub const DEFAULT_DENIED: bool = false;
    pub const DEFAULT_CONSUMED: bool = false;

    /// `expires_at` (`:84`) is required; `last_polled_at` (`:85`) is
    /// nullable + blank.
    pub const EXPIRES_AT_NULLABLE: bool = false;
    pub const LAST_POLLED_AT_NULLABLE: bool = true;
    pub const LAST_POLLED_AT_BLANK: bool = true;

    /// Audit FK delete behavior (`db/mixins.py:26-38`, `SET_NULL`).
    pub const CREATED_BY_ON_DELETE: OnDelete = OnDelete::SetNull;
    pub const UPDATED_BY_ON_DELETE: OnDelete = OnDelete::SetNull;

    // ------------------------------------------------------------------
    // Generators (`api.py:24-32`)
    // ------------------------------------------------------------------

    /// Input bytes for [`generate_device_code`]: `secrets.token_urlsafe(32)`
    /// reads 32 random bytes (`:24-25`).
    pub const DEVICE_CODE_RANDOM_BYTES: usize = 32;

    /// Emitted length of [`generate_device_code`]: 32 bytes base64url
    /// without padding is `ceil(32/3)*4 - 1 = 43` chars (verified against
    /// CPython `secrets.token_urlsafe(32)` 2026-09-29, recorded in
    /// AUTHOAUTH-F2).
    pub const DEVICE_CODE_EMITTED_LEN: usize = 43;

    /// Ambiguity-free alphabet for [`generate_user_code`] (`:29-30`):
    /// consonants + digits without `AEIOU01`, 28 chars.
    pub const USER_CODE_ALPHABET: &str = "BCDFGHJKLMNPQRSTVWXZ23456789";

    /// Length of [`USER_CODE_ALPHABET`].
    pub const USER_CODE_ALPHABET_LEN: usize = 28;

    /// Raw draws before hyphenation (`:31`).
    pub const USER_CODE_RAW_LEN: usize = 8;

    /// `generate_device_code` (`api.py:24-25`):
    /// `secrets.token_urlsafe(32)` — base64url (no padding) over 32
    /// random bytes, always [`DEVICE_CODE_EMITTED_LEN`] chars.
    pub fn generate_device_code() -> String {
        let mut rng = rand::thread_rng();
        generate_device_code_with_rng(&mut rng)
    }

    /// [`generate_device_code`] with an injected RNG (same bytes-to-text
    /// mapping; the `#[cfg(test)]` goldens drive this with a seeded RNG).
    pub fn generate_device_code_with_rng<R: rand::RngCore>(rng: &mut R) -> String {
        let mut bytes = [0u8; DEVICE_CODE_RANDOM_BYTES];
        rng.fill_bytes(&mut bytes);
        use base64::Engine as _;
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
    }

    /// `generate_user_code` (`api.py:28-32`): 8 uniform draws from
    /// [`USER_CODE_ALPHABET`], hyphenated `XXXX-XXXX` (`:32`).
    pub fn generate_user_code() -> String {
        let mut rng = rand::thread_rng();
        generate_user_code_with_rng(&mut rng)
    }

    /// [`generate_user_code`] with an injected RNG (same draw/order
    /// mapping; the `#[cfg(test)]` goldens drive this with a seeded RNG).
    pub fn generate_user_code_with_rng<R: rand::Rng>(rng: &mut R) -> String {
        let alphabet = USER_CODE_ALPHABET.as_bytes();
        let raw: Vec<u8> = (0..USER_CODE_RAW_LEN)
            .map(|_| alphabet[rng.gen_range(0..USER_CODE_ALPHABET_LEN)])
            .collect();
        let (head, tail) = raw.split_at(4);
        format!(
            "{}-{}",
            std::str::from_utf8(head).expect("alphabet is ASCII"),
            std::str::from_utf8(tail).expect("alphabet is ASCII")
        )
    }

    /// Check the `XXXX-XXXX` hyphen form over [`USER_CODE_ALPHABET`]
    /// (AUTHOAUTH-F2 `output_pattern`).
    pub fn user_code_is_wellformed(code: &str) -> bool {
        let bytes = code.as_bytes();
        if bytes.len() != USER_CODE_RAW_LEN + 1 || bytes[4] != b'-' {
            return false;
        }
        bytes
            .iter()
            .enumerate()
            .all(|(i, b)| i == 4 || USER_CODE_ALPHABET.as_bytes().contains(b))
    }

    /// One `CLIDeviceCode` row. `user_id` / `workspace_id` are `None`
    /// until approval stamps them (`device.py` approve path).
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    pub struct CLIDeviceCode {
        pub id: uuid::Uuid,
        pub created_at: chrono::DateTime<chrono::Utc>,
        pub updated_at: chrono::DateTime<chrono::Utc>,
        pub created_by_id: Option<uuid::Uuid>,
        pub updated_by_id: Option<uuid::Uuid>,
        pub deleted_at: Option<chrono::DateTime<chrono::Utc>>,
        pub device_code: String,
        pub user_code: String,
        pub user_id: Option<uuid::Uuid>,
        pub workspace_id: Option<uuid::Uuid>,
        pub approved: bool,
        pub denied: bool,
        pub consumed: bool,
        pub expires_at: chrono::DateTime<chrono::Utc>,
        pub last_polled_at: Option<chrono::DateTime<chrono::Utc>>,
    }

    impl std::fmt::Display for CLIDeviceCode {
        /// `__str__` (`api.py:93-94`): `return self.user_code`.
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str(&self.user_code)
        }
    }
}

/// `APIToken` device-flow reference (`api.py:35-60`, read-only).
///
/// Only the columns the device flow touches (AUTHOAUTH-F2
/// `api_token_fields_used_by_device_flow`): the mint in
/// `authentication/views/cli/device.py:343-349` and the deactivate filter
/// in `authentication/services/cli_tokens.py:1-19` (owned by
/// PIDASHCONV-327). This is not the full `APIToken` port.
pub mod api_token_device_flow {
    /// Django table name (`Meta.db_table`, `api.py:55`).
    pub const TABLE: &str = "api_tokens";

    /// Default `ORDER BY` (`Meta.ordering`, `api.py:56`).
    pub const ORDERING: &str = "-created_at";

    /// Columns the device flow reads/writes (AUTHOAUTH-F2
    /// `api_token_fields_used_by_device_flow.columns`), in fixture order
    /// with Django attnames.
    pub const COLUMNS: &[&str] = &[
        "label",
        "description",
        "is_active",
        "last_used",
        "token",
        "user_id",
        "user_type",
        "workspace_id",
        "expired_at",
        "is_service",
        "allowed_rate_limit",
    ];

    /// `user_type` choices (`api.py:47`).
    pub const USER_TYPE_CHOICES: &[(i32, &str)] = &[(0, "Human"), (1, "Bot")];

    /// Defaults the device mint relies on (`api.py:37,39,47,50-51`).
    pub const DEFAULT_IS_ACTIVE: bool = true;
    pub const DEFAULT_USER_TYPE: i32 = 0;
    pub const DEFAULT_IS_SERVICE: bool = false;
    pub const DEFAULT_ALLOWED_RATE_LIMIT: &str = "60/min";

    /// Token description stamped by the device mint
    /// (`authentication/services/cli_tokens.py:9`,
    /// `authentication/views/cli/device.py:363`).
    pub const DEVICE_FLOW_DESCRIPTION: &str = "Issued by pidash auth login (device-code flow).";

    /// Token label stamped by the device mint
    /// (`device.py:362`): `f"pidash CLI · {now:%Y-%m-%d %H:%M} UTC"`.
    pub fn device_flow_label(now: &chrono::DateTime<chrono::Utc>) -> String {
        format!("pidash CLI · {} UTC", now.format("%Y-%m-%d %H:%M"))
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_support as ts;
    use super::*;

    fn fixture_f1() -> serde_json::Value {
        ts::fixture_file("F1_social_login_connection.columns.json")
    }

    fn fixture_f2() -> serde_json::Value {
        ts::fixture_file("F2_cli_device_code.columns.json")
    }

    // -- AUTHOAUTH-F1 -------------------------------------------------------

    #[test]
    fn social_login_connection_columns_match_fixture() {
        let f1 = fixture_f1();
        assert_eq!(f1["model"].as_str(), Some("SocialLoginConnection"));
        assert_eq!(
            f1["meta"]["db_table"].as_str(),
            Some(social_login_connection::TABLE)
        );
        assert_eq!(
            f1["meta"]["ordering"]
                .as_array()
                .expect("F1 meta.ordering is an array")
                .iter()
                .map(|v| v.as_str().expect("ordering entry is a string"))
                .collect::<Vec<_>>(),
            vec![social_login_connection::ORDERING]
        );
        assert_eq!(
            ts::owned_columns(social_login_connection::COLUMNS),
            ts::fixture_columns(&f1, ""),
            "F1 columns byte-exact in Django field order",
        );
    }

    #[test]
    fn social_login_connection_field_semantics_match_fixture() {
        let f1 = fixture_f1();
        let by_name = |name: &str| {
            f1["columns"]
                .as_array()
                .expect("F1 has columns")
                .iter()
                .find(|c| c["name"] == name)
                .unwrap_or_else(|| panic!("F1 has column {name}"))
                .clone()
        };
        let medium = by_name("medium");
        assert_eq!(medium["max_length"], 20);
        assert_eq!(
            medium["choices"],
            serde_json::json!([
                ["Google", "google"],
                ["Github", "github"],
                ["GitLab", "gitlab"],
                ["Jira", "jira"]
            ])
        );
        assert_eq!(
            social_login_connection::MEDIUM_CHOICES
                .iter()
                .map(|(stored, display)| serde_json::json!([stored, display]))
                .collect::<Vec<_>>(),
            serde_json::from_value::<Vec<serde_json::Value>>(medium["choices"].clone())
                .expect("choices parse"),
            "stored value is the first element, as Django persists it",
        );
        assert_eq!(
            social_login_connection::MEDIUM_MAX_LENGTH,
            medium["max_length"].as_u64().expect("max_length") as usize
        );
        // `default=None` without `null=True`: NOT NULL, must be supplied.
        let has_default: bool = social_login_connection::MEDIUM_HAS_DEFAULT;
        assert!(!has_default);
        let medium_nullable: bool = social_login_connection::MEDIUM_NULLABLE;
        assert!(!medium_nullable);
        assert_eq!(by_name("user")["on_delete"], "CASCADE");
        assert_eq!(by_name("user")["related_name"], "user_login_connections");
        let user_nullable: bool = social_login_connection::USER_NULLABLE;
        assert!(!user_nullable);
        assert_eq!(
            social_login_connection::USER_RELATED_NAME,
            by_name("user")["related_name"]
                .as_str()
                .expect("related_name")
        );
        assert_eq!(social_login_connection::USER_ON_DELETE, OnDelete::Cascade);
        assert_eq!(
            social_login_connection::display_string("Google", "a@x.test"),
            "Google <a@x.test>"
        );
        assert_eq!(
            f1["dunder_str"]["expr"].as_str(),
            Some("f\"{self.medium} <{self.user.email}>\"")
        );
    }

    // -- AUTHOAUTH-F2: columns ------------------------------------------------

    #[test]
    fn cli_device_code_columns_match_fixture() {
        let f2 = fixture_f2();
        let section = &f2["cli_device_code_columns"];
        assert_eq!(
            section["meta"]["db_table"].as_str(),
            Some(cli_device_code::TABLE)
        );
        assert_eq!(
            section["meta"]["ordering"]
                .as_array()
                .expect("F2 meta.ordering is an array")
                .iter()
                .map(|v| v.as_str().expect("ordering entry is a string"))
                .collect::<Vec<_>>(),
            vec![cli_device_code::ORDERING]
        );
        assert_eq!(
            ts::owned_columns(cli_device_code::COLUMNS),
            ts::fixture_columns(&f2, "cli_device_code_columns"),
            "F2 device-code columns byte-exact in Django field order",
        );
    }

    #[test]
    fn cli_device_code_field_semantics_match_fixture() {
        let f2 = fixture_f2();
        let cols = &f2["cli_device_code_columns"]["columns"];
        let by_name = |name: &str| {
            cols.as_array()
                .expect("F2 device columns")
                .iter()
                .find(|c| c["name"] == name)
                .unwrap_or_else(|| panic!("F2 has column {name}"))
                .clone()
        };
        assert_eq!(by_name("device_code")["max_length"], 64);
        assert_eq!(by_name("user_code")["max_length"], 16);
        assert_eq!(
            cli_device_code::DEVICE_CODE_MAX_LENGTH,
            by_name("device_code")["max_length"].as_u64().expect("max") as usize
        );
        assert_eq!(
            cli_device_code::USER_CODE_MAX_LENGTH,
            by_name("user_code")["max_length"].as_u64().expect("max") as usize
        );
        assert_eq!(by_name("user")["related_name"], "device_codes");
        assert_eq!(by_name("workspace")["related_name"], "device_codes");
        assert_eq!(cli_device_code::USER_RELATED_NAME, "device_codes");
        assert_eq!(cli_device_code::WORKSPACE_RELATED_NAME, "device_codes");
        let user_nullable: bool = cli_device_code::USER_NULLABLE;
        assert!(user_nullable);
        let workspace_nullable: bool = cli_device_code::WORKSPACE_NULLABLE;
        assert!(workspace_nullable);
        let approved: bool = cli_device_code::DEFAULT_APPROVED;
        assert!(!approved);
        let denied: bool = cli_device_code::DEFAULT_DENIED;
        assert!(!denied);
        let consumed: bool = cli_device_code::DEFAULT_CONSUMED;
        assert!(!consumed);
        let expires_nullable: bool = cli_device_code::EXPIRES_AT_NULLABLE;
        assert!(!expires_nullable);
        let polled_nullable: bool = cli_device_code::LAST_POLLED_AT_NULLABLE;
        assert!(polled_nullable);
        // `__str__` returns `self.user_code`.
        assert_eq!(
            f2["cli_device_code_columns"]["dunder_str"]["expr"].as_str(),
            Some("return self.user_code")
        );
        for code in cli_device_code::UNIQUE_COLUMNS {
            assert!(
                cli_device_code::COLUMNS.contains(code),
                "unique column {code} is a known column"
            );
        }
    }

    #[test]
    fn api_token_device_flow_reference_matches_fixture() {
        let f2 = fixture_f2();
        let section = &f2["api_token_fields_used_by_device_flow"];
        assert_eq!(
            section["meta"]["db_table"].as_str(),
            Some(api_token_device_flow::TABLE)
        );
        assert_eq!(
            section["meta"]["ordering"]
                .as_array()
                .expect("api_token meta.ordering is an array")
                .iter()
                .map(|v| v.as_str().expect("ordering entry is a string"))
                .collect::<Vec<_>>(),
            vec![api_token_device_flow::ORDERING]
        );
        assert_eq!(
            ts::owned_columns(api_token_device_flow::COLUMNS),
            ts::fixture_columns(&f2, "api_token_fields_used_by_device_flow"),
            "APIToken device-flow reference subset byte-exact",
        );
        assert_eq!(api_token_device_flow::DEFAULT_USER_TYPE, 0);
        let is_active: bool = api_token_device_flow::DEFAULT_IS_ACTIVE;
        assert!(is_active);
        let is_service: bool = api_token_device_flow::DEFAULT_IS_SERVICE;
        assert!(!is_service);
        assert_eq!(api_token_device_flow::DEFAULT_ALLOWED_RATE_LIMIT, "60/min");
        // The fixture pins the constant *name* (`device.py:363` passes the
        // `CLI_DEVICE_API_TOKEN_DESCRIPTION` import); the value below is
        // the constant itself (`cli_tokens.py:9`).
        assert_eq!(
            section["device_flow_create"]["description"]
                .as_str()
                .expect("description"),
            "CLI_DEVICE_API_TOKEN_DESCRIPTION"
        );
        assert_eq!(
            api_token_device_flow::DEVICE_FLOW_DESCRIPTION,
            "Issued by pidash auth login (device-code flow)."
        );
    }

    // -- AUTHOAUTH-F2: generators ----------------------------------------------

    #[test]
    fn generator_goldens_match_fixture() {
        let f2 = fixture_f2();
        let gens = &f2["generators"];
        assert_eq!(
            gens["generate_device_code"]["code"].as_str(),
            Some("secrets.token_urlsafe(32)")
        );
        assert_eq!(
            gens["generate_device_code"]["emitted_length_chars"]
                .as_u64()
                .expect("emitted length") as usize,
            cli_device_code::DEVICE_CODE_EMITTED_LEN
        );
        assert_eq!(
            gens["generate_user_code"]["alphabet"].as_str(),
            Some(cli_device_code::USER_CODE_ALPHABET)
        );
        assert_eq!(
            gens["generate_user_code"]["alphabet_len"]
                .as_u64()
                .expect("alphabet len") as usize,
            cli_device_code::USER_CODE_ALPHABET_LEN
        );
        assert_eq!(cli_device_code::USER_CODE_ALPHABET.len(), 28);
        // 28-char alphabet excludes the ambiguous AEIOU01.
        for ambiguous in ['A', 'E', 'I', 'O', 'U', '0', '1'] {
            assert!(
                !cli_device_code::USER_CODE_ALPHABET.contains(ambiguous),
                "alphabet excludes {ambiguous}"
            );
        }
    }

    #[test]
    fn device_code_matches_cpython_vectors() {
        // Fixed vectors from CPython
        // `base64.urlsafe_b64encode(<bytes>).rstrip(b"=")` (2026-09-29):
        // the Rust mapping must render the same text for the same bytes.
        struct Fixed([u8; 32]);
        impl rand::RngCore for Fixed {
            fn next_u32(&mut self) -> u32 {
                unimplemented!()
            }
            fn next_u64(&mut self) -> u64 {
                unimplemented!()
            }
            fn fill_bytes(&mut self, dest: &mut [u8]) {
                dest.copy_from_slice(&self.0[..dest.len()]);
            }
            fn try_fill_bytes(&mut self, dest: &mut [u8]) -> Result<(), rand::Error> {
                self.fill_bytes(dest);
                Ok(())
            }
        }
        let mut seq: Fixed = Fixed([0u8; 32]);
        for (i, b) in seq.0.iter_mut().enumerate() {
            *b = i as u8;
        }
        assert_eq!(
            cli_device_code::generate_device_code_with_rng(&mut seq),
            "AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8"
        );
        let mut zeros = Fixed([0u8; 32]);
        assert_eq!(
            cli_device_code::generate_device_code_with_rng(&mut zeros),
            "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"
        );
    }

    #[test]
    fn device_code_is_43_urlsafe_chars() {
        use rand::SeedableRng as _;
        let mut rng = rand::rngs::StdRng::seed_from_u64(0xD17C0DE);
        for _ in 0..64 {
            let code = cli_device_code::generate_device_code_with_rng(&mut rng);
            assert_eq!(code.len(), cli_device_code::DEVICE_CODE_EMITTED_LEN);
            assert!(
                code.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_'),
                "urlsafe alphabet, no padding: {code}"
            );
            assert!(!code.contains('='));
        }
        // Live RNG path emits the same shape.
        let live = cli_device_code::generate_device_code();
        assert_eq!(live.len(), cli_device_code::DEVICE_CODE_EMITTED_LEN);
    }

    #[test]
    fn user_code_is_hyphenated_alphabet_form() {
        use rand::SeedableRng as _;
        let mut rng = rand::rngs::StdRng::seed_from_u64(0xC11C0DE);
        for _ in 0..256 {
            let code = cli_device_code::generate_user_code_with_rng(&mut rng);
            assert_eq!(code.len(), 9, "XXXX-XXXX is 9 chars: {code}");
            assert_eq!(&code[4..5], "-");
            assert!(
                cli_device_code::user_code_is_wellformed(&code),
                "wellformed: {code}"
            );
        }
        assert!(!cli_device_code::user_code_is_wellformed("ABCD1234"));
        assert!(!cli_device_code::user_code_is_wellformed("ABCD-123"));
        assert!(!cli_device_code::user_code_is_wellformed("ABCE-1234"));
        assert!(!cli_device_code::user_code_is_wellformed("abcd-1234"));
        // Live RNG path emits the same shape.
        assert!(cli_device_code::user_code_is_wellformed(
            &cli_device_code::generate_user_code()
        ));
    }

    #[test]
    fn device_flow_label_matches_python_strftime() {
        let now = chrono::DateTime::parse_from_rfc3339("2026-09-29T07:05:00Z")
            .expect("parse")
            .with_timezone(&chrono::Utc);
        assert_eq!(
            api_token_device_flow::device_flow_label(&now),
            "pidash CLI · 2026-09-29 07:05 UTC"
        );
    }
}
