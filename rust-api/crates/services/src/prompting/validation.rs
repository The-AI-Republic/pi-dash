//! Save-time override validation.
//!
//! Port of `apps/api/pi_dash/prompting/validation.py` (363 lines):
//!
//! * `validation.py:35` (`MAX_BODY_LENGTH`) — [`MAX_BODY_LENGTH`],
//!   restated here exactly like Python restates the registry cap for its
//!   own error message.
//! * `validation.py:38-39` (`OverrideValidationError`) —
//!   [`OverrideValidationError`].
//! * `validation.py:42-220` (`_issue_sample`) + `223-275`
//!   (`_scheduler_sample`) — [`sample_contexts`]: the sample contexts
//!   mirror the keys emitted by the context builders; values are ported
//!   literally (`None` → `null`, empty containers stay empty).
//! * `validation.py:285-294` (`kinds_for_section`) — [`kinds_for_section`].
//! * `validation.py:297-320` (`_compose_with_candidate`) —
//!   `compose_with_candidate` (private): other sections resolve through
//!   the passed override index rather than per-section direct queries —
//!   `FIX-compose` asserts both paths agree
//!   (`precedence.index_vs_fallback_same`), and this crate holds no
//!   database handle for the fallback.
//! * `validation.py:323-363` (`validate_override`) — [`validate_override`].
//!
//! Error-type note (same seam as `composer.rs`): Python lets
//! `PromptRegistryError` / `RecipeNotFound` propagate untouched past the
//! validator. There are no typed catchers yet, so those failures surface
//! here as message-preserving [`OverrideValidationError`]s —
//! `unknown section key: '…'` reads identically; only the Rust type name
//! differs. Likewise the character limit uses `chars().count()`, because
//! Python's `len()` counts code points, not bytes (semantic-traps row).
//!
//! A section that lives in the registry but in no recipe fails closed,
//! exactly like Python (`validation.py:344-352`): it would otherwise save
//! with zero render validation and go live silently if a future recipe
//! adds it. Cloud-only sections therefore raise the recipe lookup
//! message (`no recipe for kind 'direct'`), because `recipe_for`
//! deliberately cannot see the locked Cloud tables — observed Python
//! behavior, ported as-is.

use super::{composer, recipes, registry, renderer};

/// Mirrors `registry::MAX_SECTION_BODY_LENGTH` — restated for the
/// validator's own error message (`validation.py:33-35`).
pub const MAX_BODY_LENGTH: usize = registry::MAX_SECTION_BODY_LENGTH;

/// Raised when a candidate override body fails save-time validation
/// (`validation.py:38-39`).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{message}")]
pub struct OverrideValidationError {
    message: String,
}

impl OverrideValidationError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    /// The validation failure message.
    pub fn message(&self) -> &str {
        &self.message
    }
}

/// Issue-context sample matching the `build_context` keys
/// (`validation.py:42-220`). `kind` lands in `run.kind`, as in Python.
fn issue_sample(kind: &str, populated: bool) -> serde_json::Value {
    if populated {
        serde_json::json!({
            "issue": {
                "id": "00000000-0000-0000-0000-000000000001",
                "identifier": "SAMPLE-1",
                "title": "Sample issue title",
                "description": "Sample description body.",
                "state": "In Progress",
                "state_group": "started",
                "priority": "medium",
                "labels": ["backend", "bug"],
                "assignees": ["Sample Assignee"],
                "url": "/sample/projects/p/issues/i",
                "target_date": "2026-01-01",
                "project_states": [
                    {"name": "In Progress", "group": "started", "description": "Active"},
                    {"name": "In Review", "group": "review", "description": "Awaiting human review"},
                    {"name": "In Test", "group": "test", "description": "Testing / QA"},
                    {"name": "Done", "group": "completed", "description": "Finished"},
                ],
            },
            "workspace": {"slug": "sample-ws", "name": "Sample Workspace"},
            "project": {
                "id": "00000000-0000-0000-0000-000000000002",
                "identifier": "SAMPLE",
                "name": "Sample Project",
                "description": "Sample project description.",
            },
            "repo": {
                "url": "https://example.com/repo.git",
                "base_branch": "main",
                "work_branch": "pi-dash/sample-1",
                "provider": "github",
                "provider_display_name": "GitHub",
                "host_url": "https://github.com",
                "full_name": "sample-org/sample-repo",
                "code_review_term": "pull request",
            },
            "code_reviews": [
                {
                    "url": "https://github.com/sample-org/sample-repo/pull/7",
                    "title": "Sample PR title",
                    "state": "open",
                    "merged": false,
                    "draft": false,
                    "provider": "github",
                    "external_iid": "7",
                },
            ],
            "parent": {
                "identifier": "SAMPLE-0",
                "title": "Parent issue",
                "state": "In Test",
                "work_branch": "pi-dash/sample-0",
                "description": "Parent description.",
                "comments_count": 3,
            },
            "lineage": [
                {"identifier": "SAMPLE-1", "title": "Sample issue title"},
                {"identifier": "SAMPLE-0", "title": "Parent issue"},
                {"identifier": "SAMPLE-root", "title": "Root issue"},
            ],
            "children": [
                {"identifier": "SAMPLE-2", "title": "Child issue", "state": "Backlog"},
            ],
            "related": [
                {"identifier": "SAMPLE-9", "title": "Related issue", "state": "Cancelled"},
            ],
            "blocked_by": [
                {"identifier": "SAMPLE-3", "title": "Blocker issue", "state": "In Review", "state_group": "review"},
            ],
            "blocking": [
                {"identifier": "SAMPLE-4", "title": "Dependent issue", "state": "Todo", "state_group": "unstarted"},
            ],
            "other_relations": [
                {
                    "identifier": "SAMPLE-5",
                    "title": "Implementing issue",
                    "state": "Backlog",
                    "state_group": "backlog",
                    "relation": "Implemented by",
                },
            ],
            "open_blockers": ["SAMPLE-3"],
            "has_open_blockers": true,
            "run": {
                "id": "00000000-0000-0000-0000-000000000003",
                "kind": kind,
                "attempt": 2,
                "turn_number": 1,
                "trigger": "tick",
                "executor_kind": "local_runner",
            },
            "available_tools": [],
            "unavailable_capabilities": [],
            // Populated means every optional branch renders: this run
            // carries deployment-provided toolsets, so the section's
            // extra-tools block and its schema-fetch instruction are both
            // exercised.
            "extra_toolsets": true,
            "extra_toolsets_schema_tool": "sample_get_tool_schema",
            "limits": {},
            "tick": {
                // A wait run is a run: with two waits the cap is the pool
                // plus those two, so 5 of 12 with 7 remaining.
                "count": 5,
                "cap": 12,
                "remaining": 7,
                "waited": 2,
                "wait_allowance": 8,
                "spent": false,
                "clock_live": true,
                "interval_seconds": 10800,
                "interval_human": "3 hours",
            },
            "comments_section": "### Comment 1 — Human: Sample at 2026-01-01\n\nHello.",
            "parent_done_payload": "{\n  \"pr_url\": \"https://example.com/pr/1\"\n}",
            "workpad_body": "### Phase\n- investigating",
        })
    } else {
        // Minimal: every key present, all optionals empty/`None`
        // (`validation.py:165-220`).
        serde_json::json!({
            "issue": {
                "id": "00000000-0000-0000-0000-000000000001",
                "identifier": "SAMPLE-1",
                "title": "",
                "description": "",
                "state": "",
                "state_group": "",
                "priority": "none",
                "labels": [],
                "assignees": [],
                "url": "",
                "target_date": null,
                "project_states": [],
            },
            "workspace": {"slug": "sample-ws", "name": ""},
            "project": {"id": "", "identifier": "SAMPLE", "name": "", "description": ""},
            "repo": {
                "url": null,
                "base_branch": null,
                "work_branch": null,
                "provider": null,
                "provider_display_name": "Git provider",
                "host_url": null,
                "full_name": null,
                "code_review_term": "code review",
            },
            "code_reviews": [],
            "parent": null,
            "lineage": null,
            "children": [],
            "related": [],
            "blocked_by": [],
            "blocking": [],
            "other_relations": [],
            "open_blockers": [],
            "has_open_blockers": false,
            "run": {
                "id": "00000000-0000-0000-0000-000000000003",
                "kind": kind,
                "attempt": 1,
                "turn_number": 1,
                "trigger": "state_transition",
                "executor_kind": "local_runner",
            },
            "available_tools": [],
            "unavailable_capabilities": [],
            "extra_toolsets": false,
            "extra_toolsets_schema_tool": "",
            "limits": {},
            "tick": null,
            "comments_section": "(no comments on this issue yet)",
            "parent_done_payload": "(no parent run done payload available)",
            "workpad_body": "",
        })
    }
}

/// Scheduler-context sample matching the `build_scheduler_context` keys
/// (`validation.py:223-275`).
fn scheduler_sample(populated: bool) -> serde_json::Value {
    if populated {
        serde_json::json!({
            "workspace": {"slug": "sample-ws", "name": "Sample Workspace"},
            "project": {
                "id": "00000000-0000-0000-0000-000000000002",
                "identifier": "SAMPLE",
                "name": "Sample Project",
                "description": "Sample project description.",
            },
            "scheduler": {
                "slug": "nightly-audit",
                "name": "Nightly Audit",
                "description": "Scan for issues nightly.",
            },
            "run": {
                "id": "00000000-0000-0000-0000-000000000003",
                "kind": recipes::KIND_SCHEDULER,
                "attempt": 1,
                "turn_number": 1,
                "executor_kind": "local_runner",
            },
            "available_tools": [],
            "unavailable_capabilities": [],
            // Populated means every optional branch renders: this run
            // carries deployment-provided toolsets, so the section's
            // extra-tools block and its schema-fetch instruction are both
            // exercised.
            "extra_toolsets": true,
            "extra_toolsets_schema_tool": "sample_get_tool_schema",
            "limits": {},
            "scheduler_task_body": "Audit the codebase for TODOs.",
        })
    } else {
        serde_json::json!({
            "workspace": {"slug": "sample-ws", "name": ""},
            "project": {"id": "", "identifier": "SAMPLE", "name": "", "description": ""},
            "scheduler": {"slug": "s", "name": "", "description": ""},
            "run": {
                "id": "00000000-0000-0000-0000-000000000003",
                "kind": recipes::KIND_SCHEDULER,
                "attempt": 1,
                "turn_number": 1,
                "executor_kind": "local_runner",
            },
            "available_tools": [],
            "unavailable_capabilities": [],
            "extra_toolsets": false,
            "extra_toolsets_schema_tool": "",
            "limits": {},
            "scheduler_task_body": "",
        })
    }
}

/// The (populated, minimal) sample contexts for `kind`
/// (`validation.py:278-282`). Scheduler kinds get scheduler samples;
/// every other kind gets issue samples.
pub fn sample_contexts(kind: &str) -> Vec<serde_json::Value> {
    if kind == recipes::KIND_SCHEDULER {
        vec![scheduler_sample(true), scheduler_sample(false)]
    } else {
        vec![issue_sample(kind, true), issue_sample(kind, false)]
    }
}

/// All recipe kinds whose ordered section list contains `section_key`
/// (`validation.py:285-294`): local recipes, then managed (an alias —
/// contributes nothing new), then Cloud, each deduplicated in order.
pub fn kinds_for_section(section_key: &str) -> Vec<&'static str> {
    let mut kinds: Vec<&'static str> = Vec::new();
    for table in [
        recipes::RECIPES,
        recipes::MANAGED_RECIPES,
        recipes::CLOUD_RECIPES,
    ] {
        for (kind, keys) in table {
            if keys.contains(&section_key) && !kinds.contains(kind) {
                kinds.push(*kind);
            }
        }
    }
    kinds
}

/// Assemble `kind` with `section_key` forced to `candidate_body`
/// (`validation.py:297-320`). Other sections resolve normally (existing
/// overrides + defaults) so the candidate is validated in the real
/// assembled context, not in isolation. The candidate is labeled
/// `"candidate"` at version 0, exactly like Python.
fn compose_with_candidate(
    kind: &str,
    section_key: &str,
    candidate_body: &str,
    workspace_id: Option<&str>,
    user_id: Option<&str>,
    index: &composer::OverrideIndex,
) -> Result<String, OverrideValidationError> {
    let section = registry::get_section(section_key)
        .map_err(|e| OverrideValidationError::new(e.message()))?;
    let recipe =
        recipes::recipe_for(kind, None).map_err(|e| OverrideValidationError::new(e.message()))?;
    let mut resolved: Vec<composer::ResolvedSection> = Vec::with_capacity(recipe.len());
    for key in recipe {
        if *key == section_key {
            resolved.push(composer::ResolvedSection {
                key: section.key.to_owned(),
                title: section.title.to_owned(),
                customizable: section.customizable.to_owned(),
                body: candidate_body.to_owned(),
                source: "candidate".to_owned(),
                version: 0,
            });
        } else {
            resolved.push(
                composer::resolve_section(key, workspace_id, user_id, index, None)
                    .map_err(|e| OverrideValidationError::new(e.message()))?,
            );
        }
    }
    Ok(composer::assemble(&resolved).0)
}

/// Validate a candidate override body, raising on the first failure
/// (`validation.py:323-363`):
///
/// 1. Length cap + Jinja syntax parse.
/// 2. For every kind containing the section: assemble the full prompt
///    with the candidate slotted in and render it against the populated
///    AND minimal sample contexts for that kind.
pub fn validate_override(
    section_key: &str,
    candidate_body: &str,
    workspace_id: Option<&str>,
    user_id: Option<&str>,
    index: &composer::OverrideIndex,
) -> Result<(), OverrideValidationError> {
    let section = registry::get_section(section_key)
        .map_err(|e| OverrideValidationError::new(e.message()))?;
    if section.is_locked() {
        return Err(OverrideValidationError::new(format!(
            "section '{section_key}' is locked and cannot be overridden"
        )));
    }
    let len = candidate_body.chars().count();
    if len > MAX_BODY_LENGTH {
        return Err(OverrideValidationError::new(format!(
            "override body exceeds {MAX_BODY_LENGTH}-character limit (got {len} characters)"
        )));
    }
    if let Err(err) = renderer::validate_syntax(candidate_body) {
        return Err(OverrideValidationError::new(format!(
            "invalid Jinja syntax: {}",
            err.message()
        )));
    }

    let kinds = kinds_for_section(section_key);
    if kinds.is_empty() {
        // Fail closed: a section present in the registry but in no recipe
        // would otherwise be saved with zero render validation, and
        // silently become live if a future recipe adds it. Refuse rather
        // than save unvalidated.
        return Err(OverrideValidationError::new(format!(
            "section '{section_key}' is not used by any prompt kind; \
             it cannot be overridden until a recipe references it"
        )));
    }
    for kind in kinds {
        let template_body = compose_with_candidate(
            kind,
            section_key,
            candidate_body,
            workspace_id,
            user_id,
            index,
        )?;
        for ctx in sample_contexts(kind) {
            if let Err(err) = renderer::render(&template_body, &ctx) {
                return Err(OverrideValidationError::new(format!(
                    "override for section '{section_key}' fails to render \
                     as part of the '{kind}' prompt: {}",
                    err.message()
                )));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    fn fixture() -> Value {
        let path = format!(
            "{}/../../fixtures/prompting/FIX-compose.json",
            env!("CARGO_MANIFEST_DIR")
        );
        serde_json::from_str(&std::fs::read_to_string(&path).expect("golden exists"))
            .expect("golden parses")
    }

    fn validation_data(fixture: &Value) -> &Value {
        fixture
            .get("data")
            .and_then(|data| data.get("validation"))
            .expect("fixture carries data.validation")
    }

    fn sorted_keys(value: &Value) -> Vec<String> {
        let obj = value.as_object().expect("context is an object");
        let mut keys: Vec<String> = obj.keys().cloned().collect();
        keys.sort();
        keys
    }

    #[test]
    fn sample_context_keys_match_fixture() {
        // `FIX-compose / validation.{issue,scheduler}_sample_keys_*`.
        let golden = fixture();
        let data = validation_data(&golden);
        let issue = sample_contexts(recipes::KIND_CODING_TASK);
        assert_eq!(issue.len(), 2);
        for (ctx, key) in issue.iter().zip(["populated", "minimal"]) {
            let mut expected: Vec<String> = data[format!("issue_sample_keys_{key}")]
                .as_array()
                .expect("key list")
                .iter()
                .map(|k| k.as_str().expect("key").to_owned())
                .collect();
            expected.sort();
            assert_eq!(sorted_keys(ctx), expected, "issue {key} keys");
        }
        let scheduler = sample_contexts(recipes::KIND_SCHEDULER);
        assert_eq!(scheduler.len(), 2);
        for (ctx, key) in scheduler.iter().zip(["populated", "minimal"]) {
            let mut expected: Vec<String> = data[format!("scheduler_sample_keys_{key}")]
                .as_array()
                .expect("key list")
                .iter()
                .map(|k| k.as_str().expect("key").to_owned())
                .collect();
            expected.sort();
            assert_eq!(sorted_keys(ctx), expected, "scheduler {key} keys");
        }
        // The populated issue run carries the requested kind, like
        // `_issue_sample(kind, ...)` (`validation.py:129-136`).
        assert_eq!(
            issue[0]["run"]["kind"],
            Value::String("coding-task".to_owned())
        );
        assert_eq!(
            scheduler[0]["run"]["kind"],
            Value::String(recipes::KIND_SCHEDULER.to_owned())
        );
    }

    #[test]
    fn sample_contexts_cover_every_kind() {
        // `FIX-compose / validation.sample_contexts_counts`.
        let golden = fixture();
        let counts = &validation_data(&golden)["sample_contexts_counts"];
        for kind in ["coding-task", "review", "scheduler", "direct"] {
            assert_eq!(
                sample_contexts(kind).len(),
                counts[kind].as_u64().expect("count") as usize,
                "two samples for {kind}"
            );
        }
    }

    #[test]
    fn kinds_for_section_matches_fixture() {
        // `FIX-compose / validation.kinds_for_{intro,cloud_intro,unknown}`.
        let golden = fixture();
        let data = validation_data(&golden);
        let kinds: Vec<String> = kinds_for_section("intro")
            .iter()
            .map(|k| k.to_string())
            .collect();
        let expected: Vec<String> = data["kinds_for_intro"]
            .as_array()
            .expect("array")
            .iter()
            .map(|k| k.as_str().expect("kind").to_owned())
            .collect();
        assert_eq!(kinds, expected);
        let kinds: Vec<String> = kinds_for_section("cloud-intro")
            .iter()
            .map(|k| k.to_string())
            .collect();
        let expected: Vec<String> = data["kinds_for_cloud_intro"]
            .as_array()
            .expect("array")
            .iter()
            .map(|k| k.as_str().expect("kind").to_owned())
            .collect();
        assert_eq!(kinds, expected);
        assert!(kinds_for_section("definitely-not-a-registry-key").is_empty());
        assert_eq!(
            kinds_for_section("no-such-key").len(),
            data["kinds_for_unknown_key"]
                .as_array()
                .expect("array")
                .len()
        );
    }

    #[test]
    fn max_body_length_matches_fixture() {
        let golden = fixture();
        assert_eq!(
            MAX_BODY_LENGTH,
            validation_data(&golden)["max_body_length"]
                .as_u64()
                .expect("cap") as usize
        );
    }

    #[test]
    fn validate_accepts_registry_default_body() {
        // `FIX-compose / validation.validate_accept`: the default body of
        // `autonomy` renders clean in every kind/sample combination.
        let golden = fixture();
        assert!(validation_data(&golden)["validate_accept"]
            .as_str()
            .expect("accept note")
            .contains("returned cleanly"));
        let body = registry::get_section("autonomy")
            .expect("autonomy exists")
            .default_body
            .clone();
        validate_override(
            "autonomy",
            &body,
            Some("ws"),
            Some("user"),
            &composer::OverrideIndex::new(),
        )
        .expect("default body validates");
    }

    #[test]
    fn validate_rejects_locked_section() {
        let err = validate_override(
            "guardrails",
            "anything",
            Some("ws"),
            Some("user"),
            &composer::OverrideIndex::new(),
        )
        .expect_err("locked section refuses");
        assert_eq!(
            err.message(),
            validation_data(&fixture())["validate_rejects"]["locked_section"]
                .as_str()
                .expect("golden message")
                .split_once(": ")
                .expect("prefix")
                .1
        );
    }

    #[test]
    fn validate_rejects_overlong_body() {
        let err = validate_override(
            "autonomy",
            &"x".repeat(MAX_BODY_LENGTH + 1),
            Some("ws"),
            Some("user"),
            &composer::OverrideIndex::new(),
        )
        .expect_err("overlong body refuses");
        assert_eq!(
            err.message(),
            validation_data(&fixture())["validate_rejects"]["too_long"]
                .as_str()
                .expect("golden message")
                .split_once(": ")
                .expect("prefix")
                .1
        );
    }

    #[test]
    fn body_limit_counts_chars_not_bytes() {
        // Python's `len()` counts code points: 100_000 `é` (200_000
        // bytes) is exactly at the cap, so length passes and the plain
        // text renders clean everywhere.
        validate_override(
            "autonomy",
            &"é".repeat(MAX_BODY_LENGTH),
            Some("ws"),
            Some("user"),
            &composer::OverrideIndex::new(),
        )
        .expect("char-counted body validates");
    }

    #[test]
    fn validate_rejects_bad_syntax() {
        let err = validate_override(
            "autonomy",
            "Hello {{!}}",
            Some("ws"),
            Some("user"),
            &composer::OverrideIndex::new(),
        )
        .expect_err("bad syntax refuses");
        let golden = fixture();
        let golden_prefix = validation_data(&golden)["validate_rejects"]["bad_syntax"]
            .as_str()
            .expect("golden message")
            .split_once(": ")
            .expect("prefix")
            .1;
        // Engine prose differs (`unexpected char '!'` is CPython Jinja);
        // the contract is the `invalid Jinja syntax:` attribution.
        let expected_head = golden_prefix.split_once(':').expect("head").0;
        assert!(
            err.message().starts_with(&format!("{expected_head}: ")),
            "unexpected message: {}",
            err.message()
        );
    }

    #[test]
    fn validate_rejects_unknown_variable() {
        let err = validate_override(
            "autonomy",
            "Hello {{ nosuchvar_xyz }}",
            Some("ws"),
            Some("user"),
            &composer::OverrideIndex::new(),
        )
        .expect_err("unknown variable refuses");
        let golden = validation_data(&fixture())["validate_rejects"]["unknown_var"]
            .as_str()
            .expect("golden message")
            .split_once(": ")
            .expect("prefix")
            .1
            .to_owned();
        // The kind/section attribution wrapper is byte-exact; only the
        // trailing engine detail varies in prose (minijinja omits the
        // variable name entirely — renderer engine-message caveat), so
        // split it off.
        let head = golden.split_once(": 'nosuchvar_xyz'").expect("head").0;
        assert!(
            err.message().starts_with(&format!("{head}: ")),
            "unexpected message: {}",
            err.message()
        );
    }

    #[test]
    fn validate_unknown_section_reports_registry_message() {
        // `FIX-compose / validation.validate_rejects.not_in_any_recipe`:
        // the registry lookup runs before validation, so its message
        // survives verbatim.
        let err = validate_override(
            "definitely-not-a-section",
            "anything",
            Some("ws"),
            Some("user"),
            &composer::OverrideIndex::new(),
        )
        .expect_err("unknown section refuses");
        let golden_json = fixture();
        let golden = validation_data(&golden_json)["validate_rejects"]["not_in_any_recipe"]
            .as_str()
            .expect("golden message");
        let expected = golden.split_once(": ").expect("prefix").1;
        assert_eq!(err.message(), expected);
    }
}
