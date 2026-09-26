//! DRF-compatible JSON kernel for the Pi Dash Rust backend.
//!
//! Mirrors the output half of `DynamicBaseSerializer`
//! (`pi_dash/app/serializers/base.py`, also `pi_dash/space/serializer/base.py`)
//! plus the value-rendering rules of DRF 3.15 with the project's settings
//! (`REST_FRAMEWORK` in `pi_dash/settings/common.py`: default JSONRenderer,
//! no custom `DATETIME_FORMAT`; `USE_TZ = True`, `TIME_ZONE = "UTC"`).
//!
//! Rules ported one for one:
//!
//! - `fields` / `expand` query params: comma-split, empties dropped, `None`
//!   when nothing remains (`BaseViewSet.fields` / `.expand`).
//! - The `fields` constructor kwarg is accepted and then **discarded**: the
//!   very first thing `DynamicBaseSerializer.__init__` does is
//!   `fields = self.expand` (a ported bug, listed in this crate's docs).
//!   [`effective_selection`] is that assignment.
//! - `_filter_fields` never removes fields; it only appends expansion fields
//!   for selected names the serializer does not declare. Unknown names are
//!   silently ignored. A selected dict (nested fields) crashes Python with
//!   `TypeError` (a `Field` is iterated); here it is
//!   [`SelectError::Nested`] so handlers can answer 500 the same way.
//! - `to_representation`: every expanded attribute present in the fields is
//!   replaced by its nested serialization (lists serialize `many=True`);
//!   an expanded name with no expansion entry falls back to the
//!   `<expand>_id` attribute; `issue_attachments` in fields or expand forces
//!   the attachments key. [`expand_value`] and [`needs_attachments`] are the
//!   JSON-level halves (fetching stays with the domain handlers).
//! - Datetimes render as DRF `iso-8601`: `+00:00` becomes `Z`, other offsets
//!   are kept, microseconds print only when nonzero. The request's zone comes
//!   from `TimezoneMixin` (the user's `user_timezone`, default `UTC`).
//! - Decimals render as plain strings (`'{0:f}'`), never exponent notation,
//!   with scale preserved (`Decimal('2.50')` stays `"2.50"`). Per-field
//!   quantize (`max_digits` / `decimal_places`) stays with the domain
//!   serializers; the kernel only guarantees the string shape.
//! - `None` renders as JSON `null`; a trimmed field is absent from the
//!   object. [`is_none`] is the `skip_serializing_if` predicate for that.

use std::collections::HashMap;

use chrono::{DateTime, NaiveDateTime, TimeZone};
use rust_decimal::Decimal;
use serde_json::Value;
use thiserror::Error as ThisError;

/// Split a `fields` / `expand` query parameter. Mirrors
/// `[f for f in request.GET.get("fields", "").split(",") if f]`: no
/// whitespace stripping, empties dropped, `None` when nothing remains.
pub fn parse_list_param(raw: Option<&str>) -> Option<Vec<String>> {
    let items: Vec<String> = raw
        .unwrap_or("")
        .split(',')
        .filter(|part| !part.is_empty())
        .map(str::to_owned)
        .collect();
    if items.is_empty() {
        None
    } else {
        Some(items)
    }
}

/// Render an aware datetime exactly like DRF `DateTimeField` with the
/// default `iso-8601` format: ISO-8601 with `+00:00` rewritten to `Z`.
pub fn render_datetime<Tz: TimeZone>(dt: &DateTime<Tz>) -> String {
    let utc = dt.with_timezone(&chrono::Utc);
    render_utc_parts(
        &utc.format("%Y-%m-%dT%H:%M:%S").to_string(),
        utc.timestamp_subsec_nanos(),
        "Z",
    )
}

/// Render an aware datetime in a caller-chosen zone (the request's zone).
/// Non-UTC offsets are kept verbatim; a `+00:00` offset is rewritten to `Z`,
/// exactly like DRF does.
pub fn render_datetime_in<TzFrom: TimeZone, TzTo: TimeZone>(
    dt: &DateTime<TzFrom>,
    tz: &TzTo,
) -> String
where
    TzTo::Offset: std::fmt::Display,
{
    let local = dt.with_timezone(tz);
    let suffix = local.format("%:z").to_string();
    let suffix = if suffix == "+00:00" {
        "Z".to_owned()
    } else {
        suffix
    };
    render_utc_parts(
        &local.format("%Y-%m-%dT%H:%M:%S").to_string(),
        local.timestamp_subsec_nanos(),
        &suffix,
    )
}

/// Render a naive datetime through the request's zone. Under the project's
/// settings (`USE_TZ = True`) DRF makes naive values aware in the field's
/// timezone before rendering, so a naive value renders exactly like the same
/// wall time in `tz` (including the `Z` rewrite for `+00:00`).
pub fn render_naive_datetime_in<TzTo: TimeZone>(dt: &NaiveDateTime, tz: &TzTo) -> String
where
    TzTo::Offset: std::fmt::Display,
{
    use chrono::MappedLocalTime;
    let local = match tz.from_local_datetime(dt) {
        MappedLocalTime::Single(local) | MappedLocalTime::Ambiguous(local, _) => local,
        // A wall time inside a DST gap has no local offset; fall back to the
        // same wall time read as UTC (Python's zoneinfo attach never raises).
        MappedLocalTime::None => return render_datetime_in(&dt.and_utc(), tz),
    };
    let suffix = local.format("%:z").to_string();
    let suffix = if suffix == "+00:00" {
        "Z".to_owned()
    } else {
        suffix
    };
    render_utc_parts(
        &dt.format("%Y-%m-%dT%H:%M:%S").to_string(),
        dt.and_utc().timestamp_subsec_nanos(),
        &suffix,
    )
}

fn render_utc_parts(base: &str, nanos: u32, suffix: &str) -> String {
    if nanos == 0 {
        format!("{base}{suffix}")
    } else if nanos.is_multiple_of(1000) {
        format!("{base}.{:06}{suffix}", nanos / 1000)
    } else {
        format!("{base}.{nanos:09}{suffix}")
    }
}

/// Render a `Decimal` as DRF does with `COERCE_DECIMAL_TO_STRING`:
/// plain notation, never scientific, scale preserved.
pub fn render_decimal(value: &Decimal) -> String {
    value.to_string()
}

/// `skip_serializing_if` predicate: `None` renders as `null` when the field
/// is present, and the field is absent only when trimmed by selection.
pub fn is_none<T>(value: &Option<T>) -> bool {
    value.is_none()
}

/// One entry of an `expand` selection: a plain field name, or the nested-dict
/// form Python accepts syntactically and then crashes on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SelItem {
    Field(String),
    Nested(String, Vec<SelItem>),
}

impl SelItem {
    pub fn field(name: &str) -> Self {
        SelItem::Field(name.to_owned())
    }
}

/// Why an `expand` selection was rejected. A [`SelectError::Nested`] is the
/// port of Python's `TypeError` on nested dicts: the handler answers 500.
#[derive(Debug, Clone, PartialEq, Eq, ThisError)]
pub enum SelectError {
    #[error("nested field selection is not supported")]
    Nested(String),
}

/// The `fields = self.expand` assignment: whatever the caller passed as
/// `fields` is discarded and `expand` (defaulting to empty) wins.
pub fn effective_selection(
    _fields: Option<Vec<String>>,
    expand: Option<Vec<SelItem>>,
) -> Vec<SelItem> {
    expand.unwrap_or_default()
}

/// Whether an expansion entry serializes `many=True`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExpansionKind {
    Single,
    Many,
}

/// The expansion mapper from `DynamicBaseSerializer._filter_fields`, without
/// `issue_attachment` (absent there).
pub const EXPANSION_FIELDS_FILTER: &[(&str, ExpansionKind)] = &[
    ("user", ExpansionKind::Single),
    ("workspace", ExpansionKind::Single),
    ("project", ExpansionKind::Single),
    ("default_assignee", ExpansionKind::Single),
    ("project_lead", ExpansionKind::Single),
    ("state", ExpansionKind::Single),
    ("created_by", ExpansionKind::Single),
    ("issue", ExpansionKind::Single),
    ("actor", ExpansionKind::Single),
    ("owned_by", ExpansionKind::Single),
    ("members", ExpansionKind::Many),
    ("assignees", ExpansionKind::Many),
    ("labels", ExpansionKind::Many),
    ("issue_cycle", ExpansionKind::Many),
    ("parent", ExpansionKind::Single),
    ("issue_relation", ExpansionKind::Many),
    ("issue_intake", ExpansionKind::Many),
    ("issue_related", ExpansionKind::Many),
    ("issue_reactions", ExpansionKind::Many),
    ("issue_link", ExpansionKind::Many),
    ("sub_issues", ExpansionKind::Many),
];

/// The expansion mapper from `DynamicBaseSerializer.to_representation`,
/// which additionally maps `issue_attachment` (many).
pub const EXPANSION_FIELDS_REPR: &[(&str, ExpansionKind)] = &[
    ("user", ExpansionKind::Single),
    ("workspace", ExpansionKind::Single),
    ("project", ExpansionKind::Single),
    ("default_assignee", ExpansionKind::Single),
    ("project_lead", ExpansionKind::Single),
    ("state", ExpansionKind::Single),
    ("created_by", ExpansionKind::Single),
    ("issue", ExpansionKind::Single),
    ("actor", ExpansionKind::Single),
    ("owned_by", ExpansionKind::Single),
    ("members", ExpansionKind::Many),
    ("assignees", ExpansionKind::Many),
    ("labels", ExpansionKind::Many),
    ("issue_cycle", ExpansionKind::Many),
    ("parent", ExpansionKind::Single),
    ("issue_relation", ExpansionKind::Many),
    ("issue_intake", ExpansionKind::Many),
    ("issue_related", ExpansionKind::Many),
    ("issue_reactions", ExpansionKind::Many),
    ("issue_attachment", ExpansionKind::Many),
    ("issue_link", ExpansionKind::Many),
    ("sub_issues", ExpansionKind::Many),
];

/// Port of `_filter_fields`: append expansion entries for selected names the
/// serializer does not declare. Declared or unknown names are untouched;
/// nested entries are an error.
pub fn apply_expansion(
    declared: &mut Vec<String>,
    selected: &[SelItem],
    table: &[(&str, ExpansionKind)],
) -> Result<(), SelectError> {
    let known: HashMap<&str, ExpansionKind> = table.iter().copied().collect();
    for item in selected {
        match item {
            SelItem::Nested(name, _) => return Err(SelectError::Nested(name.clone())),
            SelItem::Field(name) => {
                if !declared.iter().any(|f| f == name) && known.contains_key(name.as_str()) {
                    declared.push(name.clone());
                }
            }
        }
    }
    Ok(())
}

/// Look an expansion entry up in a mapper table.
pub fn expansion_kind(table: &[(&str, ExpansionKind)], name: &str) -> Option<ExpansionKind> {
    table
        .iter()
        .find(|(key, _)| *key == name)
        .map(|(_, kind)| *kind)
}

/// Port of the `to_representation` replacement: the expanded key takes the
/// nested serialization. Whether the nested value was built `many=True` is
/// decided by the mapper: a JSON array means `many=True` was used.
pub fn expand_value(response: &mut serde_json::Map<String, Value>, name: &str, nested: Value) {
    response.insert(name.to_owned(), nested);
}

/// Port of the `to_representation` fallback: an expanded name with no
/// expansion entry reads `<expand>_id` (defaulting to `None` / null).
pub fn expand_fallback_id(instance: &serde_json::Map<String, Value>, name: &str) -> Value {
    let id_key = format!("{name}_id");
    instance
        .get(id_key.as_str())
        .cloned()
        .unwrap_or(Value::Null)
}

/// The `issue_attachments` special case: the key is forced into the response
/// whenever it appears in the serializer fields or in expand.
pub fn needs_attachments(fields: &[String], expand: &[SelItem]) -> bool {
    if fields.iter().any(|f| f == "issue_attachments") {
        return true;
    }
    expand.iter().any(|item| match item {
        SelItem::Field(name) => name == "issue_attachments",
        SelItem::Nested(name, _) => name == "issue_attachments",
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Timelike as _;
    use chrono_tz::Tz;
    use std::str::FromStr;

    #[test]
    fn list_param_splitting_matches_python() {
        assert_eq!(parse_list_param(None), None);
        assert_eq!(parse_list_param(Some("")), None);
        assert_eq!(parse_list_param(Some(",")), None);
        assert_eq!(
            parse_list_param(Some("state,assignees")),
            Some(vec!["state".to_owned(), "assignees".to_owned()])
        );
        // No stripping, empties dropped: "a, b," -> ["a", " b"].
        assert_eq!(
            parse_list_param(Some("a, b,")),
            Some(vec!["a".to_owned(), " b".to_owned()])
        );
    }

    #[test]
    fn datetime_renders_drf_iso8601_with_z() {
        let dt = chrono::Utc
            .with_ymd_and_hms(2024, 1, 2, 3, 4, 5)
            .unwrap()
            .with_nanosecond(123_456_000)
            .unwrap();
        assert_eq!(render_datetime(&dt), "2024-01-02T03:04:05.123456Z");
    }

    #[test]
    fn datetime_without_micros_has_no_fraction() {
        let dt = chrono::Utc.with_ymd_and_hms(2024, 1, 2, 3, 4, 5).unwrap();
        assert_eq!(render_datetime(&dt), "2024-01-02T03:04:05Z");
    }

    #[test]
    fn datetime_in_user_zone_keeps_offset() {
        let dt = chrono::Utc.with_ymd_and_hms(2024, 1, 2, 3, 4, 5).unwrap();
        let eastern: Tz = "America/New_York".parse().unwrap();
        assert_eq!(
            render_datetime_in(&dt, &eastern),
            "2024-01-01T22:04:05-05:00"
        );
    }

    #[test]
    fn datetime_fixed_offset_zone() {
        let dt = chrono::Utc.with_ymd_and_hms(2024, 1, 2, 3, 4, 5).unwrap();
        let zone = chrono::FixedOffset::east_opt(5 * 3600 + 1800).unwrap();
        assert_eq!(render_datetime_in(&dt, &zone), "2024-01-02T08:34:05+05:30");
    }

    #[test]
    fn utc_zone_renders_z_not_offset() {
        // Oracle: DRF rewrites a trailing `+00:00` to `Z` for every zone,
        // so the default request zone (UTC) takes the `Z` path.
        let dt = chrono::Utc.with_ymd_and_hms(2024, 1, 2, 3, 4, 5).unwrap();
        assert_eq!(
            render_datetime_in(&dt, &chrono::Utc),
            "2024-01-02T03:04:05Z"
        );
    }

    #[test]
    fn naive_datetime_renders_through_the_request_zone() {
        // Oracle (DRF 3.15, USE_TZ=True): naive values are made aware in the
        // field timezone before rendering.
        let dt = chrono::NaiveDate::from_ymd_opt(2024, 1, 2)
            .unwrap()
            .and_hms_opt(3, 4, 5)
            .unwrap();
        assert_eq!(
            render_naive_datetime_in(&dt, &chrono::Utc),
            "2024-01-02T03:04:05Z"
        );
        let eastern: Tz = "America/New_York".parse().unwrap();
        assert_eq!(
            render_naive_datetime_in(&dt, &eastern),
            "2024-01-02T03:04:05-05:00"
        );
    }

    #[test]
    fn sub_microsecond_nanos_use_nine_digits() {
        let dt = chrono::Utc
            .with_ymd_and_hms(2024, 1, 2, 3, 4, 5)
            .unwrap()
            .with_nanosecond(123_456_789)
            .unwrap();
        assert_eq!(render_datetime(&dt), "2024-01-02T03:04:05.123456789Z");
    }

    #[test]
    fn decimal_renders_plain_string_like_drf() {
        // Oracle: python3 -c "from decimal import Decimal; print('{0:f}'.format(Decimal(s)))"
        for (input, expected) in [
            ("1E+2", "100"),
            ("2.50", "2.50"),
            ("0.000", "0.000"),
            ("123.456", "123.456"),
            ("-0.5", "-0.5"),
            ("100", "100"),
        ] {
            assert_eq!(
                render_decimal(&Decimal::from_str(input).unwrap()),
                expected,
                "input {input}"
            );
        }
    }

    #[test]
    fn fields_kwarg_is_discarded_in_favour_of_expand() {
        let sel = effective_selection(
            Some(vec!["state".to_owned()]),
            Some(vec![SelItem::field("assignees")]),
        );
        assert_eq!(sel, vec![SelItem::field("assignees")]);
        assert!(effective_selection(None, None).is_empty());
    }

    #[test]
    fn expansion_only_adds_undeclared_known_fields() {
        let mut declared = vec!["id".to_owned(), "state".to_owned()];
        apply_expansion(
            &mut declared,
            &[
                SelItem::field("labels"),
                SelItem::field("state"),
                SelItem::field("nope"),
            ],
            EXPANSION_FIELDS_FILTER,
        )
        .unwrap();
        assert_eq!(declared, vec!["id", "state", "labels"]);
    }

    #[test]
    fn nested_selection_is_an_error() {
        let mut declared = vec!["id".to_owned()];
        let err = apply_expansion(
            &mut declared,
            &[SelItem::Nested(
                "labels".to_owned(),
                vec![SelItem::field("id")],
            )],
            EXPANSION_FIELDS_FILTER,
        )
        .unwrap_err();
        assert_eq!(err, SelectError::Nested("labels".to_owned()));
    }

    #[test]
    fn expansion_tables_differ_by_issue_attachment() {
        assert_eq!(
            expansion_kind(EXPANSION_FIELDS_FILTER, "issue_attachment"),
            None
        );
        assert_eq!(
            expansion_kind(EXPANSION_FIELDS_REPR, "issue_attachment"),
            Some(ExpansionKind::Many)
        );
        assert_eq!(
            expansion_kind(EXPANSION_FIELDS_FILTER, "parent"),
            Some(ExpansionKind::Single)
        );
    }

    #[test]
    fn expand_fallback_reads_expand_id() {
        let mut instance = serde_json::Map::new();
        instance.insert("owner_id".to_owned(), Value::from("u-1"));
        assert_eq!(expand_fallback_id(&instance, "owner"), Value::from("u-1"));
        assert_eq!(expand_fallback_id(&instance, "missing"), Value::Null);
    }

    #[test]
    fn attachments_key_rule() {
        assert!(needs_attachments(&["issue_attachments".to_owned()], &[]));
        assert!(needs_attachments(
            &[],
            &[SelItem::field("issue_attachments")]
        ));
        assert!(!needs_attachments(
            &["labels".to_owned()],
            &[SelItem::field("state")]
        ));
    }

    #[test]
    fn is_none_predicate() {
        assert!(is_none(&None::<String>));
        assert!(!is_none(&Some("x".to_owned())));
    }
}
