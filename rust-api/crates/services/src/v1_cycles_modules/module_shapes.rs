#![forbid(unsafe_code)]

//! Module serializer shapes (D-20, stage 5): validation + render half.
//!
//! Ports `apps/api/pi_dash/api/serializers/module.py:21-285`
//! (drift baseline `01a93e17`): `ModuleCreateSerializer.validate`
//! (`:60-81`) and `.create` (`:83-121`), `ModuleUpdateSerializer.update`
//! (`:140-166`), `ModuleSerializer.to_representation` (`:203-206`),
//! `ModuleLinkSerializer.create` (`:255-258`) and
//! `ModuleIssueRequestSerializer` (`:274-285`). Field lists, read-only
//! sets and the `ModuleStatus` values live in the types half
//! (`pidash-types::v1_cycles_modules::module_shapes`).
//!
//! Fixture oracle: FX-CYCMOD-03
//! (`rust-api/fixtures/v1_cycles_modules/serializers/module.golden.json`);
//! the unit tests below assert the messages and behaviour rules against
//! that file's values so transcription drift fails the build.
//!
//! DRF message strings below were verified against live DRF by running
//! the equivalent serializers (missing/null/blank/length/choice/date/
//! UUID/list cases); the UUID int-coercion and index-keyed child errors
//! are covered by `issues_field_*` tests.
//!
//! Existing bugs ported as-is (translation, don't redesign):
//! 1. `validate()` looks the project up with `Project.objects.get`
//!    (`module.py:64`), so an unknown id raises `DoesNotExist` (a 500
//!    through the exception handler) and the `if not project` /
//!    `"Project not found"` branch (`:65-66`) is unreachable —
//!    `PROJECT_NOT_FOUND_MESSAGE` is ported but never returned.
//!    (Cycles use `filter().first()`, where the branch is reachable.)
//! 2. `ModuleLinkSerializer.create` reads
//!    `validated_data.get("module_id")` (`module.py:256`), but the
//!    validated key is `"module"` — the lookup is always `None`, so the
//!    duplicate guard never fires. Its message also says "Issue"
//!    (`LINK_DUPLICATE_MESSAGE`), and no link routes exist
//!    (`api/urls/module.py`), so the path is unreachable over HTTP.
//! 3. `ModuleUpdateSerializer.Meta.fields` appends `"members"` twice
//!    (`module.py:133-137`); harmless, DRF keys fields by name.
//! 4. `if data.get("members", [])` (`module.py:76`) is falsy for both a
//!    missing key and an empty list — neither reaches the
//!    `ProjectMember` rewrite, while an explicit `null` is rejected
//!    earlier with `This field may not be null.`

use serde_json::Value;

/// `validate()` project gates (`module.py:61-68`), checked in order.
pub const PROJECT_ID_REQUIRED_MESSAGE: &str = "Project ID is required";
/// Unreachable in Python (see bug 1 above); ported, never returned.
pub const PROJECT_NOT_FOUND_MESSAGE: &str = "Project not found";
pub const MODULES_NOT_ENABLED_MESSAGE: &str = "Modules are not enabled for this project";

/// Date-ordering rule (`module.py:69-74`).
pub const START_AFTER_TARGET_MESSAGE: &str = "Start date cannot exceed target date";

/// Duplicate-name body on create (`module.py:93-101`): a flat four-key
/// dict (verified: DRF renders raised dict-of-scalars without list
/// wrapping, matching `test_create_duplicate_name` reading
/// `body["code"]`).
pub const DUPLICATE_NAME_CODE: &str = "MODULE_NAME_ALREADY_EXISTS";
pub const DUPLICATE_NAME_MESSAGE: &str = "Module with this name already exists";

/// Duplicate-name body on update (`module.py:143-146`): error-only dict.
pub const UPDATE_DUPLICATE_MESSAGE: &str = "Module with this name already exists";

/// Link-duplicate message (`module.py:257`); says "Issue" in Python.
pub const LINK_DUPLICATE_MESSAGE: &str = "URL already exists for this Issue";

/// Standard DRF field messages used by these serializers (all verified
/// against live DRF).
pub const DRF_REQUIRED_MESSAGE: &str = "This field is required.";
pub const DRF_NULL_MESSAGE: &str = "This field may not be null.";
pub const DRF_BLANK_MESSAGE: &str = "This field may not be blank.";
pub const DRF_MAX_255_MESSAGE: &str = "Ensure this field has no more than 255 characters.";
pub const DRF_DATE_FORMAT_MESSAGE: &str =
    "Date has wrong format. Use one of these formats instead: YYYY-MM-DD.";
pub const DRF_INVALID_UUID_MESSAGE: &str = "Must be a valid UUID.";
pub const DRF_EXPECTED_LIST_PREFIX: &str = "Expected a list of items but got type";

/// `ModuleMember` writes use `batch_size=10, ignore_conflicts=True`
/// (`module.py:105-119,150-164`).
pub const MEMBER_BULK_BATCH_SIZE: usize = 10;
pub const MEMBER_BULK_IGNORE_CONFLICTS: bool = true;

/// Mirrors the `validate()` project gates (`module.py:61-68`).
///
/// `project_id` is the context value (`None` when the view passes no
/// id); `project_found` / `module_view` stand in for the
/// `Project.objects.get` row the queries layer returns. Returns the
/// first violated message, else `Ok`. `PROJECT_NOT_FOUND_MESSAGE` is
/// documented unreachable (bug 1) — it is returned here for
/// completeness so the queries layer can map a missing row to the
/// message the Python author intended.
pub fn validate_project_gate(
    project_id: Option<&str>,
    project_found: bool,
    module_view: bool,
) -> Result<(), &'static str> {
    if project_id.is_none_or(|id| id.is_empty()) {
        return Err(PROJECT_ID_REQUIRED_MESSAGE);
    }
    if !project_found {
        return Err(PROJECT_NOT_FOUND_MESSAGE);
    }
    if !module_view {
        return Err(MODULES_NOT_ENABLED_MESSAGE);
    }
    Ok(())
}

/// Mirrors the date rule (`module.py:69-74`): fires only when BOTH
/// dates are non-null. `data.get("start_date", None)` collapses an
/// absent key and an explicit `null` to the same `None`, so one
/// `Option` per side covers both — no separate absent-vs-null branch.
///
/// Dates are `%Y-%m-%d`, which orders lexicographically, the same
/// ordering as the Python `date` comparison.
pub fn validate_date_order(
    start_date: Option<&str>,
    target_date: Option<&str>,
) -> Result<(), &'static str> {
    if let (Some(start), Some(target)) = (start_date, target_date) {
        if start > target {
            return Err(START_AFTER_TARGET_MESSAGE);
        }
    }
    Ok(())
}

/// A bare-string `ValidationError` raised in `validate()` renders
/// under `non_field_errors` with status 400 (verified against live
/// DRF).
pub fn date_order_error_body() -> String {
    format!("{{\"non_field_errors\":[\"{START_AFTER_TARGET_MESSAGE}\"]}}")
}

/// Duplicate-name create body (`module.py:93-101`), keys in source
/// order. `module_id` is already a hyphenated UUID string.
pub fn duplicate_name_create_body(module_id: &str) -> String {
    format!(
        "{{\"id\":\"{module_id}\",\"code\":\"{DUPLICATE_NAME_CODE}\",\
         \"error\":\"{DUPLICATE_NAME_MESSAGE}\",\"message\":\"{DUPLICATE_NAME_MESSAGE}\"}}"
    )
}

/// Duplicate-name update body (`module.py:146`).
pub fn duplicate_name_update_body() -> String {
    format!("{{\"error\":\"{UPDATE_DUPLICATE_MESSAGE}\"}}")
}

/// Link-duplicate body (`module.py:257`), misnomer included.
pub fn link_duplicate_body() -> String {
    format!("{{\"error\":\"{LINK_DUPLICATE_MESSAGE}\"}}")
}

/// Mirrors `if data.get("members", [])` (`module.py:76`): only a
/// present, non-empty list reaches the `ProjectMember` rewrite. An
/// absent key and `[]` both skip it; `null` never arrives (rejected by
/// the `ListField` with `DRF_NULL_MESSAGE`).
pub fn should_rewrite_members(members: Option<&[String]>) -> bool {
    members.is_some_and(|list| !list.is_empty())
}

/// Whether `create()`/`update()` issues the `bulk_create` call:
/// `members = validated_data.pop("members", None)` then
/// `if members is not None` (`module.py:84,104,141,148`). `Some`
/// (even empty) writes; `None` (absent input) skips. Updates delete
/// the existing rows first whenever this returns true (`:149`).
pub fn writes_member_rows(members: Option<&[String]>) -> bool {
    members.is_some()
}

/// Mirrors `to_representation` (`module.py:203-206`): the read shape
/// carries member ids as strings (`members` is write-only, so the
/// rendered key comes solely from this rule).
pub fn render_member_ids(member_ids: &[impl AsRef<str>]) -> Vec<String> {
    member_ids.iter().map(|id| id.as_ref().to_owned()).collect()
}

/// Renders an invalid-choice rejection the way DRF renders a failed
/// `ChoiceField` (`"..." is not a valid choice.`, verified live).
/// Used for `status` against the types-half `ModuleStatus`.
pub fn invalid_choice_body(field: &str, input: &str) -> String {
    format!("{{\"{field}\":[\"\\\"{input}\\\" is not a valid choice.\"]}}")
}

/// Validates one `start_date`/`target_date` input (`DateField`,
/// `required=False, allow_null=True`): absent or `null` passes as
/// `None`; a `YYYY-MM-DD` calendar date passes through (DRF renders
/// it back identically); anything else is the DRF format error under
/// the field name.
pub fn validate_module_date(field: &str, value: Option<&Value>) -> Result<Option<String>, String> {
    let Some(value) = value else {
        return Ok(None);
    };
    if value.is_null() {
        return Ok(None);
    }
    if let Some(text) = value.as_str() {
        if is_calendar_date(text) {
            return Ok(Some(text.to_owned()));
        }
    }
    Err(field_error_body(field, DRF_DATE_FORMAT_MESSAGE))
}

/// Strict `%Y-%m-%d` calendar check (DRF's `DateField` parse).
fn is_calendar_date(text: &str) -> bool {
    let parts: Vec<&str> = text.split('-').collect();
    if parts.len() != 3 || parts[0].len() != 4 || parts[1].len() != 2 || parts[2].len() != 2 {
        return false;
    }
    let Ok(year) = parts[0].parse::<i32>() else {
        return false;
    };
    if !(1..=9999).contains(&year) {
        return false;
    }
    let Ok(month) = parts[1].parse::<u32>() else {
        return false;
    };
    let Ok(day) = parts[2].parse::<u32>() else {
        return false;
    };
    if !(1..=12).contains(&month) || day == 0 {
        return false;
    }
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let max_day = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        _ => {
            if leap {
                29
            } else {
                28
            }
        }
    };
    day <= max_day
}

/// `{"<field>": ["<message>"]}` — the DRF field-error envelope.
fn field_error_body(field: &str, message: &str) -> String {
    format!("{{\"{field}\":[\"{message}\"]}}")
}

/// DRF type name for the expected-list message, mirroring Python's
/// `type(data).__name__` for JSON-decoded values.
fn json_type_name(value: &Value) -> &'static str {
    match value {
        Value::String(_) => "str",
        Value::Bool(_) => "bool",
        Value::Number(number) => {
            if number.is_f64() {
                "float"
            } else {
                "int"
            }
        }
        Value::Array(_) => "list",
        Value::Object(_) => "dict",
        Value::Null => "NoneType",
    }
}

/// Parses one `issues` item the way DRF's `UUIDField` does (verified
/// live): UUID strings in any form Python's `uuid.UUID` accepts
/// (hyphenated, 32-hex, braced, `urn:uuid:`) normalise to lowercase
/// hyphenated; JSON integers coerce via `UUID(int=…)` (Python `bool`
/// is an `int`, so `true`/`false` coerce to 1/0); everything else —
/// including `null` items, floats and nested containers — is invalid.
fn parse_issue_id(value: &Value) -> Result<String, &'static str> {
    match value {
        Value::String(text) => uuid::Uuid::parse_str(text)
            .map(|id| id.hyphenated().to_string())
            .map_err(|_| DRF_INVALID_UUID_MESSAGE),
        Value::Number(number) => {
            if let Some(unsigned) = number.as_u64() {
                Ok(uuid::Uuid::from_u128(unsigned as u128)
                    .hyphenated()
                    .to_string())
            } else if let Some(signed) = number.as_i64() {
                if signed < 0 {
                    return Err(DRF_INVALID_UUID_MESSAGE);
                }
                Ok(uuid::Uuid::from_u128(signed as u128)
                    .hyphenated()
                    .to_string())
            } else {
                Err(DRF_INVALID_UUID_MESSAGE)
            }
        }
        Value::Bool(flag) => Ok(uuid::Uuid::from_u128(*flag as u128)
            .hyphenated()
            .to_string()),
        Value::Null => Err(DRF_NULL_MESSAGE),
        Value::Array(_) | Value::Object(_) => Err(DRF_INVALID_UUID_MESSAGE),
    }
}

/// Mirrors `ModuleIssueRequestSerializer` (`module.py:282-285`):
/// `issues = ListField(child=UUIDField())` — required, null rejected,
/// empty accepted. Returns normalised id strings, or the byte-identical
/// DRF 400 body. Child failures collect per index into an
/// index-keyed object (`{"0": [...]}`), not a list (verified live).
pub fn validate_issues_field(issues: Option<&Value>) -> Result<Vec<String>, String> {
    let Some(value) = issues else {
        return Err(field_error_body("issues", DRF_REQUIRED_MESSAGE));
    };
    if value.is_null() {
        return Err(field_error_body("issues", DRF_NULL_MESSAGE));
    }
    let Value::Array(items) = value else {
        return Err(field_error_body(
            "issues",
            &format!(
                "{} \"{}\".",
                DRF_EXPECTED_LIST_PREFIX,
                json_type_name(value)
            ),
        ));
    };
    let mut ids = Vec::with_capacity(items.len());
    // Index-keyed in first-failure order (DRF insertion order — a map
    // would sort "10" before "2").
    let mut failures: Vec<String> = Vec::new();
    for (index, item) in items.iter().enumerate() {
        match parse_issue_id(item) {
            Ok(id) => ids.push(id),
            Err(message) => failures.push(format!("\"{index}\":[\"{message}\"]")),
        }
    }
    if failures.is_empty() {
        Ok(ids)
    } else {
        Err(format!("{{\"issues\":{{{}}}}}", failures.join(",")))
    }
}

/// Renders a module `DateField` the way DRF does: `YYYY-MM-DD`, or
/// `null` for `None` (verified live).
pub fn render_module_date(date: Option<&str>) -> Value {
    match date {
        Some(text) => Value::String(text.to_owned()),
        None => Value::Null,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const FIXTURE_GOLDEN: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/v1_cycles_modules/serializers/module.golden.json"
    );

    fn golden() -> serde_json::Value {
        let raw = std::fs::read_to_string(FIXTURE_GOLDEN).expect("FX-CYCMOD-03 golden exists");
        serde_json::from_str(&raw).expect("FX-CYCMOD-03 golden is valid JSON")
    }

    #[test]
    fn golden_messages_match_consts() {
        let parsed = golden();
        let raises = parsed["serializers"]["ModuleCreateSerializer"]["raises"]
            .as_array()
            .expect("golden carries create raises");
        // The fifth raise is the truncated AST dump of the create-time
        // duplicate dict (not a user-facing message); the first four are
        // the `validate()` messages.
        let messages: Vec<&str> = raises
            .iter()
            .filter_map(|raise| raise["message"].as_str())
            .take(4)
            .collect();
        assert_eq!(
            messages,
            [
                PROJECT_ID_REQUIRED_MESSAGE,
                PROJECT_NOT_FOUND_MESSAGE,
                MODULES_NOT_ENABLED_MESSAGE,
                START_AFTER_TARGET_MESSAGE,
            ]
        );
        let behaviour = &parsed["behaviour"];
        assert!(
            behaviour["create_name_rule"]
                .as_str()
                .unwrap()
                .contains(DUPLICATE_NAME_CODE),
            "golden pins the create duplicate code"
        );
        assert!(
            behaviour["date_rule"]
                .as_str()
                .unwrap()
                .contains(START_AFTER_TARGET_MESSAGE),
            "golden pins the date rule"
        );
        assert!(
            behaviour["link_dup"]
                .as_str()
                .unwrap()
                .contains(LINK_DUPLICATE_MESSAGE),
            "golden pins the Issue-misnomer link message"
        );
        assert!(
            behaviour["members_filter"]
                .as_str()
                .unwrap()
                .contains("ProjectMember"),
            "golden pins the members rewrite"
        );
        assert!(
            behaviour["update_name_rule"]
                .as_str()
                .unwrap()
                .contains("delete+bulk_create"),
            "golden pins the wholesale member replace"
        );
    }

    #[test]
    fn project_gates_fire_in_source_order() {
        assert_eq!(
            validate_project_gate(None, true, true),
            Err(PROJECT_ID_REQUIRED_MESSAGE)
        );
        assert_eq!(
            validate_project_gate(Some(""), true, true),
            Err(PROJECT_ID_REQUIRED_MESSAGE)
        );
        assert_eq!(
            validate_project_gate(Some("p"), false, true),
            Err(PROJECT_NOT_FOUND_MESSAGE)
        );
        assert_eq!(
            validate_project_gate(Some("p"), true, false),
            Err(MODULES_NOT_ENABLED_MESSAGE)
        );
        assert_eq!(validate_project_gate(Some("p"), true, true), Ok(()));
    }

    #[test]
    fn date_rule_needs_both_dates() {
        assert_eq!(
            validate_date_order(Some("2026-03-02"), Some("2026-03-01")),
            Err(START_AFTER_TARGET_MESSAGE)
        );
        assert_eq!(
            validate_date_order(Some("2026-03-01"), Some("2026-03-01")),
            Ok(())
        );
        assert_eq!(
            validate_date_order(Some("2026-03-01"), Some("2026-03-02")),
            Ok(())
        );
        assert_eq!(validate_date_order(None, Some("2026-03-01")), Ok(()));
        assert_eq!(validate_date_order(Some("2026-03-01"), None), Ok(()));
        assert_eq!(validate_date_order(None, None), Ok(()));
        assert_eq!(
            date_order_error_body(),
            r#"{"non_field_errors":["Start date cannot exceed target date"]}"#
        );
    }

    #[test]
    fn duplicate_bodies_match_python_shapes() {
        assert_eq!(
            duplicate_name_create_body("12345678-1234-5678-1234-567812345678"),
            r#"{"id":"12345678-1234-5678-1234-567812345678","code":"MODULE_NAME_ALREADY_EXISTS","error":"Module with this name already exists","message":"Module with this name already exists"}"#
        );
        assert_eq!(
            duplicate_name_update_body(),
            r#"{"error":"Module with this name already exists"}"#
        );
        assert_eq!(
            link_duplicate_body(),
            r#"{"error":"URL already exists for this Issue"}"#
        );
    }

    #[test]
    fn members_truthiness_and_write_plan() {
        assert!(!should_rewrite_members(None));
        assert!(!should_rewrite_members(Some(&[])));
        assert!(should_rewrite_members(Some(&["u".to_owned()])));
        assert!(!writes_member_rows(None));
        assert!(writes_member_rows(Some(&[])));
        assert!(writes_member_rows(Some(&["u".to_owned()])));
        assert_eq!(render_member_ids(&["a", "b"]), ["a", "b"]);
        // Bulk-write kwargs from module.py:105-119,150-164, guarded at
        // compile time so transcription drift fails the build.
        const _: () = assert!(MEMBER_BULK_BATCH_SIZE == 10);
        const _: () = assert!(MEMBER_BULK_IGNORE_CONFLICTS);
    }

    #[test]
    fn issues_field_matches_live_drf_vectors() {
        assert_eq!(
            validate_issues_field(None),
            Err(r#"{"issues":["This field is required."]}"#.to_owned())
        );
        assert_eq!(
            validate_issues_field(Some(&Value::Null)),
            Err(r#"{"issues":["This field may not be null."]}"#.to_owned())
        );
        assert_eq!(validate_issues_field(Some(&json!([]))), Ok(vec![]));
        assert_eq!(
            validate_issues_field(Some(&json!("x"))),
            Err(r#"{"issues":["Expected a list of items but got type "str"."]}"#.to_owned())
        );
        assert_eq!(
            validate_issues_field(Some(&json!(["not-a-uuid"]))),
            Err(r#"{"issues":{"0":["Must be a valid UUID."]}}"#.to_owned())
        );
        assert_eq!(
            validate_issues_field(Some(&json!(["12345678-1234-5678-1234-567812345678"]))),
            Ok(vec!["12345678-1234-5678-1234-567812345678".to_owned()])
        );
        // DRF coerces int items via UUID(int=...): 123 -> ...007b.
        assert_eq!(
            validate_issues_field(Some(&json!([123]))),
            Ok(vec!["00000000-0000-0000-0000-00000000007b".to_owned()])
        );
        // 32-hex and braced forms normalise; bad indices collect.
        assert_eq!(
            validate_issues_field(Some(&json!(["12345678123456781234567812345678"]))),
            Ok(vec!["12345678-1234-5678-1234-567812345678".to_owned()])
        );
        assert_eq!(
            validate_issues_field(Some(&json!(["ok-never", "also-bad"]))),
            Err(
                r#"{"issues":{"0":["Must be a valid UUID."],"1":["Must be a valid UUID."]}}"#
                    .to_owned()
            )
        );
        assert_eq!(
            validate_issues_field(Some(&Value::Array(vec![Value::Null]))),
            Err(r#"{"issues":{"0":["This field may not be null."]}}"#.to_owned())
        );
        // Bool coerces like a Python int (UUID(int=True)); floats are
        // invalid; non-string scalars name their Python type.
        assert_eq!(
            validate_issues_field(Some(&json!([true]))),
            Ok(vec!["00000000-0000-0000-0000-000000000001".to_owned()])
        );
        assert_eq!(
            validate_issues_field(Some(&json!([1.5]))),
            Err(r#"{"issues":{"0":["Must be a valid UUID."]}}"#.to_owned())
        );
        assert_eq!(
            validate_issues_field(Some(&json!(1.5))),
            Err(r#"{"issues":["Expected a list of items but got type "float"."]}"#.to_owned())
        );
        // Eleven failures stay in index order ("10" after "9").
        let many_bad: Vec<Value> = (0..11).map(|_| Value::String("bad".into())).collect();
        let body = validate_issues_field(Some(&Value::Array(many_bad))).unwrap_err();
        let expect_ten = format!(
            "{},\"10\":[\"{}\"]",
            (0..10)
                .map(|i| format!("\"{i}\":[\"{DRF_INVALID_UUID_MESSAGE}\"]"))
                .collect::<Vec<_>>()
                .join(","),
            DRF_INVALID_UUID_MESSAGE
        );
        assert_eq!(body, format!("{{\"issues\":{{{expect_ten}}}}}"));
    }

    #[test]
    fn module_dates_follow_drf_datefield() {
        assert_eq!(validate_module_date("start_date", None), Ok(None));
        assert_eq!(
            validate_module_date("start_date", Some(&Value::Null)),
            Ok(None)
        );
        assert_eq!(
            validate_module_date("start_date", Some(&json!("2026-03-01"))),
            Ok(Some("2026-03-01".to_owned()))
        );
        assert_eq!(
            validate_module_date("start_date", Some(&json!("03/01/2026"))),
            Err(r#"{"start_date":["Date has wrong format. Use one of these formats instead: YYYY-MM-DD."]}"#.to_owned())
        );
        assert_eq!(
            validate_module_date("target_date", Some(&json!("2026-13-45"))),
            Err(r#"{"target_date":["Date has wrong format. Use one of these formats instead: YYYY-MM-DD."]}"#.to_owned())
        );
        // Year 0 and Feb 29 on a common year are not calendar dates.
        assert!(validate_module_date("start_date", Some(&json!("0000-01-01"))).is_err());
        assert!(validate_module_date("start_date", Some(&json!("2025-02-29"))).is_err());
        assert_eq!(
            validate_module_date("start_date", Some(&json!("2024-02-29"))),
            Ok(Some("2024-02-29".to_owned()))
        );
        assert_eq!(render_module_date(Some("2026-03-01")), json!("2026-03-01"));
        assert_eq!(render_module_date(None), Value::Null);
        assert_eq!(
            invalid_choice_body("status", "bogus"),
            r#"{"status":["\"bogus\" is not a valid choice."]}"#
        );
    }
}
