#![forbid(unsafe_code)]

//! D-35 porter + exporter format engines (stage 5, PIDASHCONV-381).
//!
//! Port of the format layer named by the issue:
//!
//! * `utils/porters/formatters.py:25-274` — `JSONFormatter`, `CSVFormatter`
//!   (flatten/prettify/sanitize/decode), `XLSXFormatter` (cell rules, empty
//!   workbook, read-only decode).
//! * `utils/porters/exporter.py:9-107` — `DataExporter` (formats, ctor
//!   errors, `export`, legacy `to_string`/`to_file`).
//! * `utils/exporters/exporter.py:12-76` — `Exporter` (ctor, `export`,
//!   `register_formatter`, `get_available_formats`).
//! * `utils/exporters/formatters.py:16-206` — CSV (`QUOTE_ALL`, schema
//!   labels), JSON (label keys, no indent), XLSX formatters.
//! * `utils/exporters/schemas/base.py` — field types, label resolution,
//!   `serialize`/`serialize_queryset` field filtering.
//! * `utils/exporters/schemas/issue.py:23-213` — attachment/cycle context
//!   queries, `IssueExportSchema` field order + every `prepare_*`.
//! * `utils/csv_utils.py` — formula-injection sanitising (reused from
//!   [`crate::tasks_cleanup::exports`], never re-implemented).
//!
//! Everything here is pure over injected snapshots: serialised rows enter
//! as ordered [`PVal`] pairs (JSON-object insertion order — the order the
//! DRF serializers emitted), never as a live queryset. The task wire
//! (`bgtasks/export_task.py:128-226`) lives in `pidash-jobs`
//! (`tasks_export`); the exporter queryset SQL lives in [`super::queries`]
//! (FX-A-Q-06).
//!
//! Fixture: `rust-api/fixtures/app_analytics/formats/porter_formats.golden.json`
//! (FX-A-FMT-01, traced in `rust-api/fixtures/app_analytics/TRACE.md`).
//! Unit tests below replay its goldens.
//!
//! Byte-parity notes (translate, don't redesign):
//!
//! * JSON uses Python `json.dumps` bytes: `ensure_ascii` escaping,
//!   `(', ', ': ')` separators, `indent=2` layout, `NaN`/`Infinity` tokens.
//! * CSV `QUOTE_MINIMAL` (porter) and `QUOTE_ALL` (exporters) match the
//!   `csv` module byte for byte, including `\r\n` terminators.
//! * Headers use `str.title()` semantics; normalisation is
//!   `strip/lower/space→underscore`, exactly as Python.
//! * Floats render with Python `repr` shortest-round-trip digits
//!   (`serde_json` carries the same algorithm; `1e16` style exponents keep
//!   `e16` where Python writes `e+16` — no export golden hits that range).
//! * XLSX parity is structural + round-trip: headers, row order, cell
//!   values and types match; the container bytes cannot equal openpyxl's
//!   (timestamps, creator strings). [`xlsx_decode`] reads both engines'
//!   files, including shared-string tables.
//! * `decode` JSON parsing is strict (Python `json.loads` also accepts
//!   `NaN`/`Infinity` literals — no golden hits that path).

use std::collections::HashMap;
use std::io::Read;

use chrono::{DateTime, Datelike, NaiveDate, Timelike, Utc};
use serde_json::Value;

use crate::tasks_cleanup::exports as cleanup;

// ---------------------------------------------------------------------------
// Ordered value model
// ---------------------------------------------------------------------------

/// One export cell/field value. `Dict` preserves serializer emission
/// order (Python dict insertion order); `serde_json::Map` does not, so
/// ordered pairs are the primary input and [`PVal::from_json`] is only a
/// convenience bridge.
#[derive(Debug, Clone, PartialEq)]
pub enum PVal {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(String),
    List(Vec<PVal>),
    Dict(Vec<(String, PVal)>),
}

impl PVal {
    /// Bridge from `serde_json`. Object key order follows the map's own
    /// order — prefer ordered pairs where wire order matters.
    pub fn from_json(value: &Value) -> Self {
        match value {
            Value::Null => PVal::Null,
            Value::Bool(b) => PVal::Bool(*b),
            Value::Number(n) => {
                if let Some(i) = n.as_i64() {
                    PVal::Int(i)
                } else if let Some(f) = n.as_f64() {
                    PVal::Float(f)
                } else {
                    PVal::Str(n.to_string())
                }
            }
            Value::String(s) => PVal::Str(s.clone()),
            Value::Array(items) => PVal::List(items.iter().map(PVal::from_json).collect()),
            Value::Object(map) => PVal::Dict(
                map.iter()
                    .map(|(k, v)| (k.clone(), PVal::from_json(v)))
                    .collect(),
            ),
        }
    }
}

// ---------------------------------------------------------------------------
// Python-compatible JSON rendering
// ---------------------------------------------------------------------------

/// Escape one string exactly like `json.dumps` with `ensure_ascii=True`:
/// `"`, `\`, control characters (`\b \f \n \r \t`, else `\u00XX`) and
/// every non-ASCII code point (`\uXXXX`, surrogate pairs above U+FFFF).
pub fn py_escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    for ch in text.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\x08' => out.push_str("\\b"),
            '\x0c' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                out.push_str(&format!("\\u{:04x}", c as u32));
            }
            c if (c as u32) < 0x7f => out.push(c),
            c => {
                let n = c as u32;
                if n > 0xffff {
                    let v = n - 0x1_0000;
                    out.push_str(&format!(
                        "\\u{:04x}\\u{:04x}",
                        0xd800 + (v >> 10),
                        0xdc00 + (v & 0x3ff)
                    ));
                } else {
                    out.push_str(&format!("\\u{n:04x}"));
                }
            }
        }
    }
    out
}

/// Python `repr(float)`: shortest round-trip digits; non-finite values
/// use the bare `NaN`/`Infinity`/`-Infinity` tokens (`allow_nan` default).
pub fn py_float_repr(value: f64) -> String {
    if value.is_nan() {
        return "NaN".to_owned();
    }
    if value.is_infinite() {
        return if value > 0.0 {
            "Infinity".to_owned()
        } else {
            "-Infinity".to_owned()
        };
    }
    match serde_json::Number::from_f64(value) {
        Some(n) => n.to_string(),
        None => "null".to_owned(),
    }
}

/// Python `str(value)` for export cells: `None` → `"None"`, bools →
/// `"True"`/`"False"`, floats → [`py_float_repr`], strings verbatim,
/// lists/dicts → Python `repr` (single-quoted, `", "`/`": "` separators).
pub fn py_str(value: &PVal) -> String {
    match value {
        PVal::Null => "None".to_owned(),
        PVal::Bool(true) => "True".to_owned(),
        PVal::Bool(false) => "False".to_owned(),
        PVal::Int(i) => i.to_string(),
        PVal::Float(f) => py_float_repr(*f),
        PVal::Str(s) => s.clone(),
        PVal::List(items) => {
            let inner: Vec<String> = items.iter().map(py_repr).collect();
            format!("[{}]", inner.join(", "))
        }
        PVal::Dict(pairs) => {
            let inner: Vec<String> = pairs
                .iter()
                .map(|(k, v)| format!("{}: {}", py_repr_str(k), py_repr(v)))
                .collect();
            format!("{{{}}}", inner.join(", "))
        }
    }
}

/// Python `repr()` of a string: single quotes unless the value holds a
/// single quote but no double quote; backslash, quote-char and control
/// escapes; printable non-ASCII verbatim.
pub fn py_repr_str(text: &str) -> String {
    let quote = if text.contains('\'') && !text.contains('"') {
        '"'
    } else {
        '\''
    };
    let mut out = String::with_capacity(text.len() + 2);
    out.push(quote);
    for ch in text.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c == quote => {
                out.push('\\');
                out.push(c);
            }
            c if (c as u32) < 0x20 => {
                out.push_str(&format!("\\x{:02x}", c as u32));
            }
            c => out.push(c),
        }
    }
    out.push(quote);
    out
}

/// Python `repr()` for any export value (strings via [`py_repr_str`],
/// everything else via [`py_str`).
pub fn py_repr(value: &PVal) -> String {
    match value {
        PVal::Str(s) => py_repr_str(s),
        other => py_str(other),
    }
}

fn dumps_compact_into(value: &PVal, out: &mut String) {
    match value {
        PVal::Null => out.push_str("null"),
        PVal::Bool(true) => out.push_str("true"),
        PVal::Bool(false) => out.push_str("false"),
        PVal::Int(i) => out.push_str(&i.to_string()),
        PVal::Float(f) => out.push_str(&py_float_repr(*f)),
        PVal::Str(s) => {
            out.push('"');
            out.push_str(&py_escape(s));
            out.push('"');
        }
        PVal::List(items) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                dumps_compact_into(item, out);
            }
            out.push(']');
        }
        PVal::Dict(pairs) => {
            out.push('{');
            for (i, (k, v)) in pairs.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                out.push('"');
                out.push_str(&py_escape(k));
                out.push_str("\": ");
                dumps_compact_into(v, out);
            }
            out.push('}');
        }
    }
}

/// `json.dumps(value)` with default separators (`', '`, `': '`) and
/// `ensure_ascii` — the porter flatten path and the exporters JSON path.
pub fn py_dumps(value: &PVal) -> String {
    let mut out = String::new();
    dumps_compact_into(value, &mut out);
    out
}

fn dumps_indent_into(value: &PVal, level: usize, out: &mut String) {
    let pad = "  ".repeat(level);
    let child_pad = "  ".repeat(level + 1);
    match value {
        PVal::List(items) => {
            if items.is_empty() {
                out.push_str("[]");
                return;
            }
            out.push_str("[\n");
            for (i, item) in items.iter().enumerate() {
                out.push_str(&child_pad);
                dumps_indent_into(item, level + 1, out);
                if i + 1 < items.len() {
                    out.push(',');
                }
                out.push('\n');
            }
            out.push_str(&pad);
            out.push(']');
        }
        PVal::Dict(pairs) => {
            if pairs.is_empty() {
                out.push_str("{}");
                return;
            }
            out.push_str("{\n");
            for (i, (k, v)) in pairs.iter().enumerate() {
                out.push_str(&child_pad);
                out.push('"');
                out.push_str(&py_escape(k));
                out.push_str("\": ");
                dumps_indent_into(v, level + 1, out);
                if i + 1 < pairs.len() {
                    out.push(',');
                }
                out.push('\n');
            }
            out.push_str(&pad);
            out.push('}');
        }
        other => dumps_compact_into(other, out),
    }
}

/// `json.dumps(data, indent=2, default=str)` — `JSONFormatter.encode`
/// (`porters/formatters.py:46-47`). Non-string scalars never reach this
/// path unserializable (rows are JSON values), so `default=str` is only
/// honoured for floats via [`py_float_repr`].
pub fn py_dumps_indent(value: &PVal) -> String {
    let mut out = String::new();
    dumps_indent_into(value, 0, &mut out);
    out
}

// ---------------------------------------------------------------------------
// Header prettify / normalize (porter formatters.py:69-75, :180-186)
// ---------------------------------------------------------------------------

/// `str.title()` semantics (`formatters.py:71,182`): a cased letter is
/// uppercased after an uncased character and lowercased after a cased one.
pub fn py_title(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut prev_cased = false;
    for ch in text.chars() {
        if ch.is_lowercase() {
            if !prev_cased {
                out.extend(ch.to_uppercase());
            } else {
                out.push(ch);
            }
            prev_cased = true;
        } else if ch.is_uppercase() {
            if prev_cased {
                out.extend(ch.to_lowercase());
            } else {
                out.push(ch);
            }
            prev_cased = true;
        } else {
            out.push(ch);
            prev_cased = false;
        }
    }
    out
}

/// `_prettify_header`: `'created_by_name'` → `'Created By Name'`.
pub fn prettify_header(header: &str) -> String {
    py_title(&header.replace('_', " "))
}

/// `_normalize_header`: `'Display Name'` → `'display_name'` — strip,
/// lowercase, spaces to underscores, nothing else.
pub fn normalize_header(header: &str) -> String {
    header.trim().to_lowercase().replace(' ', "_")
}

// ---------------------------------------------------------------------------
// Porter flatten / unflatten (formatters.py:77-106)
// ---------------------------------------------------------------------------

/// `_flatten`: nested dicts → `parent__child` keys; lists → `json.dumps`;
/// scalars verbatim.
pub fn flatten_row(row: &[(String, PVal)]) -> Vec<(String, PVal)> {
    let mut out = Vec::with_capacity(row.len());
    flatten_into(row, None, &mut out);
    out
}

fn flatten_into(row: &[(String, PVal)], parent: Option<&str>, out: &mut Vec<(String, PVal)>) {
    for (key, value) in row {
        let name = match parent {
            Some(p) => format!("{p}__{key}"),
            None => key.clone(),
        };
        match value {
            PVal::Dict(pairs) => flatten_into(pairs, Some(&name), out),
            PVal::List(_) => out.push((name, PVal::Str(py_dumps(value)))),
            scalar => out.push((name, scalar.clone())),
        }
    }
}

/// Try `json.loads` on a string cell, keeping the value only when it
/// parses to a list or dict (`formatters.py:97-103`, `:260-266`).
fn json_list_or_dict(text: &str) -> Option<PVal> {
    match serde_json::from_str::<Value>(text) {
        Ok(Value::Array(items)) => Some(PVal::List(items.iter().map(PVal::from_json).collect())),
        Ok(Value::Object(map)) => Some(PVal::Dict(
            map.iter()
                .map(|(k, v)| (k.clone(), PVal::from_json(v)))
                .collect(),
        )),
        _ => None,
    }
}

/// `_unflatten`: `__`-split keys re-nested; string cells JSON-parsed back
/// to lists/dicts where possible.
pub fn unflatten_row(row: &[(String, PVal)]) -> Vec<(String, PVal)> {
    let mut root: Vec<(String, PVal)> = Vec::new();
    for (key, value) in row {
        let parts: Vec<&str> = key.split("__").collect();
        let leaf = match value {
            PVal::Str(s) => json_list_or_dict(s).unwrap_or_else(|| value.clone()),
            _ => value.clone(),
        };
        insert_nested(&mut root, &parts, leaf);
    }
    root
}

fn insert_nested(node: &mut Vec<(String, PVal)>, parts: &[&str], leaf: PVal) {
    let Some((head, rest)) = parts.split_first() else {
        return;
    };
    if rest.is_empty() {
        match node.iter_mut().find(|(k, _)| k == head) {
            Some(slot) => slot.1 = leaf,
            None => node.push((head.to_string(), leaf)),
        }
        return;
    }
    let pos = match node.iter().position(|(k, _)| k == head) {
        Some(i) => i,
        None => {
            node.push((head.to_string(), PVal::Dict(Vec::new())));
            node.len() - 1
        }
    };
    if !matches!(node[pos].1, PVal::Dict(_)) {
        node[pos].1 = PVal::Dict(Vec::new());
    }
    if let PVal::Dict(children) = &mut node[pos].1 {
        insert_nested(children, rest, leaf);
    }
}

// ---------------------------------------------------------------------------
// Porter CSV (formatters.py:108-165)
// ---------------------------------------------------------------------------

/// Quote one field like `csv.writer` with `QUOTE_MINIMAL`: quotes,
/// delimiters and line breaks trigger quoting; embedded quotes double.
pub fn quote_minimal(field: &str, delimiter: char) -> String {
    if field.contains('"')
        || field.contains(delimiter)
        || field.contains('\r')
        || field.contains('\n')
    {
        format!("\"{}\"", field.replace('"', "\"\""))
    } else {
        field.to_owned()
    }
}

/// Render rows with `\r\n` terminators (the `csv` module lineterminator).
pub fn write_csv_minimal(rows: &[Vec<String>], delimiter: char) -> String {
    let mut out = String::new();
    for row in rows {
        let quoted: Vec<String> = row.iter().map(|c| quote_minimal(c, delimiter)).collect();
        out.push_str(&quoted.join(&delimiter.to_string()));
        out.push_str("\r\n");
    }
    out
}

/// Render one porter cell the way `csv.writer` sees it: `None`/missing →
/// `""`, strings verbatim, numbers/bools via Python `str()`.
pub fn porter_cell_text(value: &PVal) -> String {
    match value {
        PVal::Null => String::new(),
        PVal::Str(s) => s.clone(),
        other => py_str(other),
    }
}

/// `CSVFormatter.encode` (`formatters.py:108-142`): empty data → `""`;
/// optional flatten; first-seen field order; prettified headers via
/// `csv.writer` or raw keys via `DictWriter`; every cell sanitised
/// (`sanitize_csv_row` — strings only, see [`cleanup`]).
pub fn porter_csv_encode(
    rows: &[Vec<(String, PVal)>],
    flatten: bool,
    delimiter: char,
    prettify: bool,
) -> String {
    if rows.is_empty() {
        return String::new();
    }
    let flat: Vec<Vec<(String, PVal)>> = if flatten {
        rows.iter().map(|r| flatten_row(r)).collect()
    } else {
        rows.to_vec()
    };
    let mut fieldnames: Vec<String> = Vec::new();
    for row in &flat {
        for (key, _) in row {
            if !fieldnames.iter().any(|f| f == key) {
                fieldnames.push(key.clone());
            }
        }
    }
    let header: Vec<String> = if prettify {
        fieldnames.iter().map(|k| prettify_header(k)).collect()
    } else {
        fieldnames.clone()
    };
    let mut table: Vec<Vec<String>> = Vec::with_capacity(flat.len() + 1);
    table.push(header);
    for row in &flat {
        let map: HashMap<&str, &PVal> = row.iter().map(|(k, v)| (k.as_str(), v)).collect();
        let mut out_row = Vec::with_capacity(fieldnames.len());
        for key in &fieldnames {
            match map.get(key.as_str()) {
                // `sanitize_csv_value` is `isinstance(value, str)`-gated:
                // only string cells pass through it.
                Some(PVal::Str(s)) => out_row.push(cleanup::sanitize_csv_value(s)),
                Some(other) => out_row.push(porter_cell_text(other)),
                None => out_row.push(String::new()),
            }
        }
        table.push(out_row);
    }
    write_csv_minimal(&table, delimiter)
}

/// Minimal `csv.reader`: quotes, doubled quotes, `\r\n`/`\n`/`\r` line
/// breaks (including inside quoted fields), custom delimiter.
pub fn parse_csv(content: &str, delimiter: char) -> Vec<Vec<String>> {
    let mut rows: Vec<Vec<String>> = Vec::new();
    let mut row: Vec<String> = Vec::new();
    let mut field = String::new();
    let mut in_quotes = false;
    let mut chars = content.chars().peekable();
    let mut any = false;
    while let Some(ch) = chars.next() {
        any = true;
        if in_quotes {
            if ch == '"' {
                if chars.peek() == Some(&'"') {
                    chars.next();
                    field.push('"');
                } else {
                    in_quotes = false;
                }
            } else {
                field.push(ch);
            }
        } else if ch == '"' && field.is_empty() {
            in_quotes = true;
        } else if ch == delimiter {
            row.push(std::mem::take(&mut field));
        } else if ch == '\r' {
            if chars.peek() == Some(&'\n') {
                chars.next();
            }
            row.push(std::mem::take(&mut field));
            rows.push(std::mem::take(&mut row));
        } else if ch == '\n' {
            row.push(std::mem::take(&mut field));
            rows.push(std::mem::take(&mut row));
        } else {
            field.push(ch);
        }
    }
    if any {
        row.push(field);
        rows.push(row);
    }
    rows
}

/// `CSVFormatter.decode` (`formatters.py:144-161`): `DictReader` over the
/// content, header normalisation, then unflattening. Every cell arrives
/// as a string.
pub fn porter_csv_decode(
    content: &str,
    delimiter: char,
    normalize_headers: bool,
    unflatten: bool,
) -> Vec<Vec<(String, PVal)>> {
    let mut parsed = parse_csv(content, delimiter);
    // A trailing line break leaves one empty row; `csv.reader` yields it
    // as `[]` and `DictReader` skips it.
    parsed.retain(|r| !(r.len() == 1 && r[0].is_empty()));
    if parsed.is_empty() {
        return Vec::new();
    }
    let headers: Vec<String> = parsed[0]
        .iter()
        .map(|h| {
            if normalize_headers {
                normalize_header(h)
            } else {
                h.clone()
            }
        })
        .collect();
    let mut out = Vec::with_capacity(parsed.len() - 1);
    for record in parsed.iter().skip(1) {
        let mut row = Vec::with_capacity(headers.len());
        for (i, header) in headers.iter().enumerate() {
            let cell = record.get(i).cloned().unwrap_or_default();
            row.push((header.clone(), PVal::Str(cell)));
        }
        out.push(if unflatten { unflatten_row(&row) } else { row });
    }
    out
}

// ---------------------------------------------------------------------------
// Porter JSON (formatters.py:42-54)
// ---------------------------------------------------------------------------

/// `JSONFormatter.encode` with the default `indent=2`.
pub fn porter_json_encode(rows: &[Vec<(String, PVal)>]) -> String {
    porter_json_encode_indent(rows, 2)
}

/// `JSONFormatter.encode` with an explicit indent.
pub fn porter_json_encode_indent(rows: &[Vec<(String, PVal)>], indent: usize) -> String {
    let list = PVal::List(rows.iter().map(|r| PVal::Dict(r.clone())).collect());
    if indent == 0 {
        return py_dumps(&list);
    }
    let mut out = String::new();
    dumps_indent_into(&list, 0, &mut out);
    debug_assert!(indent == 2, "porter JSON indent is always 2 in Python");
    out
}

/// `JSONFormatter.decode` (`formatters.py:49-50`): `json.loads`.
pub fn porter_json_decode(content: &str) -> Result<Vec<Vec<(String, PVal)>>, String> {
    let value: Value = serde_json::from_str(content).map_err(|e| format!("invalid JSON: {e}"))?;
    match value {
        Value::Array(items) => {
            let mut out = Vec::with_capacity(items.len());
            for item in items {
                match item {
                    Value::Object(map) => out.push(
                        map.iter()
                            .map(|(k, v)| (k.clone(), PVal::from_json(v)))
                            .collect(),
                    ),
                    _ => return Err("JSON export must be a list of objects".to_owned()),
                }
            }
            Ok(out)
        }
        _ => Err("JSON export must be a list of objects".to_owned()),
    }
}

// ---------------------------------------------------------------------------
// Porter XLSX (formatters.py:168-274)
// ---------------------------------------------------------------------------

/// One worksheet cell. `Empty` covers both `None` (`_format_value` →
/// `""`) and empty strings — openpyxl reads both back as blank.
#[derive(Debug, Clone, PartialEq)]
pub enum XlsxCell {
    Empty,
    Text(String),
    Int(i64),
    Float(f64),
    Bool(bool),
}

/// `_format_value` (`formatters.py:188-196`): `None` → `""`; lists joined
/// with `list_joiner` over Python `str()` items; dicts → `json.dumps`;
/// everything else verbatim.
pub fn porter_format_value(value: &PVal, list_joiner: &str) -> XlsxCell {
    match value {
        PVal::Null => XlsxCell::Empty,
        PVal::Str(s) => {
            if s.is_empty() {
                XlsxCell::Empty
            } else {
                XlsxCell::Text(s.clone())
            }
        }
        PVal::Int(i) => XlsxCell::Int(*i),
        PVal::Float(f) => XlsxCell::Float(*f),
        PVal::Bool(b) => XlsxCell::Bool(*b),
        PVal::List(items) => {
            let joined = items
                .iter()
                .map(py_str)
                .collect::<Vec<_>>()
                .join(list_joiner);
            if joined.is_empty() {
                XlsxCell::Empty
            } else {
                XlsxCell::Text(joined)
            }
        }
        PVal::Dict(_) => XlsxCell::Text(py_dumps(value)),
    }
}

/// Column index → Excel letters: 0 → `A`, 25 → `Z`, 26 → `AA`.
pub fn col_letters(mut index: usize) -> String {
    let mut out = String::new();
    loop {
        out.insert(0, (b'A' + (index % 26) as u8) as char);
        if index < 26 {
            break;
        }
        index = index / 26 - 1;
    }
    out
}

/// XML-escape cell text (`&`, `<`, `>`).
pub fn xml_escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            c => out.push(c),
        }
    }
    out
}

/// Reverse [`xml_escape`] plus `&quot;`, `&apos;` and numeric character
/// references (for files written by other engines).
pub fn xml_unescape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(amp) = rest.find('&') {
        out.push_str(&rest[..amp]);
        let tail = &rest[amp..];
        let semi = tail.find(';');
        let entity = semi.map(|i| &tail[..i + 1]);
        match entity {
            Some("&amp;") => {
                out.push('&');
                rest = &tail[5..];
            }
            Some("&lt;") => {
                out.push('<');
                rest = &tail[4..];
            }
            Some("&gt;") => {
                out.push('>');
                rest = &tail[4..];
            }
            Some("&quot;") => {
                out.push('"');
                rest = &tail[6..];
            }
            Some("&apos;") => {
                out.push('\'');
                rest = &tail[6..];
            }
            Some(e) if e.starts_with("&#") => {
                let num = &e[2..e.len() - 1];
                let parsed =
                    if let Some(hex) = num.strip_prefix('x').or_else(|| num.strip_prefix('X')) {
                        u32::from_str_radix(hex, 16).ok().and_then(char::from_u32)
                    } else {
                        num.parse::<u32>().ok().and_then(char::from_u32)
                    };
                match parsed {
                    Some(ch) => {
                        out.push(ch);
                        rest = &tail[e.len()..];
                    }
                    None => {
                        out.push_str(e);
                        rest = &tail[e.len()..];
                    }
                }
            }
            _ => {
                out.push('&');
                rest = &tail[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

fn xlsx_cell_xml(col: &str, row_no: usize, cell: &XlsxCell) -> Option<String> {
    let cell_ref = format!("{col}{row_no}");
    match cell {
        XlsxCell::Empty => None,
        XlsxCell::Text(s) => Some(format!(
            "<c r=\"{cell_ref}\" t=\"inlineStr\"><is><t>{}</t></is></c>",
            xml_escape(s)
        )),
        XlsxCell::Int(i) => Some(format!("<c r=\"{cell_ref}\"><v>{i}</v></c>")),
        XlsxCell::Float(f) => Some(format!(
            "<c r=\"{cell_ref}\"><v>{}</v></c>",
            py_float_repr(*f)
        )),
        XlsxCell::Bool(b) => Some(format!(
            "<c r=\"{cell_ref}\" t=\"b\"><v>{}</v></c>",
            if *b { 1 } else { 0 }
        )),
    }
}

const XLSX_CONTENT_TYPES: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/><Override PartName="/xl/worksheets/sheet1.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/><Override PartName="/xl/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.styles+xml"/></Types>"#;
const XLSX_ROOT_RELS: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/></Relationships>"#;
const XLSX_WORKBOOK: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets><sheet name="Sheet" sheetId="1" r:id="rId1"/></sheets></workbook>"#;
const XLSX_WORKBOOK_RELS: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/><Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="styles.xml"/></Relationships>"#;
const XLSX_STYLES: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<styleSheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><fonts count="1"><font><sz val="11"/><name val="Calibri"/></font></fonts><fills count="1"><fill><patternFill patternType="none"/></fill></fills><borders count="1"><border><left/><right/><top/><bottom/><diagonal/></border></borders><cellStyleXfs count="1"><xf numFmtId="0" fontId="0" fillId="0" borderId="0"/></cellStyleXfs><cellXfs count="1"><xf numFmtId="0" fontId="0" fillId="0" borderId="0" xfId="0"/></cellXfs></styleSheet>"#;

/// Build a minimal `.xlsx` workbook: `headers=None` writes an empty
/// `<sheetData/>` (the `not data` branch — "workbook with headers-less
/// active sheet, no rows appended"); otherwise one header row plus data.
pub fn xlsx_workbook(headers: Option<&[String]>, rows: &[Vec<XlsxCell>]) -> Vec<u8> {
    let mut sheet = String::from(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData>"#,
    );
    if let Some(heads) = headers {
        // Header row.
        sheet.push_str("<row r=\"1\">");
        for (i, head) in heads.iter().enumerate() {
            if let Some(xml) = xlsx_cell_xml(&col_letters(i), 1, &XlsxCell::Text(head.clone())) {
                sheet.push_str(&xml);
            }
        }
        sheet.push_str("</row>");
        // Data rows.
        for (n, data_row) in rows.iter().enumerate() {
            let r = n + 2;
            sheet.push_str(&format!("<row r=\"{r}\">"));
            for (i, cell) in data_row.iter().enumerate() {
                if let Some(xml) = xlsx_cell_xml(&col_letters(i), r, cell) {
                    sheet.push_str(&xml);
                }
            }
            sheet.push_str("</row>");
        }
    }
    sheet.push_str("</sheetData></worksheet>");
    cleanup::build_zip(&[
        cleanup::ZipEntry {
            name: "[Content_Types].xml",
            content: XLSX_CONTENT_TYPES.as_bytes(),
        },
        cleanup::ZipEntry {
            name: "_rels/.rels",
            content: XLSX_ROOT_RELS.as_bytes(),
        },
        cleanup::ZipEntry {
            name: "xl/workbook.xml",
            content: XLSX_WORKBOOK.as_bytes(),
        },
        cleanup::ZipEntry {
            name: "xl/_rels/workbook.xml.rels",
            content: XLSX_WORKBOOK_RELS.as_bytes(),
        },
        cleanup::ZipEntry {
            name: "xl/styles.xml",
            content: XLSX_STYLES.as_bytes(),
        },
        cleanup::ZipEntry {
            name: "xl/worksheets/sheet1.xml",
            content: sheet.as_bytes(),
        },
    ])
}

/// `XLSXFormatter.encode` (`formatters.py:198-231`): empty data → empty
/// workbook; otherwise prettified (or raw) headers plus `_format_value`
/// rows.
pub fn porter_xlsx_encode(
    rows: &[Vec<(String, PVal)>],
    prettify: bool,
    list_joiner: &str,
) -> Vec<u8> {
    if rows.is_empty() {
        return xlsx_workbook(None, &[]);
    }
    let mut fieldnames: Vec<String> = Vec::new();
    for row in rows {
        for (key, _) in row {
            if !fieldnames.iter().any(|f| f == key) {
                fieldnames.push(key.clone());
            }
        }
    }
    let headers: Vec<String> = if prettify {
        fieldnames.iter().map(|k| prettify_header(k)).collect()
    } else {
        fieldnames.clone()
    };
    let mut table: Vec<Vec<XlsxCell>> = Vec::with_capacity(rows.len());
    for row in rows {
        let map: HashMap<&str, &PVal> = row.iter().map(|(k, v)| (k.as_str(), v)).collect();
        let mut out_row = Vec::with_capacity(fieldnames.len());
        for key in &fieldnames {
            match map.get(key.as_str()) {
                Some(v) => out_row.push(porter_format_value(v, list_joiner)),
                None => out_row.push(XlsxCell::Empty),
            }
        }
        table.push(out_row);
    }
    xlsx_workbook(Some(&headers), &table)
}

/// Read the local-header entries of a ZIP archive (stored or deflated),
/// returning `(name, inflated bytes)` in archive order.
pub fn read_zip_entries(data: &[u8]) -> Result<Vec<(String, Vec<u8>)>, String> {
    let mut entries = Vec::new();
    let mut pos = 0;
    while pos + 30 <= data.len() {
        let sig = u32::from_le_bytes(data[pos..pos + 4].try_into().map_err(|_| "zip truncated")?);
        if sig != 0x0403_4b50 {
            break;
        }
        let method = u16::from_le_bytes(
            data[pos + 8..pos + 10]
                .try_into()
                .map_err(|_| "zip truncated")?,
        );
        let comp_len = u32::from_le_bytes(
            data[pos + 18..pos + 22]
                .try_into()
                .map_err(|_| "zip truncated")?,
        ) as usize;
        let name_len = u16::from_le_bytes(
            data[pos + 26..pos + 28]
                .try_into()
                .map_err(|_| "zip truncated")?,
        ) as usize;
        let extra_len = u16::from_le_bytes(
            data[pos + 28..pos + 30]
                .try_into()
                .map_err(|_| "zip truncated")?,
        ) as usize;
        let name_start = pos + 30;
        let name_end = name_start.checked_add(name_len).ok_or("zip truncated")?;
        let body_end = name_end
            .checked_add(extra_len)
            .and_then(|s| s.checked_add(comp_len))
            .ok_or("zip truncated")?;
        if body_end > data.len() {
            return Err("zip truncated".to_owned());
        }
        let name = String::from_utf8(data[name_start..name_end].to_vec())
            .map_err(|_| "zip entry name is not UTF-8".to_owned())?;
        let body = &data[name_end + extra_len..body_end];
        let content = match method {
            0 => body.to_vec(),
            8 => {
                let mut decoder = flate2::read::DeflateDecoder::new(body);
                let mut content = Vec::new();
                decoder
                    .read_to_end(&mut content)
                    .map_err(|e| format!("xlsx inflate failed: {e}"))?;
                content
            }
            other => return Err(format!("unsupported zip method {other}")),
        };
        entries.push((name, content));
        pos = body_end;
    }
    Ok(entries)
}

/// Column letters → zero-based index: `A` → 0, `AA` → 26.
fn col_index(letters: &str) -> Option<usize> {
    if letters.is_empty() {
        return None;
    }
    let mut index = 0usize;
    for ch in letters.chars() {
        if !ch.is_ascii_uppercase() {
            return None;
        }
        index = index * 26 + (ch as usize - 'A' as usize + 1);
    }
    Some(index - 1)
}

/// Split a cell reference into `(column letters, row number)`.
fn split_cell_ref(cell_ref: &str) -> Option<(&str, usize)> {
    let digits = cell_ref.find(|c: char| c.is_ascii_digit())?;
    let (letters, number) = cell_ref.split_at(digits);
    Some((letters, number.parse::<usize>().ok()?))
}

/// Collect the text of every `<t>` element inside one shared-string item
/// (plain and rich-text runs alike).
fn shared_item_text(item: &str) -> String {
    let mut out = String::new();
    let mut rest = item;
    while let Some(start) = rest.find("<t") {
        let tag = &rest[start..];
        let body = match tag.find('>') {
            Some(i) => &tag[i + 1..],
            None => break,
        };
        match body.find("</t>") {
            Some(end) => {
                out.push_str(&xml_unescape(&body[..end]));
                rest = &body[end + 4..];
            }
            None => break,
        }
    }
    out
}

/// Parse `xl/sharedStrings.xml` into its item list (absent → empty).
fn parse_shared_strings(xml: &str) -> Vec<String> {
    let mut items = Vec::new();
    let mut rest = xml;
    while let Some(start) = rest.find("<si>") {
        let body = &rest[start + 4..];
        match body.find("</si>") {
            Some(end) => {
                items.push(shared_item_text(&body[..end]));
                rest = &body[end + 5..];
            }
            None => break,
        }
    }
    items
}

/// Parse a numeric cell literal: plain integers stay ints (`8.0` and
/// exponents fall through to float); anything else stays text.
fn parse_number_cell(text: &str) -> PVal {
    if let Ok(i) = text.parse::<i64>() {
        if !text.contains('.') && !text.contains('e') && !text.contains('E') {
            return PVal::Int(i);
        }
    }
    match text.parse::<f64>() {
        Ok(f) => PVal::Float(f),
        Err(_) => PVal::Str(text.to_owned()),
    }
}

/// Find the next `<c`, `<c `, `<c/` or `<c>` cell open tag.
fn next_cell_tag(xml: &str) -> Option<usize> {
    let mut from = 0;
    while let Some(i) = xml[from..].find("<c") {
        let pos = from + i;
        match xml[pos + 2..].chars().next() {
            Some(' ') | Some('/') | Some('>') => return Some(pos),
            _ => from = pos + 2,
        }
    }
    None
}

/// Decoded cell kind: text (inline/shared/string), numeric literal, or
/// boolean (`t="b"`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CellKind {
    Text,
    Num,
    Bool,
}

/// Decode one `<c>` element to text plus its kind: inline/shared strings
/// stay text; `b` cells keep their `0`/`1` literal for boolean parsing;
/// numeric cells keep their literal for number parsing.
fn decode_cell(attrs: &str, inner: &str, shared: &[String]) -> Option<(String, CellKind)> {
    let cell_type = if attrs.contains("t=\"inlineStr\"") {
        "inline"
    } else if attrs.contains("t=\"s\"") {
        "shared"
    } else if attrs.contains("t=\"b\"") {
        "bool"
    } else if attrs.contains("t=\"str\"") || attrs.contains("t=\"e\"") {
        "other"
    } else {
        "num"
    };
    match cell_type {
        "inline" => {
            let start = inner.find("<t")?;
            let body = &inner[start..];
            let text_start = body.find('>')? + 1;
            let end = body.find("</t>")?;
            Some((xml_unescape(&body[text_start..end]), CellKind::Text))
        }
        "shared" => {
            let v_start = inner.find("<v>")? + 3;
            let v_end = inner[v_start..].find("</v>")?;
            let idx: usize = inner[v_start..v_start + v_end].trim().parse().ok()?;
            Some((shared.get(idx).cloned().unwrap_or_default(), CellKind::Text))
        }
        "bool" => {
            let v_start = inner.find("<v>")? + 3;
            let v_end = inner[v_start..].find("</v>")?;
            Some((
                inner[v_start..v_start + v_end].trim().to_owned(),
                CellKind::Bool,
            ))
        }
        _ => {
            let v_start = inner.find("<v>")? + 3;
            let v_end = inner[v_start..].find("</v>")?;
            let kind = if cell_type == "num" {
                CellKind::Num
            } else {
                CellKind::Text
            };
            Some((inner[v_start..v_start + v_end].trim().to_owned(), kind))
        }
    }
}

/// `XLSXFormatter.decode` (`formatters.py:233-270`): read-only load of the
/// active sheet, header normalisation, per-string JSON recovery. Reads
/// files from both this module and openpyxl (shared-string tables
/// included). Empty sheets → `[]`.
pub fn xlsx_decode(
    content: &[u8],
    normalize_headers: bool,
) -> Result<Vec<Vec<(String, PVal)>>, String> {
    let entries = read_zip_entries(content)?;
    let table: HashMap<&str, &[u8]> = entries
        .iter()
        .map(|(n, b)| (n.as_str(), b.as_slice()))
        .collect();
    let sheet = table
        .get("xl/worksheets/sheet1.xml")
        .ok_or("xlsx has no xl/worksheets/sheet1.xml")?;
    let sheet_text =
        std::str::from_utf8(sheet).map_err(|_| "xlsx sheet is not UTF-8".to_owned())?;
    let shared = match table.get("xl/sharedStrings.xml") {
        Some(xml) => parse_shared_strings(
            std::str::from_utf8(xml).map_err(|_| "xlsx shared strings are not UTF-8".to_owned())?,
        ),
        None => Vec::new(),
    };
    // Rows in document order; cells keyed by column index.
    let mut grid: Vec<Vec<(usize, PVal)>> = Vec::new();
    let mut rest: &str = sheet_text;
    while let Some(row_start) = rest.find("<row") {
        let row_body = &rest[row_start..];
        let row_end = row_body.find("</row>").ok_or("xlsx row is not closed")?;
        let row_xml = &row_body[..row_end];
        let mut cells: Vec<(usize, PVal)> = Vec::new();
        let mut cell_rest = row_xml;
        while let Some(c_start) = next_cell_tag(cell_rest) {
            let cell_body = &cell_rest[c_start..];
            let tag_end = cell_body.find('>').ok_or("xlsx cell tag is not closed")?;
            let attrs = &cell_body[..tag_end];
            let after = &cell_body[tag_end + 1..];
            // `<c .../>` is an empty element; otherwise the body runs to `</c>`.
            let (inner, next) = if attrs.ends_with('/') {
                ("", after)
            } else {
                match after.find("</c>") {
                    Some(end) => (&after[..end], &after[end + 4..]),
                    None => return Err("xlsx cell is not closed".to_owned()),
                }
            };
            cell_rest = next;
            let col = attrs
                .find("r=\"")
                .and_then(|i| {
                    let tail = &attrs[i + 3..];
                    tail.find('"').map(|j| &tail[..j])
                })
                .and_then(split_cell_ref)
                .and_then(|(letters, _)| col_index(letters));
            if let Some(col) = col {
                if let Some((text, kind)) = decode_cell(attrs, inner, &shared) {
                    let value = if text.is_empty() && kind != CellKind::Bool {
                        PVal::Null
                    } else {
                        match kind {
                            CellKind::Num => parse_number_cell(&text),
                            // openpyxl reads `t="b"` back as Python bools.
                            CellKind::Bool => PVal::Bool(text.trim() != "0"),
                            CellKind::Text => json_list_or_dict(&text).unwrap_or(PVal::Str(text)),
                        }
                    };
                    cells.push((col, value));
                }
            }
        }
        cells.sort_by_key(|(c, _)| *c);
        let dense: Vec<PVal> = {
            let max = cells.iter().map(|(c, _)| *c).max().unwrap_or(0);
            let mut dense = vec![PVal::Null; max + 1];
            for (c, v) in cells {
                dense[c] = v;
            }
            dense
        };
        // Every `<row>` is a record (blank rows decode to all-`None`,
        // exactly as openpyxl yields them); a sheet with no rows at all
        // returns `[]` below (`if not rows: return []`).
        grid.push(dense.into_iter().enumerate().collect());
        rest = &row_body[row_end + 6..];
    }
    if grid.is_empty() {
        return Ok(Vec::new());
    }
    let raw_headers: Vec<String> = grid[0]
        .iter()
        .map(|(_, v)| match v {
            PVal::Str(s) => s.clone(),
            PVal::Null => String::new(),
            other => py_str(other),
        })
        .collect();
    let headers: Vec<String> = raw_headers
        .iter()
        .map(|h| {
            if normalize_headers {
                normalize_header(h)
            } else {
                h.clone()
            }
        })
        .collect();
    let mut out = Vec::with_capacity(grid.len() - 1);
    for row in grid.iter().skip(1) {
        let map: HashMap<usize, &PVal> = row.iter().map(|(c, v)| (*c, v)).collect();
        let mut record = Vec::new();
        for (i, header) in headers.iter().enumerate() {
            if header.is_empty() {
                continue;
            }
            let value = map.get(&i).copied().cloned().unwrap_or(PVal::Null);
            // `decode` keeps every cell verbatim except the string
            // JSON-recovery, which already ran above.
            record.push((header.clone(), value));
        }
        out.push(record);
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// Porter DataExporter (utils/porters/exporter.py:9-107)
// ---------------------------------------------------------------------------

/// Valid porter `format_type` values, in `FORMATTERS` dict order
/// (`exporter.py:24-28`). Shared with the task layer via
/// `pidash-types` (`EXPORT_FORMATS`); re-exported here for callers.
pub use pidash_types::tasks_cleanup::exports_dto::EXPORT_FORMATS as PORTER_FORMATS;

/// `DataExporter`'s xlsx formatter option (`exporter.py:56`).
pub const PORTER_XLSX_LIST_JOINER: &str = ", ";

/// `export()` without a configured format (`exporter.py:84`).
pub const MISSING_FORMAT_TYPE_MSG: &str =
    "format_type must be provided during initialization to use export() method";

/// `to_file` opens text mode (`exporter.py:100`); byte content fails the
/// way Python fails it.
pub const TOFILE_BYTES_MSG: &str = "write() argument must be str, not bytes";

/// Encoded porter payload: text for csv/json, bytes for xlsx.
#[derive(Debug, Clone, PartialEq)]
pub enum PorterContent {
    Text(String),
    Bytes(Vec<u8>),
}

/// Validate a porter `format_type`, raising the exact `ValueError` text
/// (`exporter.py:46`).
pub fn validate_porter_format(format_type: &str) -> Result<(), String> {
    cleanup::validate_provider(format_type)
}

/// Extension for a validated porter format.
pub fn porter_extension(format_type: &str) -> &'static str {
    match format_type {
        "xlsx" => "xlsx",
        "json" => "json",
        _ => "csv",
    }
}

/// `DataExporter.export` (`exporter.py:69-90`): serialised rows (the
/// caller ran the serializer — [`PVal`] pairs are `serializer.data`) →
/// formatter encode → `(filename.ext, content)`.
pub fn porter_export(
    format_type: &str,
    filename: &str,
    rows: &[Vec<(String, PVal)>],
) -> Result<(String, PorterContent), String> {
    validate_porter_format(format_type)?;
    let full = format!("{filename}.{}", porter_extension(format_type));
    let content = match format_type {
        "json" => PorterContent::Text(porter_json_encode(rows)),
        "xlsx" => PorterContent::Bytes(porter_xlsx_encode(rows, true, PORTER_XLSX_LIST_JOINER)),
        _ => PorterContent::Text(porter_csv_encode(rows, true, ',', true)),
    };
    Ok((full, content))
}

/// Available export formats (`exporter.py:104-107`).
pub fn porter_available_formats() -> Vec<&'static str> {
    PORTER_FORMATS.to_vec()
}

/// Legacy `to_string` (`exporter.py:92-95`): encode rows with an explicit
/// formatter's defaults.
pub fn porter_to_string(
    format_type: &str,
    rows: &[Vec<(String, PVal)>],
) -> Result<PorterContent, String> {
    validate_porter_format(format_type)?;
    Ok(match format_type {
        "json" => PorterContent::Text(porter_json_encode(rows)),
        "xlsx" => PorterContent::Bytes(porter_xlsx_encode(rows, true, PORTER_XLSX_LIST_JOINER)),
        _ => PorterContent::Text(porter_csv_encode(rows, true, ',', true)),
    })
}

/// Legacy `to_file` (`exporter.py:97-102`): text mode UTF-8, returning
/// the path. Byte (xlsx) content fails exactly the way Python fails it.
pub fn porter_to_file_text(path: &str, content: &str) -> std::io::Result<String> {
    std::fs::write(path, content)?;
    Ok(path.to_owned())
}

/// Byte-content arm of legacy `to_file`: always the Python `TypeError`.
pub fn porter_to_file_bytes(_path: &str, _content: &[u8]) -> Result<String, String> {
    Err(TOFILE_BYTES_MSG.to_owned())
}

// ---------------------------------------------------------------------------
// Exporters utils: Exporter + formatters + schema base
// (utils/exporters/exporter.py:12-76, formatters.py:16-206, schemas/base.py)
// ---------------------------------------------------------------------------

/// One export format of the `utils/exporters` plane.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ExporterFormat {
    Csv,
    Json,
    Xlsx,
}

impl ExporterFormat {
    /// Extension appended to the export filename.
    pub fn extension(self) -> &'static str {
        match self {
            ExporterFormat::Csv => "csv",
            ExporterFormat::Json => "json",
            ExporterFormat::Xlsx => "xlsx",
        }
    }
}

/// Parse a `format_type` (`Exporter.__init__`, `exporter.py:30-31`) with
/// the exact `ValueError` text.
pub fn parse_exporter_format(format_type: &str) -> Result<ExporterFormat, String> {
    cleanup::validate_provider(format_type)?;
    Ok(match format_type {
        "json" => ExporterFormat::Json,
        "xlsx" => ExporterFormat::Xlsx,
        _ => ExporterFormat::Csv,
    })
}

/// Port of the `Exporter.FORMATTERS` class dict plus `register_formatter`
/// (`exporter.py:16-20,74-76`): the three builtins with their formatter
/// names; `register` extends/overwrites exactly like dict assignment.
/// (No in-repo Python caller registers a custom formatter.)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExporterRegistry {
    entries: Vec<(String, String)>,
}

impl Default for ExporterRegistry {
    fn default() -> Self {
        Self {
            entries: vec![
                ("csv".to_owned(), "CSVFormatter".to_owned()),
                ("json".to_owned(), "JSONFormatter".to_owned()),
                ("xlsx".to_owned(), "XLSXFormatter".to_owned()),
            ],
        }
    }
}

impl ExporterRegistry {
    /// `Exporter.register_formatter`: `FORMATTERS[format_type] =
    /// formatter_class` (stored by class name).
    pub fn register(&mut self, format_type: &str, formatter: &str) {
        match self.entries.iter_mut().find(|(k, _)| k == format_type) {
            Some(slot) => slot.1 = formatter.to_owned(),
            None => self
                .entries
                .push((format_type.to_owned(), formatter.to_owned())),
        }
    }

    /// `Exporter.get_available_formats`.
    pub fn available(&self) -> Vec<String> {
        self.entries.iter().map(|(k, _)| k.clone()).collect()
    }

    /// Formatter class registered for a format, if any.
    pub fn lookup(&self, format_type: &str) -> Option<&str> {
        self.entries
            .iter()
            .find(|(k, _)| k == format_type)
            .map(|(_, v)| v.as_str())
    }
}

/// One declared schema field (`schemas/base.py:12-20`): display label
/// plus the Python field kind it was declared with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldDef {
    pub name: String,
    pub label: Option<String>,
}

impl FieldDef {
    pub fn new(name: &str, label: Option<&str>) -> Self {
        Self {
            name: name.to_owned(),
            label: label.map(str::to_owned),
        }
    }
}

/// `_get_field_info` (`exporters/formatters.py:40-59`): field order from
/// `_declared_fields` plus labels (`field.label`, else
/// `name.replace("_", " ").title()`). A schema without declared fields
/// raises `ValueError`.
pub fn schema_field_info(
    schema_name: &str,
    declared: Option<&[FieldDef]>,
) -> Result<(Vec<String>, HashMap<String, String>), String> {
    let fields = declared.ok_or_else(|| {
        format!("Schema class {schema_name} must have _declared_fields attribute")
    })?;
    let order: Vec<String> = fields.iter().map(|f| f.name.clone()).collect();
    let labels: HashMap<String, String> = fields
        .iter()
        .map(|f| {
            let label = f.label.clone().unwrap_or_else(|| prettify_header(&f.name));
            (f.name.clone(), label)
        })
        .collect();
    Ok((order, labels))
}

/// `_format_field_value` (`exporters/formatters.py:65-75,153-163`):
/// `None` → `""`; lists joined with `list_joiner` over Python `str()`
/// items; dicts → `json.dumps`; everything else → Python `str()`.
pub fn format_field_value(value: &PVal, list_joiner: &str) -> String {
    match value {
        PVal::Null => String::new(),
        PVal::List(items) => items
            .iter()
            .map(py_str)
            .collect::<Vec<_>>()
            .join(list_joiner),
        PVal::Dict(_) => py_dumps(value),
        other => py_str(other),
    }
}

/// Quote one field like `csv.writer(..., quoting=QUOTE_ALL)`.
pub fn quote_all(field: &str) -> String {
    format!("\"{}\"", field.replace('"', "\"\""))
}

/// Render rows `QUOTE_ALL` with `\r\n` terminators
/// (`_create_csv_file`, `formatters.py:85-92`).
pub fn write_csv_quote_all(rows: &[Vec<String>]) -> String {
    let mut out = String::new();
    for row in rows {
        let quoted: Vec<String> = row.iter().map(|c| quote_all(c)).collect();
        out.push_str(&quoted.join(","));
        out.push_str("\r\n");
    }
    out
}

/// Filter the schema order to the requested `fields` option
/// (`formatters.py:102-105,137-140,193-196`); unknown names drop out.
pub fn select_export_fields(order: &[String], requested: Option<&[String]>) -> Vec<String> {
    match requested {
        Some(wanted) => order
            .iter()
            .filter(|f| wanted.iter().any(|w| w == *f))
            .cloned()
            .collect(),
        None => order.to_vec(),
    }
}

fn export_table(
    records: &[Vec<(String, PVal)>],
    order: &[String],
    list_joiner: &str,
) -> Vec<Vec<String>> {
    records
        .iter()
        .map(|record| {
            let map: HashMap<&str, &PVal> = record.iter().map(|(k, v)| (k.as_str(), v)).collect();
            order
                .iter()
                .map(|f| {
                    format_field_value(
                        map.get(f.as_str()).copied().unwrap_or(&PVal::Null),
                        list_joiner,
                    )
                })
                .collect()
        })
        .collect()
}

/// `CSVFormatter.format` (`exporters/formatters.py:94-114`): empty
/// records → `(f.csv, "")`; header from schema labels; sanitised
/// `QUOTE_ALL` rows.
pub fn exporters_csv_format(
    filename: &str,
    records: &[Vec<(String, PVal)>],
    order: &[String],
    labels: &HashMap<String, String>,
    requested: Option<&[String]>,
    list_joiner: &str,
) -> (String, String) {
    if records.is_empty() {
        return (format!("{filename}.csv"), String::new());
    }
    let fields = select_export_fields(order, requested);
    // `_create_csv_file` sanitises every row, header included.
    let sanitize = |row: Vec<String>| {
        row.iter()
            .map(|c| cleanup::sanitize_csv_value(c))
            .collect::<Vec<_>>()
    };
    let mut table = vec![sanitize(
        fields
            .iter()
            .map(|f| labels[f.as_str()].clone())
            .collect::<Vec<_>>(),
    )];
    for row in export_table(records, &fields, list_joiner) {
        table.push(sanitize(row));
    }
    (format!("{filename}.csv"), write_csv_quote_all(&table))
}

/// `JSONFormatter.format` (`exporters/formatters.py:129-147`): empty
/// records → `(f.json, "[]")`; rows keyed by label, types preserved
/// (`{label: record[field]}` — missing fields stay absent);
/// `json.dumps` with default separators.
pub fn exporters_json_format(
    filename: &str,
    records: &[Vec<(String, PVal)>],
    order: &[String],
    labels: &HashMap<String, String>,
    requested: Option<&[String]>,
) -> (String, String) {
    if records.is_empty() {
        return (format!("{filename}.json"), "[]".to_owned());
    }
    let fields = select_export_fields(order, requested);
    let rows: Vec<PVal> = records
        .iter()
        .map(|record| {
            let map: HashMap<&str, &PVal> = record.iter().map(|(k, v)| (k.as_str(), v)).collect();
            PVal::Dict(
                fields
                    .iter()
                    .filter_map(|f| {
                        map.get(f.as_str())
                            .map(|v| (labels[f.as_str()].clone(), (*v).clone()))
                    })
                    .collect(),
            )
        })
        .collect();
    (format!("{filename}.json"), py_dumps(&PVal::List(rows)))
}

/// Encoded `utils/exporters` payload: text for csv/json, bytes for xlsx.
#[derive(Debug, Clone, PartialEq)]
pub enum ExporterContent {
    Text(String),
    Bytes(Vec<u8>),
}

/// `Exporter.export` (`exporters/exporter.py:38-66`): already-serialised
/// records (a queryset would first run `serialize_queryset`) dispatched
/// to the configured formatter with `fields` merged into the options.
pub fn exporter_export(
    format: ExporterFormat,
    filename: &str,
    records: &[Vec<(String, PVal)>],
    order: &[String],
    labels: &HashMap<String, String>,
    requested: Option<&[String]>,
    list_joiner: &str,
) -> (String, ExporterContent) {
    match format {
        ExporterFormat::Csv => {
            let (name, content) =
                exporters_csv_format(filename, records, order, labels, requested, list_joiner);
            (name, ExporterContent::Text(content))
        }
        ExporterFormat::Json => {
            let (name, content) =
                exporters_json_format(filename, records, order, labels, requested);
            (name, ExporterContent::Text(content))
        }
        ExporterFormat::Xlsx => {
            let (name, content) =
                exporters_xlsx_format(filename, records, order, labels, requested, list_joiner);
            (name, ExporterContent::Bytes(content))
        }
    }
}

/// `XLSXFormatter.format` (`exporters/formatters.py:184-206`): empty
/// records → zero-row workbook; otherwise label headers plus
/// `_format_field_value` rows.
pub fn exporters_xlsx_format(
    filename: &str,
    records: &[Vec<(String, PVal)>],
    order: &[String],
    labels: &HashMap<String, String>,
    requested: Option<&[String]>,
    list_joiner: &str,
) -> (String, Vec<u8>) {
    if records.is_empty() {
        return (format!("{filename}.xlsx"), xlsx_workbook(None, &[]));
    }
    let fields = select_export_fields(order, requested);
    let headers: Vec<String> = fields.iter().map(|f| labels[f.as_str()].clone()).collect();
    let rows: Vec<Vec<XlsxCell>> = export_table(records, &fields, list_joiner)
        .into_iter()
        .map(|row| {
            row.into_iter()
                .map(|cell| {
                    if cell.is_empty() {
                        XlsxCell::Empty
                    } else {
                        XlsxCell::Text(cell)
                    }
                })
                .collect()
        })
        .collect();
    (
        format!("{filename}.xlsx"),
        xlsx_workbook(Some(&headers), &rows),
    )
}

// ---------------------------------------------------------------------------
// IssueExportSchema (utils/exporters/schemas/issue.py:70-213, base.py)
// ---------------------------------------------------------------------------

/// `IssueExportSchema._declared_fields` in source order with display
/// labels (`issue.py:91-120`).
pub const ISSUE_EXPORT_FIELDS: &[(&str, &str)] = &[
    ("id", "ID"),
    ("project_identifier", "Project Identifier"),
    ("project_name", "Project"),
    ("project_id", "Project ID"),
    ("sequence_id", "Sequence ID"),
    ("name", "Name"),
    ("description", "Description"),
    ("priority", "Priority"),
    ("start_date", "Start Date"),
    ("target_date", "Target Date"),
    ("state_name", "State"),
    ("created_at", "Created At"),
    ("updated_at", "Updated At"),
    ("completed_at", "Completed At"),
    ("archived_at", "Archived At"),
    ("module_name", "Module Name"),
    ("created_by", "Created By"),
    ("labels", "Labels"),
    ("comments", "Comments"),
    ("estimate", "Estimate"),
    ("link", "Link"),
    ("assignees", "Assignees"),
    ("subscribers_count", "Subscribers Count"),
    ("attachment_count", "Attachment Count"),
    ("attachment_links", "Attachment Links"),
    ("cycle_name", "Cycle Name"),
    ("cycle_start_date", "Cycle Start Date"),
    ("cycle_end_date", "Cycle End Date"),
    ("parent", "Parent"),
    ("relations", "Relations"),
];

/// Field order of [`ISSUE_EXPORT_FIELDS`].
pub fn issue_export_field_order() -> Vec<String> {
    ISSUE_EXPORT_FIELDS
        .iter()
        .map(|(n, _)| n.to_string())
        .collect()
}

/// Label map of [`ISSUE_EXPORT_FIELDS`].
pub fn issue_export_labels() -> HashMap<String, String> {
    ISSUE_EXPORT_FIELDS
        .iter()
        .map(|(n, l)| (n.to_string(), l.to_string()))
        .collect()
}

const WEEKDAY_SHORT: [&str; 7] = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];
const MONTH_SHORT: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

/// `DateField` rendering (`schemas/base.py:64-70`): `strftime("%a, %d %b
/// %Y")` with C-locale English names (Django stores naive dates; no
/// timezone applies).
pub fn format_export_date(date: NaiveDate) -> String {
    format!(
        "{}, {:02} {} {}",
        WEEKDAY_SHORT[date.weekday().num_days_from_monday() as usize],
        date.day(),
        MONTH_SHORT[date.month0() as usize],
        date.year()
    )
}

/// `DateTimeField` rendering (`schemas/base.py:79-85`):
/// `strftime("%a, %d %b %Y %I:%M:%S %Z%z")`. Stored datetimes are UTC;
/// `%Z` is `UTC`, `%z` is `+0000`.
pub fn format_export_datetime(moment: DateTime<Utc>) -> String {
    let (is_pm, hour12) = moment.hour12();
    let _ = is_pm;
    format!(
        "{}, {:02} {} {} {:02}:{:02}:{:02} UTC+0000",
        WEEKDAY_SHORT[moment.weekday().num_days_from_monday() as usize],
        moment.day(),
        MONTH_SHORT[moment.month0() as usize],
        moment.year(),
        hour12,
        moment.minute(),
        moment.second()
    )
}

/// A user display name. `_get_created_by` (`issue.py:73-81`) wraps
/// everything in a broad `except` and yields `""` — `None` here is the
/// `getattr(obj, "created_by", None)`-falsy path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NameRef {
    pub first_name: String,
    pub last_name: String,
}

/// `_get_created_by`: `"first last"` or `""`.
pub fn created_by_display(author: Option<&NameRef>) -> String {
    match author {
        Some(a) => format!("{} {}", a.first_name, a.last_name),
        None => String::new(),
    }
}

/// One serialised comment (`prepare_comments`, `issue.py:137-145`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommentSnap {
    pub stripped: String,
    pub created_at: NaiveDate,
    pub author: Option<NameRef>,
}

/// The last cycle of an issue (`prepare_cycle_*`, `issue.py:168-185`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CycleSnap {
    pub name: String,
    pub start: Option<NaiveDate>,
    pub end: Option<NaiveDate>,
}

/// One issue plus its prefetched relations, as `serialize_queryset`
/// hands it to `serialize` (`schemas/base.py:218-237` + `issue.py`'s
/// `prepare_*`). Dates that are `None` render as `""` (field defaults).
#[derive(Debug, Clone, PartialEq)]
pub struct IssueExportSnap {
    pub project_identifier: String,
    pub project_name: String,
    pub project_id: String,
    pub sequence_id: i64,
    pub name: String,
    pub description_stripped: String,
    /// `StringField(source="priority")`: `None` renders as `""`.
    pub priority: Option<String>,
    pub start_date: Option<NaiveDate>,
    pub target_date: Option<NaiveDate>,
    pub state_name: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
    pub archived_at: Option<DateTime<Utc>>,
    pub module_names: Vec<String>,
    pub created_by: Option<NameRef>,
    pub labels: Vec<String>,
    pub comments: Vec<CommentSnap>,
    /// `estimate_point.value`, falsy → `""` (`prepare_estimate`).
    pub estimate: Option<String>,
    pub links: Vec<String>,
    pub assignees: Vec<String>,
    pub subscribers_count: i64,
    /// This issue's asset ids from the attachments context
    /// (`get_issue_attachments_dict`).
    pub attachments: Vec<String>,
    pub workspace_slug: String,
    pub issue_id: String,
    pub cycle: Option<CycleSnap>,
    /// `(parent project identifier, parent sequence_id)`.
    pub parent: Option<(String, i64)>,
    /// `(relation_type, related project identifier, related sequence_id)`.
    pub outgoing: Vec<(String, String, i64)>,
    /// `(relation_type, issue project identifier, issue sequence_id)`.
    pub incoming: Vec<(String, String, i64)>,
}

/// Attachment download link (`prepare_attachment_links`, `issue.py:162-166`).
pub fn attachment_link(
    workspace_slug: &str,
    project_id: &str,
    issue_id: &str,
    asset_id: &str,
) -> String {
    format!("/api/assets/v2/workspaces/{workspace_slug}/projects/{project_id}/issues/{issue_id}/attachments/{asset_id}/")
}

/// Reverse relation lookup (`IssueRelationChoices._REVERSE_MAPPING`,
/// `db/models/issue.py:383-393`).
pub fn reverse_relation(relation_type: &str) -> Option<&'static str> {
    match relation_type {
        "blocked_by" => Some("blocking"),
        "relates_to" => Some("relates_to"),
        "duplicate" => Some("duplicate"),
        "start_before" => Some("start_after"),
        "finish_before" => Some("finish_after"),
        "implemented_by" => Some("implements"),
        _ => None,
    }
}

/// `prepare_relations` (`issue.py:192-206`): outgoing keyed by type, then
/// incoming keyed by reverse type — a dict, so duplicate types overwrite
/// (later writes win, incoming after outgoing). An unknown incoming type
/// is the Python `KeyError` path and fails here instead of silently
/// dropping.
pub fn prepare_relations(
    outgoing: &[(String, String, i64)],
    incoming: &[(String, String, i64)],
) -> Result<Vec<(String, PVal)>, String> {
    let mut relations: Vec<(String, PVal)> = Vec::new();
    let mut put = |key: String, ident: &str, seq: i64| {
        let value = PVal::Str(format!("{ident}-{seq}"));
        match relations.iter_mut().find(|(k, _)| *k == key) {
            Some(slot) => slot.1 = value,
            None => relations.push((key, value)),
        }
    };
    for (rel_type, ident, seq) in outgoing {
        put(rel_type.clone(), ident, *seq);
    }
    for (rel_type, ident, seq) in incoming {
        match reverse_relation(rel_type) {
            Some(reverse) => put(reverse.to_owned(), ident, *seq),
            None => return Err(format!("unknown relation type: {rel_type}")),
        }
    }
    Ok(relations)
}

/// `prepare_id` (`issue.py:122-123`): `project.identifier-sequence_id`
/// (not the uuid).
pub fn prepare_export_id(project_identifier: &str, sequence_id: i64) -> String {
    format!("{project_identifier}-{sequence_id}")
}

/// `IssueExportSchema.serialize` for one issue: every field in
/// [`ISSUE_EXPORT_FIELDS`] order, `prepare_*` where Python defines one,
/// plain source rendering otherwise.
pub fn serialize_issue_export(snap: &IssueExportSnap) -> Result<Vec<(String, PVal)>, String> {
    let str_val = |s: &str| PVal::Str(s.to_owned());
    let opt_date = |d: Option<NaiveDate>| match d {
        Some(v) => str_val(&format_export_date(v)),
        None => PVal::Str(String::new()),
    };
    let opt_moment = |m: Option<DateTime<Utc>>| match m {
        Some(v) => str_val(&format_export_datetime(v)),
        None => PVal::Str(String::new()),
    };
    let relations = prepare_relations(&snap.outgoing, &snap.incoming)?;
    let comments = snap
        .comments
        .iter()
        .map(|c| {
            PVal::Dict(vec![
                ("comment".to_owned(), str_val(&c.stripped)),
                (
                    "created_at".to_owned(),
                    str_val(&format_export_date(c.created_at)),
                ),
                (
                    "created_by".to_owned(),
                    str_val(&created_by_display(c.author.as_ref())),
                ),
            ])
        })
        .collect::<Vec<_>>();
    let cycle_name = snap
        .cycle
        .as_ref()
        .map(|c| c.name.clone())
        .unwrap_or_default();
    let cycle_start = snap.cycle.as_ref().and_then(|c| c.start);
    let cycle_end = snap.cycle.as_ref().and_then(|c| c.end);
    let parent = match &snap.parent {
        Some((ident, seq)) => str_val(&prepare_export_id(ident, *seq)),
        None => PVal::Str(String::new()),
    };
    let links: Vec<PVal> = snap
        .attachments
        .iter()
        .map(|asset| {
            str_val(&attachment_link(
                &snap.workspace_slug,
                &snap.project_id,
                &snap.issue_id,
                asset,
            ))
        })
        .collect();
    Ok(vec![
        (
            "id".to_owned(),
            str_val(&prepare_export_id(
                &snap.project_identifier,
                snap.sequence_id,
            )),
        ),
        (
            "project_identifier".to_owned(),
            str_val(&snap.project_identifier),
        ),
        ("project_name".to_owned(), str_val(&snap.project_name)),
        ("project_id".to_owned(), str_val(&snap.project_id)),
        ("sequence_id".to_owned(), PVal::Int(snap.sequence_id)),
        ("name".to_owned(), str_val(&snap.name)),
        (
            "description".to_owned(),
            str_val(&snap.description_stripped),
        ),
        (
            "priority".to_owned(),
            snap.priority
                .clone()
                .map(|p| str_val(&p))
                .unwrap_or(PVal::Str(String::new())),
        ),
        ("start_date".to_owned(), opt_date(snap.start_date)),
        ("target_date".to_owned(), opt_date(snap.target_date)),
        (
            "state_name".to_owned(),
            snap.state_name
                .clone()
                .map(|s| str_val(&s))
                .unwrap_or(PVal::Null),
        ),
        (
            "created_at".to_owned(),
            str_val(&format_export_datetime(snap.created_at)),
        ),
        (
            "updated_at".to_owned(),
            str_val(&format_export_datetime(snap.updated_at)),
        ),
        ("completed_at".to_owned(), opt_moment(snap.completed_at)),
        ("archived_at".to_owned(), opt_moment(snap.archived_at)),
        (
            "module_name".to_owned(),
            PVal::List(snap.module_names.iter().map(|m| str_val(m)).collect()),
        ),
        (
            "created_by".to_owned(),
            str_val(&created_by_display(snap.created_by.as_ref())),
        ),
        (
            "labels".to_owned(),
            PVal::List(snap.labels.iter().map(|l| str_val(l)).collect()),
        ),
        ("comments".to_owned(), PVal::List(comments)),
        (
            "estimate".to_owned(),
            match &snap.estimate {
                Some(v) if !v.is_empty() => str_val(v),
                _ => PVal::Str(String::new()),
            },
        ),
        (
            "link".to_owned(),
            PVal::List(snap.links.iter().map(|l| str_val(l)).collect()),
        ),
        (
            "assignees".to_owned(),
            PVal::List(snap.assignees.iter().map(|a| str_val(a)).collect()),
        ),
        (
            "subscribers_count".to_owned(),
            PVal::Int(snap.subscribers_count),
        ),
        (
            "attachment_count".to_owned(),
            PVal::Int(snap.attachments.len() as i64),
        ),
        ("attachment_links".to_owned(), PVal::List(links)),
        ("cycle_name".to_owned(), str_val(&cycle_name)),
        ("cycle_start_date".to_owned(), opt_date(cycle_start)),
        ("cycle_end_date".to_owned(), opt_date(cycle_end)),
        ("parent".to_owned(), parent),
        ("relations".to_owned(), PVal::Dict(relations)),
    ])
}

/// `get_issue_attachments_dict` (`schemas/issue.py:23-41`): assets of
/// type `ISSUE_ATTACHMENT` for the queryset's issues, annotated as
/// `work_item_id`/`asset_id`. `ids` is the caller's `values_list("id")`
/// placeholder list (e.g. `$1, $2`).
pub fn issue_attachments_sql(ids: &[String]) -> String {
    format!(
        "SELECT \"file_assets\".\"id\" AS \"asset_id\", \"file_assets\".\"issue_id\" AS \"work_item_id\" \
         FROM \"file_assets\" \
         WHERE (\"file_assets\".\"issue_id\" IN ({}) AND \"file_assets\".\"entity_type\" = 'ISSUE_ATTACHMENT' \
         AND \"file_assets\".\"deleted_at\" IS NULL)",
        ids.join(", ")
    )
}

/// `get_issue_last_cycles_dict` (`schemas/issue.py:44-67`): every
/// `CycleIssue` for the queryset's issues with its cycle, newest first
/// per issue (`ORDER BY issue_id, -created_at`); the caller keeps the
/// first row per issue.
pub fn issue_last_cycles_sql(ids: &[String]) -> String {
    format!(
        "SELECT \"cycle_issues\".*, \"cycles\".* FROM \"cycle_issues\" \
         LEFT OUTER JOIN \"cycles\" ON (\"cycle_issues\".\"cycle_id\" = \"cycles\".\"id\") \
         WHERE (\"cycle_issues\".\"issue_id\" IN ({}) AND \"cycle_issues\".\"deleted_at\" IS NULL \
         AND \"cycles\".\"deleted_at\" IS NULL) \
         ORDER BY \"cycle_issues\".\"issue_id\" ASC, \"cycle_issues\".\"created_at\" DESC",
        ids.join(", ")
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn pair(key: &str, value: PVal) -> (String, PVal) {
        (key.to_owned(), value)
    }

    fn text(value: &str) -> PVal {
        PVal::Str(value.to_owned())
    }

    /// Fixture `sample_row` as ordered pairs.
    fn sample_row() -> Vec<(String, PVal)> {
        vec![
            pair("project_identifier", text("AN")),
            pair("identifier", text("AN-3")),
            pair("name", text("I3")),
            pair("priority", text("medium")),
            pair("assignees", PVal::List(vec![])),
            pair("labels", PVal::List(vec![])),
            pair("estimate", PVal::Int(8)),
            pair("is_draft", PVal::Bool(false)),
        ]
    }

    #[test]
    fn json_encode_matches_python_indent() {
        // `JSONFormatter.encode`: `json.dumps(data, indent=2, default=str)`.
        assert_eq!(
            porter_json_encode(&[sample_row()]),
            "[\n  {\n    \"project_identifier\": \"AN\",\n    \"identifier\": \"AN-3\",\n    \"name\": \"I3\",\n    \"priority\": \"medium\",\n    \"assignees\": [],\n    \"labels\": [],\n    \"estimate\": 8,\n    \"is_draft\": false\n  }\n]"
        );
    }

    #[test]
    fn json_round_trips() {
        let rows = vec![sample_row()];
        let decoded = porter_json_decode(&porter_json_encode(&rows)).expect("decodes");
        assert_eq!(decoded, rows);
    }

    #[test]
    fn json_decode_rejects_non_objects() {
        assert!(porter_json_decode("[1, 2]").is_err());
        assert!(porter_json_decode("{\"a\": 1}").is_err());
        assert!(porter_json_decode("nope").is_err());
    }

    #[test]
    fn json_escapes_non_ascii_like_ensure_ascii() {
        let rows = vec![vec![pair("name", text("caf\u{e9} \"q\""))]];
        assert_eq!(
            porter_json_encode(&rows),
            "[\n  {\n    \"name\": \"caf\\u00e9 \\\"q\\\"\"\n  }\n]"
        );
    }

    #[test]
    fn csv_golden_matches_fixture() {
        // Fixture `CSVFormatter.golden_encode` byte for byte.
        let rows = vec![vec![
            pair("identifier", text("AN-3")),
            pair("name", text("I3")),
            pair("priority", text("medium")),
            pair("estimate", PVal::Int(8)),
        ]];
        assert_eq!(
            porter_csv_encode(&rows, true, ',', true),
            "Identifier,Name,Priority,Estimate\r\nAN-3,I3,medium,8\r\n"
        );
    }

    #[test]
    fn csv_empty_is_empty_string() {
        assert_eq!(porter_csv_encode(&[], true, ',', true), "");
    }

    #[test]
    fn csv_flatten_and_sanitize() {
        // Nested dicts flatten with `__`; lists dump with Python spacing;
        // formula-leading strings gain the `'` prefix.
        let rows = vec![vec![
            pair(
                "meta",
                PVal::Dict(vec![pair("owner", text("Ada")), pair("n", PVal::Int(1))]),
            ),
            pair("tags", PVal::List(vec![text("a"), text("b")])),
            pair("cmd", text("=run")),
            pair("neg", PVal::Int(-5)),
        ]];
        assert_eq!(
            porter_csv_encode(&rows, true, ',', true),
            "Meta  Owner,Meta  N,Tags,Cmd,Neg\r\nAda,1,\"[\"\"a\"\", \"\"b\"\"]\",'=run,-5\r\n"
        );
    }

    #[test]
    fn csv_raw_headers_and_custom_delimiter() {
        let rows = vec![vec![pair("a_b", text("x;y"))]];
        assert_eq!(
            porter_csv_encode(&rows, false, ';', false),
            "a_b\r\n\"x;y\"\r\n"
        );
    }

    #[test]
    fn csv_decode_normalizes_and_unflattens() {
        let content = "Display Name,Meta__Owner,Tags\r\nAda,\"{\"\"x\"\": 1}\",\"[1, 2]\"\r\n";
        let decoded = porter_csv_decode(content, ',', true, true);
        // `meta__owner` nests under `meta`; the cell JSON-parses to a
        // dict first (Python `_unflatten` parses before nesting).
        assert_eq!(
            decoded,
            vec![vec![
                pair("display_name", text("Ada")),
                pair(
                    "meta",
                    PVal::Dict(vec![pair(
                        "owner",
                        PVal::Dict(vec![pair("x", PVal::Int(1))])
                    )])
                ),
                pair("tags", PVal::List(vec![PVal::Int(1), PVal::Int(2)])),
            ]]
        );
    }

    #[test]
    fn title_matches_python_str_title() {
        assert_eq!(prettify_header("created_by_name"), "Created By Name");
        assert_eq!(prettify_header("field2name"), "Field2Name");
        assert_eq!(normalize_header("  Display Name "), "display_name");
    }

    #[test]
    fn xlsx_round_trips_typed_cells() {
        let rows = vec![vec![
            pair("identifier", text("AN-3")),
            pair("estimate", PVal::Int(8)),
            pair("ratio", PVal::Float(2.5)),
            pair("is_draft", PVal::Bool(false)),
            pair("nothing", PVal::Null),
            pair("tags", PVal::List(vec![text("a"), text("b")])),
            pair("meta", PVal::Dict(vec![pair("k", PVal::Int(1))])),
        ]];
        let bytes = porter_xlsx_encode(&rows, true, ", ");
        let decoded = xlsx_decode(&bytes, true).expect("decodes");
        assert_eq!(decoded.len(), 1);
        let map: HashMap<&str, &PVal> = decoded[0].iter().map(|(k, v)| (k.as_str(), v)).collect();
        assert_eq!(map["identifier"], &text("AN-3"));
        assert_eq!(map["estimate"], &PVal::Int(8));
        assert_eq!(map["ratio"], &PVal::Float(2.5));
        assert_eq!(map["is_draft"], &PVal::Bool(false));
        assert_eq!(map["nothing"], &PVal::Null);
        assert_eq!(map["tags"], &text("a, b"));
        assert_eq!(map["meta"], &PVal::Dict(vec![pair("k", PVal::Int(1))]));
        // Header keys are the normalized pretty headers.
        assert!(decoded[0].iter().any(|(k, _)| k == "identifier"));
    }

    #[test]
    fn xlsx_empty_decodes_to_empty() {
        let bytes = porter_xlsx_encode(&[], true, ", ");
        assert_eq!(
            xlsx_decode(&bytes, true).expect("decodes"),
            Vec::<Vec<(String, PVal)>>::new()
        );
        // The empty file is still a valid zip with a header-less sheet.
        let entries = read_zip_entries(&bytes).expect("zip reads");
        assert!(entries.iter().any(|(n, _)| n == "xl/worksheets/sheet1.xml"));
    }

    #[test]
    fn xlsx_missing_cells_decode_null() {
        let rows = vec![
            vec![pair("a", text("1")), pair("b", text("2"))],
            vec![pair("a", text("3"))],
        ];
        let bytes = porter_xlsx_encode(&rows, false, ", ");
        let decoded = xlsx_decode(&bytes, false).expect("decodes");
        assert_eq!(
            decoded[1],
            vec![pair("a", text("3")), pair("b", PVal::Null)]
        );
    }

    #[test]
    fn exporter_errors_are_byte_exact() {
        assert_eq!(
            validate_porter_format("xml"),
            Err("Unsupported format: xml. Available: ['csv', 'json', 'xlsx']".to_owned())
        );
        assert_eq!(
            porter_export("xml", "f", &[]),
            Err("Unsupported format: xml. Available: ['csv', 'json', 'xlsx']".to_owned())
        );
        assert_eq!(
            MISSING_FORMAT_TYPE_MSG,
            "format_type must be provided during initialization to use export() method"
        );
        assert_eq!(porter_available_formats(), vec!["csv", "json", "xlsx"]);
    }

    #[test]
    fn porter_export_filenames_and_payloads() {
        let rows = vec![sample_row()];
        let (name, content) = porter_export("csv", "an-ws", &rows).expect("csv");
        assert_eq!(name, "an-ws.csv");
        assert!(matches!(content, PorterContent::Text(_)));
        let (name, content) = porter_export("json", "an-ws", &rows).expect("json");
        assert_eq!(name, "an-ws.json");
        assert!(matches!(content, PorterContent::Text(_)));
        let (name, content) = porter_export("xlsx", "an-ws", &rows).expect("xlsx");
        assert_eq!(name, "an-ws.xlsx");
        assert!(matches!(content, PorterContent::Bytes(_)));
    }

    #[test]
    fn legacy_to_string_and_to_file() {
        let rows = vec![sample_row()];
        assert!(matches!(
            porter_to_string("csv", &rows),
            Ok(PorterContent::Text(_))
        ));
        assert!(matches!(
            porter_to_string("xlsx", &rows),
            Ok(PorterContent::Bytes(_))
        ));
        assert!(porter_to_string("xml", &rows).is_err());
        // Byte content fails the way Python text-mode fails it.
        assert_eq!(
            porter_to_file_bytes("f.xlsx", b"data"),
            Err("write() argument must be str, not bytes".to_owned())
        );
        let path = std::env::temp_dir().join("pidashconv-381-legacy.csv");
        let back = porter_to_file_text(path.to_str().expect("utf8"), "a,b\r\n").expect("writes");
        assert_eq!(back, path.to_str().expect("utf8"));
        assert_eq!(std::fs::read_to_string(&path).expect("reads"), "a,b\r\n");
        std::fs::remove_file(&path).expect("cleans up");
    }

    #[test]
    fn exporter_export_dispatches_by_format() {
        let order = vec!["name".to_owned()];
        let mut labels = HashMap::new();
        labels.insert("name".to_owned(), "Name".to_owned());
        let records = vec![vec![pair("name", text("I3"))]];
        let (name, content) = exporter_export(
            ExporterFormat::Csv,
            "f",
            &records,
            &order,
            &labels,
            None,
            ", ",
        );
        assert_eq!(name, "f.csv");
        assert!(matches!(content, ExporterContent::Text(_)));
        let (name, content) = exporter_export(
            ExporterFormat::Json,
            "f",
            &records,
            &order,
            &labels,
            None,
            ", ",
        );
        assert_eq!(name, "f.json");
        assert!(matches!(content, ExporterContent::Text(_)));
        let (name, content) = exporter_export(
            ExporterFormat::Xlsx,
            "f",
            &records,
            &order,
            &labels,
            None,
            ", ",
        );
        assert_eq!(name, "f.xlsx");
        assert!(matches!(content, ExporterContent::Bytes(_)));
        assert_eq!(
            parse_exporter_format("xml"),
            Err("Unsupported format: xml. Available: ['csv', 'json', 'xlsx']".to_owned())
        );
    }

    #[test]
    fn registry_tracks_register_formatter() {
        let mut registry = ExporterRegistry::default();
        assert_eq!(registry.available(), vec!["csv", "json", "xlsx"]);
        assert_eq!(registry.lookup("csv"), Some("CSVFormatter"));
        registry.register("parquet", "ParquetFormatter");
        assert_eq!(registry.lookup("parquet"), Some("ParquetFormatter"));
        registry.register("csv", "CustomCSV");
        assert_eq!(registry.lookup("csv"), Some("CustomCSV"));
        assert_eq!(registry.available(), vec!["csv", "json", "xlsx", "parquet"]);
    }

    #[test]
    fn schema_field_info_labels_and_missing() {
        let declared = vec![
            FieldDef::new("project_identifier", Some("Project Identifier")),
            FieldDef::new("sequence_id", None),
        ];
        let (order, labels) = schema_field_info("S", Some(&declared)).expect("info");
        assert_eq!(order, vec!["project_identifier", "sequence_id"]);
        assert_eq!(labels["project_identifier"], "Project Identifier");
        assert_eq!(labels["sequence_id"], "Sequence Id");
        assert_eq!(
            schema_field_info("S", None),
            Err("Schema class S must have _declared_fields attribute".to_owned())
        );
    }

    #[test]
    fn exporters_csv_is_quote_all_sanitised() {
        let order = vec!["name".to_owned(), "note".to_owned()];
        let mut labels = HashMap::new();
        labels.insert("name".to_owned(), "Name".to_owned());
        labels.insert("note".to_owned(), "Note".to_owned());
        let records = vec![vec![pair("name", text("I3")), pair("note", text("=x, y"))]];
        assert_eq!(
            exporters_csv_format("f", &records, &order, &labels, None, ", "),
            (
                "f.csv".to_owned(),
                "\"Name\",\"Note\"\r\n\"I3\",\"'=x, y\"\r\n".to_owned()
            )
        );
        assert_eq!(
            exporters_csv_format("f", &[], &order, &labels, None, ", "),
            ("f.csv".to_owned(), String::new())
        );
    }

    #[test]
    fn exporters_json_keeps_types_and_drops_missing() {
        let order = vec!["name".to_owned(), "estimate".to_owned(), "gone".to_owned()];
        let mut labels = HashMap::new();
        labels.insert("name".to_owned(), "Name".to_owned());
        labels.insert("estimate".to_owned(), "Estimate".to_owned());
        labels.insert("gone".to_owned(), "Gone".to_owned());
        let records = vec![vec![
            pair("name", text("I3")),
            pair("estimate", PVal::Int(8)),
        ]];
        assert_eq!(
            exporters_json_format("f", &records, &order, &labels, None),
            (
                "f.json".to_owned(),
                "[{\"Name\": \"I3\", \"Estimate\": 8}]".to_owned()
            )
        );
        assert_eq!(
            exporters_json_format("f", &[], &order, &labels, None),
            ("f.json".to_owned(), "[]".to_owned())
        );
    }

    #[test]
    fn exporters_field_filtering() {
        let order = vec!["a".to_owned(), "b".to_owned()];
        assert_eq!(
            select_export_fields(&order, Some(&["b".to_owned(), "zzz".to_owned()])),
            vec!["b".to_owned()]
        );
    }

    #[test]
    fn exporters_xlsx_round_trips_labels() {
        let order = vec!["name".to_owned(), "estimate".to_owned()];
        let mut labels = HashMap::new();
        labels.insert("name".to_owned(), "Name".to_owned());
        labels.insert("estimate".to_owned(), "Estimate".to_owned());
        let records = vec![vec![
            pair("name", text("I3")),
            pair("estimate", PVal::Int(8)),
        ]];
        let (name, bytes) = exporters_xlsx_format("f", &records, &order, &labels, None, ", ");
        assert_eq!(name, "f.xlsx");
        let decoded = xlsx_decode(&bytes, false).expect("decodes");
        assert_eq!(
            decoded,
            vec![vec![pair("Name", text("I3")), pair("Estimate", text("8"))]]
        );
        let (empty_name, empty) = exporters_xlsx_format("f", &[], &order, &labels, None, ", ");
        assert_eq!(empty_name, "f.xlsx");
        assert_eq!(
            xlsx_decode(&empty, false).expect("decodes"),
            Vec::<Vec<(String, PVal)>>::new()
        );
    }

    #[test]
    fn issue_schema_field_order_and_labels() {
        assert_eq!(ISSUE_EXPORT_FIELDS.len(), 30);
        assert_eq!(issue_export_field_order()[0], "id");
        assert_eq!(issue_export_field_order()[29], "relations");
        let labels = issue_export_labels();
        assert_eq!(labels["project_identifier"], "Project Identifier");
        assert_eq!(labels["id"], "ID");
    }

    #[test]
    fn export_dates_render_like_strftime() {
        let date = NaiveDate::from_ymd_opt(2024, 1, 1).expect("date");
        assert_eq!(format_export_date(date), "Mon, 01 Jan 2024");
        let moment = Utc.with_ymd_and_hms(2024, 1, 1, 2, 30, 45).unwrap();
        assert_eq!(
            format_export_datetime(moment),
            "Mon, 01 Jan 2024 02:30:45 UTC+0000"
        );
        let midnight = Utc.with_ymd_and_hms(2024, 1, 1, 0, 5, 6).unwrap();
        assert_eq!(
            format_export_datetime(midnight),
            "Mon, 01 Jan 2024 12:05:06 UTC+0000"
        );
    }

    fn export_snap() -> IssueExportSnap {
        IssueExportSnap {
            project_identifier: "AN".to_owned(),
            project_name: "Analytics".to_owned(),
            project_id: "proj-1".to_owned(),
            sequence_id: 3,
            name: "I3".to_owned(),
            description_stripped: "stripped".to_owned(),
            priority: Some("medium".to_owned()),
            start_date: Some(NaiveDate::from_ymd_opt(2024, 1, 1).expect("date")),
            target_date: None,
            state_name: None,
            created_at: Utc.with_ymd_and_hms(2024, 1, 1, 2, 30, 45).unwrap(),
            updated_at: Utc.with_ymd_and_hms(2024, 1, 2, 3, 4, 5).unwrap(),
            completed_at: None,
            archived_at: None,
            module_names: vec!["M1".to_owned()],
            created_by: Some(NameRef {
                first_name: "Ada".to_owned(),
                last_name: "L".to_owned(),
            }),
            labels: vec!["bug".to_owned()],
            comments: vec![CommentSnap {
                stripped: "hi".to_owned(),
                created_at: NaiveDate::from_ymd_opt(2024, 1, 3).expect("date"),
                author: None,
            }],
            estimate: Some("8".to_owned()),
            links: vec!["https://x".to_owned()],
            assignees: vec!["Ada L".to_owned()],
            subscribers_count: 2,
            attachments: vec!["asset-1".to_owned()],
            workspace_slug: "ws".to_owned(),
            issue_id: "issue-1".to_owned(),
            cycle: Some(CycleSnap {
                name: "C1".to_owned(),
                start: Some(NaiveDate::from_ymd_opt(2024, 1, 1).expect("date")),
                end: None,
            }),
            parent: Some(("AN".to_owned(), 1)),
            outgoing: vec![("blocked_by".to_owned(), "AN".to_owned(), 2)],
            incoming: vec![("blocked_by".to_owned(), "AN".to_owned(), 9)],
        }
    }

    #[test]
    fn issue_serialize_matches_prepare_goldens() {
        let pairs = serialize_issue_export(&export_snap()).expect("serializes");
        assert_eq!(pairs.len(), 30);
        let map: HashMap<&str, &PVal> = pairs.iter().map(|(k, v)| (k.as_str(), v)).collect();
        // `prepare_id` uses the project identifier, not the uuid.
        assert_eq!(map["id"], &text("AN-3"));
        assert_eq!(map["description"], &text("stripped"));
        assert_eq!(map["priority"], &text("medium"));
        assert_eq!(map["start_date"], &text("Mon, 01 Jan 2024"));
        assert_eq!(map["target_date"], &text(""));
        // `prepare_state_name`: no state → null (not "").
        assert_eq!(map["state_name"], &PVal::Null);
        assert_eq!(
            map["created_at"],
            &text("Mon, 01 Jan 2024 02:30:45 UTC+0000")
        );
        assert_eq!(map["created_by"], &text("Ada L"));
        assert_eq!(map["labels"], &PVal::List(vec![text("bug")]));
        assert_eq!(
            map["comments"],
            &PVal::List(vec![PVal::Dict(vec![
                pair("comment", text("hi")),
                pair("created_at", text("Wed, 03 Jan 2024")),
                pair("created_by", text("")),
            ])])
        );
        assert_eq!(map["estimate"], &text("8"));
        assert_eq!(map["attachment_count"], &PVal::Int(1));
        assert_eq!(
            map["attachment_links"],
            &PVal::List(vec![text(
                "/api/assets/v2/workspaces/ws/projects/proj-1/issues/issue-1/attachments/asset-1/"
            )])
        );
        assert_eq!(map["cycle_name"], &text("C1"));
        assert_eq!(map["cycle_end_date"], &text(""));
        assert_eq!(map["parent"], &text("AN-1"));
        // Duplicate relation types overwrite: outgoing `blocked_by` is
        // replaced by the incoming reverse `blocking`... and the incoming
        // `blocked_by` maps to `blocking`, a different key.
        assert_eq!(
            map["relations"],
            &PVal::Dict(vec![
                pair("blocked_by", text("AN-2")),
                pair("blocking", text("AN-9")),
            ])
        );
    }

    #[test]
    fn issue_serialize_empty_estimate_and_no_cycle() {
        let mut snap = export_snap();
        snap.estimate = Some(String::new());
        snap.cycle = None;
        snap.outgoing = vec![
            ("duplicate".to_owned(), "AN".to_owned(), 1),
            ("duplicate".to_owned(), "AN".to_owned(), 2),
        ];
        snap.incoming = vec![];
        let pairs = serialize_issue_export(&snap).expect("serializes");
        let map: HashMap<&str, &PVal> = pairs.iter().map(|(k, v)| (k.as_str(), v)).collect();
        // Falsy estimate → "".
        assert_eq!(map["estimate"], &text(""));
        assert_eq!(map["cycle_name"], &text(""));
        // Same-type outgoing relations overwrite: last wins.
        assert_eq!(
            map["relations"],
            &PVal::Dict(vec![pair("duplicate", text("AN-2"))])
        );
    }

    #[test]
    fn relations_reject_unknown_incoming_type() {
        assert!(prepare_relations(&[], &[("weird".to_owned(), "AN".to_owned(), 1)]).is_err());
        // All six reverse pairs resolve.
        for (forward, reverse) in [
            ("blocked_by", "blocking"),
            ("relates_to", "relates_to"),
            ("duplicate", "duplicate"),
            ("start_before", "start_after"),
            ("finish_before", "finish_after"),
            ("implemented_by", "implements"),
        ] {
            let out =
                prepare_relations(&[], &[(forward.to_owned(), "AN".to_owned(), 1)]).expect("known");
            assert_eq!(out, vec![pair(reverse, text("AN-1"))]);
        }
    }

    #[test]
    fn context_sql_pins_tables_and_guards() {
        let attach = issue_attachments_sql(&["$1".to_owned()]);
        assert!(attach.contains("\"file_assets\""));
        assert!(attach.contains("entity_type\" = 'ISSUE_ATTACHMENT'"));
        assert!(attach.contains("AS \"asset_id\""));
        assert!(attach.contains("AS \"work_item_id\""));
        let cycles = issue_last_cycles_sql(&["$1".to_owned(), "$2".to_owned()]);
        assert!(cycles.contains("\"cycle_issues\""));
        assert!(cycles.contains("JOIN \"cycles\""));
        assert!(cycles.contains("ORDER BY \"cycle_issues\".\"issue_id\" ASC"));
        assert!(cycles.contains("\"created_at\" DESC"));
    }
}
