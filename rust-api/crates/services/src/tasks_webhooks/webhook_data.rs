//! D-08 webhook send-path data dispatch (services layer).
//!
//! Port of `SERIALIZER_MAPPER` / `MODEL_MAPPER`
//! (`apps/api/pi_dash/bgtasks/webhook_task.py:58-80`),
//! `get_issue_prefetches` (`:86-90`) and `get_model_data` (`:143-187`).
//!
//! The serializers themselves belong to their owning domains
//! (issue/project/cycle/module/comment/user/intake); only the class
//! bindings and the dispatch rules are recorded here — no serializer
//! shape is duplicated. [`plan_for`] resolves an event key to the exact
//! fetch + render plan the Python code executes; the caller performs
//! the fetch with its own pool and renders with the owning domain's
//! renderer when it lands.
//!
//! Ported bugs (translate, don't redesign):
//!
//! * BUG-1 (`:169-172`): the serializer-missing check runs *after* the
//!   queryset fetch, so while both mappers share keys it is unreachable.
//!   The order is kept: [`plan_for`] resolves the model first, then the
//!   serializer.
//! * BUG-2 (`:180`): the single-issue path re-fetches with
//!   `.filter(pk).prefetch_related(..).first()` and passes the result to
//!   the serializer with no `None` guard. A missing id raises at the
//!   first `.get`, so `None` is unreachable there — but were it ever
//!   `None`, the code would serialize `None`. The plan always yields a
//!   render step; no guard is added.
//! * QUIRK-1 (`:165`): `many=True` with zero matches does *not* raise:
//!   `filter(pk__in=[])` renders `[]` through the serializer.
//!
//! Evidence: `rust-api/fixtures/tasks_webhooks/fx-web-02-get-model-data.json`.

/// One `SERIALIZER_MAPPER` / `MODEL_MAPPER` row (`webhook_task.py:58-80`).
pub struct Binding {
    /// Event key, e.g. `"issue"`.
    pub event: &'static str,
    /// Django model class name from `MODEL_MAPPER`.
    pub model: &'static str,
    /// Serializer class name from `SERIALIZER_MAPPER`.
    pub serializer: &'static str,
    /// Only `"issue"` carries prefetches + expand context (`:174-182`).
    pub issue_prefetch: bool,
}

/// All nine mapper rows in source order (`:58-80`).
pub const BINDINGS: [Binding; 9] = [
    Binding {
        event: "project",
        model: "Project",
        serializer: "ProjectSerializer",
        issue_prefetch: false,
    },
    Binding {
        event: "issue",
        model: "Issue",
        serializer: "IssueExpandSerializer",
        issue_prefetch: true,
    },
    Binding {
        event: "cycle",
        model: "Cycle",
        serializer: "CycleSerializer",
        issue_prefetch: false,
    },
    Binding {
        event: "module",
        model: "Module",
        serializer: "ModuleSerializer",
        issue_prefetch: false,
    },
    Binding {
        event: "cycle_issue",
        model: "CycleIssue",
        serializer: "CycleIssueSerializer",
        issue_prefetch: false,
    },
    Binding {
        event: "module_issue",
        model: "ModuleIssue",
        serializer: "ModuleIssueSerializer",
        issue_prefetch: false,
    },
    Binding {
        event: "issue_comment",
        model: "IssueComment",
        serializer: "IssueCommentSerializer",
        issue_prefetch: false,
    },
    Binding {
        event: "user",
        model: "User",
        serializer: "UserLiteSerializer",
        issue_prefetch: false,
    },
    Binding {
        event: "intake_issue",
        model: "IntakeIssue",
        serializer: "IntakeIssueSerializer",
        issue_prefetch: false,
    },
];

/// Look up the mapper row for an event key (`MODEL_MAPPER.get(event)`).
pub fn binding_for(event: &str) -> Option<&'static Binding> {
    BINDINGS.iter().find(|binding| binding.event == event)
}

/// `get_issue_prefetches` (`:86-90`): the two `Prefetch` specs, verbatim.
pub const ISSUE_PREFETCH_LABEL: &str =
    "Prefetch('label_issue', queryset=IssueLabel.objects.select_related('label'))";
/// `get_issue_prefetches` (`:86-90`): the two `Prefetch` specs, verbatim.
pub const ISSUE_PREFETCH_ASSIGNEE: &str =
    "Prefetch('issue_assignee', queryset=IssueAssignee.objects.select_related('assignee'))";

/// `context={"expand": ["labels", "assignees"]}` (`:182`): issue-only.
pub const ISSUE_EXPAND: [&str; 2] = ["labels", "assignees"];

/// How the queryset is fetched: single `.get(pk=)` vs `.filter(pk__in=)`
/// (`:164-167`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lookup {
    /// `model.objects.get(pk=event_id)` (`many=False`).
    SingleGet,
    /// `model.objects.filter(pk__in=event_id)` (`many=True`).
    ManyFilter,
}

/// The fetch + render plan `get_model_data` executes for one call.
#[derive(Debug)]
pub struct ModelPlan {
    /// Event key as passed in.
    pub event: &'static str,
    /// Model class name (`MODEL_MAPPER`).
    pub model: &'static str,
    /// Serializer class name (`SERIALIZER_MAPPER`).
    pub serializer: &'static str,
    /// Single-get vs many-filter (`:164-167`).
    pub lookup: Lookup,
    /// Issue-only prefetch pair (`:175-182`); false renders with no
    /// context and no prefetches (`:184`).
    pub prefetch: bool,
    /// Issue-only expand context (`:182`); `None` renders bare (`:184`).
    pub expand: Option<&'static [&'static str]>,
}

/// Every failure `get_model_data` raises. Display text matches the
/// Python messages byte for byte.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum Error {
    /// `ValueError(f"Model not found for event: {event}")` (`:160-161`).
    #[error("Model not found for event: {0}")]
    UnknownModel(String),
    /// `ValueError(f"Serializer not found for event: {event}")`
    /// (`:171-172`); unreachable while the tables share keys (BUG-1).
    #[error("Serializer not found for event: {0}")]
    UnknownSerializer(String),
    /// `ObjectDoesNotExist(f"No {event} found with id: {event_id}")`
    /// (`:186`): raised when the single-get fetch misses.
    #[error("No {0} found with id: {1}")]
    NotFound(String, String),
}

/// Resolve the fetch + render plan for `get_model_data(event, event_id,
/// many)` (`:143-187`).
///
/// Mirrors the Python check order: model lookup, then (caller-side)
/// queryset fetch, then serializer lookup (BUG-1). The actual fetch and
/// render are the caller's job — this returns *which* model, *which*
/// lookup, *which* serializer and *which* context.
pub fn plan_for(event: &str, many: bool) -> Result<ModelPlan, Error> {
    let binding = binding_for(event).ok_or_else(|| Error::UnknownModel(event.to_owned()))?;
    // Serializer lookup ordered after the model lookup, as in `:169-172`.
    let serializer = BINDINGS
        .iter()
        .find(|candidate| candidate.event == event)
        .map(|candidate| candidate.serializer)
        .ok_or_else(|| Error::UnknownSerializer(event.to_owned()))?;
    Ok(ModelPlan {
        event: binding.event,
        model: binding.model,
        serializer,
        lookup: if many {
            Lookup::ManyFilter
        } else {
            Lookup::SingleGet
        },
        prefetch: binding.issue_prefetch,
        expand: if binding.issue_prefetch {
            Some(&ISSUE_EXPAND)
        } else {
            None
        },
    })
}

/// Render the `ObjectDoesNotExist` message for a missed single-get
/// (`:186`): `f"No {event} found with id: {event_id}"`.
pub fn not_found(event: &str, event_id: &str) -> Error {
    Error::NotFound(event.to_owned(), event_id.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    /// Committed translation evidence this port replays:
    /// `rust-api/fixtures/tasks_webhooks/fx-web-02-get-model-data.json`.
    static FIXTURE: &str =
        include_str!("../../../../fixtures/tasks_webhooks/fx-web-02-get-model-data.json");

    fn fixture() -> Value {
        serde_json::from_str(FIXTURE).expect("fixture parses")
    }

    #[test]
    fn all_nine_keys_bind_model_and_serializer() {
        let keys = &fixture()["keys"];
        assert_eq!(keys.as_object().map(|map| map.len()), Some(9));
        for binding in &BINDINGS {
            let row = &keys[binding.event];
            assert!(row.is_object(), "fixture covers {}", binding.event);
            assert_eq!(row["model"], Value::String(binding.model.to_owned()));
            assert_eq!(
                row["serializer"],
                Value::String(binding.serializer.to_owned())
            );
        }
    }

    #[test]
    fn only_issue_carries_prefetch_and_expand() {
        for binding in &BINDINGS {
            let plan = plan_for(binding.event, false).expect("known key plans");
            assert_eq!(plan.prefetch, binding.event == "issue");
            assert_eq!(plan.expand.is_some(), binding.event == "issue");
            if binding.event == "issue" {
                assert_eq!(plan.expand, Some(&ISSUE_EXPAND[..]));
                assert_eq!(plan.serializer, "IssueExpandSerializer");
            } else {
                assert_eq!(plan.serializer, binding.serializer);
            }
        }
    }

    #[test]
    fn issue_prefetch_specs_match_fixture_verbatim() {
        let parsed = fixture();
        let prefetches = parsed["issue_prefetch"]["prefetches"]
            .as_array()
            .expect("prefetch list");
        assert_eq!(prefetches.len(), 2);
        assert_eq!(
            prefetches[0],
            Value::String(ISSUE_PREFETCH_LABEL.to_owned())
        );
        assert_eq!(
            prefetches[1],
            Value::String(ISSUE_PREFETCH_ASSIGNEE.to_owned())
        );
        let context: Vec<String> = ISSUE_EXPAND.iter().map(|field| field.to_string()).collect();
        assert_eq!(
            fixture()["issue_prefetch"]["context"],
            serde_json::json!({ "expand": context })
        );
    }

    #[test]
    fn lookup_mode_follows_many_flag() {
        assert_eq!(plan_for("issue", false).unwrap().lookup, Lookup::SingleGet);
        assert_eq!(
            plan_for("project", true).unwrap().lookup,
            Lookup::ManyFilter
        );
        assert_eq!(
            fixture()["lookup"]["many_false"],
            Value::String("model.objects.get(pk=event_id)".to_owned())
        );
        assert_eq!(
            fixture()["lookup"]["many_true"],
            Value::String("model.objects.filter(pk__in=event_id)".to_owned())
        );
    }

    #[test]
    fn unknown_event_reports_model_not_found_first() {
        // Check order (BUG-1): the model lookup runs before the
        // serializer lookup, so an unknown key reports the model.
        let error = plan_for("nonsense", false).expect_err("unknown key fails");
        assert_eq!(error, Error::UnknownModel("nonsense".to_owned()));
        assert_eq!(error.to_string(), "Model not found for event: nonsense");
        assert_eq!(
            fixture()["error_cases"][0]["error"],
            Value::String("ValueError('Model not found for event: <event>')".to_owned())
        );
    }

    #[test]
    fn serializer_lookup_never_misses_while_tables_share_keys() {
        // BUG-1 documents the unreachable branch; every known key must
        // still resolve a serializer.
        for binding in &BINDINGS {
            assert!(plan_for(binding.event, true).is_ok());
        }
        assert_eq!(
            fixture()["error_cases"][1]["error"],
            Value::String("ValueError('Serializer not found for event: <event>')".to_owned())
        );
    }

    #[test]
    fn missed_single_get_message_matches() {
        let error = not_found("issue", "iid-9");
        assert_eq!(error.to_string(), "No issue found with id: iid-9");
        assert_eq!(
            fixture()["error_cases"][2]["error"],
            Value::String("ObjectDoesNotExist('No <event> found with id: <event_id>')".to_owned())
        );
    }

    #[test]
    fn many_with_zero_matches_still_plans_render() {
        // QUIRK-1: filter-then-serialize never raises on empty input;
        // the plan resolves and the owning renderer emits `[]`.
        let plan = plan_for("issue_comment", true).expect("many plans");
        assert_eq!(plan.lookup, Lookup::ManyFilter);
        assert!(plan.expand.is_none());
    }
}
