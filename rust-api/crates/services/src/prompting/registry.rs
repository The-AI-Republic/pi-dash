//! Prompt section registry: the static catalog + its reader.
//!
//! Port of `apps/api/pi_dash/prompting/registry.py` (178 lines):
//!
//! * `registry.py:26` (`SECTIONS_DIR`) — here the catalog is baked in at
//!   compile time: byte-identical mirrors of `prompting/sections/*.md` live
//!   under `src/prompting/sections/` and are embedded with `include_str!`,
//!   so the server has no runtime path dependency. A contract test asserts
//!   every mirror is byte-identical to its Python source.
//! * `registry.py:28-56` (tier constants + `tier_allows_workspace_override` /
//!   `tier_allows_personal_override`).
//! * `registry.py:62-66` (`MAX_SECTION_BODY_LENGTH`).
//! * `registry.py:102-147` (`_parse_front_matter`) —
//!   [`parse_front_matter`]; error strings are byte-identical to Python.
//! * `registry.py:150-161` (`_load_registry`), `166` (`REGISTRY`),
//!   `169-178` (`get_section`, `all_sections`).
//!
//! Fail-loud like Python's import-time `REGISTRY = _load_registry()`: a
//! malformed mirror panics on first registry access with the same message
//! Python would raise at import.
//!
//! No ported bugs: the `bugs` arrays of `FIX-registry` are empty.

use std::collections::BTreeMap;
use std::sync::LazyLock;

/// Governance tier: nobody edits the body; the registry default always wins.
pub const CUSTOMIZABLE_LOCKED: &str = "locked";
/// Governance tier: a workspace admin may set a workspace-level override.
pub const CUSTOMIZABLE_WORKSPACE: &str = "workspace";
/// Governance tier: fully open — personal overrides allowed.
pub const CUSTOMIZABLE_OVERRIDABLE: &str = "overridable";

/// Upper bound on an override body, mirroring the legacy template cap
/// (`registry.py:66`). Enforced at the API boundary (serializer).
pub const MAX_SECTION_BODY_LENGTH: usize = 100_000;

/// Raised when the on-disk section registry is malformed
/// (`registry.py:69-70`).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{message}")]
pub struct PromptRegistryError {
    message: String,
}

impl PromptRegistryError {
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

/// Whether a workspace admin may set a workspace-level override at `tier`
/// (`registry.py:44-51`).
pub fn tier_allows_workspace_override(tier: &str) -> bool {
    tier == CUSTOMIZABLE_WORKSPACE || tier == CUSTOMIZABLE_OVERRIDABLE
}

/// Whether a member may keep a personal (user-scope) override at `tier`
/// (`registry.py:54-56`).
pub fn tier_allows_personal_override(tier: &str) -> bool {
    tier == CUSTOMIZABLE_OVERRIDABLE
}

/// One registry section: identity, metadata, and default body
/// (`registry.py:73-99`). Identity and metadata borrow the section file;
/// the default body is owned because parsing normalizes trailing newlines
/// (Python builds a new `str` too).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromptSection<'a> {
    pub key: &'a str,
    pub title: &'a str,
    pub customizable: &'a str,
    pub default_body: String,
}

impl<'a> PromptSection<'a> {
    /// True only for the `locked` tier (`registry.py:82-84`).
    pub fn is_locked(&self) -> bool {
        self.customizable == CUSTOMIZABLE_LOCKED
    }

    /// True only for the fully-open tier (`registry.py:86-89`).
    pub fn is_overridable(&self) -> bool {
        self.customizable == CUSTOMIZABLE_OVERRIDABLE
    }

    /// Whether a workspace admin may set a workspace-level override
    /// (`registry.py:91-94`).
    pub fn allows_workspace_override(&self) -> bool {
        tier_allows_workspace_override(self.customizable)
    }

    /// Whether a member may keep a personal override (`registry.py:96-99`).
    pub fn allows_personal_override(&self) -> bool {
        tier_allows_personal_override(self.customizable)
    }
}

/// Parse a `<key>.md` section file with a leading `---` front-matter block
/// (`registry.py:102-147`). Deliberately the same tiny hand-rolled parser
/// (key: value lines, no YAML dependency). `file_name` is the base name
/// (e.g. `intro.md`); `text` is the full file content.
pub fn parse_front_matter<'a>(
    file_name: &str,
    text: &'a str,
) -> Result<PromptSection<'a>, PromptRegistryError> {
    if !text.starts_with("---") {
        return Err(PromptRegistryError::new(format!(
            "section '{file_name}' is missing the leading '---' front-matter block"
        )));
    }
    // Split off the front-matter between the first two '---' fences
    // (`str.split("---", 2)`).
    let mut parts = text.splitn(3, "---");
    let (_head, raw_meta, body) = match (parts.next(), parts.next(), parts.next()) {
        (Some(head), Some(raw_meta), Some(body)) if head.is_empty() => (head, raw_meta, body),
        _ => {
            return Err(PromptRegistryError::new(format!(
                "section '{file_name}' has a malformed front-matter block \
                 (expected '---\\n<meta>\\n---\\n<body>')"
            )));
        }
    };
    let mut meta: BTreeMap<&str, &str> = BTreeMap::new();
    for line in raw_meta.trim().lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((field, value)) = line.split_once(':') else {
            return Err(PromptRegistryError::new(format!(
                "section '{file_name}' has an invalid front-matter line: '{line}'"
            )));
        };
        meta.insert(field.trim(), value.trim());
    }

    let mut missing: Vec<&str> = ["key", "title", "customizable"]
        .into_iter()
        .filter(|field| !meta.contains_key(field))
        .collect();
    missing.sort_unstable();
    if !missing.is_empty() {
        return Err(PromptRegistryError::new(format!(
            "section '{file_name}' is missing front-matter field(s): {}",
            missing.join(", ")
        )));
    }
    let customizable = meta["customizable"];
    if !matches!(
        customizable,
        CUSTOMIZABLE_LOCKED | CUSTOMIZABLE_WORKSPACE | CUSTOMIZABLE_OVERRIDABLE
    ) {
        return Err(PromptRegistryError::new(format!(
            "section '{file_name}' has invalid customizable='{customizable}' \
             (expected one of ['locked', 'overridable', 'workspace'])"
        )));
    }
    let key = meta["key"];
    let stem = file_name.strip_suffix(".md").unwrap_or(file_name);
    if stem != key {
        return Err(PromptRegistryError::new(format!(
            "section file '{file_name}' does not match its front-matter key='{key}' \
             (filename stem must equal the key)"
        )));
    }
    // Drop a single leading newline left by the closing '---\n', keep the
    // rest verbatim. Trailing whitespace is normalized to a single newline
    // (`body.lstrip("\n").rstrip("\n") + "\n"`).
    let body = body.trim_start_matches('\n').trim_end_matches('\n');
    Ok(PromptSection {
        key,
        title: meta["title"],
        customizable,
        default_body: format!("{body}\n"),
    })
}

/// Embedded catalog: `(file_name, file_bytes)` for every `sections/*.md`,
/// in filename order (the load order of `_load_registry`).
const SECTION_FILES: &[(&str, &str)] = &[
    (
        "analyze-and-scope.md",
        include_str!("sections/analyze-and-scope.md"),
    ),
    ("autonomy.md", include_str!("sections/autonomy.md")),
    ("blocking.md", include_str!("sections/blocking.md")),
    (
        "cloud-capabilities.md",
        include_str!("sections/cloud-capabilities.md"),
    ),
    (
        "cloud-direct-task.md",
        include_str!("sections/cloud-direct-task.md"),
    ),
    ("cloud-ending.md", include_str!("sections/cloud-ending.md")),
    (
        "cloud-execution-loop.md",
        include_str!("sections/cloud-execution-loop.md"),
    ),
    ("cloud-intro.md", include_str!("sections/cloud-intro.md")),
    (
        "cloud-issue-context.md",
        include_str!("sections/cloud-issue-context.md"),
    ),
    (
        "cloud-review-intro.md",
        include_str!("sections/cloud-review-intro.md"),
    ),
    (
        "cloud-review-loop.md",
        include_str!("sections/cloud-review-loop.md"),
    ),
    (
        "cloud-scheduler-intro.md",
        include_str!("sections/cloud-scheduler-intro.md"),
    ),
    (
        "cloud-scheduler-loop.md",
        include_str!("sections/cloud-scheduler-loop.md"),
    ),
    (
        "cloud-scheduler-task.md",
        include_str!("sections/cloud-scheduler-task.md"),
    ),
    (
        "cloud-test-intro.md",
        include_str!("sections/cloud-test-intro.md"),
    ),
    (
        "cloud-test-loop.md",
        include_str!("sections/cloud-test-loop.md"),
    ),
    (
        "cloud-write-policy.md",
        include_str!("sections/cloud-write-policy.md"),
    ),
    (
        "default-posture.md",
        include_str!("sections/default-posture.md"),
    ),
    ("ending-run.md", include_str!("sections/ending-run.md")),
    ("guardrails.md", include_str!("sections/guardrails.md")),
    (
        "implementation.md",
        include_str!("sections/implementation.md"),
    ),
    ("intro.md", include_str!("sections/intro.md")),
    ("pidash-cli.md", include_str!("sections/pidash-cli.md")),
    (
        "relationships.md",
        include_str!("sections/relationships.md"),
    ),
    ("repo-context.md", include_str!("sections/repo-context.md")),
    ("review-cycle.md", include_str!("sections/review-cycle.md")),
    ("review-intro.md", include_str!("sections/review-intro.md")),
    (
        "scheduler-ending.md",
        include_str!("sections/scheduler-ending.md"),
    ),
    (
        "scheduler-intro.md",
        include_str!("sections/scheduler-intro.md"),
    ),
    (
        "scheduler-task.md",
        include_str!("sections/scheduler-task.md"),
    ),
    (
        "session-framing.md",
        include_str!("sections/session-framing.md"),
    ),
    (
        "state-routing.md",
        include_str!("sections/state-routing.md"),
    ),
    (
        "task-lifecycle.md",
        include_str!("sections/task-lifecycle.md"),
    ),
    ("test-cycle.md", include_str!("sections/test-cycle.md")),
    ("test-intro.md", include_str!("sections/test-intro.md")),
    (
        "workpad-context.md",
        include_str!("sections/workpad-context.md"),
    ),
    (
        "workpad-setup.md",
        include_str!("sections/workpad-setup.md"),
    ),
    (
        "workpad-template.md",
        include_str!("sections/workpad-template.md"),
    ),
];

/// The loaded registry, parsed once (`registry.py:166`). `BTreeMap` keeps
/// key-sorted order for [`all_sections`].
static REGISTRY: LazyLock<BTreeMap<&'static str, PromptSection<'static>>> = LazyLock::new(|| {
    let mut registry: BTreeMap<&'static str, PromptSection<'static>> = BTreeMap::new();
    for (file_name, text) in SECTION_FILES {
        let section = parse_front_matter(file_name, text).unwrap_or_else(|err| {
            panic!("prompt registry is malformed: {}", err.message());
        });
        if registry.contains_key(section.key) {
            panic!(
                "prompt registry is malformed: duplicate section key: '{}'",
                section.key
            );
        }
        registry.insert(section.key, section);
    }
    if registry.is_empty() {
        panic!("prompt registry is malformed: no sections found");
    }
    registry
});

/// Fetch one section by key (`registry.py:169-173`).
pub fn get_section(key: &str) -> Result<&'static PromptSection<'static>, PromptRegistryError> {
    REGISTRY
        .get(key)
        .ok_or_else(|| PromptRegistryError::new(format!("unknown section key: '{key}'")))
}

/// Registry sections in stable (key-sorted) order (`registry.py:176-178`).
pub fn all_sections() -> Vec<&'static PromptSection<'static>> {
    REGISTRY.values().collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    fn fixture() -> Value {
        let path = format!(
            "{}/../../fixtures/prompting/FIX-registry.json",
            env!("CARGO_MANIFEST_DIR")
        );
        serde_json::from_str(&std::fs::read_to_string(&path).expect("golden exists"))
            .expect("golden parses")
    }

    fn registry_data(fixture: &Value) -> &Value {
        fixture
            .get("data")
            .and_then(|data| data.get("registry"))
            .expect("fixture carries data.registry")
    }

    #[test]
    fn tier_constants_match_python() {
        let golden = fixture();
        let tiers = &registry_data(&golden)["tier_constants"];
        assert_eq!(tiers["locked"].as_str(), Some(CUSTOMIZABLE_LOCKED));
        assert_eq!(tiers["workspace"].as_str(), Some(CUSTOMIZABLE_WORKSPACE));
        assert_eq!(
            tiers["overridable"].as_str(),
            Some(CUSTOMIZABLE_OVERRIDABLE)
        );
    }

    #[test]
    fn tier_truth_table() {
        let golden = fixture();
        let table = registry_data(&golden)["tier_truth_table"]
            .as_array()
            .expect("tier_truth_table is an array");
        assert_eq!(table.len(), 3);
        for row in table {
            let tier = row["tier"].as_str().expect("tier is a string");
            assert_eq!(
                tier_allows_workspace_override(tier),
                row["workspace_override"].as_bool().expect("bool"),
                "workspace gate for tier {tier}"
            );
            assert_eq!(
                tier_allows_personal_override(tier),
                row["personal_override"].as_bool().expect("bool"),
                "personal gate for tier {tier}"
            );
        }
        // Unknown tiers allow nothing (Python: `in` membership test).
        assert!(!tier_allows_workspace_override("nope"));
        assert!(!tier_allows_personal_override("nope"));
    }

    #[test]
    fn max_section_body_length() {
        let golden = fixture();
        assert_eq!(
            MAX_SECTION_BODY_LENGTH,
            registry_data(&golden)["max_section_body_length"]
                .as_u64()
                .expect("u64") as usize
        );
    }

    #[test]
    fn section_count_and_sorted_order() {
        let golden = fixture();
        let data = registry_data(&golden);
        let sections = all_sections();
        assert_eq!(
            sections.len(),
            data["section_count"].as_u64().expect("u64") as usize
        );
        assert!(data["all_sections_sorted"].as_bool().expect("bool"));
        let keys: Vec<&str> = sections.iter().map(|section| section.key).collect();
        let mut sorted = keys.clone();
        sorted.sort_unstable();
        assert_eq!(keys, sorted, "all_sections is key-sorted");
    }

    #[test]
    fn every_section_matches_fixture() {
        let golden = fixture();
        let data = registry_data(&golden);
        for entry in data["sections"].as_array().expect("sections is an array") {
            let key = entry["key"].as_str().expect("key is a string");
            let section = get_section(key)
                .unwrap_or_else(|_| panic!("registry carries fixture section {key}"));
            assert_eq!(section.title, entry["title"].as_str().expect("title"));
            assert_eq!(
                section.customizable,
                entry["customizable"].as_str().expect("customizable")
            );
            assert_eq!(
                section.is_locked(),
                entry["is_locked"].as_bool().expect("bool"),
                "is_locked for {key}"
            );
            assert_eq!(
                section.is_overridable(),
                entry["is_overridable"].as_bool().expect("bool"),
                "is_overridable for {key}"
            );
            assert_eq!(
                section.allows_workspace_override(),
                entry["allows_workspace_override"].as_bool().expect("bool"),
                "workspace gate for {key}"
            );
            assert_eq!(
                section.allows_personal_override(),
                entry["allows_personal_override"].as_bool().expect("bool"),
                "personal gate for {key}"
            );
            // Python `len()` counts code points, not bytes (the bodies
            // carry multi-byte em-dashes), so compare `chars().count()`.
            assert_eq!(
                section.default_body.chars().count(),
                entry["body_len"].as_u64().expect("u64") as usize,
                "body_len for {key}"
            );
        }
    }

    #[test]
    fn unknown_section_key_errors() {
        let err = get_section("no-such-section").expect_err("unknown key raises");
        assert_eq!(format!("{err}"), "unknown section key: 'no-such-section'");
        let golden = fixture();
        let expected = registry_data(&golden)["get_section_unknown"]
            .as_str()
            .expect("string");
        assert!(
            expected.ends_with(err.message()),
            "fixture message matches: {expected}"
        );
    }

    #[test]
    fn front_matter_errors_match_python() {
        // Missing leading '---'.
        let err = parse_front_matter("intro.md", "no fence here\n").expect_err("raises");
        assert_eq!(
            err.message(),
            "section 'intro.md' is missing the leading '---' front-matter block"
        );
        // Only one fence.
        let err = parse_front_matter("intro.md", "---\nkey: intro\n").expect_err("raises");
        assert_eq!(
            err.message(),
            "section 'intro.md' has a malformed front-matter block \
             (expected '---\\n<meta>\\n---\\n<body>')"
        );
        // Line without a colon.
        let err =
            parse_front_matter("intro.md", "---\nbogus line\n---\nbody\n").expect_err("raises");
        assert_eq!(
            err.message(),
            "section 'intro.md' has an invalid front-matter line: 'bogus line'"
        );
        // Missing fields, sorted in the message.
        let err =
            parse_front_matter("intro.md", "---\nkey: intro\n---\nbody\n").expect_err("raises");
        assert_eq!(
            err.message(),
            "section 'intro.md' is missing front-matter field(s): customizable, title"
        );
        // Invalid tier.
        let err = parse_front_matter(
            "intro.md",
            "---\nkey: intro\ntitle: T\ncustomizable: wild\n---\nbody\n",
        )
        .expect_err("raises");
        assert_eq!(
            err.message(),
            "section 'intro.md' has invalid customizable='wild' \
             (expected one of ['locked', 'overridable', 'workspace'])"
        );
        // Filename stem must equal the key.
        let err = parse_front_matter(
            "other.md",
            "---\nkey: intro\ntitle: T\ncustomizable: locked\n---\nbody\n",
        )
        .expect_err("raises");
        assert_eq!(
            err.message(),
            "section file 'other.md' does not match its front-matter key='intro' \
             (filename stem must equal the key)"
        );
    }

    #[test]
    fn body_normalization_matches_python() {
        let section = parse_front_matter(
            "intro.md",
            "---\nkey: intro\ntitle: T\ncustomizable: locked\n---\n\n\nbody line  \n\n\n",
        )
        .expect("parses");
        // `lstrip("\n").rstrip("\n") + "\n"`: leading blank lines dropped,
        // trailing newlines collapsed to one, inner trailing spaces kept.
        assert_eq!(section.default_body, "body line  \n");
    }
}
