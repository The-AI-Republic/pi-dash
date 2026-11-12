//! Asset serializer validation kernels (D-21 serializers, PIDASHCONV-392).
//!
//! Ports `apps/api/pi_dash/api/serializers/asset.py:13-91`:
//!
//! * `UserAssetUploadSerializer` (`:13-43`) → [`validate_user_asset_upload`]
//! * `AssetUpdateSerializer` (`:45-54`) → [`validate_asset_update`]
//! * `GenericAssetUploadSerializer` (`:56-80`) → [`validate_generic_asset_upload`]
//! * `GenericAssetUpdateSerializer` (`:82-91`) → [`validate_generic_asset_update`]
//!
//! Fixture: `rust-api/fixtures/v1_assets/fx-ser-asset.json` (`fx-ser-asset`).
//!
//! Each entry point mirrors DRF `Serializer.is_valid()` over an already-parsed
//! JSON object (the Rust stack serves JSON bodies only): `Ok` is the
//! `validated_data` object, `Err` is the `serializer.errors` object, both with
//! DRF key order (declaration order for data, field order for errors; every
//! field is validated even when an earlier one fails). Unknown input keys are
//! ignored, exactly like `to_internal_value`. Field semantics below are DRF
//! 3.15.2 (`rest_framework/fields.py`), verified by executing the real
//! serializers under `pi_dash.settings.test`.
//!
//! DRF error envelopes reproduced here:
//!
//! * field failure → `{"<field>": ["<message>"]}` (one list per field);
//! * missing required field → `"This field is required."`;
//! * explicit `null` with `allow_null=False` → `"This field may not be null."`;
//! * `ChoiceField` rejection → `"\"<input>\" is not a valid choice."`.
//!
//! Ported bugs / divergences (translate, don't redesign; listed for the PR):
//!
//! * BUG-views-bypass: no view instantiates these serializers (user-asset
//!   `views/asset.py:110-220`, server-asset `:282-376`, generic `:497-619`
//!   read `request.data` directly). They are request-shape documentation only;
//!   the handler goldens (`fx-h-asset-*.json`) own runtime behavior.
//! * DIVERGENCE-attributes-string: `fx-ser-asset` records
//!   `{"attributes": "[1,2]"}` → `{"attributes": [1, 2]}`. Live DRF 3.15.2
//!   only JSON-parses strings marked `is_json_string` (HTML-form input,
//!   `fields.py:1769`); over parsed JSON the string passes through unchanged,
//!   so this kernel returns `"[1,2]"`. Filed as a fixture correction.
//!
//! Intentional approximations, all outside any golden or handler path:
//!
//! * `IntegerField`: Python `int()` accepts non-ASCII digits; this kernel
//!   accepts ASCII only. Values beyond `i64` range are rejected (Python ints
//!   are unbounded); `size` is a byte count.
//! * `ChoiceField` rejection input renders Python-style for scalars (`True` /
//!   `5`); containers fall back to compact JSON rendering.
//! * `CharField` float rendering uses `serde_json` shortest-roundtrip
//!   (`1e22`, Python spells `1e+22`); only the stringified value differs.

use serde_json::{Map, Value};

/// `UserAssetUploadSerializer.type` choices (`serializers/asset.py:23-31`).
pub const USER_ASSET_TYPES: &[&str] = &[
    "image/jpeg",
    "image/png",
    "image/webp",
    "image/jpg",
    "image/gif",
];

/// `UserAssetUploadSerializer.type` default (`serializers/asset.py:31`).
pub const DEFAULT_USER_ASSET_TYPE: &str = "image/jpeg";

/// `UserAssetUploadSerializer.entity_type` choices, from
/// `FileAsset.EntityTypeContext` (`db/models/asset.py:38-39`, used at
/// `serializers/asset.py:36-42`).
pub const USER_ENTITY_TYPES: &[&str] = &["USER_AVATAR", "USER_COVER"];

/// DRF `required` failure (`fields.py`, `Field.validate_empty_values`).
pub const MSG_REQUIRED: &str = "This field is required.";
/// DRF `null` failure (`allow_null=False`).
pub const MSG_NULL: &str = "This field may not be null.";
/// DRF `blank` failure (`CharField`, `allow_blank=False`).
pub const MSG_BLANK: &str = "This field may not be blank.";
/// DRF `invalid` failure (`CharField`, non-string input).
pub const MSG_INVALID_STRING: &str = "Not a valid string.";
/// DRF `invalid` failure (`IntegerField`).
pub const MSG_INVALID_INT: &str = "A valid integer is required.";
/// DRF `max_string_length` failure (`IntegerField`, >1000-char strings).
pub const MSG_STRING_TOO_LARGE: &str = "String value too large.";
/// DRF `invalid` failure (`BooleanField`).
pub const MSG_INVALID_BOOL: &str = "Must be a valid boolean.";
/// DRF `invalid` failure (`UUIDField`).
pub const MSG_INVALID_UUID: &str = "Must be a valid UUID.";
/// DRF `ProhibitNullCharactersValidator` message (runs on every `CharField`).
pub const MSG_NULL_CHARS: &str = "Null characters are not allowed.";

/// DRF `invalid_choice` template (`ChoiceField`, `fields.py`).
pub fn invalid_choice_msg(input_repr: &str) -> String {
    format!("\"{input_repr}\" is not a valid choice.")
}

/// Render a JSON scalar the way Python `str(data)` would, for `ChoiceField`
/// `invalid_choice` messages (`ChoiceField.to_internal_value` formats the raw
/// input). Containers use compact JSON (documented approximation).
fn python_scalar_repr(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        Value::Bool(true) => "True".to_owned(),
        Value::Bool(false) => "False".to_owned(),
        Value::Number(n) => n.to_string(),
        Value::Null => "None".to_owned(),
        Value::Array(_) | Value::Object(_) => value.to_string(),
    }
}

/// `CharField` (`fields.py`): blank check on the raw value, bool/composite
/// rejection, numeric coercion via `str()`, whitespace trim, then the null
/// character validator. `None` (absent key) skips; `Null` fails `null`.
/// Surrogate validation is unneeded: `serde_json` rejects lone surrogates.
fn char_field(value: Option<&Value>) -> Result<Option<String>, String> {
    let raw = match value {
        None => return Ok(None),
        Some(Value::Null) => return Err(MSG_NULL.to_owned()),
        Some(v) => v,
    };
    let text = match raw {
        Value::String(s) => s.clone(),
        Value::Bool(_) | Value::Array(_) | Value::Object(_) => {
            return Err(MSG_INVALID_STRING.to_owned());
        }
        Value::Number(n) => n.to_string(),
        Value::Null => unreachable!("handled above"),
    };
    if text.is_empty() || text.trim().is_empty() {
        return Err(MSG_BLANK.to_owned());
    }
    let trimmed = text.trim().to_owned();
    if trimmed.contains('\0') {
        return Err(MSG_NULL_CHARS.to_owned());
    }
    Ok(Some(trimmed))
}

/// `ChoiceField` (`fields.py`): lookup over `str(data)`; `Null` fails `null`
/// before lookup. Returns the matched choice string.
fn choice_field(value: Option<&Value>, choices: &[&str]) -> Result<Option<String>, String> {
    let raw = match value {
        None => return Ok(None),
        Some(Value::Null) => return Err(MSG_NULL.to_owned()),
        Some(v) => v,
    };
    let key = python_scalar_repr(raw);
    match choices.iter().find(|c| **c == key) {
        Some(matched) => Ok(Some((*matched).to_owned())),
        None => Err(invalid_choice_msg(&key)),
    }
}

/// Parse like Python `int(text, 10)` after DRF's `re_decimal` (`\.0*\s*$`)
/// strip: surrounding whitespace, optional sign, ASCII digits with single
/// underscores between digits (DRF accepts `'1_0'` → `10`, verified live).
fn parse_python_int(text: &str) -> Option<i64> {
    let mut core = text.trim();
    if let Some(dot) = core.find('.') {
        if !core[dot + 1..].chars().all(|c| c == '0') {
            return None;
        }
        core = &core[..dot];
    }
    let (negative, digits) = match core.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, core.strip_prefix('+').unwrap_or(core)),
    };
    if digits.is_empty() {
        return None;
    }
    let mut cleaned = String::with_capacity(digits.len());
    let mut prev_underscore = true;
    for c in digits.chars() {
        if c == '_' {
            if prev_underscore {
                return None;
            }
            prev_underscore = true;
        } else if c.is_ascii_digit() {
            cleaned.push(c);
            prev_underscore = false;
        } else {
            return None;
        }
    }
    if prev_underscore {
        return None;
    }
    let magnitude: i64 = cleaned.parse().ok()?;
    Some(if negative {
        magnitude.checked_neg()?
    } else {
        magnitude
    })
}

/// `IntegerField` (`fields.py`): bools rejected, >1000-char strings rejected,
/// otherwise `int(re_decimal.sub('', str(data)))`. JSON floats render via
/// `serde_json` shortest-roundtrip (`10.0` → `"10.0"` → `10`, same verdict as
/// Python's `str(10.0)`).
fn int_field(value: Option<&Value>) -> Result<Option<i64>, String> {
    let raw = match value {
        None => return Ok(None),
        Some(Value::Null) => return Err(MSG_NULL.to_owned()),
        Some(v) => v,
    };
    match raw {
        Value::Bool(_) | Value::Array(_) | Value::Object(_) => Err(MSG_INVALID_INT.to_owned()),
        Value::Number(n) => parse_python_int(&n.to_string())
            .map(Some)
            .ok_or_else(|| MSG_INVALID_INT.to_owned()),
        Value::String(s) => {
            if s.len() > 1000 {
                return Err(MSG_STRING_TOO_LARGE.to_owned());
            }
            parse_python_int(s)
                .map(Some)
                .ok_or_else(|| MSG_INVALID_INT.to_owned())
        }
        Value::Null => unreachable!("handled above"),
    }
}

/// Lowercase-then-match strings, mirroring `BooleanField._lower_if_str`
/// (`fields.py`). `allow_null` is false on our only `BooleanField`, so `None`
/// (absent) yields the default and `Null` fails `null`.
fn bool_field(value: Option<&Value>, default: bool) -> Result<bool, String> {
    const TRUE_STRINGS: &[&str] = &["t", "y", "yes", "true", "on", "1"];
    const FALSE_STRINGS: &[&str] = &["f", "n", "no", "false", "off", "0"];
    let raw = match value {
        None => return Ok(default),
        Some(Value::Null) => return Err(MSG_NULL.to_owned()),
        Some(v) => v,
    };
    match raw {
        Value::Bool(b) => Ok(*b),
        Value::String(s) => {
            let lowered = s.to_lowercase();
            if TRUE_STRINGS.contains(&lowered.as_str()) {
                Ok(true)
            } else if FALSE_STRINGS.contains(&lowered.as_str()) {
                Ok(false)
            } else {
                Err(MSG_INVALID_BOOL.to_owned())
            }
        }
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                match i {
                    1 => Ok(true),
                    0 => Ok(false),
                    _ => Err(MSG_INVALID_BOOL.to_owned()),
                }
            } else if let Some(f) = n.as_f64() {
                if f == 1.0 {
                    Ok(true)
                } else if f == 0.0 {
                    Ok(false)
                } else {
                    Err(MSG_INVALID_BOOL.to_owned())
                }
            } else {
                Err(MSG_INVALID_BOOL.to_owned())
            }
        }
        Value::Array(_) | Value::Object(_) | Value::Null => Err(MSG_INVALID_BOOL.to_owned()),
    }
}

/// `UUIDField` (`fields.py`): ints (bools included, `True` → `1`) map via
/// `uuid.UUID(int=...)`; strings via `uuid.UUID(hex=...)` (hyphenated,
/// 32-hex, braced and `urn:uuid:` forms, case-insensitive); everything else
/// fails. Output is the canonical lowercase hyphenated form.
fn uuid_field(value: Option<&Value>) -> Result<Option<String>, String> {
    let raw = match value {
        None => return Ok(None),
        Some(Value::Null) => return Err(MSG_NULL.to_owned()),
        Some(v) => v,
    };
    let id = match raw {
        Value::Bool(b) => uuid::Uuid::from_u128(*b as u128),
        Value::Number(n) => match n.as_i64() {
            Some(i) if i >= 0 => uuid::Uuid::from_u128(i as u128),
            _ => return Err(MSG_INVALID_UUID.to_owned()),
        },
        Value::String(s) => match uuid::Uuid::parse_str(s) {
            Ok(id) => id,
            Err(_) => return Err(MSG_INVALID_UUID.to_owned()),
        },
        Value::Array(_) | Value::Object(_) | Value::Null => {
            return Err(MSG_INVALID_UUID.to_owned());
        }
    };
    Ok(Some(id.hyphenated().to_string()))
}

/// `JSONField(binary=False)` over parsed JSON (`fields.py:1776`): strings
/// carry no `is_json_string` marker here, so every non-null value passes
/// through unchanged (`json.dumps` always succeeds on parsed JSON).
fn json_field(value: Option<&Value>) -> Result<Option<Value>, String> {
    match value {
        None => Ok(None),
        Some(Value::Null) => Err(MSG_NULL.to_owned()),
        Some(v) => Ok(Some(v.clone())),
    }
}

/// Build `{"<field>": ["<message>"]}` in field order (`preserve_order` keeps
/// insertion order, so multi-error bodies serialize byte-identical to DRF).
fn push_error(errors: &mut Map<String, Value>, field: &str, message: String) {
    errors.insert(field.to_owned(), Value::Array(vec![Value::String(message)]));
}

fn finish(out: Map<String, Value>, errors: Map<String, Value>) -> Result<Value, Value> {
    if errors.is_empty() {
        Ok(Value::Object(out))
    } else {
        Err(Value::Object(errors))
    }
}

/// `UserAssetUploadSerializer` (`serializers/asset.py:13-43`): `name` / `size`
/// / `entity_type` required; `type` defaults to `image/jpeg` when absent.
pub fn validate_user_asset_upload(input: &Map<String, Value>) -> Result<Value, Value> {
    let mut out = Map::new();
    let mut errors = Map::new();
    match char_field(input.get("name")) {
        Ok(None) => push_error(&mut errors, "name", MSG_REQUIRED.to_owned()),
        Ok(Some(v)) => {
            out.insert("name".to_owned(), Value::String(v));
        }
        Err(m) => push_error(&mut errors, "name", m),
    }
    match input.get("type") {
        None => {
            out.insert(
                "type".to_owned(),
                Value::String(DEFAULT_USER_ASSET_TYPE.to_owned()),
            );
        }
        Some(v) => match choice_field(Some(v), USER_ASSET_TYPES) {
            Ok(Some(choice)) => {
                out.insert("type".to_owned(), Value::String(choice));
            }
            Ok(None) => {}
            Err(m) => push_error(&mut errors, "type", m),
        },
    }
    match int_field(input.get("size")) {
        Ok(None) => push_error(&mut errors, "size", MSG_REQUIRED.to_owned()),
        Ok(Some(v)) => {
            out.insert("size".to_owned(), Value::Number(v.into()));
        }
        Err(m) => push_error(&mut errors, "size", m),
    }
    match choice_field(input.get("entity_type"), USER_ENTITY_TYPES) {
        Ok(None) => push_error(&mut errors, "entity_type", MSG_REQUIRED.to_owned()),
        Ok(Some(v)) => {
            out.insert("entity_type".to_owned(), Value::String(v));
        }
        Err(m) => push_error(&mut errors, "entity_type", m),
    }
    finish(out, errors)
}

/// `AssetUpdateSerializer` (`serializers/asset.py:45-54`): `attributes` JSON,
/// not required, no default (absent key validates to absent).
pub fn validate_asset_update(input: &Map<String, Value>) -> Result<Value, Value> {
    let mut out = Map::new();
    let mut errors = Map::new();
    match json_field(input.get("attributes")) {
        Ok(None) => {}
        Ok(Some(v)) => {
            out.insert("attributes".to_owned(), v);
        }
        Err(m) => push_error(&mut errors, "attributes", m),
    }
    finish(out, errors)
}

/// `GenericAssetUploadSerializer` (`serializers/asset.py:56-80`): `name` /
/// `size` required; `type` (free `CharField`), `project_id`, `external_id`,
/// `external_source` optional with no default (absent when missing).
pub fn validate_generic_asset_upload(input: &Map<String, Value>) -> Result<Value, Value> {
    let mut out = Map::new();
    let mut errors = Map::new();
    match char_field(input.get("name")) {
        Ok(None) => push_error(&mut errors, "name", MSG_REQUIRED.to_owned()),
        Ok(Some(v)) => {
            out.insert("name".to_owned(), Value::String(v));
        }
        Err(m) => push_error(&mut errors, "name", m),
    }
    match char_field(input.get("type")) {
        Ok(None) => {}
        Ok(Some(v)) => {
            out.insert("type".to_owned(), Value::String(v));
        }
        Err(m) => push_error(&mut errors, "type", m),
    }
    match int_field(input.get("size")) {
        Ok(None) => push_error(&mut errors, "size", MSG_REQUIRED.to_owned()),
        Ok(Some(v)) => {
            out.insert("size".to_owned(), Value::Number(v.into()));
        }
        Err(m) => push_error(&mut errors, "size", m),
    }
    match uuid_field(input.get("project_id")) {
        Ok(None) => {}
        Ok(Some(v)) => {
            out.insert("project_id".to_owned(), Value::String(v));
        }
        Err(m) => push_error(&mut errors, "project_id", m),
    }
    match char_field(input.get("external_id")) {
        Ok(None) => {}
        Ok(Some(v)) => {
            out.insert("external_id".to_owned(), Value::String(v));
        }
        Err(m) => push_error(&mut errors, "external_id", m),
    }
    match char_field(input.get("external_source")) {
        Ok(None) => {}
        Ok(Some(v)) => {
            out.insert("external_source".to_owned(), Value::String(v));
        }
        Err(m) => push_error(&mut errors, "external_source", m),
    }
    finish(out, errors)
}

/// `GenericAssetUpdateSerializer` (`serializers/asset.py:82-91`):
/// `is_uploaded` defaults to `True` when absent.
pub fn validate_generic_asset_update(input: &Map<String, Value>) -> Result<Value, Value> {
    let mut out = Map::new();
    let mut errors = Map::new();
    match bool_field(input.get("is_uploaded"), true) {
        Ok(v) => {
            out.insert("is_uploaded".to_owned(), Value::Bool(v));
        }
        Err(m) => push_error(&mut errors, "is_uploaded", m),
    }
    finish(out, errors)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    static FIXTURE: &str = include_str!("../../../../fixtures/v1_assets/fx-ser-asset.json");

    fn input_of(raw: &Value) -> Map<String, Value> {
        raw.as_object().cloned().unwrap_or_default()
    }

    type ValidateFn = dyn Fn(&Map<String, Value>) -> Result<Value, Value>;

    /// Run every `goldens` entry of one serializer section against its kernel:
    /// entries with `"status": 400` must fail with exactly `out`, the rest must
    /// validate to `out`. Comparison is value equality: the fixture files
    /// store object keys alphabetically while live DRF emits declaration
    /// order, so byte order is pinned separately in `*_key_order_is_drf_*`
    /// below (order verified live).
    fn check_goldens(section: &str, validate: &ValidateFn) {
        let fixture: Value = serde_json::from_str(FIXTURE).expect("fixture parses");
        let serializers = &fixture["serializers"];
        let goldens = serializers[section]["goldens"]
            .as_array()
            .unwrap_or_else(|| panic!("{section} has goldens"));
        assert!(!goldens.is_empty(), "{section} has goldens");
        for (i, golden) in goldens.iter().enumerate() {
            let input = input_of(&golden["in"]);
            let expected_out = &golden["out"];
            let result = validate(&input);
            match golden.get("status") {
                Some(status) if status == 400 => match result {
                    Err(body) => assert_eq!(
                        &body, expected_out,
                        "{section} golden {i}: error body mismatch"
                    ),
                    Ok(body) => panic!("{section} golden {i}: expected 400, got {body}"),
                },
                _ => match result {
                    Ok(body) => {
                        assert_eq!(&body, expected_out, "{section} golden {i}: data mismatch");
                    }
                    Err(body) => panic!("{section} golden {i}: expected valid, got {body}"),
                },
            }
        }
    }

    #[test]
    fn validated_data_key_order_is_drf_declaration_order() {
        // Live `validated_data` is an OrderedDict in field-declaration order;
        // the kernels build their maps in the same order so the handler layer
        // serializes byte-identical bodies.
        let user = Map::from_iter([
            ("entity_type".to_owned(), json!("USER_AVATAR")),
            ("name".to_owned(), json!("profile.jpg")),
            ("size".to_owned(), json!(1024000)),
        ]);
        assert_eq!(
            serde_json::to_string(&validate_user_asset_upload(&user).expect("valid")).unwrap(),
            r#"{"name":"profile.jpg","type":"image/jpeg","size":1024000,"entity_type":"USER_AVATAR"}"#
        );
        let generic = Map::from_iter([
            ("external_id".to_owned(), json!("123")),
            ("external_source".to_owned(), json!("github")),
            ("name".to_owned(), json!("image.jpg")),
            (
                "project_id".to_owned(),
                json!("123e4567-e89b-12d3-a456-426614174000"),
            ),
            ("size".to_owned(), json!(1024000)),
            ("type".to_owned(), json!("image/jpeg")),
        ]);
        assert_eq!(
            serde_json::to_string(&validate_generic_asset_upload(&generic).expect("valid"))
                .unwrap(),
            r#"{"name":"image.jpg","type":"image/jpeg","size":1024000,"project_id":"123e4567-e89b-12d3-a456-426614174000","external_id":"123","external_source":"github"}"#
        );
    }

    #[test]
    fn user_asset_upload_matches_fixture() {
        check_goldens("UserAssetUploadSerializer", &validate_user_asset_upload);
    }

    #[test]
    fn generic_asset_upload_matches_fixture() {
        check_goldens(
            "GenericAssetUploadSerializer",
            &validate_generic_asset_upload,
        );
    }

    #[test]
    fn generic_asset_update_matches_fixture() {
        check_goldens(
            "GenericAssetUpdateSerializer",
            &validate_generic_asset_update,
        );
    }

    #[test]
    fn asset_update_matches_fixture_live_behavior() {
        // Two of three `AssetUpdateSerializer` goldens pass verbatim (absent
        // key, dict passthrough). The `"[1,2]"` golden records the parsed
        // value `[1, 2]`; live DRF 3.15.2 over parsed JSON passes the string
        // through (`is_json_string` is only set for HTML-form input), so the
        // kernel asserts the live behavior here. Fixture correction filed.
        let fixture: Value = serde_json::from_str(FIXTURE).expect("fixture parses");
        let goldens = fixture["serializers"]["AssetUpdateSerializer"]["goldens"]
            .as_array()
            .expect("goldens");
        assert_eq!(goldens.len(), 3);
        assert_eq!(
            validate_asset_update(&input_of(&goldens[0]["in"])),
            Ok(json!({}))
        );
        assert_eq!(
            validate_asset_update(&input_of(&goldens[1]["in"])),
            Ok(json!({"attributes": {"caption": "me"}}))
        );
        assert_eq!(
            validate_asset_update(&Map::from_iter([(
                "attributes".to_owned(),
                Value::String("[1,2]".to_owned())
            )])),
            Ok(json!({"attributes": "[1,2]"}))
        );
    }

    #[test]
    fn asset_update_rejects_null_and_passes_any_json() {
        assert_eq!(
            validate_asset_update(&Map::from_iter([("attributes".to_owned(), Value::Null)])),
            Err(json!({"attributes": ["This field may not be null."]}))
        );
        for raw in [json!(5), json!([1, 2]), json!("x"), json!("{bad")] {
            let mut input = Map::new();
            input.insert("attributes".to_owned(), raw.clone());
            assert_eq!(
                validate_asset_update(&input),
                Ok(json!({"attributes": raw}))
            );
        }
    }

    #[test]
    fn char_edges_match_drf() {
        // int coerces, bool rejected, whitespace-only blank, trim applied,
        // null chars rejected with the validator message.
        let full = |name: Value| {
            Map::from_iter([
                ("entity_type".to_owned(), json!("USER_AVATAR")),
                ("name".to_owned(), name),
                ("size".to_owned(), json!(1)),
            ])
        };
        assert_eq!(
            validate_user_asset_upload(&full(json!(123))).expect("valid")["name"],
            json!("123")
        );
        assert_eq!(
            validate_user_asset_upload(&full(json!(true))),
            Err(json!({"name": ["Not a valid string."]}))
        );
        assert_eq!(
            validate_user_asset_upload(&full(json!("   "))),
            Err(json!({"name": ["This field may not be blank."]}))
        );
        assert_eq!(
            validate_user_asset_upload(&full(json!("  x  "))).expect("valid")["name"],
            json!("x")
        );
    }

    #[test]
    fn char_trim_and_int_edges_match_drf() {
        let full = |v: Value| {
            Map::from_iter([
                ("entity_type".to_owned(), json!("USER_AVATAR")),
                ("name".to_owned(), json!("a")),
                ("size".to_owned(), v),
            ])
        };
        assert_eq!(
            validate_user_asset_upload(&full(json!(" 10 "))).expect("valid")["size"],
            json!(10)
        );
        assert_eq!(
            validate_user_asset_upload(&full(json!("1.0"))).expect("valid")["size"],
            json!(1)
        );
        assert_eq!(
            validate_user_asset_upload(&full(json!("1_0"))).expect("valid")["size"],
            json!(10)
        );
        assert_eq!(
            validate_user_asset_upload(&full(json!(1000.0))).expect("valid")["size"],
            json!(1000)
        );
        assert!(validate_user_asset_upload(&full(json!(10.5))).is_err());
        assert!(validate_user_asset_upload(&{
            let mut m = full(json!(1));
            m.insert("name".to_owned(), json!("a\0b"));
            m
        })
        .is_err());
    }

    #[test]
    fn choice_and_uuid_edges_match_drf() {
        let sized = || {
            Map::from_iter([
                ("name".to_owned(), json!("x")),
                ("size".to_owned(), json!(1)),
            ])
        };
        let with = |key: &str, v: Value| {
            let mut m = sized();
            m.insert(key.to_owned(), v);
            m
        };
        // Generic `type` is a free CharField (no choices): `""` fails blank.
        assert_eq!(
            validate_generic_asset_upload(&with("type", json!(""))),
            Err(json!({"type": ["This field may not be blank."]}))
        );
        // User `type` is a ChoiceField: `""` and `5` miss the choice table.
        let mut user_type = Map::from_iter([
            ("entity_type".to_owned(), json!("USER_AVATAR")),
            ("name".to_owned(), json!("a")),
            ("size".to_owned(), json!(1)),
            ("type".to_owned(), json!("")),
        ]);
        assert_eq!(
            validate_user_asset_upload(&user_type),
            Err(json!({"type": ["\"\" is not a valid choice."]}))
        );
        user_type.insert("type".to_owned(), json!(5));
        assert_eq!(
            validate_user_asset_upload(&user_type),
            Err(json!({"type": ["\"5\" is not a valid choice."]}))
        );
        // UUID forms normalize; ints/bools coerce; floats/lists fail.
        assert_eq!(
            validate_generic_asset_upload(&with(
                "project_id",
                json!("123e4567e89b12d3a456426614174000")
            ))
            .expect("valid")["project_id"],
            json!("123e4567-e89b-12d3-a456-426614174000")
        );
        assert_eq!(
            validate_generic_asset_upload(&with("project_id", json!(5))).expect("valid")
                ["project_id"],
            json!("00000000-0000-0000-0000-000000000005")
        );
        assert_eq!(
            validate_generic_asset_upload(&with("project_id", json!(true))).expect("valid")
                ["project_id"],
            json!("00000000-0000-0000-0000-000000000001")
        );
        assert!(validate_generic_asset_upload(&with("project_id", json!(1.5))).is_err());
        // Unknown keys ignored; external_id coerces ints.
        let mut extra = with("external_id", json!(5));
        extra.insert("zzz".to_owned(), json!("ignored?"));
        let body = validate_generic_asset_upload(&extra).expect("valid");
        assert_eq!(body["external_id"], json!("5"));
        assert!(body.get("zzz").is_none());
        // Empty user upload collects all three required errors in order.
        assert_eq!(
            validate_user_asset_upload(&Map::new()),
            Err(json!({
                "name": ["This field is required."],
                "size": ["This field is required."],
                "entity_type": ["This field is required."]
            }))
        );
        assert_eq!(
            serde_json::to_string(&validate_user_asset_upload(&Map::new()).unwrap_err()).unwrap(),
            r#"{"name":["This field is required."],"size":["This field is required."],"entity_type":["This field is required."]}"#
        );
    }

    #[test]
    fn bool_edges_match_drf() {
        assert_eq!(
            validate_generic_asset_update(&Map::new()),
            Ok(json!({"is_uploaded": true}))
        );
        assert_eq!(
            validate_generic_asset_update(&Map::from_iter([(
                "is_uploaded".to_owned(),
                Value::Null
            )])),
            Err(json!({"is_uploaded": ["This field may not be null."]}))
        );
        assert!(
            validate_generic_asset_update(&input_of(&json!({"is_uploaded": "maybe"}))).is_err()
        );
        assert_eq!(
            validate_generic_asset_update(&input_of(&json!({"is_uploaded": "off"}))),
            Ok(json!({"is_uploaded": false}))
        );
        assert_eq!(
            validate_generic_asset_update(&input_of(&json!({"is_uploaded": 1}))),
            Ok(json!({"is_uploaded": true}))
        );
    }
}
