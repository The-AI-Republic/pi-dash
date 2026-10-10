//! Lax argument + output validation with pydantic-core error rendering.
//!
//! Ports the pydantic-ai 1.107.0 function-tool and output validation the
//! cloud-agent loop depends on (`ToolManager.validate_tool_call`,
//! `validate_output_tool_call`, the text-output path): JSON-shape checks,
//! lax scalar coercion, and the exact `ErrorDetails` rendering fed back to
//! the model as retry prompts. Every message shape below was captured from
//! stub-driven `Agent.run` probes (see `fx-disp-08-model-runtime` battery
//! and the PIDASHCONV-866 workpad); the unit tests replay the golden
//! battery byte-for-byte.
//!
//! Validated shapes (the only ones the 15 granted tools + `final_result`
//! use): `str` (no coercion), `int` (lax), `list[str]`, the output object.
//! Anything else in a schema is a [`SpecError`] (fail-closed: the golden
//! is the contract, so an unknown shape is a bug, never a guess).

use super::schemas::ToolSchema;
use pidash_types::dispatch::output::{CloudAgentOutput, Outcome};
use serde_json::{Map, Value};

/// A single pydantic-core-style error detail (`type`/`loc`/`msg`/`input`,
/// context excluded, as `error.errors(include_url=False,
/// include_context=False)`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidationDetail {
    /// `missing`, `int_type`, `string_too_long`, ...
    pub error_type: &'static str,
    /// Location path (field names / list indices).
    pub loc: Vec<LocSeg>,
    /// The human message (`Field required`, ...).
    pub msg: String,
    /// The offending input value (original, pre-coercion/strip).
    pub input: Value,
}

/// A location segment: object field or list index.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LocSeg {
    Field(String),
    Index(usize),
}

impl ValidationDetail {
    fn new(error_type: &'static str, loc: Vec<LocSeg>, msg: String, input: Value) -> Self {
        ValidationDetail {
            error_type,
            loc,
            msg,
            input,
        }
    }

    /// Render as a `{"type","loc","msg","input"}` object (that key order).
    fn to_json(&self) -> Value {
        let mut map = Map::with_capacity(4);
        map.insert(
            "type".to_string(),
            Value::String(self.error_type.to_string()),
        );
        map.insert(
            "loc".to_string(),
            Value::Array(
                self.loc
                    .iter()
                    .map(|seg| match seg {
                        LocSeg::Field(name) => Value::String(name.clone()),
                        LocSeg::Index(index) => {
                            Value::Number(serde_json::Number::from(*index as u64))
                        }
                    })
                    .collect(),
            ),
        );
        map.insert("msg".to_string(), Value::String(self.msg.clone()));
        map.insert("input".to_string(), self.input.clone());
        Value::Object(map)
    }
}

/// Render the model-facing retry prompt for one or more details:
///
/// `{n} validation error(s):\n```json\n{details}\n```\n\nFix the errors
/// and try again.`
///
/// (`RetryPromptPart.model_response`, `messages.py:1445-1467`.) The JSON
/// block matches `json.dumps(errors, indent=2)` byte-for-byte, including
/// `ensure_ascii` escaping of non-ASCII text. Known micro-divergence:
/// extreme-float exponents inside echoed inputs render in serde/ryu style
/// rather than CPython `repr` style (`1e30` vs `1e+30`); the values are
/// identical once parsed.
pub fn render_retry_message(details: &[ValidationDetail]) -> String {
    let noun = if details.len() == 1 {
        "error"
    } else {
        "errors"
    };
    let values: Vec<Value> = details.iter().map(ValidationDetail::to_json).collect();
    let array = Value::Array(values);
    let pretty = serde_json::to_string_pretty(&array).unwrap_or_else(|_| "[]".to_string());
    format!(
        "{} validation {noun}:\n```json\n{}\n```\n\nFix the errors and try again.",
        details.len(),
        ensure_ascii(&pretty),
    )
}

/// Re-escape non-ASCII text in an already-serialized JSON document the way
/// CPython `json.dumps` does with `ensure_ascii=True`: every `c >= 0x7F`
/// inside a string becomes `\uXXXX` (lowercase hex; surrogate pairs for
/// astral planes). String boundaries are tracked with escape awareness;
/// structural characters are never touched.
fn ensure_ascii(pretty: &str) -> String {
    let mut out = String::with_capacity(pretty.len());
    let mut chars = pretty.chars().peekable();
    let mut in_string = false;
    let mut escaped = false;
    while let Some(c) = chars.next() {
        if !in_string {
            out.push(c);
            if c == '"' {
                in_string = true;
            }
            continue;
        }
        if escaped {
            out.push(c);
            escaped = false;
            continue;
        }
        match c {
            '\\' => {
                out.push(c);
                escaped = true;
            }
            '"' => {
                out.push(c);
                in_string = false;
            }
            c if (c as u32) < 0x7f => out.push(c),
            c => {
                let code = c as u32;
                if code <= 0xffff {
                    out.push_str(&format!("\\u{code:04x}"));
                } else {
                    let shifted = code - 0x1_0000;
                    let high = 0xd800 + (shifted >> 10);
                    let low = 0xdc00 + (shifted & 0x3ff);
                    out.push_str(&format!("\\u{high:04x}\\u{low:04x}"));
                }
            }
        }
    }
    out
}

/// Unknown-tool retry text (`Tool 'x' is unknown` path,
/// `tool_manager.py:356-369`): the available names are byte-sorted and
/// include the output tool.
pub fn unknown_tool_message(name: &str, available_sorted: &[&str]) -> String {
    let quoted: Vec<String> = available_sorted
        .iter()
        .map(|tool| format!("'{tool}'"))
        .collect();
    format!(
        "Unknown tool name: '{name}'. Available tools: {}\n\nFix the errors and try again.",
        quoted.join(", ")
    )
}

/// A validated parameter value (the only shapes tool executors consume).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValidatedValue {
    Str(String),
    Int(i64),
    StrList(Vec<String>),
}

/// Validated tool arguments, keyed by parameter name (defaults applied).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ValidatedArgs {
    values: std::collections::BTreeMap<String, ValidatedValue>,
}

impl ValidatedArgs {
    /// Fetch a validated string parameter.
    pub fn get_str(&self, name: &str) -> Option<&str> {
        match self.values.get(name) {
            Some(ValidatedValue::Str(text)) => Some(text),
            _ => None,
        }
    }

    /// Fetch a validated integer parameter.
    pub fn get_int(&self, name: &str) -> Option<i64> {
        match self.values.get(name) {
            Some(ValidatedValue::Int(value)) => Some(*value),
            _ => None,
        }
    }

    /// Fetch a validated string-list parameter.
    pub fn get_str_list(&self, name: &str) -> Option<&[String]> {
        match self.values.get(name) {
            Some(ValidatedValue::StrList(items)) => Some(items),
            _ => None,
        }
    }
}

/// A schema-derived parameter specification.
#[derive(Debug, Clone)]
struct ParamSpec {
    name: String,
    kind: ParamKind,
    required: bool,
    default: Option<ValidatedValue>,
}

/// The parameter kinds the granted tools use.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ParamKind {
    Str,
    Int,
    StrList,
}

/// A tool schema the runtime cannot validate (unknown shape in the
/// golden). Fail-closed: the caller surfaces this as an internal error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpecError(pub String);

impl std::fmt::Display for SpecError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "unsupported tool parameter shape: {}", self.0)
    }
}

impl std::error::Error for SpecError {}

/// Derive parameter specs from a golden tool schema, in `properties`
/// order (pydantic validates in signature order).
fn param_specs(schema: &ToolSchema) -> Result<Vec<ParamSpec>, SpecError> {
    let properties = schema
        .parameters
        .get("properties")
        .and_then(Value::as_object)
        .ok_or_else(|| SpecError(format!("schema `{}` has no properties", schema.name)))?;
    let required: Vec<&str> = schema
        .parameters
        .get("required")
        .and_then(Value::as_array)
        .map(|items| items.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    let mut specs = Vec::with_capacity(properties.len());
    for (name, def) in properties {
        let kind = match def.get("type").and_then(Value::as_str) {
            Some("string") => ParamKind::Str,
            Some("integer") => ParamKind::Int,
            Some("array") => {
                let items_type = def
                    .get("items")
                    .and_then(|items| items.get("type"))
                    .and_then(Value::as_str);
                if items_type != Some("string") {
                    return Err(SpecError(format!(
                        "schema `{}` param `{name}`: only array-of-string is supported",
                        schema.name
                    )));
                }
                ParamKind::StrList
            }
            other => {
                return Err(SpecError(format!(
                    "schema `{}` param `{name}`: unsupported type {other:?}",
                    schema.name
                )))
            }
        };
        let is_required = required.iter().any(|field| *field == name);
        let default = match def.get("default") {
            None => None,
            Some(value) => Some(default_value(&schema.name, name, kind, value)?),
        };
        specs.push(ParamSpec {
            name: name.clone(),
            kind,
            required: is_required,
            default,
        });
    }
    Ok(specs)
}

fn default_value(
    tool: &str,
    param: &str,
    kind: ParamKind,
    value: &Value,
) -> Result<ValidatedValue, SpecError> {
    let bad = || {
        SpecError(format!(
            "schema `{tool}` param `{param}`: bad default {value}"
        ))
    };
    match kind {
        ParamKind::Str => value
            .as_str()
            .map(|text| ValidatedValue::Str(text.to_string()))
            .ok_or_else(bad),
        ParamKind::Int => value.as_i64().map(ValidatedValue::Int).ok_or_else(bad),
        ParamKind::StrList => value
            .as_array()
            .and_then(|items| {
                items
                    .iter()
                    .map(Value::as_str)
                    .collect::<Option<Vec<_>>>()
                    .map(|strs| {
                        ValidatedValue::StrList(strs.into_iter().map(str::to_string).collect())
                    })
            })
            .ok_or_else(bad),
    }
}

/// Validate a raw tool-call `arguments` string against a tool schema.
///
/// A JSON syntax failure yields the single `json_invalid` detail;
/// otherwise arguments validate as an object (see [`validate_args`]).
pub fn validate_args_text(
    schema: &ToolSchema,
    text: &str,
) -> Result<ValidatedArgs, Vec<ValidationDetail>> {
    match serde_json::from_str::<Value>(text) {
        Ok(value) => validate_args_value(schema, &value),
        Err(err) => Err(vec![ValidationDetail::new(
            "json_invalid",
            Vec::new(),
            format!("Invalid JSON: {err}"),
            Value::String(text.to_string()),
        )]),
    }
}

/// Validate an already-parsed arguments value against a tool schema.
///
/// Non-object input yields the single `dict_type` detail; otherwise each
/// known parameter validates in schema order, then unknown keys yield
/// `extra_forbidden` in input order.
pub fn validate_args_value(
    schema: &ToolSchema,
    args: &Value,
) -> Result<ValidatedArgs, Vec<ValidationDetail>> {
    let specs = param_specs(schema).map_err(|err| {
        vec![ValidationDetail::new(
            "internal",
            Vec::new(),
            err.to_string(),
            args.clone(),
        )]
    })?;
    let object = match args.as_object() {
        Some(object) => object,
        None => {
            return Err(vec![ValidationDetail::new(
                "dict_type",
                Vec::new(),
                "Input should be an object".to_string(),
                args.clone(),
            )]);
        }
    };
    validate_args(&specs, object, args)
}

fn validate_args(
    specs: &[ParamSpec],
    object: &Map<String, Value>,
    whole: &Value,
) -> Result<ValidatedArgs, Vec<ValidationDetail>> {
    let mut details = Vec::new();
    let mut values = std::collections::BTreeMap::new();
    for spec in specs {
        match object.get(&spec.name) {
            None => {
                if spec.required {
                    details.push(ValidationDetail::new(
                        "missing",
                        vec![LocSeg::Field(spec.name.clone())],
                        "Field required".to_string(),
                        whole.clone(),
                    ));
                } else if let Some(default) = spec.default.clone() {
                    values.insert(spec.name.clone(), default);
                }
            }
            Some(value) => match validate_param(spec, value) {
                Ok(valid) => {
                    values.insert(spec.name.clone(), valid);
                }
                Err(detail) => details.push(detail),
            },
        }
    }
    for (key, value) in object {
        if !specs.iter().any(|spec| spec.name == *key) {
            details.push(ValidationDetail::new(
                "extra_forbidden",
                vec![LocSeg::Field(key.clone())],
                "Extra inputs are not permitted".to_string(),
                value.clone(),
            ));
        }
    }
    if details.is_empty() {
        Ok(ValidatedArgs { values })
    } else {
        Err(details)
    }
}

fn validate_param(spec: &ParamSpec, value: &Value) -> Result<ValidatedValue, ValidationDetail> {
    let loc = vec![LocSeg::Field(spec.name.clone())];
    match spec.kind {
        ParamKind::Str => match value.as_str() {
            Some(text) => Ok(ValidatedValue::Str(text.to_string())),
            None => Err(ValidationDetail::new(
                "string_type",
                loc,
                "Input should be a valid string".to_string(),
                value.clone(),
            )),
        },
        ParamKind::Int => match lax_int(value) {
            Ok(number) => Ok(ValidatedValue::Int(number)),
            Err((error_type, msg)) => {
                Err(ValidationDetail::new(error_type, loc, msg, value.clone()))
            }
        },
        ParamKind::StrList => match value.as_array() {
            None => Err(ValidationDetail::new(
                "list_type",
                loc,
                "Input should be a valid array".to_string(),
                value.clone(),
            )),
            Some(items) => {
                // Relate lists carry no length cap; every item must be a string.
                let mut validated = Vec::with_capacity(items.len());
                for item in items {
                    match item.as_str() {
                        Some(text) => validated.push(text.to_string()),
                        None => {
                            return Err(ValidationDetail::new(
                                "string_type",
                                vec![
                                    LocSeg::Field(spec.name.clone()),
                                    LocSeg::Index(validated.len()),
                                ],
                                "Input should be a valid string".to_string(),
                                item.clone(),
                            ));
                        }
                    }
                }
                Ok(ValidatedValue::StrList(validated))
            }
        },
    }
}

/// Pydantic lax `int` coercion, probed case-by-case (see the workpad):
/// JSON integers (arbitrary precision, saturated to `i64` — the only
/// integer parameter, `limit`, is clamped downstream so saturation is
/// exact); integral floats inside the open `(-2^63, 2^63)` range; bools;
/// strings matching `[+-]?digits(_digits)*(\.0+)?` after trimming
/// (single underscores between digits only, fraction all zeros).
fn lax_int(value: &Value) -> Result<i64, (&'static str, String)> {
    match value {
        Value::Number(number) => {
            if let Some(int) = number.as_i64() {
                return Ok(int);
            }
            if let Some(uint) = number.as_u64() {
                return Ok(i64::try_from(uint).unwrap_or(i64::MAX));
            }
            match number.as_f64() {
                Some(float)
                    if float.fract() == 0.0
                        && float > -9_223_372_036_854_775_808.0
                        && float < 9_223_372_036_854_775_808.0 =>
                {
                    Ok(float as i64)
                }
                Some(float) if float.fract() != 0.0 => Err((
                    "int_from_float",
                    "Input should be a valid integer, got a number with a fractional part"
                        .to_string(),
                )),
                _ => Err((
                    "int_parsing_size",
                    "Unable to parse input string as an integer, exceeded maximum size".to_string(),
                )),
            }
        }
        Value::Bool(flag) => Ok(i64::from(*flag)),
        Value::String(text) => lax_int_from_str(text).ok_or_else(|| {
            (
                "int_parsing",
                "Input should be a valid integer, unable to parse string as an integer".to_string(),
            )
        }),
        _ => Err(("int_type", "Input should be a valid integer".to_string())),
    }
}

fn lax_int_from_str(text: &str) -> Option<i64> {
    let trimmed = text.trim();
    let (negative, rest) = match trimmed.strip_prefix(['+', '-']) {
        Some(rest) => (trimmed.starts_with('-'), rest),
        None => (false, trimmed),
    };
    let (int_part, frac_part) = match rest.split_once('.') {
        Some((int_part, frac_part)) => (int_part, Some(frac_part)),
        None => (rest, None),
    };
    if int_part.is_empty()
        || !int_part
            .bytes()
            .all(|byte| byte.is_ascii_digit() || byte == b'_')
        || int_part.starts_with('_')
        || int_part.ends_with('_')
        || int_part.as_bytes().windows(2).any(|pair| pair == b"__")
    {
        return None;
    }
    if let Some(frac_part) = frac_part {
        if frac_part.is_empty() || !frac_part.bytes().all(|byte| byte == b'0') {
            return None;
        }
    }
    let digits: String = int_part
        .bytes()
        .filter(|byte| *byte != b'_')
        .map(char::from)
        .collect();
    let magnitude: i128 = digits.parse().unwrap_or(i128::MAX);
    let signed = if negative { -magnitude } else { magnitude };
    Some(signed.clamp(i128::from(i64::MIN), i128::from(i64::MAX)) as i64)
}

/// Validate `final_result` arguments into a [`CloudAgentOutput`].
///
/// Fields validate in model order (`outcome`, `summary`, `evidence`,
/// `limitations`); unknown keys are ignored (pydantic default
/// `extra='ignore'`); an over-long list yields only `too_long` (item
/// errors suppressed, probed); item length errors report the original
/// (unstripped) item as input.
pub fn validate_output(args: &Value) -> Result<CloudAgentOutput, Vec<ValidationDetail>> {
    let object = match args.as_object() {
        Some(object) => object,
        None => {
            return Err(vec![ValidationDetail::new(
                "model_type",
                Vec::new(),
                "Input should be an object".to_string(),
                args.clone(),
            )]);
        }
    };
    let mut details = Vec::new();
    let outcome = match object.get("outcome") {
        None => {
            details.push(ValidationDetail::new(
                "missing",
                vec![LocSeg::Field("outcome".to_string())],
                "Field required".to_string(),
                args.clone(),
            ));
            None
        }
        Some(value) => match value.as_str() {
            Some("completed") => Some(Outcome::Completed),
            Some("blocked") => Some(Outcome::Blocked),
            Some("noop") => Some(Outcome::Noop),
            _ => {
                details.push(ValidationDetail::new(
                    "literal_error",
                    vec![LocSeg::Field("outcome".to_string())],
                    "Input should be 'completed', 'blocked' or 'noop'".to_string(),
                    value.clone(),
                ));
                None
            }
        },
    };
    let summary = match object.get("summary") {
        None => {
            details.push(ValidationDetail::new(
                "missing",
                vec![LocSeg::Field("summary".to_string())],
                "Field required".to_string(),
                args.clone(),
            ));
            None
        }
        Some(value) => match value.as_str() {
            None => {
                details.push(ValidationDetail::new(
                    "string_type",
                    vec![LocSeg::Field("summary".to_string())],
                    "Input should be a valid string".to_string(),
                    value.clone(),
                ));
                None
            }
            Some(text) => {
                if text.chars().count() > 30_000 {
                    details.push(ValidationDetail::new(
                        "string_too_long",
                        vec![LocSeg::Field("summary".to_string())],
                        "String should have at most 30000 characters".to_string(),
                        value.clone(),
                    ));
                    None
                } else {
                    Some(text.to_string())
                }
            }
        },
    };
    let evidence = validate_output_items(object, "evidence", 10, 1_000, &mut details);
    let limitations = validate_output_items(object, "limitations", 10, 500, &mut details);
    if !details.is_empty() {
        return Err(details);
    }
    let (Some(outcome), Some(summary), Some(evidence), Some(limitations)) =
        (outcome, summary, evidence, limitations)
    else {
        // Unreachable: every `None` above pushes a detail.
        return Err(details);
    };
    CloudAgentOutput::new(outcome, summary, evidence, limitations).map_err(|err| {
        // Defensive: the checks above already enforce the constructor's
        // constraints, so this only fires on an internal mismatch.
        vec![ValidationDetail::new(
            "internal",
            vec![LocSeg::Field(err.field().to_string())],
            err.to_string(),
            args.clone(),
        )]
    })
}

fn validate_output_items(
    object: &Map<String, Value>,
    field: &str,
    max_items: usize,
    max_chars: usize,
    details: &mut Vec<ValidationDetail>,
) -> Option<Vec<String>> {
    let value = match object.get(field) {
        None => return Some(Vec::new()),
        Some(value) => value,
    };
    let items = match value.as_array() {
        Some(items) => items,
        None => {
            details.push(ValidationDetail::new(
                "list_type",
                vec![LocSeg::Field(field.to_string())],
                "Input should be a valid array".to_string(),
                value.clone(),
            ));
            return None;
        }
    };
    if items.len() > max_items {
        details.push(ValidationDetail::new(
            "too_long",
            vec![LocSeg::Field(field.to_string())],
            format!(
                "List should have at most {max_items} items after validation, not {}",
                items.len()
            ),
            value.clone(),
        ));
        return None;
    }
    let mut validated = Vec::with_capacity(items.len());
    for (index, item) in items.iter().enumerate() {
        match item.as_str() {
            None => {
                details.push(ValidationDetail::new(
                    "string_type",
                    vec![LocSeg::Field(field.to_string()), LocSeg::Index(index)],
                    "Input should be a valid string".to_string(),
                    item.clone(),
                ));
                return None;
            }
            Some(text) => {
                // Strip-then-length, storing the stripped value (the
                // ported constructor agrees); the error reports the
                // original item (probed).
                let stripped = text.trim();
                if stripped.chars().count() > max_chars {
                    details.push(ValidationDetail::new(
                        "string_too_long",
                        vec![LocSeg::Field(field.to_string()), LocSeg::Index(index)],
                        format!("String should have at most {max_chars} characters"),
                        item.clone(),
                    ));
                    return None;
                }
                validated.push(stripped.to_string());
            }
        }
    }
    Some(validated)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model_runtime::schemas::Golden;

    /// Replay every golden battery case: invalid inputs must reproduce the
    /// recorded retry message byte-for-byte; valid inputs must validate.
    #[test]
    fn battery_replay_matches_golden() {
        let golden = Golden::load().expect("golden parses");
        let battery = golden
            .document()
            .get("battery")
            .and_then(Value::as_array)
            .expect("battery array");
        assert_eq!(battery.len(), 17, "battery case count");
        for case in battery {
            let label = case["label"].as_str().unwrap_or("?");
            let tool = case["tool"].as_str().unwrap_or("?");
            let raw = case["input_args_raw"].as_str().unwrap_or("");
            // Successful calls also record the follow-up tool-result
            // message; only retry prompts carry the fix-errors trailer.
            let failed = case
                .get("retry_message")
                .and_then(|message| message.get("content"))
                .and_then(Value::as_str)
                .is_some_and(|content| content.contains("Fix the errors and try again."));
            if tool == "final_result" {
                let args: Value = serde_json::from_str(raw).expect("battery args parse");
                match validate_output(&args) {
                    Ok(_) => assert!(!failed, "{label}: expected failure, output validated"),
                    Err(details) => {
                        assert!(failed, "{label}: expected success, got {details:?}");
                        let expected = case["retry_message"]["content"]
                            .as_str()
                            .expect("retry content");
                        assert_eq!(
                            render_retry_message(&details),
                            expected,
                            "{label}: retry message drift"
                        );
                    }
                }
                continue;
            }
            let Some(schema) = golden.function_schema(tool) else {
                // The unknown-tool battery case: the retry text names the
                // available tools (that run granted the search replica).
                assert_eq!(tool, "nope_tool", "{label}: unexpected unknown tool");
                let expected = case["retry_message"]["content"]
                    .as_str()
                    .expect("retry content");
                assert_eq!(
                    unknown_tool_message(tool, &["final_result", "pidash_search_project_issues"]),
                    expected,
                    "{label}: unknown-tool message drift"
                );
                continue;
            };
            match validate_args_text(schema, raw) {
                Ok(args) => {
                    assert!(!failed, "{label}: expected failure, args validated");
                    let recorded = case
                        .get("recorded")
                        .and_then(|recorded| recorded.get(tool).and_then(Value::as_object));
                    if let Some(recorded) = recorded {
                        for (param, value) in recorded {
                            match value {
                                Value::String(text) => {
                                    assert_eq!(
                                        args.get_str(param),
                                        Some(text.as_str()),
                                        "{label}: coerced str param `{param}`"
                                    );
                                }
                                Value::Number(number) => {
                                    assert_eq!(
                                        args.get_int(param),
                                        number.as_i64(),
                                        "{label}: coerced int param `{param}`"
                                    );
                                }
                                other => panic!("{label}: unexpected recorded shape {other}"),
                            }
                        }
                    }
                }
                Err(details) => {
                    assert!(failed, "{label}: expected success, got {details:?}");
                    let expected = case["retry_message"]["content"]
                        .as_str()
                        .expect("retry content");
                    assert_eq!(
                        render_retry_message(&details),
                        expected,
                        "{label}: retry message drift"
                    );
                }
            }
        }
    }

    #[test]
    fn int_lax_edges_match_probes() {
        let cases: Vec<(&str, Result<i64, &str>)> = vec![
            ("5", Ok(5)),
            (" 5 ", Ok(5)),
            ("+5", Ok(5)),
            ("-5", Ok(-5)),
            ("5.0", Ok(5)),
            ("5.00", Ok(5)),
            ("  -12_34.0  ", Ok(-1234)),
            ("12_34_56", Ok(123456)),
            ("0_0", Ok(0)),
            ("0", Ok(0)),
            ("007", Ok(7)),
            ("-0", Ok(0)),
            ("-0.0", Ok(0)),
            ("0x10", Err("int_parsing")),
            ("1__0", Err("int_parsing")),
            ("_5", Err("int_parsing")),
            ("5_", Err("int_parsing")),
            ("", Err("int_parsing")),
            ("  ", Err("int_parsing")),
            ("5e2", Err("int_parsing")),
            ("5.5", Err("int_parsing")),
            ("5.", Err("int_parsing")),
            (".5", Err("int_parsing")),
            ("inf", Err("int_parsing")),
            ("nan", Err("int_parsing")),
            ("５", Err("int_parsing")),
        ];
        for (text, expected) in cases {
            let got = lax_int(&Value::String(text.to_string()));
            match expected {
                Ok(number) => assert_eq!(got, Ok(number), "int text {text:?}"),
                Err(kind) => assert_eq!(
                    got.map_err(|(kind, _)| kind),
                    Err(kind),
                    "int text {text:?}"
                ),
            }
        }
        assert_eq!(lax_int(&serde_json::json!(5.0)), Ok(5));
        assert_eq!(lax_int(&serde_json::json!(-0.0)), Ok(0));
        assert_eq!(lax_int(&serde_json::json!(true)), Ok(1));
        assert_eq!(lax_int(&serde_json::json!(false)), Ok(0));
        assert_eq!(
            lax_int(&serde_json::json!(5.5)).map_err(|(kind, _)| kind),
            Err("int_from_float")
        );
        assert_eq!(
            lax_int(&serde_json::json!(1e30)).map_err(|(kind, _)| kind),
            Err("int_parsing_size")
        );
        assert_eq!(
            lax_int(&serde_json::json!(null)).map_err(|(kind, _)| kind),
            Err("int_type")
        );
        // Arbitrary-precision integers saturate (downstream clamp exact).
        assert_eq!(
            lax_int(&Value::String("99999999999999999999999".to_string())),
            Ok(i64::MAX)
        );
        assert_eq!(
            lax_int(&serde_json::json!(9_223_372_036_854_775_807u64)),
            Ok(i64::MAX)
        );
    }

    #[test]
    fn unknown_tool_message_matches_probe() {
        assert_eq!(
            unknown_tool_message("nope_tool", &["final_result", "probe_tool"]),
            "Unknown tool name: 'nope_tool'. Available tools: 'final_result', 'probe_tool'\n\nFix the errors and try again."
        );
    }

    #[test]
    fn ensure_ascii_matches_cpython() {
        assert_eq!(ensure_ascii(r#""caf\u00e9""#), r#""caf\u00e9""#);
        assert_eq!(ensure_ascii("\"café\""), r#""caf\u00e9""#);
        assert_eq!(ensure_ascii("\"😀\""), r#""\ud83d\ude00""#);
        assert_eq!(ensure_ascii("\"a\x7fb\""), r#""a\u007fb""#);
        // Structural characters and existing escapes pass through.
        assert_eq!(ensure_ascii("{\n  \"a\": 1\n}"), "{\n  \"a\": 1\n}");
        assert_eq!(ensure_ascii(r#""a\nb""#), r#""a\nb""#);
    }
}
