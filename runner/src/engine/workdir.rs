//! Where engine runners work (design §10).
//!
//! `[engine] task_root` is a parent, never a runner's working dir itself.
//! Turning the engine on for a project allocates
//! `<root>/<workspace-slug>/<project-identifier>/`, with a numeric suffix
//! when that folder is already some runner's working dir, would nest with
//! one, or exists and is not empty. Changing the root moves nothing.

use std::path::{Component, Path, PathBuf};

use crate::config::schema::{Config, canonical_for_compare};
use crate::util::fs::create_dir_all_private;
use crate::util::paths::Paths;

/// `<daemon data>/engine/workdirs`: the root while `[engine] task_root` is
/// unset (until PIDESKAPP-42 decides the default location).
pub fn default_task_root(paths: &Paths) -> PathBuf {
    super::codex_config::engine_dir(paths).join("workdirs")
}

/// The root new engine runners are allocated under.
pub fn task_root(paths: &Paths, cfg: &Config) -> PathBuf {
    cfg.engine
        .task_root
        .clone()
        .unwrap_or_else(|| default_task_root(paths))
}

/// Longest root accepted on Windows, leaving room for
/// `\<workspace-slug>\<project-identifier>-NN\` plus the deepest paths a
/// repository checks out below it within `MAX_PATH` (260).
pub const WINDOWS_MAX_ROOT_LEN: usize = 120;

/// A rule [`validate_task_root`] found broken. The message names the rule.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RootError {
    #[error("the task root must be an absolute path, got {0}")]
    NotAbsolute(String),
    #[error("cannot create or open {path}: {why}")]
    NotCreatable { path: String, why: String },
    #[error("{0} is not writable")]
    NotWritable(String),
    #[error("the task root cannot be a filesystem root ({0})")]
    FilesystemRoot(String),
    #[error("the task root cannot be the home directory itself ({0})")]
    HomeDir(String),
    #[error("the task root cannot be inside the Pi Dash config directory ({0})")]
    InsideConfigDir(String),
    #[error(
        "the task root cannot be inside the Pi Dash data directory ({0}); only the default, {1}, is allowed there"
    )]
    InsideDataDir(String, String),
    #[error("the task root cannot be inside runner {runner:?}'s working dir ({path})")]
    InsideRunnerDir { runner: String, path: String },
    #[error(
        "the task root is too long for Windows ({len} characters; at most {WINDOWS_MAX_ROOT_LEN})"
    )]
    TooLong { len: usize },
}

/// A root that passed, with the canonical path and any warnings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidRoot {
    pub path: PathBuf,
    pub warnings: Vec<String>,
}

/// Validate a task root (design §10). Done by the daemon (or by the CLI
/// when no daemon runs), never by the app's webview. Creates the directory
/// when missing; a failed check leaves the configured value as it was.
pub fn validate_task_root(
    paths: &Paths,
    cfg: &Config,
    candidate: &Path,
) -> Result<ValidRoot, RootError> {
    validate_task_root_for(paths, cfg, candidate, cfg!(windows), home_dir().as_deref())
}

fn home_dir() -> Option<PathBuf> {
    directories::BaseDirs::new().map(|b| b.home_dir().to_path_buf())
}

/// [`validate_task_root`] with the platform and home directory passed in,
/// so every rule is testable on every host.
pub fn validate_task_root_for(
    paths: &Paths,
    cfg: &Config,
    candidate: &Path,
    windows: bool,
    home: Option<&Path>,
) -> Result<ValidRoot, RootError> {
    let shown = candidate.display().to_string();
    if !candidate.is_absolute() {
        return Err(RootError::NotAbsolute(shown));
    }
    if windows && candidate.as_os_str().len() > WINDOWS_MAX_ROOT_LEN {
        return Err(RootError::TooLong {
            len: candidate.as_os_str().len(),
        });
    }
    // Cheap lexical checks first, so a bad root is refused before anything
    // is created on disk.
    let lexical = std::path::absolute(candidate).unwrap_or_else(|_| candidate.to_path_buf());
    if lexical.parent().is_none() {
        return Err(RootError::FilesystemRoot(shown));
    }
    create_dir_all_private(candidate).map_err(|e| RootError::NotCreatable {
        path: shown.clone(),
        why: e.to_string(),
    })?;
    let root = std::fs::canonicalize(candidate).map_err(|e| RootError::NotCreatable {
        path: shown.clone(),
        why: e.to_string(),
    })?;
    if !root.is_dir() {
        return Err(RootError::NotCreatable {
            path: shown,
            why: "not a directory".into(),
        });
    }
    let root_shown = root.display().to_string();
    if root.parent().is_none() {
        return Err(RootError::FilesystemRoot(root_shown));
    }
    if let Some(home) = home
        && canonical_for_compare(home) == root
    {
        return Err(RootError::HomeDir(root_shown));
    }
    let config_dir = canonical_for_compare(&paths.config_dir);
    if root.starts_with(&config_dir) {
        return Err(RootError::InsideConfigDir(root_shown));
    }
    let data_dir = canonical_for_compare(&paths.data_dir);
    let default_root = canonical_for_compare(&default_task_root(paths));
    // Inside the data directory (which holds the engine home and the
    // runners' data) only the default root is allowed.
    if root.starts_with(&data_dir) && !root.starts_with(&default_root) {
        return Err(RootError::InsideDataDir(
            root_shown,
            default_root.display().to_string(),
        ));
    }
    // It may contain other runners' working dirs (each allocated folder is
    // checked on its own), but not sit inside one.
    for r in &cfg.runners {
        let wd = canonical_for_compare(&r.workspace.working_dir);
        if root.starts_with(&wd) {
            return Err(RootError::InsideRunnerDir {
                runner: r.name.clone(),
                path: wd.display().to_string(),
            });
        }
    }
    // Writable: create and remove a probe file.
    let probe = root.join(format!(".pidash-write-probe-{}", std::process::id()));
    match std::fs::write(&probe, b"") {
        Ok(()) => {
            let _ = std::fs::remove_file(&probe);
        }
        Err(_) => return Err(RootError::NotWritable(root_shown)),
    }
    let mut warnings = Vec::new();
    if looks_removable_or_network(&root) {
        warnings.push(format!(
            "{} looks like a network or removable drive; engine runs fail while it is not mounted",
            root.display()
        ));
    }
    Ok(ValidRoot {
        path: root,
        warnings,
    })
}

/// Heuristic for the "network or removable drive" warning.
fn looks_removable_or_network(path: &Path) -> bool {
    let s = path.to_string_lossy();
    s.starts_with(r"\\")
        || s.starts_with("//")
        || ["/media/", "/mnt/", "/Volumes/", "/run/media/", "/net/"]
            .iter()
            .any(|p| s.starts_with(p))
}

/// Allocate and create the folder of a new engine runner for `project` in
/// `workspace` under the configured task root (design §10). Used by every
/// path that turns the engine on for a project: `EngineRunnerAdd`,
/// `pidash runner add --agent engine`, machine control's `create_runner`
/// and the reconcile's re-add.
pub fn allocate_engine_folder(
    paths: &Paths,
    workspace: &str,
    project: &str,
) -> anyhow::Result<PathBuf> {
    let cfg = crate::config::file::load_config(paths)?;
    let root = task_root(paths, &cfg);
    let dir = allocate_working_dir(&root, workspace, project, &cfg)?;
    create_dir_all_private(&dir).map_err(|e| anyhow::anyhow!("creating {}: {e}", dir.display()))?;
    Ok(dir)
}

/// A safe single path component from a slug or project identifier.
fn component(raw: &str, fallback: &str) -> String {
    let out: String = raw
        .trim()
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.') {
                c
            } else {
                '-'
            }
        })
        .collect();
    let out = out.trim_matches(|c| c == '.' || c == '-').to_string();
    if out.is_empty()
        || Path::new(&out)
            .components()
            .any(|c| c != Component::Normal(out.as_ref()))
    {
        fallback.to_string()
    } else {
        out
    }
}

/// Allocate the folder for turning the engine on for `project` in
/// `workspace`: `<root>/<workspace>/<project>/`, else `<project>-2`, `-3`, …
/// The first candidate that is nobody's working dir, does not nest with any
/// runner's, and is missing or empty wins. Fails when the workspace folder
/// itself sits inside a runner's working dir (no suffix can fix that).
pub fn allocate_working_dir(
    root: &Path,
    workspace: &str,
    project: &str,
    cfg: &Config,
) -> anyhow::Result<PathBuf> {
    let parent = root.join(component(workspace, "workspace"));
    let name = component(project, "project");
    let taken: Vec<(String, PathBuf)> = cfg
        .runners
        .iter()
        .map(|r| {
            (
                r.name.clone(),
                canonical_for_compare(&r.workspace.working_dir),
            )
        })
        .collect();
    let parent_c = canonical_for_compare(&parent);
    if let Some((runner, dir)) = taken.iter().find(|(_, t)| parent_c.starts_with(t)) {
        anyhow::bail!(
            "{} is inside runner {runner:?}'s working dir ({}); pick another task root",
            parent.display(),
            dir.display()
        );
    }
    for n in 1..=1000 {
        let candidate = if n == 1 {
            parent.join(&name)
        } else {
            parent.join(format!("{name}-{n}"))
        };
        let c = canonical_for_compare(&candidate);
        let collides = taken
            .iter()
            .any(|(_, t)| *t == c || t.starts_with(&c) || c.starts_with(t));
        if collides || !is_missing_or_empty(&candidate) {
            continue;
        }
        return Ok(candidate);
    }
    anyhow::bail!(
        "no free folder for {name} under {} after 1000 tries",
        parent.display()
    )
}

fn is_missing_or_empty(path: &Path) -> bool {
    match std::fs::read_dir(path) {
        Ok(mut entries) => entries.next().is_none(),
        Err(e) => e.kind() == std::io::ErrorKind::NotFound,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::schema::{DaemonConfig, RunnerConfig, WorkspaceSection};

    fn paths(root: &Path) -> Paths {
        Paths {
            config_dir: root.join("config"),
            data_dir: root.join("data"),
            runtime_dir: root.join("runtime"),
            machine_context_dir: root.join("config"),
        }
    }

    fn runner(name: &str, wd: PathBuf) -> RunnerConfig {
        RunnerConfig {
            name: name.into(),
            runner_id: uuid::Uuid::new_v4(),
            workspace_slug: Some("acme".into()),
            project_slug: Some("WEB".into()),
            pod_id: None,
            workspace: WorkspaceSection { working_dir: wd },
            agent: Default::default(),
            codex: Default::default(),
            claude_code: Default::default(),
            cursor_agent: Default::default(),
            openclaw: Default::default(),
            grok: Default::default(),
            muse_code: Default::default(),
            approval_policy: Default::default(),
        }
    }

    fn cfg(runners: Vec<RunnerConfig>) -> Config {
        Config {
            version: 2,
            daemon: DaemonConfig::default(),
            runners,
            cli: None,
            engine: Default::default(),
            auth: Default::default(),
        }
    }

    #[test]
    fn default_root_is_under_the_daemon_data_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let p = paths(tmp.path());
        assert_eq!(
            task_root(&p, &cfg(vec![])),
            p.data_dir.join("engine/workdirs")
        );
        let mut c = cfg(vec![]);
        c.engine.task_root = Some(tmp.path().join("tasks"));
        assert_eq!(task_root(&p, &c), tmp.path().join("tasks"));
    }

    #[test]
    fn allocates_workspace_project_folders_with_a_suffix_on_clash() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("root");
        let c = cfg(vec![]);
        assert_eq!(
            allocate_working_dir(&root, "acme", "WEB", &c).unwrap(),
            root.join("acme/WEB")
        );
        // Same identifier in another workspace: no clash.
        assert_eq!(
            allocate_working_dir(&root, "beta", "WEB", &c).unwrap(),
            root.join("beta/WEB")
        );
        // Already some runner's working dir → suffix.
        let c = cfg(vec![runner("a", root.join("acme/WEB"))]);
        assert_eq!(
            allocate_working_dir(&root, "acme", "WEB", &c).unwrap(),
            root.join("acme/WEB-2")
        );
        // A non-empty folder that is nobody's → suffix too, never reused.
        std::fs::create_dir_all(root.join("acme/WEB-2")).unwrap();
        std::fs::write(root.join("acme/WEB-2/file"), b"x").unwrap();
        assert_eq!(
            allocate_working_dir(&root, "acme", "WEB", &c).unwrap(),
            root.join("acme/WEB-3")
        );
        // An empty existing folder is fine.
        std::fs::create_dir_all(root.join("beta/API")).unwrap();
        assert_eq!(
            allocate_working_dir(&root, "beta", "API", &c).unwrap(),
            root.join("beta/API")
        );
        // A runner working inside the candidate (`acme/API/sub`) forces a
        // suffix; a runner working in the workspace folder itself cannot be
        // worked around and is refused.
        let c = cfg(vec![runner("inner", root.join("acme/API/sub"))]);
        assert_eq!(
            allocate_working_dir(&root, "acme", "API", &c).unwrap(),
            root.join("acme/API-2")
        );
        let c = cfg(vec![runner("parent", root.join("gamma"))]);
        assert!(allocate_working_dir(&root, "gamma", "APP", &c).is_err());
    }

    #[test]
    fn hostile_identifiers_stay_one_component() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("root");
        let got = allocate_working_dir(&root, "../..", "a/b", &cfg(vec![])).unwrap();
        assert_eq!(got, root.join("workspace").join("a-b"));
    }

    #[test]
    fn root_validation_rules() {
        let tmp = tempfile::tempdir().unwrap();
        let p = paths(tmp.path());
        let c = cfg(vec![runner("r", tmp.path().join("repo"))]);
        std::fs::create_dir_all(tmp.path().join("repo")).unwrap();
        let home = tmp.path().join("home");
        std::fs::create_dir_all(&home).unwrap();
        let check =
            |path: &Path, windows: bool| validate_task_root_for(&p, &c, path, windows, Some(&home));

        assert!(matches!(
            check(Path::new("relative/dir"), false),
            Err(RootError::NotAbsolute(_))
        ));
        assert!(matches!(
            check(Path::new("/"), false),
            Err(RootError::FilesystemRoot(_))
        ));
        assert!(matches!(check(&home, false), Err(RootError::HomeDir(_))));
        assert!(matches!(
            check(&p.config_dir.join("tasks"), false),
            Err(RootError::InsideConfigDir(_))
        ));
        assert!(matches!(
            check(&p.data_dir.join("engine/acct/codex-home"), false),
            Err(RootError::InsideDataDir(..))
        ));
        assert!(matches!(
            check(&tmp.path().join("repo/sub"), false),
            Err(RootError::InsideRunnerDir { .. })
        ));
        let long = tmp.path().join("x".repeat(WINDOWS_MAX_ROOT_LEN));
        assert!(matches!(check(&long, true), Err(RootError::TooLong { .. })));

        // The default root is allowed inside the data dir, and is created.
        let ok = check(&default_task_root(&p), false).unwrap();
        assert!(ok.path.is_dir());
        // A root that contains a runner's working dir is fine.
        let ok = check(tmp.path(), false);
        assert!(ok.is_ok(), "{ok:?}");
        let ok = check(&tmp.path().join("tasks"), false).unwrap();
        assert!(ok.warnings.is_empty());
    }

    /// A task root the daemon creates must be owner-only: engine runs hold
    /// source checkouts and generated output under it, and a configured root
    /// may sit outside the daemon's data dir, where no other hardening
    /// applies.
    #[cfg(unix)]
    #[test]
    fn a_missing_task_root_is_created_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().unwrap();
        let p = paths(tmp.path());
        let candidate = tmp.path().join("tasks");
        let valid = validate_task_root_for(&p, &cfg(vec![]), &candidate, false, None).unwrap();
        let mode = std::fs::metadata(&valid.path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o700, "created task root must be owner-only");
    }

    /// An allocated engine folder (`<root>/<ws>/<project>/`, daemon-created)
    /// is owner-only, including the parents created along the way. The root
    /// itself is left alone when it already exists.
    #[cfg(unix)]
    #[test]
    fn an_allocated_engine_folder_is_created_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().unwrap();
        let p = paths(tmp.path());
        std::fs::create_dir_all(&p.config_dir).unwrap();
        std::fs::create_dir_all(&p.data_dir).unwrap();
        std::fs::write(
            p.config_path(),
            "version = 2\n\n[daemon]\ncloud_url = \"https://x\"\n",
        )
        .unwrap();
        let dir = allocate_engine_folder(&p, "acme", "WEB").unwrap();
        for dir in [
            p.data_dir.join("engine"),
            p.data_dir.join("engine/workdirs"),
            p.data_dir.join("engine/workdirs/acme"),
            dir.clone(),
        ] {
            let mode = std::fs::metadata(&dir).unwrap().permissions().mode() & 0o777;
            assert_eq!(
                mode,
                0o700,
                "created dir {} must be owner-only",
                dir.display()
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn unwritable_root_is_refused() {
        use std::os::unix::fs::PermissionsExt;
        if nix::unistd::Uid::effective().is_root() {
            return; // root can write anywhere
        }
        let tmp = tempfile::tempdir().unwrap();
        let p = paths(tmp.path());
        let ro = tmp.path().join("ro");
        std::fs::create_dir_all(&ro).unwrap();
        std::fs::set_permissions(&ro, std::fs::Permissions::from_mode(0o500)).unwrap();
        let got = validate_task_root_for(&p, &cfg(vec![]), &ro, false, None);
        std::fs::set_permissions(&ro, std::fs::Permissions::from_mode(0o700)).unwrap();
        assert!(matches!(got, Err(RootError::NotWritable(_))), "{got:?}");
    }
}
