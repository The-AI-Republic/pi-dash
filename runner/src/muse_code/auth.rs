//! Muse Code credential presence check.
//!
//! A runner can start `muse exec` only if Muse credentials are present in the
//! environment the daemon spawns `muse` with. Credentials count as present
//! when either `META_API_KEY` is set (non-empty, after trimming) or the Muse
//! auth file exists and is non-empty at `~/.config/muse/auth.json` under the
//! same home the daemon uses. Only existence is checked — the key is never
//! validated against the Muse API.
//!
//! The check is used in three places: `pidash doctor` (`muse-auth`), the
//! pre-`Accept` gate for `ServerMsg::Assign` on `AgentKind::MuseCode` runners,
//! and daemon startup (for early operator visibility).

use std::path::{Path, PathBuf};

/// The env var Muse reads for API credentials.
pub const META_API_KEY_ENV: &str = "META_API_KEY";

/// Path of the Muse auth file under a home dir: `~/.config/muse/auth.json`.
pub fn auth_file_path(home: &Path) -> PathBuf {
    home.join(".config").join("muse").join("auth.json")
}

/// Resolve the home directory the daemon uses for `~`.
/// Prefers `directories::BaseDirs::home_dir()` (the XDG/.pidash convention the
/// daemon already uses) and falls back to `$HOME`.
pub fn home_dir() -> Option<PathBuf> {
    if let Some(bases) = directories::BaseDirs::new() {
        return Some(bases.home_dir().to_path_buf());
    }
    std::env::var("HOME").ok().map(PathBuf::from)
}

/// Pure presence check for tests: `meta_api_key` is the value of
/// `META_API_KEY` (`None` when unset), `home` is the home dir to look for the
/// auth file under. Present if either source is present.
pub fn credentials_present(meta_api_key: Option<&str>, home: &Path) -> bool {
    if meta_api_key.is_some_and(|v| !v.trim().is_empty()) {
        return true;
    }
    file_present(home)
}

/// True when `~/.config/muse/auth.json` under `home` exists and is a
/// non-empty regular file.
fn file_present(home: &Path) -> bool {
    let path = auth_file_path(home);
    match std::fs::metadata(&path) {
        Ok(meta) => meta.is_file() && meta.len() > 0,
        Err(_) => false,
    }
}

/// Production presence check: reads `META_API_KEY` from the daemon
/// environment and the auth file under [`home_dir`].
pub fn has_muse_credentials() -> bool {
    let meta_api_key = std::env::var(META_API_KEY_ENV).ok();
    if meta_api_key
        .as_deref()
        .is_some_and(|v| !v.trim().is_empty())
    {
        return true;
    }
    if let Some(home) = home_dir() {
        return file_present(&home);
    }
    false
}

/// Fix hint shared by `pidash doctor`, the pre-pickup warning, and startup.
pub const FIX_HINT: &str = "run `muse login` or set META_API_KEY for the daemon's environment";

#[cfg(test)]
mod tests {
    use super::*;

    fn empty_home(dir: &tempfile::TempDir) -> PathBuf {
        dir.path().to_path_buf()
    }

    fn write_auth_file(home: &Path, contents: &str) {
        let path = auth_file_path(home);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, contents).unwrap();
    }

    #[test]
    fn env_var_set_counts_as_present() {
        let dir = tempfile::tempdir().unwrap();
        let home = empty_home(&dir);
        assert!(credentials_present(Some("sk-test"), &home));
    }

    #[test]
    fn auth_file_present_non_empty_counts_as_present() {
        let dir = tempfile::tempdir().unwrap();
        let home = empty_home(&dir);
        write_auth_file(&home, "{\"access_token\":\"x\"}");
        assert!(credentials_present(None, &home));
    }

    #[test]
    fn auth_file_empty_does_not_count() {
        let dir = tempfile::tempdir().unwrap();
        let home = empty_home(&dir);
        write_auth_file(&home, "");
        assert!(!credentials_present(None, &home));
    }

    #[test]
    fn neither_env_nor_file_is_missing() {
        let dir = tempfile::tempdir().unwrap();
        let home = empty_home(&dir);
        assert!(!credentials_present(None, &home));
        assert!(!credentials_present(Some(""), &home));
        assert!(!credentials_present(Some("   "), &home));
    }

    #[test]
    fn env_wins_over_empty_file() {
        let dir = tempfile::tempdir().unwrap();
        let home = empty_home(&dir);
        write_auth_file(&home, "");
        assert!(credentials_present(Some("sk-x"), &home));
    }

    #[test]
    fn auth_file_missing_dir_is_missing() {
        let dir = tempfile::tempdir().unwrap();
        let home = empty_home(&dir);
        // No .config/muse directory created at all.
        assert!(!credentials_present(None, &home));
    }

    #[test]
    fn auth_file_path_is_under_home_config_muse() {
        let home = Path::new("/home/tester");
        assert_eq!(
            auth_file_path(home),
            PathBuf::from("/home/tester/.config/muse/auth.json")
        );
    }
}
