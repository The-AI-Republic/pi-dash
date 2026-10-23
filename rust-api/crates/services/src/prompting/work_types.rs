//! Work types — per-work-type prompt guidance plugged into stage recipes.
//!
//! Port of `apps/api/pi_dash/prompting/work_types.py` (PDASHOSS01-234).
//!
//! A **work type** describes *what kind of task* an issue is (`software`,
//! `general`, later custom types); the **stage** (In Progress / In Review /
//! In Test) describes *where in the workflow* it sits. The stage keeps
//! selecting the prompt kind, the recipe, `phase_kind`, ticking, and the
//! outcome guard — the work type only decides which sections fill the
//! recipe's named slots ([`RecipeEntry::Slot`]).
//!
//! A work type is a folder mirrored from Python and embedded at compile time
//! ([`super::registry::WORK_TYPE_FILES`]):
//!
//! ```text
//! work_types/<key>/
//!     work_type.md   # metadata: key, title, description (not a section)
//!     context.md     # fills Slot("context")   — optional
//!     execute.md     # fills Slot("execute")   — optional
//!     review.md      # fills Slot("review")    — optional
//!     test.md        # fills Slot("test")      — optional
//! ```
//!
//! The slot files are ordinary registry sections with namespaced keys
//! (`software.execute`). A work type may leave a slot empty; a missing slot
//! falls back to the `general` work type's section for that slot when one
//! exists.
//!
//! Fail-loud like Python's import-time `WORK_TYPES = _load_work_types()`: a
//! malformed folder panics on first access with the message Python raises.

use std::collections::BTreeMap;
use std::sync::LazyLock;

use super::recipes::RecipeEntry;
use super::registry::{self, WORK_TYPE_FILES, WORK_TYPE_META_FILENAME};

/// Slot names a work type may fill — must match the [`RecipeEntry::Slot`]
/// entries used in [`super::recipes::RECIPES`] (`work_types.py`
/// `SLOT_NAMES`).
pub const SLOT_NAMES: &[&str] = &["context", "execute", "review", "test"];

/// Built-in work-type key: today's coding guidance.
pub const WORK_TYPE_SOFTWARE: &str = "software";
/// Built-in work-type key: neutral guidance for any task.
pub const WORK_TYPE_GENERAL: &str = "general";

/// The work type an absent selection composes with (`work_types.py`
/// `DEFAULT_WORK_TYPE`): today's behavior, unchanged.
pub const DEFAULT_WORK_TYPE: &str = WORK_TYPE_SOFTWARE;

/// Raised when the embedded work-type registry is malformed, or an unknown
/// work-type key is requested (`work_types.py` `WorkTypeError`).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{message}")]
pub struct WorkTypeError {
    message: String,
}

impl WorkTypeError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    /// The error message, byte-identical to what Python raises.
    pub fn message(&self) -> &str {
        &self.message
    }
}

/// One work type: identity, metadata, and its slot → section-key map
/// (`work_types.py` `WorkType`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkType {
    pub key: &'static str,
    pub title: &'static str,
    pub description: &'static str,
    /// slot name → registry section key (`"software.execute"`); only the
    /// slots this work type actually supplies are present.
    pub sections: BTreeMap<&'static str, String>,
}

/// Parse a `work_type.md` metadata file (`work_types.py` `_parse_meta`):
/// the same tiny front-matter format as sections; the body is ignored.
fn parse_meta(
    expected_key: &str,
    text: &'static str,
) -> Result<BTreeMap<&'static str, &'static str>, WorkTypeError> {
    let parts: Vec<&'static str> = text.splitn(3, "---").collect();
    if !text.starts_with("---") || parts.len() < 3 {
        return Err(WorkTypeError::new(format!(
            "work type '{expected_key}': {WORK_TYPE_META_FILENAME} is missing the '---' front-matter block"
        )));
    }
    let mut meta = BTreeMap::new();
    for line in parts[1].trim().lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((field, value)) = line.split_once(':') else {
            return Err(WorkTypeError::new(format!(
                "work type '{expected_key}': invalid front-matter line: '{line}'"
            )));
        };
        meta.insert(field.trim(), value.trim());
    }
    let missing: Vec<&str> = ["description", "key", "title"]
        .into_iter()
        .filter(|field| !meta.contains_key(field))
        .collect();
    if !missing.is_empty() {
        return Err(WorkTypeError::new(format!(
            "work type '{expected_key}': work_type.md missing field(s): {}",
            missing.join(", ")
        )));
    }
    if meta["key"] != expected_key {
        return Err(WorkTypeError::new(format!(
            "work type folder '{expected_key}' does not match its work_type.md key='{}'",
            meta["key"]
        )));
    }
    Ok(meta)
}

/// Build the work-type table from the embedded folders (`work_types.py`
/// `_load_work_types`).
fn load_work_types() -> Result<BTreeMap<&'static str, WorkType>, WorkTypeError> {
    let mut folders: BTreeMap<&'static str, Vec<(&'static str, &'static str)>> = BTreeMap::new();
    for (work_type, file_name, text) in WORK_TYPE_FILES {
        folders
            .entry(work_type)
            .or_default()
            .push((file_name, text));
    }
    let mut out = BTreeMap::new();
    for (key, mut files) in folders {
        files.sort_by_key(|(file_name, _)| *file_name);
        let Some((_, meta_text)) = files
            .iter()
            .find(|(file_name, _)| *file_name == WORK_TYPE_META_FILENAME)
        else {
            return Err(WorkTypeError::new(format!(
                "work type '{key}' has no {WORK_TYPE_META_FILENAME}"
            )));
        };
        let meta = parse_meta(key, meta_text)?;
        let mut sections = BTreeMap::new();
        for (file_name, _) in &files {
            if *file_name == WORK_TYPE_META_FILENAME {
                continue;
            }
            let slot = file_name.strip_suffix(".md").unwrap_or(file_name);
            if !SLOT_NAMES.contains(&slot) {
                return Err(WorkTypeError::new(format!(
                    "work type '{key}' has section file '{file_name}' which is not a slot \
                     (expected one of {})",
                    py_list(SLOT_NAMES.iter().copied())
                )));
            }
            let section_key = format!("{key}.{slot}");
            // The registry loaded the same file; fail loudly if it didn't.
            registry::get_section(&section_key)
                .map_err(|err| WorkTypeError::new(err.message().to_owned()))?;
            sections.insert(slot, section_key);
        }
        out.insert(
            key,
            WorkType {
                key,
                title: meta["title"],
                description: meta["description"],
                sections,
            },
        );
    }
    if !out.contains_key(DEFAULT_WORK_TYPE) {
        return Err(WorkTypeError::new(format!(
            "the default work type '{DEFAULT_WORK_TYPE}' is not defined on disk"
        )));
    }
    Ok(out)
}

/// The loaded work types, parsed once; malformed folders panic on first
/// access (`work_types.py` `WORK_TYPES`).
static WORK_TYPES: LazyLock<BTreeMap<&'static str, WorkType>> = LazyLock::new(|| {
    load_work_types().unwrap_or_else(|err| panic!("work types are malformed: {}", err.message()))
});

/// Python `repr` of a sorted list of strings: `['a', 'b']`.
fn py_list<'a>(items: impl IntoIterator<Item = &'a str>) -> String {
    let mut items: Vec<&str> = items.into_iter().collect();
    items.sort_unstable();
    let quoted: Vec<String> = items.iter().map(|item| format!("'{item}'")).collect();
    format!("[{}]", quoted.join(", "))
}

/// Whether `key` names a registered work type (`key in WORK_TYPES`).
pub fn is_known(key: &str) -> bool {
    WORK_TYPES.contains_key(key)
}

/// Registered work-type keys, sorted (`sorted(WORK_TYPES)`).
pub fn keys() -> Vec<&'static str> {
    WORK_TYPES.keys().copied().collect()
}

/// `sorted(WORK_TYPES)` rendered as Python's list repr — the tail of the
/// serializers' `unknown work type; expected one of [...]` messages.
pub fn keys_repr() -> String {
    py_list(WORK_TYPES.keys().copied())
}

/// The serializer validation message for an unknown key (shared by the
/// issue and project serializers on every surface).
pub fn unknown_work_type_message() -> String {
    format!("unknown work type; expected one of {}", keys_repr())
}

/// Fetch one work type by key (`work_types.py` `get_work_type`).
pub fn get_work_type(key: &str) -> Result<&'static WorkType, WorkTypeError> {
    WORK_TYPES
        .get(key)
        .ok_or_else(|| WorkTypeError::new(format!("unknown work type: '{key}'")))
}

/// Work types in stable (key-sorted) order (`work_types.py`
/// `all_work_types`).
pub fn all_work_types() -> Vec<&'static WorkType> {
    WORK_TYPES.values().collect()
}

/// The section key filling `slot_name` for `work_type`, or `None`
/// (`work_types.py` `section_key_for_slot`). A work type that doesn't supply
/// a slot falls back to the `general` work type's section for it.
pub fn section_key_for_slot(work_type: &'static WorkType, slot_name: &str) -> Option<&'static str> {
    let own = work_type.sections.get(slot_name).map(String::as_str);
    if own.is_some() || work_type.key == WORK_TYPE_GENERAL {
        return own;
    }
    WORK_TYPES
        .get(WORK_TYPE_GENERAL)
        .and_then(|general| general.sections.get(slot_name))
        .map(String::as_str)
}

/// Resolve a recipe's slots to `work_type_key`'s section keys; unfilled
/// slots are dropped (`work_types.py` `expand`). Recipes without slots
/// (scheduler) pass through unchanged.
pub fn expand(
    recipe: &[RecipeEntry],
    work_type_key: &str,
) -> Result<Vec<&'static str>, WorkTypeError> {
    let work_type = get_work_type(work_type_key)?;
    let mut out = Vec::with_capacity(recipe.len());
    for entry in recipe {
        match entry {
            RecipeEntry::Section(key) => out.push(*key),
            RecipeEntry::Slot(name) => {
                if let Some(key) = section_key_for_slot(work_type, name) {
                    out.push(key);
                }
            }
        }
    }
    Ok(out)
}

/// The work-type key that applies to an issue (`work_types.py`
/// `effective_work_type`): the issue's explicit `work_type` → the project's
/// `default_work_type` → `general`. An unknown stored key is treated as
/// absent — the next rung applies — rather than failing the run.
pub fn effective_work_type(
    issue_work_type: Option<&str>,
    project_default_work_type: Option<&str>,
) -> &'static str {
    for key in [issue_work_type, project_default_work_type]
        .into_iter()
        .flatten()
    {
        if let Some((known, _)) = WORK_TYPES.get_key_value(key) {
            return known;
        }
    }
    if WORK_TYPES.contains_key(WORK_TYPE_GENERAL) {
        WORK_TYPE_GENERAL
    } else {
        DEFAULT_WORK_TYPE
    }
}

/// The default work type a newly created project gets when none was given
/// (`Project.save`): `software` when a repo is bound, `general` otherwise.
pub fn default_for_new_project(repo_url: Option<&str>) -> &'static str {
    if repo_url.is_some_and(|url| !url.is_empty()) {
        WORK_TYPE_SOFTWARE
    } else {
        WORK_TYPE_GENERAL
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::prompting::recipes;

    #[test]
    fn built_in_work_types_load() {
        assert_eq!(keys(), vec!["general", "software"]);
        let software = get_work_type("software").expect("software");
        assert_eq!(software.title, "Software");
        assert!(!software.description.is_empty());
        assert_eq!(
            software.sections.keys().copied().collect::<Vec<_>>(),
            SLOT_NAMES.to_vec()
        );
        assert_eq!(software.sections["execute"], "software.execute");
        let general = get_work_type("general").expect("general");
        assert_eq!(general.title, "General");
        // `general` has no context slot by design.
        assert_eq!(
            general.sections.keys().copied().collect::<Vec<_>>(),
            vec!["execute", "review", "test"]
        );
        for work_type in all_work_types() {
            for section_key in work_type.sections.values() {
                assert_eq!(
                    registry::get_section(section_key).expect("section").key,
                    section_key
                );
            }
        }
    }

    #[test]
    fn unknown_work_type_errors() {
        let err = get_work_type("nope").expect_err("unknown raises");
        assert_eq!(err.message(), "unknown work type: 'nope'");
        assert_eq!(
            unknown_work_type_message(),
            "unknown work type; expected one of ['general', 'software']"
        );
    }

    #[test]
    fn expand_fills_slots_per_work_type() {
        let recipe = recipes::recipe_for(recipes::KIND_CODING_TASK, None).expect("recipe");
        let software = expand(recipe, "software").expect("expands");
        assert_eq!(software[1], "software.context");
        assert!(software.contains(&"software.execute"));
        let general = expand(recipe, "general").expect("expands");
        // No context slot for general: the entry is dropped, not left empty.
        assert_eq!(general.len(), software.len() - 1);
        assert!(general.contains(&"general.execute"));
        assert!(!general.iter().any(|key| key.starts_with("software.")));
        // Slotless recipes pass through unchanged.
        let scheduler = recipes::recipe_for(recipes::KIND_SCHEDULER, None).expect("recipe");
        assert_eq!(
            expand(scheduler, "general").expect("expands"),
            expand(scheduler, "software").expect("expands")
        );
        assert!(expand(recipe, "nope").is_err());
    }

    #[test]
    fn every_stage_and_work_type_expands_to_registered_sections() {
        for (kind, entries) in recipes::RECIPES {
            for entry in *entries {
                if let RecipeEntry::Slot(name) = entry {
                    assert!(SLOT_NAMES.contains(name), "{kind} has unknown slot {name}");
                }
            }
            for work_type in keys() {
                for key in expand(entries, work_type).expect("expands") {
                    registry::get_section(key).unwrap_or_else(|_| {
                        panic!("{kind} ({work_type}) references unknown section {key}")
                    });
                }
            }
        }
    }

    #[test]
    fn effective_work_type_resolution_chain() {
        assert_eq!(
            effective_work_type(Some("general"), Some("software")),
            "general"
        );
        assert_eq!(effective_work_type(None, Some("software")), "software");
        assert_eq!(effective_work_type(Some(""), Some("software")), "software");
        // Unknown issue key falls through to the project default.
        assert_eq!(
            effective_work_type(Some("bogus"), Some("software")),
            "software"
        );
        assert_eq!(effective_work_type(None, Some("")), "general");
        assert_eq!(effective_work_type(None, None), "general");
        assert_eq!(effective_work_type(Some("bogus"), Some("bogus")), "general");
    }

    #[test]
    fn new_project_default() {
        assert_eq!(default_for_new_project(Some("https://x/y.git")), "software");
        assert_eq!(default_for_new_project(Some("")), "general");
        assert_eq!(default_for_new_project(None), "general");
    }
}
