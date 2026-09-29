#![forbid(unsafe_code)]

//! Module serializers A: write / flat / issue shapes.
//!
//! Ports `apps/api/pi_dash/app/serializers/module.py:26-155`:
//! - `ModuleWriteSerializer` (`:26-120`): `__all__` plus `lead_id`
//!   (`PrimaryKeyRelatedField`, `source="lead"`, optional/nullable) and
//!   `member_ids` (write-only list of user PKs, optional). Covers `Meta`
//!   (`:36-48`), `to_representation` (`:50-53`), `validate` (`:55-62`),
//!   `create` (`:64-92`) and `update` (`:94-120`).
//! - `ModuleFlatSerializer` (`:123-134`): bare `__all__` read shape.
//! - `ModuleIssueSerializer` (`:137-153`): `__all__` plus the nested
//!   `module_detail` (`ModuleFlatSerializer`, `source="module"`),
//!   `issue_detail` (`ProjectLiteSerializer`, `source="issue"`) and the
//!   annotated `sub_issues_count`.
//!
//! Key order: DRF `__all__` emits `BaseSerializer.id` first, then declared
//! fields, then model fields in model order (verified live against the
//! project venv; no DB needed for field construction). `member_ids` is
//! write-only, so it is absent from the first render pass and
//! `to_representation` re-appends it LAST in PUT responses, even though it
//! is declared third. `WRITE_FIELD_ORDER` is the declared order;
//! `WRITE_RESPONSE_ORDER` is the wire order.
//!
//! Nested shapes owned elsewhere and referenced by name only:
//! `ProjectLiteSerializer` (`project.py:120-132`, project domain). The
//! annotated row (`MODULE_ROW_KEYS`) and the retrieve envelope belong to
//! the handlers layer, not to these serializers.
//!
//! Fixtures: `rust-api/fixtures/app_modules/serializers/module_write.golden.json`,
//! `rust-api/fixtures/app_modules/serializers/module_flat_issue_detail.golden.json`
//! (flat + issue sections).
//!
//! Part B (PIDASHCONV-313: link / detail / userprops, `module.py:156-280`)
//! appends below in its own section; nothing above moves or renames.

use serde_json::Value;

/// `ModuleWriteSerializer` fields in declared order
/// (`module.py:26-48`; order verified live via the project venv).
pub const WRITE_FIELD_ORDER: &[&str] = &[
    "id",
    "lead_id",
    "member_ids",
    "created_at",
    "updated_at",
    "deleted_at",
    "name",
    "description",
    "description_text",
    "description_html",
    "start_date",
    "target_date",
    "status",
    "view_props",
    "sort_order",
    "external_source",
    "external_id",
    "archived_at",
    "logo_props",
    "created_by",
    "updated_by",
    "project",
    "workspace",
    "lead",
    "members",
];

/// `ModuleWriteSerializer` wire order as rendered by `to_representation`
/// (`module.py:50-53`): `member_ids` is write-only, so the assignment in
/// `to_representation` appends it after `members`.
pub const WRITE_RESPONSE_ORDER: &[&str] = &[
    "id",
    "lead_id",
    "created_at",
    "updated_at",
    "deleted_at",
    "name",
    "description",
    "description_text",
    "description_html",
    "start_date",
    "target_date",
    "status",
    "view_props",
    "sort_order",
    "external_source",
    "external_id",
    "archived_at",
    "logo_props",
    "created_by",
    "updated_by",
    "project",
    "workspace",
    "lead",
    "members",
    "member_ids",
];

/// `ModuleWriteSerializer.Meta.read_only_fields` (`module.py:39-48`).
pub const WRITE_READ_ONLY_FIELDS: &[&str] = &[
    "workspace",
    "project",
    "created_by",
    "updated_by",
    "created_at",
    "updated_at",
    "archived_at",
    "deleted_at",
];

/// `ModuleFlatSerializer` fields in declared order
/// (`module.py:123-134`; order verified live via the project venv).
pub const FLAT_KEY_ORDER: &[&str] = &[
    "id",
    "created_at",
    "updated_at",
    "deleted_at",
    "name",
    "description",
    "description_text",
    "description_html",
    "start_date",
    "target_date",
    "status",
    "view_props",
    "sort_order",
    "external_source",
    "external_id",
    "archived_at",
    "logo_props",
    "created_by",
    "updated_by",
    "project",
    "workspace",
    "lead",
    "members",
];

/// `ModuleFlatSerializer.Meta.read_only_fields` (`module.py:127-134`).
pub const FLAT_READ_ONLY_FIELDS: &[&str] = &[
    "workspace",
    "project",
    "created_by",
    "updated_by",
    "created_at",
    "updated_at",
];

/// `ModuleIssueSerializer` fields in declared order
/// (`module.py:137-153`; order verified live via the project venv).
/// Declared nested fields follow `id`, then model fields in model order.
pub const MODULE_ISSUE_KEY_ORDER: &[&str] = &[
    "id",
    "module_detail",
    "issue_detail",
    "sub_issues_count",
    "created_at",
    "updated_at",
    "deleted_at",
    "created_by",
    "updated_by",
    "project",
    "workspace",
    "module",
    "issue",
];

/// `ModuleIssueSerializer` declared nested fields (`module.py:138-140`).
/// `module_detail` renders the `ModuleFlatSerializer` shape of the linked
/// module, `issue_detail` the `ProjectLiteSerializer` shape of the linked
/// issue, `sub_issues_count` the view annotation; all three are read-only.
pub const MODULE_ISSUE_NESTED_FIELDS: &[&str] =
    &["module_detail", "issue_detail", "sub_issues_count"];

/// `ModuleIssueSerializer.Meta.read_only_fields` (`module.py:145-153`).
/// `module` is read-only: the link endpoints set it from the URL, never
/// from the payload.
pub const MODULE_ISSUE_READ_ONLY_FIELDS: &[&str] = &[
    "workspace",
    "project",
    "created_by",
    "updated_by",
    "created_at",
    "updated_at",
    "module",
];

/// `ModuleWriteSerializer.validate` rejection message (`module.py:61`).
pub const START_AFTER_TARGET_MESSAGE: &str = "Start date cannot exceed target date";

/// Duplicate-name rejection message used by both `create` (`module.py:72`)
/// and `update` (`module.py:100`).
pub const DUPLICATE_NAME_MESSAGE: &str = "Module with this name already exists";

/// `ModuleMember` bulk-write batch size in `create` (`module.py:88`) and
/// `update` (`module.py:116`).
pub const MEMBER_BULK_BATCH_SIZE: usize = 10;

/// `ignore_conflicts=True` on both member bulk writes (`module.py:89`,
/// `:117`): a colliding `(module, member)` row is silently skipped.
pub const MEMBER_BULK_IGNORE_CONFLICTS: bool = true;

/// Mirror of `ModuleWriteSerializer.validate` (`module.py:55-62`): reject
/// only when both dates are present in the input and `start > target`.
/// Comparison is lexicographic over the `YYYY-MM-DD` wire format, which
/// orders identically to the `date` objects DRF compares. A partial update
/// carrying a single date skips the check (ported quirk: `data.get` sees
/// input only, never the stored row).
pub fn validate_module_dates(
    start: Option<&str>,
    target: Option<&str>,
) -> Result<(), &'static str> {
    match (start, target) {
        (Some(s), Some(t)) if s > t => Err(START_AFTER_TARGET_MESSAGE),
        _ => Ok(()),
    }
}

/// Wire body for the `validate` rejection: a bare `ValidationError(str)`
/// renders under `non_field_errors` (`test_modules.py` pins this shape).
pub fn date_violation_body() -> Value {
    Value::Object(
        [(
            "non_field_errors".to_owned(),
            Value::Array(vec![Value::String(START_AFTER_TARGET_MESSAGE.to_owned())]),
        )]
        .into_iter()
        .collect(),
    )
}

/// Wire body for the duplicate-name rejection: raised as
/// `ValidationError({"error": ...})`, so the body is the dict itself.
pub fn duplicate_name_body() -> Value {
    Value::Object(
        [(
            "error".to_owned(),
            Value::String(DUPLICATE_NAME_MESSAGE.to_owned()),
        )]
        .into_iter()
        .collect(),
    )
}

/// Mirror of `to_representation`'s `member_ids` line (`module.py:52`):
/// the stringified ids of `instance.members.all()` in iteration order.
pub fn member_ids_representation(member_ids: &[impl AsRef<str>]) -> Vec<String> {
    member_ids.iter().map(|id| id.as_ref().to_owned()).collect()
}

/// Whether `create`/`update` syncs memberships (`module.py:65`, `:95`):
/// `member_ids` is popped and only acted on when present. `None` keeps
/// existing memberships; `Some([])` clears them.
pub fn should_replace_members(member_ids: &Option<Vec<String>>) -> bool {
    member_ids.is_some()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;
    use std::collections::BTreeSet;

    fn golden(name: &str) -> Value {
        let path = format!(
            "{}/../../fixtures/app_modules/serializers/{name}",
            env!("CARGO_MANIFEST_DIR")
        );
        serde_json::from_str(&std::fs::read_to_string(&path).expect("golden exists"))
            .expect("golden parses")
    }

    fn set(keys: &[&str]) -> BTreeSet<String> {
        keys.iter().map(|k| k.to_string()).collect()
    }

    // Member bulk-write params (`module.py:88-89`, `:116-117`; pinned in
    // the fixture `create`/`update` strings): batch 10, conflicts skipped.
    const _: () = assert!(MEMBER_BULK_IGNORE_CONFLICTS);
    const _: () = assert!(MEMBER_BULK_BATCH_SIZE == 10);

    /// PUT wire order is the declared order with write-only `member_ids`
    /// moved last (`module.py:50-53`).
    #[test]
    fn write_response_order_appends_member_ids_last() {
        let mut expected: Vec<&str> = WRITE_FIELD_ORDER
            .iter()
            .filter(|k| **k != "member_ids")
            .copied()
            .collect();
        expected.push("member_ids");
        assert_eq!(WRITE_RESPONSE_ORDER, expected.as_slice());
    }

    /// Declared and wire orders carry the contract-test `WRITE_MODULE_KEYS`
    /// set (`contract-tests/app_modules/test_modules.py:21-27`).
    #[test]
    fn write_key_set_matches_contract() {
        let contract: BTreeSet<String> = [
            "id",
            "lead_id",
            "created_at",
            "updated_at",
            "deleted_at",
            "name",
            "description",
            "description_text",
            "description_html",
            "start_date",
            "target_date",
            "status",
            "view_props",
            "sort_order",
            "external_source",
            "external_id",
            "archived_at",
            "logo_props",
            "created_by",
            "updated_by",
            "project",
            "workspace",
            "lead",
            "members",
            "member_ids",
        ]
        .iter()
        .map(|k| k.to_string())
        .collect();
        assert_eq!(set(WRITE_FIELD_ORDER), contract);
        assert_eq!(set(WRITE_RESPONSE_ORDER), contract);
    }

    /// Flat shape is the write shape minus the two declared extras.
    #[test]
    fn flat_key_set_is_write_minus_declared() {
        let mut expected = set(WRITE_FIELD_ORDER);
        expected.remove("lead_id");
        expected.remove("member_ids");
        assert_eq!(set(FLAT_KEY_ORDER), expected);
        assert_eq!(FLAT_KEY_ORDER.len(), 23);
    }

    /// Fixture replay: read-only sets and traces
    /// (`module_write.golden.json`, `module_flat_issue_detail.golden.json`).
    #[test]
    fn read_only_sets_match_fixtures() {
        let write = golden("module_write.golden.json");
        let flat_issue = golden("module_flat_issue_detail.golden.json");
        let write_ro: Vec<String> = write
            .get("read_only")
            .expect("write read_only")
            .as_array()
            .expect("array")
            .iter()
            .map(|v| v.as_str().expect("str").to_owned())
            .collect();
        let expected: Vec<String> = WRITE_READ_ONLY_FIELDS
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert_eq!(expected, write_ro);
        assert!(write
            .get("trace")
            .and_then(Value::as_str)
            .expect("trace")
            .contains("app/serializers/module.py:26-120"));
        let flat = flat_issue
            .get("module_flat_serializer")
            .expect("flat section");
        let flat_ro: Vec<String> = flat
            .get("read_only")
            .expect("flat read_only")
            .as_array()
            .expect("array")
            .iter()
            .map(|v| v.as_str().expect("str").to_owned())
            .collect();
        let expected_flat: Vec<String> = FLAT_READ_ONLY_FIELDS
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert_eq!(expected_flat, flat_ro);
        let issue = flat_issue
            .get("module_issue_serializer")
            .expect("issue section");
        let issue_ro: Vec<String> = issue
            .get("read_only")
            .expect("issue read_only")
            .as_array()
            .expect("array")
            .iter()
            .map(|v| v.as_str().expect("str").to_owned())
            .collect();
        let expected_issue: Vec<String> = MODULE_ISSUE_READ_ONLY_FIELDS
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert_eq!(expected_issue, issue_ro);
        assert_eq!(MODULE_ISSUE_KEY_ORDER.len(), 13);
        for nested in MODULE_ISSUE_NESTED_FIELDS {
            assert!(MODULE_ISSUE_KEY_ORDER.contains(nested));
        }
    }

    /// Fixture replay: the three behavioral write cases
    /// (`module_write.golden.json` `cases`).
    #[test]
    fn write_cases_replay() {
        // create start>target -> 400 non_field_errors.
        assert_eq!(
            validate_module_dates(Some("2026-10-01"), Some("2026-09-01")),
            Err(START_AFTER_TARGET_MESSAGE)
        );
        assert_eq!(
            serde_json::to_string(&date_violation_body()).expect("serializes"),
            r#"{"non_field_errors":["Start date cannot exceed target date"]}"#
        );
        // Equal and missing dates pass validation.
        assert!(validate_module_dates(Some("2026-09-01"), Some("2026-09-01")).is_ok());
        assert!(validate_module_dates(Some("2026-10-01"), None).is_ok());
        assert!(validate_module_dates(None, Some("2026-09-01")).is_ok());
        assert!(validate_module_dates(None, None).is_ok());
        // create duplicate name -> 400 {"error": ...}.
        assert_eq!(
            serde_json::to_string(&duplicate_name_body()).expect("serializes"),
            r#"{"error":"Module with this name already exists"}"#
        );
    }

    /// `member_ids` sync rule (`module.py:65`, `:94-95`): absent means keep,
    /// present (even empty) means replace; representation stringifies ids.
    #[test]
    fn member_sync_rule() {
        assert!(!should_replace_members(&None));
        assert!(should_replace_members(&Some(vec![])));
        assert!(should_replace_members(&Some(vec!["a".to_owned()])));
        assert_eq!(
            member_ids_representation(&["a", "b"]),
            vec!["a".to_owned(), "b".to_owned()]
        );
    }
}
