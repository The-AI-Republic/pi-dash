//! Prompting ops decisions: revalidate orchestration (D-37, stage 7).
//!
//! Ports the `handle()` logic of
//! `apps/api/pi_dash/prompting/management/commands/revalidate_section_overrides.py`
//! (scan active rows, flag-or-clear `needs_attention`, print the summary).
//! Fixture F37-09 (`rust-api/fixtures/ops/commands/prompting.golden.json`).
//!
//! Pure decisions only — this module holds no database handle, so no
//! `sqlx` here. The DB edge (`pidash_db::ops::prompting`) preloads rows
//! and applies the writes this module plans; the CLI edge prints the
//! lines. Every kernel call goes to the already-ported prompting domain
//! (`super::super::prompting::{seed, validation, composer, registry}`)
//! for real — nothing is reimplemented or forked.
//!
//! Ported behaviors (translate, don't redesign — same as the kernels):
//!
//! * A revalidate row with `user_id=NULL` renders as the literal string
//!   `None` in the flagged line (Python f-string of `None`).
//! * An unknown section key is broken *without* rendering
//!   (`revalidate_section_overrides.py:40-41`).
//! * An already-flagged broken row stays flagged silently (no recount);
//!   a clean flagged row only clears under `--clear`.
//! * A core stage section override that renders cleanly but carries
//!   software/git/PR wording is flagged with the neutrality detail
//!   (`prompting::neutrality`, PDASHOSS01-234).
//! * Validation runs against the initial snapshot: flagging only flips
//!   `needs_attention`, which the validator never reads, so planning the
//!   whole scan before writing matches Python's row-by-row loop exactly.

use super::super::prompting::{composer, neutrality, seed};

/// One active override row as handed over by the DB edge: the columns
/// the revalidate loop reads, with ids as strings (UUID text, exactly
/// as the validator and the flagged line consume them).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActiveOverride {
    /// Row id, passed through so the edge can target its writes.
    pub id: String,
    pub workspace_id: String,
    /// `None` for workspace-level rows (renders as `None` in the
    /// flagged line, like Python's f-string of `None`).
    pub user_id: Option<String>,
    pub section_key: String,
    pub body: String,
    pub version: i64,
    pub needs_attention: bool,
}

impl ActiveOverride {
    /// The composer index row (`composer.py:100-122`): active by
    /// construction (the edge only hands over active rows).
    fn composer_row(&self) -> composer::OverrideRow {
        composer::OverrideRow {
            workspace_id: self.workspace_id.clone(),
            section_key: self.section_key.clone(),
            body: self.body.clone(),
            version: self.version,
            is_active: true,
            user_id: self.user_id.clone(),
        }
    }
}

/// What the scan does with one row, plus the line to print (if any).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RevalidateStep {
    /// The row id, passed through for the edge's write.
    pub id: String,
    pub action: seed::RevalidateAction,
    /// `Some` only for newly flagged rows (the `flagged …` line).
    pub line: Option<String>,
}

/// The planned revalidate: per-row steps plus the summary counts/line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RevalidatePlan {
    pub steps: Vec<RevalidateStep>,
    pub checked: u64,
    pub flagged: u64,
    pub cleared: u64,
    /// The stdout summary:
    /// `"checked {checked} active override(s): {flagged} newly flagged,
    /// {cleared} cleared."`.
    pub summary: String,
}

/// Pure half of `revalidate_section_overrides.handle` (`:33-68`): run the
/// registry-membership check plus the save-time validator over every
/// active row and decide flag / clear / keep per row.
///
/// `rows` must be the full active set in scan order (the edge hands over
/// exactly what Django's `.filter(is_active=True).iterator()` yields);
/// each row's validation index is built from that same snapshot for its
/// own `(workspace, user)` scope, matching the per-section DB lookups
/// Python's `resolve_section` performs (`FIX-compose` asserts both paths
/// agree).
pub fn plan_revalidate(rows: &[ActiveOverride], clear: bool) -> RevalidatePlan {
    let composer_rows: Vec<composer::OverrideRow> =
        rows.iter().map(ActiveOverride::composer_row).collect();
    let mut steps = Vec::with_capacity(rows.len());
    let mut checked = 0u64;
    let mut flagged = 0u64;
    let mut cleared = 0u64;
    for row in rows {
        checked += 1;
        // A section that no longer exists is always a problem; flag it
        // without attempting to render (:38-41).
        let mut broken = !seed::section_known(&row.section_key);
        let mut detail = if broken {
            seed::UNKNOWN_SECTION_DETAIL.to_owned()
        } else {
            String::new()
        };
        if !broken {
            let index = composer::build_override_index(
                Some(&row.workspace_id),
                row.user_id.as_deref(),
                &composer_rows,
            );
            if let Err(err) = seed::validate_row(
                &row.section_key,
                &row.body,
                Some(&row.workspace_id),
                row.user_id.as_deref(),
                &index,
            ) {
                broken = true;
                detail = err.message().to_owned();
            } else if let Some(issue) =
                neutrality::override_neutrality_issue(&row.section_key, &row.body)
            {
                // A core stage section override carrying work-type-specific
                // wording would leak into every work type
                // (`revalidate_section_overrides.py`, PDASHOSS01-234).
                broken = true;
                detail = issue;
            } else if let Some(rewritten) =
                seed::rewritten_section_detail(&row.section_key, &row.body)
            {
                // The lean workflow change rewrote this section; an override
                // written against the old procedure needs an admin's review
                // (design §9.5).
                broken = true;
                detail = rewritten.to_owned();
            }
        }
        let action = seed::decide_revalidate(broken, row.needs_attention, clear);
        let line = match action {
            seed::RevalidateAction::Flag => {
                flagged += 1;
                Some(seed::flagged_line(
                    &row.workspace_id,
                    &row.section_key,
                    row.user_id.as_deref(),
                    &detail,
                ))
            }
            seed::RevalidateAction::Clear => {
                cleared += 1;
                None
            }
            seed::RevalidateAction::Keep => None,
        };
        steps.push(RevalidateStep {
            id: row.id.clone(),
            action,
            line,
        });
    }
    let summary = seed::revalidate_summary(checked, flagged, cleared);
    RevalidatePlan {
        steps,
        checked,
        flagged,
        cleared,
        summary,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn active_override(section_key: &str, body: &str) -> ActiveOverride {
        ActiveOverride {
            id: "00000000-0000-0000-0000-000000000001".to_owned(),
            workspace_id: "00000000-0000-0000-0000-000000000002".to_owned(),
            user_id: None,
            section_key: section_key.to_owned(),
            body: body.to_owned(),
            version: 1,
            needs_attention: false,
        }
    }

    /// A section key the embedded registry really carries that is also
    /// usable in a recipe (unlocked, and referenced by at least one kind
    /// so validation does not fail closed on the no-recipe rule), and
    /// whose override is not flagged for dropping a required step.
    fn known_key() -> String {
        use crate::prompting::{registry, validation};
        let sections = registry::all_sections();
        let section = sections
            .iter()
            .find(|s| {
                !s.is_locked()
                    && !validation::kinds_for_section(s.key).is_empty()
                    && !seed::LEAN_WORKFLOW_REQUIRED_STEPS
                        .iter()
                        .any(|(key, _)| *key == s.key)
            })
            .expect("registry carries a usable section");
        section.key.to_owned()
    }

    #[test]
    fn empty_scan_summarizes_zeros() {
        let plan = plan_revalidate(&[], false);
        assert!(plan.steps.is_empty());
        assert_eq!(plan.checked, 0);
        assert_eq!(
            plan.summary,
            "checked 0 active override(s): 0 newly flagged, 0 cleared."
        );
    }

    /// Unknown keys flag without rendering (`:40-41`); the literal
    /// A renderable core-section override that still carries git/PR
    /// wording is flagged with the neutrality detail; a work-type
    /// section override with the same body is not a neutrality problem
    /// (PDASHOSS01-234) — it is flagged only because it still uses the
    /// procedure the lean workflow change retired (design §9.5).
    #[test]
    fn non_neutral_core_override_flags_with_neutrality_detail() {
        let body = "Commit and push the branch, then open a PR.";
        let mut work_type_row = active_override(
            "software.execute",
            "Commit and push the branch, open a PR, then `pidash run yield --outcome done`.",
        );
        work_type_row.id = "00000000-0000-0000-0000-000000000003".to_owned();
        let plan = plan_revalidate(
            &[active_override("implementation", body), work_type_row],
            false,
        );
        assert_eq!(plan.flagged, 2);
        assert_eq!(plan.steps[0].action, seed::RevalidateAction::Flag);
        let line = plan.steps[0].line.as_deref().expect("flagged line");
        assert!(
            line.ends_with(
                "override for core section 'implementation' carries work-type-specific wording \
                 ('branch', 'Commit', 'push', 'PR'); core sections are shared by every work type, \
                 so move that guidance into a work-type section (e.g. 'software.execute')"
            ),
            "{line}"
        );
        assert_eq!(plan.steps[1].action, seed::RevalidateAction::Flag);
        assert!(plan.steps[1]
            .line
            .as_deref()
            .expect("flagged line")
            .ends_with(seed::LEAN_WORKFLOW_REWRITTEN_DETAIL));
    }

    /// Overrides on sections the lean workflow change rewrote are flagged
    /// while they still use the retired procedure; once an admin rewrites
    /// one, it is no longer flagged and `--clear` clears the old flag
    /// (design §9.5). An override on a new section key is kept.
    #[test]
    fn rewritten_section_overrides_flag_for_review() {
        let old = "When done, run `pidash run yield --outcome done --stop-ticking`.";
        // Up to date: no retired wording, and keeps the workpad step that
        // only `implementation` carries.
        let rewritten = "Plain text with no Jinja. Note it in the workpad's Keep in mind.";
        for key in seed::LEAN_WORKFLOW_REWRITTEN_KEYS {
            let plan = plan_revalidate(&[active_override(key, old)], false);
            assert_eq!(plan.steps[0].action, seed::RevalidateAction::Flag, "{key}");
            assert_eq!(
                plan.steps[0].line.as_deref(),
                Some(
                    format!(
                        "flagged 00000000-0000-0000-0000-000000000002/{key} (user=None): \
                         section rewritten by the lean workflow change; review this override"
                    )
                    .as_str()
                ),
            );
            // Still old and already flagged: `--clear` keeps the flag.
            let mut flagged = active_override(key, old);
            flagged.needs_attention = true;
            let plan = plan_revalidate(&[flagged], true);
            assert_eq!(plan.steps[0].action, seed::RevalidateAction::Keep, "{key}");
            // Rewritten by an admin: not flagged again...
            let plan = plan_revalidate(&[active_override(key, rewritten)], false);
            assert_eq!(plan.steps[0].action, seed::RevalidateAction::Keep, "{key}");
            // ...and an old flag on it clears with `--clear`.
            let mut stale = active_override(key, rewritten);
            stale.needs_attention = true;
            let plan = plan_revalidate(&[stale], true);
            assert_eq!(plan.steps[0].action, seed::RevalidateAction::Clear, "{key}");
        }
        let plan = plan_revalidate(&[active_override("review", rewritten)], false);
        assert_eq!(plan.steps[0].action, seed::RevalidateAction::Keep);
        // An override still teaching the removed hand-off is flagged.
        let handoff = "Write the hand-off and pass it with --handoff-file.";
        let plan = plan_revalidate(&[active_override("review", handoff)], false);
        assert_eq!(plan.steps[0].action, seed::RevalidateAction::Flag);
    }

    /// An `implementation` override with its own guidelines and none of
    /// the retired wording still drops the workpad step, so it is flagged.
    #[test]
    fn implementation_override_without_the_workpad_note_is_flagged() {
        let custom = "Follow our team guidelines: small steps, checks first.";
        let plan = plan_revalidate(&[active_override("implementation", custom)], false);
        assert_eq!(plan.steps[0].action, seed::RevalidateAction::Flag);
        // Other rewritten sections have no such step: the same body is kept.
        let plan = plan_revalidate(&[active_override("autonomy", custom)], false);
        assert_eq!(plan.steps[0].action, seed::RevalidateAction::Keep);
    }

    /// `None` user matches Python's f-string of `None`.
    #[test]
    fn unknown_section_key_flags_without_rendering() {
        let row = active_override("no-such-section", "{{ anything }}");
        let plan = plan_revalidate(&[row], false);
        assert_eq!(plan.checked, 1);
        assert_eq!(plan.flagged, 1);
        assert_eq!(plan.cleared, 0);
        assert_eq!(plan.steps[0].action, seed::RevalidateAction::Flag);
        assert_eq!(
            plan.steps[0].line.as_deref(),
            Some(
                "flagged 00000000-0000-0000-0000-000000000002/no-such-section \
                 (user=None): section key no longer exists"
            )
        );
        assert_eq!(
            plan.summary,
            "checked 1 active override(s): 1 newly flagged, 0 cleared."
        );
    }

    #[test]
    fn clean_unflagged_row_is_kept_silently() {
        let row = active_override(&known_key(), "Plain text with no Jinja.");
        let plan = plan_revalidate(&[row], false);
        assert_eq!((plan.flagged, plan.cleared), (0, 0));
        assert_eq!(plan.steps[0].action, seed::RevalidateAction::Keep);
        assert_eq!(plan.steps[0].line, None);
    }

    /// The full flag/clear/no-op matrix (`:54-64`): broken × flagged × clear.
    #[test]
    fn revalidate_flag_clear_matrix() {
        let key = known_key();
        // (broken body?, already flagged?, clear?) → (action, flagged, cleared)
        let cases = [
            (true, false, false, seed::RevalidateAction::Flag, 1, 0),
            (true, false, true, seed::RevalidateAction::Flag, 1, 0),
            (true, true, false, seed::RevalidateAction::Keep, 0, 0),
            (true, true, true, seed::RevalidateAction::Keep, 0, 0),
            (false, false, false, seed::RevalidateAction::Keep, 0, 0),
            (false, false, true, seed::RevalidateAction::Keep, 0, 0),
            (false, true, false, seed::RevalidateAction::Keep, 0, 0),
            (false, true, true, seed::RevalidateAction::Clear, 0, 1),
        ];
        for (broken_body, flagged, clear, action, want_flagged, want_cleared) in cases {
            let body = if broken_body {
                "{{ missing.nope }}".to_owned()
            } else {
                "Plain text with no Jinja.".to_owned()
            };
            let mut row = active_override(&key, &body);
            row.needs_attention = flagged;
            let plan = plan_revalidate(&[row], clear);
            assert_eq!(
                plan.steps[0].action, action,
                "broken={broken_body} flagged={flagged} clear={clear}"
            );
            assert_eq!(
                plan.flagged, want_flagged,
                "broken={broken_body} flagged={flagged} clear={clear}"
            );
            assert_eq!(
                plan.cleared, want_cleared,
                "broken={broken_body} flagged={flagged} clear={clear}"
            );
            assert_eq!(
                plan.steps[0].line.is_some(),
                action == seed::RevalidateAction::Flag,
                "only Flag prints a line"
            );
        }
    }

    /// A personal (user-scope) row renders its user id in the flagged line.
    #[test]
    fn flagged_line_carries_user_scope_id() {
        let mut row = active_override("no-such-section", "body");
        row.user_id = Some("00000000-0000-0000-0000-000000000009".to_owned());
        let plan = plan_revalidate(&[row], false);
        assert!(
            plan.steps[0]
                .line
                .as_deref()
                .unwrap_or_default()
                .contains("(user=00000000-0000-0000-0000-000000000009)"),
            "{:?}",
            plan.steps[0].line
        );
    }
}
