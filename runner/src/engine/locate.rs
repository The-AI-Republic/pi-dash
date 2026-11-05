//! Find the built-in engine for this `pidash`.
//!
//! The daemon resolves the engine once, at start ([`init_at_daemon_start`]),
//! and keeps that path for its whole life ([`resolved`]). `pidash update`
//! installs a new engine version directory beside the old one and then swaps
//! `pidash`, so a running daemon is never handed a different binary
//! mid-run; the new engine is picked up from the next daemon start.
//!
//! Resolution order:
//!
//! 1. `[engine] binary` in `config.toml` (development override). If set, it
//!    must name an existing file; nothing else is tried.
//! 2. The installed layouts, relative to the running executable (and to its
//!    canonical path, so a symlinked `pidash` still finds its install):
//!    - Unix: `<exe dir>/../lib/pidash/engine/<version>/` (shell installer),
//!      then `<exe dir>/engine/<version>/` (an extracted release archive).
//!    - Windows: `<exe dir>\engine\<version>\` (PowerShell installer, MSI and
//!      the archive alike).
//!
//! `<version>` is always the engine this `pidash` was released with
//! ([`super::engine_version`]): the CLI and engine are signed and tested
//! together, so a daemon never picks up another release's engine.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use crate::config::schema::EngineSection;

/// Which install layout an engine was found in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Layout {
    /// `<exe dir>/../lib/pidash/engine/` — the cargo-dist shell installer.
    UnixLib,
    /// `<exe dir>/engine/` — the PowerShell installer, the MSI, and an
    /// extracted release archive.
    Sibling,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EngineSource {
    /// `[engine] binary` from the config file.
    ConfigOverride,
    /// Found in an installed layout next to the executable.
    Installed(Layout),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedEngine {
    /// Absolute path to the engine executable.
    pub binary: PathBuf,
    /// Engine version directory the binary lives in. `None` for the config
    /// override, whose version is whatever the developer pointed at.
    pub version: Option<String>,
    pub source: EngineSource,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum LocateError {
    #[error("[engine] binary = {0:?} does not exist or is not a file")]
    OverrideMissing(PathBuf),
    #[error(
        "built-in engine {version} not found; looked in: {}. Run `pidash engine install` to install it",
        display_paths(.searched)
    )]
    NotInstalled {
        version: String,
        searched: Vec<PathBuf>,
    },
    #[error("cannot determine the pidash executable path: {0}")]
    CurrentExe(String),
}

fn display_paths(paths: &[PathBuf]) -> String {
    paths
        .iter()
        .map(|p| p.display().to_string())
        .collect::<Vec<_>>()
        .join(", ")
}

/// Engine roots (the directories that hold `<version>/` subdirectories) for
/// an executable living in `exe_dir`, most specific first.
pub fn engine_roots(exe_dir: &Path, windows: bool) -> Vec<(Layout, PathBuf)> {
    let sibling = (Layout::Sibling, exe_dir.join("engine"));
    if windows {
        return vec![sibling];
    }
    let mut roots = Vec::with_capacity(2);
    if let Some(prefix) = exe_dir.parent() {
        roots.push((
            Layout::UnixLib,
            prefix.join("lib").join("pidash").join("engine"),
        ));
    }
    roots.push(sibling);
    roots
}

/// The root a fresh install writes to for an executable in `exe_dir`: the
/// first layout in [`engine_roots`].
pub fn install_root(exe_dir: &Path, windows: bool) -> PathBuf {
    engine_roots(exe_dir, windows)
        .into_iter()
        .next()
        .map(|(_, root)| root)
        .expect("engine_roots always yields at least the sibling layout")
}

/// Pure resolution over explicit inputs, so every layout is testable on
/// every host. `exe_paths` are the executable's path as invoked and its
/// canonical path (duplicates are fine).
pub fn resolve_from(
    override_binary: Option<&Path>,
    exe_paths: &[PathBuf],
    version: &str,
    windows: bool,
) -> Result<ResolvedEngine, LocateError> {
    if let Some(binary) = override_binary {
        if binary.is_file() {
            return Ok(ResolvedEngine {
                binary: binary.to_path_buf(),
                version: None,
                source: EngineSource::ConfigOverride,
            });
        }
        return Err(LocateError::OverrideMissing(binary.to_path_buf()));
    }

    let name = super::binary_name(windows);
    let mut searched: Vec<PathBuf> = Vec::new();
    for exe in exe_paths {
        let Some(exe_dir) = exe.parent() else {
            continue;
        };
        for (layout, root) in engine_roots(exe_dir, windows) {
            let dir = root.join(version);
            if searched.contains(&dir) {
                continue;
            }
            let binary = dir.join(name);
            if binary.is_file() {
                return Ok(ResolvedEngine {
                    binary,
                    version: Some(version.to_string()),
                    source: EngineSource::Installed(layout),
                });
            }
            searched.push(dir);
        }
    }
    Err(LocateError::NotInstalled {
        version: version.to_string(),
        searched,
    })
}

/// The running executable as invoked plus its canonical path.
pub fn current_exe_paths() -> Result<Vec<PathBuf>, LocateError> {
    let exe = std::env::current_exe().map_err(|e| LocateError::CurrentExe(e.to_string()))?;
    let mut paths = vec![exe.clone()];
    if let Ok(canonical) = std::fs::canonicalize(&exe)
        && canonical != exe
    {
        paths.push(canonical);
    }
    Ok(paths)
}

/// Expand a leading `~/` in the config override.
fn expand_home(path: &Path) -> PathBuf {
    if let Ok(rest) = path.strip_prefix("~")
        && let Some(base) = directories::BaseDirs::new()
    {
        return base.home_dir().join(rest);
    }
    path.to_path_buf()
}

/// Resolve the engine for the running executable and this config.
pub fn resolve(config: &EngineSection) -> Result<ResolvedEngine, LocateError> {
    let override_binary = config.binary.as_deref().map(expand_home);
    resolve_from(
        override_binary.as_deref(),
        &current_exe_paths()?,
        super::engine_version(),
        cfg!(windows),
    )
}

static RESOLVED: OnceLock<Option<ResolvedEngine>> = OnceLock::new();

/// Resolve the engine once for this daemon process and keep the result.
/// A missing engine is logged, not fatal: nothing dispatches to the engine
/// yet, and runners driving the user's own agent must keep working.
/// Later calls return the first result unchanged.
pub fn init_at_daemon_start(config: &EngineSection) -> Option<&'static ResolvedEngine> {
    RESOLVED
        .get_or_init(|| match resolve(config) {
            Ok(engine) => {
                tracing::info!(
                    binary = %engine.binary.display(),
                    version = engine.version.as_deref().unwrap_or("override"),
                    source = ?engine.source,
                    "built-in engine resolved"
                );
                Some(engine)
            }
            Err(err) => {
                tracing::warn!(error = %err, "built-in engine unavailable");
                None
            }
        })
        .as_ref()
}

/// The engine resolved at daemon start, if any. `None` before
/// [`init_at_daemon_start`] has run or when no engine was found.
pub fn resolved() -> Option<&'static ResolvedEngine> {
    RESOLVED.get().and_then(Option::as_ref)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    const V: &str = "0.153.4";

    fn touch(path: &Path) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, b"engine").unwrap();
    }

    #[test]
    fn shell_installer_layout_is_found_under_lib() {
        let tmp = tempfile::tempdir().unwrap();
        let exe = tmp.path().join(".local/bin/pidash");
        touch(&exe);
        let binary = tmp
            .path()
            .join(".local/lib/pidash/engine")
            .join(V)
            .join("pidash-agent-engine");
        touch(&binary);

        let got = resolve_from(None, &[exe], V, false).unwrap();
        assert_eq!(got.binary, binary);
        assert_eq!(got.version.as_deref(), Some(V));
        assert_eq!(got.source, EngineSource::Installed(Layout::UnixLib));
    }

    #[test]
    fn extracted_archive_layout_is_found_beside_pidash_on_unix() {
        let tmp = tempfile::tempdir().unwrap();
        let exe = tmp.path().join("pidash-x86_64-unknown-linux-gnu/pidash");
        touch(&exe);
        let binary = tmp
            .path()
            .join("pidash-x86_64-unknown-linux-gnu/engine")
            .join(V)
            .join("pidash-agent-engine");
        touch(&binary);

        let got = resolve_from(None, &[exe], V, false).unwrap();
        assert_eq!(got.binary, binary);
        assert_eq!(got.source, EngineSource::Installed(Layout::Sibling));
    }

    #[test]
    fn lib_layout_wins_over_sibling_on_unix() {
        let tmp = tempfile::tempdir().unwrap();
        let exe = tmp.path().join("bin/pidash");
        touch(&exe);
        let lib = tmp
            .path()
            .join("lib/pidash/engine")
            .join(V)
            .join("pidash-agent-engine");
        touch(&lib);
        touch(
            &tmp.path()
                .join("bin/engine")
                .join(V)
                .join("pidash-agent-engine"),
        );

        let got = resolve_from(None, &[exe], V, false).unwrap();
        assert_eq!(got.binary, lib);
    }

    #[test]
    fn windows_layout_is_engine_dir_beside_the_exe() {
        // Same rule for the PowerShell installer (~\.local\bin) and the MSI
        // (Program Files\pidash\bin).
        let tmp = tempfile::tempdir().unwrap();
        let exe = tmp.path().join("pidash/bin/pidash.exe");
        touch(&exe);
        let binary = tmp
            .path()
            .join("pidash/bin/engine")
            .join(V)
            .join("pidash-agent-engine.exe");
        touch(&binary);

        let got = resolve_from(None, &[exe], V, true).unwrap();
        assert_eq!(got.binary, binary);
        assert_eq!(got.source, EngineSource::Installed(Layout::Sibling));
    }

    #[test]
    fn windows_never_looks_in_the_unix_lib_layout() {
        let tmp = tempfile::tempdir().unwrap();
        let exe = tmp.path().join("bin/pidash.exe");
        touch(&exe);
        touch(
            &tmp.path()
                .join("lib/pidash/engine")
                .join(V)
                .join("pidash-agent-engine.exe"),
        );

        let err = resolve_from(None, &[exe], V, true).unwrap_err();
        let LocateError::NotInstalled { searched, .. } = err else {
            panic!("expected NotInstalled, got {err:?}");
        };
        assert_eq!(searched, vec![tmp.path().join("bin/engine").join(V)]);
    }

    #[test]
    fn only_this_releases_version_is_used() {
        let tmp = tempfile::tempdir().unwrap();
        let exe = tmp.path().join("bin/pidash");
        touch(&exe);
        touch(
            &tmp.path()
                .join("lib/pidash/engine/0.150.0")
                .join("pidash-agent-engine"),
        );

        let err = resolve_from(None, &[exe], V, false).unwrap_err();
        assert!(matches!(err, LocateError::NotInstalled { .. }), "{err:?}");
        assert!(err.to_string().contains("pidash engine install"), "{err}");
    }

    #[test]
    fn a_directory_named_like_the_binary_is_not_an_engine() {
        let tmp = tempfile::tempdir().unwrap();
        let exe = tmp.path().join("bin/pidash");
        touch(&exe);
        fs::create_dir_all(
            tmp.path()
                .join("lib/pidash/engine")
                .join(V)
                .join("pidash-agent-engine"),
        )
        .unwrap();
        assert!(resolve_from(None, &[exe], V, false).is_err());
    }

    #[test]
    fn canonical_exe_path_is_tried_after_the_invoked_one() {
        // A `pidash` symlinked onto PATH from elsewhere still finds the
        // engine of the install it points into.
        let tmp = tempfile::tempdir().unwrap();
        let link = tmp.path().join("elsewhere/bin/pidash");
        touch(&link);
        let real = tmp.path().join("prefix/bin/pidash");
        touch(&real);
        let binary = tmp
            .path()
            .join("prefix/lib/pidash/engine")
            .join(V)
            .join("pidash-agent-engine");
        touch(&binary);

        let got = resolve_from(None, &[link, real], V, false).unwrap();
        assert_eq!(got.binary, binary);
    }

    #[test]
    fn config_override_wins_over_installed_layouts() {
        let tmp = tempfile::tempdir().unwrap();
        let exe = tmp.path().join("bin/pidash");
        touch(&exe);
        touch(
            &tmp.path()
                .join("lib/pidash/engine")
                .join(V)
                .join("pidash-agent-engine"),
        );
        let dev = tmp.path().join("codex-dev/target/debug/codex");
        touch(&dev);

        let got = resolve_from(Some(&dev), &[exe], V, false).unwrap();
        assert_eq!(got.binary, dev);
        assert_eq!(got.version, None);
        assert_eq!(got.source, EngineSource::ConfigOverride);
    }

    #[test]
    fn missing_override_is_an_error_not_a_fallback() {
        let tmp = tempfile::tempdir().unwrap();
        let exe = tmp.path().join("bin/pidash");
        touch(&exe);
        touch(
            &tmp.path()
                .join("lib/pidash/engine")
                .join(V)
                .join("pidash-agent-engine"),
        );
        let missing = tmp.path().join("nope/codex");

        let err = resolve_from(Some(&missing), &[exe], V, false).unwrap_err();
        assert_eq!(err, LocateError::OverrideMissing(missing));
    }

    #[test]
    fn install_root_is_the_primary_layout() {
        let dir = Path::new("/home/u/.local/bin");
        assert_eq!(
            install_root(dir, false),
            Path::new("/home/u/.local/lib/pidash/engine")
        );
        assert_eq!(install_root(dir, true), dir.join("engine"));
    }

    #[test]
    fn home_relative_override_is_expanded() {
        let expanded = expand_home(Path::new("~/codex/bin/codex"));
        if let Some(base) = directories::BaseDirs::new() {
            assert_eq!(expanded, base.home_dir().join("codex/bin/codex"));
        }
        assert_eq!(
            expand_home(Path::new("/abs/codex")),
            Path::new("/abs/codex")
        );
    }
}
