//! Prompt composition: override resolution into assembled prompts.
//!
//! Port of `apps/api/pi_dash/prompting/composer.py` (514 lines), scoped to
//! the lines this issue owns:
//!
//! * `composer.py:31-36` (source consts) — [`SOURCE_DEFAULT`],
//!   [`SOURCE_WORKSPACE`], [`SOURCE_DRAFT`].
//! * `composer.py:38-82` (`ResolvedSection` / `ManifestEntry` /
//!   `ComposedPrompt`) — [`ResolvedSection`], [`ManifestEntry`],
//!   [`ComposedPrompt`].
//! * `composer.py:84-92` (`effective_customizability`) —
//!   [`effective_customizability`].
//! * `composer.py:100-147` (`load_override_index` / `_lookup_override`) —
//!   [`OverrideRow`], [`build_override_index`]: the services layer holds no
//!   database handle (no `sqlx` in this crate), so the Django ORM query is
//!   split at the seam every sibling port uses — the DB edge issues the
//!   equivalent `WHERE` (see the contract on [`build_override_index`]) and
//!   hands the rows over; filtering, scoping and precedence live here as
//!   pure code with the same semantics. The single-section direct-query
//!   fallback (`_lookup_override` with no index) owns to that DB edge:
//!   callers always pass the preloaded index (`FIX-compose` asserts the two
//!   paths agree: `precedence.index_vs_fallback_same`).
//! * `composer.py:150-201` (`resolve_section`) — [`resolve_section`]. The
//!   `project` parameter is accepted-and-ignored in Python (the §9.4 seam
//!   for a future project-level rung); it is dropped here, not carried as
//!   a dead argument.
//! * `composer.py:204-230` (`_assemble`) — [`assemble`].
//! * `composer.py:233-283` (culprit attribution) — [`PromptComposeError`]:
//!   `renderer::PromptRenderError` has no public constructor (it is only
//!   built at the renderer boundary), so attribution wraps the engine
//!   detail string in this local error type. Messages are identical to
//!   Python; only the Rust type name differs. The Jinja-lineno fast path
//!   (`_find_culprit_section` via `exc.lineno`) cannot cross that
//!   boundary — the detail string carries no line — so attribution runs
//!   the isolation loop (re-render each non-default section alone; the
//!   first failure is the culprit), which is also Python's path for the
//!   common `UndefinedError` case that carries no lineno.
//! * `composer.py:286-321` (`compose`), `324-344` (`compose_cloud`),
//!   `347-364` (`compile_template`) — [`compose`], [`compose_cloud`],
//!   [`compile_template`]. `compose_cloud` drops Python's
//!   accepted-and-ignored `workspace` / `project` parameters.
//! * `composer.py:372-380` (`_user_for_run`) — [`user_id_for_run`]: the
//!   `run_is_human_triggered` predicate owns to the runner layer, so this
//!   takes its boolean result plus the triggering user's id.
//!
//! Out of scope (context layer, PIDASHCONV-142): the turn builders
//! (`composer.py:383+` — `build_first_turn`, `build_first_turn_context`,
//! `build_scheduler_turn`, `build_direct_turn`).
//!
//! Ported bugs (translate, don't redesign — from the `bugs` array of
//! `FIX-compose`): none in this file's lines. The recorded bug (local
//! `build_first_turn` stamps `run.prompt_manifest` as a bare list while
//! the cloud path stamps a versioned dict, `composer.py:398-414`) lives in
//! the turn builders and travels with PIDASHCONV-142.
//!
//! Semantic traps watched: `str.strip()` strips U+001C–U+001F on top of
//! Unicode whitespace (replicated in [`assemble`]); `len(candidate)`
//! counts chars (see `validation.rs`);
//! `if draft_overrides:` treats an empty dict as absent; `f"user:{id}"`
//! labels use the requesting user's id, not the row's.

use std::collections::{BTreeMap, HashMap};

use serde::Serialize;

use super::{recipes, registry, renderer};

/// Manifest/source label for a registry-default section body
/// (`composer.py:32`).
pub const SOURCE_DEFAULT: &str = "default";
/// Manifest/source label for a workspace-level override body
/// (`composer.py:33`).
pub const SOURCE_WORKSPACE: &str = "workspace";
/// Manifest/source label for an unsaved preview-draft body
/// (`composer.py:35`). The version is unchanged (the draft is unversioned).
pub const SOURCE_DRAFT: &str = "draft";

/// A section after override resolution, ready to assemble
/// (`composer.py:38-47`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedSection {
    pub key: String,
    pub title: String,
    pub customizable: String,
    pub body: String,
    /// `"default" | "workspace" | "user:<id>"` (or `"draft"` / `"candidate"`
    /// for preview / validation substitutes).
    pub source: String,
    /// The override row version; `0` for the registry default.
    pub version: i64,
}

/// One section's provenance + position in the assembled template body
/// (`composer.py:50-67`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ManifestEntry {
    pub section_key: String,
    pub source: String,
    pub version: i64,
    pub line_start: i64,
    pub line_end: i64,
}

impl ManifestEntry {
    /// `to_dict` (`composer.py:60-67`): wire order is declaration order.
    pub fn to_dict(&self) -> serde_json::Value {
        serde_json::json!({
            "section_key": self.section_key,
            "source": self.source,
            "version": self.version,
            "line_start": self.line_start,
            "line_end": self.line_end,
        })
    }
}

/// Result of composing a kind: rendered text + provenance + raw template
/// (`composer.py:70-81`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComposedPrompt {
    pub text: String,
    pub manifest: Vec<ManifestEntry>,
    pub template_body: String,
    pub resolved: Vec<ResolvedSection>,
}

impl ComposedPrompt {
    /// `manifest_dicts` (`composer.py:79-81`).
    pub fn manifest_dicts(&self) -> Vec<serde_json::Value> {
        self.manifest.iter().map(ManifestEntry::to_dict).collect()
    }
}

/// The customizability tier that actually applies for a workspace
/// (`composer.py:84-92`). Today the static registry flag — the §9.2 seam
/// for dynamic per-workspace admin-locking, which lands here without
/// touching the resolver or callers. The workspace id is accepted and
/// ignored, exactly like Python's `workspace` parameter.
pub fn effective_customizability<'a>(
    section: &'a registry::PromptSection<'a>,
    _workspace_id: Option<&str>,
) -> &'a str {
    section.customizable
}

/// One active-override row as handed over by the DB edge
/// (`PromptSectionOverride` columns read by `load_override_index`,
/// `composer.py:100-122`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OverrideRow {
    pub workspace_id: String,
    pub section_key: String,
    pub body: String,
    pub version: i64,
    pub is_active: bool,
    /// `None` for workspace-level rows; `Some` for personal (user-scope) rows.
    pub user_id: Option<String>,
}

/// Override-index scopes (`composer.py:96-97`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum OverrideScope {
    User,
    Workspace,
}

/// Bulk-loaded active overrides keyed by `(scope, section_key)`
/// (`composer.py:100-122`). `BTreeMap` keeps the fixture's sorted order.
pub type OverrideIndex = BTreeMap<(OverrideScope, String), OverrideRow>;

/// Bulk-load active overrides for a scope into `{(scope, key): row}`
/// (`composer.py:100-122`).
///
/// Same SQL semantics as the Django query, executed by the caller (the DB
/// edge) and filtered here:
///
/// ```sql
/// -- workspace_id set, user_id set (FIX-compose `sql.load_override_index_scoped`):
/// WHERE is_active AND workspace_id = :ws
///   AND (user_id IS NULL OR user_id = :user)
/// -- user_id None (FIX-compose `sql.load_override_index_all_active`):
/// WHERE is_active AND workspace_id = :ws AND user_id IS NULL
/// ```
///
/// `workspace_id = None` (no workspace context, e.g. a global preview)
/// returns the empty index without touching rows (`composer.py:108-109`).
/// The partial-unique constraints guarantee at most one active row per
/// (scope, key), so later rows simply overwrite earlier ones, as in Python.
pub fn build_override_index(
    workspace_id: Option<&str>,
    user_id: Option<&str>,
    rows: &[OverrideRow],
) -> OverrideIndex {
    let mut index = OverrideIndex::new();
    let Some(workspace_id) = workspace_id else {
        return index;
    };
    for row in rows {
        if !row.is_active || row.workspace_id != workspace_id {
            continue;
        }
        let scope = match (&row.user_id, user_id) {
            (None, _) => OverrideScope::Workspace,
            (Some(row_user), Some(uid)) if row_user == uid => OverrideScope::User,
            _ => continue,
        };
        index.insert((scope, row.section_key.clone()), row.clone());
    }
    index
}

/// Resolve the `(scope, row)` for `key` from a preloaded index
/// (`composer.py:125-133`, index branch). Personal scope wins when the
/// caller passes a user; otherwise only the workspace row is consulted.
fn lookup_override<'a>(
    user_id: Option<&str>,
    key: &str,
    index: &'a OverrideIndex,
) -> (OverrideScope, Option<&'a OverrideRow>) {
    if user_id.is_some() {
        if let Some(row) = index.get(&(OverrideScope::User, key.to_owned())) {
            return (OverrideScope::User, Some(row));
        }
    }
    (
        OverrideScope::Workspace,
        index.get(&(OverrideScope::Workspace, key.to_owned())),
    )
}

/// Resolve one section's body via the precedence chain
/// (`composer.py:150-201`): user override → workspace override → registry
/// default. Locked sections skip the chain entirely. `section` lets a
/// caller that already fetched the registry entry skip the lookup, as in
/// Python.
pub fn resolve_section(
    key: &str,
    workspace_id: Option<&str>,
    user_id: Option<&str>,
    index: &OverrideIndex,
    section: Option<&registry::PromptSection<'_>>,
) -> Result<ResolvedSection, registry::PromptRegistryError> {
    let section: &registry::PromptSection<'_> = match section {
        Some(section) => section,
        None => registry::get_section(key)?,
    };
    let default = ResolvedSection {
        key: section.key.to_owned(),
        title: section.title.to_owned(),
        customizable: section.customizable.to_owned(),
        body: section.default_body.clone(),
        source: SOURCE_DEFAULT.to_owned(),
        version: 0,
    };
    let tier = effective_customizability(section, workspace_id);
    if tier == registry::CUSTOMIZABLE_LOCKED {
        return Ok(default);
    }
    if workspace_id.is_none() {
        // No workspace context (e.g. a global preview): defaults only.
        return Ok(default);
    }
    // Personal (user-scope) overrides only apply to the fully-open tier. A
    // `workspace`-tier section is admin-governed: even if a stale personal
    // row survives a tier downgrade, it must not resolve.
    let lookup_user = if tier == registry::CUSTOMIZABLE_OVERRIDABLE {
        user_id
    } else {
        None
    };
    let (scope, row) = lookup_override(lookup_user, key, index);
    match row {
        Some(row) => {
            // `f"user:{user.id}"` (`composer.py:192`): the requesting
            // user's id, not the row's.
            let source = match scope {
                OverrideScope::User => format!("user:{}", lookup_user.unwrap_or_default()),
                OverrideScope::Workspace => SOURCE_WORKSPACE.to_owned(),
            };
            Ok(ResolvedSection {
                key: section.key.to_owned(),
                title: section.title.to_owned(),
                customizable: section.customizable.to_owned(),
                body: row.body.clone(),
                source,
                version: row.version,
            })
        }
        None => Ok(default),
    }
}

/// Concatenate resolved sections, tracking each one's line range
/// (`composer.py:204-230`). Mirrors the legacy `fragments.assemble`:
/// strip each body, join with a blank line, append a trailing newline.
/// The line ranges index the assembled (pre-render) template so a Jinja
/// error lineno maps back to its section.
pub fn assemble(resolved: &[ResolvedSection]) -> (String, Vec<ManifestEntry>) {
    // Python `str.strip()` also strips U+001C–U+001F, which Rust's
    // `char::is_whitespace` (Unicode White_Space) does not.
    let parts: Vec<String> = resolved
        .iter()
        .map(|r| {
            r.body
                .trim_matches(|c: char| c.is_whitespace() || ('\u{1c}'..='\u{1f}').contains(&c))
                .to_owned()
        })
        .collect();
    let template_body = parts.join("\n\n") + "\n";
    let mut manifest = Vec::with_capacity(resolved.len());
    let mut line: i64 = 1;
    for (r, part) in resolved.iter().zip(parts.iter()) {
        let n_lines = part.matches('\n').count() as i64 + 1;
        let line_start = line;
        let line_end = line + n_lines - 1;
        manifest.push(ManifestEntry {
            section_key: r.key.clone(),
            source: r.source.clone(),
            version: r.version,
            line_start,
            line_end,
        });
        // The "\n\n" join inserts exactly one blank line between parts.
        line = line_end + 2;
    }
    (template_body, manifest)
}

/// Render failure with section/source attribution (`composer.py:266-283`).
/// The display string is the whole contract: handlers log it and the
/// fixture asserts it byte for byte.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{message}")]
pub struct PromptComposeError {
    message: String,
}

impl PromptComposeError {
    pub(crate) fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    /// The attributed failure message.
    pub fn message(&self) -> &str {
        &self.message
    }
}

/// Best-effort: identify which section caused a render failure
/// (`composer.py:233-263`, isolation half). Re-render each *overridden*
/// section in isolation with the same context; the first that raises is
/// the culprit. Defaults are validated in CI — suspect overrides first.
/// A `None` context disables the probe, as in Python.
fn find_culprit_section(
    resolved: &[ResolvedSection],
    manifest: &[ManifestEntry],
    context: Option<&serde_json::Value>,
) -> Option<ManifestEntry> {
    let context = context?;
    let by_key: HashMap<&str, &ManifestEntry> = manifest
        .iter()
        .map(|e| (e.section_key.as_str(), e))
        .collect();
    for section in resolved {
        if section.source == SOURCE_DEFAULT {
            continue;
        }
        if renderer::render(&section.body, context).is_err() {
            return by_key.get(section.key.as_str()).cloned().cloned();
        }
    }
    None
}

/// Re-wrap a render failure's detail string with section/source
/// attribution (`composer.py:266-283`).
fn attributed_detail(
    detail: &str,
    manifest: &[ManifestEntry],
    resolved: &[ResolvedSection],
    context: Option<&serde_json::Value>,
) -> String {
    if let Some(culprit) = find_culprit_section(resolved, manifest, context) {
        return format!(
            "section '{}' (source={}, v{}) failed to render: {detail}",
            culprit.section_key, culprit.source, culprit.version
        );
    }
    let overridden: Vec<&ManifestEntry> = manifest
        .iter()
        .filter(|e| e.source != SOURCE_DEFAULT)
        .collect();
    if !overridden.is_empty() {
        let srcs = overridden
            .iter()
            .map(|e| format!("{}({})", e.section_key, e.source))
            .collect::<Vec<_>>()
            .join(", ");
        return format!("prompt render failed (active overrides: {srcs}): {detail}");
    }
    detail.to_owned()
}

/// Resolve, assemble, and render the recipe for `kind`
/// (`composer.py:286-321`). Raises [`PromptComposeError`] (attributed to
/// the failing section) when rendering fails.
///
/// `draft_overrides` (section key → body) substitutes an *unsaved* body
/// for the resolved one before assembly; keys outside this recipe are
/// ignored, and an empty map is the same as absent (`if
/// draft_overrides:`). A draft keeps its resolved version and is labeled
/// [`SOURCE_DRAFT`].
///
/// Unknown kinds / sections surface message-preserving
/// [`PromptComposeError`]s (Python propagates `RecipeNotFound` /
/// `PromptRegistryError` as-is; there are no typed catchers yet, so the
/// message is the contract).
pub fn compose(
    kind: &str,
    workspace_id: Option<&str>,
    user_id: Option<&str>,
    index: &OverrideIndex,
    context: &serde_json::Value,
    draft_overrides: Option<&HashMap<String, String>>,
    executor_kind: Option<&str>,
) -> Result<ComposedPrompt, PromptComposeError> {
    let recipe = recipes::recipe_for(kind, executor_kind)
        .map_err(|e| PromptComposeError::new(e.message()))?;
    let mut resolved = Vec::with_capacity(recipe.len());
    for key in recipe {
        resolved.push(
            resolve_section(key, workspace_id, user_id, index, None)
                .map_err(|e| PromptComposeError::new(e.message()))?,
        );
    }
    if let Some(drafts) = draft_overrides {
        if !drafts.is_empty() {
            for r in &mut resolved {
                if let Some(body) = drafts.get(&r.key) {
                    r.body = body.clone();
                    r.source = SOURCE_DRAFT.to_owned();
                }
            }
        }
    }
    let (template_body, manifest) = assemble(&resolved);
    match renderer::render(&template_body, context) {
        Ok(text) => Ok(ComposedPrompt {
            text,
            manifest,
            template_body,
            resolved,
        }),
        Err(err) => Err(PromptComposeError::new(attributed_detail(
            err.message(),
            &manifest,
            &resolved,
            Some(context),
        ))),
    }
}

/// Compose an immutable Cloud recipe without user/workspace overrides
/// (`composer.py:324-344`).
pub fn compose_cloud(
    kind: &str,
    context: &serde_json::Value,
) -> Result<ComposedPrompt, PromptComposeError> {
    let recipe =
        recipes::cloud_recipe_for(kind).map_err(|e| PromptComposeError::new(e.message()))?;
    let mut resolved = Vec::with_capacity(recipe.len());
    for key in recipe {
        let section =
            registry::get_section(key).map_err(|e| PromptComposeError::new(e.message()))?;
        resolved.push(ResolvedSection {
            key: key.to_string(),
            title: section.title.to_owned(),
            customizable: section.customizable.to_owned(),
            body: section.default_body.clone(),
            source: SOURCE_DEFAULT.to_owned(),
            version: 0,
        });
    }
    let (template_body, manifest) = assemble(&resolved);
    match renderer::render(&template_body, context) {
        Ok(text) => Ok(ComposedPrompt {
            text,
            manifest,
            template_body,
            resolved,
        }),
        Err(err) => Err(PromptComposeError::new(attributed_detail(
            err.message(),
            &manifest,
            &resolved,
            Some(context),
        ))),
    }
}

/// Assemble the recipe for `kind` **without rendering** (Jinja markers
/// intact), powering the "see the final template" view
/// (`composer.py:347-364`). The returned `text` is the raw assembled body.
pub fn compile_template(
    kind: &str,
    workspace_id: Option<&str>,
    user_id: Option<&str>,
    index: &OverrideIndex,
) -> Result<ComposedPrompt, PromptComposeError> {
    let recipe =
        recipes::recipe_for(kind, None).map_err(|e| PromptComposeError::new(e.message()))?;
    let mut resolved = Vec::with_capacity(recipe.len());
    for key in recipe {
        resolved.push(
            resolve_section(key, workspace_id, user_id, index, None)
                .map_err(|e| PromptComposeError::new(e.message()))?,
        );
    }
    let (template_body, manifest) = assemble(&resolved);
    Ok(ComposedPrompt {
        text: template_body.clone(),
        manifest,
        template_body,
        resolved,
    })
}

/// The user id whose overrides apply to a run (`composer.py:372-380`).
///
/// Human-triggered runs resolve with the triggering user's id; automatic
/// runs (tick, scheduler beat, system-bot) resolve workspace + defaults
/// only. The `run_is_human_triggered(run)` predicate owns to the runner
/// layer — this takes its boolean result — and ids cross here as strings,
/// since this crate never sees the ORM run object.
pub fn user_id_for_run(human_triggered: bool, created_by_user_id: Option<&str>) -> Option<&str> {
    if human_triggered {
        created_by_user_id
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    const WORKSPACE_ID: &str = "204fd67f-6485-47a0-98e2-079bd6b34202";
    const USER_ID: &str = "19a3bb94-30b3-4e62-8116-edea68ffe7eb";

    fn fixture() -> Value {
        let path = format!(
            "{}/../../fixtures/prompting/FIX-compose.json",
            env!("CARGO_MANIFEST_DIR")
        );
        serde_json::from_str(&std::fs::read_to_string(&path).expect("golden exists"))
            .expect("golden parses")
    }

    fn compose_data(fixture: &Value) -> &Value {
        fixture
            .get("data")
            .and_then(|data| data.get("compose"))
            .expect("fixture carries data.compose")
    }

    fn precedence_data(fixture: &Value) -> &Value {
        fixture
            .get("data")
            .and_then(|data| data.get("precedence"))
            .expect("fixture carries data.precedence")
    }

    fn row(section_key: &str, body: &str, version: i64, user_id: Option<&str>) -> OverrideRow {
        OverrideRow {
            workspace_id: WORKSPACE_ID.to_owned(),
            section_key: section_key.to_owned(),
            body: body.to_owned(),
            version,
            is_active: true,
            user_id: user_id.map(str::to_owned),
        }
    }

    /// The `FIX-compose` precedence world: a personal + a workspace row on
    /// `autonomy` (overridable), a personal row on `intro` (locked — a
    /// stale row surviving a tier downgrade), and a personal row on
    /// `repo-context`.
    fn precedence_rows() -> Vec<OverrideRow> {
        vec![
            row("autonomy", "PERSONAL autonomy", 1, Some(USER_ID)),
            row("autonomy", "WORKSPACE autonomy", 2, None),
            row("intro", "STALE PERSONAL intro", 9, Some(USER_ID)),
            row("repo-context", "PERSONAL repo-context", 3, Some(USER_ID)),
        ]
    }

    fn populated_issue_context() -> Value {
        crate::prompting::validation::sample_contexts(recipes::KIND_CODING_TASK)
            .into_iter()
            .next()
            .expect("populated sample first")
    }

    #[test]
    fn source_consts_match_fixture() {
        let golden = fixture();
        let consts = &compose_data(&golden)["source_consts"];
        assert_eq!(consts["default"], Value::String(SOURCE_DEFAULT.to_owned()));
        assert_eq!(
            consts["workspace"],
            Value::String(SOURCE_WORKSPACE.to_owned())
        );
        assert_eq!(consts["draft"], Value::String(SOURCE_DRAFT.to_owned()));
    }

    #[test]
    fn effective_customizability_is_the_registry_flag() {
        // `FIX-compose: effective_customizability_is_registry_flag`.
        let golden = fixture();
        assert!(
            compose_data(&golden)["effective_customizability_is_registry_flag"]
                .as_bool()
                .expect("bool")
        );
        let section = registry::get_section("autonomy").expect("autonomy exists");
        assert_eq!(effective_customizability(section, None), "overridable");
        assert_eq!(
            effective_customizability(section, Some(WORKSPACE_ID)),
            "overridable"
        );
    }

    #[test]
    fn build_override_index_applies_sql_semantics() {
        let mut rows = precedence_rows();
        rows.push(OverrideRow {
            is_active: false,
            ..row("autonomy", "INACTIVE autonomy", 7, None)
        });
        rows.push(OverrideRow {
            workspace_id: "other-workspace".to_owned(),
            ..row("autonomy", "OTHER WS autonomy", 8, None)
        });
        rows.push(OverrideRow {
            user_id: Some("other-user".to_owned()),
            ..row("autonomy", "OTHER USER autonomy", 9, Some("other-user"))
        });
        let index = build_override_index(Some(WORKSPACE_ID), Some(USER_ID), &rows);
        let keys: Vec<String> = index
            .keys()
            .map(|(scope, key)| {
                format!(
                    "{}:{key}",
                    match scope {
                        OverrideScope::User => "user",
                        OverrideScope::Workspace => "workspace",
                    }
                )
            })
            .collect();
        // Inactive rows, foreign workspaces and other users' personal rows
        // never enter the index.
        assert_eq!(
            keys,
            vec![
                "user:autonomy",
                "user:intro",
                "user:repo-context",
                "workspace:autonomy"
            ]
        );
        // No requesting user: personal rows are excluded, like the
        // `user_id IS NULL` variant of the fixture SQL.
        let index = build_override_index(Some(WORKSPACE_ID), None, &rows);
        assert!(index
            .keys()
            .all(|(scope, _)| *scope == OverrideScope::Workspace));
        // No workspace context: defaults only, no query at all.
        assert!(build_override_index(None, Some(USER_ID), &rows).is_empty());
    }

    #[test]
    fn resolve_personal_beats_workspace() {
        // `FIX-compose / precedence.personal_beats_workspace`.
        let golden = fixture();
        let expected = &precedence_data(&golden)["personal_beats_workspace"];
        let index = build_override_index(Some(WORKSPACE_ID), Some(USER_ID), &precedence_rows());
        let resolved = resolve_section("autonomy", Some(WORKSPACE_ID), Some(USER_ID), &index, None)
            .expect("resolves");
        assert_eq!(resolved.body, expected["body"].as_str().expect("body"));
        assert_eq!(
            resolved.source,
            expected["source"].as_str().expect("source")
        );
        assert_eq!(
            resolved.version,
            expected["version"].as_i64().expect("version")
        );
        assert_eq!(resolved.key, "autonomy");
    }

    #[test]
    fn resolve_workspace_when_no_personal() {
        // `FIX-compose / precedence.workspace_when_no_personal`.
        let golden = fixture();
        let expected = &precedence_data(&golden)["workspace_when_no_personal"];
        let rows: Vec<OverrideRow> = precedence_rows()
            .into_iter()
            .filter(|r| r.user_id.is_none())
            .collect();
        let index = build_override_index(Some(WORKSPACE_ID), Some(USER_ID), &rows);
        let resolved = resolve_section("autonomy", Some(WORKSPACE_ID), Some(USER_ID), &index, None)
            .expect("resolves");
        assert_eq!(resolved.body, expected["body"].as_str().expect("body"));
        assert_eq!(resolved.source, "workspace");
        assert_eq!(
            resolved.version,
            expected["version"].as_i64().expect("version")
        );
    }

    #[test]
    fn resolve_automatic_runs_ignore_personal() {
        // `FIX-compose / precedence.automatic_runs_workspace_only`:
        // automatic runs pass no user, so the workspace row wins even
        // though a personal row exists.
        let golden = fixture();
        let expected = &precedence_data(&golden)["automatic_runs_workspace_only"];
        let index = build_override_index(Some(WORKSPACE_ID), None, &precedence_rows());
        let resolved =
            resolve_section("autonomy", Some(WORKSPACE_ID), None, &index, None).expect("resolves");
        assert_eq!(resolved.body, expected["body"].as_str().expect("body"));
        assert_eq!(
            resolved.source,
            expected["source"].as_str().expect("source")
        );
    }

    #[test]
    fn resolve_locked_ignores_stale_personal() {
        // `FIX-compose / precedence.locked_ignores_stale_personal`: `intro`
        // is locked, so the stale personal row must not resolve.
        let index = build_override_index(Some(WORKSPACE_ID), Some(USER_ID), &precedence_rows());
        let resolved = resolve_section("intro", Some(WORKSPACE_ID), Some(USER_ID), &index, None)
            .expect("resolves");
        assert_eq!(resolved.source, SOURCE_DEFAULT);
        assert_eq!(resolved.version, 0);
        assert_eq!(
            resolved.body,
            registry::get_section("intro")
                .expect("intro exists")
                .default_body
        );
    }

    #[test]
    fn resolve_no_workspace_returns_default() {
        // `FIX-compose / precedence.no_workspace_defaults_only`.
        let index = build_override_index(Some(WORKSPACE_ID), Some(USER_ID), &precedence_rows());
        let resolved =
            resolve_section("autonomy", None, Some(USER_ID), &index, None).expect("resolves");
        assert_eq!(resolved.source, SOURCE_DEFAULT);
        assert_eq!(resolved.version, 0);
    }

    #[test]
    fn resolve_workspace_tier_ignores_personal() {
        // Tier gate (`composer.py:189`): a `workspace`-tier section never
        // resolves a personal row, even a live one. No workspace-tier
        // section ships today (31 locked, 7 overridable), so the section
        // comes in through the `section` parameter, exactly as a caller
        // that already fetched the registry entry would pass it.
        let section = registry::PromptSection {
            key: "synthetic",
            title: "Synthetic",
            customizable: registry::CUSTOMIZABLE_WORKSPACE,
            default_body: "DEFAULT".to_owned(),
        };
        let mut rows = precedence_rows();
        rows.push(row("synthetic", "STALE PERSONAL", 5, Some(USER_ID)));
        rows.push(row("synthetic", "WORKSPACE BODY", 6, None));
        let index = build_override_index(Some(WORKSPACE_ID), Some(USER_ID), &rows);
        let resolved = resolve_section(
            "synthetic",
            Some(WORKSPACE_ID),
            Some(USER_ID),
            &index,
            Some(&section),
        )
        .expect("resolves");
        assert_eq!(resolved.source, SOURCE_WORKSPACE);
        assert_eq!(resolved.body, "WORKSPACE BODY");
    }

    #[test]
    fn assemble_matches_fixture() {
        // `FIX-compose / compose.assemble_body + assemble_manifest`.
        let golden = fixture();
        let data = compose_data(&golden);
        let resolved = vec![
            ResolvedSection {
                key: "a".to_owned(),
                title: "A".to_owned(),
                customizable: "overridable".to_owned(),
                body: "line1\nline2".to_owned(),
                source: SOURCE_DEFAULT.to_owned(),
                version: 0,
            },
            ResolvedSection {
                key: "b".to_owned(),
                title: "B".to_owned(),
                customizable: "workspace".to_owned(),
                body: "solo".to_owned(),
                source: SOURCE_WORKSPACE.to_owned(),
                version: 4,
            },
        ];
        let (body, manifest) = assemble(&resolved);
        assert_eq!(body, data["assemble_body"].as_str().expect("body"));
        let dicts: Vec<Value> = manifest.iter().map(ManifestEntry::to_dict).collect();
        assert_eq!(
            dicts,
            *data["assemble_manifest"].as_array().expect("manifest")
        );
        // Empty recipe edge: `"".join + "\n"`, no manifest rows.
        let (body, manifest) = assemble(&[]);
        assert_eq!(body, "\n");
        assert!(manifest.is_empty());
        // `str.strip()` parity: U+001C–U+001F strip like whitespace.
        let resolved = vec![ResolvedSection {
            key: "a".to_owned(),
            title: "A".to_owned(),
            customizable: "overridable".to_owned(),
            body: "x\u{1c}".to_owned(),
            source: SOURCE_DEFAULT.to_owned(),
            version: 0,
        }];
        assert_eq!(assemble(&resolved).0, "x\n");
    }

    #[test]
    fn compose_defaults_manifest_matches_fixture() {
        // `FIX-compose / compose.compose_manifest_* + compose_first_manifest_entry`.
        let golden = fixture();
        let data = compose_data(&golden);
        let index = OverrideIndex::new();
        let composed = compose(
            recipes::KIND_CODING_TASK,
            Some(WORKSPACE_ID),
            Some(USER_ID),
            &index,
            &populated_issue_context(),
            None,
            None,
        )
        .expect("defaults compose");
        assert_eq!(
            composed.manifest.len(),
            data["compose_manifest_entry_count"]
                .as_u64()
                .expect("count") as usize
        );
        assert_eq!(composed.resolved.len(), composed.manifest.len());
        assert!(
            data["compose_manifest_all_default"]
                .as_bool()
                .expect("bool"),
            "no overrides active"
        );
        assert!(composed.manifest.iter().all(|e| e.source == SOURCE_DEFAULT));
        assert_eq!(
            composed.manifest_dicts()[0],
            data["compose_first_manifest_entry"].clone()
        );
        assert!(composed
            .text
            .contains("Pi Dash is a project management tool"));
        assert!(
            !composed.text.contains("{{") && !composed.text.contains("{%"),
            "compose_defaults_no_jinja_left"
        );
    }

    #[test]
    fn compose_draft_renders_and_labels_source() {
        // `FIX-compose / compose.draft_renders_value + draft_source_label`.
        let index = OverrideIndex::new();
        let mut drafts = HashMap::new();
        drafts.insert("intro".to_owned(), "Hello {{ workspace.slug }}!".to_owned());
        // Keys outside the recipe are ignored, like Python's per-resolved
        // membership check.
        drafts.insert("not-a-section".to_owned(), "ignored".to_owned());
        let composed = compose(
            recipes::KIND_CODING_TASK,
            Some(WORKSPACE_ID),
            Some(USER_ID),
            &index,
            &populated_issue_context(),
            Some(&drafts),
            None,
        )
        .expect("draft composes");
        assert!(composed.text.contains("Hello sample-ws!"));
        let intro = composed
            .resolved
            .iter()
            .find(|r| r.key == "intro")
            .expect("intro");
        assert_eq!(intro.source, SOURCE_DRAFT);
        assert_eq!(intro.version, 0);
        // An empty draft map is the same as absent (`if draft_overrides:`).
        let composed = compose(
            recipes::KIND_CODING_TASK,
            Some(WORKSPACE_ID),
            Some(USER_ID),
            &index,
            &populated_issue_context(),
            Some(&HashMap::new()),
            None,
        )
        .expect("empty drafts compose");
        assert!(composed.resolved.iter().all(|r| r.source == SOURCE_DEFAULT));
    }

    #[test]
    fn compose_draft_bad_attr_matches_fixture() {
        // `FIX-compose / compose.draft_bad_attr_error`: error attribution
        // golden — the isolation probe pins the draft section.
        let golden = fixture();
        let index = OverrideIndex::new();
        let mut drafts = HashMap::new();
        drafts.insert("intro".to_owned(), "broken {{ nope_xyz }}".to_owned());
        let err = compose(
            recipes::KIND_CODING_TASK,
            Some(WORKSPACE_ID),
            Some(USER_ID),
            &index,
            &populated_issue_context(),
            Some(&drafts),
            None,
        )
        .expect_err("bad draft fails");
        // Attribution wrapper is byte-exact; the trailing engine detail
        // is engine-specific prose (minijinja says `undefined value (in
        // <string>:1)` where Jinja says `'nope_xyz' is undefined` —
        // renderer engine-message caveat), so split it off.
        let golden_message = compose_data(&golden)["draft_bad_attr_error"]
            .as_str()
            .expect("golden message")
            .split_once(": ")
            .expect("prefix")
            .1
            .to_owned();
        let head = golden_message
            .split_once("failed to render: ")
            .expect("wrapper")
            .0;
        assert!(
            err.message()
                .starts_with(&format!("{head}failed to render: ")),
            "unexpected message: {}",
            err.message()
        );
    }

    #[test]
    fn compile_template_returns_raw_body() {
        // `FIX-compose / compose.compile_*`: markers intact, text is the
        // assembled body.
        let golden = fixture();
        let data = compose_data(&golden);
        assert!(data["compile_has_jinja_markers"].as_bool().expect("bool"));
        assert!(data["compile_text_equals_template_body"]
            .as_bool()
            .expect("bool"));
        let index = OverrideIndex::new();
        let compiled = compile_template(
            recipes::KIND_CODING_TASK,
            Some(WORKSPACE_ID),
            Some(USER_ID),
            &index,
        )
        .expect("compiles");
        assert!(compiled.text.contains("{{"));
        assert_eq!(compiled.text, compiled.template_body);
        assert_eq!(compiled.manifest.len(), compiled.resolved.len());
    }

    #[test]
    fn compose_cloud_resolves_defaults_only() {
        // `FIX-compose / compose.cloud_direct_*`.
        let ctx = crate::prompting::validation::sample_contexts(recipes::KIND_DIRECT)
            .into_iter()
            .next()
            .expect("direct sample");
        let composed = compose_cloud(recipes::KIND_DIRECT, &ctx).expect("cloud composes");
        assert!(composed.manifest.iter().all(|e| e.source == SOURCE_DEFAULT));
        assert!(composed.manifest.iter().all(|e| e.version == 0));
        assert_eq!(
            composed.manifest.len(),
            recipes::cloud_recipe_for(recipes::KIND_DIRECT)
                .expect("direct cloud recipe")
                .len()
        );
    }

    #[test]
    fn compose_unknown_kind_reports_recipe_message() {
        let err = compose(
            "nope",
            Some(WORKSPACE_ID),
            Some(USER_ID),
            &OverrideIndex::new(),
            &populated_issue_context(),
            None,
            None,
        )
        .expect_err("unknown kind fails");
        assert_eq!(err.message(), "no recipe for kind 'nope'");
    }

    #[test]
    fn user_id_for_run_matrix() {
        // `composer.py:372-380` + `precedence.automatic_runs_workspace_only`.
        assert_eq!(user_id_for_run(true, Some(USER_ID)), Some(USER_ID));
        assert_eq!(user_id_for_run(true, None), None);
        assert_eq!(user_id_for_run(false, Some(USER_ID)), None);
    }

    #[test]
    fn attribution_without_context_returns_detail() {
        // No context disables the probe (`composer.py:253-254`); with no
        // overrides active the detail passes through unchanged.
        let resolved = vec![ResolvedSection {
            key: "intro".to_owned(),
            title: "Intro".to_owned(),
            customizable: "locked".to_owned(),
            body: "hi".to_owned(),
            source: SOURCE_DEFAULT.to_owned(),
            version: 0,
        }];
        let (_, manifest) = assemble(&resolved);
        assert_eq!(
            attributed_detail("boom", &manifest, &resolved, None),
            "boom"
        );
        assert!(find_culprit_section(&resolved, &manifest, None).is_none());
    }
}
