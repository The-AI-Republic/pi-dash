//! Local tool argument validation: the pydantic-ai lax-validation port
//! (PIDASHCONV-1113).
//!
//! Before a tool body runs, pydantic-ai validates the model's JSON args
//! against the tool's `FunctionSchema` validator (lax mode). Failures
//! become a `RetryPromptPart` whose content is the `ErrorDetails` list.
//! This module replicates that validator exactly — coercion table, error
//! shapes, error order — as probed live against the pinned pydantic-ai
//! 1.107 / pydantic 2.14 (fixture `assistant/tools-local-args.json`,
//! 103 vectors).
//!
//! Probed rules, by annotation shape:
//!
//! * `str`: only JSON strings; anything else (int, bool, null, list,
//!   dict) is `string_type`. No coercion, no trimming.
//! * `Optional[str]`: string or null; missing fills the default.
//! * `int`: JSON ints, bools (`true` → 1), whole floats (2.0 → 2;
//!   fractional → `int_from_float`; non-finite → `finite_number`), and
//!   numeric strings (whitespace-trimmed, optional sign, single
//!   underscores between digits like `int()`, dotted-whole forms like
//!   `"2.0"` → 2; anything else → `int_parsing`). Null/lists/dicts are
//!   `int_type`. Python ints are unbounded; values beyond `i128` (and
//!   JSON literals beyond `u64`, which serde reads as lossy `f64`)
//!   saturate — the only deliberate deviation, confined to inputs no
//!   model emits, documented on [`coerce_int`].
//! * `bool`: bools as-is; case-insensitive `true/1/yes/on/t/y` and
//!   `false/0/no/off/f/n` with no trimming; ints 0/1 and floats 0.0/1.0.
//!   Unknown strings and out-of-range ints are `bool_parsing`;
//!   fractional floats, null, lists and dicts are `bool_type`.
//! * `list[str]`: JSON arrays only (`list_type` otherwise); every
//!   non-string item is a `string_type` error at `[field, index]`.
//! * Missing required fields are `missing` (with the whole args object
//!   as `input`); unknown keys are `extra_forbidden` in input order.
//!   Non-object args are a single `dict_type` error. Errors run in
//!   field-definition order, extras last.

// Validation errors are cold-path values moved once into the
// `RetryPromptPart` details list; boxing them would only obscure the
// probed `ErrorDetails` shape.
#![allow(clippy::result_large_err)]

use serde_json::Value;

/// Pydantic error-URL base pinned to the vendored minor (probed in every
/// `ErrorDetails`: `https://errors.pydantic.dev/2.14/v/<type>`).
pub const PYDANTIC_ERRORS_URL: &str = "https://errors.pydantic.dev/2.14/v";

/// One `loc` segment of an `ErrorDetails` (`["limit"]`, `["related_issues", 0]`, `[]`).
#[derive(Debug, Clone, PartialEq)]
pub enum LocSegment {
    Field(String),
    Index(usize),
}

/// One validation failure, rendered as pydantic's `ErrorDetails`
/// (`{"type", "loc", "msg", "input", "url"}`, in that key order).
#[derive(Debug, Clone, PartialEq)]
pub struct ArgError {
    pub error_type: &'static str,
    pub loc: Vec<LocSegment>,
    pub message: &'static str,
    pub input: Value,
}

impl ArgError {
    pub fn to_value(&self) -> Value {
        let loc: Vec<Value> = self
            .loc
            .iter()
            .map(|segment| match segment {
                LocSegment::Field(name) => Value::String(name.clone()),
                LocSegment::Index(index) => Value::from(*index as u64),
            })
            .collect();
        let mut map = serde_json::Map::with_capacity(5);
        map.insert("type".to_owned(), Value::String(self.error_type.to_owned()));
        map.insert("loc".to_owned(), Value::Array(loc));
        map.insert("msg".to_owned(), Value::String(self.message.to_owned()));
        map.insert("input".to_owned(), self.input.clone());
        map.insert(
            "url".to_owned(),
            Value::String(format!("{PYDANTIC_ERRORS_URL}/{}", self.error_type)),
        );
        Value::Object(map)
    }
}

/// A validated argument value.
#[derive(Debug, Clone, PartialEq)]
pub enum Coerced {
    Str(String),
    Int(i128),
    Bool(bool),
    List(Vec<String>),
    Null,
}

/// The annotation shapes the 17 tools use.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldKind {
    Str,
    OptStr,
    Int,
    Bool,
    StrList,
}

/// A field default (const-friendly; [`Coerced`] holds a `String`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldDefault {
    Null,
    Int(i128),
    Bool(bool),
    Str(&'static str),
}

impl FieldDefault {
    fn to_coerced(self) -> Coerced {
        match self {
            FieldDefault::Null => Coerced::Null,
            FieldDefault::Int(value) => Coerced::Int(value),
            FieldDefault::Bool(value) => Coerced::Bool(value),
            FieldDefault::Str(value) => Coerced::Str(value.to_owned()),
        }
    }
}

/// One tool parameter, in signature order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FieldSpec {
    pub name: &'static str,
    pub kind: FieldKind,
    /// `None` means required.
    pub default: Option<FieldDefault>,
}

// ---------------------------------------------------------------------------
// Coercion cores
// ---------------------------------------------------------------------------

fn string_type_error(field: &str, input: &Value) -> ArgError {
    ArgError {
        error_type: "string_type",
        loc: vec![LocSegment::Field(field.to_owned())],
        message: "Input should be a valid string",
        input: input.clone(),
    }
}

/// `str`: JSON strings only — no coercion of any other type (probed:
/// ints, bools, null, lists and dicts all fail `string_type`).
pub fn coerce_str(field: &str, value: &Value) -> Result<String, ArgError> {
    match value {
        Value::String(text) => Ok(text.clone()),
        _ => Err(string_type_error(field, value)),
    }
}

/// `Optional[str]`: strings pass through, null is `None`, anything else
/// is `string_type` (probed: `5` fails; the `anyOf` still rejects
/// non-strings rather than falling through).
pub fn coerce_opt_str(field: &str, value: &Value) -> Result<Option<String>, ArgError> {
    match value {
        Value::Null => Ok(None),
        Value::String(text) => Ok(Some(text.clone())),
        _ => Err(string_type_error(field, value)),
    }
}

/// Parse a trimmed numeric string the way pydantic-core's lax int does
/// (probed): `int()`-style digits with single interior underscores and
/// an optional sign, or a dotted form that parses to a whole finite
/// float (`"2.0"` → 2). Exponent forms (`"1e3"`), non-ASCII digits,
/// `nan`/`inf` spellings and empty strings all fail.
fn parse_int_str(text: &str) -> Option<i128> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return None;
    }
    let (sign, digits) = match trimmed.strip_prefix(['+', '-']) {
        Some(rest) => (trimmed.starts_with('-'), rest),
        None => (false, trimmed),
    };
    if !digits.is_empty()
        && digits.bytes().all(|b| b.is_ascii_digit() || b == b'_')
        && !digits.starts_with('_')
        && !digits.ends_with('_')
        && !digits.contains("__")
    {
        let compact: String = digits
            .bytes()
            .filter(|b| *b != b'_')
            .map(|b| b as char)
            .collect();
        if let Ok(value) = compact.parse::<i128>() {
            return Some(if sign { -value } else { value });
        }
        // Beyond i128: Python would accept and the tool would clamp;
        // saturate (documented deviation, absurd inputs only).
        return Some(if sign { i128::MIN } else { i128::MAX });
    }
    // Dotted-whole fallback (`"2.0"` → 2, `"2.5"` fails, `"1e3"` fails
    // for lack of a `.`): mirrors every probe vector.
    if digits.contains('.') {
        if let Ok(float) = trimmed.parse::<f64>() {
            if float.is_finite() && float.fract() == 0.0 {
                if float >= i128::MAX as f64 {
                    return Some(i128::MAX);
                }
                if float <= i128::MIN as f64 {
                    return Some(i128::MIN);
                }
                return Some(float as i128);
            }
        }
    }
    None
}

/// `int` lax coercion (probed): ints, bools, whole floats and numeric
/// strings as above. Null/lists/dicts are `int_type`; fractional floats
/// are `int_from_float`; non-finite floats are `finite_number` (the
/// latter is unreachable from real JSON, which has no such literals,
/// but reachable from `1e400`-style overflow parses).
pub fn coerce_int(field: &str, value: &Value) -> Result<i128, ArgError> {
    let loc = vec![LocSegment::Field(field.to_owned())];
    match value {
        Value::Bool(flag) => Ok(i128::from(*flag)),
        Value::Number(number) => {
            if let Some(int) = number.as_i64() {
                return Ok(i128::from(int));
            }
            if let Some(int) = number.as_u64() {
                return Ok(i128::from(int));
            }
            match number.as_f64() {
                Some(float) if !float.is_finite() => Err(ArgError {
                    error_type: "finite_number",
                    loc,
                    message: "Input should be a finite number",
                    input: value.clone(),
                }),
                Some(float) if float.fract() != 0.0 => Err(ArgError {
                    error_type: "int_from_float",
                    loc,
                    message: "Input should be a valid integer, got a number with a fractional part",
                    input: value.clone(),
                }),
                Some(float) => {
                    if float >= i128::MAX as f64 {
                        return Ok(i128::MAX);
                    }
                    if float <= i128::MIN as f64 {
                        return Ok(i128::MIN);
                    }
                    Ok(float as i128)
                }
                // Out-of-f64-range literals (`1e400`): serde parses
                // them to a number `as_f64` cannot represent — the
                // infinite case, exactly as probed.
                None => Err(ArgError {
                    error_type: "finite_number",
                    loc,
                    message: "Input should be a finite number",
                    input: value.clone(),
                }),
            }
        }
        Value::String(text) => parse_int_str(text).ok_or_else(|| ArgError {
            error_type: "int_parsing",
            loc,
            message: "Input should be a valid integer, unable to parse string as an integer",
            input: value.clone(),
        }),
        _ => Err(ArgError {
            error_type: "int_type",
            loc,
            message: "Input should be a valid integer",
            input: value.clone(),
        }),
    }
}

/// `bool` lax coercion (probed): bools as-is; case-insensitive
/// `true/1/yes/on/t/y` and `false/0/no/off/f/n` with no trimming
/// (`" Yes"` fails); ints 0/1 and floats 0.0/1.0. Unknown strings and
/// out-of-range ints are `bool_parsing`; fractional floats, null,
/// lists and dicts are `bool_type`.
pub fn coerce_bool(field: &str, value: &Value) -> Result<bool, ArgError> {
    let loc = vec![LocSegment::Field(field.to_owned())];
    match value {
        Value::Bool(flag) => Ok(*flag),
        Value::String(text) => {
            const TRUE_WORDS: [&str; 6] = ["true", "1", "yes", "on", "t", "y"];
            const FALSE_WORDS: [&str; 6] = ["false", "0", "no", "off", "f", "n"];
            let lowered = text.to_lowercase();
            if TRUE_WORDS.contains(&lowered.as_str()) {
                Ok(true)
            } else if FALSE_WORDS.contains(&lowered.as_str()) {
                Ok(false)
            } else {
                Err(ArgError {
                    error_type: "bool_parsing",
                    loc,
                    message: "Input should be a valid boolean, unable to interpret input",
                    input: value.clone(),
                })
            }
        }
        Value::Number(number) => {
            if let Some(int) = number.as_i64() {
                return match int {
                    0 => Ok(false),
                    1 => Ok(true),
                    _ => Err(ArgError {
                        error_type: "bool_parsing",
                        loc,
                        message: "Input should be a valid boolean, unable to interpret input",
                        input: value.clone(),
                    }),
                };
            }
            if let Some(int) = number.as_u64() {
                return match int {
                    0 => Ok(false),
                    1 => Ok(true),
                    _ => Err(ArgError {
                        error_type: "bool_parsing",
                        loc,
                        message: "Input should be a valid boolean, unable to interpret input",
                        input: value.clone(),
                    }),
                };
            }
            match number.as_f64() {
                Some(0.0) => Ok(false),
                Some(1.0) => Ok(true),
                _ => Err(ArgError {
                    error_type: "bool_type",
                    loc,
                    message: "Input should be a valid boolean",
                    input: value.clone(),
                }),
            }
        }
        _ => Err(ArgError {
            error_type: "bool_type",
            loc,
            message: "Input should be a valid boolean",
            input: value.clone(),
        }),
    }
}

/// `list[str]` (probed): JSON arrays only; every non-string item —
/// including nulls and nested containers — is a `string_type` error at
/// `[field, index]`, and all of them are reported, not just the first.
pub fn coerce_str_list(field: &str, value: &Value) -> Result<Vec<String>, Vec<ArgError>> {
    match value {
        Value::Array(items) => {
            let mut out = Vec::with_capacity(items.len());
            let mut errors = Vec::new();
            for (index, item) in items.iter().enumerate() {
                match item {
                    Value::String(text) => out.push(text.clone()),
                    _ => errors.push(ArgError {
                        error_type: "string_type",
                        loc: vec![
                            LocSegment::Field(field.to_owned()),
                            LocSegment::Index(index),
                        ],
                        message: "Input should be a valid string",
                        input: item.clone(),
                    }),
                }
            }
            if errors.is_empty() {
                Ok(out)
            } else {
                Err(errors)
            }
        }
        _ => Err(vec![ArgError {
            error_type: "list_type",
            loc: vec![LocSegment::Field(field.to_owned())],
            message: "Input should be a valid list",
            input: value.clone(),
        }]),
    }
}

// ---------------------------------------------------------------------------
// Engine
// ---------------------------------------------------------------------------

/// Validate one tool-call's args object against its field specs: fields
/// in definition order (missing → default or `missing`), then unknown
/// keys in input order (`extra_forbidden`). Non-object args are a
/// single `dict_type` error. On success the coerced values, in spec
/// order, defaults filled.
pub fn validate_tool_args(
    spec: &[FieldSpec],
    args: &Value,
) -> Result<Vec<(String, Coerced)>, Vec<ArgError>> {
    let Some(object) = args.as_object() else {
        return Err(vec![ArgError {
            error_type: "dict_type",
            loc: vec![],
            message: "Input should be a valid dictionary",
            input: args.clone(),
        }]);
    };
    let mut out = Vec::with_capacity(spec.len());
    let mut errors = Vec::new();
    for field in spec {
        match object.get(field.name) {
            None => match field.default {
                Some(default) => out.push((field.name.to_owned(), default.to_coerced())),
                None => errors.push(ArgError {
                    error_type: "missing",
                    loc: vec![LocSegment::Field(field.name.to_owned())],
                    message: "Field required",
                    input: args.clone(),
                }),
            },
            Some(value) => match field.kind {
                FieldKind::Str => match coerce_str(field.name, value) {
                    Ok(text) => out.push((field.name.to_owned(), Coerced::Str(text))),
                    Err(error) => errors.push(error),
                },
                FieldKind::OptStr => match coerce_opt_str(field.name, value) {
                    Ok(None) => out.push((field.name.to_owned(), Coerced::Null)),
                    Ok(Some(text)) => out.push((field.name.to_owned(), Coerced::Str(text))),
                    Err(error) => errors.push(error),
                },
                FieldKind::Int => match coerce_int(field.name, value) {
                    Ok(int) => out.push((field.name.to_owned(), Coerced::Int(int))),
                    Err(error) => errors.push(error),
                },
                FieldKind::Bool => match coerce_bool(field.name, value) {
                    Ok(flag) => out.push((field.name.to_owned(), Coerced::Bool(flag))),
                    Err(error) => errors.push(error),
                },
                FieldKind::StrList => match coerce_str_list(field.name, value) {
                    Ok(list) => out.push((field.name.to_owned(), Coerced::List(list))),
                    Err(mut list_errors) => errors.append(&mut list_errors),
                },
            },
        }
    }
    for (key, value) in object {
        if !spec.iter().any(|field| field.name == key) {
            errors.push(ArgError {
                error_type: "extra_forbidden",
                loc: vec![LocSegment::Field(key.clone())],
                message: "Extra inputs are not permitted",
                input: value.clone(),
            });
        }
    }
    if errors.is_empty() {
        Ok(out)
    } else {
        Err(errors)
    }
}

// ---------------------------------------------------------------------------
// Per-tool specs (signature order, defaults from the Python signatures)
// ---------------------------------------------------------------------------

pub const SEARCH_ISSUES_SPEC: &[FieldSpec] = &[
    FieldSpec {
        name: "query",
        kind: FieldKind::Str,
        default: None,
    },
    FieldSpec {
        name: "project_id",
        kind: FieldKind::OptStr,
        default: Some(FieldDefault::Null),
    },
    FieldSpec {
        name: "limit",
        kind: FieldKind::Int,
        default: Some(FieldDefault::Int(20)),
    },
    FieldSpec {
        name: "offset",
        kind: FieldKind::Int,
        default: Some(FieldDefault::Int(0)),
    },
];

pub const LIST_ISSUES_SPEC: &[FieldSpec] = &[
    FieldSpec {
        name: "project_id",
        kind: FieldKind::Str,
        default: None,
    },
    FieldSpec {
        name: "state",
        kind: FieldKind::OptStr,
        default: Some(FieldDefault::Null),
    },
    FieldSpec {
        name: "state_group",
        kind: FieldKind::OptStr,
        default: Some(FieldDefault::Null),
    },
    FieldSpec {
        name: "parent_issue_id",
        kind: FieldKind::OptStr,
        default: Some(FieldDefault::Null),
    },
    FieldSpec {
        name: "labels",
        kind: FieldKind::OptStr,
        default: Some(FieldDefault::Null),
    },
    FieldSpec {
        name: "priority",
        kind: FieldKind::OptStr,
        default: Some(FieldDefault::Null),
    },
    FieldSpec {
        name: "limit",
        kind: FieldKind::Int,
        default: Some(FieldDefault::Int(20)),
    },
    FieldSpec {
        name: "offset",
        kind: FieldKind::Int,
        default: Some(FieldDefault::Int(0)),
    },
];

pub const LIST_MY_ISSUES_SPEC: &[FieldSpec] = &[
    FieldSpec {
        name: "scope",
        kind: FieldKind::Str,
        default: Some(FieldDefault::Str("all")),
    },
    FieldSpec {
        name: "limit",
        kind: FieldKind::Int,
        default: Some(FieldDefault::Int(20)),
    },
    FieldSpec {
        name: "offset",
        kind: FieldKind::Int,
        default: Some(FieldDefault::Int(0)),
    },
];

pub const GET_ISSUE_SPEC: &[FieldSpec] = &[FieldSpec {
    name: "issue_id",
    kind: FieldKind::Str,
    default: None,
}];

pub const CREATE_ISSUE_SPEC: &[FieldSpec] = &[
    FieldSpec {
        name: "project_id",
        kind: FieldKind::Str,
        default: None,
    },
    FieldSpec {
        name: "name",
        kind: FieldKind::Str,
        default: None,
    },
    FieldSpec {
        name: "description_md",
        kind: FieldKind::OptStr,
        default: Some(FieldDefault::Null),
    },
    FieldSpec {
        name: "state_id",
        kind: FieldKind::OptStr,
        default: Some(FieldDefault::Null),
    },
    FieldSpec {
        name: "priority",
        kind: FieldKind::OptStr,
        default: Some(FieldDefault::Null),
    },
    FieldSpec {
        name: "parent_issue_id",
        kind: FieldKind::OptStr,
        default: Some(FieldDefault::Null),
    },
];

pub const UPDATE_ISSUE_SPEC: &[FieldSpec] = &[
    FieldSpec {
        name: "issue_id",
        kind: FieldKind::Str,
        default: None,
    },
    FieldSpec {
        name: "name",
        kind: FieldKind::OptStr,
        default: Some(FieldDefault::Null),
    },
    FieldSpec {
        name: "description_md",
        kind: FieldKind::OptStr,
        default: Some(FieldDefault::Null),
    },
    FieldSpec {
        name: "state_id",
        kind: FieldKind::OptStr,
        default: Some(FieldDefault::Null),
    },
    FieldSpec {
        name: "priority",
        kind: FieldKind::OptStr,
        default: Some(FieldDefault::Null),
    },
    FieldSpec {
        name: "parent_issue_id",
        kind: FieldKind::OptStr,
        default: Some(FieldDefault::Str("__unset__")),
    },
];

pub const RELATE_ISSUES_SPEC: &[FieldSpec] = &[
    FieldSpec {
        name: "issue",
        kind: FieldKind::Str,
        default: None,
    },
    FieldSpec {
        name: "relation_type",
        kind: FieldKind::Str,
        default: None,
    },
    FieldSpec {
        name: "related_issues",
        kind: FieldKind::StrList,
        default: None,
    },
];

pub const ISSUE_REF_SPEC: &[FieldSpec] = &[FieldSpec {
    name: "issue",
    kind: FieldKind::Str,
    default: None,
}];

pub const PROJECT_ID_SPEC: &[FieldSpec] = &[FieldSpec {
    name: "project_id",
    kind: FieldKind::Str,
    default: None,
}];

pub const ISSUE_ID_SPEC: &[FieldSpec] = &[FieldSpec {
    name: "issue_id",
    kind: FieldKind::Str,
    default: None,
}];

pub const CREATE_COMMENT_SPEC: &[FieldSpec] = &[
    FieldSpec {
        name: "issue_id",
        kind: FieldKind::Str,
        default: None,
    },
    FieldSpec {
        name: "body_md",
        kind: FieldKind::Str,
        default: None,
    },
    FieldSpec {
        name: "fold",
        kind: FieldKind::Bool,
        default: Some(FieldDefault::Bool(false)),
    },
];

pub const PR_STATUS_SPEC: &[FieldSpec] = &[FieldSpec {
    name: "url",
    kind: FieldKind::Str,
    default: None,
}];

pub const DISPATCH_RUN_SPEC: &[FieldSpec] = &[
    FieldSpec {
        name: "issue_id",
        kind: FieldKind::Str,
        default: None,
    },
    FieldSpec {
        name: "target_state_id",
        kind: FieldKind::OptStr,
        default: Some(FieldDefault::Null),
    },
];

pub const NO_ARGS_SPEC: &[FieldSpec] = &[];

/// Spec lookup by tool name (`None` for unknown tools).
pub fn tool_spec(name: &str) -> Option<&'static [FieldSpec]> {
    match name {
        "search_issues" => Some(SEARCH_ISSUES_SPEC),
        "list_issues" => Some(LIST_ISSUES_SPEC),
        "list_my_issues" => Some(LIST_MY_ISSUES_SPEC),
        "get_issue" => Some(GET_ISSUE_SPEC),
        "create_issue" => Some(CREATE_ISSUE_SPEC),
        "update_issue" => Some(UPDATE_ISSUE_SPEC),
        "relate_issues" | "unrelate_issues" => Some(RELATE_ISSUES_SPEC),
        "list_issue_relations" => Some(ISSUE_REF_SPEC),
        "list_projects" => Some(NO_ARGS_SPEC),
        "list_states" | "list_labels" | "list_project_members" => Some(PROJECT_ID_SPEC),
        "get_run_status" => Some(ISSUE_ID_SPEC),
        "create_comment" => Some(CREATE_COMMENT_SPEC),
        "get_pull_request_status" => Some(PR_STATUS_SPEC),
        "dispatch_coding_run" => Some(DISPATCH_RUN_SPEC),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Typed args (engine output, one struct per tool)
// ---------------------------------------------------------------------------

fn find<'a>(values: &'a [(String, Coerced)], name: &str) -> &'a Coerced {
    values
        .iter()
        .find(|(key, _)| key == name)
        .map(|(_, value)| value)
        .unwrap_or_else(|| panic!("spec guarantees field {name}"))
}

fn take_str(values: &[(String, Coerced)], name: &str) -> String {
    match find(values, name) {
        Coerced::Str(text) => text.clone(),
        _ => panic!("spec guarantees {name} is a string"),
    }
}

fn take_opt_str(values: &[(String, Coerced)], name: &str) -> Option<String> {
    match find(values, name) {
        Coerced::Str(text) => Some(text.clone()),
        Coerced::Null => None,
        _ => panic!("spec guarantees {name} is an optional string"),
    }
}

fn take_int(values: &[(String, Coerced)], name: &str) -> i128 {
    match find(values, name) {
        Coerced::Int(int) => *int,
        _ => panic!("spec guarantees {name} is an int"),
    }
}

fn take_bool(values: &[(String, Coerced)], name: &str) -> bool {
    match find(values, name) {
        Coerced::Bool(flag) => *flag,
        _ => panic!("spec guarantees {name} is a bool"),
    }
}

fn take_list(values: &[(String, Coerced)], name: &str) -> Vec<String> {
    match find(values, name) {
        Coerced::List(list) => list.clone(),
        _ => panic!("spec guarantees {name} is a string list"),
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct SearchIssuesArgs {
    pub query: String,
    pub project_id: Option<String>,
    pub limit: i128,
    pub offset: i128,
}

impl SearchIssuesArgs {
    pub fn parse(args: &Value) -> Result<Self, Vec<ArgError>> {
        let values = validate_tool_args(SEARCH_ISSUES_SPEC, args)?;
        Ok(Self {
            query: take_str(&values, "query"),
            project_id: take_opt_str(&values, "project_id"),
            limit: take_int(&values, "limit"),
            offset: take_int(&values, "offset"),
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ListIssuesArgs {
    pub project_id: String,
    pub state: Option<String>,
    pub state_group: Option<String>,
    pub parent_issue_id: Option<String>,
    pub labels: Option<String>,
    pub priority: Option<String>,
    pub limit: i128,
    pub offset: i128,
}

impl ListIssuesArgs {
    pub fn parse(args: &Value) -> Result<Self, Vec<ArgError>> {
        let values = validate_tool_args(LIST_ISSUES_SPEC, args)?;
        Ok(Self {
            project_id: take_str(&values, "project_id"),
            state: take_opt_str(&values, "state"),
            state_group: take_opt_str(&values, "state_group"),
            parent_issue_id: take_opt_str(&values, "parent_issue_id"),
            labels: take_opt_str(&values, "labels"),
            priority: take_opt_str(&values, "priority"),
            limit: take_int(&values, "limit"),
            offset: take_int(&values, "offset"),
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ListMyIssuesArgs {
    pub scope: String,
    pub limit: i128,
    pub offset: i128,
}

impl ListMyIssuesArgs {
    pub fn parse(args: &Value) -> Result<Self, Vec<ArgError>> {
        let values = validate_tool_args(LIST_MY_ISSUES_SPEC, args)?;
        Ok(Self {
            scope: take_str(&values, "scope"),
            limit: take_int(&values, "limit"),
            offset: take_int(&values, "offset"),
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct GetIssueArgs {
    pub issue_id: String,
}

impl GetIssueArgs {
    pub fn parse(args: &Value) -> Result<Self, Vec<ArgError>> {
        let values = validate_tool_args(GET_ISSUE_SPEC, args)?;
        Ok(Self {
            issue_id: take_str(&values, "issue_id"),
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct CreateIssueArgs {
    pub project_id: String,
    pub name: String,
    pub description_md: Option<String>,
    pub state_id: Option<String>,
    pub priority: Option<String>,
    pub parent_issue_id: Option<String>,
}

impl CreateIssueArgs {
    pub fn parse(args: &Value) -> Result<Self, Vec<ArgError>> {
        let values = validate_tool_args(CREATE_ISSUE_SPEC, args)?;
        Ok(Self {
            project_id: take_str(&values, "project_id"),
            name: take_str(&values, "name"),
            description_md: take_opt_str(&values, "description_md"),
            state_id: take_opt_str(&values, "state_id"),
            priority: take_opt_str(&values, "priority"),
            parent_issue_id: take_opt_str(&values, "parent_issue_id"),
        })
    }
}

/// `update_issue.parent_issue_id` three ways (`issues.py:30-33,315-319`):
/// omitted (or the literal sentinel) leaves the parent untouched, an
/// explicit null unlinks, anything else re-parents.
#[derive(Debug, Clone, PartialEq)]
pub enum ParentArg {
    Unset,
    Null,
    Value(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct UpdateIssueArgs {
    pub issue_id: String,
    pub name: Option<String>,
    pub description_md: Option<String>,
    pub state_id: Option<String>,
    pub priority: Option<String>,
    pub parent_issue_id: ParentArg,
}

impl UpdateIssueArgs {
    pub fn parse(args: &Value) -> Result<Self, Vec<ArgError>> {
        let values = validate_tool_args(UPDATE_ISSUE_SPEC, args)?;
        let parent_issue_id = match take_opt_str(&values, "parent_issue_id") {
            // The engine fills a missing field with the `"__unset__"`
            // default, so `None` here is always an explicit null —
            // and, exactly as in Python (`!= _PARENT_UNSET`), an
            // explicit `"__unset__"` string also means untouched.
            None => ParentArg::Null,
            Some(text) if text == "__unset__" => ParentArg::Unset,
            Some(text) => ParentArg::Value(text),
        };
        Ok(Self {
            issue_id: take_str(&values, "issue_id"),
            name: take_opt_str(&values, "name"),
            description_md: take_opt_str(&values, "description_md"),
            state_id: take_opt_str(&values, "state_id"),
            priority: take_opt_str(&values, "priority"),
            parent_issue_id,
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct RelateIssuesArgs {
    pub issue: String,
    pub relation_type: String,
    pub related_issues: Vec<String>,
}

impl RelateIssuesArgs {
    pub fn parse(args: &Value) -> Result<Self, Vec<ArgError>> {
        let values = validate_tool_args(RELATE_ISSUES_SPEC, args)?;
        Ok(Self {
            issue: take_str(&values, "issue"),
            relation_type: take_str(&values, "relation_type"),
            related_issues: take_list(&values, "related_issues"),
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct IssueRefArgs {
    pub issue: String,
}

impl IssueRefArgs {
    pub fn parse(args: &Value) -> Result<Self, Vec<ArgError>> {
        let values = validate_tool_args(ISSUE_REF_SPEC, args)?;
        Ok(Self {
            issue: take_str(&values, "issue"),
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ProjectIdArgs {
    pub project_id: String,
}

impl ProjectIdArgs {
    pub fn parse(args: &Value) -> Result<Self, Vec<ArgError>> {
        let values = validate_tool_args(PROJECT_ID_SPEC, args)?;
        Ok(Self {
            project_id: take_str(&values, "project_id"),
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct IssueIdArgs {
    pub issue_id: String,
}

impl IssueIdArgs {
    pub fn parse(args: &Value) -> Result<Self, Vec<ArgError>> {
        let values = validate_tool_args(ISSUE_ID_SPEC, args)?;
        Ok(Self {
            issue_id: take_str(&values, "issue_id"),
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct CreateCommentArgs {
    pub issue_id: String,
    pub body_md: String,
    pub fold: bool,
}

impl CreateCommentArgs {
    pub fn parse(args: &Value) -> Result<Self, Vec<ArgError>> {
        let values = validate_tool_args(CREATE_COMMENT_SPEC, args)?;
        Ok(Self {
            issue_id: take_str(&values, "issue_id"),
            body_md: take_str(&values, "body_md"),
            fold: take_bool(&values, "fold"),
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct PrStatusArgs {
    pub url: String,
}

impl PrStatusArgs {
    pub fn parse(args: &Value) -> Result<Self, Vec<ArgError>> {
        let values = validate_tool_args(PR_STATUS_SPEC, args)?;
        Ok(Self {
            url: take_str(&values, "url"),
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct DispatchRunArgs {
    pub issue_id: String,
    pub target_state_id: Option<String>,
}

impl DispatchRunArgs {
    pub fn parse(args: &Value) -> Result<Self, Vec<ArgError>> {
        let values = validate_tool_args(DISPATCH_RUN_SPEC, args)?;
        Ok(Self {
            issue_id: take_str(&values, "issue_id"),
            target_state_id: take_opt_str(&values, "target_state_id"),
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct NoArgs;

impl NoArgs {
    pub fn parse(args: &Value) -> Result<Self, Vec<ArgError>> {
        validate_tool_args(NO_ARGS_SPEC, args)?;
        Ok(Self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture_cases() -> Vec<Value> {
        let path = format!(
            "{}/../../fixtures/assistant/tools-local-args.json",
            env!("CARGO_MANIFEST_DIR")
        );
        let fixture: Value =
            serde_json::from_str(&std::fs::read_to_string(&path).expect("fixture exists"))
                .expect("fixture parses");
        fixture["cases"].as_array().expect("cases array").clone()
    }

    /// The probe tools mirror real signatures: `probe_search` ≡
    /// `search_issues`, `probe_comment` ≡ `create_comment`,
    /// `probe_relate` ≡ `relate_issues` (rounds 3-4 use partial
    /// signatures — handled below), `probe_scope` ≡ the `scope` field
    /// of `list_my_issues`.
    fn spec_for_probe(tool: &str) -> &'static [FieldSpec] {
        match tool {
            "probe_search" => SEARCH_ISSUES_SPEC,
            "probe_comment" => CREATE_COMMENT_SPEC,
            "probe_relate" => RELATE_ISSUES_SPEC,
            "probe_scope" => LIST_MY_ISSUES_SPEC,
            _ => panic!("unknown probe tool {tool}"),
        }
    }

    fn coerced_to_json(value: &Coerced) -> Value {
        match value {
            Coerced::Str(text) => Value::String(text.clone()),
            // JSON has no i128: realistic values fit i64 and compare
            // exactly; the absurd-magnitude vectors are asserted
            // separately below.
            Coerced::Int(int) => Value::from(i64::try_from(*int).expect("fits i64")),
            Coerced::Bool(flag) => Value::Bool(*flag),
            Coerced::List(list) => Value::Array(list.iter().cloned().map(Value::String).collect()),
            Coerced::Null => Value::Null,
        }
    }

    /// Every probed vector validates exactly as pydantic-ai did: same
    /// accept/reject, same coerced values, same `ErrorDetails` in the
    /// same order.
    #[test]
    fn validation_matches_probed_pydantic_ai_vectors() {
        let cases = fixture_cases();
        assert_eq!(cases.len(), 103, "all probe rounds recorded");
        for (index, case) in cases.iter().enumerate() {
            let tool = case["tool"].as_str().unwrap();
            let mut input = case["input"].clone();
            let mut expected_value = case["value"].clone();
            // Rounds 3-4 probed `fold` with a partial `(issue_id, fold)`
            // signature (always carrying `fold`); the real
            // `create_comment` also requires `body_md`, which validates
            // independently (see the multi-error vectors), so fill it on
            // the input side (and on the expected side for passing
            // cases). The round-1 vector omitting both fields
            // intentionally tests `missing` and is left alone.
            if tool == "probe_comment"
                && input.get("body_md").is_none()
                && input.get("fold").is_some()
            {
                input["body_md"] = Value::String("b".to_owned());
                if expected_value.get("body_md").is_none() && expected_value.is_object() {
                    expected_value["body_md"] = Value::String("b".to_owned());
                }
            }
            let result = validate_tool_args(spec_for_probe(tool), &input);
            if case["ok"] == Value::Bool(true) {
                let actual = result.unwrap_or_else(|errors| {
                    panic!("case {index} ({tool} {input}) should pass, got {errors:?}")
                });
                let expected = expected_value.as_object().unwrap();
                for (key, expected_field) in expected {
                    let actual_field = actual
                        .iter()
                        .find(|(name, _)| name == key)
                        .unwrap_or_else(|| panic!("case {index}: missing {key}"));
                    if expected_field.is_number() && expected_field.as_i64().is_none() {
                        // The ±2^70 vectors: Python's unbounded int
                        // accepts; assert acceptance plus saturation
                        // past i64 (documented on `coerce_int`).
                        match actual_field.1 {
                            Coerced::Int(int) => assert!(
                                int > i128::from(i64::MAX) || int < i128::from(i64::MIN),
                                "case {index}: big int saturates past i64"
                            ),
                            _ => panic!("case {index}: big int stays an int"),
                        }
                    } else {
                        assert_eq!(
                            coerced_to_json(&actual_field.1),
                            *expected_field,
                            "case {index} ({tool} {input}): field {key}"
                        );
                    }
                }
            } else {
                let actual = result.unwrap_err();
                let expected = case["errors"].as_array().unwrap();
                let rendered: Vec<Value> = actual.iter().map(ArgError::to_value).collect();
                assert_eq!(
                    rendered, *expected,
                    "case {index} ({tool} {input}): error details"
                );
            }
        }
    }

    #[test]
    fn update_parent_three_ways() {
        let unset = UpdateIssueArgs::parse(&serde_json::json!({"issue_id": "i"})).expect("parses");
        assert_eq!(unset.parent_issue_id, ParentArg::Unset);
        let explicit = UpdateIssueArgs::parse(
            &serde_json::json!({"issue_id": "i", "parent_issue_id": "__unset__"}),
        )
        .expect("parses");
        assert_eq!(explicit.parent_issue_id, ParentArg::Unset);
        let null =
            UpdateIssueArgs::parse(&serde_json::json!({"issue_id": "i", "parent_issue_id": null}))
                .expect("parses");
        assert_eq!(null.parent_issue_id, ParentArg::Null);
        let value =
            UpdateIssueArgs::parse(&serde_json::json!({"issue_id": "i", "parent_issue_id": "p"}))
                .expect("parses");
        assert_eq!(value.parent_issue_id, ParentArg::Value("p".to_owned()));
    }

    #[test]
    fn tool_spec_covers_all_seventeen_tools() {
        for name in super::super::tool_schemas::tool_names_sorted() {
            assert!(tool_spec(name).is_some(), "spec for {name}");
        }
        assert!(tool_spec("nope").is_none());
    }
}
