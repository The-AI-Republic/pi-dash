//! Export-task logic: CSV, rows, ZIP, S3 protocol, expiry, mail.
//!
//! Source lines below refer to `apps/api/pi_dash/bgtasks/` unless noted.

use std::collections::HashMap;
use std::io::Write;

use pidash_types::tasks_cleanup::exports_dto::{
    self, CsvCell, ExportEmail, EXPORT_FORMATS, FALLBACK_X_HEADER, FALLBACK_Y_HEADER,
};

/// `utils/csv_utils.py:9`: formula-triggering first characters.
const CSV_FORMULA_TRIGGERS: [char; 7] = ['=', '+', '-', '@', '\t', '\r', '\n'];

/// `sanitize_csv_value`: prefix strings starting with a trigger char
/// with a single quote (OWASP CSV injection).
pub fn sanitize_csv_value(value: &str) -> String {
    match value.chars().next() {
        Some(first) if CSV_FORMULA_TRIGGERS.contains(&first) => format!("'{value}"),
        _ => value.to_owned(),
    }
}

/// `sanitize_csv_row`: sanitise every cell of a row.
pub fn sanitize_csv_row(row: &[String]) -> Vec<String> {
    row.iter().map(|cell| sanitize_csv_value(cell)).collect()
}

/// Quote one field exactly as `csv.writer(..., quoting=QUOTE_ALL)`
/// does: wrap in `"`, double embedded quotes. The CRLF terminator is
/// added by [`write_csv_rows`], never stored in the field.
fn quote_field(rendered: &str) -> String {
    format!("\"{}\"", rendered.replace('"', "\"\""))
}

/// Render rows to CSV text: QUOTE_ALL + `\r\n` lineterminator
/// (`generate_csv_from_rows:180-185`), mirroring
/// `[writer.writerow(sanitize_csv_row(row)) for row in rows]`.
/// Sanitising applies per Python `sanitize_csv_value`: only `str` cells
/// are ever prefixed, so only [`CsvCell::Text`] goes through
/// [`sanitize_csv_value`] — rendered numerics/bools pass through
/// untouched (a Python `int(-5)` writes `"-5"`, never `"'-5"`).
/// `None` renders as `""`, exactly as `csv.writer` writes it.
pub fn write_csv_rows(rows: &[Vec<CsvCell>]) -> String {
    let mut out = String::new();
    for row in rows {
        let rendered: Vec<String> = row
            .iter()
            .map(|cell| match cell {
                CsvCell::Text(text) => sanitize_csv_value(text),
                other => other.render(),
            })
            .collect();
        let quoted: Vec<String> = rendered.iter().map(|cell| quote_field(cell)).collect();
        out.push_str(&quoted.join(","));
        out.push_str("\r\n");
    }
    out
}

/// Convenience form for pre-rendered string rows (the
/// `export_analytics_to_csv_email:424-426` path, where rows are already
/// `item.get(key, "")` strings).
pub fn write_string_rows(rows: &[Vec<String>]) -> String {
    let cells: Vec<Vec<CsvCell>> = rows
        .iter()
        .map(|row| row.iter().cloned().map(CsvCell::Text).collect())
        .collect();
    write_csv_rows(&cells)
}

/// A distribution point for the segmented builder: the segment label plus
/// the aggregated value, if present (`None` mirrors `obj.get(key)` being
/// `None`, which the `:212` sum skips).
#[derive(Debug, Clone, PartialEq)]
pub struct SegmentPoint {
    pub segment: String,
    pub value: Option<Numeric>,
}

/// Integer or float aggregate. Counts arrive as integers
/// (`Count("*")`), estimates as floats (`Sum(Cast(..., FloatField()))`,
/// `analytics_plot.py:107-114`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Numeric {
    Int(i64),
    Float(f64),
}

impl Numeric {
    fn add(self, other: Numeric) -> Numeric {
        match (self, other) {
            (Numeric::Int(a), Numeric::Int(b)) => Numeric::Int(a + b),
            (Numeric::Int(a), Numeric::Float(b)) => Numeric::Float(a as f64 + b),
            (Numeric::Float(a), Numeric::Int(b)) => Numeric::Float(a + b as f64),
            (Numeric::Float(a), Numeric::Float(b)) => Numeric::Float(a + b),
        }
    }

    fn cell(self) -> CsvCell {
        match self {
            Numeric::Int(value) => CsvCell::Int(value),
            Numeric::Float(value) => CsvCell::Float(value),
        }
    }
}

/// Sum values, skipping `None` (`sum(obj.get(key) ... if ... is not None)`,
/// `:212`). Starts at integer zero like Python's `sum`.
fn sum_values(points: &[SegmentPoint]) -> Numeric {
    points
        .iter()
        .filter_map(|point| point.value)
        .fold(Numeric::Int(0), Numeric::add)
}

/// `row_mapping.get(axis, fallback)` (`:204-207,345`).
fn header_for(axis: &str, fallback: &str) -> String {
    exports_dto::ROW_MAPPING
        .iter()
        .find(|(key, _)| *key == axis)
        .map(|(_, label)| label.to_string())
        .unwrap_or_else(|| fallback.to_owned())
}

/// Resolve an x-axis item id to its display name through a detail list
/// (`str(detail[key]) == str(item)`, `:220-253`). Returns the item id
/// unchanged when nothing matches (unresolved ids stay raw).
fn resolve_name(
    details: &[HashMap<String, String>],
    key: &str,
    item: &str,
    name_key: &str,
) -> String {
    details
        .iter()
        .find(|row| row.get(key).map(String::as_str).unwrap_or("") == item)
        .and_then(|row| row.get(name_key).cloned())
        .unwrap_or_else(|| item.to_owned())
}

fn resolve_assignee_name(details: &[HashMap<String, String>], key: &str, item: &str) -> String {
    details
        .iter()
        .find(|row| row.get(key).map(String::as_str).unwrap_or("") == item)
        .map(|row| {
            format!(
                "{} {}",
                row.get("assignees__first_name")
                    .map(String::as_str)
                    .unwrap_or(""),
                row.get("assignees__last_name")
                    .map(String::as_str)
                    .unwrap_or("")
            )
        })
        .unwrap_or_else(|| item.to_owned())
}

/// `generate_segmented_rows:188-290`. `distribution` preserves the
/// queryset dimension order (Python dict insertion order); each point
/// carries the `:360` value (`"count"` iff `y_axis == "issue_count"`,
/// else `"estimate"` — see [`resolve_value_key_is_count`]).
/// Detail slices arrive as string maps; only the axes in use are
/// populated, the rest are empty (`analytic_export_task:362-372`).
#[allow(clippy::too_many_arguments)]
pub fn generate_segmented_rows(
    distribution: &[(String, Vec<SegmentPoint>)],
    x_axis: &str,
    y_axis: &str,
    segment: &str,
    assignee_details: &[HashMap<String, String>],
    label_details: &[HashMap<String, String>],
    state_details: &[HashMap<String, String>],
    cycle_details: &[HashMap<String, String>],
    module_details: &[HashMap<String, String>],
) -> Vec<Vec<CsvCell>> {
    use pidash_types::tasks_cleanup::exports_dto::{
        ASSIGNEE_ID, CYCLE_ID, LABEL_ID, MODULE_ID, STATE_ID,
    };

    // `:200`: unique segments; first-seen order (see module docs).
    let mut segment_zero: Vec<String> = Vec::new();
    for (_, points) in distribution {
        for point in points {
            if !segment_zero.contains(&point.segment) {
                segment_zero.push(point.segment.clone());
            }
        }
    }
    let mut header = vec![
        CsvCell::Text(header_for(x_axis, FALLBACK_X_HEADER)),
        CsvCell::Text(header_for(y_axis, FALLBACK_Y_HEADER)),
    ];
    header.extend(segment_zero.iter().cloned().map(CsvCell::Text));

    let mut rows = vec![header];
    for (item, points) in distribution {
        let mut generated = vec![CsvCell::Text(item.clone()), sum_values(points).cell()];
        for seg in &segment_zero {
            // `:217`: `next((x.get(key) ...), "0")` — the STRING "0"
            // (BUG-2, kept) applies only when NO point carries the
            // segment. A point whose value is `None` (`x.get(key)` is
            // `None`, e.g. an all-null `Sum` estimate) yields `None`,
            // which `csv.writer` writes as `""`.
            let value = match points.iter().find(|point| &point.segment == seg) {
                Some(point) => point.value.map(Numeric::cell).unwrap_or(CsvCell::Empty),
                None => CsvCell::Text("0".to_owned()),
            };
            generated.push(value);
        }
        generated[0] = CsvCell::Text(resolve_x_name(
            x_axis,
            item,
            assignee_details,
            label_details,
            state_details,
            cycle_details,
            module_details,
        ));
        rows.push(generated);
    }

    // Segment-header resolution `:257-288`.
    for (index, segm) in segment_zero.iter().enumerate() {
        let resolved = match segment {
            s if s == ASSIGNEE_ID => Some(resolve_assignee_name(assignee_details, s, segm)),
            s if s == LABEL_ID => label_details
                .iter()
                .find(|row| row.get(s).map(String::as_str).unwrap_or("") == segm)
                .map(|row| {
                    row.get("labels__name")
                        .cloned()
                        .unwrap_or_else(|| segm.clone())
                }),
            s if s == STATE_ID => state_details
                .iter()
                .find(|row| row.get(s).map(String::as_str).unwrap_or("") == segm)
                .map(|row| {
                    row.get("state__name")
                        .cloned()
                        .unwrap_or_else(|| segm.clone())
                }),
            // BUG-1 (`:278-282`): MODULE segment headers read
            // `label_details` with module-id keys, so they never match.
            s if s == MODULE_ID => label_details
                .iter()
                .find(|row| row.get(s).map(String::as_str).unwrap_or("") == segm)
                .map(|row| {
                    row.get("issue_module__module__name")
                        .cloned()
                        .unwrap_or_else(|| segm.clone())
                }),
            s if s == CYCLE_ID => cycle_details
                .iter()
                .find(|row| row.get(s).map(String::as_str).unwrap_or("") == segm)
                .map(|row| {
                    row.get("issue_cycle__cycle__name")
                        .cloned()
                        .unwrap_or_else(|| segm.clone())
                }),
            _ => None,
        };
        if let Some(name) = resolved {
            rows[0][index + 2] = CsvCell::Text(name);
        }
    }
    rows
}

fn resolve_x_name(
    x_axis: &str,
    item: &str,
    assignee_details: &[HashMap<String, String>],
    label_details: &[HashMap<String, String>],
    state_details: &[HashMap<String, String>],
    cycle_details: &[HashMap<String, String>],
    module_details: &[HashMap<String, String>],
) -> String {
    use pidash_types::tasks_cleanup::exports_dto::{
        ASSIGNEE_ID, CYCLE_ID, LABEL_ID, MODULE_ID, STATE_ID,
    };
    if x_axis == ASSIGNEE_ID {
        return resolve_assignee_name(assignee_details, ASSIGNEE_ID, item);
    }
    if x_axis == LABEL_ID {
        return resolve_name(label_details, LABEL_ID, item, "labels__name");
    }
    if x_axis == STATE_ID {
        return resolve_name(state_details, STATE_ID, item, "state__name");
    }
    if x_axis == CYCLE_ID {
        return resolve_name(cycle_details, CYCLE_ID, item, "issue_cycle__cycle__name");
    }
    if x_axis == MODULE_ID {
        return resolve_name(
            module_details,
            MODULE_ID,
            item,
            "issue_module__module__name",
        );
    }
    item.to_owned()
}

/// One non-segmented distribution row: only the FIRST element is read
/// (`data[0].get("count"|"estimate")`, `:306`).
#[derive(Debug, Clone, PartialEq)]
pub struct FlatPoint {
    pub count: Option<Numeric>,
    pub estimate: Option<Numeric>,
}

/// `generate_non_segmented_rows:293-346`. The value key follows `y_axis`
/// (`"count"` iff `y_axis == "issue_count"`, else `"estimate"`).
#[allow(clippy::too_many_arguments)]
pub fn generate_non_segmented_rows(
    distribution: &[(String, FlatPoint)],
    x_axis: &str,
    y_axis: &str,
    assignee_details: &[HashMap<String, String>],
    label_details: &[HashMap<String, String>],
    state_details: &[HashMap<String, String>],
    cycle_details: &[HashMap<String, String>],
    module_details: &[HashMap<String, String>],
) -> Vec<Vec<CsvCell>> {
    let header = vec![
        CsvCell::Text(header_for(x_axis, FALLBACK_X_HEADER)),
        CsvCell::Text(header_for(y_axis, FALLBACK_Y_HEADER)),
    ];
    let mut rows = vec![header];
    for (item, point) in distribution {
        let raw = if y_axis == "issue_count" {
            point.count
        } else {
            point.estimate
        };
        let value = raw.map(Numeric::cell).unwrap_or(CsvCell::Empty);
        let name = resolve_x_name(
            x_axis,
            item,
            assignee_details,
            label_details,
            state_details,
            cycle_details,
            module_details,
        );
        rows.push(vec![CsvCell::Text(name), value]);
    }
    rows
}

/// `analytic_export_task:360`: the distribution value key.
pub fn resolve_value_key_is_count(y_axis: &str) -> bool {
    y_axis == "issue_count"
}

/// Which detail querysets the task fetches: only axes in use, `{}` for
/// the rest (`:362-372`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DetailNeeds {
    pub assignee: bool,
    pub label: bool,
    pub state: bool,
    pub cycle: bool,
    pub module: bool,
}

/// `x_axis` / `segment` arrive as `data.get(..., False)`; `None` and `""`
/// mirror falsy.
pub fn detail_needs(x_axis: Option<&str>, segment: Option<&str>) -> DetailNeeds {
    use pidash_types::tasks_cleanup::exports_dto::{
        ASSIGNEE_ID, CYCLE_ID, LABEL_ID, MODULE_ID, STATE_ID,
    };
    let uses = |axis: &str| x_axis == Some(axis) || segment == Some(axis);
    DetailNeeds {
        assignee: uses(ASSIGNEE_ID),
        label: uses(LABEL_ID),
        state: uses(STATE_ID),
        cycle: uses(CYCLE_ID),
        module: uses(MODULE_ID),
    }
}

/// `data.get("segment", False)` truthiness: segmented rows iff the
/// segment is present and non-empty (`:374`).
pub fn is_segmented(segment: Option<&str>) -> bool {
    segment.map(|s| !s.is_empty()).unwrap_or(false)
}

/// `DataExporter` provider check (`exporter.py:44-46`): only
/// csv/json/xlsx construct; anything else raises `ValueError`, which the
/// task records as `failed` + reason (`export_task.py:192-201`). The
/// message is byte-exact: Python formats the key list with `repr`
/// (single quotes).
pub fn validate_provider(provider: &str) -> Result<(), String> {
    if EXPORT_FORMATS.contains(&provider) {
        Ok(())
    } else {
        let available = EXPORT_FORMATS
            .iter()
            .map(|format| format!("'{format}'"))
            .collect::<Vec<_>>()
            .join(", ");
        Err(format!(
            "Unsupported format: {provider}. Available: [{available}]"
        ))
    }
}

/// One ZIP entry for [`build_zip`].
pub struct ZipEntry<'a> {
    pub name: &'a str,
    pub content: &'a [u8],
}

/// Plain-format ceiling for ZIP sizes and offsets: CPython 3.12 `zipfile`
/// (`ZIP64_LIMIT`) writes zip64 records once a value is *above* this limit
/// — so entries between 2 GiB and 4 GiB already carry zip64 extras on the
/// Django side, and anything at or past 4 GiB needs them for correctness.
/// The contract suites pin Python 3.12, so the writer mirrors it exactly.
const ZIP64_LIMIT: u64 = 0x7FFF_FFFF;

/// One compressed entry ready to serialize. Lengths are declared, not
/// derived, so tests can probe the zip64 thresholds with tiny payloads;
/// [`build_zip`] always passes the true lengths and offsets.
struct ZipRecord<'a> {
    name: &'a [u8],
    crc: u32,
    uncomp_len: u64,
    comp: &'a [u8],
    comp_len: u64,
    header_offset: u64,
}

/// `create_zip_file:28-38`: ZIP_DEFLATED entries, duplicate names append,
/// buffer rewound (`seek(0)`) before return — the `Vec<u8>` is the whole
/// buffer from position zero. Deflate compression mirrors
/// `ZIP_DEFLATED`; entry timestamps are fixed metadata (Python stamps
/// local time, which no consumer reads).
///
/// Sizes and offsets that pass [`ZIP64_LIMIT`] ride zip64 extras and the
/// zip64 end-of-central-directory record, exactly as CPython 3.12
/// `zipfile` writes them (`ZipFile.writestr` + `_write_end_record`, both
/// triggered by `>` comparisons); below every threshold the bytes are the
/// plain format with no zip64 records. All arithmetic is checked `u64`, so
/// no size or offset can wrap or truncate (PIDASHCONV-1700).
pub fn build_zip(entries: &[ZipEntry<'_>]) -> Vec<u8> {
    let mut blocks: Vec<Vec<u8>> = Vec::with_capacity(entries.len());
    for entry in entries {
        let mut encoder =
            flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::default());
        encoder
            .write_all(entry.content)
            .expect("deflate encode cannot fail");
        blocks.push(encoder.finish().expect("deflate finish cannot fail"));
    }
    // Layout: local header (30) + name + extra + payload per entry, with
    // checked offsets. `usize as u64` widens and cannot truncate.
    let mut records: Vec<ZipRecord<'_>> = Vec::with_capacity(entries.len());
    let mut offset: u64 = 0;
    for (entry, comp) in entries.iter().zip(blocks.iter()) {
        let uncomp_len = entry.content.len() as u64;
        let comp_len = comp.len() as u64;
        let extra_len: u64 = if uncomp_len > ZIP64_LIMIT || comp_len > ZIP64_LIMIT {
            // Extra id + size + the paired u64 sizes (see `assemble_zip`).
            20
        } else {
            0
        };
        records.push(ZipRecord {
            name: entry.name.as_bytes(),
            crc: crc32fast::hash(entry.content),
            uncomp_len,
            comp,
            comp_len,
            header_offset: offset,
        });
        offset = offset
            .checked_add(30)
            .and_then(|v| v.checked_add(entry.name.len() as u64))
            .and_then(|v| v.checked_add(extra_len))
            .and_then(|v| v.checked_add(comp_len))
            .expect("zip local section exceeds u64");
    }
    assemble_zip(&records)
}

/// Serialize compressed records: local headers + payloads, the central
/// directory, then the zip64 EOCD record + locator (only when needed) and
/// the plain EOCD. Mirrors CPython 3.12 `zipfile`: a per-entry extra
/// (id `0x0001`) when either size passes [`ZIP64_LIMIT`] — local extras
/// always pair both sizes, central extras append the header offset when it
/// passes the limit — version 45 on entries carrying an extra, and a zip64
/// EOCD record when the count passes `0xFFFF` or the central directory
/// size or offset passes the limit (plain EOCD fields clamp to the marker,
/// as `zipfile`'s `min(...)` does).
fn assemble_zip(records: &[ZipRecord<'_>]) -> Vec<u8> {
    const METHOD_DEFLATED: u16 = 8;
    const VERSION_PLAIN: u16 = 20;
    const VERSION_ZIP64: u16 = 45;
    let mut out: Vec<u8> = Vec::new();

    for record in records {
        let size64 = record.uncomp_len > ZIP64_LIMIT || record.comp_len > ZIP64_LIMIT;
        let version = if size64 { VERSION_ZIP64 } else { VERSION_PLAIN };
        // CPython `struct.pack("<H", len(name))` raises past 64 KiB; export
        // names are short, so reaching this is a programming error either way.
        let name_len = u16::try_from(record.name.len()).expect("zip entry name exceeds u16");
        // Local file header.
        out.extend_from_slice(&0x0403_4b50u32.to_le_bytes());
        out.extend_from_slice(&version.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&METHOD_DEFLATED.to_le_bytes());
        // Fixed MS-DOS date/time (metadata only; see docs above).
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&0x2Bu16.to_le_bytes());
        out.extend_from_slice(&record.crc.to_le_bytes());
        if size64 {
            out.extend_from_slice(&0xFFFF_FFFFu32.to_le_bytes());
            out.extend_from_slice(&0xFFFF_FFFFu32.to_le_bytes());
        } else {
            // Guarded by `size64`: both values fit u32 here.
            out.extend_from_slice(&u32::try_from(record.comp_len).expect("u32").to_le_bytes());
            out.extend_from_slice(&u32::try_from(record.uncomp_len).expect("u32").to_le_bytes());
        }
        out.extend_from_slice(&name_len.to_le_bytes());
        if size64 {
            out.extend_from_slice(&20u16.to_le_bytes());
        } else {
            out.extend_from_slice(&0u16.to_le_bytes());
        }
        out.extend_from_slice(record.name);
        if size64 {
            out.extend_from_slice(&1u16.to_le_bytes());
            out.extend_from_slice(&16u16.to_le_bytes());
            out.extend_from_slice(&record.uncomp_len.to_le_bytes());
            out.extend_from_slice(&record.comp_len.to_le_bytes());
        }
        out.extend_from_slice(record.comp);
    }

    let central_start = out.len() as u64;
    for record in records {
        let size64 = record.uncomp_len > ZIP64_LIMIT || record.comp_len > ZIP64_LIMIT;
        let off64 = record.header_offset > ZIP64_LIMIT;
        let version = if size64 || off64 {
            VERSION_ZIP64
        } else {
            VERSION_PLAIN
        };
        let name_len = u16::try_from(record.name.len()).expect("zip entry name exceeds u16");
        out.extend_from_slice(&0x0201_4b50u32.to_le_bytes());
        out.extend_from_slice(&version.to_le_bytes());
        out.extend_from_slice(&version.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&8u16.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&0x2Bu16.to_le_bytes());
        out.extend_from_slice(&record.crc.to_le_bytes());
        if size64 {
            out.extend_from_slice(&0xFFFF_FFFFu32.to_le_bytes());
            out.extend_from_slice(&0xFFFF_FFFFu32.to_le_bytes());
        } else {
            // Guarded by `size64`: both values fit u32 here.
            out.extend_from_slice(&u32::try_from(record.comp_len).expect("u32").to_le_bytes());
            out.extend_from_slice(&u32::try_from(record.uncomp_len).expect("u32").to_le_bytes());
        }
        out.extend_from_slice(&name_len.to_le_bytes());
        // Central extras carry only the overflowing fields, sizes first.
        let mut extra_fields: u64 = 0;
        if size64 {
            extra_fields += 2;
        }
        if off64 {
            extra_fields += 1;
        }
        let central_extra_len = if size64 || off64 {
            4 + 8 * extra_fields
        } else {
            0
        };
        out.extend_from_slice(
            &u16::try_from(central_extra_len)
                .expect("zip64 extra fits u16")
                .to_le_bytes(),
        );
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());
        if off64 {
            out.extend_from_slice(&0xFFFF_FFFFu32.to_le_bytes());
        } else {
            // Guarded by `off64`: the offset fits u32 here.
            out.extend_from_slice(
                &u32::try_from(record.header_offset)
                    .expect("u32")
                    .to_le_bytes(),
            );
        }
        out.extend_from_slice(record.name);
        if size64 || off64 {
            out.extend_from_slice(&1u16.to_le_bytes());
            out.extend_from_slice(
                &u16::try_from(8 * extra_fields)
                    .expect("zip64 extra fits u16")
                    .to_le_bytes(),
            );
            if size64 {
                out.extend_from_slice(&record.uncomp_len.to_le_bytes());
                out.extend_from_slice(&record.comp_len.to_le_bytes());
            }
            if off64 {
                out.extend_from_slice(&record.header_offset.to_le_bytes());
            }
        }
    }
    let central_len = out.len() as u64 - central_start;

    let count = records.len() as u64;
    if count > 0xFFFF || central_len > ZIP64_LIMIT || central_start > ZIP64_LIMIT {
        // Zip64 end-of-central-directory record + locator, ahead of the
        // plain EOCD (CPython `_write_end_record` field order).
        let record_at = out.len() as u64;
        out.extend_from_slice(&0x0606_4b50u32.to_le_bytes());
        out.extend_from_slice(&44u64.to_le_bytes());
        out.extend_from_slice(&VERSION_ZIP64.to_le_bytes());
        out.extend_from_slice(&VERSION_ZIP64.to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend_from_slice(&count.to_le_bytes());
        out.extend_from_slice(&count.to_le_bytes());
        out.extend_from_slice(&central_len.to_le_bytes());
        out.extend_from_slice(&central_start.to_le_bytes());
        out.extend_from_slice(&0x0706_4b50u32.to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend_from_slice(&record_at.to_le_bytes());
        out.extend_from_slice(&1u32.to_le_bytes());
    }
    out.extend_from_slice(&0x0605_4b50u32.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(
        &u16::try_from(count.min(0xFFFF))
            .expect("clamped")
            .to_le_bytes(),
    );
    out.extend_from_slice(
        &u16::try_from(count.min(0xFFFF))
            .expect("clamped")
            .to_le_bytes(),
    );
    out.extend_from_slice(
        &u32::try_from(central_len.min(0xFFFF_FFFF))
            .expect("clamped")
            .to_le_bytes(),
    );
    out.extend_from_slice(
        &u32::try_from(central_start.min(0xFFFF_FFFF))
            .expect("clamped")
            .to_le_bytes(),
    );
    out.extend_from_slice(&0u16.to_le_bytes());
    out
}

/// S3 client branch (`upload_to_s3:49-97`, `delete_old_s3_link:28-42`):
/// MinIO when `USE_MINIO`, else the explicit endpoint URL when set, else
/// the configured region.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum S3Branch {
    Minio,
    EndpointUrl,
    Region,
}

pub fn select_s3_branch(use_minio: bool, endpoint_url: &str) -> S3Branch {
    if use_minio {
        S3Branch::Minio
    } else if !endpoint_url.is_empty() {
        S3Branch::EndpointUrl
    } else {
        S3Branch::Region
    }
}

/// `ExtraArgs` for the upload: MinIO sets `ACL: public-read` plus
/// `ContentType: application/zip` (`:57-62`); the standard branch sets
/// only the content type (`:100-105`).
pub fn upload_extra_args(branch: S3Branch) -> Vec<(&'static str, &'static str)> {
    match branch {
        S3Branch::Minio => vec![("ACL", "public-read"), ("ContentType", "application/zip")],
        S3Branch::EndpointUrl | S3Branch::Region => {
            vec![("ContentType", "application/zip")]
        }
    }
}

/// MinIO presign endpoint (`:65-73`):
/// `{protocol}//{custom_domain minus "/uploads"}/`.
pub fn minio_presign_endpoint(protocol: &str, custom_domain: &str) -> String {
    format!("{protocol}//{}/", custom_domain.replace("/uploads", ""))
}

/// Upload outcome on the exporter row (`:114-124`): a truthy presigned
/// URL saves url + `completed` + key (`update_fields status,url,key`);
/// a falsy one saves `failed` (same fields).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UploadRowUpdate {
    pub status: &'static str,
    pub url: Option<String>,
    pub key: Option<String>,
}

pub fn resolve_upload_update(presigned_url: Option<&str>, file_name: &str) -> UploadRowUpdate {
    match presigned_url {
        Some(url) if !url.is_empty() => UploadRowUpdate {
            status: "completed",
            url: Some(url.to_owned()),
            key: Some(file_name.to_owned()),
        },
        _ => UploadRowUpdate {
            status: "failed",
            url: None,
            key: None,
        },
    }
}

/// `delete_old_s3_link:25-27` expiry scan SQL. `cutoff_literal` is the
/// Django-rendered `now() - 8d` timestamp (`2026-09-20 06:00:00+00:00`
/// for the frozen fixture instant); the predicate is
/// `url IS NOT NULL AND created_at <= now()-8d`, projected as
/// `.values_list("key", "id")`, newest first.
pub fn delete_old_s3_link_sql(cutoff_literal: &str) -> String {
    format!(
        "SELECT \"exporters\".\"key\", \"exporters\".\"id\" FROM \"exporters\" \
         WHERE (\"exporters\".\"deleted_at\" IS NULL AND \"exporters\".\"url\" IS NOT NULL \
         AND \"exporters\".\"created_at\" <= {cutoff_literal}) \
         ORDER BY \"exporters\".\"created_at\" DESC"
    )
}

/// One expired row's plan (`:45-53`, BUG-3 kept): `delete_object` runs
/// only when the key is truthy, while `url` is cleared unconditionally.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExpiryAction {
    pub exporter_id: String,
    pub delete_object: bool,
    pub key: Option<String>,
}

pub fn plan_expiry_deletes(rows: &[(Option<String>, String)]) -> Vec<ExpiryAction> {
    rows.iter()
        .map(|(key, id)| {
            let delete_object = key.as_deref().map(|k| !k.is_empty()).unwrap_or(false);
            ExpiryAction {
                exporter_id: id.clone(),
                delete_object,
                key: key.clone(),
            }
        })
        .collect()
}

/// `send_export_email:70-77` SMTP connection inputs. `port` mirrors
/// `int(EMAIL_PORT)`; a non-numeric port fails the task like Python's
/// `ValueError`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SmtpConnection {
    pub host: String,
    pub port: u16,
    pub username: String,
    pub password: String,
    pub use_tls: bool,
    pub use_ssl: bool,
    pub from_email: String,
}

pub fn smtp_connection(
    host: &str,
    port_raw: &str,
    username: &str,
    password: &str,
    tls_raw: &str,
    ssl_raw: &str,
    from_email: &str,
) -> Result<SmtpConnection, String> {
    use pidash_types::tasks_cleanup::exports_dto::email_flag_is_one;
    let port: u16 = port_raw
        .parse()
        .map_err(|_| format!("invalid EMAIL_PORT: {port_raw}"))?;
    Ok(SmtpConnection {
        host: host.to_owned(),
        port,
        username: username.to_owned(),
        password: password.to_owned(),
        use_tls: email_flag_is_one(tls_raw),
        use_ssl: email_flag_is_one(ssl_raw),
        from_email: from_email.to_owned(),
    })
}

/// Build the `send_export_email:52-88` payload: `csv_buffer.seek(0)`
/// before read (the byte slice is already whole), template rendered
/// with `{}`, attached as `{slug}-analytics.csv`, sent with
/// `fail_silently=False`.
pub fn build_export_email(
    to: &str,
    slug: &str,
    csv_text: &str,
    plain_text_body: &str,
    tls_raw: &str,
    ssl_raw: &str,
) -> ExportEmail {
    ExportEmail::new(
        to,
        slug,
        plain_text_body,
        csv_text.as_bytes(),
        tls_raw,
        ssl_raw,
    )
}

/// Structural spec of one detail queryset (`get_*_details:91-177`): the
/// `DISTINCT ON` + `ORDER BY` key, the `.values()` key list and the
/// extra scope guards. Full SQL text stays with the db layer; this pins
/// the semantics the SQL must implement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DetailSpec {
    pub distinct_on: &'static str,
    pub order_by: &'static str,
    pub value_keys: Vec<&'static str>,
    /// Extra scope beyond the `Issue.issue_objects` /
    /// `Issue.objects` manager (soft-delete, triage, archived, draft).
    pub guards: Vec<&'static str>,
}

pub fn detail_spec(axis: &str) -> Option<DetailSpec> {
    use pidash_types::tasks_cleanup::exports_dto::{
        ASSIGNEE_ID, CYCLE_ID, LABEL_ID, MODULE_ID, STATE_ID,
    };
    match axis {
        s if s == ASSIGNEE_ID => Some(DetailSpec {
            // `:91-125`: avatar OR avatar_asset non-null; asset wins in
            // the `assignees__avatar_url` CASE, avatar is the fallback.
            distinct_on: "assignees__id",
            order_by: "assignees__id",
            value_keys: vec![
                "assignees__avatar_url",
                "assignees__display_name",
                "assignees__first_name",
                "assignees__last_name",
                "assignees__id",
            ],
            guards: vec!["assignees__avatar IS NOT NULL OR assignees__avatar_asset IS NOT NULL"],
        }),
        // `:128-140`: plain manager (no issue_objects scope), join-guard
        // column is `label_issue.deleted_at`.
        s if s == LABEL_ID => Some(DetailSpec {
            distinct_on: "labels__id",
            order_by: "labels__id",
            value_keys: vec!["labels__id", "labels__color", "labels__name"],
            guards: vec!["labels__id IS NOT NULL", "label_issue__deleted_at IS NULL"],
        }),
        s if s == STATE_ID => Some(DetailSpec {
            distinct_on: "state_id",
            order_by: "state_id",
            value_keys: vec!["state_id", "state__name", "state__color"],
            guards: vec![],
        }),
        s if s == MODULE_ID => Some(DetailSpec {
            distinct_on: "issue_module__module_id",
            order_by: "issue_module__module_id",
            value_keys: vec!["issue_module__module_id", "issue_module__module__name"],
            guards: vec![
                "issue_module__module_id IS NOT NULL",
                "issue_module__deleted_at IS NULL",
            ],
        }),
        s if s == CYCLE_ID => Some(DetailSpec {
            distinct_on: "issue_cycle__cycle_id",
            order_by: "issue_cycle__cycle_id",
            value_keys: vec!["issue_cycle__cycle_id", "issue_cycle__cycle__name"],
            guards: vec![
                "issue_cycle__cycle_id IS NOT NULL",
                "issue_cycle__deleted_at IS NULL",
            ],
        }),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pidash_types::tasks_cleanup::exports_dto::{
        ASSIGNEE_ID, CYCLE_ID, EXPORT_EMAIL_SUBJECT, EXPORT_EMAIL_TEMPLATE, LABEL_ID, MODULE_ID,
        STATE_ID,
    };

    fn row(cells: &[&str]) -> Vec<CsvCell> {
        cells.iter().map(|c| CsvCell::Text(c.to_string())).collect()
    }

    fn detail(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn csv_basic_golden() {
        // Fixture `analytic.csv_basic`.
        let out = write_csv_rows(&[row(&["A", "B"]), row(&["1", "2"])]);
        assert_eq!(out, "\"A\",\"B\"\r\n\"1\",\"2\"\r\n");
    }

    #[test]
    fn csv_sanitize_golden() {
        // Fixture `analytic.csv_sanitize_prefix_quote` byte for byte:
        // trigger-leading cells gain the `'` prefix, the rest pass
        // through untouched.
        let out = write_csv_rows(&[
            row(&["=cmd", "+x"]),
            row(&["@a", "-b"]),
            row(&["ok", "fine"]),
        ]);
        assert_eq!(
            out,
            "\"'=cmd\",\"'+x\"\r\n\"'@a\",\"'-b\"\r\n\"ok\",\"fine\"\r\n"
        );
    }

    #[test]
    fn csv_empty_rows_yield_empty() {
        assert_eq!(write_csv_rows(&[]), "");
    }

    #[test]
    fn csv_sanitize_applies_to_strings_only() {
        // `sanitize_csv_value` is `isinstance(value, str)`-gated: a
        // Python `int(-5)` writes `"-5"` with no `'` prefix, while the
        // string `"-5"` is prefixed.
        let out = write_csv_rows(&[vec![
            CsvCell::Int(-5),
            CsvCell::Float(-2.5),
            CsvCell::Text("-5".into()),
            CsvCell::Bool(true),
            CsvCell::Empty,
        ]]);
        assert_eq!(out, "\"-5\",\"-2.5\",\"'-5\",\"True\",\"\"\r\n");
    }

    #[test]
    fn sanitize_triggers_match_owasp_set() {
        for trigger in ["=cmd", "+x", "-b", "@a", "\ttab", "\rlf", "\nlf"] {
            assert_eq!(sanitize_csv_value(trigger), format!("'{trigger}"));
        }
        assert_eq!(sanitize_csv_value("ok"), "ok");
        assert_eq!(sanitize_csv_value(""), "");
    }

    #[test]
    fn segmented_state_golden() {
        // Fixture `analytic.segmented_state_golden`: x_axis `state_id`
        // falls back to "X-Axis"; the missing Done/seg-b cell is the
        // STRING "0".
        let dist = vec![
            (
                "Todo".to_owned(),
                vec![
                    SegmentPoint {
                        segment: "seg-a".into(),
                        value: Some(Numeric::Int(2)),
                    },
                    SegmentPoint {
                        segment: "seg-b".into(),
                        value: Some(Numeric::Int(3)),
                    },
                ],
            ),
            (
                "Done".to_owned(),
                vec![SegmentPoint {
                    segment: "seg-a".into(),
                    value: Some(Numeric::Int(1)),
                }],
            ),
        ];
        let rows = generate_segmented_rows(
            &dist,
            STATE_ID,
            "issue_count",
            "seg",
            &[],
            &[],
            &[],
            &[],
            &[],
        );
        let text = write_csv_rows(&rows);
        assert_eq!(
            text,
            "\"X-Axis\",\"Issue Count\",\"seg-a\",\"seg-b\"\r\n\
             \"Todo\",\"5\",\"2\",\"3\"\r\n\
             \"Done\",\"1\",\"1\",\"0\"\r\n"
        );
    }

    #[test]
    fn segmented_null_value_renders_empty_not_zero() {
        // `next((x.get(key) ...), "0")`: the `"0"` default fires only
        // when no point carries the segment. A point with a `None`
        // value (e.g. an all-null `Sum` estimate) yields `None`, which
        // `csv.writer` writes as `""` — while the total still skips it.
        let dist = vec![(
            "Todo".to_owned(),
            vec![
                SegmentPoint {
                    segment: "seg-a".into(),
                    value: Some(Numeric::Int(2)),
                },
                SegmentPoint {
                    segment: "seg-b".into(),
                    value: None,
                },
            ],
        )];
        let rows = generate_segmented_rows(
            &dist,
            STATE_ID,
            "issue_count",
            "seg",
            &[],
            &[],
            &[],
            &[],
            &[],
        );
        let text = write_csv_rows(&rows);
        assert_eq!(
            text,
            "\"X-Axis\",\"Issue Count\",\"seg-a\",\"seg-b\"\r\n\
             \"Todo\",\"2\",\"2\",\"\"\r\n"
        );
    }

    #[test]
    fn segmented_module_bug_kept() {
        // Fixture `analytic.segmented_module_bug`: MODULE segment
        // headers resolve against label_details and stay raw.
        let dist = vec![(
            "m-1".to_owned(),
            vec![SegmentPoint {
                segment: "s-x".into(),
                value: Some(Numeric::Int(1)),
            }],
        )];
        let labels = vec![detail(&[(LABEL_ID, "l-1"), ("labels__name", "Bug")])];
        let rows = generate_segmented_rows(
            &dist,
            MODULE_ID,
            "issue_count",
            MODULE_ID,
            &[],
            &labels,
            &[],
            &[],
            &[],
        );
        let text = write_csv_rows(&rows);
        assert_eq!(
            text,
            "\"Module\",\"Issue Count\",\"s-x\"\r\n\"m-1\",\"1\",\"1\"\r\n"
        );
    }

    #[test]
    fn segmented_resolves_known_names() {
        let dist = vec![(
            "u-1".to_owned(),
            vec![SegmentPoint {
                segment: "u-2".into(),
                value: Some(Numeric::Int(4)),
            }],
        )];
        let users = vec![
            detail(&[
                (ASSIGNEE_ID, "u-1"),
                ("assignees__first_name", "Ada"),
                ("assignees__last_name", "L"),
            ]),
            detail(&[
                (ASSIGNEE_ID, "u-2"),
                ("assignees__first_name", "Bo"),
                ("assignees__last_name", "K"),
            ]),
        ];
        let rows = generate_segmented_rows(
            &dist,
            ASSIGNEE_ID,
            "issue_count",
            ASSIGNEE_ID,
            &users,
            &[],
            &[],
            &[],
            &[],
        );
        let text = write_csv_rows(&rows);
        assert_eq!(
            text,
            "\"Assignee Name\",\"Issue Count\",\"Bo K\"\r\n\"Ada L\",\"4\",\"4\"\r\n"
        );
    }

    #[test]
    fn nonsegmented_assignee_golden() {
        // Fixture `analytic.nonsegmented_assignee_golden`: only the
        // FIRST distribution element is read; unresolved ids stay raw.
        let dist = vec![
            (
                "u-1".to_owned(),
                FlatPoint {
                    count: Some(Numeric::Int(4)),
                    estimate: None,
                },
            ),
            (
                "u-9".to_owned(),
                FlatPoint {
                    count: Some(Numeric::Int(1)),
                    estimate: None,
                },
            ),
        ];
        let users = vec![detail(&[
            (ASSIGNEE_ID, "u-1"),
            ("assignees__first_name", "Ada"),
            ("assignees__last_name", "L"),
        ])];
        let rows = generate_non_segmented_rows(
            &dist,
            ASSIGNEE_ID,
            "issue_count",
            &users,
            &[],
            &[],
            &[],
            &[],
        );
        let text = write_csv_rows(&rows);
        assert_eq!(
            text,
            "\"Assignee Name\",\"Issue Count\"\r\n\"Ada L\",\"4\"\r\n\"u-9\",\"1\"\r\n"
        );
    }

    #[test]
    fn nonsegmented_uses_estimate_key() {
        // `priority` is a mapped header; `state_id` would fall back to
        // "X-Axis" like the segmented golden.
        let dist = vec![(
            "high".to_owned(),
            FlatPoint {
                count: Some(Numeric::Int(9)),
                estimate: Some(Numeric::Float(2.5)),
            },
        )];
        let rows =
            generate_non_segmented_rows(&dist, "priority", "estimate", &[], &[], &[], &[], &[]);
        let text = write_csv_rows(&rows);
        assert_eq!(text, "\"Priority\",\"Estimate\"\r\n\"high\",\"2.5\"\r\n");
    }

    /// Minimal ZIP reader for tests: local headers + raw deflate.
    fn read_zip(data: &[u8]) -> Vec<(String, Vec<u8>)> {
        use std::io::Read;
        let mut entries = Vec::new();
        let mut pos = 0;
        while pos + 30 <= data.len() {
            let sig = u32::from_le_bytes(data[pos..pos + 4].try_into().unwrap());
            if sig != 0x0403_4b50 {
                break;
            }
            let method = u16::from_le_bytes(data[pos + 8..pos + 10].try_into().unwrap());
            let crc = u32::from_le_bytes(data[pos + 14..pos + 18].try_into().unwrap());
            let comp_len =
                u32::from_le_bytes(data[pos + 18..pos + 22].try_into().unwrap()) as usize;
            let name_len =
                u16::from_le_bytes(data[pos + 26..pos + 28].try_into().unwrap()) as usize;
            let extra_len =
                u16::from_le_bytes(data[pos + 28..pos + 30].try_into().unwrap()) as usize;
            let name_start = pos + 30;
            let name = String::from_utf8(data[name_start..name_start + name_len].to_vec()).unwrap();
            let body_start = name_start + name_len + extra_len;
            let body = &data[body_start..body_start + comp_len];
            assert_eq!(method, 8, "entries must be ZIP_DEFLATED");
            let mut decoder = flate2::read::DeflateDecoder::new(body);
            let mut content = Vec::new();
            decoder.read_to_end(&mut content).unwrap();
            assert_eq!(crc32fast::hash(&content), crc);
            entries.push((name, content));
            pos = body_start + comp_len;
        }
        entries
    }

    #[test]
    fn zip_round_trips_deflated_entries() {
        // Fixture `create_zip_file`: namelist + contents + rewind.
        let json = "{\"a\": 1}";
        let csv = "h1,h2\nv1,v2\n";
        let data = build_zip(&[
            ZipEntry {
                name: "issues.json",
                content: json.as_bytes(),
            },
            ZipEntry {
                name: "issues.csv",
                content: csv.as_bytes(),
            },
        ]);
        let entries = read_zip(&data);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].0, "issues.json");
        assert_eq!(entries[0].1, json.as_bytes());
        assert_eq!(entries[1].0, "issues.csv");
        assert_eq!(entries[1].1, csv.as_bytes());
    }

    #[test]
    fn zip_empty_and_duplicates_match_python() {
        assert_eq!(read_zip(&build_zip(&[])), vec![]);
        let data = build_zip(&[
            ZipEntry {
                name: "a",
                content: b"1",
            },
            ZipEntry {
                name: "a",
                content: b"2",
            },
        ]);
        let entries = read_zip(&data);
        assert_eq!(
            entries,
            vec![
                ("a".to_owned(), b"1".to_vec()),
                ("a".to_owned(), b"2".to_vec())
            ]
        );
    }

    /// End-of-central-directory fields of a `build_zip` archive (our
    /// writer emits no archive comment, so EOCD is the last 22 bytes).
    fn read_eocd(data: &[u8]) -> (u16, u16, u32, u32) {
        let eocd = &data[data.len() - 22..];
        assert_eq!(
            u32::from_le_bytes(eocd[0..4].try_into().unwrap()),
            0x0605_4b50
        );
        (
            u16::from_le_bytes(eocd[8..10].try_into().unwrap()),
            u16::from_le_bytes(eocd[10..12].try_into().unwrap()),
            u32::from_le_bytes(eocd[12..16].try_into().unwrap()),
            u32::from_le_bytes(eocd[16..20].try_into().unwrap()),
        )
    }

    fn has_sig(data: &[u8], sig: u32) -> bool {
        let sig = sig.to_le_bytes();
        data.windows(4).any(|w| w == sig)
    }

    #[test]
    fn zip_count_past_u16_uses_zip64_eocd() {
        // PIDASHCONV-1700: counts past 0xFFFF ride the zip64
        // end-of-central-directory record (CPython 3.12 `zipfile`:
        // `centDirCount > ZIP_FILECOUNT_LIMIT`), not a truncated u16.
        let names: Vec<String> = (0..70_000).map(|i| format!("f-{i:06}")).collect();
        let entries: Vec<ZipEntry<'_>> = names
            .iter()
            .map(|nm| ZipEntry {
                name: nm.as_str(),
                content: b"x",
            })
            .collect();
        let data = build_zip(&entries);
        assert!(has_sig(&data, 0x0606_4b50), "zip64 EOCD record missing");
        assert!(has_sig(&data, 0x0706_4b50), "zip64 EOCD locator missing");
        // Plain EOCD counts clamp to the marker; the 20-byte locator sits
        // directly before it and points at the zip64 record, whose total
        // entry count is the true u64 value.
        assert_eq!(read_eocd(&data).0, 0xFFFF);
        assert_eq!(read_eocd(&data).1, 0xFFFF);
        let locator = &data[data.len() - 42..data.len() - 22];
        assert_eq!(
            u32::from_le_bytes(locator[0..4].try_into().unwrap()),
            0x0706_4b50
        );
        let record_at = u64::from_le_bytes(locator[8..16].try_into().unwrap()) as usize;
        let record = &data[record_at..];
        assert_eq!(
            u32::from_le_bytes(record[0..4].try_into().unwrap()),
            0x0606_4b50
        );
        assert_eq!(
            u64::from_le_bytes(record[32..40].try_into().unwrap()),
            70_000
        );
        // Count-only overflow leaves every entry header untouched: local
        // extras stay empty and central versions stay 20.
        assert_eq!(u16::from_le_bytes(data[28..30].try_into().unwrap()), 0);
        let central_at = read_eocd(&data).3 as usize;
        assert_eq!(
            u32::from_le_bytes(data[central_at..central_at + 4].try_into().unwrap()),
            0x0201_4b50
        );
        assert_eq!(
            u16::from_le_bytes(data[central_at + 4..central_at + 6].try_into().unwrap()),
            20
        );
        assert_eq!(
            u16::from_le_bytes(data[central_at + 6..central_at + 8].try_into().unwrap()),
            20
        );
        assert_eq!(read_zip(&data).len(), 70_000);
    }

    #[test]
    fn zip_count_at_u16_limit_has_no_zip64() {
        // Boundary: exactly 0xFFFF entries still fit the EOCD u16 fields
        // (CPython uses `>`, not `>=`), so no zip64 records are written.
        let names: Vec<String> = (0..0xFFFF).map(|i| format!("f-{i:06}")).collect();
        let entries: Vec<ZipEntry<'_>> = names
            .iter()
            .map(|nm| ZipEntry {
                name: nm.as_str(),
                content: b"x",
            })
            .collect();
        let data = build_zip(&entries);
        assert!(!has_sig(&data, 0x0606_4b50));
        assert!(!has_sig(&data, 0x0706_4b50));
        let (this_disk, total, _, _) = read_eocd(&data);
        assert_eq!((this_disk, total), (0xFFFF, 0xFFFF));
        assert_eq!(read_zip(&data).len(), 0xFFFF);
    }

    #[test]
    fn zip_size_past_limit_uses_zip64_extras() {
        // Synthetic 4 GiB declared size over a 1-byte payload: the local
        // header carries version 45, 0xFFFFFFFF placeholders and the
        // paired u64 sizes; the central header matches. Small central
        // directory, so no zip64 EOCD (as in the Django 4 GiB baseline).
        let data = assemble_zip(&[ZipRecord {
            name: b"big.bin",
            crc: 0x1234_5678,
            uncomp_len: 0x1_0000_0000,
            comp: b"x",
            comp_len: 3,
            header_offset: 0,
        }]);
        assert_eq!(
            u32::from_le_bytes(data[0..4].try_into().unwrap()),
            0x0403_4b50
        );
        assert_eq!(u16::from_le_bytes(data[4..6].try_into().unwrap()), 45);
        assert_eq!(
            u32::from_le_bytes(data[18..22].try_into().unwrap()),
            0xFFFF_FFFF
        );
        assert_eq!(
            u32::from_le_bytes(data[22..26].try_into().unwrap()),
            0xFFFF_FFFF
        );
        let name_len = u16::from_le_bytes(data[26..28].try_into().unwrap()) as usize;
        assert_eq!(u16::from_le_bytes(data[28..30].try_into().unwrap()), 20);
        let extra = &data[30 + name_len..30 + name_len + 20];
        assert_eq!(u16::from_le_bytes(extra[0..2].try_into().unwrap()), 1);
        assert_eq!(u16::from_le_bytes(extra[2..4].try_into().unwrap()), 16);
        assert_eq!(
            u64::from_le_bytes(extra[4..12].try_into().unwrap()),
            0x1_0000_0000
        );
        assert_eq!(u64::from_le_bytes(extra[12..20].try_into().unwrap()), 3);
        let central_at = 30 + name_len + 20 + 1;
        assert_eq!(
            u32::from_le_bytes(data[central_at..central_at + 4].try_into().unwrap()),
            0x0201_4b50
        );
        assert_eq!(
            u16::from_le_bytes(data[central_at + 4..central_at + 6].try_into().unwrap()),
            45
        );
        assert_eq!(
            u16::from_le_bytes(data[central_at + 6..central_at + 8].try_into().unwrap()),
            45
        );
        assert_eq!(
            u32::from_le_bytes(data[central_at + 20..central_at + 24].try_into().unwrap()),
            0xFFFF_FFFF
        );
        assert_eq!(
            u32::from_le_bytes(data[central_at + 24..central_at + 28].try_into().unwrap()),
            0xFFFF_FFFF
        );
        assert_eq!(
            u16::from_le_bytes(data[central_at + 30..central_at + 32].try_into().unwrap()),
            20
        );
        assert_eq!(
            u32::from_le_bytes(data[central_at + 42..central_at + 46].try_into().unwrap()),
            0
        );
        let cextra = &data[central_at + 46 + name_len..central_at + 46 + name_len + 20];
        assert_eq!(u16::from_le_bytes(cextra[0..2].try_into().unwrap()), 1);
        assert_eq!(u16::from_le_bytes(cextra[2..4].try_into().unwrap()), 16);
        assert_eq!(
            u64::from_le_bytes(cextra[4..12].try_into().unwrap()),
            0x1_0000_0000
        );
        assert_eq!(u64::from_le_bytes(cextra[12..20].try_into().unwrap()), 3);
        assert!(!has_sig(&data, 0x0606_4b50));
        assert!(!has_sig(&data, 0x0706_4b50));
    }

    #[test]
    fn zip_offset_past_limit_rides_central_extra() {
        // Synthetic 4 GiB header offset with small sizes: the local header
        // stays plain (version 20, no extra — CPython never puts the
        // offset in local extras) while the central header carries version
        // 45, the offset placeholder and an 8-byte extra holding the
        // offset alone.
        let data = assemble_zip(&[ZipRecord {
            name: b"late.bin",
            crc: 0,
            uncomp_len: 1,
            comp: b"x",
            comp_len: 1,
            header_offset: 0x1_0000_0000,
        }]);
        assert_eq!(u16::from_le_bytes(data[4..6].try_into().unwrap()), 20);
        assert_eq!(u32::from_le_bytes(data[18..22].try_into().unwrap()), 1);
        assert_eq!(u32::from_le_bytes(data[22..26].try_into().unwrap()), 1);
        assert_eq!(u16::from_le_bytes(data[28..30].try_into().unwrap()), 0);
        let central_at = 30 + b"late.bin".len() + 1;
        assert_eq!(
            u16::from_le_bytes(data[central_at + 4..central_at + 6].try_into().unwrap()),
            45
        );
        assert_eq!(
            u16::from_le_bytes(data[central_at + 6..central_at + 8].try_into().unwrap()),
            45
        );
        assert_eq!(
            u32::from_le_bytes(data[central_at + 20..central_at + 24].try_into().unwrap()),
            1
        );
        assert_eq!(
            u16::from_le_bytes(data[central_at + 30..central_at + 32].try_into().unwrap()),
            12
        );
        assert_eq!(
            u32::from_le_bytes(data[central_at + 42..central_at + 46].try_into().unwrap()),
            0xFFFF_FFFF
        );
        let cextra =
            &data[central_at + 46 + b"late.bin".len()..central_at + 46 + b"late.bin".len() + 12];
        assert_eq!(u16::from_le_bytes(cextra[0..2].try_into().unwrap()), 1);
        assert_eq!(u16::from_le_bytes(cextra[2..4].try_into().unwrap()), 8);
        assert_eq!(
            u64::from_le_bytes(cextra[4..12].try_into().unwrap()),
            0x1_0000_0000
        );
    }

    #[test]
    fn zip_size_at_limit_stays_plain() {
        // `>` semantics: exactly ZIP64_LIMIT fits the u32 fields, so no
        // extras and version 20 (mirrors CPython's comparisons).
        let data = assemble_zip(&[ZipRecord {
            name: b"edge.bin",
            crc: 0,
            uncomp_len: ZIP64_LIMIT,
            comp: b"x",
            comp_len: ZIP64_LIMIT,
            header_offset: ZIP64_LIMIT,
        }]);
        assert_eq!(u16::from_le_bytes(data[4..6].try_into().unwrap()), 20);
        assert_eq!(
            u32::from_le_bytes(data[18..22].try_into().unwrap()),
            0x7FFF_FFFF
        );
        assert_eq!(
            u32::from_le_bytes(data[22..26].try_into().unwrap()),
            0x7FFF_FFFF
        );
        assert_eq!(u16::from_le_bytes(data[28..30].try_into().unwrap()), 0);
        assert!(!has_sig(&data, 0x0606_4b50));
        assert!(!has_sig(&data, 0x0706_4b50));
    }

    #[test]
    fn zip_under_limit_has_no_zip64_records() {
        // PIDASHCONV-1700 second criterion at writer level: small archives
        // stay plain — no zip64 EOCD, no extras, version 20 throughout.
        let data = build_zip(&[
            ZipEntry {
                name: "issues.json",
                content: br#"{"a": 1}"#,
            },
            ZipEntry {
                name: "issues.csv",
                content: b"h1,h2\nv1,v2\n",
            },
        ]);
        assert!(!has_sig(&data, 0x0606_4b50));
        assert!(!has_sig(&data, 0x0706_4b50));
        let (this_disk, total, central_len, central_start) = read_eocd(&data);
        assert_eq!((this_disk, total), (2, 2));
        let mut pos = 0;
        for _ in 0..2 {
            assert_eq!(
                u32::from_le_bytes(data[pos..pos + 4].try_into().unwrap()),
                0x0403_4b50
            );
            assert_eq!(
                u16::from_le_bytes(data[pos + 4..pos + 6].try_into().unwrap()),
                20
            );
            let comp_len =
                u32::from_le_bytes(data[pos + 18..pos + 22].try_into().unwrap()) as usize;
            let name_len =
                u16::from_le_bytes(data[pos + 26..pos + 28].try_into().unwrap()) as usize;
            assert_eq!(
                u16::from_le_bytes(data[pos + 28..pos + 30].try_into().unwrap()),
                0
            );
            pos += 30 + name_len + comp_len;
        }
        assert_eq!(pos as u64, u64::from(central_start));
        let mut cpos = usize::try_from(central_start).expect("central dir fits usize");
        for _ in 0..2 {
            assert_eq!(
                u32::from_le_bytes(data[cpos..cpos + 4].try_into().unwrap()),
                0x0201_4b50
            );
            assert_eq!(
                u16::from_le_bytes(data[cpos + 4..cpos + 6].try_into().unwrap()),
                20
            );
            assert_eq!(
                u16::from_le_bytes(data[cpos + 6..cpos + 8].try_into().unwrap()),
                20
            );
            let name_len =
                u16::from_le_bytes(data[cpos + 28..cpos + 30].try_into().unwrap()) as usize;
            assert_eq!(
                u16::from_le_bytes(data[cpos + 30..cpos + 32].try_into().unwrap()),
                0
            );
            cpos += 46 + name_len;
        }
        assert_eq!(
            cpos as u64 - u64::from(central_start),
            u64::from(central_len)
        );
    }

    #[test]
    fn expiry_sql_matches_fixture() {
        // Fixture `delete_old_s3_link.filter_sql` byte for byte.
        let sql = delete_old_s3_link_sql("2026-09-20 06:00:00+00:00");
        assert_eq!(
            sql,
            "SELECT \"exporters\".\"key\", \"exporters\".\"id\" FROM \"exporters\" \
             WHERE (\"exporters\".\"deleted_at\" IS NULL AND \"exporters\".\"url\" IS NOT NULL \
             AND \"exporters\".\"created_at\" <= 2026-09-20 06:00:00+00:00) \
             ORDER BY \"exporters\".\"created_at\" DESC"
        );
    }

    #[test]
    fn expiry_plan_deletes_iff_truthy_clears_always() {
        let plan = plan_expiry_deletes(&[
            (Some("k/1.zip".into()), "id-1".into()),
            (Some("".into()), "id-2".into()),
            (None, "id-3".into()),
        ]);
        assert!(plan[0].delete_object);
        assert!(!plan[1].delete_object);
        assert!(!plan[2].delete_object);
        // Every row is planned (url cleared unconditionally).
        assert_eq!(plan.len(), 3);
        assert_eq!(plan[0].exporter_id, "id-1");
    }

    #[test]
    fn s3_branches_and_args_match_python() {
        assert_eq!(select_s3_branch(true, ""), S3Branch::Minio);
        assert_eq!(
            select_s3_branch(false, "https://s3.example"),
            S3Branch::EndpointUrl
        );
        assert_eq!(select_s3_branch(false, ""), S3Branch::Region);
        assert_eq!(
            upload_extra_args(S3Branch::Minio),
            vec![("ACL", "public-read"), ("ContentType", "application/zip")]
        );
        assert_eq!(
            upload_extra_args(S3Branch::Region),
            vec![("ContentType", "application/zip")]
        );
        assert_eq!(
            minio_presign_endpoint("https:", "cdn.example/uploads"),
            "https://cdn.example/"
        );
    }

    #[test]
    fn upload_outcome_matches_row_lifecycle() {
        let done = resolve_upload_update(Some("https://cdn/x.zip"), "w/export-s-ab-2026-09-28.zip");
        assert_eq!(done.status, "completed");
        assert_eq!(done.url.as_deref(), Some("https://cdn/x.zip"));
        assert_eq!(done.key.as_deref(), Some("w/export-s-ab-2026-09-28.zip"));
        let failed = resolve_upload_update(None, "w/export-s-ab-2026-09-28.zip");
        assert_eq!(failed.status, "failed");
        assert_eq!(failed.url, None);
    }

    #[test]
    fn provider_validation_matches_exporter() {
        assert!(validate_provider("csv").is_ok());
        assert!(validate_provider("json").is_ok());
        assert!(validate_provider("xlsx").is_ok());
        // Byte-exact `ValueError` text (single-quoted `repr` list), saved
        // as the exporter row `reason` on the failure path.
        assert_eq!(
            validate_provider("pdf").unwrap_err(),
            "Unsupported format: pdf. Available: ['csv', 'json', 'xlsx']"
        );
    }

    #[test]
    fn detail_needs_fetch_only_axes_in_use() {
        let needs = detail_needs(Some(ASSIGNEE_ID), Some(STATE_ID));
        assert!(needs.assignee && needs.state);
        assert!(!needs.label && !needs.cycle && !needs.module);
        assert_eq!(
            detail_needs(None, None),
            DetailNeeds {
                assignee: false,
                label: false,
                state: false,
                cycle: false,
                module: false
            }
        );
        assert!(detail_needs(Some(CYCLE_ID), None).cycle);
    }

    #[test]
    fn orchestration_flags_match_task() {
        assert!(resolve_value_key_is_count("issue_count"));
        assert!(!resolve_value_key_is_count("estimate"));
        assert!(is_segmented(Some(STATE_ID)));
        assert!(!is_segmented(None));
        assert!(!is_segmented(Some("")));
    }

    #[test]
    fn detail_specs_carry_value_keys() {
        let assignee = detail_spec(ASSIGNEE_ID).unwrap();
        assert_eq!(assignee.distinct_on, "assignees__id");
        assert_eq!(assignee.value_keys.len(), 5);
        let label = detail_spec(LABEL_ID).unwrap();
        assert_eq!(
            label.value_keys,
            vec!["labels__id", "labels__color", "labels__name"]
        );
        assert!(detail_spec("nope").is_none());
    }

    #[test]
    fn export_email_payload_shape() {
        let mail = build_export_email("a@x.io", "ws", "\"H\"\r\n", "plain", "1", "0");
        assert_eq!(mail.to, "a@x.io");
        assert_eq!(mail.subject, EXPORT_EMAIL_SUBJECT);
        assert_eq!(mail.attachment_name, "ws-analytics.csv");
        assert_eq!(mail.attachment_bytes, b"\"H\"\r\n");
        assert!(mail.use_tls && !mail.use_ssl);
        assert_eq!(EXPORT_EMAIL_TEMPLATE, "emails/exports/analytics.html");
        let conn = smtp_connection("h", "587", "u", "p", "1", "1", "f@x.io").unwrap();
        assert_eq!(conn.port, 587);
        assert!(smtp_connection("h", "nope", "u", "p", "1", "0", "f").is_err());
    }
}
