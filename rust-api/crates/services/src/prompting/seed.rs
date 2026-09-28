//! Default/review/test template seeding, reseed commands, override revalidation.
//!
//! Port of `apps/api/pi_dash/prompting/seed.py` (330 lines) and the four
//! management commands under `prompting/management/commands/`, scoped to
//! the lines this issue owns:
//!
//! * `seed.py:29-96` (`REVIEW_TEMPLATE_BODY`) — [`REVIEW_TEMPLATE_BODY`].
//! * `seed.py:105-197` (`TEST_TEMPLATE_BODY`) — [`TEST_TEMPLATE_BODY`].
//! * `seed.py:200-212` (`read_review_body` / `read_test_body` /
//!   `read_default_body`) — [`read_review_body`], [`read_test_body`],
//!   [`read_default_body`]. The default body is composed from the ordered
//!   fragments in `prompting/fragments/`; byte-identical mirrors of the 14
//!   `NN_*.md` files live under `src/prompting/fragments/` and are
//!   embedded with `include_str!` (same pattern as the `sections/`
//!   mirrors in [`super::registry`]).
//! * `seed.py:215-244` (`seed_default_template`) — [`decide_seed`] with
//!   [`DEFAULT_TEMPLATE_NAME`]; `247-274` (`seed_review_template`) and
//!   `277-308` (`seed_test_template`) are the same decision over
//!   [`REVIEW_TEMPLATE_NAME`] / [`TEST_TEMPLATE_NAME`].
//! * `seed.py:311-330` (`seed_default_template_on_migrate`) —
//!   [`migrate_seed_enabled`].
//! * `reseed_default_template.py` / `reseed_review_template.py` /
//!   `reseed_test_template.py` (`handle`: `seed_*(force)` + the
//!   `"<name> template: <result>"` line) — [`reseed_message`].
//! * `revalidate_section_overrides.py` (`handle`: iterate active rows,
//!   unknown key or `OverrideValidationError` → `needs_attention=True`,
//!   `--clear` resets clean rows, never deletes/deactivates) —
//!   [`decide_revalidate`], [`revalidate_summary`], [`flagged_line`],
//!   [`UNKNOWN_SECTION_DETAIL`].
//!
//! DB seam (the same split every sibling port uses — this crate holds no
//! database handle, so no `sqlx` here): the Django ORM queries become
//! contract SQL the DB edge issues, with the pure decision living here:
//!
//! * global lookup: `PromptTemplate.objects.filter(workspace__isnull=True,
//!   name=<name>).order_by("-updated_at").first()` — see
//!   [`global_lookup_sql`]. The fixture records the emitted statement
//!   (`FIX-seed.sql_global_select`); the `$1` placeholder carries the
//!   template name at the edge.
//! * global create (`existing is None`): `workspace=NULL, name, body,
//!   is_active=True, version=1` — see [`global_insert_sql`].
//! * global refresh (`force and existing.body != body`): `body, version
//!   = (version or 0) + 1, is_active=True`, saved with
//!   `update_fields=["body", "version", "is_active", "updated_at"]` — see
//!   [`global_refresh_sql`]. Workspace-scoped rows are never matched (the
//!   lookup filters `workspace__isnull=True`), so `--force` cannot clobber
//!   them (`FIX-seed.db.default.workspace_row_untouched_by_force`).
//! * revalidate scan: `PromptSectionOverride.objects.filter(is_active=True)`
//!   iterated row by row; membership is [`section_known`], validation is
//!   [`validate_row`] (both call the already-ported [`super::registry`] /
//!   [`super::validation`] for real — no reimplementation).
//!
//! Coexistence note: none of the five Python units emits a Celery task or
//! touches the queue (verified by grep: no `delay` / `apply_async` /
//! `enqueue` in any of them), so there is no transactional enqueue and no
//! Celery-format publisher on these paths — the F-09 queue kernel is
//! untouched. The seed writes run inside the caller's transaction at the
//! DB edge, exactly like the Django ORM calls they translate.
//!
//! Ported bugs (translate, don't redesign — from the `bugs` array of
//! `FIX-seed`): none in these lines. Two behaviors that look like bugs
//! are preserved as observed: `force` with an unchanged body reports
//! `skipped` (no write); a revalidate row with `user_id=NULL` renders as
//! the literal string `None` in the flagged line.
//!
//! Semantic traps watched: `len(body)` counts code points
//! (`chars().count()`, never byte length); `str.strip()` also strips
//! U+001C–U+001F (same closure as `composer::assemble`); `existing.version
//! or 0` treats both `NULL` and `0` as `0` ([`refreshed_version`]); an
//! empty-body template still counts as present (presence is row
//! existence, never truthiness).

use super::{registry, validation};

/// Global default template name (`models.py:20`, `PromptTemplate.DEFAULT_NAME`).
pub const DEFAULT_TEMPLATE_NAME: &str = "coding-task";

/// Global review template name (`seed.py:27`).
pub const REVIEW_TEMPLATE_NAME: &str = "review";

/// Global test template name (`seed.py:103`).
pub const TEST_TEMPLATE_NAME: &str = "test";

/// Polymorphic review prompt (`seed.py:29-96`).
pub const REVIEW_TEMPLATE_BODY: &str = r#"You are reviewing the work product of a previous implementation pass.
"Review" can mean different things depending on what was produced.

Issue: {{ issue.title }}
Issue Description: {{ issue.description }}
Recent activity:
{{ comments_section }}
Latest implementation run output (read this carefully —
it is your authoritative record of what was produced):
{{ parent_done_payload }}

Step 1 — Decide what kind of review this is.
Inspect parent_done_payload, the issue description, and the working
tree. Choose ONE:
  (a) CODE — the issue produced a GitHub PR (look for a `pr_url` in
      done_payload, or a feature branch ahead of main).
  (b) DESIGN — the issue produced a design / planning document
      (look for paths under `.ai_design/`, paths in
      `done_payload.design_doc_paths`, or markdown artifacts
      referenced as outputs).
  (c) DESIGN_THEN_CODE — both a design doc AND a PR exist. Review
      the design first, then the code.
  (d) GENERIC — none of the above. Review the work product against
      the issue description and leave a summary on the pidash issue.

If you cannot decide, ask the human via `paused`.

Step 2 — Run the cycle for the chosen kind. All cycles share this
shape:
  i.   Find issues with the work product.
  ii.  Validate your findings (no hallucinations) — re-read the
       artifact, confirm each issue is real, drop any that aren't.
  iii. Read existing reviewer comments (in the PR, in the doc, or
       on the pidash issue depending on kind) and reconcile your
       findings against them.
  iv.  Comment on the validated issues at the appropriate surface:
       - CODE: comments on the GitHub PR (use `gh` CLI).
       - DESIGN: inline comments on the doc, or a structured
         comment on the pidash issue if the doc has no comment
         surface.
       - DESIGN_THEN_CODE: design comments first, then PR comments.
       - GENERIC: a structured comment on the pidash issue.
  v.   If you can fix a confirmed issue and the kind permits it,
       apply the fix and resolve the corresponding comment:
       - CODE: edit, commit, push to the PR branch, resolve the
         PR comment thread.
       - DESIGN: edit the doc and resolve / strike the inline
         comment.
       - GENERIC: usually does NOT auto-apply — leave the summary
         and let the human act.
  vi.  Post a summary back to the pidash issue as a comment:
       confirmed issues found, what you fixed automatically, what
       still needs human action.

Step 3 — Emit a done-signal.
- `completed` = approved, no further automatic ticking needed
  (the review pass is satisfied).
- `blocked` = real issues found that you couldn't auto-fix and
  need human attention.
- `paused` = clarifying question for the human.
- `noop` = nothing has changed since your last review pass.
"#;

/// Polymorphic test prompt (`seed.py:105-197`).
pub const TEST_TEMPLATE_BODY: &str = r#"You are testing the work product of a previous implementation pass —
as its FIRST USER. The consumer of a change may be a human at a
screen, a program calling an API, another service, an operator, or a
reader; your job is to impersonate that consumer and verify by acting
and observing, never by inspecting code or trusting a pipeline. CI
and repo gates corroborate; they are not the verdict. Two questions
decide the pass: does the change achieve its stated goal exercised
from the outside, and do the neighboring flows the user relies on
still work (a short regression smoke, not just the new happy path)?

Issue: {{ issue.title }}
Issue Description: {{ issue.description }}
Recent activity:
{{ comments_section }}
Latest implementation run output (read this carefully — it is your
authoritative record of what was produced, including any acceptance
criteria, pr_url, or design_doc_paths it reported):
{{ parent_done_payload }}

Step 1 — Identify the user, and choose the kind.
Ask: who consumes this change, and through what surface? Inspect
parent_done_payload, the issue description, and the working tree.
Choose ONE:
  (a) AUTOMATED — the user is a program: API client, CLI, library
      caller, or another service (the issue produced code — a PR /
      feature branch). Check out the branch, act as that consumer
      (real calls against a running instance where feasible), and
      run THIS repo's own gates as corroboration — discover them
      from the README/CONTRIBUTING, the CI config, and the package
      manifest (package.json / Makefile / pyproject.toml /
      Cargo.toml / go.mod); never assume a toolchain. Run
      format/lint, types, the targeted unit/integration tests for
      the changed surface, and build. Add missing tests for the
      changed surface.
  (b) UI / EXPLORATORY — the user is a human at a screen. Launch
      the app, drive the changed flow as that human would, check
      the acceptance criteria by observation, and click through the
      adjacent flows the change could have disturbed. Tests and a
      clean build do not substitute for looking at it. If you
      cannot boot the app or drive a browser in this environment,
      say so and emit `blocked` — do not report a false pass.
  (c) OPS / INFRA — the user is an operator (or the deploy
      machinery). Run the operator's procedure: dry-run, validate
      config, apply where safe, health-check, confirm idempotency.
      Ops changes often produce no CI signal at all — your run may
      be the only verification this change gets.
  (d) DESIGN — the user is a reader who must act on the doc. Read
      it cold: internal consistency, open questions resolved,
      implementable/testable from the text alone.
  (e) NON_TECHNICAL / GENERIC — none of the above. Stand in for
      whoever the deliverable is for; verify the stated acceptance
      criteria one by one and report pass/fail per criterion for a
      human to confirm.
To act as the user you need somewhere the software runs: boot it
locally, stand up an ephemeral environment, or use the project's own
pipeline (preview deploy / CI) to obtain a running instance when
local cannot work end-to-end. The pipeline's exit code is a data
point, not the verdict. No route to a running instance = an honest
`blocked` stating exactly what was missing.

If the acceptance criteria are ambiguous or absent, derive a plan
from the description, STATE YOUR ASSUMPTIONS in your comment, and
test against them — or emit `paused` to ask if the deliverable is
high-stakes.

Step 2 — Run the test cycle (uniform across kinds):
  i.   Derive a test plan from the acceptance criteria, plus a short
       regression smoke over the neighboring flows the user relies
       on.
  ii.  Set up the environment for the chosen kind.
  iii. Execute from the user's side of the surface — drive the UI,
       call the API, run the procedure. "The diff looks right" is
       not a test result.
  iv.  Collect evidence (test output, coverage, logs, screenshots).
  v.   Validate your findings — re-run / confirm; never report a
       hallucinated failure.
  vi.  Post a STRUCTURED results comment to the pidash issue:
        - Kind detected and what was tested (scope).
        - Method — commands run / flows exercised.
        - Result — pass/fail per acceptance criterion, with evidence.
        - Defects found — and whether you auto-fixed (pushed to the
          PR branch) or it needs a human / a follow-up issue.
  vii. Optionally push trivial fixes to the PR branch, or file a
       follow-up issue for real defects.

Step 3 — Emit a done-signal.
- `completed` = all tests pass / acceptance criteria met.
- `blocked`   = real defects found that need a human/dev, OR the
                test couldn't be run (missing env / creds / tooling).
- `paused`    = acceptance criteria ambiguous — ask the human.
- `noop`      = nothing changed since the last test pass.
"#;

/// Fragment files in assembly order (lexical = `fragment_paths()` order,
/// `fragments/__init__.py:40-43`). Each entry is `(file_name, bytes)`.
pub const FRAGMENT_FILES: &[(&str, &str)] = &[
    ("01_intro.md", include_str!("fragments/01_intro.md")),
    (
        "02_relationships.md",
        include_str!("fragments/02_relationships.md"),
    ),
    (
        "03_session_framing.md",
        include_str!("fragments/03_session_framing.md"),
    ),
    (
        "04_pidash_cli.md",
        include_str!("fragments/04_pidash_cli.md"),
    ),
    (
        "05_default_posture.md",
        include_str!("fragments/05_default_posture.md"),
    ),
    ("06_autonomy.md", include_str!("fragments/06_autonomy.md")),
    (
        "07_state_routing.md",
        include_str!("fragments/07_state_routing.md"),
    ),
    (
        "08_analyze_and_scope.md",
        include_str!("fragments/08_analyze_and_scope.md"),
    ),
    (
        "09_workpad_setup.md",
        include_str!("fragments/09_workpad_setup.md"),
    ),
    (
        "10_implementation.md",
        include_str!("fragments/10_implementation.md"),
    ),
    ("11_blocking.md", include_str!("fragments/11_blocking.md")),
    (
        "12_guardrails.md",
        include_str!("fragments/12_guardrails.md"),
    ),
    (
        "13_workpad_template.md",
        include_str!("fragments/13_workpad_template.md"),
    ),
    (
        "14_ending_run.md",
        include_str!("fragments/14_ending_run.md"),
    ),
];

/// Strip one fragment body (`fragments/__init__.py:46-49`).
///
/// `read_text` + `.strip()`; Python `str.strip()` also strips U+001C–U+001F,
/// which Rust's `char::is_whitespace` does not — same closure as
/// `composer::assemble`.
pub fn strip_fragment(body: &str) -> &str {
    body.trim_matches(|c: char| c.is_whitespace() || ('\u{1c}'..='\u{1f}').contains(&c))
}

/// Concatenate stripped fragment bodies (`fragments/__init__.py:46-49`:
/// `"\n\n".join(parts) + "\n"`).
pub fn assemble_fragments(bodies: &[&str]) -> String {
    let parts: Vec<&str> = bodies.iter().map(|b| strip_fragment(b)).collect();
    parts.join("\n\n") + "\n"
}

/// Return the review template body (`seed.py:200-203`).
pub fn read_review_body() -> &'static str {
    REVIEW_TEMPLATE_BODY
}

/// Return the test template body (`seed.py:206-209`).
pub fn read_test_body() -> &'static str {
    TEST_TEMPLATE_BODY
}

/// Return the default prompt body, composed from the embedded fragments
/// (`seed.py:212`, `read_default_body` → `assemble()`).
pub fn read_default_body() -> String {
    let bodies: Vec<&str> = FRAGMENT_FILES.iter().map(|(_, b)| *b).collect();
    assemble_fragments(&bodies)
}

/// Seed result strings (`seed.py:215-308` return values).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SeedOutcome {
    Created,
    Refreshed,
    Skipped,
}

impl SeedOutcome {
    /// The exact command-output word (`"created"` / `"refreshed"` / `"skipped"`).
    pub fn as_str(self) -> &'static str {
        match self {
            SeedOutcome::Created => "created",
            SeedOutcome::Refreshed => "refreshed",
            SeedOutcome::Skipped => "skipped",
        }
    }
}

/// The global row the DB edge preloaded (`None` when the lookup finds no
/// row). Only the global (`workspace IS NULL`) row is ever passed here —
/// workspace rows never reach this decision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExistingTemplate {
    pub body: String,
    /// Mirrors the nullable read of `existing.version` before the
    /// `or 0` fallback (`seed.py:237`).
    pub version: Option<i32>,
}

/// The refresh version bump (`seed.py:237`: `existing.version =
/// (existing.version or 0) + 1`). `None` and `0` both yield `1`.
pub fn refreshed_version(existing_version: Option<i32>) -> i32 {
    existing_version.unwrap_or(0) + 1
}

/// Pure half of `seed_*_template` (`seed.py:215-308`): given the preloaded
/// global row (or `None`), the fresh body and the `--force` flag, return
/// the outcome plus the version to write (`Some` on create/refresh).
/// The DB edge performs the write and reports [`SeedOutcome::as_str`].
pub fn decide_seed(
    existing: Option<&ExistingTemplate>,
    body: &str,
    force: bool,
) -> (SeedOutcome, Option<i32>) {
    match existing {
        None => (SeedOutcome::Created, Some(1)),
        Some(row) => {
            if force && row.body != body {
                (SeedOutcome::Refreshed, Some(refreshed_version(row.version)))
            } else {
                (SeedOutcome::Skipped, None)
            }
        }
    }
}

/// Global-row lookup (`seed.py:226-230` and the review/test twins):
/// `filter(workspace__isnull=True, name=$1).order_by("-updated_at").first()`.
/// `$1` carries the template name at the edge.
pub fn global_lookup_sql() -> &'static str {
    "SELECT \"prompt_template\".\"id\", \"prompt_template\".\"workspace_id\", \
     \"prompt_template\".\"name\", \"prompt_template\".\"body\", \
     \"prompt_template\".\"is_active\", \"prompt_template\".\"version\", \
     \"prompt_template\".\"updated_by_id\", \"prompt_template\".\"created_at\", \
     \"prompt_template\".\"updated_at\" \
     FROM \"prompt_template\" \
     WHERE (\"prompt_template\".\"workspace_id\" IS NULL AND \"prompt_template\".\"name\" = $1) \
     ORDER BY \"prompt_template\".\"updated_at\" DESC LIMIT 1"
}

/// Global-row create (`seed.py:231-237` and twins): `workspace=NULL`,
/// `is_active=True`, `version=1`.
pub fn global_insert_sql() -> &'static str {
    "INSERT INTO \"prompt_template\" \
     (\"id\", \"workspace_id\", \"name\", \"body\", \"is_active\", \"version\", \
     \"updated_by_id\", \"created_at\", \"updated_at\") \
     VALUES ($1, NULL, $2, $3, TRUE, 1, NULL, now(), now())"
}

/// Global-row refresh (`seed.py:239-242` and twins):
/// `save(update_fields=["body", "version", "is_active", "updated_at"])`.
pub fn global_refresh_sql() -> &'static str {
    "UPDATE \"prompt_template\" \
     SET \"body\" = $1, \"version\" = $2, \"is_active\" = TRUE, \"updated_at\" = now() \
     WHERE \"id\" = $3"
}

/// Management-command output line (`reseed_*_template.py:handle`):
/// `"<name> template: <result>"` (`name` is `default` / `review` / `test`).
pub fn reseed_message(template: &str, outcome: SeedOutcome) -> String {
    format!("{template} template: {}", outcome.as_str())
}

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

/// Pure half of the revalidate loop (`revalidate_section_overrides.py:handle`):
/// `broken` is "unknown section key, or `validate_override` raised".
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

/// `post_migrate` gate (`seed.py:311-330`, `seed_default_template_on_migrate`):
/// an unrelated app label returns early (the signal fires once per app);
/// `PI_DASH_SKIP_PROMPT_SEED=1` skips; otherwise all three seeds run with
/// `force=False`. A `None` label (direct call, no app config) proceeds.
/// The best-effort swallow (`except Exception` + `print` when `verbosity`)
/// owns to the DB edge, which must never fail a migration on a seed error.
pub fn migrate_seed_enabled(app_label: Option<&str>, skip_seed_env: bool) -> bool {
    if let Some(label) = app_label {
        if label != "prompting" {
            return false;
        }
    }
    if skip_seed_env {
        return false;
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;
    use sha2::{Digest, Sha256};

    fn fixture() -> Value {
        let path = format!(
            "{}/../../fixtures/prompting/FIX-seed.json",
            env!("CARGO_MANIFEST_DIR")
        );
        serde_json::from_str(&std::fs::read_to_string(&path).expect("golden exists"))
            .expect("golden parses")
    }

    fn sha_hex(body: &str) -> String {
        Sha256::digest(body.as_bytes())
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect()
    }

    fn char_len(body: &str) -> usize {
        body.chars().count()
    }

    #[test]
    fn review_body_is_byte_identical() {
        // `FIX-seed / statics.review_body_*` + full `data.review_template_body`.
        let golden = fixture();
        let data = golden.get("data").expect("fixture carries data");
        let expected = data
            .get("review_template_body")
            .and_then(Value::as_str)
            .expect("fixture carries review_template_body");
        assert_eq!(read_review_body(), expected);
        assert_eq!(REVIEW_TEMPLATE_NAME, "review");
        assert_eq!(char_len(REVIEW_TEMPLATE_BODY), 2815);
        assert_eq!(
            sha_hex(REVIEW_TEMPLATE_BODY),
            data["statics"]["review_body_sha256"]
                .as_str()
                .expect("fixture carries review_body_sha256")
        );
        assert_eq!(
            sha_hex(REVIEW_TEMPLATE_BODY),
            "f0677cf99da065f5853a21eef75fadbc2aa693b3ecff3fc5c6740a57011642b4"
        );
    }

    #[test]
    fn review_body_carries_its_template_vars() {
        // `FIX-seed / statics.review_body_vars [true, true, true]`.
        for marker in [
            "{{ issue.title }}",
            "{{ comments_section }}",
            "{{ parent_done_payload }}",
        ] {
            assert!(REVIEW_TEMPLATE_BODY.contains(marker), "missing {marker}");
        }
    }

    #[test]
    fn test_body_is_byte_identical() {
        // `FIX-seed / statics.test_body_*` + full `data.test_template_body`.
        let golden = fixture();
        let data = golden.get("data").expect("fixture carries data");
        let expected = data
            .get("test_template_body")
            .and_then(Value::as_str)
            .expect("fixture carries test_template_body");
        assert_eq!(read_test_body(), expected);
        assert_eq!(TEST_TEMPLATE_NAME, "test");
        assert_eq!(char_len(TEST_TEMPLATE_BODY), 4927);
        assert_eq!(
            sha_hex(TEST_TEMPLATE_BODY),
            "9b5869a316b488062058b28c07fb17952c830b2220c0b6710710267e91428e2f"
        );
    }

    #[test]
    fn default_body_assembles_from_fragments_in_order() {
        // `FIX-seed / statics.default_body_*`: 53594 chars, sha over UTF-8 bytes.
        assert_eq!(FRAGMENT_FILES.len(), 14);
        let names: Vec<&str> = FRAGMENT_FILES.iter().map(|(n, _)| *n).collect();
        let mut sorted = names.clone();
        sorted.sort_unstable();
        assert_eq!(names, sorted, "fragments embed in lexical order");
        let body = read_default_body();
        assert_eq!(DEFAULT_TEMPLATE_NAME, "coding-task");
        assert_eq!(char_len(&body), 53594);
        assert_eq!(
            sha_hex(&body),
            "c4a63023bb1ca4f249017b7e99f2403b7e51cbf9bf8b9910e00bba2e6f0e3331"
        );
        assert!(body.ends_with('\n') && !body.ends_with("\n\n"));
    }

    #[test]
    fn seed_decision_table() {
        // `FIX-seed / db.default + db.review + db.test`: missing -> created v1;
        // present -> skipped; force+different -> refreshed v+1.
        let fresh = "fresh body";
        assert_eq!(
            decide_seed(None, fresh, false),
            (SeedOutcome::Created, Some(1))
        );
        assert_eq!(
            decide_seed(None, fresh, true),
            (SeedOutcome::Created, Some(1))
        );
        let same = ExistingTemplate {
            body: fresh.to_owned(),
            version: Some(1),
        };
        assert_eq!(
            decide_seed(Some(&same), fresh, false),
            (SeedOutcome::Skipped, None)
        );
        assert_eq!(
            decide_seed(Some(&same), fresh, true),
            (SeedOutcome::Skipped, None)
        );
        // Force with an unchanged body reports skipped: no write happens.
        let stale = ExistingTemplate {
            body: "old".to_owned(),
            version: Some(1),
        };
        assert_eq!(
            decide_seed(Some(&stale), fresh, false),
            (SeedOutcome::Skipped, None)
        );
        assert_eq!(
            decide_seed(Some(&stale), fresh, true),
            (SeedOutcome::Refreshed, Some(2))
        );
        // `or 0`: NULL and 0 versions both refresh to 1.
        assert_eq!(refreshed_version(None), 1);
        assert_eq!(refreshed_version(Some(0)), 1);
        assert_eq!(refreshed_version(Some(7)), 8);
        assert_eq!(SeedOutcome::Created.as_str(), "created");
        assert_eq!(SeedOutcome::Refreshed.as_str(), "refreshed");
        assert_eq!(SeedOutcome::Skipped.as_str(), "skipped");
    }

    #[test]
    fn global_lookup_only_matches_global_rows() {
        // Never-clobber invariant: the lookup filters `workspace IS NULL`
        // and orders newest-first, so workspace rows are unreachable.
        let sql = global_lookup_sql();
        assert!(sql.contains("\"prompt_template\".\"workspace_id\" IS NULL"));
        assert!(sql.contains("\"prompt_template\".\"name\" = $1"));
        assert!(sql.contains("ORDER BY \"prompt_template\".\"updated_at\" DESC"));
        let insert = global_insert_sql();
        assert!(insert.contains("NULL") && insert.contains(", TRUE, 1,"));
        let refresh = global_refresh_sql();
        for col in [
            "\"body\" = $1",
            "\"version\" = $2",
            "\"is_active\" = TRUE",
            "\"updated_at\" = now()",
        ] {
            assert!(refresh.contains(col), "missing {col}");
        }
    }

    #[test]
    fn reseed_messages_match_fixture() {
        // `FIX-seed / commands.reseed_*`.
        let golden = fixture();
        let commands = &golden["data"]["commands"];
        let golden_str = |key: &str| {
            commands
                .get(key)
                .and_then(Value::as_str)
                .unwrap_or_else(|| panic!("fixture carries commands.{key}"))
        };
        assert_eq!(
            reseed_message("default", SeedOutcome::Skipped),
            golden_str("reseed_default_no_force")
        );
        assert_eq!(
            reseed_message("default", SeedOutcome::Skipped),
            golden_str("reseed_default_force")
        );
        assert_eq!(
            reseed_message("review", SeedOutcome::Skipped),
            golden_str("reseed_review")
        );
        assert_eq!(
            reseed_message("test", SeedOutcome::Skipped),
            golden_str("reseed_test")
        );
        assert_eq!(
            reseed_message("default", SeedOutcome::Created),
            "default template: created"
        );
        assert_eq!(
            reseed_message("review", SeedOutcome::Refreshed),
            "review template: refreshed"
        );
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

    #[test]
    fn migrate_gate_truth_table() {
        // `seed.py:311-330`: unrelated app -> return; skip env -> return;
        // else all three seeds run with force=False.
        assert!(!migrate_seed_enabled(Some("db"), false));
        assert!(!migrate_seed_enabled(Some("prompting"), true));
        assert!(migrate_seed_enabled(Some("prompting"), false));
        assert!(migrate_seed_enabled(None, false));
        assert!(!migrate_seed_enabled(None, true));
    }
}
