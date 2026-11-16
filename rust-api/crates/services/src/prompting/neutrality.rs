//! Work-type neutrality of core stage sections (PDASHOSS01-234).
//!
//! Ports `apps/api/pi_dash/prompting/neutrality.py`: the single definition
//! of "software wording" that core stage sections (shared by every work
//! type) must not carry. `revalidate_section_overrides` uses
//! [`override_neutrality_issue`] to flag existing workspace/user overrides
//! of core sections that still carry the pre-work-type git/PR steps.
//!
//! Patterns, their order, case sensitivity, and the detail message are
//! byte-identical to Python (the flagged line is compared across engines).

use std::sync::LazyLock;

use regex::Regex;

use super::recipes::{self, RecipeEntry};

/// `NEUTRALITY_BANNED` (`neutrality.py`): matched case-insensitively.
pub const NEUTRALITY_BANNED: &[&str] = &[
    r"\bgit\b",
    r"\bgithub\b",
    r"\bgitlab\b",
    r"\bbranch(es|ed|ing)?\b",
    r"\bcommit(s|ted|ting)?\b",
    r"\bpush(es|ed|ing)?\b",
    r"pull request",
    r"merge request",
    r"code review",
    r"\brepo\b",
    r"\brepositor(y|ies)\b",
    r"\bcodebase\b",
    r"CLAUDE\.md",
    r"AGENTS\.md",
    r"\.ai_design",
    r"\bgh\b",
    r"\bglab\b",
    r"\brebase(s|d)?\b",
    r"\bcheckout\b",
    r"\bmerge\b",
    r"\bpipeline(s)?\b",
    r"\bdiff(s)?\b",
];

/// `NEUTRALITY_BANNED_CASE_SENSITIVE` (`neutrality.py`).
pub const NEUTRALITY_BANNED_CASE_SENSITIVE: &[&str] = &[r"\bPRs?\b", r"\bMRs?\b", r"\bCI\b"];

/// Compiled patterns in Python's order: insensitive list, then sensitive.
static COMPILED: LazyLock<Vec<Regex>> = LazyLock::new(|| {
    NEUTRALITY_BANNED
        .iter()
        .map(|p| Regex::new(&format!("(?i){p}")).expect("neutrality pattern compiles"))
        .chain(
            NEUTRALITY_BANNED_CASE_SENSITIVE
                .iter()
                .map(|p| Regex::new(p).expect("neutrality pattern compiles")),
        )
        .collect()
});

/// `STAGE_KINDS` (`neutrality.py`).
pub const STAGE_KINDS: &[&str] = &[
    recipes::KIND_CODING_TASK,
    recipes::KIND_REVIEW,
    recipes::KIND_TEST,
];

/// `matched_terms` (`neutrality.py`): the first matched word per pattern,
/// as written, in pattern order, duplicates dropped.
pub fn matched_terms(text: &str) -> Vec<String> {
    let mut terms: Vec<String> = Vec::new();
    for rx in COMPILED.iter() {
        if let Some(m) = rx.find(text) {
            let term = m.as_str();
            if !terms.iter().any(|t| t == term) {
                terms.push(term.to_owned());
            }
        }
    }
    terms
}

/// `core_stage_section_keys` (`neutrality.py`): core (non-slot) section
/// keys of the three stage recipes.
pub fn is_core_stage_section(section_key: &str) -> bool {
    recipes::RECIPES
        .iter()
        .filter(|(kind, _)| STAGE_KINDS.contains(kind))
        .flat_map(|(_, entries)| entries.iter())
        .any(|entry| matches!(entry, RecipeEntry::Section(key) if *key == section_key))
}

/// `override_neutrality_issue` (`neutrality.py`): why an override body of
/// `section_key` breaks core neutrality, or `None`.
pub fn override_neutrality_issue(section_key: &str, body: &str) -> Option<String> {
    if !is_core_stage_section(section_key) {
        return None;
    }
    let terms = matched_terms(body);
    if terms.is_empty() {
        return None;
    }
    let listed = terms
        .iter()
        .map(|t| format!("'{t}'"))
        .collect::<Vec<_>>()
        .join(", ");
    Some(format!(
        "override for core section '{section_key}' carries work-type-specific wording ({listed}); \
         core sections are shared by every work type, so move that guidance into a work-type section \
         (e.g. 'software.execute')"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    const LEGACY: &str =
        "## Step 2\n1. Commit and push the branch, then open a PR with `gh pr create`.\n";

    /// Same expectation as Python's
    /// `test_override_neutrality_flags_core_section_with_git_wording`.
    #[test]
    fn flags_core_section_with_git_wording() {
        assert_eq!(
            override_neutrality_issue("implementation", LEGACY).as_deref(),
            Some(
                "override for core section 'implementation' carries work-type-specific wording \
                 ('branch', 'Commit', 'push', 'gh', 'PR'); core sections are shared by every work type, \
                 so move that guidance into a work-type section (e.g. 'software.execute')"
            )
        );
    }

    #[test]
    fn allows_neutral_core_and_any_work_type_section() {
        assert_eq!(
            override_neutrality_issue("implementation", "Deliver the part and record it."),
            None
        );
        assert_eq!(override_neutrality_issue("software.execute", LEGACY), None);
        assert!(!is_core_stage_section("scheduler-task"));
        assert_eq!(override_neutrality_issue("scheduler-task", LEGACY), None);
    }

    /// The shipped core defaults stay neutral (mirrors Python's
    /// `test_core_stage_sections_are_work_type_neutral`).
    #[test]
    fn shipped_core_stage_sections_are_neutral() {
        for section in super::super::registry::all_sections() {
            if is_core_stage_section(section.key) {
                assert!(
                    matched_terms(&section.default_body).is_empty(),
                    "core section {} carries software wording: {:?}",
                    section.key,
                    matched_terms(&section.default_body)
                );
            }
        }
    }
}
