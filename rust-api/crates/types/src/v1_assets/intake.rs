//! Intake serializers: pure shapes + validation rules (D-21, stage 5).
//!
//! Port of `apps/api/pi_dash/api/serializers/intake.py:12-170`:
//!
//! * `IssueForIntakeSerializer` (`:12-40`) → [`ISSUE_FOR_INTAKE_FIELDS`] +
//!   [`IssueForIntakeView`]. `description` is an explicit `JSONField` with
//!   `source="description_json"`, `required=False`, `allow_null=True` (`:20`);
//!   `description_json` is also an auto model field (`:24-30`), so both keys
//!   render the same content (see BUG-dual-key below). The view holds one
//!   source and emits both keys, so the two cannot diverge.
//! * `IntakeIssueCreateSerializer` (`:42-55`) → [`CREATE_FIELDS`] +
//!   [`IntakeIssueCreateView`]. Nested `issue` is required (no
//!   `required=False`).
//! * `IntakeIssueSerializer` (`:57-82`) → [`IntakeIssueView`] with
//!   `issue_detail` (`IssueExpandSerializer`, `source="issue"`, read-only,
//!   `:62`) and `inbox` (`UUIDField`, `source="intake.id"`, read-only,
//!   `:63`). `Meta.fields` is `__all__` (`:66`); the read-only list is
//!   [`INTAKE_ISSUE_READ_ONLY_FIELDS`] (`:67-78`). The nested
//!   `IssueExpandSerializer` belongs to another layer, so `issue_detail` is
//!   an opaque `serde_json::Value` passthrough (`None` renders `null`).
//!   `inbox` renders the parent `Intake` row id, not the `IntakeIssue` id.
//! * `IntakeIssueUpdateSerializer` (`:83-158`) → [`UPDATE_FIELDS`] +
//!   [`validate_accept`] + [`should_transition_on_update`] +
//!   [`resolve_triage_transition`]. Nested `issue` has `required=False`
//!   (`:94`).
//! * `IssueDataSerializer` (`:160-170`) → [`validate_issue_name`] +
//!   [`validate_description_html`] + [`validate_priority`]. Plain
//!   `serializers.Serializer` (no `Meta`/model).
//!
//! Fixture id replayed by the unit tests alongside this module:
//! `rust-api/fixtures/v1_assets/fx-ser-intake.json` (PIDASHCONV-375).
//! Supporting sources: `Issue.PRIORITY_CHOICES`
//! (`db/models/issue.py:108-114`), `StateGroup`
//! (`db/models/state.py:14-22`, `TRIAGE = "triage"`), `IntakeIssueStatus`
//! (`db/models/intake.py:42-47`).
//!
//! Layering notes (license-domain precedent):
//!
//! * Datetimes cross this boundary already rendered as DRF `iso-8601`
//!   strings and ids as strings (borrowed `&str`); rendering owns to the
//!   handlers layer. The DB lookups inside `validate`/`update` (the
//!   `State.objects.filter(workspace, project, default=True).first()`
//!   queries at `:125-127` and `:149-151`) live outside the types crate
//!   (crate graph `types -> db -> services -> api`), so the pure rules take
//!   their outcomes as arguments: `default_state_exists` /
//!   `default_state_id`. The branch conditions themselves live here,
//!   exactly as in Python.
//! * `validate_accept` takes the already-validated status as `Option<i32>`.
//!   The fixture records the guard as int-strict (`attrs.get("status") == 1`):
//!   a non-integer status never reaches the guard as `1`, so only
//!   `Some(1)` can fire it.
//! * DRF renders a `ValidationError({"status": msg})` body as
//!   `{"status": [msg]}` on the wire; the list wrapping owns to the
//!   handlers edge. The fixture pins the message payload as
//!   `{"status": "<msg>"}`, and [`ACCEPT_GUARD_MESSAGE`] is that payload.
//!
//! Ported bugs (translate, don't redesign):
//!
//! * BUG-dual-key (`intake.py:20` + `:24-30`): `IssueForIntakeSerializer`
//!   emits BOTH `description` (explicit field, `source="description_json"`)
//!   and `description_json` (auto model field) with identical content.
//!   [`IssueForIntakeView`] renders both keys from the single source.
//! * BUG-wire-dead (`views/intake.py:22-27`): `IssueDataSerializer` is
//!   imported nowhere in the views; the create path validates
//!   name/priority inline (`views/intake.py:148-170`), so its
//!   max-length/choices rules never fire on the wire. The shape and rules
//!   are ported here for the layer; the handler goldens own the wire
//!   behaviour.

use serde::ser::SerializeStruct;
use serde::Serialize;
use serde_json::Value;

/// `IssueForIntakeSerializer.Meta.fields` order (`intake.py:24-30`).
pub const ISSUE_FOR_INTAKE_FIELDS: [&str; 5] = [
    "name",
    "description",
    "description_json",
    "description_html",
    "priority",
];

/// `IssueForIntakeSerializer.Meta.read_only_fields` (`intake.py:31-39`).
/// Write-constraining only; recorded for the handlers layer.
pub const ISSUE_FOR_INTAKE_READ_ONLY_FIELDS: [&str; 7] = [
    "id",
    "workspace",
    "project",
    "created_by",
    "updated_by",
    "created_at",
    "updated_at",
];

/// `IntakeIssueCreateSerializer.Meta.fields` (`intake.py:53-54`).
pub const CREATE_FIELDS: [&str; 1] = ["issue"];

/// `IntakeIssueSerializer.Meta.read_only_fields` (`intake.py:67-78`).
pub const INTAKE_ISSUE_READ_ONLY_FIELDS: [&str; 8] = [
    "id",
    "workspace",
    "project",
    "issue",
    "created_by",
    "updated_by",
    "created_at",
    "updated_at",
];

/// `IntakeIssueUpdateSerializer.Meta.fields` order (`intake.py:99-106`).
pub const UPDATE_FIELDS: [&str; 6] = [
    "status",
    "snoozed_till",
    "duplicate_to",
    "source",
    "source_email",
    "issue",
];

/// `IntakeIssueUpdateSerializer.Meta.read_only_fields` (`intake.py:107-115`).
pub const UPDATE_READ_ONLY_FIELDS: [&str; 7] = [
    "id",
    "workspace",
    "project",
    "created_by",
    "updated_by",
    "created_at",
    "updated_at",
];

/// `IntakeIssueStatus` values (`db/models/intake.py:42-47`).
pub const STATUS_PENDING: i32 = -2;
/// `IntakeIssueStatus.REJECTED` (`db/models/intake.py:44`).
pub const STATUS_REJECTED: i32 = -1;
/// `IntakeIssueStatus.SNOOZED` (`db/models/intake.py:45`).
pub const STATUS_SNOOZED: i32 = 0;
/// `IntakeIssueStatus.ACCEPTED` (`db/models/intake.py:46`) — the only status
/// the accept guard and the triage transition care about.
pub const STATUS_ACCEPTED: i32 = 1;
/// `IntakeIssueStatus.DUPLICATE` (`db/models/intake.py:47`).
pub const STATUS_DUPLICATE: i32 = 2;

/// `StateGroup.TRIAGE.value` (`db/models/state.py:22`).
pub const STATE_GROUP_TRIAGE: &str = "triage";

/// `IntakeIssueUpdateSerializer.validate` failure
/// (`intake.py:130-133`; fixture `accept_guard.error_body`).
pub const ACCEPT_GUARD_MESSAGE: &str =
    "Cannot accept intake issue: No default state found for the project";

/// `Issue.PRIORITY_CHOICES` values (`db/models/issue.py:108-114`).
pub const PRIORITY_CHOICES: [&str; 5] = ["urgent", "high", "medium", "low", "none"];
/// `IssueDataSerializer.priority` default (`intake.py:169`).
pub const PRIORITY_DEFAULT: &str = "none";

/// `IssueDataSerializer.name` column cap (`intake.py:166`).
pub const ISSUE_NAME_MAX_LENGTH: usize = 255;

/// DRF `required` failure (django-rest-framework 3.15 `fields.py`).
pub const REQUIRED_MESSAGE: &str = "This field is required.";
/// DRF `null` failure.
pub const NULL_MESSAGE: &str = "This field may not be null.";
/// DRF `blank` failure (`CharField`, `allow_blank=False` default).
pub const BLANK_MESSAGE: &str = "This field may not be blank.";
/// DRF `max_length` failure (`intake.py:166`, `max_length=255`).
pub const NAME_MAX_LENGTH_MESSAGE: &str = "Ensure this field has no more than 255 characters.";

/// DRF `invalid_choice` failure shape (`ChoiceField`, `intake.py:169`):
/// `"\"{input}\" is not a valid choice."`.
pub fn invalid_priority_choice(input: &str) -> String {
    format!("\"{input}\" is not a valid choice.")
}

/// `IssueForIntakeSerializer` output shape (`intake.py:12-40`).
///
/// `description` and `description_json` render the same content by
/// construction (BUG-dual-key): the single `description_source` is emitted
/// under both keys, in `Meta.fields` order. `None` renders `null` under
/// both keys (`allow_null=True`); the key is always present.
pub struct IssueForIntakeView<'a> {
    /// `Issue.name`.
    pub name: Option<&'a str>,
    /// `Issue.description_json` — emitted as both `description` and
    /// `description_json`.
    pub description_source: Option<Value>,
    /// `Issue.description_html`.
    pub description_html: Option<&'a str>,
    /// `Issue.priority`.
    pub priority: &'a str,
}

impl Serialize for IssueForIntakeView<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut out = serializer.serialize_struct("IssueForIntakeView", 5)?;
        match self.name {
            Some(name) => out.serialize_field("name", name)?,
            None => out.serialize_field("name", &Value::Null)?,
        }
        let description = self.description_source.clone().unwrap_or(Value::Null);
        // BUG-dual-key: explicit `description` field (`source="description_json"`)
        // and the auto `description_json` model field render identically.
        out.serialize_field("description", &description)?;
        out.serialize_field("description_json", &description)?;
        match self.description_html {
            Some(html) => out.serialize_field("description_html", html)?,
            None => out.serialize_field("description_html", &Value::Null)?,
        }
        out.serialize_field("priority", self.priority)?;
        out.end()
    }
}

/// `IntakeIssueCreateSerializer` input shape (`intake.py:42-55`): exactly
/// one key, `issue`, required (no `required=False`).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct IntakeIssueCreateView {
    /// Nested `IssueForIntakeSerializer` payload.
    pub issue: Value,
}

/// `IntakeIssueSerializer` output shape (`intake.py:57-82`).
///
/// `Meta.fields` is `__all__`: `[pk] + declared (base-first) + concrete
/// fields + forward relations in model order` (DRF `ModelSerializer`
/// mechanics, probed for the sibling app-intake port). `issue_detail` is
/// the nested `IssueExpandSerializer` rendering of `issue` (opaque to this
/// layer; `None` renders `null`); `inbox` is the parent `Intake` row id.
/// Plain FKs render as pk strings (`PrimaryKeyRelatedField`, read-only).
/// Datetimes are pre-rendered DRF strings; ids are strings.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct IntakeIssueView<'a> {
    /// `BaseSerializer.id` (`serializers/base.py:11`).
    pub id: &'a str,
    /// Declared first (`:62`): `IssueExpandSerializer(source="issue")`.
    pub issue_detail: Option<Value>,
    /// Declared second (`:63`): parent `intake.id`, not this row's id.
    pub inbox: &'a str,
    pub created_at: &'a str,
    pub updated_at: &'a str,
    pub deleted_at: Option<&'a str>,
    pub created_by: Option<&'a str>,
    pub updated_by: Option<&'a str>,
    pub workspace: &'a str,
    pub project: &'a str,
    /// `IntakeIssue.intake` FK (writable in Python; not in `read_only_fields`).
    pub intake: &'a str,
    /// `IntakeIssue.issue` FK (read-only, `:71`).
    pub issue: &'a str,
    pub status: i32,
    pub snoozed_till: Option<&'a str>,
    pub duplicate_to: Option<&'a str>,
    pub source: Option<&'a str>,
    pub source_email: Option<&'a str>,
    pub external_source: Option<&'a str>,
    pub external_id: Option<&'a str>,
    pub extra: Value,
}

/// `IntakeIssueUpdateSerializer.validate` (`intake.py:117-136`).
///
/// Fires if and only if ALL hold: the validated `status` is `1`
/// (`attrs.get("status") == 1`, `:120`), the linked issue has a state
/// (`:125`), that state's group is `triage` (`:125`), and no default
/// `State` row exists for the workspace+project (`:125-129`). Any other
/// combination is valid. `default_state_exists` / `issue_state_group` are
/// the outcomes of the caller's DB reads.
pub fn validate_accept(
    status: Option<i32>,
    issue_state_group: Option<&str>,
    default_state_exists: bool,
) -> Result<(), &'static str> {
    if status == Some(STATUS_ACCEPTED)
        && issue_state_group == Some(STATE_GROUP_TRIAGE)
        && !default_state_exists
    {
        return Err(ACCEPT_GUARD_MESSAGE);
    }
    Ok(())
}

/// Whether `IntakeIssueUpdateSerializer.update` attempts the triage
/// transition (`intake.py:147-149`): validated `status` is `1` and the
/// linked issue's state group is `triage`. Mirrors the `validate` branch
/// shape exactly — including that a missing issue state skips both.
pub fn should_transition_on_update(status: Option<i32>, issue_state_group: Option<&str>) -> bool {
    status == Some(STATUS_ACCEPTED) && issue_state_group == Some(STATE_GROUP_TRIAGE)
}

/// `IntakeIssueUpdateSerializer.update` triage transition
/// (`intake.py:149-155`): when `should_transition_on_update`, the linked
/// issue moves to the default project state via a plain `issue.save()`.
///
/// The asymmetry is exact: `validate` rejects when no default state exists,
/// but `update` re-reads the default and, if it is gone by save time
/// (`deleted between validate and save`), keeps the triage state silently
/// (`if default_state:` / no `else`, `:153-155`). `None` therefore means
/// "no transition" — both for non-triage issues and for the vanished-default
/// path — and the caller must leave `issue.state` untouched.
pub fn resolve_triage_transition<'a>(
    status: Option<i32>,
    issue_state_group: Option<&str>,
    default_state_id: Option<&'a str>,
) -> Option<&'a str> {
    if should_transition_on_update(status, issue_state_group) {
        return default_state_id;
    }
    None
}

/// `IssueDataSerializer.name` (`intake.py:166`): required `CharField`,
/// `max_length=255` (`allow_blank=False` default). Counts Unicode scalar
/// values, as DRF's `MaxLengthValidator` (`len(value)`) does — never slice
/// the input (UTF-8 boundary panic trap).
pub fn validate_issue_name(value: Option<&str>) -> Result<&str, &'static str> {
    let Some(name) = value else {
        return Err(REQUIRED_MESSAGE);
    };
    if name.is_empty() {
        return Err(BLANK_MESSAGE);
    }
    if name.chars().count() > ISSUE_NAME_MAX_LENGTH {
        return Err(NAME_MAX_LENGTH_MESSAGE);
    }
    Ok(name)
}

/// `IssueDataSerializer.description_html` (`intake.py:167-171`):
/// `required=False, allow_null=True` (`allow_blank=False` default).
/// `None` = key absent (skipped, valid); `Some(None)` = explicit `null`
/// (valid); `Some(Some(""))` = blank (rejected).
pub fn validate_description_html(
    value: Option<Option<&str>>,
) -> Result<Option<&str>, &'static str> {
    match value {
        None | Some(None) => Ok(None),
        Some(Some(html)) => {
            if html.is_empty() {
                return Err(BLANK_MESSAGE);
            }
            Ok(Some(html))
        }
    }
}

/// `IssueDataSerializer.priority` (`intake.py:169`): `ChoiceField` over
/// `Issue.PRIORITY_CHOICES` with `default="none"`. Missing input takes the
/// default; anything outside the choices fails with DRF's `invalid_choice`
/// body. (BUG-wire-dead: this never fires on the wire today — the create
/// view checks priority inline — but the rule is ported for the layer.)
pub fn validate_priority(value: Option<&str>) -> Result<&str, String> {
    match value {
        None => Ok(PRIORITY_DEFAULT),
        Some(priority) if PRIORITY_CHOICES.contains(&priority) => Ok(priority),
        Some(priority) => Err(invalid_priority_choice(priority)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};

    fn fixture() -> Value {
        let path = format!(
            "{}/../../fixtures/v1_assets/fx-ser-intake.json",
            env!("CARGO_MANIFEST_DIR")
        );
        serde_json::from_str(&std::fs::read_to_string(&path).expect("golden exists"))
            .expect("golden parses")
    }

    /// Top-level JSON key order of a serialization, read off the serialized
    /// bytes (struct fields emit in declaration order).
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
                    for ch in chars.by_ref() {
                        if ch == '"' {
                            break;
                        }
                        key.push(ch);
                    }
                    if chars.peek() == Some(&':') {
                        chars.next();
                        keys.push(key);
                    }
                }
                _ => {}
            }
        }
        keys
    }

    #[test]
    fn issue_for_intake_fields_and_read_only_match_fixture() {
        let fixture = fixture();
        let unit = &fixture["serializers"]["IssueForIntakeSerializer"];
        let fields: Vec<&str> = unit["fields"]
            .as_array()
            .expect("fields")
            .iter()
            .map(|v| v.as_str().expect("field"))
            .collect();
        assert_eq!(fields, ISSUE_FOR_INTAKE_FIELDS);
        assert_eq!(unit["model"], "Issue");
        assert_eq!(
            serialized_keys(&IssueForIntakeView {
                name: Some("n"),
                description_source: Some(json!({"a": 1})),
                description_html: None,
                priority: "none",
            }),
            ISSUE_FOR_INTAKE_FIELDS
        );
    }

    #[test]
    fn description_dual_key_renders_identical_content() {
        // BUG-dual-key: `description` (explicit, source=description_json) and
        // `description_json` (auto model field) carry the same content.
        for source in [
            Some(json!({"type": "doc", "content": []})),
            Some(Value::Null),
            None,
        ] {
            let view = IssueForIntakeView {
                name: Some("Triage item"),
                description_source: source.clone(),
                description_html: Some("<p></p>"),
                priority: "high",
            };
            let body = serde_json::to_value(&view).expect("value");
            let expected = source.clone().unwrap_or(Value::Null);
            assert_eq!(body["description"], expected, "source {source:?}");
            assert_eq!(body["description_json"], expected, "source {source:?}");
        }
        // `allow_null=True`: explicit null renders `null`, key present.
        let view = IssueForIntakeView {
            name: None,
            description_source: None,
            description_html: None,
            priority: "none",
        };
        let body = serde_json::to_value(&view).expect("value");
        assert!(body.get("description").is_some());
        assert_eq!(body["description"], Value::Null);
        assert_eq!(body["name"], Value::Null);
    }

    #[test]
    fn create_shape_is_single_required_nested_issue() {
        let fixture = fixture();
        let unit = &fixture["serializers"]["IntakeIssueCreateSerializer"];
        let fields: Vec<&str> = unit["fields"]
            .as_array()
            .expect("fields")
            .iter()
            .map(|v| v.as_str().expect("field"))
            .collect();
        assert_eq!(fields, CREATE_FIELDS);

        let view = IntakeIssueCreateView {
            issue: json!({
                "name": "Triage item",
                "description": {"type": "doc"},
                "description_json": {"type": "doc"},
                "description_html": "<p></p>",
                "priority": "none",
            }),
        };
        let body = serde_json::to_value(&view).expect("value");
        assert_eq!(serialized_keys(&view), ["issue"]);
        assert_eq!(
            body["issue"]["description"],
            body["issue"]["description_json"]
        );
    }

    #[test]
    fn read_serializer_extras_match_fixture() {
        let fixture = fixture();
        let unit = &fixture["serializers"]["IntakeIssueSerializer"];
        assert_eq!(unit["fields"], "__all__");
        assert_eq!(unit["issue_detail"]["source"], "issue");
        assert_eq!(unit["issue_detail"]["read_only"], true);
        assert_eq!(unit["inbox"]["source"], "intake.id");
        assert_eq!(unit["inbox"]["read_only"], true);
        let read_only: Vec<&str> = unit["read_only_fields"]
            .as_array()
            .expect("read_only_fields")
            .iter()
            .map(|v| v.as_str().expect("field"))
            .collect();
        assert_eq!(read_only, INTAKE_ISSUE_READ_ONLY_FIELDS);

        // `inbox` renders the parent Intake row id, not the IntakeIssue id.
        let view = full_view();
        let body = serde_json::to_value(&view).expect("value");
        assert_eq!(body["inbox"], "11111111-1111-4111-8111-111111111111");
        assert_eq!(body["id"], "22222222-2222-4222-8222-222222222222");
        assert_ne!(body["inbox"], body["id"]);
        // `issue_detail` is the nested IssueExpandSerializer passthrough.
        assert_eq!(
            body["issue_detail"]["id"],
            "33333333-3333-4333-8333-333333333333"
        );
        // Read-only FK renders as pk string.
        assert_eq!(body["issue"], "33333333-3333-4333-8333-333333333333");
    }

    fn full_view() -> IntakeIssueView<'static> {
        IntakeIssueView {
            id: "22222222-2222-4222-8222-222222222222",
            issue_detail: Some(json!({"id": "33333333-3333-4333-8333-333333333333"})),
            inbox: "11111111-1111-4111-8111-111111111111",
            created_at: "2026-09-29T00:00:00Z",
            updated_at: "2026-09-29T00:00:01Z",
            deleted_at: None,
            created_by: Some("44444444-4444-4444-8444-444444444444"),
            updated_by: Some("44444444-4444-4444-8444-444444444444"),
            workspace: "55555555-5555-4555-8555-555555555555",
            project: "66666666-6666-4666-8666-666666666666",
            intake: "11111111-1111-4111-8111-111111111111",
            issue: "33333333-3333-4333-8333-333333333333",
            status: STATUS_PENDING,
            snoozed_till: None,
            duplicate_to: None,
            source: Some("IN_APP"),
            source_email: None,
            external_source: None,
            external_id: None,
            extra: json!({}),
        }
    }

    #[test]
    fn update_fields_match_fixture() {
        let fixture = fixture();
        let unit = &fixture["serializers"]["IntakeIssueUpdateSerializer"];
        let fields: Vec<&str> = unit["fields"]
            .as_array()
            .expect("fields")
            .iter()
            .map(|v| v.as_str().expect("field"))
            .collect();
        assert_eq!(fields, UPDATE_FIELDS);
        let read_only: Vec<&str> = unit["read_only_fields"]
            .as_array()
            .expect("read_only_fields")
            .iter()
            .map(|v| v.as_str().expect("field"))
            .collect();
        assert_eq!(read_only, UPDATE_READ_ONLY_FIELDS);
    }

    #[test]
    fn accept_guard_matches_fixture_goldens() {
        let fixture = fixture();
        let guard = &fixture["serializers"]["IntakeIssueUpdateSerializer"]["accept_guard"];
        assert_eq!(guard["error_body"]["status"], ACCEPT_GUARD_MESSAGE);

        // Golden 1: status=1 + triage + no default -> 400 error body.
        assert_eq!(
            validate_accept(Some(1), Some("triage"), false),
            Err(ACCEPT_GUARD_MESSAGE)
        );
        let golden = &fixture["serializers"]["IntakeIssueUpdateSerializer"]["goldens"][0];
        assert_eq!(golden["status_code"], 400);
        assert_eq!(golden["out"]["status"], ACCEPT_GUARD_MESSAGE);

        // Golden 2: status=1 + non-triage -> valid, no transition on save.
        assert_eq!(validate_accept(Some(1), Some("backlog"), false), Ok(()));
        assert_eq!(
            resolve_triage_transition(Some(1), Some("backlog"), Some("state-default")),
            None
        );

        // Golden 3: status=-1 -> valid (guard is status==1 only).
        assert_eq!(validate_accept(Some(-1), Some("triage"), false), Ok(()));
        assert_eq!(
            resolve_triage_transition(Some(-1), Some("triage"), Some("state-default")),
            None
        );
    }

    #[test]
    fn accept_guard_edge_cases() {
        // Default exists -> valid even for triage.
        assert_eq!(validate_accept(Some(1), Some("triage"), true), Ok(()));
        // Missing issue state skips the check entirely (`:125`).
        assert_eq!(validate_accept(Some(1), None, false), Ok(()));
        // Missing status cannot fire the guard.
        assert_eq!(validate_accept(None, Some("triage"), false), Ok(()));
        // Non-int statuses never reach the guard as 1 (int-strict).
        for status in [
            STATUS_PENDING,
            STATUS_REJECTED,
            STATUS_SNOOZED,
            STATUS_DUPLICATE,
        ] {
            assert_eq!(validate_accept(Some(status), Some("triage"), false), Ok(()));
        }
    }

    #[test]
    fn triage_transition_ports_the_asymmetry_exactly() {
        // status=1 + triage + default present -> move to the default state.
        assert_eq!(
            resolve_triage_transition(Some(1), Some("triage"), Some("state-default")),
            Some("state-default")
        );
        // status=1 + triage + default GONE by save time -> silent no-op:
        // the issue keeps its triage state (`if default_state:`, no `else`).
        // `validate` would have rejected this same state moments earlier —
        // that asymmetry is the ported behaviour, not a bug in the port.
        assert_eq!(
            resolve_triage_transition(Some(1), Some("triage"), None),
            None
        );
        // Non-triage and non-accept statuses never transition.
        assert_eq!(
            resolve_triage_transition(Some(1), Some("backlog"), Some("state-default")),
            None
        );
        assert_eq!(
            resolve_triage_transition(Some(0), Some("triage"), Some("state-default")),
            None
        );
        assert!(should_transition_on_update(Some(1), Some("triage")));
        assert!(!should_transition_on_update(Some(1), None));
    }

    #[test]
    fn status_and_group_consts_match_models() {
        assert_eq!(
            (
                STATUS_PENDING,
                STATUS_REJECTED,
                STATUS_SNOOZED,
                STATUS_ACCEPTED,
                STATUS_DUPLICATE
            ),
            (-2, -1, 0, 1, 2)
        );
        assert_eq!(STATE_GROUP_TRIAGE, "triage");
    }

    #[test]
    fn issue_data_name_rules() {
        assert_eq!(validate_issue_name(Some("Triage item")), Ok("Triage item"));
        assert_eq!(validate_issue_name(None), Err(REQUIRED_MESSAGE));
        assert_eq!(validate_issue_name(Some("")), Err(BLANK_MESSAGE));
        let max = "n".repeat(ISSUE_NAME_MAX_LENGTH);
        assert_eq!(validate_issue_name(Some(&max)), Ok(max.as_str()));
        // Length counts code points, never slices (UTF-8 boundary trap).
        let wide = "é".repeat(ISSUE_NAME_MAX_LENGTH);
        assert_eq!(validate_issue_name(Some(&wide)), Ok(wide.as_str()));
        assert_eq!(
            validate_issue_name(Some(&format!("{max}x"))),
            Err(NAME_MAX_LENGTH_MESSAGE)
        );
        // 255 multi-byte chars + 1 ASCII char is 256 code points, still long.
        assert_eq!(
            validate_issue_name(Some(&format!("{wide}x"))),
            Err(NAME_MAX_LENGTH_MESSAGE)
        );
    }

    #[test]
    fn issue_data_description_html_rules() {
        // Not required: absent key is valid.
        assert_eq!(validate_description_html(None), Ok(None));
        // Nullable: explicit null is valid.
        assert_eq!(validate_description_html(Some(None)), Ok(None));
        assert_eq!(
            validate_description_html(Some(Some("<p>hi</p>"))),
            Ok(Some("<p>hi</p>"))
        );
        assert_eq!(
            validate_description_html(Some(Some(""))),
            Err(BLANK_MESSAGE)
        );
    }

    #[test]
    fn issue_data_priority_rules() {
        // Default applies when the key is absent.
        assert_eq!(validate_priority(None), Ok(PRIORITY_DEFAULT));
        for choice in PRIORITY_CHOICES {
            assert_eq!(validate_priority(Some(choice)), Ok(choice));
        }
        assert_eq!(
            validate_priority(Some("CRITICAL")),
            Err("\"CRITICAL\" is not a valid choice.".to_string())
        );
        assert_eq!(
            validate_priority(Some("")),
            Err("\"\" is not a valid choice.".to_string())
        );
    }
}
