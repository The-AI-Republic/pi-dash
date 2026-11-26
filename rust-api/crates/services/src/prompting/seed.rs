//! Override revalidation decisions (NEWPIDASH-404 trimmed the seed half).
//!
//! Port of `apps/api/pi_dash/prompting/management/commands/`
//! `revalidate_section_overrides.py` (`handle`: iterate active rows,
//! unknown key or `OverrideValidationError` → `needs_attention=True`,
//! `--clear` resets clean rows, never deletes/deactivates) —
//! [`decide_revalidate`], [`revalidate_summary`], [`flagged_line`],
//! [`UNKNOWN_SECTION_DETAIL`].
//!
//! The legacy seed half is gone (NEWPIDASH-404): the 14 ordered legacy
//! prompt pieces (including the stale duplicate of the pidash CLI copy),
//! the default/review/test body readers and consts, and the legacy
//! template refresh commands wrote rows into the `prompt_template` table
//! that nothing reads at run time — prompts come from `sections/` plus
//! recipes only. The table and its SeaORM entity / model stay so the
//! shared database keeps working with Django, whose historical migrations
//! replay against it; only the Rust refresh path was removed.
//!
//! DB seam (the same split every sibling port uses — this crate holds no
//! database handle, so no `sqlx` here): the Django ORM queries become
//! contract SQL the DB edge issues, with the pure decision living here:
//!
//! * revalidate scan: `PromptSectionOverride.objects.filter(is_active=True)`
//!   iterated row by row; membership is [`section_known`], validation is
//!   [`validate_row`] (both call the already-ported [`super::registry`] /
//!   [`super::validation`] for real — no reimplementation).
//!
//! Coexistence note: the Python revalidate unit emits no Celery task and
//! touches no queue, so there is no transactional enqueue and no
//! Celery-format publisher on this path — the F-09 queue kernel is
//! untouched. The scan runs inside the caller's transaction at the DB
//! edge, exactly like the Django ORM call it translates.
//!
//! Ported behaviors (translate, don't redesign — from the `bugs` array of
//! the revalidate half of `FIX-seed`): a revalidate row with `user_id=NULL`
//! renders as the literal string `None` in the flagged line.

use super::{registry, validation};

/// Detail recorded when an override's section key no longer exists in the
/// registry (`revalidate_section_overrides.py:handle`).
pub const UNKNOWN_SECTION_DETAIL: &str = "section key no longer exists";

/// What the revalidate scan does with one active row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RevalidateAction {
    /// Broken and not yet flagged: set `needs_attention=True`.
    Flag,
    /// Clean but still flagged, with `--clear`: set `needs_attention=False`.
    Clear,
    /// Anything else: leave the row alone (never deletes/deactivates).
    Keep,
}

/// Section keys whose built-in body the lean agent workflow change
/// rewrote (design `.ai_design/lean_agent_workflow/design.md` §9.5). An
/// active override on one of these still wins over the new default, but
/// it was written against the old procedure (it may name `pidash run
/// yield` or workpad blocks that no longer exist), so the revalidate scan
/// flags it for review. Deleted keys (`review-cycle`, `test-cycle`,
/// `workpad-setup`, …) need no entry: they are unknown keys and are
/// flagged with [`UNKNOWN_SECTION_DETAIL`]. The unified workpad change
/// (`.ai_design/unified_workpad/design.md` §6.4) rewrote `workpad`,
/// `review` and `test` as well.
pub const LEAN_WORKFLOW_REWRITTEN_KEYS: &[&str] = &[
    "analyze-and-scope",
    "autonomy",
    "default-posture",
    "implementation",
    "workpad",
    "review",
    "test",
    "general.execute",
    "general.review",
    "general.test",
    "software.execute",
    "software.review",
    "software.test",
];

/// Detail recorded for an override on a [`LEAN_WORKFLOW_REWRITTEN_KEYS`]
/// section.
pub const LEAN_WORKFLOW_REWRITTEN_DETAIL: &str =
    "section rewritten by the lean workflow change; review this override";

/// Wording only the procedure before the lean workflow change used: the
/// separate end-of-run commands and the retired workpad blocks. An
/// override that still carries one was written against the old
/// procedure; one an admin has rewritten no longer does.
pub const LEAN_WORKFLOW_RETIRED_MARKERS: &[&str] = &[
    "pidash run yield",
    "--stop-ticking",
    "parent_done_payload",
    "Progress Checkpoints",
    "Autonomy / Escalation",
    "Acceptance Criteria / How to Test",
    "workpad-template",
    // NEWPIDASH-325: the pre-rewrite workpad recipe fetched the whole body
    // back before every edit (`pidash workpad get | jq`); the workpad file
    // round-trip replaced it.
    "pidash workpad get | jq",
    // Unified workpad (`.ai_design/unified_workpad/design.md` §6.4): the
    // stage hand-off and the old workpad blocks are gone.
    "--handoff-file",
    "{{ handoff }}",
    "{% if handoff",
    "### Hand-off",
    "### Path to done",
    "workpad `Plan`",
    "workpad `Notes`",
    "not read by review or test",
    "Pick up from its open items and plan",
];

/// Whether an active override of `section_key` is flagged because the
/// lean workflow change rewrote that section (§9.5), returning the detail.
/// An override is flagged while its body still uses the retired procedure,
/// or drops a step only that section carries (the `implementation`
/// workpad note), so the flag clears once an admin brings it up to date (and
/// `--clear` can clear it).
pub fn rewritten_section_detail(section_key: &str, body: &str) -> Option<&'static str> {
    if !LEAN_WORKFLOW_REWRITTEN_KEYS.contains(&section_key) {
        return None;
    }
    let retired = LEAN_WORKFLOW_RETIRED_MARKERS
        .iter()
        .any(|marker| body.contains(marker));
    (retired || lacks_required_procedure(section_key, body))
        .then_some(LEAN_WORKFLOW_REWRITTEN_DETAIL)
}

/// Sections that carry a step of the new procedure no other section
/// repeats: an override that replaces one must keep that step. The
/// `implementation` section is where an In Progress run is told to leave
/// its note in the workpad; without it, `finish --to "In Review"` is
/// refused (`.ai_design/unified_workpad/design.md` §8.2).
pub const LEAN_WORKFLOW_REQUIRED_STEPS: &[(&str, &[&str])] =
    &[("implementation", &["Keep in mind"])];

/// Whether an override of `section_key` drops a step of the new procedure
/// that only that section carries ([`LEAN_WORKFLOW_REQUIRED_STEPS`]).
fn lacks_required_procedure(section_key: &str, body: &str) -> bool {
    LEAN_WORKFLOW_REQUIRED_STEPS
        .iter()
        .find(|(key, _)| *key == section_key)
        .is_some_and(|(_, any_of)| !any_of.iter().any(|word| body.contains(word)))
}

/// Pure half of the revalidate loop (`revalidate_section_overrides.py:handle`):
/// `broken` is "unknown section key, or `validate_override` raised, or the
/// override on a section the lean workflow change rewrote still uses the
/// retired procedure ([`rewritten_section_detail`])". Once an admin
/// rewrites such an override it is no longer broken, and `--clear`
/// clears its flag.
/// Truth table mirrors the `if broken and not needs_attention /
/// elif not broken and needs_attention and clear` branches exactly.
pub fn decide_revalidate(broken: bool, needs_attention: bool, clear: bool) -> RevalidateAction {
    if broken && !needs_attention {
        RevalidateAction::Flag
    } else if !broken && needs_attention && clear {
        RevalidateAction::Clear
    } else {
        RevalidateAction::Keep
    }
}

/// Revalidate summary line (`revalidate_section_overrides.py:handle` tail):
/// `"checked {checked} active override(s): {flagged} newly flagged,
/// {cleared} cleared."`
pub fn revalidate_summary(checked: u64, flagged: u64, cleared: u64) -> String {
    format!("checked {checked} active override(s): {flagged} newly flagged, {cleared} cleared.")
}

/// Per-row flag line (`revalidate_section_overrides.py:handle`):
/// `"flagged {workspace_id}/{section_key} (user={user_id}): {detail}"`.
/// A `NULL` user renders as the literal string `None`, exactly like
/// Python's `f"...{row.user_id}..."`.
pub fn flagged_line(
    workspace_id: &str,
    section_key: &str,
    user_id: Option<&str>,
    detail: &str,
) -> String {
    format!(
        "flagged {workspace_id}/{section_key} (user={}): {detail}",
        user_id.unwrap_or("None")
    )
}

/// Registry membership half of the revalidate check
/// (`revalidate_section_overrides.py:handle`: `row.section_key not in
/// registry.REGISTRY`). Calls the already-ported registry for real.
pub fn section_known(section_key: &str) -> bool {
    registry::get_section(section_key).is_ok()
}

/// Validation half of the revalidate check: runs the already-ported
/// save-time validator over one override body. `index` is the preloaded
/// override index (same seam as `composer`); the DB edge preloads it.
pub fn validate_row(
    section_key: &str,
    body: &str,
    workspace_id: Option<&str>,
    user_id: Option<&str>,
    index: &super::composer::OverrideIndex,
) -> Result<(), validation::OverrideValidationError> {
    validation::validate_override(section_key, body, workspace_id, user_id, index)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    fn fixture() -> Value {
        let path = format!(
            "{}/../../fixtures/prompting/FIX-seed.json",
            env!("CARGO_MANIFEST_DIR")
        );
        serde_json::from_str(&std::fs::read_to_string(&path).expect("golden exists"))
            .expect("golden parses")
    }

    #[test]
    fn revalidate_truth_table_and_messages() {
        // `revalidate_section_overrides.py:handle` branch-for-branch.
        assert_eq!(
            decide_revalidate(true, false, false),
            RevalidateAction::Flag
        );
        assert_eq!(decide_revalidate(true, true, false), RevalidateAction::Keep);
        assert_eq!(
            decide_revalidate(false, true, true),
            RevalidateAction::Clear
        );
        assert_eq!(
            decide_revalidate(false, true, false),
            RevalidateAction::Keep
        );
        assert_eq!(
            decide_revalidate(false, false, true),
            RevalidateAction::Keep
        );
        assert_eq!(
            decide_revalidate(false, false, false),
            RevalidateAction::Keep
        );
        // `FIX-seed / commands.revalidate_first_run + revalidate_clear_run`.
        let golden = fixture();
        let commands = &golden["data"]["commands"];
        let golden_str = |key: &str| {
            commands
                .get(key)
                .and_then(Value::as_str)
                .unwrap_or_else(|| panic!("fixture carries commands.{key}"))
        };
        assert_eq!(
            revalidate_summary(4, 2, 0),
            golden_str("revalidate_first_run")
        );
        assert_eq!(
            revalidate_summary(4, 0, 1),
            golden_str("revalidate_clear_run")
        );
        assert_eq!(UNKNOWN_SECTION_DETAIL, "section key no longer exists");
        // NULL user renders as the literal string `None`.
        assert_eq!(
            flagged_line("ws-1", "autonomy", None, "boom"),
            "flagged ws-1/autonomy (user=None): boom"
        );
        assert_eq!(
            flagged_line("ws-1", "autonomy", Some("u-9"), "boom"),
            "flagged ws-1/autonomy (user=u-9): boom"
        );
    }

    #[test]
    fn revalidate_wires_registry_and_validator_for_real() {
        // The membership + validation halves call the ported crates, not stubs.
        assert!(section_known("autonomy"));
        assert!(!section_known("oops_no_such_section"));
        let index = super::super::composer::OverrideIndex::new();
        // A locked section can never validate (fixture: intro/personal stays flagged).
        let err = validate_row("intro", "anything", Some("ws"), Some("u"), &index)
            .expect_err("locked section rejects");
        assert!(
            err.message().contains("locked"),
            "unexpected: {}",
            err.message()
        );
        // An undefined variable fails render validation (fixture: autonomy BROKEN var).
        let err = validate_row(
            "autonomy",
            "BROKEN {{ oops_xyz }}",
            Some("ws"),
            None,
            &index,
        )
        .expect_err("undefined var rejects");
        assert!(!err.message().is_empty());
        // Unknown keys fail through the same validator path.
        assert!(validate_row("oops_no_such_section", "x", None, None, &index).is_err());
    }
}
