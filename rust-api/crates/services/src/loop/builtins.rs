#![forbid(unsafe_code)]

//! Builtin loop jobs catalog (D-03).
//!
//! Port of `apps/api/pi_dash/loop/builtins.py:17-59`. That module is
//! intentionally Django-free (a dataclass + a list) so the seed data
//! migration can import it without the apps registry — this module keeps
//! the same property: a `&'static` const catalog with no imports beyond
//! the core language.
//!
//! The MVP catalog holds exactly one builtin (`auto-close-merged`,
//! `min_role = 15`, `rrule = "FREQ=DAILY;BYHOUR=3;BYMINUTE=0"`); more ship
//! later as code with no schema change. Seeding (`enabled = false` by
//! `db/migrations/0149_loop_mvp.py:16,44`) belongs to the models layer,
//! not this catalog.

/// One builtin loop job (`loop/builtins.py:17-27`, `BuiltinLoopJob`).
/// `tzid` defaults to `"UTC"` in Python; the default is materialized here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuiltinLoopJob {
    pub slug: &'static str,
    pub name: &'static str,
    pub public_name: &'static str,
    pub public_description: &'static str,
    pub prompt: &'static str,
    pub min_role: i32,
    pub rrule: &'static str,
    pub tzid: &'static str,
}

/// `loop/builtins.py:29-39` (`AUTO_CLOSE_MERGED_PROMPT`), verbatim.
pub const AUTO_CLOSE_MERGED_PROMPT: &str = r#"Review open issues in the projects you can access, oldest first. An issue is a candidate when it references a pull request — in its links, description, or comments. For each candidate, call get_pull_request_status on the PR URL. If — and only if — the tool reports state "merged", move the issue to a state in its project's "completed" state group (use list_states to find one) and add a one-line comment naming the merged PR. If merge state is "unknown" or the issue's state is already in the completed group, leave it untouched. Do not create or delete anything."#;

/// The MVP catalog (`loop/builtins.py:46-59`, `BUILTIN_LOOP_JOBS`):
/// exactly one builtin to validate the wiring end to end.
pub const BUILTIN_LOOP_JOBS: &[BuiltinLoopJob] = &[BuiltinLoopJob {
    slug: "auto-close-merged",
    name: "Auto-close merged-PR issues",
    public_name: "Close issues when their PR merges",
    public_description: "Checks your projects once a day and marks an issue Done when the pull request that implements it has been merged.",
    prompt: AUTO_CLOSE_MERGED_PROMPT,
    min_role: 15,
    rrule: "FREQ=DAILY;BYHOUR=3;BYMINUTE=0",
    tzid: "UTC",
}];

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    fn fixture(name: &str) -> Value {
        let path = format!("{}/../../fixtures/loop/{name}", env!("CARGO_MANIFEST_DIR"));
        serde_json::from_str(&std::fs::read_to_string(&path).expect("golden exists"))
            .expect("golden parses")
    }

    #[test]
    fn builtin_catalog_replays_golden() {
        // Fixture serializers/builtin_catalog.golden.json:
        // `loop/builtins.py:17-59`.
        let golden = fixture("serializers/builtin_catalog.golden.json");
        assert_eq!(golden.get("count").and_then(Value::as_u64), Some(1));
        let jobs = golden
            .get("jobs")
            .and_then(Value::as_array)
            .expect("jobs array");
        assert_eq!(jobs.len(), 1, "exactly one builtin");
        assert_eq!(
            BUILTIN_LOOP_JOBS.len(),
            1,
            "catalog holds exactly one builtin"
        );

        let job = &BUILTIN_LOOP_JOBS[0];
        let expected = &jobs[0];
        let req = |key: &str| {
            expected
                .get(key)
                .and_then(Value::as_str)
                .unwrap_or_else(|| panic!("golden job lacks string key {key}"))
        };
        assert_eq!(job.slug, req("slug"));
        assert_eq!(job.name, req("name"));
        assert_eq!(job.public_name, req("public_name"));
        assert_eq!(job.public_description, req("public_description"));
        assert_eq!(job.rrule, req("rrule"));
        assert_eq!(job.tzid, req("tzid"));
        assert_eq!(
            job.min_role,
            expected
                .get("min_role")
                .and_then(Value::as_i64)
                .expect("min_role") as i32
        );
        // Byte-identical prompt: string equality on `str` is byte equality.
        assert_eq!(
            job.prompt, AUTO_CLOSE_MERGED_PROMPT,
            "catalog prompt is the shared constant"
        );
        assert_eq!(
            AUTO_CLOSE_MERGED_PROMPT,
            req("prompt"),
            "prompt verbatim vs golden"
        );
        assert_eq!(AUTO_CLOSE_MERGED_PROMPT.len(), req("prompt").len());
    }
}
