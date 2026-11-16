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

// ================================================================
// Part B (PIDASHCONV-313): link / detail / userprops shapes.
//
// Ports `apps/api/pi_dash/app/serializers/module.py:156-280`:
// - `ModuleLinkSerializer` (`:156-203`): `Meta` (`:157-168`),
//   `to_internal_value` (`:170-176`), `validate_url` (`:178-186`),
//   `create` (`:188-192`), `update` (`:194-203`).
// - `ModuleSerializer` (`:206-254`) / `ModuleDetailSerializer`
//   (`:257-273`), including the `DynamicBaseSerializer` field-expansion
//   contract (`app/serializers/base.py:12-201`).
// - `ModuleUserPropertiesSerializer` (`:276-280`); its endpoint PATCH
//   semantics live in `app/views/module/base.py:825-855` (handlers layer)
//   and are mirrored here as pure value rules only.
// Fixtures: `rust-api/fixtures/app_modules/serializers/module_link.golden.json`,
// `rust-api/fixtures/app_modules/serializers/module_userprops.golden.json`,
// `rust-api/fixtures/app_modules/serializers/module_flat_issue_detail.golden.json`
// (`module_serializer` + `module_detail_serializer` sections).
// Every key order below was verified live against the project venv
// (test settings, no DB needed for field construction).
//
// Nested shapes owned elsewhere and referenced by name only:
// `ModuleFlatSerializer` (part A above), `ProjectLiteSerializer`
// (project domain), the `UserLite`/`WorkspaceLite`/… expansion targets
// (their own domains).

/// `ModuleLinkSerializer` fields in wire order (`module.py:156-168`;
/// order verified live: `id` first, then model fields in model order).
/// Matches the contract-test `LINK_KEYS` set
/// (`contract-tests/app_modules/test_module_links.py:19-23`).
pub const LINK_KEY_ORDER: &[&str] = &[
    "id",
    "created_at",
    "updated_at",
    "deleted_at",
    "title",
    "url",
    "metadata",
    "created_by",
    "updated_by",
    "project",
    "workspace",
    "module",
];

/// `ModuleLinkSerializer.Meta.read_only_fields` (`module.py:160-168`).
/// `id` is additionally read-only via `BaseSerializer` (`base.py:8-9`);
/// `deleted_at` is NOT read-only (verified live).
pub const LINK_READ_ONLY_FIELDS: &[&str] = &[
    "workspace",
    "project",
    "created_by",
    "updated_by",
    "created_at",
    "updated_at",
    "module",
];

/// `validate_url` rejection message (`module.py:184`).
pub const INVALID_URL_MESSAGE: &str = "Invalid URL format.";

/// Duplicate-URL rejection on `create` (`module.py:191`).
pub const DUPLICATE_LINK_MESSAGE: &str = "URL already exists.";

/// Duplicate-URL rejection on `update` (`module.py:201`).
/// The word "Issue" is verbatim from the Python source (the model is a
/// module link, not an issue link); ported as-is.
pub const DUPLICATE_LINK_UPDATE_MESSAGE: &str = "URL already exists for this Issue";

/// DRF `URLField` field-level message, surfaced when the (possibly
/// scheme-prefixed) value fails Django validation
/// (`test_module_links.py:57-58` pins `{url: [this]}`).
pub const DRF_INVALID_URL_MESSAGE: &str = "Enter a valid URL.";

/// DRF `URLField` field-level message when `url` is absent
/// (`module.py:172` yields `""`, which is falsy, so no prefix applies and
/// the required check fires).
pub const DRF_URL_REQUIRED_MESSAGE: &str = "This field is required.";

/// Mirror of `to_internal_value`'s scheme step (`module.py:170-176`):
/// a present, non-empty url without an `http://`/`https://` prefix gains
/// an `http://` prefix BEFORE DRF validation. The prefix test is
/// case-sensitive, so e.g. `HTTP://…` is double-prefixed and then fails
/// field validation (verified live). `None` (key absent) and `""` pass
/// through untouched; the required check fires later.
pub fn normalize_link_url(url: Option<&str>) -> Option<String> {
    match url {
        None => None,
        Some("") => Some(String::new()),
        Some(u) if u.starts_with("http://") || u.starts_with("https://") => Some(u.to_owned()),
        Some(u) => Some(format!("http://{u}")),
    }
}

/// Mirror of `validate_url` (`module.py:178-186`): Django's `URLValidator`
/// accepts anything it accepts; `None` and `""` both raise, mapped to the
/// `{"error": …}` dict form (verified live). The full regex verdict stays
/// with Django's validator; this mirror ports the portable edge: missing
/// or empty input is always invalid. That is also why a PATCH omitting
/// `url` fails: `update` validates unconditionally (`:194-195`).
pub fn validate_link_url_present(url: Option<&str>) -> Result<(), Value> {
    match url {
        Some(u) if !u.is_empty() => Ok(()),
        _ => Err(invalid_url_body()),
    }
}

/// Wire body for the `validate_url` rejection: raised as
/// `ValidationError({"error": …})`, so the body is the dict itself
/// (`module.py:184`; `test_module_links.py:96-105` pins the PATCH case).
pub fn invalid_url_body() -> Value {
    Value::Object(
        [(
            "error".to_owned(),
            Value::String(INVALID_URL_MESSAGE.to_owned()),
        )]
        .into_iter()
        .collect(),
    )
}

/// Wire body for the `create` duplicate-url rejection (`module.py:191`).
pub fn duplicate_link_body() -> Value {
    Value::Object(
        [(
            "error".to_owned(),
            Value::String(DUPLICATE_LINK_MESSAGE.to_owned()),
        )]
        .into_iter()
        .collect(),
    )
}

/// Wire body for the `update` duplicate-url rejection (`module.py:201`).
pub fn duplicate_link_update_body() -> Value {
    Value::Object(
        [(
            "error".to_owned(),
            Value::String(DUPLICATE_LINK_UPDATE_MESSAGE.to_owned()),
        )]
        .into_iter()
        .collect(),
    )
}

/// DRF field-level error envelope for the `url` field: `{url: [...]}`.
/// Used for post-prefix `URLField` failures (`invalid`) and the absent-url
/// `required` failure, which `create`/`update` never rewrite.
pub fn link_field_error_body(messages: &[&str]) -> Value {
    Value::Object(
        [(
            "url".to_owned(),
            Value::Array(
                messages
                    .iter()
                    .map(|m| Value::String((*m).to_owned()))
                    .collect(),
            ),
        )]
        .into_iter()
        .collect(),
    )
}

/// `ModuleSerializer` fields in `Meta.fields` order (`module.py:220-253`;
/// order verified live). This is the render order for list (`base.py:357`)
/// and retrieve shells; the annotated `.values()` row (`MODULE_ROW_KEYS`,
/// contract conftest) belongs to the handlers layer.
pub const MODULE_LIST_FIELD_ORDER: &[&str] = &[
    "id",
    "workspace_id",
    "project_id",
    "name",
    "description",
    "description_text",
    "description_html",
    "start_date",
    "target_date",
    "status",
    "lead_id",
    "member_ids",
    "view_props",
    "sort_order",
    "external_source",
    "external_id",
    "logo_props",
    "total_estimate_points",
    "completed_estimate_points",
    "is_favorite",
    "total_issues",
    "cancelled_issues",
    "completed_issues",
    "started_issues",
    "unstarted_issues",
    "backlog_issues",
    "created_at",
    "updated_at",
    "archived_at",
];

/// `ModuleDetailSerializer` extra fields appended after the list fields
/// (`module.py:266-273`; order verified live).
pub const MODULE_DETAIL_EXTRA_FIELDS: &[&str] = &[
    "link_module",
    "sub_issues",
    "backlog_estimate_points",
    "unstarted_estimate_points",
    "started_estimate_points",
    "cancelled_estimate_points",
];

/// The only writable fields on `ModuleSerializer`/`ModuleDetailSerializer`
/// (verified live). `Meta.read_only_fields = fields` (`module.py:254`)
/// does NOT override the explicitly declared `member_ids`
/// (`ListField(UUIDField, required=False, allow_null)`); every other field
/// is read-only.
pub const MODULE_WRITABLE_FIELDS: &[&str] = &["member_ids"];

/// Whether the `fields=` constructor kwarg is honored by
/// `DynamicBaseSerializer` (`base.py:13-19`). It is not: the popped value
/// is immediately overwritten with `self.expand`, so the list endpoint's
/// `ModuleSerializer(queryset, many=True, fields=self.fields)`
/// (`app/views/module/base.py:357`) always renders the full 29-field
/// shape.
pub const DYNAMIC_FIELDS_KWARG_HONORED: bool = false;

/// `_filter_fields` expansion targets (`base.py:74-96`): names that may be
/// appended to the field set at init when requested via `expand=`.
/// `issue_attachment` is absent here (present only in the render map
/// below); ported as-is.
pub const FILTER_EXPANSION_KEYS: &[&str] = &[
    "user",
    "workspace",
    "project",
    "default_assignee",
    "project_lead",
    "state",
    "created_by",
    "issue",
    "actor",
    "owned_by",
    "members",
    "assignees",
    "labels",
    "issue_cycle",
    "parent",
    "issue_relation",
    "issue_intake",
    "issue_related",
    "issue_reactions",
    "issue_link",
    "sub_issues",
];

/// `to_representation` expansion targets (`base.py:148-171`). Same keys as
/// [`FILTER_EXPANSION_KEYS`] plus `issue_attachment`; ported as-is.
pub const REPRESENTATION_EXPANSION_KEYS: &[&str] = &[
    "user",
    "workspace",
    "project",
    "default_assignee",
    "project_lead",
    "state",
    "created_by",
    "issue",
    "actor",
    "owned_by",
    "members",
    "assignees",
    "labels",
    "issue_cycle",
    "parent",
    "issue_relation",
    "issue_intake",
    "issue_related",
    "issue_reactions",
    "issue_attachment",
    "issue_link",
    "sub_issues",
];

/// `_filter_fields` init-time `many=` names (`base.py:100-116`).
/// `issue_attachment` is listed although the init map above has no such
/// key (dead entry); ported as-is.
pub const FILTER_EXPANSION_MANY: &[&str] = &[
    "members",
    "assignees",
    "labels",
    "issue_cycle",
    "issue_relation",
    "issue_intake",
    "issue_reactions",
    "issue_attachment",
    "issue_link",
    "sub_issues",
    "issue_related",
];

/// Render-time outcome of one `expand` name in `to_representation`
/// (`base.py:126-181`).
pub enum ExpandRender {
    /// In fields and in the expansion map, rendered value is a list.
    NestedMany,
    /// In fields and in the expansion map, rendered value is not a list.
    NestedOne,
    /// In fields but not expandable: overwritten with
    /// `getattr(instance, f"{name}_id", None)`.
    IdFallback,
    /// Not a serializer field: response entry left untouched.
    Unchanged,
}

/// Mirror of the `to_representation` expand loop (`base.py:126-181`):
/// names outside the field set are ignored; mapped names render through
/// their lite serializer (`many` follows the rendered value's runtime
/// type, unlike the init-time name set); unmapped field names fall back
/// to the `<name>_id` attribute. No module view passes `expand=`, so this
/// path is latent for this domain.
pub fn resolve_expand_render(name: &str, in_fields: bool, value_is_list: bool) -> ExpandRender {
    if !in_fields {
        return ExpandRender::Unchanged;
    }
    if REPRESENTATION_EXPANSION_KEYS.contains(&name) {
        if value_is_list {
            return ExpandRender::NestedMany;
        }
        return ExpandRender::NestedOne;
    }
    ExpandRender::IdFallback
}

/// Mirror of the `_filter_fields` init append (`base.py:98-118`):
/// returns `Some(many)` when `expand=` names a field the serializer does
/// not already have and the init map provides it (`many` from
/// [`FILTER_EXPANSION_MANY`]); otherwise `None` (field set unchanged —
/// init never removes fields).
pub fn init_expand_append(name: &str, already_in_fields: bool) -> Option<bool> {
    if already_in_fields || !FILTER_EXPANSION_KEYS.contains(&name) {
        return None;
    }
    Some(FILTER_EXPANSION_MANY.contains(&name))
}

/// `ModuleUserPropertiesSerializer` fields in wire order
/// (`module.py:276-280`; order verified live: `id` first, then model
/// fields in model order). Matches the contract-test `PROPS_KEYS` set
/// (`test_favorites_properties.py:20-24`).
pub const USERPROPS_KEY_ORDER: &[&str] = &[
    "id",
    "created_at",
    "updated_at",
    "deleted_at",
    "filters",
    "display_filters",
    "display_properties",
    "rich_filters",
    "created_by",
    "updated_by",
    "project",
    "workspace",
    "module",
    "user",
];

/// `ModuleUserPropertiesSerializer.Meta.read_only_fields`
/// (`module.py:280`). `id`/`created_at`/`updated_at` are additionally
/// read-only via `BaseSerializer`/DRF mapping; `deleted_at` is NOT
/// read-only (verified live).
pub const USERPROPS_READ_ONLY_FIELDS: &[&str] = &["workspace", "project", "module", "user"];

/// PATCH on the user-properties endpoint answers 201, not 200
/// (`app/views/module/base.py:844`; suite pins
/// `test_favorites_properties.py:124-131`).
pub const USERPROPS_PATCH_STATUS: u16 = 201;

/// Mirror of the PATCH merge step (`app/views/module/base.py:835-841`):
/// each of `filters`/`rich_filters`/`display_filters`/`display_properties`
/// is replaced wholesale by the request value when the key is present,
/// otherwise the stored value is kept (no deep merge).
pub fn userprops_patch_value<'a>(requested: Option<&'a Value>, stored: &'a Value) -> &'a Value {
    requested.unwrap_or(stored)
}

#[cfg(test)]
mod part_b_tests {
    use super::*;
    use serde_json::{json, Value};
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

    /// Link wire order carries the contract-test `LINK_KEYS` set and the
    /// exact live order (`module.py:156-168`).
    #[test]
    fn link_key_order_matches_contract() {
        let contract: BTreeSet<String> = [
            "id",
            "created_at",
            "updated_at",
            "deleted_at",
            "title",
            "url",
            "metadata",
            "created_by",
            "updated_by",
            "project",
            "workspace",
            "module",
        ]
        .iter()
        .map(|k| k.to_string())
        .collect();
        assert_eq!(set(LINK_KEY_ORDER), contract);
        assert_eq!(
            LINK_KEY_ORDER,
            &[
                "id",
                "created_at",
                "updated_at",
                "deleted_at",
                "title",
                "url",
                "metadata",
                "created_by",
                "updated_by",
                "project",
                "workspace",
                "module"
            ]
        );
    }

    /// Fixture replay: read-only set + trace
    /// (`module_link.golden.json`).
    #[test]
    fn link_read_only_matches_fixture() {
        let fixture = golden("module_link.golden.json");
        let ro: Vec<String> = fixture
            .get("read_only")
            .expect("link read_only")
            .as_array()
            .expect("array")
            .iter()
            .map(|v| v.as_str().expect("str").to_owned())
            .collect();
        let expected: Vec<String> = LINK_READ_ONLY_FIELDS
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert_eq!(expected, ro);
        assert!(fixture
            .get("trace")
            .and_then(Value::as_str)
            .expect("trace")
            .contains("app/serializers/module.py:156-203"));
    }

    /// Fixture replay: the five link cases
    /// (`module_link.golden.json` `cases`).
    #[test]
    fn link_cases_replay() {
        // create ok: bare domain gains the prefix before validation.
        assert_eq!(
            normalize_link_url(Some("example.com/docs")),
            Some("http://example.com/docs".to_owned())
        );
        // https URLs pass through untouched.
        assert_eq!(
            normalize_link_url(Some("https://example.com/specs")),
            Some("https://example.com/specs".to_owned())
        );
        // Ported quirk: the prefix test is case-sensitive, so an
        // uppercase scheme is double-prefixed and then fails DRF field
        // validation (verified live).
        assert_eq!(
            normalize_link_url(Some("HTTP://x.com/a")),
            Some("http://HTTP://x.com/a".to_owned())
        );
        assert_eq!(
            serde_json::to_string(&link_field_error_body(&[DRF_INVALID_URL_MESSAGE]))
                .expect("serializes"),
            r#"{"url":["Enter a valid URL."]}"#
        );
        // Missing/empty urls fail presence validation with the dict body.
        assert!(validate_link_url_present(None).is_err());
        assert!(validate_link_url_present(Some("")).is_err());
        assert!(validate_link_url_present(Some("http://example.com/x")).is_ok());
        assert_eq!(
            serde_json::to_string(&invalid_url_body()).expect("serializes"),
            r#"{"error":"Invalid URL format."}"#
        );
        // Absent url key stays absent through normalization (the DRF
        // required check fires later).
        assert_eq!(normalize_link_url(None), None);
        assert_eq!(
            serde_json::to_string(&link_field_error_body(&[DRF_URL_REQUIRED_MESSAGE]))
                .expect("serializes"),
            r#"{"url":["This field is required."]}"#
        );
        // Duplicate bodies on create vs update (note the verbatim
        // "Issue" on update).
        assert_eq!(
            serde_json::to_string(&duplicate_link_body()).expect("serializes"),
            r#"{"error":"URL already exists."}"#
        );
        assert_eq!(
            serde_json::to_string(&duplicate_link_update_body()).expect("serializes"),
            r#"{"error":"URL already exists for this Issue"}"#
        );
    }

    /// List order matches the fixture's `fields_in_order` exactly
    /// (`module_flat_issue_detail.golden.json` `module_serializer`).
    #[test]
    fn list_order_matches_fixture() {
        let fixture = golden("module_flat_issue_detail.golden.json");
        let order: Vec<String> = fixture
            .get("module_serializer")
            .expect("module_serializer section")
            .get("fields_in_order")
            .expect("fields_in_order")
            .as_array()
            .expect("array")
            .iter()
            .map(|v| v.as_str().expect("str").to_owned())
            .collect();
        let expected: Vec<String> = MODULE_LIST_FIELD_ORDER
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert_eq!(expected, order);
        assert_eq!(MODULE_LIST_FIELD_ORDER.len(), 29);
    }

    /// Detail is the list order plus the six extras, in live order
    /// (`module.py:266-273`).
    #[test]
    fn detail_order_is_list_plus_extras() {
        let mut expected: Vec<&str> = MODULE_LIST_FIELD_ORDER.to_vec();
        expected.extend_from_slice(MODULE_DETAIL_EXTRA_FIELDS);
        assert_eq!(expected.len(), 35);
        assert_eq!(
            MODULE_DETAIL_EXTRA_FIELDS,
            &[
                "link_module",
                "sub_issues",
                "backlog_estimate_points",
                "unstarted_estimate_points",
                "started_estimate_points",
                "cancelled_estimate_points"
            ]
        );
        let fixture = golden("module_flat_issue_detail.golden.json");
        let detail = fixture
            .get("module_detail_serializer")
            .expect("detail section");
        assert_eq!(
            detail
                .get("extends")
                .and_then(Value::as_str)
                .expect("extends"),
            "ModuleSerializer"
        );
        for extra in MODULE_DETAIL_EXTRA_FIELDS {
            let found = detail
                .get("extra_fields")
                .and_then(Value::as_array)
                .expect("extra_fields")
                .iter()
                .any(|v| v.as_str().expect("str").starts_with(extra));
            assert!(found, "fixture pins {extra}");
        }
    }

    /// Only `member_ids` is writable on the list/detail shapes: the
    /// declared `ListField` beats `Meta.read_only_fields = fields`
    /// (verified live on both serializers).
    #[test]
    fn only_member_ids_writable() {
        assert_eq!(MODULE_WRITABLE_FIELDS, &["member_ids"]);
    }

    /// `fields=` is dead and init only appends (`base.py:13-24`,
    /// `:26-120`): unknown or already-present names change nothing, mapped
    /// names append with the init-time `many` flag.
    #[test]
    fn dynamic_init_rules() {
        const _: () = assert!(!DYNAMIC_FIELDS_KWARG_HONORED);
        // `lead` is not a ModuleSerializer field and not expandable.
        assert_eq!(init_expand_append("lead", false), None);
        // Already-present fields are untouched.
        assert_eq!(init_expand_append("members", true), None);
        // Mapped names append; `members` is many, `issue` is one.
        assert_eq!(init_expand_append("members", false), Some(true));
        assert_eq!(init_expand_append("issue", false), Some(false));
        // `issue_attachment` is in the init `many` set but has no init map
        // entry, so it never appends (ported as-is).
        assert_eq!(init_expand_append("issue_attachment", false), None);
        // Init map lacks `issue_attachment` while the render map has it.
        assert!(!FILTER_EXPANSION_KEYS.contains(&"issue_attachment"));
        assert!(REPRESENTATION_EXPANSION_KEYS.contains(&"issue_attachment"));
        assert_eq!(FILTER_EXPANSION_KEYS.len(), 21);
        assert_eq!(REPRESENTATION_EXPANSION_KEYS.len(), 22);
    }

    /// Render-time expand branches (`base.py:126-181`).
    #[test]
    fn expand_render_branches() {
        // Outside the field set: untouched.
        assert!(matches!(
            resolve_expand_render("lead", false, false),
            ExpandRender::Unchanged
        ));
        // Mapped: many follows the rendered value's runtime type.
        assert!(matches!(
            resolve_expand_render("members", true, true),
            ExpandRender::NestedMany
        ));
        assert!(matches!(
            resolve_expand_render("members", true, false),
            ExpandRender::NestedOne
        ));
        assert!(matches!(
            resolve_expand_render("issue", true, false),
            ExpandRender::NestedOne
        ));
        // In fields but unmapped: `<name>_id` fallback.
        assert!(matches!(
            resolve_expand_render("name", true, false),
            ExpandRender::IdFallback
        ));
    }

    /// Userprops wire order carries the contract-test `PROPS_KEYS` set and
    /// the exact live order (`module.py:276-280`).
    #[test]
    fn userprops_order_matches_contract() {
        let contract: BTreeSet<String> = [
            "id",
            "created_at",
            "updated_at",
            "deleted_at",
            "filters",
            "display_filters",
            "display_properties",
            "rich_filters",
            "created_by",
            "updated_by",
            "project",
            "workspace",
            "module",
            "user",
        ]
        .iter()
        .map(|k| k.to_string())
        .collect();
        assert_eq!(set(USERPROPS_KEY_ORDER), contract);
        let fixture = golden("module_userprops.golden.json");
        let rows: Vec<String> = fixture
            .get("row_keys")
            .expect("row_keys")
            .as_array()
            .expect("array")
            .iter()
            .map(|v| v.as_str().expect("str").to_owned())
            .collect();
        let expected: Vec<String> = USERPROPS_KEY_ORDER.iter().map(|s| s.to_string()).collect();
        assert_eq!(expected, rows);
        let ro: Vec<String> = fixture
            .get("read_only")
            .expect("userprops read_only")
            .as_array()
            .expect("array")
            .iter()
            .map(|v| v.as_str().expect("str").to_owned())
            .collect();
        let expected_ro: Vec<String> = USERPROPS_READ_ONLY_FIELDS
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert_eq!(expected_ro, ro);
    }

    /// Fixture replay: defaults present, PATCH replaces wholesale per key
    /// and answers 201 (`module_userprops.golden.json` `cases`).
    #[test]
    fn userprops_patch_replay() {
        let fixture = golden("module_userprops.golden.json");
        let defaults = fixture.get("defaults").expect("defaults");
        for key in ["filters", "display_filters", "display_properties"] {
            assert!(defaults.get(key).is_some(), "default {key} pinned");
        }
        assert_eq!(
            defaults.get("rich_filters").expect("rich_filters default"),
            &json!({})
        );
        assert_eq!(USERPROPS_PATCH_STATUS, 201);
        let stored = json!({"priority": Value::Null, "state": Value::Null});
        let requested = json!({"priority": "high"});
        // Key present: replaced wholesale, not deep-merged.
        assert_eq!(userprops_patch_value(Some(&requested), &stored), &requested);
        // Key absent: stored value kept.
        assert_eq!(userprops_patch_value(None, &stored), &stored);
    }
}
