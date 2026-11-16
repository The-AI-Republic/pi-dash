#![forbid(unsafe_code)]

//! FileAsset golden shape (D-31, stage 5).
//!
//! Ports `apps/api/pi_dash/app/serializers/asset.py:9-13`
//! (`FileAssetSerializer`: `fields = "__all__"`,
//! `read_only_fields = ["created_by", "updated_by", "created_at",
//! "updated_at"]`). `id` is read-only via the declared field on
//! `BaseSerializer` (`app/serializers/base.py:11`,
//! `PrimaryKeyRelatedField(read_only=True)`).
//!
//! JSON contract: the 24-key set in [`FILEASSET_KEY_ORDER`] is the empirical
//! DRF order recorded in the fixture (`output_json.key_set`, pinned by the
//! oracle `V1_ROW_KEYS` in
//! `rust-api/contract-tests/app_assets/test_legacy_v1.py:12-17`).
//! [`FileAssetRecord`] declares fields in that order so struct serialization
//! emits the DRF sequence; [`row_to_json`] returns the same content as
//! a `Value` for key-set/content checks (a `Value` object iterates
//! alphabetically without the workspace `preserve_order` feature, so byte
//! order is asserted off the struct rendering, never off the `Value`).
//!
//! Rendering rules (Porting guide semantic traps):
//! - Datetimes are carried as the DRF-rendered strings the query layer
//!   supplies (`...Z` ISO-8601; the oracle asserts `created_at` ends with
//!   `Z`). This module never formats datetimes.
//! - `size` is a Django `FloatField` (`db/models/asset.py:60`) and renders
//!   as a JSON number (`100.0`), never a string.
//! - `asset` is a `FileField` (`db/models/asset.py:46`): DRF renders the
//!   storage name, and the oracle asserts the stored key is contained in
//!   `item["asset"]` — containment, not equality.
//! - Foreign keys (`user`, `workspace`, `draft_issue`, `project`, `issue`,
//!   `comment`, `page`, `created_by`, `updated_by`) render as their raw pk
//!   (uuid string) or null.
//!
//! Write semantics: read-only fields are ignored on input (DRF drops them
//! from validated data; `created_by` is instead set by `BaseModel.save()`
//! current-user fallback or explicit `save()` kwargs — see the fixture
//! `read_only_semantics`). [`strip_read_only_fields`] models that drop.
//!
//! Ported bugs (recorded here, fixed nowhere in this module):
//! - `BUG (app/views/asset/base.py:67)`: `UserAssetsEndpoint.get`
//!   serializes a queryset without `many=True`, so any existing user row
//!   500s. Owned by the queries layer (`queries/v1.golden.json`); this
//!   module ports both call shapes' shared row contract only.
//! - `BUG (db/models/asset.py:46)`: `asset` carries no `validators=`
//!   (migration `0075_alter_fileasset_asset.py` lists
//!   `FileExtensionValidator` + `file_size`, the model dropped them).
//!   Owned by the models layer; shape output is unaffected.
//!
//! Fixture: `rust-api/fixtures/app_assets/serializers/fileasset.golden.json`
//! (PIDASHCONV-306).

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// `FileAssetSerializer` output keys in DRF render order
/// (fixture `output_json.key_set`; oracle `V1_ROW_KEYS`).
pub const FILEASSET_KEY_ORDER: &[&str] = &[
    "id",
    "created_at",
    "updated_at",
    "deleted_at",
    "attributes",
    "asset",
    "entity_type",
    "entity_identifier",
    "is_deleted",
    "is_archived",
    "external_id",
    "external_source",
    "size",
    "is_uploaded",
    "storage_metadata",
    "created_by",
    "updated_by",
    "user",
    "workspace",
    "draft_issue",
    "project",
    "issue",
    "comment",
    "page",
];

/// `FileAssetSerializer.Meta.read_only_fields` (`asset.py:12`).
pub const FILEASSET_READ_ONLY_FIELDS: &[&str] =
    &["created_by", "updated_by", "created_at", "updated_at"];

/// Whether a serializer field is read-only on write: the four
/// [`FILEASSET_READ_ONLY_FIELDS`] entries plus `id`, which is read-only via
/// the declared `PrimaryKeyRelatedField(read_only=True)` on `BaseSerializer`
/// (`app/serializers/base.py:11`).
pub fn is_read_only_field(name: &str) -> bool {
    name == "id" || FILEASSET_READ_ONLY_FIELDS.contains(&name)
}

/// Drop read-only fields from a write payload, modelling DRF ignoring them
/// on input (`asset.py:12` + `base.py:11`). Writable keys survive untouched.
pub fn strip_read_only_fields(obj: &mut Map<String, Value>) {
    for key in FILEASSET_KEY_ORDER {
        if is_read_only_field(key) {
            obj.remove(*key);
        }
    }
}

/// One `FileAsset` row rendered as `FileAssetSerializer` output
/// (`asset.py:9-13` over the 18 own fields in `db/models/asset.py:45-62`
/// plus audit columns).
///
/// Fields are declared in [`FILEASSET_KEY_ORDER`] so serialization emits the
/// DRF sequence. Datetimes are the DRF-rendered strings supplied by the
/// query layer; foreign keys are raw pk strings or null; `attributes` and
/// `storage_metadata` pass through verbatim.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FileAssetRecord {
    pub id: String,
    pub created_at: String,
    pub updated_at: String,
    pub deleted_at: Option<String>,
    pub attributes: Value,
    pub asset: String,
    pub entity_type: Option<String>,
    pub entity_identifier: Option<String>,
    pub is_deleted: bool,
    pub is_archived: bool,
    pub external_id: Option<String>,
    pub external_source: Option<String>,
    pub size: f64,
    pub is_uploaded: bool,
    pub storage_metadata: Value,
    pub created_by: Option<String>,
    pub updated_by: Option<String>,
    pub user: Option<String>,
    pub workspace: Option<String>,
    pub draft_issue: Option<String>,
    pub project: Option<String>,
    pub issue: Option<String>,
    pub comment: Option<String>,
    pub page: Option<String>,
}

/// Render a row as the serializer body (same content the struct
/// serialization emits; key order is asserted off the struct rendering).
pub fn row_to_json(record: &FileAssetRecord) -> Value {
    serde_json::to_value(record).expect("record serializes")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn golden() -> Value {
        let path = format!(
            "{}/../../fixtures/app_assets/serializers/fileasset.golden.json",
            env!("CARGO_MANIFEST_DIR")
        );
        serde_json::from_str(&std::fs::read_to_string(&path).expect("golden exists"))
            .expect("golden parses")
    }

    fn str_list(value: &Value) -> Vec<String> {
        value
            .as_array()
            .expect("array")
            .iter()
            .map(|v| v.as_str().expect("str").to_owned())
            .collect()
    }

    /// Top-level JSON key order of a struct's serialization, read off the
    /// rendered string (struct serialization emits declaration order even
    /// though a `Value` object would iterate alphabetically).
    fn serialized_keys<T: serde::Serialize>(value: &T) -> Vec<String> {
        let rendered = serde_json::to_string(value).expect("serializes");
        let mut keys = Vec::new();
        let mut depth = 0usize;
        let mut chars = rendered.chars().peekable();
        while let Some(c) = chars.next() {
            match c {
                '{' => depth += 1,
                '}' => depth -= 1,
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

    fn record_from_input(input: &Value) -> FileAssetRecord {
        let get_str = |k: &str| {
            input
                .get(k)
                .and_then(Value::as_str)
                .expect("str")
                .to_owned()
        };
        let get_opt = |k: &str| match input.get(k) {
            None | Some(Value::Null) => None,
            Some(Value::String(s)) => Some(s.clone()),
            Some(other) => panic!("expected string|null for {k}, got {other}"),
        };
        FileAssetRecord {
            id: get_str("id"),
            created_at: get_str("created_at"),
            updated_at: get_str("updated_at"),
            deleted_at: get_opt("deleted_at"),
            attributes: input.get("attributes").expect("attributes").clone(),
            asset: get_str("asset"),
            entity_type: get_opt("entity_type"),
            entity_identifier: get_opt("entity_identifier"),
            is_deleted: input
                .get("is_deleted")
                .and_then(Value::as_bool)
                .expect("bool"),
            is_archived: input
                .get("is_archived")
                .and_then(Value::as_bool)
                .expect("bool"),
            external_id: get_opt("external_id"),
            external_source: get_opt("external_source"),
            size: input.get("size").and_then(Value::as_f64).expect("number"),
            is_uploaded: input
                .get("is_uploaded")
                .and_then(Value::as_bool)
                .expect("bool"),
            storage_metadata: input
                .get("storage_metadata")
                .expect("storage_metadata")
                .clone(),
            created_by: get_opt("created_by"),
            updated_by: get_opt("updated_by"),
            user: get_opt("user"),
            workspace: get_opt("workspace"),
            draft_issue: get_opt("draft_issue"),
            project: get_opt("project"),
            issue: get_opt("issue"),
            comment: get_opt("comment"),
            page: get_opt("page"),
        }
    }

    #[test]
    fn key_order_and_meta_match_golden() {
        let fixture = golden();
        let output = fixture.get("output_json").expect("output_json");
        let order = str_list(output.get("key_set").expect("key_set"));
        let expected: Vec<String> = FILEASSET_KEY_ORDER.iter().map(|s| s.to_string()).collect();
        assert_eq!(expected, order);
        assert_eq!(order.len(), 24);

        let definition = fixture.get("definition").expect("definition");
        assert_eq!(
            definition
                .get("meta_model")
                .expect("meta_model")
                .as_str()
                .expect("str"),
            "FileAsset"
        );
        assert_eq!(
            definition
                .get("fields")
                .expect("fields")
                .as_str()
                .expect("str"),
            "__all__"
        );
        assert_eq!(
            str_list(
                definition
                    .get("read_only_fields")
                    .expect("read_only_fields")
            ),
            FILEASSET_READ_ONLY_FIELDS
                .iter()
                .map(|s| s.to_string())
                .collect::<Vec<_>>(),
        );
    }

    #[test]
    fn input_row_replays_golden_shape() {
        let fixture = golden();
        let input = fixture.get("input_row").expect("input_row");
        let output = fixture.get("output_json").expect("output_json");
        let key_set = str_list(output.get("key_set").expect("key_set"));

        let record = record_from_input(input);
        let produced = row_to_json(&record);
        let obj = produced.as_object().expect("object");

        // Exact key set, byte-exact DRF emission order.
        let produced_keys: Vec<String> = obj.keys().map(|k| k.to_owned()).collect();
        let mut produced_sorted = produced_keys.clone();
        produced_sorted.sort();
        let mut key_sorted = key_set.clone();
        key_sorted.sort();
        assert_eq!(produced_sorted, key_sorted);
        assert_eq!(
            serialized_keys(&record),
            key_set,
            "struct emission order must be the DRF order"
        );

        // Every value round-trips from the input row, except `asset`
        // (FileField renders the storage name: containment, not equality).
        for key in &key_set {
            if key == "asset" {
                let rendered = obj.get("asset").and_then(Value::as_str).expect("str");
                let stored = input.get("asset").and_then(Value::as_str).expect("str");
                assert!(
                    rendered.contains(stored),
                    "output asset must contain the stored key"
                );
                continue;
            }
            assert_eq!(
                obj.get(key).expect("key present"),
                input.get(key).expect("input key present"),
                "value mismatch for {key}"
            );
        }

        // DRF datetime rendering: ISO-8601 `Z` strings, passed through.
        for key in ["created_at", "updated_at"] {
            let rendered = obj.get(key).and_then(Value::as_str).expect("str");
            assert!(rendered.ends_with('Z'), "{key} must end with Z");
        }

        // Foreign keys render as raw pk or null (oracle asserts
        // `item['workspace'] == workspace id`, `item['created_by'] == admin id`).
        assert_eq!(
            obj.get("workspace").expect("workspace"),
            input.get("workspace").expect("input workspace"),
        );
        assert_eq!(
            obj.get("created_by").expect("created_by"),
            input.get("created_by").expect("input created_by"),
        );
        assert_eq!(obj.get("user").expect("user"), &Value::Null);
        assert_eq!(obj.get("updated_by").expect("updated_by"), &Value::Null);

        // FloatField renders a JSON number, never a string.
        assert_eq!(obj.get("size").expect("size"), &serde_json::json!(100.0));
        assert!(obj.get("size").expect("size").is_number());
    }

    #[test]
    fn read_only_enforced_on_write() {
        // `id` (base.py:11) plus the four Meta.read_only_fields (asset.py:12).
        for key in ["id", "created_by", "updated_by", "created_at", "updated_at"] {
            assert!(is_read_only_field(key), "{key} must be read-only");
        }
        for key in ["asset", "workspace", "entity_type", "size", "attributes"] {
            assert!(!is_read_only_field(key), "{key} must be writable");
        }

        // A client-supplied read-only value never sticks: strip drops the
        // five read-only keys and keeps every writable key byte-identical.
        let mut payload = Map::new();
        for key in FILEASSET_KEY_ORDER {
            payload.insert(key.to_string(), Value::String("client".to_owned()));
        }
        strip_read_only_fields(&mut payload);
        for key in ["id", "created_by", "updated_by", "created_at", "updated_at"] {
            assert!(!payload.contains_key(key), "{key} must be stripped");
        }
        for key in FILEASSET_KEY_ORDER {
            if is_read_only_field(key) {
                continue;
            }
            assert_eq!(
                payload.get(*key),
                Some(&Value::String("client".to_owned())),
                "{key} must survive"
            );
        }
    }
}
