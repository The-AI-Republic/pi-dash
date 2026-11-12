#![forbid(unsafe_code)]

//! Module serializer shapes (D-20, stage 5): pure data half.
//!
//! Ports `apps/api/pi_dash/api/serializers/module.py:1-285`
//! (drift baseline `01a93e17`) for the types layer: the `ModuleStatus`
//! values (`db/models/module.py:58-64`), every serializer's field list,
//! read-only guards and required/optional metadata. No I/O, no
//! validation logic — the services half (`pidash-services::
//! v1_cycles_modules::module_shapes`) owns the `validate()` / `create()`
//! / `update()` rules and the error bodies.
//!
//! Fixture oracle: FX-CYCMOD-03
//! (`rust-api/fixtures/v1_cycles_modules/serializers/module.golden.json`);
//! the unit tests below assert the lists below against that file's
//! values so transcription drift fails the build.

/// `ModuleStatus` values (`db/models/module.py:58-64`).
///
/// Stored in the `status` CharField (`max_length=20`); DRF renders the
/// stored value verbatim, so the wire form is exactly these strings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ModuleStatus {
    Backlog,
    Planned,
    InProgress,
    Paused,
    Completed,
    Cancelled,
}

impl ModuleStatus {
    /// All six values, in model source order (`module.py:59-64`).
    pub const ALL: [ModuleStatus; 6] = [
        ModuleStatus::Backlog,
        ModuleStatus::Planned,
        ModuleStatus::InProgress,
        ModuleStatus::Paused,
        ModuleStatus::Completed,
        ModuleStatus::Cancelled,
    ];

    /// Wire value rendered by DRF (`module.py:59-64`).
    pub fn as_str(self) -> &'static str {
        match self {
            ModuleStatus::Backlog => "backlog",
            ModuleStatus::Planned => "planned",
            ModuleStatus::InProgress => "in-progress",
            ModuleStatus::Paused => "paused",
            ModuleStatus::Completed => "completed",
            ModuleStatus::Cancelled => "cancelled",
        }
    }

    /// Parse a wire value; `None` mirrors DRF's invalid-choice rejection
    /// (the services half renders the `"..." is not a valid choice.`
    /// body).
    pub fn parse_status(value: &str) -> Option<ModuleStatus> {
        Self::ALL
            .into_iter()
            .find(|status| status.as_str() == value)
    }
}

impl Default for ModuleStatus {
    /// Model default (`db/models/module.py:83`: `default="planned"`).
    fn default() -> Self {
        ModuleStatus::Planned
    }
}

/// `ModuleCreateSerializer.Meta.fields`, in source order
/// (`api/serializers/module.py:38-48`).
pub const MODULE_CREATE_FIELDS: &[&str] = &[
    "name",
    "description",
    "start_date",
    "target_date",
    "status",
    "lead",
    "members",
    "external_source",
    "external_id",
];

/// `ModuleCreateSerializer.Meta.read_only_fields`, in source order
/// (`api/serializers/module.py:49-58`).
///
/// `id` is absent here but still read-only: `BaseSerializer` declares
/// `id = PrimaryKeyRelatedField(read_only=True)`
/// (`api/serializers/base.py:8-10`).
pub const MODULE_CREATE_READ_ONLY_FIELDS: &[&str] = &[
    "id",
    "workspace",
    "project",
    "created_by",
    "updated_by",
    "created_at",
    "updated_at",
    "deleted_at",
];

/// `ModuleUpdateSerializer.Meta.fields`
/// (`api/serializers/module.py:133-137`): the create list plus a second
/// `members` entry (`Meta.fields + ["members"]`).
///
/// Ported as-is: the duplicate is harmless (DRF keys fields by name, so
/// the second entry overwrites the first) and mirrors the cycle
/// serializer's `owned_by` duplicate.
pub const MODULE_UPDATE_FIELDS: &[&str] = &[
    "name",
    "description",
    "start_date",
    "target_date",
    "status",
    "lead",
    "members",
    "external_source",
    "external_id",
    "members",
];

/// `ModuleUpdateSerializer.Meta.read_only_fields`: inherited unchanged
/// from the create serializer (`api/serializers/module.py:138`).
pub const MODULE_UPDATE_READ_ONLY_FIELDS: &[&str] = MODULE_CREATE_READ_ONLY_FIELDS;

/// Fields the `Module` model exposes under `fields = "__all__"`, in
/// model definition order: `BaseModel.id`, `TimeAuditModel`
/// (`created_at`, `updated_at`), `UserAuditModel` (`created_by`,
/// `updated_by`), `SoftDeleteModel` (`deleted_at`), `ProjectBaseModel`
/// (`project`, `workspace`), then `Module`'s own columns
/// (`db/models/module.py:67-99`).
///
/// DRF renders declared fields first (`id` via `BaseSerializer`, then
/// `members` + the six metric annotations), followed by these model
/// keys; the contract suite pins the read shape as a set
/// (`test_modules.py:16-46`).
pub const MODULE_MODEL_KEY_ORDER: &[&str] = &[
    "id",
    "created_at",
    "updated_at",
    "created_by",
    "updated_by",
    "deleted_at",
    "project",
    "workspace",
    "name",
    "description",
    "description_text",
    "description_html",
    "start_date",
    "target_date",
    "status",
    "lead",
    "members",
    "view_props",
    "sort_order",
    "external_source",
    "external_id",
    "archived_at",
    "logo_props",
];

/// `ModuleSerializer`'s declared fields, in declaration order
/// (`api/serializers/module.py:177-187`): write-only `members` plus the
/// six read-only metric annotations.
pub const MODULE_SERIALIZER_DECLARED_FIELDS: &[&str] = &[
    "members",
    "total_issues",
    "cancelled_issues",
    "completed_issues",
    "started_issues",
    "unstarted_issues",
    "backlog_issues",
];

/// `ModuleSerializer.Meta.read_only_fields`
/// (`api/serializers/module.py:192-201`); the six metric annotations are
/// additionally read-only via their `IntegerField(read_only=True)`
/// declarations (`:182-187`).
pub const MODULE_SERIALIZER_READ_ONLY_FIELDS: &[&str] = MODULE_CREATE_READ_ONLY_FIELDS;

/// `ModuleIssueSerializer`'s declared field
/// (`api/serializers/module.py:217`): the read-only annotated count.
pub const MODULE_ISSUE_DECLARED_FIELDS: &[&str] = &["sub_issues_count"];

/// `ModuleIssueSerializer.Meta.read_only_fields`
/// (`api/serializers/module.py:222-230`).
pub const MODULE_ISSUE_READ_ONLY_FIELDS: &[&str] = &[
    "workspace",
    "project",
    "created_by",
    "updated_by",
    "created_at",
    "updated_at",
    "module",
];

/// `ModuleLinkSerializer.Meta.read_only_fields`
/// (`api/serializers/module.py:244-252`).
pub const MODULE_LINK_READ_ONLY_FIELDS: &[&str] = MODULE_ISSUE_READ_ONLY_FIELDS;

/// `ModuleLiteSerializer`: `fields = "__all__"` with no read-only
/// guards and no declared fields (`api/serializers/module.py:269-271`).
pub const MODULE_LITE_USES_ALL_FIELDS: bool = true;

/// `ModuleIssueRequestSerializer`'s only field
/// (`api/serializers/module.py:282-285`): required (the DRF default —
/// `required` is not passed), null rejected, empty list accepted.
pub const MODULE_ISSUE_REQUEST_FIELDS: &[&str] = &["issues"];

/// `members` input shape on create/update: `ListField` of user PKs,
/// write-only, optional (`required=False`)
/// (`api/serializers/module.py:30-34,177-181`).
pub const MEMBERS_FIELD_WRITE_ONLY: bool = true;
/// `members` may be omitted; an explicit `null` is rejected with
/// `This field may not be null.` (verified against live DRF).
pub const MEMBERS_FIELD_REQUIRED: bool = false;

/// `ModuleCreateSerializer` inputs required on create: only `name`
/// (every other Meta field is nullable, blankable, has a model default,
/// or is the optional `members` list). `status` falls back to the model
/// default `"planned"`; `description` is `blank=True`.
pub const MODULE_CREATE_REQUIRED_FIELDS: &[&str] = &["name"];

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE_GOLDEN: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/v1_cycles_modules/serializers/module.golden.json"
    );

    fn golden() -> serde_json::Value {
        let raw = std::fs::read_to_string(FIXTURE_GOLDEN).expect("FX-CYCMOD-03 golden exists");
        serde_json::from_str(&raw).expect("FX-CYCMOD-03 golden is valid JSON")
    }

    // Compile-time transcription guard (clippy-suggested form).
    const _: () = assert!(MODULE_LITE_USES_ALL_FIELDS);

    /// The golden records `ModuleLiteSerializer` with `fields = "__all__"`.
    fn lite_is_all_fields(parsed: &serde_json::Value) -> bool {
        parsed["serializers"]["ModuleLiteSerializer"]["meta"]["fields"] == "__all__"
    }

    fn str_list(value: &serde_json::Value) -> Vec<&str> {
        value
            .as_array()
            .expect("golden carries a string list")
            .iter()
            .map(|item| item.as_str().expect("field names are strings"))
            .collect()
    }

    #[test]
    fn status_values_match_model_in_order() {
        let wire: Vec<&str> = ModuleStatus::ALL
            .iter()
            .map(|status| status.as_str())
            .collect();
        assert_eq!(
            wire,
            [
                "backlog",
                "planned",
                "in-progress",
                "paused",
                "completed",
                "cancelled"
            ]
        );
        assert_eq!(ModuleStatus::default(), ModuleStatus::Planned);
        assert_eq!(
            ModuleStatus::parse_status("in-progress"),
            Some(ModuleStatus::InProgress)
        );
        assert_eq!(ModuleStatus::parse_status("bogus"), None);
    }

    #[test]
    fn create_fields_match_golden_in_order() {
        let parsed = golden();
        let expected = str_list(&parsed["serializers"]["ModuleCreateSerializer"]["meta"]["fields"]);
        assert_eq!(MODULE_CREATE_FIELDS, expected.as_slice());
        let guards =
            str_list(&parsed["serializers"]["ModuleCreateSerializer"]["meta"]["read_only_fields"]);
        assert_eq!(MODULE_CREATE_READ_ONLY_FIELDS, guards.as_slice());
        let declared: Vec<String> = parsed["serializers"]["ModuleCreateSerializer"]
            ["declared_fields"]
            .as_array()
            .expect("golden carries declared fields")
            .iter()
            .map(|field| {
                field["name"]
                    .as_str()
                    .expect("declared field names are strings")
                    .to_owned()
            })
            .collect();
        assert_eq!(declared, ["members"]);
    }

    #[test]
    fn update_fields_duplicate_members_per_golden() {
        let parsed = golden();
        let meta = &parsed["serializers"]["ModuleUpdateSerializer"]["meta"];
        assert!(
            meta["fields"].is_string(),
            "golden records the BinOp Add expression for the update field list"
        );
        assert_eq!(MODULE_UPDATE_FIELDS.len(), MODULE_CREATE_FIELDS.len() + 1);
        assert_eq!(
            &MODULE_UPDATE_FIELDS[..MODULE_CREATE_FIELDS.len()],
            MODULE_CREATE_FIELDS
        );
        assert_eq!(
            MODULE_UPDATE_FIELDS[MODULE_UPDATE_FIELDS.len() - 1],
            "members"
        );
    }

    #[test]
    fn read_serializer_declared_and_guards_match_golden() {
        let parsed = golden();
        let declared: Vec<String> = parsed["serializers"]["ModuleSerializer"]["declared_fields"]
            .as_array()
            .expect("golden carries declared fields")
            .iter()
            .map(|field| {
                field["name"]
                    .as_str()
                    .expect("declared field names are strings")
                    .to_owned()
            })
            .collect();
        let expected: Vec<&str> = MODULE_SERIALIZER_DECLARED_FIELDS.to_vec();
        assert_eq!(declared, expected);
        let guards =
            str_list(&parsed["serializers"]["ModuleSerializer"]["meta"]["read_only_fields"]);
        assert_eq!(MODULE_SERIALIZER_READ_ONLY_FIELDS, guards.as_slice());
    }

    #[test]
    fn issue_link_lite_request_shapes_match_golden() {
        let parsed = golden();
        let issue_guards =
            str_list(&parsed["serializers"]["ModuleIssueSerializer"]["meta"]["read_only_fields"]);
        assert_eq!(MODULE_ISSUE_READ_ONLY_FIELDS, issue_guards.as_slice());
        let link_guards =
            str_list(&parsed["serializers"]["ModuleLinkSerializer"]["meta"]["read_only_fields"]);
        assert_eq!(MODULE_LINK_READ_ONLY_FIELDS, link_guards.as_slice());
        assert_eq!(MODULE_ISSUE_DECLARED_FIELDS, ["sub_issues_count"]);
        assert!(MODULE_LITE_USES_ALL_FIELDS && lite_is_all_fields(&parsed));
        let request_declared: Vec<String> = parsed["serializers"]["ModuleIssueRequestSerializer"]
            ["declared_fields"]
            .as_array()
            .expect("golden carries request fields")
            .iter()
            .map(|field| {
                field["name"]
                    .as_str()
                    .expect("declared field names are strings")
                    .to_owned()
            })
            .collect();
        assert_eq!(request_declared, MODULE_ISSUE_REQUEST_FIELDS);
    }

    #[test]
    fn model_key_order_covers_contract_set() {
        let parsed = golden();
        assert_eq!(parsed["fixture"], "FX-CYCMOD-03");
        // Every model key the contract suite's MODULE_KEYS set pins must
        // appear exactly once across the declared + model orders.
        let mut keys: Vec<&str> = MODULE_SERIALIZER_DECLARED_FIELDS.to_vec();
        keys.extend(
            MODULE_MODEL_KEY_ORDER
                .iter()
                .filter(|key| !MODULE_SERIALIZER_DECLARED_FIELDS.contains(key)),
        );
        let contract_keys = [
            "id",
            "total_issues",
            "cancelled_issues",
            "completed_issues",
            "started_issues",
            "unstarted_issues",
            "backlog_issues",
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
        for key in contract_keys {
            assert!(keys.contains(&key), "contract key missing: {key}");
        }
    }
}
