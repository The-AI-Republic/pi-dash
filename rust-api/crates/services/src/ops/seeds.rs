#![forbid(unsafe_code)]

//! Workspace seed data + loader (D-37 ops tail, stage 7).
//!
//! Port of `read_seed_file` (`apps/api/pi_dash/bgtasks/workspace_seed_task.py:49-68`)
//! with the `SEED_DIR` setting (`apps/api/pi_dash/settings/common.py:743`,
//! `SEED_DIR = os.path.join(BASE_DIR, "seeds")`) and the 8 seed data files
//! (`apps/api/pi_dash/seeds/data/*.json`). `workspace_seed_task.py` itself
//! belongs to its task domain — this module ports only the data + loader
//! semantics the seed task calls into (8 call sites, `:83-:486`, one per
//! catalog name).
//!
//! The mirrors under `seeds_data/` are byte copies of the Python files,
//! served embedded via [`include_str!`] so the loader never depends on the
//! Python tree. [`read_seed_file`] is the direct port: with `SEED_DIR` set
//! it reads `<SEED_DIR>/data/<filename>` from disk with exact Python
//! semantics; with `SEED_DIR` absent it serves the embedded catalog.
//!
//! Logging: Python logs through the `pi_dash.worker` logger mid-flow; this
//! crate carries no `tracing` dependency (see `app_issues/move.rs`), so the
//! port collects the line on the outcome ([`SeedRead::log_line`]) and the
//! task driver emits it via `tracing::error!` — the same contract as the
//! `orchestration` [`LogLine`][crate::orchestration::dispatch::LogLine]s.
//! Fixtures judge the values, not the mechanism.
//!
//! Fixture replayed by the suite below: F37-10
//! (`rust-api/fixtures/ops/seeds/loader.golden.json`); row counts and
//! required keys are asserted against that file, log lines byte-verbatim.
//!
//! Ported from `01a93e17216faea7bfc156b0f864cbbe420d1c52`.
//!
//! Ported quirks (translate, don't redesign):
//!
//! * Python's `json.load` accepts `NaN`/`Infinity` literals; a
//!   `serde_json::Value` cannot represent non-finite floats, so such a
//!   file takes the decode-error branch here. Unreachable for the 8
//!   catalog files (strict JSON, pinned byte-identical by
//!   `rust-api/contract-tests/ops/test_seeds.py`).

use std::path::{Path, PathBuf};

use serde_json::Value;

// ---------------------------------------------------------------------------
// Embedded catalog
// ---------------------------------------------------------------------------

const PROJECTS_JSON: &str = include_str!("seeds_data/projects.json");
const STATES_JSON: &str = include_str!("seeds_data/states.json");
const LABELS_JSON: &str = include_str!("seeds_data/labels.json");
const ISSUES_JSON: &str = include_str!("seeds_data/issues.json");
const CYCLES_JSON: &str = include_str!("seeds_data/cycles.json");
const MODULES_JSON: &str = include_str!("seeds_data/modules.json");
const PAGES_JSON: &str = include_str!("seeds_data/pages.json");
const VIEWS_JSON: &str = include_str!("seeds_data/views.json");

/// The 8 seed filenames, in fixture order.
pub const SEED_FILENAMES: [&str; 8] = [
    "projects.json",
    "states.json",
    "labels.json",
    "issues.json",
    "cycles.json",
    "modules.json",
    "pages.json",
    "views.json",
];

/// The embedded bytes for a catalog name, or `None` for unknown names.
pub fn embedded(filename: &str) -> Option<&'static str> {
    match filename {
        "projects.json" => Some(PROJECTS_JSON),
        "states.json" => Some(STATES_JSON),
        "labels.json" => Some(LABELS_JSON),
        "issues.json" => Some(ISSUES_JSON),
        "cycles.json" => Some(CYCLES_JSON),
        "modules.json" => Some(MODULES_JSON),
        "pages.json" => Some(PAGES_JSON),
        "views.json" => Some(VIEWS_JSON),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Outcome + log lines
// ---------------------------------------------------------------------------

/// Outcome of [`read_seed_file`]: the parsed JSON plus, on the two failure
/// branches, the exact `logger.error(...)` line Python emits (`:63-68`).
/// Emit [`SeedRead::log_line`] at ERROR level (`tracing::error!`,
/// mirroring `logger.error` on `pi_dash.worker`) when present.
#[derive(Debug, Clone, PartialEq)]
pub struct SeedRead {
    /// The parsed file (`json.load` result), or `None` on the two
    /// documented failure branches.
    pub value: Option<Value>,
    /// The exact log line for the failure branch, if any.
    pub log_line: Option<String>,
}

/// The `:63-65` not-found line, byte-verbatim:
/// `Seed file {filename} not found in {SEED_DIR}/data` — the `SEED_DIR`
/// value as given (no `/data` suffix in the var) plus the literal `/data`.
pub fn not_found_log_line(filename: &str, seed_dir: &str) -> String {
    format!("Seed file {filename} not found in {seed_dir}/data")
}

/// The `:66-68` decode-error line, byte-verbatim:
/// `Error decoding JSON from {filename}`.
pub fn decode_error_log_line(filename: &str) -> String {
    format!("Error decoding JSON from {filename}")
}

// ---------------------------------------------------------------------------
// SEED_DIR resolution
// ---------------------------------------------------------------------------

/// Resolve the seed dir (the `SEED_DIR` setting, `common.py:743`): the
/// `SEED_DIR` process variable when present (even when empty — Python's
/// `os.path.join("", "data", f)` reads `./data/f`, and `Path::join` matches),
/// else `None`, meaning "serve the embedded catalog".
pub fn seed_dir() -> Option<PathBuf> {
    seed_dir_with(&|key| std::env::var(key).ok())
}

/// [`seed_dir`] with an injectable environment reader (used by tests so
/// they never touch the process environment).
pub fn seed_dir_with(lookup: &dyn Fn(&str) -> Option<String>) -> Option<PathBuf> {
    lookup("SEED_DIR").map(PathBuf::from)
}

/// The `:59` join: `os.path.join(SEED_DIR, "data", filename)`.
/// `Path::join` matches `os.path.join`, including the absolute-`filename`
/// reset (an absolute `filename` discards the prefix in both).
pub fn seed_file_path(seed_dir: &Path, filename: &str) -> PathBuf {
    seed_dir.join("data").join(filename)
}

// ---------------------------------------------------------------------------
// Reads
// ---------------------------------------------------------------------------

/// Parse helper: `Some` text parses like `json.load`; embedded text is
/// valid by construction (pinned by the suite), so the error branch below
/// is defensive only.
fn parse_seed_text(filename: &str, text: &str) -> SeedRead {
    match serde_json::from_str::<Value>(text) {
        Ok(value) => SeedRead {
            value: Some(value),
            log_line: None,
        },
        Err(_) => SeedRead {
            value: None,
            log_line: Some(decode_error_log_line(filename)),
        },
    }
}

/// Serve `filename` from the embedded catalog. Unknown names miss silently:
/// Python has no embedded mode, so no template-exact line exists for this
/// branch (the logged not-found branch is the filesystem one). Unreachable
/// in practice — every caller passes one of the 8 catalog names.
pub fn read_embedded(filename: &str) -> SeedRead {
    match embedded(filename) {
        Some(text) => parse_seed_text(filename, text),
        None => SeedRead {
            value: None,
            log_line: None,
        },
    }
}

/// Read `filename` from `seed_dir` on disk, with exact Python semantics
/// (`:59-68`): `FileNotFoundError` → `None` + the not-found line;
/// `JSONDecodeError` → `None` + the decode line; anything else (permission,
/// `IsADirectory`, non-UTF8 bytes, …) propagates as `Err`, as Python's
/// uncaught exceptions propagate.
pub fn read_seed_file_in(seed_dir: &Path, filename: &str) -> Result<SeedRead, std::io::Error> {
    let path = seed_file_path(seed_dir, filename);
    match std::fs::read_to_string(&path) {
        Ok(text) => Ok(parse_seed_text(filename, &text)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(SeedRead {
            value: None,
            log_line: Some(not_found_log_line(filename, &seed_dir.to_string_lossy())),
        }),
        Err(e) => Err(e),
    }
}

/// Port of `read_seed_file` (`:49-68`): `SEED_DIR` set → disk read via
/// [`read_seed_file_in`]; `SEED_DIR` absent → [`read_embedded`].
pub fn read_seed_file(filename: &str) -> Result<SeedRead, std::io::Error> {
    match seed_dir() {
        Some(dir) => read_seed_file_in(&dir, filename),
        None => Ok(read_embedded(filename)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// F37-10 golden, same file the pytest suite asserts against: row
    /// counts and required keys come from here, never from literals.
    const LOADER_GOLDEN: &str = include_str!("../../../../fixtures/ops/seeds/loader.golden.json");

    fn golden() -> Value {
        serde_json::from_str(LOADER_GOLDEN).expect("fixture parses")
    }

    fn inventory_case(name: &str) -> (usize, Vec<String>) {
        let inv = &golden()["data_inventory"][name];
        let rows = inv["rows"].as_u64().expect("rows u64") as usize;
        let keys: Vec<String> = inv["required_keys"]
            .as_array()
            .expect("keys array")
            .iter()
            .map(|k| k.as_str().expect("key str").to_owned())
            .collect();
        (rows, keys)
    }

    /// Unique scratch dir per test (no `tempfile` dep in this crate).
    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "pidashconv-811-seeds-{}-{name}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("data")).expect("scratch data/");
        dir
    }

    fn teardown(dir: &Path) {
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn embedded_happy_path_matches_fixture_per_file() {
        // F37-10 data_inventory: rows + required keys per file.
        for filename in SEED_FILENAMES {
            let name = filename.strip_suffix(".json").expect("suffixed");
            let (rows, keys) = inventory_case(name);
            let read = read_embedded(filename);
            assert_eq!(read.log_line, None, "{filename} logs nothing");
            let value = read.value.expect("{filename} parses");
            let arr = value.as_array().expect("{filename} is a list");
            assert_eq!(arr.len(), rows, "{filename} row count");
            for (i, row) in arr.iter().enumerate() {
                let obj = row.as_object().expect("{filename} rows are objects");
                let mut got: Vec<String> = obj.keys().cloned().collect();
                got.sort();
                let mut want = keys.clone();
                want.sort();
                assert_eq!(got, want, "{filename}[{i}] keys");
            }
        }
    }

    #[test]
    fn not_found_log_line_verbatim() {
        // workspace_seed_task.py:64.
        assert_eq!(
            not_found_log_line("labels.json", "/srv/pi/seeds"),
            "Seed file labels.json not found in /srv/pi/seeds/data"
        );
    }

    #[test]
    fn decode_error_log_line_verbatim() {
        // workspace_seed_task.py:67.
        assert_eq!(
            decode_error_log_line("issues.json"),
            "Error decoding JSON from issues.json"
        );
    }

    #[test]
    fn missing_file_returns_none_with_exact_line() {
        let dir = scratch("missing");
        let read = read_seed_file_in(&dir, "nope.json").expect("None, not Err");
        assert_eq!(read.value, None);
        assert_eq!(
            read.log_line.as_deref(),
            Some(not_found_log_line("nope.json", &dir.to_string_lossy()).as_str())
        );
        // The template itself, byte-verbatim (no helpers in the path).
        assert_eq!(
            read.log_line.unwrap(),
            format!("Seed file nope.json not found in {}/data", dir.display())
        );
        teardown(&dir);
    }

    #[test]
    fn corrupt_json_returns_none_with_exact_line() {
        let dir = scratch("corrupt");
        std::fs::write(dir.join("data").join("bad.json"), "{nope").expect("write");
        let read = read_seed_file_in(&dir, "bad.json").expect("None, not Err");
        assert_eq!(read.value, None);
        assert_eq!(
            read.log_line.as_deref(),
            Some("Error decoding JSON from bad.json")
        );
        teardown(&dir);
    }

    #[test]
    fn seed_dir_override_reads_from_disk() {
        // The override path end to end: SEED_DIR set → disk wins over
        // embedded, custom content served. This is the only test that
        // touches the process environment (save/set/restore); every other
        // test uses read_seed_file_in / read_embedded / seed_dir_with.
        let dir = scratch("override");
        std::fs::write(
            dir.join("data").join("custom.json"),
            r#"[{"overridden": true}]"#,
        )
        .expect("write");
        let saved = std::env::var("SEED_DIR").ok();
        std::env::set_var("SEED_DIR", &dir);
        let read = read_seed_file("custom.json").expect("override reads");
        match saved {
            Some(s) => std::env::set_var("SEED_DIR", s),
            None => std::env::remove_var("SEED_DIR"),
        }
        assert_eq!(read.log_line, None);
        assert_eq!(read.value, Some(serde_json::json!([{"overridden": true}])));
        teardown(&dir);
    }

    #[test]
    fn seed_dir_resolution() {
        assert_eq!(seed_dir_with(&|_| None), None);
        assert_eq!(
            seed_dir_with(&|_| Some("/srv/pi/seeds".to_owned())),
            Some(PathBuf::from("/srv/pi/seeds"))
        );
        // Present-but-empty stays a filesystem read (Python's
        // os.path.join("", "data", f) == "data/f", relative to CWD).
        assert_eq!(
            seed_dir_with(&|_| Some(String::new())),
            Some(PathBuf::from(""))
        );
        assert_eq!(
            seed_file_path(Path::new(""), "x.json"),
            PathBuf::from("data/x.json")
        );
    }

    #[test]
    fn absolute_filename_resets_join_like_python() {
        // os.path.join discards the prefix for absolute tails; Path::join
        // matches (:59).
        assert_eq!(
            seed_file_path(Path::new("/seed"), "/abs/x.json"),
            PathBuf::from("/abs/x.json")
        );
        let dir = scratch("absfile");
        let abs = dir.join("abs.json");
        std::fs::write(&abs, "[1]").expect("write");
        let read = read_seed_file_in(Path::new("/does/not/exist"), abs.to_str().expect("utf8"))
            .expect("absolute tail wins");
        assert_eq!(read.log_line, None);
        assert_eq!(read.value, Some(serde_json::json!([1])));
        teardown(&dir);
    }

    #[test]
    fn unexpected_io_errors_propagate() {
        // Only NotFound + JSONDecodeError become None (:63-68); anything
        // else propagates (fixture: other_exceptions).
        let dir = scratch("propagate");
        // Non-UTF8 bytes: Python raises UnicodeDecodeError (uncaught).
        std::fs::write(dir.join("data").join("bin.json"), [0xff, 0xfe, b'{']).expect("write");
        assert!(read_seed_file_in(&dir, "bin.json").is_err());
        // A directory at the leaf: Python raises IsADirectoryError.
        std::fs::create_dir(dir.join("data").join("dir.json")).expect("mkdir");
        assert!(read_seed_file_in(&dir, "dir.json").is_err());
        teardown(&dir);
        // A file where data/ should be: Python raises NotADirectoryError.
        let dir2 = scratch("notadir");
        std::fs::remove_dir_all(dir2.join("data")).expect("rmdir");
        std::fs::write(dir2.join("data"), b"x").expect("write");
        assert!(read_seed_file_in(&dir2, "x.json").is_err());
        teardown(&dir2);
    }

    #[test]
    fn unknown_embedded_name_is_silent_none() {
        // No template-exact line exists for the embedded miss (Python has
        // no embedded mode); None with no line. Unreachable in practice:
        // all 8 call sites pass catalog names.
        let read = read_embedded("nope.json");
        assert_eq!(read.value, None);
        assert_eq!(read.log_line, None);
        assert_eq!(embedded("nope.json"), None);
    }
}
