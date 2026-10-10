//! Prompting ops decisions: reseed + revalidate orchestration (D-37, stage 7).
//!
//! Ports the `handle()` logic of
//! `apps/api/pi_dash/prompting/management/commands/reseed_{default,review,test}_template.py`
//! (`seed_*(force)` + the `"<name> template: <result>"` line) and
//! `revalidate_section_overrides.py` (scan active rows, flag-or-clear
//! `needs_attention`, print the summary). Fixture F37-09
//! (`rust-api/fixtures/ops/commands/prompting.golden.json`).
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
//! * `force` with an unchanged body reports `skipped` (no write).
//! * A revalidate row with `user_id=NULL` renders as the literal string
//!   `None` in the flagged line (Python f-string of `None`).
//! * An unknown section key is broken *without* rendering
//!   (`revalidate_section_overrides.py:40-41`).
//! * An already-flagged broken row stays flagged silently (no recount);
//!   a clean flagged row only clears under `--clear`.
//! * Validation runs against the initial snapshot: flagging only flips
//!   `needs_attention`, which the validator never reads, so planning the
//!   whole scan before writing matches Python's row-by-row loop exactly.

use super::super::prompting::{composer, seed};

/// The three global templates the reseed commands own
/// (`seed.py:29,99` + `models.py:20`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TemplateKind {
    Default,
    Review,
    Test,
}

impl TemplateKind {
    /// The `PromptTemplate.name` row this kind seeds
    /// (`coding-task` / `review` / `test`).
    pub fn name(self) -> &'static str {
        match self {
            TemplateKind::Default => seed::DEFAULT_TEMPLATE_NAME,
            TemplateKind::Review => seed::REVIEW_TEMPLATE_NAME,
            TemplateKind::Test => seed::TEST_TEMPLATE_NAME,
        }
    }

    /// The command-output word (`default` / `review` / `test` in
    /// `"<name> template: <result>"`).
    pub fn label(self) -> &'static str {
        match self {
            TemplateKind::Default => "default",
            TemplateKind::Review => "review",
            TemplateKind::Test => "test",
        }
    }

    /// The fresh body for this kind: the assembled fragments for
    /// `default`, the embedded const bodies for `review` / `test`
    /// (`seed.py:200-212`). The fragment sources are the same
    /// `NN_*.md` mirrors the prompting domain embeds — this module
    /// reads them through the kernels, never from its own copies.
    pub fn body(self) -> String {
        match self {
            TemplateKind::Default => seed::read_default_body(),
            TemplateKind::Review => seed::read_review_body().to_owned(),
            TemplateKind::Test => seed::read_test_body().to_owned(),
        }
    }
}

/// The planned reseed: what to write (if anything) and what to print.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReseedPlan {
    /// The fresh body the edge inserts or refreshes with.
    pub body: String,
    pub outcome: seed::SeedOutcome,
    /// The version to write: `Some(1)` on create, the bumped version on
    /// refresh, `None` when nothing is written.
    pub version: Option<i32>,
    /// The stdout line: `"<label> template: <result>"`.
    pub line: String,
}

/// Pure half of `reseed_*_template.handle` (`seed.py:215-308` + the
/// command's stdout line): decide from the preloaded global row (or
/// `None`), the fresh body and the `--force` flag.
pub fn plan_reseed(
    kind: TemplateKind,
    existing: Option<&seed::ExistingTemplate>,
    force: bool,
) -> ReseedPlan {
    let body = kind.body();
    let (outcome, version) = seed::decide_seed(existing, &body, force);
    let line = seed::reseed_message(kind.label(), outcome);
    ReseedPlan {
        body,
        outcome,
        version,
        line,
    }
}

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

    fn existing(body: &str, version: i32) -> seed::ExistingTemplate {
        seed::ExistingTemplate {
            body: body.to_owned(),
            version: Some(version),
        }
    }

    /// `seed.py:215-308` decision table, per kind: missing → created v1;
    /// present → skipped; force + different → refreshed v+1; force +
    /// identical → skipped (the quirk).
    #[test]
    fn reseed_decision_table_per_kind() {
        for kind in [
            TemplateKind::Default,
            TemplateKind::Review,
            TemplateKind::Test,
        ] {
            let body = kind.body();
            let plan = plan_reseed(kind, None, false);
            assert_eq!(plan.outcome, seed::SeedOutcome::Created);
            assert_eq!(plan.version, Some(1));
            assert_eq!(plan.body, body);

            let same = existing(&body, 3);
            for force in [false, true] {
                let plan = plan_reseed(kind, Some(&same), force);
                assert_eq!(
                    plan.outcome,
                    seed::SeedOutcome::Skipped,
                    "{kind:?} force={force}"
                );
                assert_eq!(plan.version, None);
            }

            let stale = existing("stale body", 3);
            let plan = plan_reseed(kind, Some(&stale), false);
            assert_eq!(plan.outcome, seed::SeedOutcome::Skipped);
            assert_eq!(plan.version, None);
            let plan = plan_reseed(kind, Some(&stale), true);
            assert_eq!(plan.outcome, seed::SeedOutcome::Refreshed);
            assert_eq!(plan.version, Some(4));
        }
    }

    #[test]
    fn reseed_line_names_kind_and_outcome() {
        for (kind, label) in [
            (TemplateKind::Default, "default"),
            (TemplateKind::Review, "review"),
            (TemplateKind::Test, "test"),
        ] {
            assert_eq!(kind.name(), kind_name(kind));
            assert_eq!(kind.label(), label);
            let plan = plan_reseed(kind, None, false);
            assert_eq!(plan.line, format!("{label} template: created"));
        }
    }

    fn kind_name(kind: TemplateKind) -> &'static str {
        match kind {
            TemplateKind::Default => "coding-task",
            TemplateKind::Review => "review",
            TemplateKind::Test => "test",
        }
    }

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
    /// so validation does not fail closed on the no-recipe rule).
    fn known_key() -> String {
        use crate::prompting::{registry, validation};
        let sections = registry::all_sections();
        let section = sections
            .iter()
            .find(|s| !s.is_locked() && !validation::kinds_for_section(s.key).is_empty())
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
