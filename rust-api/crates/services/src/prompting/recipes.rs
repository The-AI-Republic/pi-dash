//! Prompt recipes — ordered section lists per prompt kind.
//!
//! Port of `apps/api/pi_dash/prompting/recipes.py` (189 lines):
//!
//! * `recipes.py:24-28` (kind constants) + `30-84` (`RECIPES`).
//! * `recipes.py:88-129` (`CLOUD_RECIPES`).
//! * `recipes.py:135` (`WORK_KIND_CODING`) + `142-152` (`kind_for`).
//! * `recipes.py:163-178` (`MANAGED_RECIPES` alias + `recipe_for`).
//! * `recipes.py:181-189` (`cloud_recipe_for`, `all_kinds`).
//!
//! `recipe_for` takes the executor kind as a plain string: the Python
//! compares against `AgentExecutorKind.MANAGED_RUNNER`
//! (`pi_dash/core/agent_execution.py:16`, value `"managed_runner"`), so
//! [`EXECUTOR_MANAGED_RUNNER`] carries that value and any other (or no)
//! executor resolves to the local table.
//!
//! Issue-stage recipes carry named [`RecipeEntry::Slot`] entries alongside
//! core section keys (PDASHOSS01-234). A slot is filled at compose time with
//! the section the issue's **work type** supplies for it
//! ([`super::work_types::expand`]) — e.g. the `execute` slot of the In
//! Progress recipe resolves to `software.execute` for the `software` work
//! type. Core sections stay work-type-neutral.
//!
//! No ported bugs: the `bugs` array of `FIX-recipes` is empty.

/// Prompt kinds (`recipes.py:24-28`).
pub const KIND_CODING_TASK: &str = "coding-task";
pub const KIND_REVIEW: &str = "review";
pub const KIND_TEST: &str = "test";
pub const KIND_SCHEDULER: &str = "scheduler";
pub const KIND_DIRECT: &str = "direct";

/// Legacy name for the default work axis value (`recipes.py`
/// `WORK_KIND_CODING`). The §9.5 work-kind axis shipped as **work type**
/// ([`super::work_types`]): the stage keeps selecting the kind/recipe
/// unchanged, and the work type only fills the recipe's slots at compose
/// time. Kept so `kind_for` callers stay stable.
pub const WORK_KIND_CODING: &str = "coding";

/// One recipe entry (`recipes.py` `Slot` + plain section keys): a core
/// section key, or a named hole filled by the issue's work type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecipeEntry {
    /// A core section key.
    Section(&'static str),
    /// A work-type slot; the name is one of
    /// [`super::work_types::SLOT_NAMES`].
    Slot(&'static str),
}

use RecipeEntry::{Section as S, Slot};

/// `Slot("context")` (`recipes.py` `SLOT_CONTEXT`).
pub const SLOT_CONTEXT: RecipeEntry = Slot("context");
/// `Slot("execute")` (`recipes.py` `SLOT_EXECUTE`).
pub const SLOT_EXECUTE: RecipeEntry = Slot("execute");
/// `Slot("review")` (`recipes.py` `SLOT_REVIEW`).
pub const SLOT_REVIEW: RecipeEntry = Slot("review");
/// `Slot("test")` (`recipes.py` `SLOT_TEST`).
pub const SLOT_TEST: RecipeEntry = Slot("test");

/// Executor kind value for the desktop-bundled managed runner
/// (`AgentExecutorKind.MANAGED_RUNNER`).
pub const EXECUTOR_MANAGED_RUNNER: &str = "managed_runner";

/// Raised when a kind has no registered recipe (`recipes.py:138-139`).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{message}")]
pub struct RecipeNotFound {
    message: String,
}

impl RecipeNotFound {
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

/// Local-runner recipes: section keys and work-type slots per kind, in
/// order (`recipes.py` `RECIPES`). Review and test share the lifecycle, the
/// work-type context block, the inlined workpad, and the blocking flow with
/// the coding task.
pub const RECIPES: &[(&str, &[RecipeEntry])] = &[
    (
        KIND_CODING_TASK,
        &[
            S("intro"),
            SLOT_CONTEXT,
            S("relationships"),
            S("session-framing"),
            S("pidash-cli"),
            S("task-lifecycle"),
            S("default-posture"),
            S("autonomy"),
            S("state-routing"),
            S("analyze-and-scope"),
            S("workpad-setup"),
            SLOT_EXECUTE,
            S("implementation"),
            S("blocking"),
            S("guardrails"),
            S("workpad-template"),
            S("ending-run"),
        ],
    ),
    (
        KIND_REVIEW,
        &[
            S("review-intro"),
            SLOT_CONTEXT,
            S("session-framing"),
            S("pidash-cli"),
            S("task-lifecycle"),
            S("workpad-context"),
            S("review-cycle"),
            SLOT_REVIEW,
            S("blocking"),
            S("guardrails"),
            S("ending-run"),
        ],
    ),
    (
        KIND_TEST,
        &[
            S("test-intro"),
            SLOT_CONTEXT,
            S("session-framing"),
            S("pidash-cli"),
            S("task-lifecycle"),
            S("workpad-context"),
            S("test-cycle"),
            SLOT_TEST,
            S("blocking"),
            S("guardrails"),
            S("ending-run"),
        ],
    ),
    (
        KIND_SCHEDULER,
        &[
            S("scheduler-intro"),
            S("session-framing"),
            S("pidash-cli"),
            S("scheduler-task"),
            S("guardrails"),
            S("scheduler-ending"),
        ],
    ),
];

/// Locked executor-owned recipes, sharing no local Runner section
/// (`recipes.py:88-129`).
pub const CLOUD_RECIPES: &[(&str, &[&str])] = &[
    (
        KIND_CODING_TASK,
        &[
            "cloud-intro",
            "cloud-capabilities",
            "cloud-issue-context",
            "cloud-execution-loop",
            "cloud-write-policy",
            "cloud-ending",
        ],
    ),
    (
        KIND_REVIEW,
        &[
            "cloud-review-intro",
            "cloud-capabilities",
            "cloud-issue-context",
            "cloud-review-loop",
            "cloud-write-policy",
            "cloud-ending",
        ],
    ),
    (
        KIND_TEST,
        &[
            "cloud-test-intro",
            "cloud-capabilities",
            "cloud-issue-context",
            "cloud-test-loop",
            "cloud-write-policy",
            "cloud-ending",
        ],
    ),
    (
        KIND_SCHEDULER,
        &[
            "cloud-scheduler-intro",
            "cloud-project-context",
            "cloud-capabilities",
            "cloud-scheduler-task",
            "cloud-scheduler-loop",
            "cloud-write-policy",
            "cloud-ending",
        ],
    ),
    (
        KIND_DIRECT,
        &[
            "cloud-intro",
            "cloud-capabilities",
            "cloud-direct-task",
            "cloud-execution-loop",
            "cloud-write-policy",
            "cloud-ending",
        ],
    ),
];

/// Recipes for the desktop-bundled managed runner (`recipes.py:163`).
/// An alias of the local map, not a copy: a managed runner has exactly
/// the same capabilities as a user-installed one.
pub const MANAGED_RECIPES: &[(&str, &[RecipeEntry])] = RECIPES;

/// Resolve a prompt *kind* from a phase template name (`recipes.py`
/// `kind_for`). An identity on `template_name` and stays one: the work-type
/// axis landed as recipe slots ([`super::work_types::expand`]) rather than
/// as extra kinds, so the kind — and everything stamped from it
/// (`phase_kind`, ticking, the outcome guard) — depends only on the stage.
/// `work_kind` is kept for signature compatibility and is ignored.
pub fn kind_for<'a>(template_name: &'a str, work_kind: &str) -> &'a str {
    let _ = work_kind;
    template_name
}

/// Recipe entries (section keys and slots) for `kind` on `executor_kind`
/// (default local runner: pass `None`) (`recipes.py` `recipe_for`). Resolve
/// slots with [`super::work_types::expand`] before composing. Cloud recipes
/// are deliberately not reachable here; use [`cloud_recipe_for`].
pub fn recipe_for(
    kind: &str,
    executor_kind: Option<&str>,
) -> Result<&'static [RecipeEntry], RecipeNotFound> {
    let table = if executor_kind == Some(EXECUTOR_MANAGED_RUNNER) {
        MANAGED_RECIPES
    } else {
        RECIPES
    };
    table
        .iter()
        .find_map(|(name, sections)| (*name == kind).then_some(*sections))
        .ok_or_else(|| RecipeNotFound::new(format!("no recipe for kind '{kind}'")))
}

/// Section keys for `kind` on the locked Cloud Agent recipes
/// (`recipes.py:181-185`).
pub fn cloud_recipe_for(kind: &str) -> Result<&'static [&'static str], RecipeNotFound> {
    CLOUD_RECIPES
        .iter()
        .find_map(|(name, sections)| (*name == kind).then_some(*sections))
        .ok_or_else(|| RecipeNotFound::new(format!("no Cloud Agent recipe for kind '{kind}'")))
}

/// All local recipe kinds, in table order (`recipes.py:188-189`).
pub fn all_kinds() -> Vec<&'static str> {
    RECIPES.iter().map(|(name, _)| *name).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    fn fixture() -> Value {
        let path = format!(
            "{}/../../fixtures/prompting/FIX-recipes.json",
            env!("CARGO_MANIFEST_DIR")
        );
        serde_json::from_str(&std::fs::read_to_string(&path).expect("golden exists"))
            .expect("golden parses")
    }

    fn recipes_data(fixture: &Value) -> &Value {
        fixture
            .get("data")
            .and_then(|data| data.get("recipes"))
            .expect("fixture carries data.recipes")
    }

    fn str_list(value: &Value) -> Vec<String> {
        value
            .as_array()
            .expect("list is an array")
            .iter()
            .map(|entry| entry.as_str().expect("entry is a string").to_owned())
            .collect()
    }

    #[test]
    fn kind_constants_match_fixture() {
        let golden = fixture();
        let kinds = &recipes_data(&golden)["kinds"];
        assert_eq!(kinds["coding_task"].as_str(), Some(KIND_CODING_TASK));
        assert_eq!(kinds["review"].as_str(), Some(KIND_REVIEW));
        assert_eq!(kinds["test"].as_str(), Some(KIND_TEST));
        assert_eq!(kinds["scheduler"].as_str(), Some(KIND_SCHEDULER));
        assert_eq!(kinds["direct"].as_str(), Some(KIND_DIRECT));
        assert_eq!(kinds["work_kind_coding"].as_str(), Some(WORK_KIND_CODING));
    }

    /// A fixture recipe entry: a section key string, or `{"slot": name}`.
    fn entry_from_fixture(value: &Value) -> RecipeEntry {
        if let Some(key) = value.as_str() {
            return RecipeEntry::Section(Box::leak(key.to_owned().into_boxed_str()));
        }
        let slot = value["slot"].as_str().expect("entry is a key or a slot");
        RecipeEntry::Slot(Box::leak(slot.to_owned().into_boxed_str()))
    }

    #[test]
    fn local_recipes_match_fixture() {
        let golden = fixture();
        let data = recipes_data(&golden);
        for (kind, entries) in RECIPES {
            let expected: Vec<RecipeEntry> = data["local_recipes"][kind]
                .as_array()
                .expect("recipe is an array")
                .iter()
                .map(entry_from_fixture)
                .collect();
            assert_eq!(*entries, &expected[..], "local recipe for {kind}");
        }
        let lengths = &data["local_recipe_lengths"];
        for (kind, sections) in RECIPES {
            assert_eq!(
                sections.len(),
                lengths[kind].as_u64().expect("u64") as usize,
                "local recipe length for {kind}"
            );
        }
    }

    #[test]
    fn cloud_recipes_match_fixture() {
        let golden = fixture();
        let data = recipes_data(&golden);
        for (kind, sections) in CLOUD_RECIPES {
            let got: &[&str] = sections;
            let expected = str_list(&data["cloud_recipes"][kind]);
            let expected: Vec<&str> = expected.iter().map(String::as_str).collect();
            assert_eq!(got, &expected[..], "cloud recipe for {kind}");
        }
        let lengths = &data["cloud_recipe_lengths"];
        for (kind, sections) in CLOUD_RECIPES {
            assert_eq!(
                sections.len(),
                lengths[kind].as_u64().expect("u64") as usize,
                "cloud recipe length for {kind}"
            );
        }
    }

    #[test]
    fn work_types_match_fixture() {
        use crate::prompting::work_types;
        let golden = fixture();
        let data = &recipes_data(&golden)["work_types"];
        assert_eq!(
            str_list(&data["slot_names"]),
            work_types::SLOT_NAMES.to_vec()
        );
        assert_eq!(
            data["default"].as_str(),
            Some(work_types::DEFAULT_WORK_TYPE)
        );
        assert_eq!(str_list(&data["keys"]), work_types::keys());
        for work_type in work_types::all_work_types() {
            let golden_type = &data["types"][work_type.key];
            assert_eq!(golden_type["title"].as_str(), Some(work_type.title));
            assert_eq!(
                golden_type["description"].as_str(),
                Some(work_type.description)
            );
            let sections = golden_type["sections"].as_object().expect("sections map");
            assert_eq!(sections.len(), work_type.sections.len());
            for (slot, key) in &work_type.sections {
                assert_eq!(sections[*slot].as_str(), Some(key.as_str()));
            }
        }
        for (kind, entries) in RECIPES {
            for key in work_types::keys() {
                assert_eq!(
                    work_types::expand(entries, key).expect("expands"),
                    str_list(&data["expanded"][kind][key]),
                    "expanded {kind} for {key}"
                );
            }
        }
    }

    #[test]
    fn kind_for_is_identity_on_template_name() {
        let golden = fixture();
        let data = recipes_data(&golden);
        assert_eq!(
            kind_for("coding-task", WORK_KIND_CODING),
            data["kind_for_identity"].as_str().expect("string")
        );
        assert_eq!(
            kind_for("review", WORK_KIND_CODING),
            data["kind_for_with_work_kind"].as_str().expect("string")
        );
    }

    #[test]
    fn managed_recipes_alias_local() {
        // `MANAGED_RECIPES = dict(RECIPES)`: equal content, separate name.
        let golden = fixture();
        assert!(recipes_data(&golden)["managed_is_alias_of_local"]
            .as_bool()
            .expect("bool"));
        assert_eq!(MANAGED_RECIPES.len(), RECIPES.len());
        for ((managed_kind, managed), (local_kind, local)) in
            MANAGED_RECIPES.iter().zip(RECIPES.iter())
        {
            assert_eq!(managed_kind, local_kind);
            assert_eq!(managed, local);
        }
        // The managed executor resolves through the same sections.
        assert_eq!(
            recipe_for(KIND_CODING_TASK, Some(EXECUTOR_MANAGED_RUNNER)).expect("managed recipe"),
            recipe_for(KIND_CODING_TASK, None).expect("local recipe"),
        );
    }

    #[test]
    fn unknown_kinds_raise() {
        // Fixture errors are formatted `"RecipeNotFound: {message}"`.
        let golden = fixture();
        let data = recipes_data(&golden);
        let err = recipe_for("nope", None).expect_err("unknown local kind raises");
        assert_eq!(err.message(), "no recipe for kind 'nope'");
        assert!(data["recipe_for_unknown"]["error"]
            .as_str()
            .expect("string")
            .ends_with(err.message()));
        // `direct` has no local recipe (cloud-only kind).
        assert_eq!(
            data["recipe_for_direct_raises"]["ok"].as_bool(),
            Some(false)
        );
        recipe_for(KIND_DIRECT, None).expect_err("direct has no local recipe");
        let err = cloud_recipe_for("nope").expect_err("unknown cloud kind raises");
        assert_eq!(err.message(), "no Cloud Agent recipe for kind 'nope'");
        assert!(data["cloud_recipe_for_unknown"]["error"]
            .as_str()
            .expect("string")
            .ends_with(err.message()));
    }

    #[test]
    fn all_kinds_lists_local_kinds_in_order() {
        let golden = fixture();
        let expected = str_list(&recipes_data(&golden)["all_kinds"]);
        assert_eq!(
            all_kinds(),
            expected.iter().map(String::as_str).collect::<Vec<_>>()
        );
    }
}
