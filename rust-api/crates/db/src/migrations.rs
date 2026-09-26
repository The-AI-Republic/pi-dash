#![forbid(unsafe_code)]

//! Private migration directory convention (F-10).
//!
//! Django stays schema owner for its own tables until switchover, and the
//! OSS Rust backend owns only its `rust_*` runtime tables (created at
//! worker boot by `pidash-jobs`, never through a Django migration). The
//! private crate's tables follow the same rule one directory over:
//!
//! - Private SQL migrations live in the private crate at
//!   [`PRIVATE_MIGRATIONS_DIR`] (`rust-api/private/migrations/`), one
//!   `NNN_name.sql` file per migration, applied in lexical order after the
//!   OSS baseline (`ensure_schema`).
//! - Private tables are prefixed [`PRIVATE_TABLE_PREFIX`] (`private_`).
//!   A private migration never creates, alters or drops a table outside
//!   that prefix — Django-owned tables and OSS `rust_*` tables are out of
//!   bounds, exactly as Django's own `pi_dash_cloud/*/migrations/`
//!   directories never touch OSS tables.
//! - The directory does not exist in this repository (the private crate is
//!   a separate build); [`private_migration_files`] is the shared
//!   discovery both builds use, so ordering and naming stay identical.
//!
//! [`private_migration_files`] lists candidate files; running them is the
//! binary's job at boot, inside its own transaction discipline.

use std::path::{Path, PathBuf};

/// Private migration directory, relative to the repository root.
pub const PRIVATE_MIGRATIONS_DIR: &str = "rust-api/private/migrations";

/// Required table prefix for private migrations.
pub const PRIVATE_TABLE_PREFIX: &str = "private_";

/// Required filename shape: `NNN_name.sql`, so lexical order is version
/// order (the same property Django's zero-padded migration numbers give).
fn is_migration_file(name: &str) -> bool {
    let stem = name.strip_suffix(".sql").unwrap_or("");
    let (digits, rest) = stem.split_at(stem.find('_').unwrap_or(stem.len()));
    !digits.is_empty()
        && digits.len() <= 4
        && digits.bytes().all(|byte| byte.is_ascii_digit())
        && rest.starts_with('_')
        && rest.len() > 1
}

/// List the private migration files in `dir` in application (lexical)
/// order. Missing directories yield no files: the OSS build has no
/// private directory, and that is the normal case, not an error.
pub fn private_migration_files(dir: &Path) -> Vec<PathBuf> {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(_) => return Vec::new(),
    };
    let mut files: Vec<PathBuf> = entries
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| {
            path.is_file()
                && path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(is_migration_file)
        })
        .collect();
    files.sort();
    files
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("pidash-migrations-{name}"));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("temp dir");
        dir
    }

    #[test]
    fn discovery_is_lexical_and_ignores_non_migrations() {
        let dir = temp_dir("order");
        for name in [
            "0002_second.sql",
            "0001_first.sql",
            "notes.txt",
            "0003.sql",
            "draft.sql",
        ] {
            fs::write(dir.join(name), "-- x").expect("seed");
        }
        let files = private_migration_files(&dir);
        let names: Vec<String> = files
            .iter()
            .map(|path| {
                path.file_name()
                    .expect("name")
                    .to_string_lossy()
                    .into_owned()
            })
            .collect();
        assert_eq!(names, vec!["0001_first.sql", "0002_second.sql"]);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_directory_is_no_migrations() {
        assert!(private_migration_files(Path::new("/nonexistent-pidash-private")).is_empty());
    }

    #[test]
    fn convention_constants_match_the_documented_layout() {
        assert_eq!(PRIVATE_MIGRATIONS_DIR, "rust-api/private/migrations");
        assert_eq!(PRIVATE_TABLE_PREFIX, "private_");
    }
}
