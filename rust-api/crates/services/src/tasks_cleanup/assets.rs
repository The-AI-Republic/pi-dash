//! File-asset background tasks: pure domain logic (D-09, stage 5).
//!
//! Port of `apps/api/pi_dash/bgtasks/file_asset_task.py` (26 lines),
//! `apps/api/pi_dash/bgtasks/storage_metadata_task.py` (30 lines) and
//! `apps/api/pi_dash/bgtasks/copy_s3_object.py` (161 lines).
//!
//! Everything here is pure: environment parsing, the Celery entity-type
//! map, the `image-component` tag walk, S3 key building, the
//! `description_json or {}` fallback, and the SQL text the jobs layer
//! executes. Database access, S3 calls and the live-document conversion
//! live behind the jobs-layer traits so tests replay the fixture goldens
//! (`rust-api/fixtures/tasks_cleanup/assets.json`) with no database.
//!
//! # Ported semantics (translate, don't redesign)
//!
//! * The sweep never hard-deletes: `.delete()` on the default manager is
//!   `SoftDeletionQuerySet.delete` → `UPDATE file_assets SET deleted_at`
//!   scoped to `deleted_at IS NULL` (`db/mixins.py:49-53`).
//! * `DRAFT_ISSUE_ATTACHMENT` exists on the enum but is absent from the
//!   entity map, so it resolves to no column — kept as-is.
//! * `copy_assets` reads `original.attributes['name']` unguarded: a `None`
//!   attributes dict crashes the task, a missing `name` renders as the
//!   string `"None"` in the destination key — both kept as-is.
//! * `replace_asset_ids` applies pairs in order per tag, so a chain where
//!   one pair's new id is a later pair's old id rewrites twice — kept.
//! * `update_description` saves unconditionally, even with zero pairs, and
//!   the live conversion runs on the re-serialized HTML either way.
//! * `QuerySet.update(is_uploaded=True)` writes that column only; unlike
//!   `save()`, it does not bump `updated_at`. `save(update_fields=[...])`
//!   *does* bump `updated_at` via `auto_now` (`pre_save` in `_save_table`).

use serde_json::{Map, Value};

/// `os.environ` key for the sweep cutoff (`file_asset_task.py:24`).
pub const UNUPLOADED_ASSET_DELETE_DAYS_ENV: &str = "UNUPLOADED_ASSET_DELETE_DAYS";

/// Default when the env var is absent (`file_asset_task.py:24`).
pub const UNUPLOADED_ASSET_DELETE_DAYS_DEFAULT: i64 = 7;

/// Port of `int(os.environ.get("UNUPLOADED_ASSET_DELETE_DAYS", "7"))`.
///
/// Python `int()` strips surrounding whitespace, takes one optional sign
/// and allows underscores between digits; anything else raises `ValueError`,
/// which escapes the sweep (it has no `try`).
pub fn parse_delete_days(raw: &str) -> Result<i64, String> {
    let trimmed = raw.trim();
    let (sign, digits) = match trimmed.strip_prefix(['+', '-']) {
        Some(rest) => (trimmed.starts_with('-'), rest),
        None => (false, trimmed),
    };
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit() || b == b'_') {
        return Err(format!("invalid UNUPLOADED_ASSET_DELETE_DAYS: {raw:?}"));
    }
    // Python rejects leading/trailing/doubled underscores.
    if digits.starts_with('_')
        || digits.ends_with('_')
        || digits.contains("__")
        || digits.bytes().all(|b| b == b'_')
    {
        return Err(format!("invalid UNUPLOADED_ASSET_DELETE_DAYS: {raw:?}"));
    }
    let compact: String = digits.chars().filter(|c| *c != '_').collect();
    let mut value: i64 = compact
        .parse()
        .map_err(|_| format!("invalid UNUPLOADED_ASSET_DELETE_DAYS: {raw:?}"))?;
    if sign {
        value = -value;
    }
    Ok(value)
}

/// Resolve the sweep lookback in days: `None` (var absent) → the default.
pub fn sweep_lookback_days(env: Option<&str>) -> Result<i64, String> {
    match env {
        None => Ok(UNUPLOADED_ASSET_DELETE_DAYS_DEFAULT),
        Some(raw) => parse_delete_days(raw),
    }
}

/// The sweep statement (`file_asset_task.py:21-26`): stamp `deleted_at` on
/// unuploaded rows older than the cutoff. `$1` is now, `$2` the cutoff.
/// Predicate shape mirrors the fixture's recorded SQL byte for byte apart
/// from parameter placeholders.
pub const SWEEP_SQL: &str = "UPDATE \"file_assets\" SET \"deleted_at\" = $1 WHERE (\"file_assets\".\"deleted_at\" IS NULL AND \"file_assets\".\"created_at\" < $2 AND NOT \"file_assets\".\"is_uploaded\")";

/// Port of `get_entity_id_field` (`copy_s3_object.py:18-30`): the create
/// kwarg column for an asset's entity type. `None` is the `{}` fallback —
/// unknown types and `DRAFT_ISSUE_ATTACHMENT` set no column.
pub fn entity_id_field(entity_type: &str) -> Option<&'static str> {
    match entity_type {
        "WORKSPACE_LOGO" => Some("workspace_id"),
        "PROJECT_COVER" => Some("project_id"),
        "USER_AVATAR" => Some("user_id"),
        "USER_COVER" => Some("user_id"),
        "ISSUE_ATTACHMENT" => Some("issue_id"),
        "ISSUE_DESCRIPTION" => Some("issue_id"),
        "PAGE_DESCRIPTION" => Some("page_id"),
        "COMMENT_DESCRIPTION" => Some("comment_id"),
        "DRAFT_ISSUE_DESCRIPTION" => Some("draft_issue_id"),
        _ => None,
    }
}

/// One old → new asset id pair (`copy_s3_object.py:104-109`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssetIdPair {
    pub old_asset_id: String,
    pub new_asset_id: String,
}

/// An `<tag ...>` open-tag occurrence used by the tag walk.
struct TagHit {
    /// `(name, value)` attributes in source order, names lowercased.
    attrs: Vec<(String, String)>,
    /// Byte spans of each `src` attribute *value*, in source order.
    src_spans: Vec<(usize, usize)>,
}

fn is_tag_char(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b':' | b'.')
}

/// Decode the five predefined XML entities plus numeric character
/// references, mirroring what `tag.get("src")` returns: BeautifulSoup
/// decodes entities in attribute values (`html.unescape` semantics).
fn decode_entities(raw: &str) -> String {
    if !raw.contains('&') {
        return raw.to_string();
    }
    let mut out = String::with_capacity(raw.len());
    let bytes = raw.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b'&' {
            out.push(bytes[i] as char);
            i += 1;
            continue;
        }
        let rest = &raw[i..];
        let end = rest.find(';').map(|p| i + p);
        let decoded = end.and_then(|e| decode_entity(&raw[i + 1..e]));
        match (decoded, end) {
            (Some(ch), Some(e)) => {
                out.push(ch);
                i = e + 1;
            }
            _ => {
                out.push('&');
                i += 1;
            }
        }
    }
    out
}

fn decode_entity(name: &str) -> Option<char> {
    match name {
        "amp" => Some('&'),
        "lt" => Some('<'),
        "gt" => Some('>'),
        "quot" => Some('"'),
        "apos" => Some('\''),
        _ => {
            if let Some(hex) = name.strip_prefix("#x").or_else(|| name.strip_prefix("#X")) {
                u32::from_str_radix(hex, 16).ok().and_then(char::from_u32)
            } else if let Some(dec) = name.strip_prefix('#') {
                dec.parse::<u32>().ok().and_then(char::from_u32)
            } else {
                None
            }
        }
    }
}

/// Walk `html` for `<tag ...>` open tags, mirroring
/// `BeautifulSoup(html, "html.parser").find_all(tag)` for the callers'
/// purposes: tag and attribute names match case-insensitively (the parser
/// lowercases both), comments/decls/processing-instructions never match,
/// and `script`/`style` bodies are CDATA (their inner `<...>` is text).
/// A tag cut off at end of input still counts (the parser recovers it).
fn find_open_tags(html: &str, tag: &str) -> Vec<TagHit> {
    let bytes = html.as_bytes();
    let want = tag.to_ascii_lowercase();
    let mut hits = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b'<' || i + 1 >= bytes.len() {
            i += 1;
            continue;
        }
        let next = bytes[i + 1];
        // Comment, declaration/doctype, processing instruction, close tag.
        if next == b'!' {
            if html[i..].starts_with("<!--") {
                i = html[i..]
                    .find("-->")
                    .map(|p| i + p + 3)
                    .unwrap_or(bytes.len());
            } else {
                i = html[i..]
                    .find('>')
                    .map(|p| i + p + 1)
                    .unwrap_or(bytes.len());
            }
            continue;
        }
        if next == b'?' {
            i = html[i..]
                .find("?>")
                .map(|p| i + p + 2)
                .unwrap_or(bytes.len());
            continue;
        }
        if next == b'/' || !next.is_ascii_alphabetic() {
            i += 1;
            continue;
        }
        // Tag name.
        let mut j = i + 1;
        while j < bytes.len() && is_tag_char(bytes[j]) {
            j += 1;
        }
        let name = html[i + 1..j].to_ascii_lowercase();
        // Attributes.
        let mut attrs: Vec<(String, String)> = Vec::new();
        let mut src_spans: Vec<(usize, usize)> = Vec::new();
        let mut k = j;
        let mut closed = false;
        while k < bytes.len() {
            while k < bytes.len() && bytes[k].is_ascii_whitespace() {
                k += 1;
            }
            if k >= bytes.len() {
                break;
            }
            if bytes[k] == b'>' {
                k += 1;
                closed = true;
                break;
            }
            if bytes[k] == b'/' {
                // Self-closing slash: still a hit (`<image-component/>`
                // parses to an element). Skip it and keep scanning so a
                // following `>` closes the tag.
                k += 1;
                continue;
            }
            let start = k;
            while k < bytes.len()
                && !bytes[k].is_ascii_whitespace()
                && !matches!(bytes[k], b'=' | b'/' | b'>')
            {
                k += 1;
            }
            if k == start {
                k += 1;
                continue;
            }
            let attr_name = html[start..k].to_ascii_lowercase();
            while k < bytes.len() && bytes[k].is_ascii_whitespace() {
                k += 1;
            }
            let mut value = String::new();
            let mut span = None;
            if k < bytes.len() && bytes[k] == b'=' {
                k += 1;
                while k < bytes.len() && bytes[k].is_ascii_whitespace() {
                    k += 1;
                }
                if k < bytes.len() && (bytes[k] == b'"' || bytes[k] == b'\'') {
                    let quote = bytes[k];
                    k += 1;
                    let vstart = k;
                    while k < bytes.len() && bytes[k] != quote {
                        k += 1;
                    }
                    value = decode_entities(&html[vstart..k]);
                    span = Some((vstart, k));
                    k += 1; // Skip the closing quote when present.
                } else {
                    let vstart = k;
                    while k < bytes.len() && !bytes[k].is_ascii_whitespace() && bytes[k] != b'>' {
                        k += 1;
                    }
                    value = decode_entities(&html[vstart..k]);
                    span = Some((vstart, k));
                }
            }
            if attr_name == "src" {
                if let Some(s) = span {
                    src_spans.push(s);
                }
            }
            attrs.push((attr_name, value));
        }
        // `script`/`style` bodies are CDATA: skip to the close tag so
        // inner `<...>` never matches, like the parser's cdata mode.
        if (name == "script" || name == "style") && closed {
            let close = format!("</{name}");
            let lower = html.to_ascii_lowercase();
            if let Some(p) = lower[k..].find(&close) {
                let end = html[k + p..].find('>').map(|q| k + p + q + 1);
                k = end.unwrap_or(bytes.len());
            } else {
                k = bytes.len();
            }
        }
        if name == want {
            hits.push(TagHit { attrs, src_spans });
        }
        i = k.max(j);
        if i == j {
            i += 1;
        }
    }
    hits
}

/// The effective `src` of a hit: last `src` attribute wins, matching the
/// parser (duplicate attributes collapse, later value kept).
fn hit_src(hit: &TagHit) -> Option<&str> {
    hit.attrs
        .iter()
        .rev()
        .find(|(name, _)| name == "src")
        .map(|(_, value)| value.as_str())
}

/// Port of `extract_asset_ids` (`copy_s3_object.py:33-39`): the non-empty
/// `src` values of every `<tag>` in document order. An empty result covers
/// both "no match" and unparseable input (the `except` → `[]` path); a
/// `None` input cannot occur (`description_html` is `NOT NULL`).
pub fn extract_asset_ids(html: &str, tag: &str) -> Vec<String> {
    find_open_tags(html, tag)
        .iter()
        .filter_map(hit_src)
        .filter(|src| !src.is_empty())
        .map(str::to_string)
        .collect()
}

/// Port of `replace_asset_ids` (`copy_s3_object.py:42-52`): rewrite `src`
/// wherever it equals a pair's old id, in pair order per tag (so chained
/// pairs rewrite twice, exactly like the nested loop). Only the matched
/// value bytes change, so on canonical editor HTML the result equals
/// `str(soup)` byte for byte.
pub fn replace_asset_ids(html: &str, tag: &str, pairs: &[AssetIdPair]) -> String {
    if pairs.is_empty() {
        // No pair can match; still parse so behavior on broken input stays
        // total. The output equals the input (nothing to rewrite).
        return html.to_string();
    }
    let hits = find_open_tags(html, tag);
    // Decision per hit, in document order: the replacement `src`, if the
    // tag carries one and a pair matches it.
    let decisions: Vec<Option<&str>> = hits
        .iter()
        .map(|hit| {
            let mut current = hit_src(hit)?;
            if current.is_empty() {
                return None;
            }
            let mut changed = false;
            for pair in pairs {
                if current == pair.old_asset_id {
                    current = pair.new_asset_id.as_str();
                    changed = true;
                }
            }
            changed.then_some(current)
        })
        .collect();
    // Splice the replacements into the `src` value spans. A matched tag
    // rewrites every `src` value span it carries (duplicate `src`
    // attributes do not occur in editor HTML).
    let mut out = String::with_capacity(html.len());
    let mut cursor = 0;
    for (hit, decision) in hits.iter().zip(decisions.iter()) {
        if let Some(new_src) = decision {
            for (start, end) in hit.src_spans.iter() {
                out.push_str(&html[cursor..*start]);
                out.push_str(new_src);
                cursor = *end;
            }
        }
    }
    out.push_str(&html[cursor..]);
    out
}

/// Port of the destination key (`copy_s3_object.py:93`):
/// `{workspace.id}/{uuid4hex}-{attributes['name']}`.
pub fn destination_key(workspace_id: &str, uuid_hex: &str, name: &str) -> String {
    format!("{workspace_id}/{uuid_hex}-{name}")
}

/// Port of `original_asset.attributes.get("name")` in the key f-string
/// (`copy_s3_object.py:93`): a missing key or JSON null renders as
/// `"None"`, booleans as `"True"`/`"False"`, numbers in Python `str()`
/// form. The caller guarantees an object (anything else crashes the
/// task, like `.get` raising `AttributeError`).
pub fn render_attribute_name(attributes: &Map<String, Value>) -> String {
    match attributes.get("name") {
        Some(Value::String(name)) => name.clone(),
        Some(Value::Bool(true)) => "True".to_string(),
        Some(Value::Bool(false)) => "False".to_string(),
        Some(Value::Number(n)) => {
            if let Some(i) = n.as_i64() {
                i.to_string()
            } else if let Some(u) = n.as_u64() {
                u.to_string()
            } else if let Some(f) = n.as_f64() {
                render_python_float(f)
            } else {
                "None".to_string()
            }
        }
        _ => "None".to_string(),
    }
}

/// Port of the duplicate's attributes (`copy_s3_object.py:95-99`):
/// exactly the `{name, type, size}` subset — missing keys stay present
/// with `None` (`.get` returns `None`, the key is still written).
pub fn duplicate_attributes(attributes: &Map<String, Value>) -> Map<String, Value> {
    let mut subset = Map::with_capacity(3);
    for key in ["name", "type", "size"] {
        subset.insert(
            key.to_string(),
            attributes.get(key).cloned().unwrap_or(Value::Null),
        );
    }
    subset
}

/// Python `str(float)`: `5.0` keeps its `.0`; anything already carrying a
/// fraction or exponent renders plainly.
fn render_python_float(f: f64) -> String {
    if !f.is_finite() {
        return "None".to_string();
    }
    let text = f.to_string();
    if text.contains(['.', 'e', 'E']) {
        text
    } else {
        format!("{text}.0")
    }
}

/// The entity a copy task duplicates (`copy_s3_object.py:134`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CopyEntity {
    Page,
    Issue,
}

impl CopyEntity {
    /// Port of `{"PAGE": Page, "ISSUE": Issue}.get(entity_name)`: anything
    /// else raises `ValueError`, caught by the task → `[]`.
    pub fn for_name(entity_name: &str) -> Option<CopyEntity> {
        match entity_name {
            "PAGE" => Some(CopyEntity::Page),
            "ISSUE" => Some(CopyEntity::Issue),
            _ => None,
        }
    }

    /// The table the entity row lives in.
    pub fn table(self) -> &'static str {
        match self {
            CopyEntity::Page => "pages",
            CopyEntity::Issue => "issues",
        }
    }

    /// Port of the convert variant (`copy_s3_object.py:71-74`): `"rich"`
    /// exactly when the entity is a page, `"document"` otherwise.
    pub fn convert_variant(self) -> &'static str {
        match self {
            CopyEntity::Page => "rich",
            CopyEntity::Issue => "document",
        }
    }
}

/// Port of `external_data.get("description_json") or {}` with its comment
/// (`copy_s3_object.py:143-148`): a missing key — or any other falsy JSON —
/// falls back to `{}` so the `NOT NULL ... default=dict` column is never
/// written as `NULL`.
pub fn description_json_or_empty(value: Option<Value>) -> Value {
    match value {
        Some(v) if json_truthy(&v) => v,
        _ => Value::Object(Map::new()),
    }
}

/// Python truthiness over JSON values.
fn json_truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => {
            n.as_i64().is_some_and(|i| i != 0)
                || n.as_u64().is_some_and(|u| u != 0)
                || n.as_f64().is_some_and(|f| f != 0.0)
        }
        Value::String(s) => !s.is_empty(),
        Value::Array(items) => !items.is_empty(),
        Value::Object(fields) => !fields.is_empty(),
    }
}

/// `FileAsset.objects.get(pk)` (`storage_metadata_task.py:18`): the live
/// row's storage key. The row id is already bound, so only `asset` is
/// projected; the manager scope (`deleted_at IS NULL`) is kept.
pub const METADATA_SELECT_SQL: &str = "SELECT \"file_assets\".\"asset\" FROM \"file_assets\" WHERE (\"file_assets\".\"deleted_at\" IS NULL AND \"file_assets\".\"id\" = $1)";

/// `asset.save(update_fields=["storage_metadata"])`: the named column plus
/// `updated_at`, which `auto_now` always includes via `pre_save`.
/// `$1` is the metadata JSON (`NULL` when S3 answered nothing).
pub const METADATA_SAVE_SQL: &str = "UPDATE \"file_assets\" SET \"storage_metadata\" = $1, \"updated_at\" = $2 WHERE \"file_assets\".\"id\" = $3";

/// `model_class.objects.get(id)` for the copy task: live rows only.
pub fn entity_select_sql(entity: CopyEntity) -> String {
    format!(
        "SELECT \"id\", \"workspace_id\", \"description_html\" FROM \"{}\" WHERE (\"deleted_at\" IS NULL AND \"id\" = $1)",
        entity.table()
    )
}

/// `FileAsset.objects.filter(workspace, project_id, id__in)` for the copy
/// scope (`copy_s3_object.py:90`). `$1` workspace; with a project the
/// project is `$2` and the id list `$3`, without one (`IS NULL`, matching
/// `filter(project_id=None)`) the id list is `$2` so the binds line up.
pub fn original_assets_sql(project_is_null: bool) -> String {
    if project_is_null {
        "SELECT \"id\", \"asset\", \"attributes\", \"size\", \"entity_type\", \"storage_metadata\" FROM \"file_assets\" WHERE (\"deleted_at\" IS NULL AND \"workspace_id\" = $1 AND \"project_id\" IS NULL AND \"id\" = ANY($2))".to_string()
    } else {
        "SELECT \"id\", \"asset\", \"attributes\", \"size\", \"entity_type\", \"storage_metadata\" FROM \"file_assets\" WHERE (\"deleted_at\" IS NULL AND \"workspace_id\" = $1 AND \"project_id\" = $2 AND \"id\" = ANY($3))".to_string()
    }
}

/// `FileAsset.objects.create(...)` (`copy_s3_object.py:94-108`): every
/// concrete column, Django field order. Unset FKs, `entity_identifier`
/// (never assigned by the task) and `updated_by` insert as `NULL`;
/// `is_deleted`/`is_archived`/`is_uploaded` as `FALSE`.
pub const DUPLICATE_INSERT_SQL: &str = "INSERT INTO \"file_assets\" (\"id\", \"created_at\", \"updated_at\", \"created_by_id\", \"updated_by_id\", \"deleted_at\", \"attributes\", \"asset\", \"user_id\", \"workspace_id\", \"draft_issue_id\", \"project_id\", \"issue_id\", \"comment_id\", \"page_id\", \"entity_type\", \"entity_identifier\", \"is_deleted\", \"is_archived\", \"external_id\", \"external_source\", \"size\", \"is_uploaded\", \"storage_metadata\") VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18, $19, $20, $21, $22, $23, $24)";

/// `filter(pk__in=new ids).update(is_uploaded=True)`
/// (`copy_s3_object.py:117`): `QuerySet.update` writes the named column
/// only — `updated_at` is untouched.
pub const MARK_UPLOADED_SQL: &str = "UPDATE \"file_assets\" SET \"is_uploaded\" = TRUE WHERE (\"deleted_at\" IS NULL AND \"id\" = ANY($1))";

/// `entity.save()` after the description rewrite: full save, so the only
/// observable changes are the new HTML and the `auto_now` bump.
pub fn description_save_sql(entity: CopyEntity) -> String {
    format!(
        "UPDATE \"{}\" SET \"description_html\" = $1, \"updated_at\" = $2 WHERE \"id\" = $3",
        entity.table()
    )
}

/// The second save when the live service answered: `description_json` is
/// never `NULL` (the `or {}` fallback); the binary column is written only
/// when the response carried it.
pub fn description_docs_save_sql(entity: CopyEntity, with_binary: bool) -> String {
    if with_binary {
        format!(
            "UPDATE \"{}\" SET \"description_json\" = $1, \"description_binary\" = $2, \"updated_at\" = $3 WHERE \"id\" = $4",
            entity.table()
        )
    } else {
        format!(
            "UPDATE \"{}\" SET \"description_json\" = $1, \"updated_at\" = $2 WHERE \"id\" = $3",
            entity.table()
        )
    }
}

/// One S3 `head_object` answer, serialized exactly like
/// `S3Storage.get_object_metadata` (`settings/storage.py:156-170`):
/// `ContentType`, `ContentLength`, ISO `LastModified`, `ETag`, `Metadata`.
#[derive(Debug, Clone, PartialEq)]
pub struct HeadMeta {
    pub content_type: Option<String>,
    pub content_length: Option<i64>,
    pub last_modified: Option<String>,
    pub etag: Option<String>,
    pub metadata: Map<String, Value>,
}

impl HeadMeta {
    pub fn to_json(&self) -> Value {
        let mut fields = Map::with_capacity(5);
        fields.insert(
            "ContentType".to_string(),
            self.content_type
                .clone()
                .map(Value::String)
                .unwrap_or(Value::Null),
        );
        fields.insert(
            "ContentLength".to_string(),
            self.content_length
                .map(|n| Value::Number(n.into()))
                .unwrap_or(Value::Null),
        );
        fields.insert(
            "LastModified".to_string(),
            self.last_modified
                .clone()
                .map(Value::String)
                .unwrap_or(Value::Null),
        );
        fields.insert(
            "ETag".to_string(),
            self.etag.clone().map(Value::String).unwrap_or(Value::Null),
        );
        fields.insert("Metadata".to_string(), Value::Object(self.metadata.clone()));
        Value::Object(fields)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn delete_days_default_and_values() {
        assert_eq!(sweep_lookback_days(None), Ok(7));
        assert_eq!(sweep_lookback_days(Some("7")), Ok(7));
        assert_eq!(sweep_lookback_days(Some("30")), Ok(30));
        assert_eq!(sweep_lookback_days(Some("  7  ")), Ok(7));
        assert_eq!(sweep_lookback_days(Some("+7")), Ok(7));
        assert_eq!(sweep_lookback_days(Some("1_0")), Ok(10));
        assert_eq!(sweep_lookback_days(Some("-3")), Ok(-3));
    }

    #[test]
    fn delete_days_rejects_non_integers() {
        for raw in ["", "seven", "7.5", "1__0", "_1", "1_", "0x10", "--7"] {
            assert!(sweep_lookback_days(Some(raw)).is_err(), "{raw:?} must fail");
        }
    }

    #[test]
    fn sweep_sql_predicate_shape() {
        assert!(SWEEP_SQL.starts_with("UPDATE \"file_assets\" SET \"deleted_at\" = $1 WHERE ("));
        assert!(SWEEP_SQL.contains("\"deleted_at\" IS NULL"));
        assert!(SWEEP_SQL.contains("\"created_at\" < $2"));
        assert!(SWEEP_SQL.contains("NOT \"file_assets\".\"is_uploaded\""));
    }

    #[test]
    fn entity_map_covers_enum_minus_draft_attachment() {
        let cases = [
            ("WORKSPACE_LOGO", Some("workspace_id")),
            ("PROJECT_COVER", Some("project_id")),
            ("USER_AVATAR", Some("user_id")),
            ("USER_COVER", Some("user_id")),
            ("ISSUE_ATTACHMENT", Some("issue_id")),
            ("ISSUE_DESCRIPTION", Some("issue_id")),
            ("PAGE_DESCRIPTION", Some("page_id")),
            ("COMMENT_DESCRIPTION", Some("comment_id")),
            ("DRAFT_ISSUE_DESCRIPTION", Some("draft_issue_id")),
            // Present on the enum but absent from the mapping: no column.
            ("DRAFT_ISSUE_ATTACHMENT", None),
            ("BOGUS", None),
            ("", None),
        ];
        for (entity_type, expected) in cases {
            assert_eq!(entity_id_field(entity_type), expected, "{entity_type}");
        }
    }

    #[test]
    fn extract_finds_srcs_in_order() {
        let html = "<p>t</p><image-component src=\"id-1\"></image-component><img src=\"keep\"/>";
        assert_eq!(extract_asset_ids(html, "image-component"), vec!["id-1"]);
        assert_eq!(extract_asset_ids(html, "img"), vec!["keep"]);
        assert!(extract_asset_ids(html, "video").is_empty());
    }

    #[test]
    fn extract_skips_empty_missing_and_comments() {
        let html = concat!(
            "<!-- <image-component src=\"x\"> -->",
            "<image-component></image-component>",
            "<image-component src=\"\"></image-component>",
            "<image-component src=\"a\"/><image-component src=\"b\"></image-component>",
        );
        assert_eq!(extract_asset_ids(html, "image-component"), vec!["a", "b"]);
    }

    #[test]
    fn extract_matches_parser_case_and_quoting() {
        // Tag/attribute names are lowercased by the parser.
        assert_eq!(
            extract_asset_ids(
                "<IMAGE-COMPONENT SRC=\"id-1\"></IMAGE-COMPONENT>",
                "image-component"
            ),
            vec!["id-1"]
        );
        assert_eq!(
            extract_asset_ids(
                "<image-component src=id-1></image-component>",
                "image-component"
            ),
            vec!["id-1"]
        );
        assert_eq!(
            extract_asset_ids(
                "<image-component src='id-1'></image-component>",
                "image-component"
            ),
            vec!["id-1"]
        );
        // Duplicate attributes collapse, later value kept.
        assert_eq!(
            extract_asset_ids(
                "<image-component src=\"a\" src=\"b\"></image-component>",
                "image-component"
            ),
            vec!["b"]
        );
        // `>` inside quotes does not end the tag; entities decode.
        assert_eq!(
            extract_asset_ids(
                "<image-component title=\"a>b\" src=\"a&amp;b\"></image-component>",
                "image-component"
            ),
            vec!["a&b"]
        );
        // Unclosed tags still count; script bodies never match.
        assert_eq!(
            extract_asset_ids(
                "<p>unclosed<image-component src=\"id-1\">",
                "image-component"
            ),
            vec!["id-1"]
        );
        assert!(extract_asset_ids(
            "<script>var s = \"<image-component src=\\\"x\\\">\";</script>",
            "image-component"
        )
        .is_empty());
        assert!(extract_asset_ids("", "image-component").is_empty());
        assert!(extract_asset_ids("plain text", "image-component").is_empty());
    }

    #[test]
    fn replace_rewrites_only_matched_src() {
        let html = "<p>t</p><image-component src=\"id-1\"></image-component><img src=\"keep\"/>";
        let pairs = [AssetIdPair {
            old_asset_id: "id-1".to_string(),
            new_asset_id: "id-2".to_string(),
        }];
        assert_eq!(
            replace_asset_ids(html, "image-component", &pairs),
            "<p>t</p><image-component src=\"id-2\"></image-component><img src=\"keep\"/>"
        );
    }

    #[test]
    fn replace_no_match_returns_reparse_identical_output() {
        let html = "<p>t</p><image-component src=\"id-1\"></image-component><img src=\"keep\"/>";
        let pairs = [AssetIdPair {
            old_asset_id: "zzz".to_string(),
            new_asset_id: "id-2".to_string(),
        }];
        // Canonical input re-serializes to itself, so the output equals
        // the input — matching the fixture's no-match golden.
        assert_eq!(replace_asset_ids(html, "image-component", &pairs), html);
        assert_eq!(replace_asset_ids(html, "image-component", &[]), html);
    }

    #[test]
    fn replace_chains_pairs_in_order() {
        let html = "<image-component src=\"a\"></image-component>";
        let pairs = [
            AssetIdPair {
                old_asset_id: "a".to_string(),
                new_asset_id: "b".to_string(),
            },
            AssetIdPair {
                old_asset_id: "b".to_string(),
                new_asset_id: "c".to_string(),
            },
        ];
        assert_eq!(
            replace_asset_ids(html, "image-component", &pairs),
            "<image-component src=\"c\"></image-component>"
        );
    }

    #[test]
    fn replace_leaves_srcless_tags_alone() {
        let html =
            "<image-component></image-component><image-component src=\"a\"></image-component>";
        let pairs = [AssetIdPair {
            old_asset_id: "a".to_string(),
            new_asset_id: "b".to_string(),
        }];
        assert_eq!(
            replace_asset_ids(html, "image-component", &pairs),
            "<image-component></image-component><image-component src=\"b\"></image-component>"
        );
    }

    #[test]
    fn destination_key_shape() {
        assert_eq!(
            destination_key("ws-id", "abc123", "photo.png"),
            "ws-id/abc123-photo.png"
        );
    }

    #[test]
    fn attribute_name_renders_like_python_str() {
        let obj = |v: Value| v.as_object().unwrap().clone();
        assert_eq!(
            render_attribute_name(&obj(json!({"name": "a.png"}))),
            "a.png"
        );
        assert_eq!(render_attribute_name(&obj(json!({}))), "None");
        assert_eq!(render_attribute_name(&obj(json!({"name": null}))), "None");
        assert_eq!(render_attribute_name(&obj(json!({"name": true}))), "True");
        assert_eq!(render_attribute_name(&obj(json!({"name": 12}))), "12");
        assert_eq!(render_attribute_name(&obj(json!({"name": 5.0}))), "5.0");
        assert_eq!(render_attribute_name(&Map::new()), "None");
    }

    #[test]
    fn duplicate_attributes_keep_name_type_size_only() {
        let attrs = json!({"name": "a.png", "type": "png", "size": 9, "extra": 1});
        assert_eq!(
            Value::Object(duplicate_attributes(attrs.as_object().unwrap())),
            json!({"name": "a.png", "type": "png", "size": 9})
        );
        // Missing keys stay present as null, like `.get` returning None.
        assert_eq!(
            Value::Object(duplicate_attributes(&Map::new())),
            json!({"name": null, "type": null, "size": null})
        );
    }

    #[test]
    fn entity_table_and_variant() {
        assert_eq!(CopyEntity::for_name("PAGE"), Some(CopyEntity::Page));
        assert_eq!(CopyEntity::for_name("ISSUE"), Some(CopyEntity::Issue));
        assert_eq!(CopyEntity::for_name("COMMENT"), None);
        assert_eq!(CopyEntity::for_name("page"), None);
        assert_eq!(CopyEntity::Page.table(), "pages");
        assert_eq!(CopyEntity::Issue.table(), "issues");
        assert_eq!(CopyEntity::Page.convert_variant(), "rich");
        assert_eq!(CopyEntity::Issue.convert_variant(), "document");
    }

    #[test]
    fn description_json_falls_back_on_falsy() {
        let doc = json!({"a": 1});
        assert_eq!(description_json_or_empty(Some(doc.clone())), doc);
        for falsy in [
            Value::Null,
            Value::Bool(false),
            json!(0),
            json!(""),
            json!([]),
            json!({}),
        ] {
            assert_eq!(description_json_or_empty(Some(falsy)), json!({}));
        }
        assert_eq!(description_json_or_empty(None), json!({}));
    }

    #[test]
    fn head_meta_serializes_like_boto() {
        let meta = HeadMeta {
            content_type: Some("image/png".to_string()),
            content_length: Some(12),
            last_modified: Some("2026-09-28T06:00:00+00:00".to_string()),
            etag: Some("\"abc\"".to_string()),
            metadata: Map::new(),
        };
        assert_eq!(
            meta.to_json(),
            json!({
                "ContentType": "image/png",
                "ContentLength": 12,
                "LastModified": "2026-09-28T06:00:00+00:00",
                "ETag": "\"abc\"",
                "Metadata": {},
            })
        );
        let missing = HeadMeta {
            content_type: None,
            content_length: None,
            last_modified: None,
            etag: None,
            metadata: Map::new(),
        };
        assert_eq!(
            missing.to_json(),
            json!({
                "ContentType": null,
                "ContentLength": null,
                "LastModified": null,
                "ETag": null,
                "Metadata": {},
            })
        );
    }

    #[test]
    fn copy_scope_sql_null_vs_value_project() {
        let with_project = original_assets_sql(false);
        assert!(with_project.contains("\"project_id\" = $2"));
        assert!(with_project.contains("\"id\" = ANY($3)"));
        let null_project = original_assets_sql(true);
        assert!(null_project.contains("\"project_id\" IS NULL"));
        // No `$3` without a project bind: the id list is `$2`.
        assert!(null_project.contains("\"id\" = ANY($2)"));
        assert!(!null_project.contains("$3"));
    }

    #[test]
    fn entity_and_docs_sql_use_entity_table() {
        assert!(entity_select_sql(CopyEntity::Page).contains("FROM \"pages\""));
        assert!(entity_select_sql(CopyEntity::Issue).contains("FROM \"issues\""));
        assert!(description_save_sql(CopyEntity::Page).contains("UPDATE \"pages\""));
        let with_bin = description_docs_save_sql(CopyEntity::Issue, true);
        assert!(with_bin.contains("description_binary"));
        let without_bin = description_docs_save_sql(CopyEntity::Issue, false);
        assert!(!without_bin.contains("description_binary"));
        assert!(without_bin.contains("description_json"));
    }
}
