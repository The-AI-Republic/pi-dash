#![forbid(unsafe_code)]

//! Namespaced user settings (`core/user_settings.py` + the
//! `ee/settings/user_settings.py` seam).
//!
//! Some preferences exist only in a particular build, so `Profile.settings`
//! is a generic `{namespace: {key: value}}` bag and the schema — which keys
//! exist and what they mean when unset — is the only thing a build declares.
//! Validation, merge and read semantics below are shared by every build; the
//! [`UserSettingsSchema`] trait is the overlay point (CE declares nothing,
//! the private crate extends the base schema instead of replacing it).
//!
//! The declaration is values-with-defaults: each default doubles as the type
//! declaration for its key.

use std::collections::BTreeMap;

use serde_json::Value;

/// Schema shape: `{namespace: {key: default}}`.
pub type SettingsSchema = BTreeMap<String, BTreeMap<String, Value>>;

/// Accepted patch shape, same nesting as the schema.
pub type SettingsPatch = BTreeMap<String, BTreeMap<String, Value>>;

/// Ceiling on one PATCH's accepted payload, encoded as JSON.
pub const MAX_SETTINGS_PATCH_BYTES: usize = 4096;

/// Rejection reason for a client-supplied settings payload.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct SettingsError(pub String);

/// Settings namespaces owned by OSS Pi Dash. The public build currently
/// declares none; the function lives outside the overlayable seam so a
/// downstream build can always import the OSS declaration and extend it.
pub fn base_settings_schema() -> SettingsSchema {
    SettingsSchema::new()
}

/// Return `base` plus an independently owned `extension` schema.
///
/// Namespaces may be shared, but a key may have only one owner: silently
/// overriding a public key from a private build would make its validation
/// and default depend on packaging order, so collisions fail here rather
/// than changing user-facing behaviour implicitly. Neither input is
/// mutated.
pub fn extend_settings_schema(
    base: &SettingsSchema,
    extension: &SettingsSchema,
) -> Result<SettingsSchema, SettingsError> {
    let mut merged = base.clone();
    for (namespace, values) in extension {
        let existing = merged.entry(namespace.clone()).or_default();
        let collisions: Vec<&str> = values
            .keys()
            .filter(|key| existing.contains_key(*key))
            .map(String::as_str)
            .collect();
        if !collisions.is_empty() {
            return Err(SettingsError(format!(
                "settings schema collision(s) in {namespace}: {}",
                collisions.join(", ")
            )));
        }
        for (key, default) in values {
            existing.insert(key.clone(), default.clone());
        }
    }
    Ok(merged)
}

/// Overlay seam for `ee/settings/user_settings.py::known_settings_schema`.
///
/// CE declares no namespaces; the private crate returns the base schema
/// extended with its own. Anything absent here is rejected on write and
/// ignored on read.
pub trait UserSettingsSchema {
    fn known_settings_schema(&self) -> SettingsSchema {
        base_settings_schema()
    }
}

/// CE schema: no namespaces, so the bag stays empty and the API rejects
/// every write to it.
pub struct CeUserSettings;

impl UserSettingsSchema for CeUserSettings {}

/// The declared default for one key, or `None` when it is not declared.
pub fn default_for(schema: &SettingsSchema, namespace: &str, key: &str) -> Option<Value> {
    schema
        .get(namespace)
        .and_then(|keys| keys.get(key))
        .cloned()
}

/// Read one setting, falling back to its declared default, so a missing or
/// partial bag is never a lookup failure.
pub fn get_setting(
    schema: &SettingsSchema,
    stored: &Value,
    namespace: &str,
    key: &str,
) -> Option<Value> {
    stored
        .get(namespace)
        .and_then(|values| values.get(key))
        .cloned()
        .or_else(|| default_for(schema, namespace, key))
}

/// Reject a value whose type does not match the key's declared default.
/// `bool` is checked before the numbers because it subclasses `int` in
/// Python; JSON keeps them distinct, but the ordering stays so the two
/// implementations accept the same values.
fn check_value(
    namespace: &str,
    key: &str,
    value: &Value,
    default: &Value,
) -> Result<(), SettingsError> {
    let ok = match default {
        Value::Null => !matches!(value, Value::Array(_) | Value::Object(_)),
        Value::Bool(_) => matches!(value, Value::Bool(_)),
        Value::Number(default_number) => match value {
            Value::Number(value_number) => {
                if default_number.is_f64() {
                    true
                } else {
                    value_number.is_i64() || value_number.is_u64()
                }
            }
            _ => false,
        },
        Value::String(_) => matches!(value, Value::String(_)),
        Value::Array(_) => matches!(value, Value::Array(_)),
        Value::Object(_) => matches!(value, Value::Object(_)),
    };
    if ok {
        return Ok(());
    }
    let expected = match default {
        Value::Null => "scalar",
        Value::Bool(_) => "bool",
        Value::Number(number) => {
            if number.is_f64() {
                "float"
            } else {
                "int"
            }
        }
        Value::String(_) => "str",
        Value::Array(_) => "list",
        Value::Object(_) => "dict",
    };
    Err(SettingsError(format!(
        "settings.{namespace}.{key} must be of type {expected}"
    )))
}

/// Validate a client-supplied `settings` payload against the schema.
/// Returns the accepted patch; rejects unknown namespaces/keys,
/// mistyped values, non-object shapes and oversized payloads.
///
/// The size cap is measured over the compact JSON encoding rather than
/// Python's spaced `json.dumps`; the cap is a denial-of-service guard, not
/// a byte-exact contract, so the boundary may differ by whitespace.
pub fn validate_settings_patch(
    schema: &SettingsSchema,
    patch: &Value,
) -> Result<SettingsPatch, SettingsError> {
    let patch_object = patch
        .as_object()
        .ok_or_else(|| SettingsError("settings must be an object".to_string()))?;
    let mut accepted = SettingsPatch::new();
    for (namespace, values) in patch_object {
        let declared = schema
            .get(namespace)
            .ok_or_else(|| SettingsError(format!("unknown settings namespace: {namespace}")))?;
        let values_object = values
            .as_object()
            .ok_or_else(|| SettingsError(format!("settings.{namespace} must be an object")))?;
        let unknown: Vec<&str> = values_object
            .keys()
            .filter(|key| !declared.contains_key(*key))
            .map(String::as_str)
            .collect();
        if !unknown.is_empty() {
            return Err(SettingsError(format!(
                "unknown settings key(s) in {namespace}: {}",
                unknown.join(", ")
            )));
        }
        for (key, value) in values_object {
            let default = &declared[key.as_str()];
            check_value(namespace, key, value, default)?;
        }
        accepted.insert(
            namespace.clone(),
            values_object
                .iter()
                .map(|(key, value)| (key.clone(), value.clone()))
                .collect(),
        );
    }
    let encoded = serde_json::to_vec(&accepted)
        .map_err(|_| SettingsError("settings must be JSON-serialisable".to_string()))?;
    if encoded.len() > MAX_SETTINGS_PATCH_BYTES {
        return Err(SettingsError(format!(
            "settings payload is too large ({} > {MAX_SETTINGS_PATCH_BYTES} bytes)",
            encoded.len()
        )));
    }
    Ok(accepted)
}

/// Merge a validated patch into the stored bag, per namespace, so patching
/// one namespace never drops another's values.
pub fn merge_settings(stored: &Value, patch: &SettingsPatch) -> SettingsPatch {
    let mut merged: SettingsPatch = stored
        .as_object()
        .map(|object| {
            object
                .iter()
                .filter_map(|(key, value)| {
                    value.as_object().map(|inner| {
                        (
                            key.clone(),
                            inner.iter().map(|(k, v)| (k.clone(), v.clone())).collect(),
                        )
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    for (namespace, values) in patch {
        merged.entry(namespace.clone()).or_default().extend(
            values
                .iter()
                .map(|(key, value)| (key.clone(), value.clone())),
        );
    }
    merged
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn schema_with_openhub() -> SettingsSchema {
        extend_settings_schema(
            &base_settings_schema(),
            &BTreeMap::from([(
                "openhub".to_string(),
                BTreeMap::from([("apps_enabled".to_string(), json!(false))]),
            )]),
        )
        .expect("extend")
    }

    #[test]
    fn ce_schema_declares_nothing() {
        assert!(CeUserSettings.known_settings_schema().is_empty());
        assert_eq!(base_settings_schema(), SettingsSchema::new());
    }

    #[test]
    fn extension_adds_namespaces_without_mutating_base() {
        let base = base_settings_schema();
        let merged = extend_settings_schema(
            &base,
            &BTreeMap::from([(
                "openhub".to_string(),
                BTreeMap::from([("apps_enabled".to_string(), json!(false))]),
            )]),
        )
        .expect("extend");
        assert!(base.is_empty());
        assert_eq!(
            merged["openhub"]["apps_enabled"],
            json!(false),
            "extension lands",
        );
    }

    #[test]
    fn key_collision_fails_at_assembly() {
        let base: SettingsSchema = BTreeMap::from([(
            "openhub".to_string(),
            BTreeMap::from([("apps_enabled".to_string(), json!(false))]),
        )]);
        let error = extend_settings_schema(
            &base,
            &BTreeMap::from([(
                "openhub".to_string(),
                BTreeMap::from([("apps_enabled".to_string(), json!(true))]),
            )]),
        )
        .expect_err("collision");
        assert!(
            error.0.contains("collision") && error.0.contains("apps_enabled"),
            "unexpected: {error}",
        );
    }

    #[test]
    fn ce_rejects_every_write() {
        let schema = CeUserSettings.known_settings_schema();
        let error = validate_settings_patch(&schema, &json!({"openhub": {"apps_enabled": true}}))
            .expect_err("unknown namespace");
        assert_eq!(error.0, "unknown settings namespace: openhub");
    }

    #[test]
    fn unknown_key_and_wrong_type_reject() {
        let schema = schema_with_openhub();
        let error = validate_settings_patch(&schema, &json!({"openhub": {"nope": true}}))
            .expect_err("unknown key");
        assert_eq!(error.0, "unknown settings key(s) in openhub: nope");
        let error = validate_settings_patch(&schema, &json!({"openhub": {"apps_enabled": "yes"}}))
            .expect_err("wrong type");
        assert_eq!(
            error.0,
            "settings.openhub.apps_enabled must be of type bool",
        );
    }

    #[test]
    fn non_object_shapes_reject() {
        let schema = schema_with_openhub();
        assert!(validate_settings_patch(&schema, &json!([1])).is_err());
        let error = validate_settings_patch(&schema, &json!({"openhub": true}))
            .expect_err("namespace must be object");
        assert_eq!(error.0, "settings.openhub must be an object");
    }

    #[test]
    fn int_key_rejects_bool_and_float() {
        let schema: SettingsSchema = BTreeMap::from([(
            "ns".to_string(),
            BTreeMap::from([
                ("count".to_string(), json!(0)),
                ("ratio".to_string(), json!(1.5)),
            ]),
        )]);
        assert!(validate_settings_patch(&schema, &json!({"ns": {"count": 3}})).is_ok());
        assert!(validate_settings_patch(&schema, &json!({"ns": {"count": true}})).is_err());
        assert!(validate_settings_patch(&schema, &json!({"ns": {"count": 1.5}})).is_err());
        assert!(validate_settings_patch(&schema, &json!({"ns": {"ratio": 2}})).is_ok());
    }

    #[test]
    fn oversized_patch_rejects() {
        let big = "x".repeat(MAX_SETTINGS_PATCH_BYTES);
        let schema: SettingsSchema = BTreeMap::from([(
            "ns".to_string(),
            BTreeMap::from([("blob".to_string(), json!(""))]),
        )]);
        let error =
            validate_settings_patch(&schema, &json!({"ns": {"blob": big}})).expect_err("too large");
        assert!(error.0.contains("too large"), "unexpected: {error}");
    }

    #[test]
    fn merge_keeps_other_namespaces() {
        let stored = json!({"a": {"x": 1}, "dropped": 7});
        let patch: SettingsPatch = BTreeMap::from([(
            "a".to_string(),
            BTreeMap::from([("y".to_string(), json!(2))]),
        )]);
        let merged = merge_settings(&stored, &patch);
        assert_eq!(merged["a"]["x"], json!(1));
        assert_eq!(merged["a"]["y"], json!(2));
        assert!(
            !merged.contains_key("dropped"),
            "non-dict namespaces are dropped"
        );
    }

    #[test]
    fn get_setting_falls_back_to_default() {
        let schema = schema_with_openhub();
        assert_eq!(
            get_setting(&schema, &json!({}), "openhub", "apps_enabled"),
            Some(json!(false)),
        );
        assert_eq!(
            get_setting(
                &schema,
                &json!({"openhub": {"apps_enabled": true}}),
                "openhub",
                "apps_enabled",
            ),
            Some(json!(true)),
        );
        assert_eq!(get_setting(&schema, &json!({}), "openhub", "missing"), None);
    }
}
