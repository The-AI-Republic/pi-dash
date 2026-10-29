//! Relation operations: relate and unrelate.
//!
//! The serializer step of the v1 agent relate/unrelate endpoints, moved
//! from `pidash_api::v1_work_items::handlers_relations` (OVL-14,
//! PIDASHCONV-1649): `_write` over `relate` / `unrelate`
//! (`views/issue.py:3163-3194`, `orchestration/relations.py`).

use std::collections::HashMap;

use chrono::Utc;
use serde_json::{Map, Value};
use sqlx::{PgPool, Row};
use uuid::Uuid;

use pidash_compat::db::is_unique_violation;

use crate::domain::relations as relation_domain;
use crate::domain::WriteCtx;
use crate::read;
use crate::v1_work_items::queries_sub::actual_relation;
use crate::v1_work_items::tasks as work_tasks;

use super::common::{db_error, map_read_error, now_utc};
use super::deps::Deps;
use super::error::{Error, Outcome};

/// The related-objects sweep enqueued by `SoftDeleteModel.delete()`
/// (`db/mixins.py:72-78` over `bgtasks/deletion_task.py:18`).
const SOFT_DELETE_TASK: &str = "pi_dash.bgtasks.deletion_task.soft_delete_related_objects";

/// Every relation type an agent may name, in display order
/// (`orchestration/relations.py:55-66`) — also the grouped-response key
/// order.
pub const RELATION_TYPES: &[&str] = &[
    "blocked_by",
    "blocking",
    "relates_to",
    "duplicate",
    "start_before",
    "start_after",
    "finish_before",
    "finish_after",
    "implemented_by",
    "implements",
];

/// Types stored with the ends swapped under their forward name
/// (`orchestration/relations.py:70`). Note the four members — the agent
/// vocabulary reverses `implements` too, unlike the endpoint POST's
/// three-member set (`views/issue.py:3077`).
const AGENT_REVERSE_TYPES: &[&str] = &["blocking", "start_after", "finish_after", "implements"];

/// Per-type cap on grouped lists (`orchestration/relations.py:74`).
const GROUP_LIMIT: usize = 100;

/// Python `str.strip()` with no argument: ASCII whitespace plus every
/// `str.isspace()` character. Rust `char::is_whitespace` (the Unicode
/// `White_Space` property) matches it except for U+001C–U+001F, which
/// Python strips and Rust excludes — hence the explicit arm.
fn py_stripped(text: &str) -> &str {
    text.trim_matches(|c: char| c.is_whitespace() || ('\u{1c}'..='\u{1f}').contains(&c))
}

/// `validate_relation_type` (`relations.py:81-85`): strip + lowercase, then
/// the ten names. A non-string truthy value raises `AttributeError`
/// (REL-3, uncaught → 500); falsy values (`None`, `0`, `false`, `""`,
/// `[]`, `{}`) take the `""` arm → the 400. `None` (missing) also 400s.
fn validate_relation_type(raw: Option<&Value>) -> Result<String, Error> {
    let text = match raw {
        None => String::new(),
        Some(Value::String(text)) => text.clone(),
        Some(Value::Number(number)) if number.as_f64() == Some(0.0) => String::new(),
        Some(Value::Bool(false)) | Some(Value::Null) => String::new(),
        Some(Value::Array(items)) if items.is_empty() => String::new(),
        Some(Value::Object(map)) if map.is_empty() => String::new(),
        // REL-3: `(relation_type or "")` keeps truthy non-strings, and
        // `.strip()` on them raises `AttributeError` → generic 500.
        Some(_) => return Err(Error::ServerError),
    };
    let normalized: String = py_stripped(&text)
        .chars()
        .flat_map(|c| c.to_lowercase())
        .collect();
    if RELATION_TYPES.contains(&normalized.as_str()) {
        Ok(normalized)
    } else {
        Err(Error::BadError(format!(
            "relation_type must be one of: {}",
            RELATION_TYPES.join(", ")
        )))
    }
}

/// CPython `uuid.UUID(hex-string)` acceptance (`resolve_refs` tries it
/// before the identifier form): optional case-sensitive `urn:` + `uuid:`
/// prefixes, any number of surrounding braces, hyphens in ANY positions,
/// then exactly 32 hex digits where `int(hex, 16)` also tolerates
/// single underscores between digits. Returns the normalized lowercase
/// hyphenated id. Anything else is `None` (→ the `PROJ-123` attempt).
fn parse_python_uuid(text: &str) -> Option<Uuid> {
    let mut hex = text;
    hex = hex.strip_prefix("urn:").unwrap_or(hex);
    hex = hex.strip_prefix("uuid:").unwrap_or(hex);
    hex = hex.trim_matches(|c| c == '{' || c == '}');
    if hex.is_empty() {
        return None;
    }
    let compact: String = hex.chars().filter(|c| *c != '-').collect();
    // `int(hex, 16)` grammar: hex digits with single `_` separators
    // (never leading, trailing, or doubled), 32 digits total.
    if compact.is_empty() || compact.starts_with('_') || compact.ends_with('_') {
        return None;
    }
    let mut digits = String::with_capacity(32);
    let mut previous_underscore = false;
    for c in compact.chars() {
        if c == '_' {
            if previous_underscore {
                return None;
            }
            previous_underscore = true;
            continue;
        }
        previous_underscore = false;
        if !c.is_ascii_hexdigit() {
            return None;
        }
        digits.push(c);
    }
    if digits.len() != 32 {
        return None;
    }
    Uuid::parse_str(&digits).ok()
}

/// One resolved reference: the visible-pool row plus its identifier facts.
#[derive(Debug, Clone, PartialEq, Eq)]
struct ResolvedIssue {
    id: Uuid,
    workspace_id: Uuid,
    project_identifier: String,
    sequence_id: i32,
}

impl ResolvedIssue {
    /// `identifier(issue)` (`relations.py:88-89`).
    fn identifier(&self) -> String {
        format!("{}-{}", self.project_identifier, self.sequence_id)
    }
}

/// The shared visible-pool predicates: `member_project_issues(user, slug)`
/// (`core/querysets.py:19-30`) — the `IssueManager` scope plus workspace
/// slug plus an active project membership of the caller. The membership
/// join carries NO `deleted_at` guard (fixture-verbatim
/// `member_guard_sql`).
const VISIBLE_POOL_JOINS: &str = r#"FROM "issues"
           LEFT OUTER JOIN "states" ON ("issues"."state_id" = "states"."id")
           INNER JOIN "projects" ON ("issues"."project_id" = "projects"."id")
           INNER JOIN "workspaces" ON ("issues"."workspace_id" = "workspaces"."id")"#;

const VISIBLE_POOL_WHERE: &str = r#""issues"."deleted_at" IS NULL
             AND NOT ("states"."group" = 'triage' AND "states"."group" IS NOT NULL)
             AND NOT ("issues"."archived_at" IS NOT NULL)
             AND NOT ("projects"."archived_at" IS NOT NULL)
             AND NOT ("issues"."is_draft")
             AND "workspaces"."slug" = $1
             AND EXISTS(SELECT 1 FROM "project_members"
                        WHERE "project_members"."project_id" = "issues"."project_id"
                          AND "project_members"."member_id" = $2
                          AND "project_members"."is_active")"#;

/// `resolve_refs(refs, visible)` (`relations.py:138-163`): each raw ref is
/// `str(raw or "").strip()`, tried as a UUID first (flexible parse),
/// then as `PROJ-123` (`project__identifier__iexact` + `sequence_id`),
/// each against the caller's visible pool (invisible ≡ nonexistent).
/// Returns `(found, unresolved)` in request order; duplicates resolve
/// repeatedly (deduped later by `_check_targets`).
async fn resolve_refs(
    pool: &PgPool,
    slug: &str,
    user_id: &Uuid,
    refs: &[Value],
) -> Result<(Vec<ResolvedIssue>, Vec<String>), Error> {
    let mut found = Vec::with_capacity(refs.len());
    let mut unresolved = Vec::new();
    for raw in refs {
        let text = python_ref_text(raw);
        let stripped = py_stripped(&text).to_owned();
        let mut matched: Option<ResolvedIssue> = None;
        if !stripped.is_empty() {
            if let Some(id) = parse_python_uuid(&stripped) {
                matched = resolve_by_id(pool, slug, user_id, &id).await?;
            } else if let Some((ident, sequence)) = split_identifier_ref(&stripped)? {
                matched = resolve_by_identifier(pool, slug, user_id, &ident, sequence).await?;
            }
        }
        match matched {
            Some(issue) => found.push(issue),
            // `ref or str(raw)`: an empty strip echoes the ORIGINAL
            // rendering (`None`, `0`, `{}`, ...), never "".
            None => unresolved.push(if stripped.is_empty() {
                python_str(raw)
            } else {
                stripped
            }),
        }
    }
    Ok((found, unresolved))
}

/// `str(raw or "")` for a ref: falsy JSON (`null`, `false`, `0`/`0.0`,
/// `""`, `[]`, `{}`) renders `""` here (the echo arm re-renders the
/// original below); everything else renders Python `str()`.
fn python_ref_text(raw: &Value) -> String {
    let falsy = match raw {
        Value::Null => true,
        Value::Bool(flag) => !flag,
        Value::Number(number) => number.as_f64() == Some(0.0),
        Value::String(text) => text.is_empty(),
        Value::Array(items) => items.is_empty(),
        Value::Object(map) => map.is_empty(),
    };
    if falsy {
        String::new()
    } else {
        python_str(raw)
    }
}

/// Python `str()` of a JSON-decoded value: strings bare, `None`/`True`/
/// `False`, ints plain, floats shortest-repr, containers `repr`.
fn python_str(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Null => "None".to_owned(),
        Value::Bool(true) => "True".to_owned(),
        Value::Bool(false) => "False".to_owned(),
        Value::Number(number) => {
            if let Some(int) = number.as_i64() {
                int.to_string()
            } else if let Some(uint) = number.as_u64() {
                uint.to_string()
            } else {
                python_float_repr(number.as_f64().expect("f64"))
            }
        }
        Value::Array(items) => {
            let inner: Vec<String> = items.iter().map(python_repr).collect();
            format!("[{}]", inner.join(", "))
        }
        Value::Object(map) => {
            let inner: Vec<String> = map
                .iter()
                .map(|(key, item)| format!("{}: {}", python_str_repr(key), python_repr(item)))
                .collect();
            format!("{{{}}}", inner.join(", "))
        }
    }
}

/// Python `repr()` of a JSON-decoded value (for container echoes).
fn python_repr(value: &Value) -> String {
    match value {
        Value::String(text) => python_str_repr(text),
        other => python_str(other),
    }
}

/// Python `repr()` of a string.
fn python_str_repr(text: &str) -> String {
    let use_double = text.contains('\'') && !text.contains('"');
    let quote = if use_double { '"' } else { '\'' };
    let mut out = String::with_capacity(text.len() + 2);
    out.push(quote);
    for c in text.chars() {
        if c == quote {
            out.push('\\');
            out.push(quote);
        } else {
            match c {
                '\\' => out.push_str("\\\\"),
                '\n' => out.push_str("\\n"),
                '\r' => out.push_str("\\r"),
                '\t' => out.push_str("\\t"),
                c if c < ' ' || c == '\u{7f}' => {
                    out.push_str(&format!("\\x{:02x}", c as u32));
                }
                c if c.is_control() => {
                    let code = c as u32;
                    if code < 0x100 {
                        out.push_str(&format!("\\x{:02x}", code));
                    } else if code < 0x10000 {
                        out.push_str(&format!("\\u{:04x}", code));
                    } else {
                        out.push_str(&format!("\\U{:08x}", code));
                    }
                }
                c => out.push(c),
            }
        }
    }
    out.push(quote);
    out
}

/// Python `repr()` of a float: shortest round-trip digits laid out by
/// Python's rules — fixed notation for decimal exponents `-3..=16` with a
/// mandatory `.0` on integral values, else `d[.ddd]e±XX` with a signed,
/// ≥2-digit exponent.
fn python_float_repr(value: f64) -> String {
    if value.is_nan() {
        return "nan".to_string();
    }
    if value.is_infinite() {
        return if value > 0.0 { "inf" } else { "-inf" }.to_string();
    }
    let negative = value.is_sign_negative();
    let abs = value.abs();
    if abs == 0.0 {
        return if negative { "-0.0" } else { "0.0" }.to_string();
    }
    let ryu = serde_json::Number::from_f64(abs)
        .expect("finite")
        .to_string();
    let (mantissa, exp): (&str, i32) = match ryu.split_once(['e', 'E']) {
        Some((mantissa, exp)) => (mantissa, exp.parse().expect("ryu exponent")),
        None => (ryu.as_str(), 0),
    };
    let point = mantissa.find('.').unwrap_or(mantissa.len());
    let mut digits: Vec<char> = mantissa.chars().filter(|c| c.is_ascii_digit()).collect();
    while digits.len() > 1 && digits[0] == '0' {
        digits.remove(0);
    }
    let after_point = mantissa.len() - point - usize::from(point < mantissa.len());
    let dec_exp = exp - after_point as i32 + digits.len() as i32;
    let mut out = String::new();
    if negative {
        out.push('-');
    }
    if (-3..=16).contains(&dec_exp) {
        if dec_exp <= 0 {
            out.push_str("0.");
            out.push_str(&"0".repeat((-dec_exp) as usize));
            out.extend(digits);
        } else if dec_exp as usize >= digits.len() {
            let pad = dec_exp as usize - digits.len();
            out.extend(digits);
            out.push_str(&"0".repeat(pad));
            out.push_str(".0");
        } else {
            let split = dec_exp as usize;
            out.extend(digits[..split].iter());
            out.push('.');
            out.extend(digits[split..].iter());
        }
    } else {
        out.push(digits[0]);
        if digits.len() > 1 {
            out.push('.');
            out.extend(digits[1..].iter());
        }
        let exp10 = dec_exp - 1;
        out.push('e');
        out.push(if exp10 < 0 { '-' } else { '+' });
        let mag = exp10.unsigned_abs().to_string();
        if mag.len() < 2 {
            out.push('0');
        }
        out.push_str(&mag);
    }
    out
}

/// `ref.rpartition("-")` + `seq.isdigit()` + `int(seq)`
/// (`relations.py:155-157`): the identifier attempt. Returns `None` when
/// the shape misses; a numeric-but-not-decimal tail raises REL-4's 500;
/// an overflowing tail matches nothing (a PG int column never holds it).
fn split_identifier_ref(text: &str) -> Result<Option<(String, i64)>, Error> {
    let Some((ident, seq)) = text.rsplit_once('-') else {
        return Ok(None);
    };
    if ident.is_empty() || seq.is_empty() {
        return Ok(None);
    }
    if seq.chars().all(|c| c.is_ascii_digit()) {
        return match seq.parse::<i64>() {
            Ok(sequence) => Ok(Some((ident.to_owned(), sequence))),
            // Unbounded Python ints filter to nothing on an int column.
            Err(_) => Ok(None),
        };
    }
    // REL-4: `seq.isdigit()` true but `int(seq)` raises → uncaught
    // `ValueError` → 500. Approximated with `is_numeric` (see the module
    // docs for the `Nl` edge).
    if seq.chars().all(|c| c.is_numeric()) {
        return Err(Error::ServerError);
    }
    Ok(None)
}

/// One `match...first()` (`relations.py:158`): the visible pool filtered
/// to the id, `Meta.ordering` (`-created_at`) + `LIMIT 1`.
async fn resolve_by_id(
    pool: &PgPool,
    slug: &str,
    user_id: &Uuid,
    id: &Uuid,
) -> Result<Option<ResolvedIssue>, Error> {
    let row = sqlx::query(&format!(
        r#"SELECT "issues"."id", "issues"."workspace_id", "projects"."identifier", "issues"."sequence_id"
           {VISIBLE_POOL_JOINS}
           WHERE ({VISIBLE_POOL_WHERE}) AND "issues"."id" = $3
           ORDER BY "issues"."created_at" DESC LIMIT 1"#
    ))
    .bind(slug)
    .bind(user_id)
    .bind(id)
    .fetch_optional(pool)
    .await
    .map_err(|error| db_error(error, "resolve-ref-id"))?;
    row.map(decode_resolved).transpose()
}

/// The identifier attempt (`relations.py:157-158`): `UPPER(identifier) =
/// UPPER($)` (`__iexact`) + `sequence_id`, same `.first()` tail.
async fn resolve_by_identifier(
    pool: &PgPool,
    slug: &str,
    user_id: &Uuid,
    ident: &str,
    sequence: i64,
) -> Result<Option<ResolvedIssue>, Error> {
    // Unbounded Python ints filter to nothing on the int column.
    if sequence > i64::from(i32::MAX) || sequence < i64::from(i32::MIN) {
        return Ok(None);
    }
    let row = sqlx::query(&format!(
        r#"SELECT "issues"."id", "issues"."workspace_id", "projects"."identifier", "issues"."sequence_id"
           {VISIBLE_POOL_JOINS}
           WHERE ({VISIBLE_POOL_WHERE}) AND UPPER("projects"."identifier") = UPPER($3)
             AND "issues"."sequence_id" = $4
           ORDER BY "issues"."created_at" DESC LIMIT 1"#
    ))
    .bind(slug)
    .bind(user_id)
    .bind(ident)
    .bind(sequence as i32)
    .fetch_optional(pool)
    .await
    .map_err(|error| db_error(error, "resolve-ref-identifier"))?;
    row.map(decode_resolved).transpose()
}

fn decode_resolved(row: sqlx::postgres::PgRow) -> Result<ResolvedIssue, Error> {
    Ok(ResolvedIssue {
        id: read::row_uuid(&row, "id").map_err(map_read_error)?,
        workspace_id: read::row_uuid(&row, "workspace_id").map_err(map_read_error)?,
        project_identifier: read::row_string(&row, "identifier").map_err(map_read_error)?,
        sequence_id: row.try_get("sequence_id").map_err(|_| Error::ServerError)?,
    })
}

/// `_source` (`views/issue.py:3153-3156`): the manager-scoped issue plus
/// its project identifier (for `identifier()`), state, and workspace.
/// A miss answers the `ObjectDoesNotExist` 404.
pub struct SourceIssue {
    pub id: Uuid,
    pub project_id: Uuid,
    pub workspace_id: Uuid,
    pub project_identifier: String,
    pub sequence_id: i32,
}

impl SourceIssue {
    pub fn identifier(&self) -> String {
        format!("{}-{}", self.project_identifier, self.sequence_id)
    }
}

pub async fn fetch_source(
    pool: &PgPool,
    slug: &str,
    project_id: &Uuid,
    issue_id: &Uuid,
) -> Result<SourceIssue, Error> {
    let row = sqlx::query(
        r#"SELECT "issues"."id", "issues"."project_id", "issues"."workspace_id",
                  "projects"."identifier", "issues"."sequence_id"
           FROM "issues"
           LEFT OUTER JOIN "states" ON ("issues"."state_id" = "states"."id")
           INNER JOIN "projects" ON ("issues"."project_id" = "projects"."id")
           INNER JOIN "workspaces" ON ("issues"."workspace_id" = "workspaces"."id")
           WHERE ("issues"."deleted_at" IS NULL
             AND NOT ("states"."group" = 'triage' AND "states"."group" IS NOT NULL)
             AND NOT ("issues"."archived_at" IS NOT NULL)
             AND NOT ("projects"."archived_at" IS NOT NULL)
             AND NOT ("issues"."is_draft")
             AND "workspaces"."slug" = $1
             AND "issues"."project_id" = $2
             AND "issues"."id" = $3)"#,
    )
    .bind(slug)
    .bind(project_id)
    .bind(issue_id)
    .fetch_optional(pool)
    .await
    .map_err(|error| db_error(error, "relation-source"))?;
    let Some(row) = row else {
        return Err(Error::NotFound);
    };
    Ok(SourceIssue {
        id: read::row_uuid(&row, "id").map_err(map_read_error)?,
        project_id: read::row_uuid(&row, "project_id").map_err(map_read_error)?,
        workspace_id: read::row_uuid(&row, "workspace_id").map_err(map_read_error)?,
        project_identifier: read::row_string(&row, "identifier").map_err(map_read_error)?,
        sequence_id: row.try_get("sequence_id").map_err(|_| Error::ServerError)?,
    })
}

/// `_check_targets` (`relations.py:166-177`): self-relation and
/// cross-workspace refusals, then first-seen dedupe. The refusals fire
/// before the dedupe check, so a duplicate self-ref still 400s.
fn check_targets(
    source: &SourceIssue,
    targets: Vec<ResolvedIssue>,
) -> Result<Vec<ResolvedIssue>, Error> {
    let mut unique = Vec::with_capacity(targets.len());
    let mut seen = std::collections::HashSet::new();
    for target in targets {
        if target.id == source.id {
            return Err(Error::BadError(format!(
                "{} cannot be related to itself",
                source.identifier()
            )));
        }
        if target.workspace_id != source.workspace_id {
            return Err(Error::BadError(format!(
                "{} is in a different workspace",
                target.identifier()
            )));
        }
        if seen.insert(target.id) {
            unique.push(target);
        }
    }
    Ok(unique)
}

/// `get_inverse_relation` (`utils/issue_relation_mapper.py:5-16`):
/// unknown wires fall through unchanged.
fn inverse_relation(relation_type: &str) -> &str {
    match relation_type {
        "start_after" => "start_before",
        "finish_after" => "finish_before",
        "blocked_by" => "blocking",
        "blocking" => "blocked_by",
        "start_before" => "start_after",
        "finish_before" => "finish_after",
        "implemented_by" => "implements",
        "implements" => "implemented_by",
        other => other,
    }
}

/// `_stored_edge` (`relations.py:92-97`): `(issue_id, related_issue_id,
/// stored_type)` for "source \<type\> target".
fn stored_edge(source_id: Uuid, relation_type: &str, target_id: Uuid) -> (Uuid, Uuid, String) {
    let stored = actual_relation(relation_type).to_owned();
    if AGENT_REVERSE_TYPES.contains(&relation_type) {
        (target_id, source_id, stored)
    } else {
        (source_id, target_id, stored)
    }
}

/// One live pair row for viewpoint classification.
struct PairRow {
    id: Uuid,
    issue_id: Uuid,
    related_issue_id: Uuid,
    relation_type: String,
}

/// `_type_from` (`relations.py:100-114`): the relation a row expresses,
/// named from the viewpoint's side. Rows stored under a reverse name
/// (which the UI never writes but older data may hold) normalize first.
fn type_from(row: &PairRow, viewpoint_id: &Uuid) -> String {
    let mut stored = row.relation_type.as_str();
    let (mut issue_id, mut related_id) = (row.issue_id, row.related_issue_id);
    if AGENT_REVERSE_TYPES.contains(&stored) {
        stored = actual_relation(stored);
        std::mem::swap(&mut issue_id, &mut related_id);
    }
    if issue_id == *viewpoint_id {
        stored.to_owned()
    } else {
        inverse_relation(stored).to_owned()
    }
}

/// `_pair_rows` (`relations.py:117-120`): live rows between the two ids in
/// either direction, `Meta.ordering` (`-created_at`).
async fn fetch_pair_rows(pool: &PgPool, a_id: &Uuid, b_id: &Uuid) -> Result<Vec<PairRow>, Error> {
    let rows = sqlx::query(
        r#"SELECT "id", "issue_id", "related_issue_id", "relation_type" FROM "issue_relations"
           WHERE "deleted_at" IS NULL
             AND (("issue_id" = $1 AND "related_issue_id" = $2)
               OR ("issue_id" = $2 AND "related_issue_id" = $1))
           ORDER BY "created_at" DESC"#,
    )
    .bind(a_id)
    .bind(b_id)
    .fetch_all(pool)
    .await
    .map_err(|error| db_error(error, "relation-pair-rows"))?;
    rows.iter()
        .map(|row| {
            Ok(PairRow {
                id: read::row_uuid(row, "id").map_err(map_read_error)?,
                issue_id: read::row_uuid(row, "issue_id").map_err(map_read_error)?,
                related_issue_id: read::row_uuid(row, "related_issue_id")
                    .map_err(map_read_error)?,
                relation_type: read::row_string(row, "relation_type").map_err(map_read_error)?,
            })
        })
        .collect()
}

/// The related-objects sweep payload (`SoftDeleteModel.delete()` calls
/// `.delay(app_label, model_name, pk, using=None)`): positionals
/// `("db", model, pk)`, kwargs `{"using": null}`.
fn soft_delete_sweep(model_name: &str, pk: &str) -> (Vec<Value>, Map<String, Value>) {
    let mut kwargs = Map::with_capacity(1);
    kwargs.insert("using".to_owned(), Value::Null);
    (
        vec![
            Value::String("db".to_owned()),
            Value::String(model_name.to_owned()),
            Value::String(pk.to_owned()),
        ],
        kwargs,
    )
}

/// One grouped item (`relations.py:269-277`).
pub struct GroupedItem {
    pub id: Uuid,
    pub project_identifier: String,
    pub sequence_id: i32,
    pub name: String,
    pub state_name: Option<String>,
    pub state_group: Option<String>,
}

pub fn render_grouped_item(item: &GroupedItem) -> Value {
    let mut out = Map::with_capacity(5);
    out.insert("id".to_owned(), Value::String(item.id.to_string()));
    out.insert(
        "identifier".to_owned(),
        Value::String(format!("{}-{}", item.project_identifier, item.sequence_id)),
    );
    out.insert("name".to_owned(), Value::String(item.name.clone()));
    out.insert(
        "state".to_owned(),
        item.state_name
            .as_deref()
            .map_or(Value::Null, |name| Value::String(name.to_owned())),
    );
    out.insert(
        "state_group".to_owned(),
        item.state_group
            .as_deref()
            .map_or(Value::Null, |group| Value::String(group.to_owned())),
    );
    Value::Object(out)
}

/// `grouped_relations` (`relations.py:280-312`): every live relation of
/// the source, grouped by type from its side, narrowed to the visible
/// pool, sorted by `(project.identifier, sequence_id)`, capped at
/// [`GROUP_LIMIT`] per type. Every type key is always present, in
/// [`RELATION_TYPES`] order.
pub async fn grouped_relations(
    pool: &PgPool,
    slug: &str,
    user_id: &Uuid,
    source: &SourceIssue,
) -> Result<Map<String, Value>, Error> {
    let rows = sqlx::query(
        r#"SELECT "issue_id", "related_issue_id", "relation_type" FROM "issue_relations"
           WHERE "deleted_at" IS NULL
             AND ("issue_id" = $1 OR "related_issue_id" = $1)
             AND "workspace_id" = $2
             AND NOT ("issue_id" = $1 AND "related_issue_id" = $1)"#,
    )
    .bind(source.id)
    .bind(source.workspace_id)
    .fetch_all(pool)
    .await
    .map_err(|error| db_error(error, "grouped-rows"))?;
    let mut other_by_type: HashMap<&str, std::collections::HashSet<Uuid>> = HashMap::new();
    for relation in RELATION_TYPES {
        other_by_type.insert(*relation, std::collections::HashSet::new());
    }
    for row in &rows {
        let issue_id = read::row_uuid(row, "issue_id").map_err(map_read_error)?;
        let related_id = read::row_uuid(row, "related_issue_id").map_err(map_read_error)?;
        let relation_type = read::row_string(row, "relation_type").map_err(map_read_error)?;
        let other = if issue_id == source.id {
            related_id
        } else {
            issue_id
        };
        // The viewpoint probe has no id (unneeded for classification).
        let probe = PairRow {
            id: Uuid::nil(),
            issue_id,
            related_issue_id: related_id,
            relation_type,
        };
        let relation = type_from(&probe, &source.id);
        if let Some(bucket) = other_by_type.get_mut(relation.as_str()) {
            bucket.insert(other);
        }
    }
    let wanted: Vec<Uuid> = other_by_type
        .values()
        .flat_map(|bucket| bucket.iter().copied())
        .collect();
    let mut others: HashMap<Uuid, GroupedItem> = HashMap::new();
    if !wanted.is_empty() {
        let pool_rows = sqlx::query(&format!(
            r#"SELECT "issues"."id", "issues"."name", "issues"."sequence_id",
                      "projects"."identifier" AS "project_identifier",
                      "states"."name" AS "state_name", "states"."group" AS "state_group"
               {VISIBLE_POOL_JOINS}
               WHERE ({VISIBLE_POOL_WHERE})
                 AND "issues"."id" = ANY($3)
                 AND "issues"."workspace_id" = $4"#
        ))
        .bind(slug)
        .bind(user_id)
        .bind(&wanted)
        .bind(source.workspace_id)
        .fetch_all(pool)
        .await
        .map_err(|error| db_error(error, "grouped-pool"))?;
        for row in &pool_rows {
            let item = GroupedItem {
                id: read::row_uuid(row, "id").map_err(map_read_error)?,
                project_identifier: read::row_string(row, "project_identifier")
                    .map_err(map_read_error)?,
                sequence_id: row.try_get("sequence_id").map_err(|_| Error::ServerError)?,
                name: read::row_string(row, "name").map_err(map_read_error)?,
                state_name: read::row_string_opt(row, "state_name").map_err(map_read_error)?,
                state_group: read::row_string_opt(row, "state_group").map_err(map_read_error)?,
            };
            others.insert(item.id, item);
        }
    }
    let mut out = Map::with_capacity(RELATION_TYPES.len());
    for relation in RELATION_TYPES {
        let bucket = other_by_type.get(*relation).expect("seeded types");
        let mut items: Vec<&GroupedItem> = bucket.iter().filter_map(|id| others.get(id)).collect();
        items.sort_by(|a, b| {
            (&a.project_identifier, a.sequence_id).cmp(&(&b.project_identifier, b.sequence_id))
        });
        items.truncate(GROUP_LIMIT);
        out.insert(
            (*relation).to_owned(),
            Value::Array(items.iter().map(|item| render_grouped_item(item)).collect()),
        );
    }
    Ok(out)
}

/// The `_write` refs read (`views/issue.py:3167-3174`): `request.data.get`
/// (last-wins for forms — hence the scalar body spec, NOT `getlist`),
/// a bare string wraps to one element, anything else must be a non-empty
/// list. A non-object JSON body 500s: `.get` on it raises
/// `AttributeError` (same wart class as REL-3).
fn write_refs(body: &Value) -> Result<Vec<Value>, Error> {
    let Value::Object(map) = body else {
        return Err(Error::ServerError);
    };
    match map.get("issues") {
        Some(Value::String(text)) => Ok(vec![Value::String(text.clone())]),
        Some(Value::Array(items)) if !items.is_empty() => Ok(items.clone()),
        _ => Err(Error::BadError(
            "issues must be a non-empty list of work item identifiers or UUIDs".to_owned(),
        )),
    }
}

/// The relate/unrelate activity kwargs (`relations.py:123-135`): the
/// 7-key [`work_tasks::issue_activity_kwargs`] order plus trailing
/// `notification` — and NO `origin` (unlike the endpoint POST).
fn agent_activity_kwargs(
    activity_type: &str,
    requested_data: &str,
    actor_id: &str,
    issue_id: &str,
    project_id: &str,
    current_instance: Option<&str>,
    epoch: i64,
) -> Map<String, Value> {
    let mut kwargs = work_tasks::issue_activity_kwargs(
        activity_type,
        Some(requested_data),
        actor_id,
        issue_id,
        project_id,
        current_instance,
        epoch,
    );
    kwargs.insert("notification".to_owned(), Value::Bool(true));
    kwargs
}

/// `relate` (`relations.py:180-232`): record "source \<type\> each
/// target", idempotent. Returns the response map minus `relations` (the
/// caller appends the grouped read).
async fn relate_op(
    deps: &Deps,
    source: &SourceIssue,
    relation_type: &str,
    targets: Vec<ResolvedIssue>,
    actor_id: &Uuid,
    emit_activity: bool,
) -> Result<Map<String, Value>, Error> {
    let pool = &deps.pool;
    let targets = check_targets(source, targets)?;
    let mut created: Vec<&ResolvedIssue> = Vec::new();
    let mut unchanged: Vec<String> = Vec::new();
    let mut conflicts: Vec<Value> = Vec::new();
    for target in &targets {
        let (issue_id, related_id, stored) = stored_edge(source.id, relation_type, target.id);
        let existing = fetch_pair_rows(pool, &source.id, &target.id).await?;
        if existing.is_empty() {
            // `objects.create()` in `transaction.atomic()` (a single
            // statement is atomic anyway): the explicit
            // `created_by`/`updated_by` pass through `BaseModel.save`,
            // whose adding branch keeps `created_by` but NULLS
            // `updated_by` (REL-5). A concurrent writer's unique
            // violation re-reads below (Python's `IntegrityError`
            // catch does the same).
            let row = relation_domain::RelationRow {
                id: Uuid::new_v4(),
                created_at: now_utc(),
                updated_at: now_utc(),
                created_by: Some(*actor_id),
                updated_by: None,
                project_id: source.project_id,
                workspace_id: source.workspace_id,
                issue_id,
                related_issue_id: related_id,
                relation_type: &stored,
            };
            match relation_domain::relate_row_or_nothing(pool, &row).await {
                Ok(true) => {
                    created.push(target);
                    continue;
                }
                Ok(false) => {}
                Err(error) if is_unique_violation(&error) => {}
                Err(error) => return Err(db_error(error, "relate-insert")),
            }
        }
        let existing = if existing.is_empty() {
            fetch_pair_rows(pool, &source.id, &target.id).await?
        } else {
            existing
        };
        let current: std::collections::BTreeSet<String> = existing
            .iter()
            .map(|row| type_from(row, &source.id))
            .collect();
        if current.len() == 1 && current.contains(relation_type) {
            unchanged.push(target.identifier());
        } else {
            let first = current.iter().next().cloned().unwrap_or_default();
            let mut conflict = Map::with_capacity(2);
            conflict.insert("identifier".to_owned(), Value::String(target.identifier()));
            conflict.insert("existing_relation".to_owned(), Value::String(first));
            conflicts.push(Value::Object(conflict));
        }
    }
    if !created.is_empty() {
        let mut requested = Map::with_capacity(2);
        requested.insert(
            "relation_type".to_owned(),
            Value::String(relation_type.to_owned()),
        );
        requested.insert(
            "issues".to_owned(),
            Value::Array(
                created
                    .iter()
                    .map(|target| Value::String(target.id.to_string()))
                    .collect(),
            ),
        );
        let dumps = pidash_compat::py::django_dumps;
        let kwargs = agent_activity_kwargs(
            work_tasks::ACTIVITY_RELATION_CREATED,
            &dumps(&Value::Object(requested)),
            &actor_id.to_string(),
            &source.id.to_string(),
            &source.project_id.to_string(),
            None,
            Utc::now().timestamp(),
        );
        if emit_activity {
            deps.enqueue
                .enqueue_task(work_tasks::ISSUE_ACTIVITY_TASK, vec![], kwargs)
                .await;
        }
    }
    let mut out = Map::with_capacity(5);
    out.insert("issue".to_owned(), Value::String(source.identifier()));
    out.insert(
        "relation_type".to_owned(),
        Value::String(relation_type.to_owned()),
    );
    out.insert(
        "created".to_owned(),
        Value::Array(
            created
                .iter()
                .map(|target| Value::String(target.identifier()))
                .collect(),
        ),
    );
    out.insert(
        "unchanged".to_owned(),
        Value::Array(
            unchanged
                .iter()
                .map(|ident| Value::String(ident.clone()))
                .collect(),
        ),
    );
    out.insert("conflicts".to_owned(), Value::Array(conflicts));
    Ok(out)
}

/// `unrelate` (`relations.py:235-266`): remove "source \<type\> each
/// target", idempotent — only exactly that type is removed. Returns the
/// response map minus `relations`. With `newest_only`, only the first
/// match is removed: [`fetch_pair_rows`] returns newest-first (the
/// model's `-created_at` ordering), so that is the row the MCP
/// queryset's `.first()` deletes.
async fn unrelate_op(
    deps: &Deps,
    source: &SourceIssue,
    relation_type: &str,
    targets: Vec<ResolvedIssue>,
    actor_id: &Uuid,
    emit_activity: bool,
    newest_only: bool,
) -> Result<Map<String, Value>, Error> {
    let pool = &deps.pool;
    let targets = check_targets(source, targets)?;
    let mut removed: Vec<String> = Vec::new();
    let mut not_related: Vec<String> = Vec::new();
    for target in &targets {
        let rows = fetch_pair_rows(pool, &source.id, &target.id).await?;
        let mut matching: Vec<&PairRow> = rows
            .iter()
            .filter(|row| type_from(row, &source.id) == relation_type)
            .collect();
        if matching.is_empty() {
            not_related.push(target.identifier());
            continue;
        }
        // `row.delete()`: `deleted_at = now`, full `save()` (`updated_at`
        // re-stamped, `updated_by` = actor per REL-5), then the sweep.
        // The filter keeps `fetch_pair_rows` order (newest first), so
        // truncating to one keeps the MCP `.first()` row.
        if newest_only {
            matching.truncate(1);
        }
        for row in matching {
            relation_domain::soft_delete_relation_row(
                pool,
                row.id,
                *actor_id,
                now_utc(),
                now_utc(),
            )
            .await
            .map_err(|error| db_error(error, "unrelate-delete"))?;
            let (sweep_args, sweep_kwargs) =
                soft_delete_sweep("issuerelation", &row.id.to_string());
            deps.enqueue
                .enqueue_task(SOFT_DELETE_TASK, sweep_args, sweep_kwargs)
                .await;
        }
        removed.push(target.identifier());
        // One activity per removed target (inside the loop).
        let mut requested = Map::with_capacity(2);
        requested.insert(
            "relation_type".to_owned(),
            Value::String(relation_type.to_owned()),
        );
        requested.insert(
            "related_issue".to_owned(),
            Value::String(target.id.to_string()),
        );
        let mut current = Map::with_capacity(1);
        current.insert(
            "relation_type".to_owned(),
            Value::String(relation_type.to_owned()),
        );
        let dumps = pidash_compat::py::django_dumps;
        let kwargs = agent_activity_kwargs(
            "issue_relation.activity.deleted",
            &dumps(&Value::Object(requested)),
            &actor_id.to_string(),
            &source.id.to_string(),
            &source.project_id.to_string(),
            Some(&dumps(&Value::Object(current))),
            Utc::now().timestamp(),
        );
        if emit_activity {
            deps.enqueue
                .enqueue_task(work_tasks::ISSUE_ACTIVITY_TASK, vec![], kwargs)
                .await;
        }
    }
    let mut out = Map::with_capacity(4);
    out.insert("issue".to_owned(), Value::String(source.identifier()));
    out.insert(
        "relation_type".to_owned(),
        Value::String(relation_type.to_owned()),
    );
    out.insert(
        "removed".to_owned(),
        Value::Array(
            removed
                .iter()
                .map(|ident| Value::String(ident.clone()))
                .collect(),
        ),
    );
    out.insert(
        "not_related".to_owned(),
        Value::Array(
            not_related
                .iter()
                .map(|ident| Value::String(ident.clone()))
                .collect(),
        ),
    );
    Ok(out)
}

/// `_write` (`views/issue.py:3163-3194`) over `relate` / `unrelate`:
/// source first (404 before any body read), then refs shape, relation
/// type, resolve, the operation, and the trailing grouped read.
/// The shared relate/unrelate core (`_write`,
/// `views/issue.py:3163-3194`): refs shape, relation type, resolve, the
/// operation, and the trailing grouped read.
///
/// The caller already ran auth, the gate and body negotiation, and
/// pre-fetched `write.source` before parsing the body, so a bogus issue
/// 404s before a malformed body 400s (`:3165-3167`). `newest_only` is
/// the unrelate leg's option only; the relate leg ignores it.
async fn relate_shared(
    deps: &Deps,
    ctx: &WriteCtx,
    write: &RelationWrite<'_>,
    emit_activity: bool,
    is_relate: bool,
    newest_only: bool,
) -> Result<Outcome, Error> {
    let pool = &deps.pool;
    let slug = write.slug;
    let source = write.source;
    let actor_id = ctx.actor.user_id;
    let body = write.body;
    let refs = write_refs(body)?;
    let relation_type =
        validate_relation_type(body.as_object().and_then(|map| map.get("relation_type")))?;
    let (targets, unresolved) = resolve_refs(pool, slug, &actor_id, &refs).await?;
    if !unresolved.is_empty() {
        let mut out = Map::with_capacity(2);
        out.insert(
            "error".to_owned(),
            Value::String(format!(
                "work items not found or not accessible: {}",
                unresolved.join(", ")
            )),
        );
        out.insert(
            "unresolved".to_owned(),
            Value::Array(
                unresolved
                    .iter()
                    .map(|text| Value::String(text.clone()))
                    .collect(),
            ),
        );
        let body = serde_json::to_string(&out).map_err(|_| Error::ServerError)?;
        return Err(Error::NotFoundBody(body));
    }
    let mut out = if is_relate {
        relate_op(
            deps,
            source,
            &relation_type,
            targets,
            &actor_id,
            emit_activity,
        )
        .await?
    } else {
        unrelate_op(
            deps,
            source,
            &relation_type,
            targets,
            &actor_id,
            emit_activity,
            newest_only,
        )
        .await?
    };
    let relations = grouped_relations(pool, slug, &actor_id, source).await?;
    out.insert("relations".to_owned(), Value::Object(relations));
    Ok(Outcome::ok(
        serde_json::to_string(&out).map_err(|_| Error::ServerError)?,
    ))
}

/// The pre-resolved relate/unrelate scope: auth, the gate and body
/// negotiation already ran; `source` is the live issue the handler
/// fetched before parsing the body; `body` is the negotiated payload.
/// Forms collapse repeated keys last-wins at parse (the scalar agent
/// spec), so no `from_form` flag travels: nothing downstream branches
/// on it.
pub struct RelationWrite<'a> {
    /// The workspace slug from the route.
    pub slug: &'a str,
    /// The pre-fetched source issue (the handler 404s before parsing).
    pub source: &'a SourceIssue,
    /// The negotiated payload.
    pub body: &'a Value,
}

/// Per-caller options for [`relate`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RelateOptions {
    /// Emit the `issue_activity` created leg. The v1 view passes `true`;
    /// callers that enqueue their own shaped activity (the MCP tools)
    /// pass `false`.
    pub emit_activity: bool,
}

/// Per-caller options for [`unrelate`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UnrelateOptions {
    /// Emit the per-target `issue_activity` deleted legs (see
    /// [`RelateOptions`]). The related-objects sweep enqueues always
    /// run: they are work, not activity.
    pub emit_activity: bool,
    /// Delete only the newest matching row per target instead of every
    /// matching row (design D6: the v1 agent endpoint deletes all —
    /// `orchestration/relations.py:251-252` via
    /// `views/issue.py:3250-3253` — while the MCP tool deletes the
    /// queryset `.first()` only — `pi_dash_cloud/mcp/tools.py:729-732`
    /// — under the same `Meta.ordering` (`-created_at`) that
    /// [`fetch_pair_rows`] uses, so the first match is the newest row
    /// on both sides). The v1 handler passes `false`; MCP passes
    /// `true`.
    pub newest_only: bool,
}

/// `POST .../relations/relate/` (`views/issue.py:3232-3235`): record
/// "source \<type\> each target", idempotent, plus the trailing
/// grouped read.
pub async fn relate(
    deps: &Deps,
    ctx: &WriteCtx,
    write: &RelationWrite<'_>,
    options: RelateOptions,
) -> Result<Outcome, Error> {
    relate_shared(deps, ctx, write, options.emit_activity, true, false).await
}

/// `POST .../relations/unrelate/` (`views/issue.py:3250-3253`): remove
/// "source \<type\> each target", idempotent, plus the trailing
/// grouped read.
pub async fn unrelate(
    deps: &Deps,
    ctx: &WriteCtx,
    write: &RelationWrite<'_>,
    options: UnrelateOptions,
) -> Result<Outcome, Error> {
    relate_shared(
        deps,
        ctx,
        write,
        options.emit_activity,
        false,
        options.newest_only,
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relation_type_validation_matrix() {
        assert_eq!(
            validate_relation_type(Some(&Value::String("blocking".to_owned()))).expect("valid"),
            "blocking"
        );
        assert_eq!(
            validate_relation_type(Some(&Value::String("  BLOCKING  ".to_owned())))
                .expect("strip+lower"),
            "blocking"
        );
        // Python `str.strip()` also strips U+001C–U+001F, which Rust
        // `char::is_whitespace` excludes.
        assert_eq!(
            validate_relation_type(Some(&Value::String("blocking\u{1c}".to_owned()))).expect("fs"),
            "blocking"
        );
        assert_eq!(py_stripped("a"), "a");
        assert_eq!(
            validate_relation_type(None).expect_err("missing"),
            Error::BadError(format!(
                "relation_type must be one of: {}",
                RELATION_TYPES.join(", ")
            )),
        );
        assert_eq!(
            validate_relation_type(Some(&Value::String("nope".to_owned()))).expect_err("unknown"),
            Error::BadError(format!(
                "relation_type must be one of: {}",
                RELATION_TYPES.join(", ")
            )),
        );
        // Falsy non-strings take the "" arm → 400 ...
        for raw in [
            Value::Null,
            Value::Bool(false),
            Value::Number(0.into()),
            Value::String(String::new()),
            Value::Array(vec![]),
            Value::Object(Map::new()),
        ] {
            assert!(
                matches!(validate_relation_type(Some(&raw)), Err(Error::BadError(_))),
                "{raw:?} 400s"
            );
        }
        // ... while truthy non-strings 500 (REL-3).
        for raw in [
            Value::Bool(true),
            Value::Number(5.into()),
            Value::Number(serde_json::Number::from_f64(1.5).expect("f64")),
            Value::Array(vec![Value::String("blocking".to_owned())]),
            Value::Object({
                let mut map = Map::new();
                map.insert("a".to_owned(), Value::Null);
                map
            }),
        ] {
            assert_eq!(
                validate_relation_type(Some(&raw)),
                Err(Error::ServerError),
                "{raw:?} 500s"
            );
        }
    }

    #[test]
    fn python_uuid_acceptance() {
        let hyphenated = "25ace52e-c64d-4043-a700-911b1e42bffc";
        let expected: Uuid = hyphenated.parse().expect("uuid");
        // Canonical forms.
        assert_eq!(parse_python_uuid(hyphenated), Some(expected));
        assert_eq!(
            parse_python_uuid("25ACE52E-C64D-4043-A700-911B1E42BFFC"),
            Some(expected)
        );
        assert_eq!(
            parse_python_uuid("25ace52ec64d4043a700911b1e42bffc"),
            Some(expected)
        );
        assert_eq!(
            parse_python_uuid("{25ace52e-c64d-4043-a700-911b1e42bffc}"),
            Some(expected)
        );
        assert_eq!(
            parse_python_uuid("urn:uuid:25ace52e-c64d-4043-a700-911b1e42bffc"),
            Some(expected)
        );
        // Pathological spellings CPython accepts (misplaced hyphens,
        // unbalanced braces, underscores).
        assert_eq!(
            parse_python_uuid("25ace52ec-64d-4043a700-911b1e42bffc"),
            Some(expected)
        );
        assert_eq!(
            parse_python_uuid("{{25ace52e-c64d-4043-a700-911b1e42bffc}"),
            Some(expected)
        );
        assert_eq!(
            parse_python_uuid("25ace52e_c64d4043a700911b1e42bffc"),
            Some(expected)
        );
        // Rejects: wrong length, bad hex, doubled/edge underscores,
        // uppercase URN prefix (case-sensitive strip), identifiers.
        for bad in [
            "25ace52e-c64d-4043-a700-911b1e42bff",
            "zzace52e-c64d-4043-a700-911b1e42bffc",
            "25ace52e__c64d4043a700911b1e42bffc",
            "_5ace52ec64d4043a700911b1e42bffc1",
            "URN:UUID:25ace52e-c64d-4043-a700-911b1e42bffc",
            "CT00003-1",
            "",
        ] {
            assert_eq!(parse_python_uuid(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn identifier_ref_splitting() {
        assert_eq!(
            split_identifier_ref("CT00003-1").expect("ok"),
            Some(("CT00003".to_owned(), 1))
        );
        assert_eq!(
            split_identifier_ref("A-B-12").expect("ok"),
            Some(("A-B".to_owned(), 12))
        );
        assert_eq!(split_identifier_ref("CT00003").expect("ok"), None);
        assert_eq!(split_identifier_ref("-12").expect("ok"), None);
        assert_eq!(split_identifier_ref("CT-").expect("ok"), None);
        assert_eq!(split_identifier_ref("CT-1x").expect("ok"), None);
        // Overflowing tails match nothing.
        assert_eq!(
            split_identifier_ref("CT-99999999999999999999999").expect("ok"),
            None
        );
        // Numeric-but-not-decimal tails 500 (REL-4).
        assert_eq!(split_identifier_ref("CT-²"), Err(Error::ServerError));
    }

    #[test]
    fn python_str_matrix() {
        assert_eq!(python_str(&Value::Null), "None");
        assert_eq!(python_str(&Value::Bool(true)), "True");
        assert_eq!(python_str(&Value::Bool(false)), "False");
        assert_eq!(python_str(&Value::Number(5.into())), "5");
        assert_eq!(
            python_str(&Value::Number(
                serde_json::Number::from_f64(1.5).expect("f64")
            )),
            "1.5"
        );
        assert_eq!(python_str(&Value::String("x".to_owned())), "x");
        assert_eq!(python_str(&Value::Array(vec![])), "[]");
        assert_eq!(python_str(&Value::Object(Map::new())), "{}");
        assert_eq!(python_str(&serde_json::json!({"a": 1})), "{'a': 1}");
        assert_eq!(python_ref_text(&Value::Null), "");
        assert_eq!(python_ref_text(&Value::Number(0.into())), "");
        assert_eq!(python_ref_text(&Value::Number(7.into())), "7");
    }

    #[test]
    fn write_refs_matrix() {
        // Bare strings wrap.
        assert_eq!(
            write_refs(&(serde_json::json!({"issues": "CT-1"}))).expect("wrap"),
            vec![Value::String("CT-1".to_owned())]
        );
        // Non-empty lists pass through.
        assert_eq!(
            write_refs(&(serde_json::json!({"issues": ["a", "b"]}))).expect("list"),
            vec![Value::String("a".to_owned()), Value::String("b".to_owned())]
        );
        // Missing / wrong shapes 400 ...
        for body in [
            serde_json::json!({}),
            serde_json::json!({"issues": []}),
            serde_json::json!({"issues": 5}),
            serde_json::json!({"issues": {"a": 1}}),
            serde_json::json!({"issues": null}),
        ] {
            assert_eq!(
                write_refs(&(body)),
                Err(Error::BadError(
                    "issues must be a non-empty list of work item identifiers or UUIDs".to_owned()
                )),
            );
        }
        // ... while non-object bodies 500 (`.get` wart).
        for body in [
            Value::Array(vec![]),
            Value::String("x".to_owned()),
            Value::Number(5.into()),
            Value::Null,
        ] {
            assert_eq!(write_refs(&(body)), Err(Error::ServerError));
        }
    }

    #[test]
    fn stored_edge_and_inverse_matrix() {
        let source = Uuid::parse_str("11111111-1111-1111-1111-111111111111").expect("uuid");
        let target = Uuid::parse_str("22222222-2222-2222-2222-222222222222").expect("uuid");
        // Forward wires keep sides; reverse wires swap.
        assert_eq!(
            stored_edge(source, "blocked_by", target),
            (source, target, "blocked_by".to_owned())
        );
        assert_eq!(
            stored_edge(source, "blocking", target),
            (target, source, "blocked_by".to_owned())
        );
        assert_eq!(
            stored_edge(source, "implements", target),
            (target, source, "implemented_by".to_owned())
        );
        assert_eq!(
            stored_edge(source, "relates_to", target),
            (source, target, "relates_to".to_owned())
        );
        assert_eq!(inverse_relation("blocked_by"), "blocking");
        assert_eq!(inverse_relation("blocking"), "blocked_by");
        assert_eq!(inverse_relation("implements"), "implemented_by");
        assert_eq!(inverse_relation("duplicate"), "duplicate");
        assert_eq!(inverse_relation("bogus"), "bogus");
    }

    #[test]
    fn type_from_matrix() {
        let viewpoint = Uuid::parse_str("11111111-1111-1111-1111-111111111111").expect("uuid");
        let other = Uuid::parse_str("22222222-2222-2222-2222-222222222222").expect("uuid");
        let row = |issue: Uuid, related: Uuid, stored: &str| PairRow {
            id: Uuid::nil(),
            issue_id: issue,
            related_issue_id: related,
            relation_type: stored.to_owned(),
        };
        // Stored forward, viewed from the issue side.
        assert_eq!(
            type_from(&row(viewpoint, other, "blocked_by"), &viewpoint),
            "blocked_by"
        );
        // Same row from the other side inverts.
        assert_eq!(
            type_from(&row(viewpoint, other, "blocked_by"), &other),
            "blocking"
        );
        // Symmetric types read the same both ways.
        assert_eq!(
            type_from(&row(viewpoint, other, "relates_to"), &other),
            "relates_to"
        );
        // Legacy reverse-stored rows normalize (swap + actual).
        assert_eq!(
            type_from(&row(viewpoint, other, "blocking"), &other),
            "blocked_by"
        );
        assert_eq!(
            type_from(&row(viewpoint, other, "blocking"), &viewpoint),
            "blocking"
        );
    }

    #[test]
    fn check_targets_matrix() {
        let workspace = Uuid::parse_str("33333333-3333-3333-3333-333333333333").expect("uuid");
        let source = SourceIssue {
            id: Uuid::parse_str("11111111-1111-1111-1111-111111111111").expect("uuid"),
            project_id: Uuid::nil(),
            workspace_id: workspace,
            project_identifier: "CT".to_owned(),
            sequence_id: 1,
        };
        let target = |id: &str, ws: Uuid| ResolvedIssue {
            id: id.parse().expect("uuid"),
            workspace_id: ws,
            project_identifier: "CT".to_owned(),
            sequence_id: 2,
        };
        // Dedupe preserves first-seen order.
        let kept = check_targets(
            &source,
            vec![
                target("22222222-2222-2222-2222-222222222222", workspace),
                target("22222222-2222-2222-2222-222222222222", workspace),
            ],
        )
        .expect("dedupe");
        assert_eq!(kept.len(), 1);
        // Self-relation 400s, even duplicated.
        assert_eq!(
            check_targets(
                &source,
                vec![target("11111111-1111-1111-1111-111111111111", workspace)],
            ),
            Err(Error::BadError(
                "CT-1 cannot be related to itself".to_owned()
            )),
        );
        // Cross-workspace 400s.
        let elsewhere = Uuid::parse_str("44444444-4444-4444-4444-444444444444").expect("uuid");
        assert!(matches!(
            check_targets(&source, vec![target(
                "22222222-2222-2222-2222-222222222222",
                elsewhere
            )]),
            Err(Error::BadError(message)) if message.ends_with("is in a different workspace"),
        ));
    }

    #[test]
    fn sweep_and_agent_kwargs_shapes() {
        let (args, kwargs) = soft_delete_sweep("issuerelation", "abc");
        assert_eq!(
            args,
            vec![
                Value::String("db".to_owned()),
                Value::String("issuerelation".to_owned()),
                Value::String("abc".to_owned()),
            ]
        );
        assert_eq!(kwargs.get("using"), Some(&Value::Null));
        let kwargs = agent_activity_kwargs("t", "r", "a", "i", "p", None, 7);
        let keys: Vec<&str> = kwargs.keys().map(String::as_str).collect();
        assert_eq!(
            keys,
            vec![
                "type",
                "requested_data",
                "actor_id",
                "issue_id",
                "project_id",
                "current_instance",
                "epoch",
                "notification",
            ]
        );
    }

    // --- Live-DB unrelate selection tests (PIDASHCONV-1653) ---

    use crate::domain::activity::TaskEnqueueHook;
    use crate::domain::transition::IssueTransitionHook;
    use crate::orchestration::entries::{FireOutcome, FireRequest};
    use std::sync::{Arc, Mutex};

    type EnqueueCall = (String, Vec<Value>, Map<String, Value>);

    struct RecordingEnqueue {
        calls: Mutex<Vec<EnqueueCall>>,
    }

    impl TaskEnqueueHook for RecordingEnqueue {
        fn enqueue_task(
            &self,
            task: &str,
            args: Vec<Value>,
            kwargs: Map<String, Value>,
        ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send + '_>> {
            self.calls
                .lock()
                .expect("calls lock")
                .push((task.to_owned(), args, kwargs));
            Box::pin(async move {})
        }
    }

    struct NoopTransition;

    impl IssueTransitionHook for NoopTransition {
        fn enqueue_completion_comment(
            &self,
            _issue_id: Uuid,
            _prev_state_id: Option<Uuid>,
            _current_state_id: Option<Uuid>,
        ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send + '_>> {
            Box::pin(async move {})
        }

        fn fire(
            &self,
            _actor_id: Uuid,
            _request: FireRequest,
        ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<FireOutcome, ()>> + Send + '_>>
        {
            Box::pin(async move { Ok(FireOutcome::NoTransition) })
        }
    }

    async fn live_pool() -> Option<sqlx::PgPool> {
        let url = match std::env::var("DATABASE_URL") {
            Ok(url) => url,
            Err(_) => {
                eprintln!("skipping live-db test: DATABASE_URL is not set");
                return None;
            }
        };
        Some(
            sqlx::postgres::PgPoolOptions::new()
                .max_connections(2)
                .connect(&url)
                .await
                .expect("connect to scratch DATABASE_URL"),
        )
    }

    struct LiveScope {
        actor: Uuid,
        workspace_id: Uuid,
        project_id: Uuid,
        project_identifier: String,
        source_id: Uuid,
        source_seq: i32,
        target_id: Uuid,
        target_seq: i32,
    }

    impl LiveScope {
        fn source(&self) -> SourceIssue {
            SourceIssue {
                id: self.source_id,
                project_id: self.project_id,
                workspace_id: self.workspace_id,
                project_identifier: self.project_identifier.clone(),
                sequence_id: self.source_seq,
            }
        }

        fn targets(&self) -> Vec<ResolvedIssue> {
            vec![ResolvedIssue {
                id: self.target_id,
                workspace_id: self.workspace_id,
                project_identifier: self.project_identifier.clone(),
                sequence_id: self.target_seq,
            }]
        }

        fn target_identifier(&self) -> String {
            format!("{}-{}", self.project_identifier, self.target_seq)
        }
    }

    async fn seed_scope(pool: &sqlx::PgPool, tag: &str) -> LiveScope {
        let actor = Uuid::new_v4();
        let workspace_id = Uuid::new_v4();
        let project_id = Uuid::new_v4();
        let state_id = Uuid::new_v4();
        let source_id = Uuid::new_v4();
        let target_id = Uuid::new_v4();
        let username = format!("ur-{tag}-{}", actor.simple());
        sqlx::query(
            "INSERT INTO users (id, password, username, email, first_name, last_name, avatar, \
             date_joined, created_at, updated_at, last_location, created_location, is_superuser, \
             is_managed, is_password_expired, is_active, is_staff, is_email_verified, \
             is_password_autoset, token, user_timezone, last_login_ip, last_logout_ip, \
             last_login_medium, last_login_uagent, is_bot, display_name, is_email_valid, \
             is_password_reset_required) VALUES ($1, '!', $2, $3, 'CT', 'User', '', now(), now(), \
             now(), '', '', false, false, false, true, false, true, false, $4, 'UTC', '', '', '', \
             '', false, $2, true, false)",
        )
        .bind(actor)
        .bind(username.clone())
        .bind(format!("{username}@example.test"))
        .bind("t".repeat(64))
        .execute(pool)
        .await
        .expect("seed user");
        sqlx::query(
            "INSERT INTO workspaces (id, created_at, updated_at, name, slug, timezone, \
             background_color, owner_id) VALUES ($1, now(), now(), 'ur ws', $2, 'UTC', '#ffffff', \
             $3)",
        )
        .bind(workspace_id)
        .bind(format!("ur-{tag}-{}", workspace_id.simple()))
        .bind(actor)
        .execute(pool)
        .await
        .expect("seed workspace");
        let project_identifier = format!("U{:07X}", (project_id.as_u128() & 0x0fff_ffff) as u32);
        sqlx::query(
            "INSERT INTO projects (id, name, description, network, identifier, workspace_id, \
             created_by_id, cycle_view, module_view, issue_views_view, page_view, intake_view, \
             archive_in, close_in, logo_props, is_time_tracking_enabled, is_issue_type_enabled, \
             guest_view_all_features, timezone, members_can_edit_states, repo_url, base_branch, \
             agent_default_interval_seconds, agent_default_max_ticks, agent_ticking_enabled, \
             is_default, agent_review_default_interval_seconds, default_agent_executor, \
             agent_test_default_interval_seconds, created_at, updated_at) VALUES ($1, \
             $2, '', 0, $3, $4, $5, true, true, true, true, false, 0, 0, '{}', true, \
             true, false, 'UTC', false, '', '', 0, 0, false, false, 0, '', 0, now(), now())",
        )
        .bind(project_id)
        .bind(format!("ur-project-{project_identifier}"))
        .bind(project_identifier.clone())
        .bind(workspace_id)
        .bind(actor)
        .execute(pool)
        .await
        .expect("seed project");
        sqlx::query(
            "INSERT INTO states (id, created_at, updated_at, name, description, color, slug, \
             project_id, workspace_id, sequence, \"group\", \"default\", is_triage) VALUES \
             ($1, now(), now(), 'Backlog', '', '#ff0000', $2, $3, $4, 100.0, 'backlog', true, false)",
        )
        .bind(state_id)
        .bind(format!("ur-backlog-{state_id}"))
        .bind(project_id)
        .bind(workspace_id)
        .execute(pool)
        .await
        .expect("seed state");
        for (id, name, sequence_id) in [(source_id, "ur source", 11), (target_id, "ur target", 12)]
        {
            sqlx::query(
                "INSERT INTO issues (id, created_at, updated_at, project_id, workspace_id, \
                 state_id, name, description_json, description_html, sequence_id, sort_order, \
                 priority, complexity_score, is_draft, git_work_branch, workpad) VALUES ($1, \
                 now(), now(), $2, $3, $4, $5, '{}', '<p>ur</p>', $6, 10000.0, 'none', 0, false, \
                 '', '')",
            )
            .bind(id)
            .bind(project_id)
            .bind(workspace_id)
            .bind(state_id)
            .bind(name)
            .bind(sequence_id)
            .execute(pool)
            .await
            .expect("seed issue");
        }
        LiveScope {
            actor,
            workspace_id,
            project_id,
            project_identifier,
            source_id,
            source_seq: 11,
            target_id,
            target_seq: 12,
        }
    }

    async fn insert_relation(
        pool: &sqlx::PgPool,
        scope: &LiveScope,
        issue_id: Uuid,
        related_id: Uuid,
        created_at: chrono::DateTime<Utc>,
    ) -> Uuid {
        let id = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO issue_relations (id, created_at, updated_at, created_by_id, \
             updated_by_id, deleted_at, project_id, workspace_id, issue_id, related_issue_id, \
             relation_type) VALUES ($1, $2, $2, $3, NULL, NULL, $4, $5, $6, $7, 'duplicate')",
        )
        .bind(id)
        .bind(created_at)
        .bind(scope.actor)
        .bind(scope.project_id)
        .bind(scope.workspace_id)
        .bind(issue_id)
        .bind(related_id)
        .execute(pool)
        .await
        .expect("insert relation");
        id
    }

    fn unrelate_deps(pool: &sqlx::PgPool) -> (Deps, Arc<RecordingEnqueue>) {
        let enqueue = Arc::new(RecordingEnqueue {
            calls: Mutex::new(Vec::new()),
        });
        let deps = Deps {
            pool: pool.clone(),
            orm: sea_orm::SqlxPostgresConnector::from_sqlx_postgres_pool(pool.clone()),
            transition: Arc::new(NoopTransition),
            enqueue: enqueue.clone(),
        };
        (deps, enqueue)
    }

    async fn deleted_at_of(pool: &sqlx::PgPool, id: Uuid) -> Option<chrono::DateTime<Utc>> {
        let row: (Option<chrono::DateTime<Utc>>,) =
            sqlx::query_as("SELECT deleted_at FROM issue_relations WHERE id = $1")
                .bind(id)
                .fetch_one(pool)
                .await
                .expect("read relation");
        row.0
    }

    fn sweep_pks(calls: &[EnqueueCall]) -> Vec<String> {
        calls
            .iter()
            .filter(|(task, _, _)| task == SOFT_DELETE_TASK)
            .map(|(_, args, _)| {
                assert_eq!(args.len(), 3);
                assert_eq!(args[0], Value::String("db".to_owned()));
                assert_eq!(args[1], Value::String("issuerelation".to_owned()));
                args[2].as_str().expect("pk str").to_owned()
            })
            .collect()
    }

    #[tokio::test]
    async fn live_newest_only_deletes_newest_of_symmetric_pair() {
        let Some(pool) = live_pool().await else {
            return;
        };
        let scope = seed_scope(&pool, "newest").await;
        let now = Utc::now();
        // The MCP suite's symmetric-duplicate pair: the older orientation
        // first, the newer orientation second.
        let older = insert_relation(
            &pool,
            &scope,
            scope.source_id,
            scope.target_id,
            now - chrono::Duration::hours(1),
        )
        .await;
        let newer = insert_relation(&pool, &scope, scope.target_id, scope.source_id, now).await;
        let (deps, enqueue) = unrelate_deps(&pool);
        let out = unrelate_op(
            &deps,
            &scope.source(),
            "duplicate",
            scope.targets(),
            &scope.actor,
            false,
            true,
        )
        .await
        .expect("unrelate newest-only");
        assert_eq!(
            out.get("removed"),
            Some(&Value::Array(vec![Value::String(
                scope.target_identifier()
            )]))
        );
        assert_eq!(out.get("not_related"), Some(&Value::Array(vec![])));
        // Exactly the newest row is gone; the older orientation survives.
        assert!(deleted_at_of(&pool, newer).await.is_some());
        assert!(deleted_at_of(&pool, older).await.is_none());
        let live: (Uuid, Uuid) = sqlx::query_as(
            "SELECT issue_id, related_issue_id FROM issue_relations \
             WHERE id = $1 AND deleted_at IS NULL",
        )
        .bind(older)
        .fetch_one(&pool)
        .await
        .expect("older row live");
        assert_eq!((live.0, live.1), (scope.source_id, scope.target_id));
        // One sweep, for the deleted row; no activity (`emit_activity` off).
        let calls = enqueue.calls.lock().expect("calls lock");
        assert_eq!(sweep_pks(&calls), vec![newer.to_string()]);
        assert_eq!(calls.len(), 1);
    }

    #[tokio::test]
    async fn live_default_deletes_all_matching_rows() {
        let Some(pool) = live_pool().await else {
            return;
        };
        let scope = seed_scope(&pool, "all").await;
        let now = Utc::now();
        let first = insert_relation(
            &pool,
            &scope,
            scope.source_id,
            scope.target_id,
            now - chrono::Duration::hours(1),
        )
        .await;
        let second = insert_relation(&pool, &scope, scope.target_id, scope.source_id, now).await;
        let (deps, enqueue) = unrelate_deps(&pool);
        let out = unrelate_op(
            &deps,
            &scope.source(),
            "duplicate",
            scope.targets(),
            &scope.actor,
            false,
            false,
        )
        .await
        .expect("unrelate default");
        assert_eq!(
            out.get("removed"),
            Some(&Value::Array(vec![Value::String(
                scope.target_identifier()
            )]))
        );
        assert_eq!(out.get("not_related"), Some(&Value::Array(vec![])));
        assert!(deleted_at_of(&pool, first).await.is_some());
        assert!(deleted_at_of(&pool, second).await.is_some());
        // One sweep per deleted row (newest first); no activity.
        let calls = enqueue.calls.lock().expect("calls lock");
        assert_eq!(
            sweep_pks(&calls),
            vec![second.to_string(), first.to_string()]
        );
        assert_eq!(calls.len(), 2);
    }
}
