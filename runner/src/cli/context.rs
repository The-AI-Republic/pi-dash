// Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `pidash context …` subcommands and the two-tier `context.md` files.
//!
//! There are two context files, written at different scopes:
//!
//! - **Machine-level** `~/.pidash/context.md` (see [`Paths::machine_context_path`]):
//!   every project in every workspace this dev machine is bound to, keyed by
//!   workspace slug. Written on `auth login`, `runner add`, daemon workspace
//!   resolution, and `pidash context refresh`. This is the lookup table for
//!   "what else exists in this workspace".
//! - **Per-working-dir** `<working_dir>/.pidash/context.md`: the single project
//!   this working dir belongs to. This is authoritative for "which project am I
//!   working in".
//!
//! Both carry a `scope`, a `generated_at` RFC 3339 UTC stamp, and each project
//! carries a `repo_url`. The front-matter is parsed by one hand-rolled,
//! indentation-aware line scanner ([`scan_front_matter`]) shared by both
//! documents; adding a field is one line in the struct and one arm in
//! [`apply_project_field`]. The scanner tolerates missing keys so files already
//! on disk (no `scope` / `generated_at` / `repo_url`) keep parsing.
//!
//! [`Paths::machine_context_path`]: crate::util::paths::Paths::machine_context_path

use std::fs;
use std::path::{Path, PathBuf};

use chrono::{SecondsFormat, Utc};
use clap::{Args, Subcommand};
use serde::Serialize;

use crate::api_client::{ApiClient, CliEnv, CliError, EXIT_INVALID, EXIT_UNKNOWN, report_error};

use super::project::{ProjectRow, list_projects, resolve_project};

#[derive(Debug, Args)]
pub struct ContextArgs {
    #[command(subcommand)]
    pub command: ContextCommand,
}

#[derive(Debug, Subcommand)]
pub enum ContextCommand {
    /// Write the per-working-dir .pidash/context.md for a project.
    Init(InitArgs),
    /// Rewrite the machine-level ~/.pidash/context.md for the bound workspace.
    Refresh,
}

#[derive(Debug, Args)]
pub struct InitArgs {
    /// Project identifier (slug like `ENG`) or project UUID.
    #[arg(long)]
    pub project: String,

    /// Local workspace directory. Defaults to the current directory.
    #[arg(long)]
    pub workspace: Option<PathBuf>,
}

#[derive(Debug, Clone, Default, Serialize)]
struct ContextProject {
    id: String,
    identifier: String,
    name: String,
    description: String,
    repo_url: String,
    is_default: bool,
}

/// A working-dir `context.md`: the one project this directory belongs to.
#[derive(Debug, Clone, Default)]
struct ProjectDoc {
    workspace_slug: String,
    default_project_id: String,
    generated_at: String,
    projects: Vec<ContextProject>,
}

/// One workspace entry inside the machine-level document.
#[derive(Debug, Clone, Default)]
struct MachineWorkspace {
    slug: String,
    default_project_id: String,
    projects: Vec<ContextProject>,
}

/// The machine-level `context.md`: every workspace this machine is bound to.
#[derive(Debug, Clone, Default)]
struct MachineDoc {
    generated_at: String,
    workspaces: Vec<MachineWorkspace>,
}

pub async fn run(args: ContextArgs, paths: &crate::util::paths::Paths) -> i32 {
    let env = match CliEnv::resolve(paths) {
        Ok(e) => e,
        Err(e) => return report_error(&e),
    };
    let client = match ApiClient::new(env) {
        Ok(c) => c,
        Err(e) => return report_error(&CliError::new(EXIT_UNKNOWN, format!("{e}"))),
    };

    let result = match args.command {
        ContextCommand::Init(a) => cmd_init(&client, a).await,
        ContextCommand::Refresh => cmd_refresh(paths, &client).await,
    };
    match result {
        Ok(()) => 0,
        Err(e) => report_error(&e),
    }
}

/// Write `<workspace_dir>/.pidash/context.md` scoped to a single project, and
/// best-effort exclude `.pidash/` from git. Callers: `runner add` and the daemon
/// after it resolves a workspace.
pub async fn write_context_for_project(
    paths: &crate::util::paths::Paths,
    workspace_dir: &Path,
    project_ref: &str,
) -> Result<PathBuf, CliError> {
    let env = CliEnv::resolve(paths)?;
    let client = ApiClient::new(env).map_err(|e| CliError::new(EXIT_UNKNOWN, format!("{e}")))?;
    let project = resolve_project(&client, project_ref).await?;
    write_project_context_file(workspace_dir, &client.env.workspace_slug, &project)
}

/// Rewrite `~/.pidash/context.md` for the currently-bound workspace, listing all
/// of its projects. Upserts this workspace into the file by slug, leaving other
/// workspaces intact. Resolves its own API client from `paths`; used by
/// `auth login` (best-effort) where no client is threaded in.
pub async fn write_machine_context_for_paths(
    paths: &crate::util::paths::Paths,
) -> Result<PathBuf, CliError> {
    let env = CliEnv::resolve(paths)?;
    let client = ApiClient::new(env).map_err(|e| CliError::new(EXIT_UNKNOWN, format!("{e}")))?;
    write_machine_context(paths, &client).await
}

async fn cmd_init(client: &ApiClient, args: InitArgs) -> Result<(), CliError> {
    if args.project.trim().is_empty() {
        return Err(CliError::new(EXIT_INVALID, "--project must not be empty"));
    }
    let workspace = match args.workspace {
        Some(p) => p,
        None => std::env::current_dir().map_err(|e| {
            CliError::new(EXIT_UNKNOWN, format!("resolving current directory: {e}"))
        })?,
    };
    let project = resolve_project(client, &args.project).await?;
    let path = write_project_context_file(&workspace, &client.env.workspace_slug, &project)?;
    println!("{}", serde_json::json!({"path": path, "project": project}));
    Ok(())
}

async fn cmd_refresh(
    paths: &crate::util::paths::Paths,
    client: &ApiClient,
) -> Result<(), CliError> {
    let path = write_machine_context(paths, client).await?;
    println!(
        "{}",
        serde_json::json!({"path": path, "workspace": client.env.workspace_slug})
    );
    Ok(())
}

/// Write the working-dir document. This **replaces** the file wholesale: a
/// working dir belongs to exactly one project, so there is nothing to preserve
/// and nothing accumulates.
fn write_project_context_file(
    workspace_dir: &Path,
    workspace_slug: &str,
    project: &ProjectRow,
) -> Result<PathBuf, CliError> {
    let pidash_dir = workspace_dir.join(".pidash");
    fs::create_dir_all(&pidash_dir)
        .map_err(|e| CliError::new(EXIT_UNKNOWN, format!("creating {:?}: {e}", pidash_dir)))?;
    let path = pidash_dir.join("context.md");
    let doc = ProjectDoc {
        workspace_slug: workspace_slug.to_string(),
        default_project_id: project.id.clone(),
        generated_at: now_stamp(),
        projects: vec![ContextProject::from(project)],
    };
    fs::write(&path, render_project_context(&doc))
        .map_err(|e| CliError::new(EXIT_UNKNOWN, format!("writing {:?}: {e}", path)))?;
    // Best-effort: keep the file out of the repo's diff without touching the
    // tracked `.gitignore`.
    if let Err(err) = exclude_pidash_from_git(workspace_dir) {
        tracing::debug!(error = %err, "could not add .pidash/ to git exclude");
    }
    Ok(path)
}

async fn write_machine_context(
    paths: &crate::util::paths::Paths,
    client: &ApiClient,
) -> Result<PathBuf, CliError> {
    let projects = list_projects(client).await?;
    let default_project_id = projects
        .iter()
        .find(|p| p.is_default)
        .map(|p| p.id.clone())
        .unwrap_or_default();
    let workspace = MachineWorkspace {
        slug: client.env.workspace_slug.clone(),
        default_project_id,
        projects: projects.iter().map(ContextProject::from).collect(),
    };

    let path = paths.machine_context_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| CliError::new(EXIT_UNKNOWN, format!("creating {:?}: {e}", parent)))?;
    }
    let mut doc = fs::read_to_string(&path)
        .ok()
        .map(|body| parse_machine_context(&body))
        .unwrap_or_default();
    upsert_workspace(&mut doc.workspaces, workspace);
    doc.generated_at = now_stamp();
    fs::write(&path, render_machine_context(&doc))
        .map_err(|e| CliError::new(EXIT_UNKNOWN, format!("writing {:?}: {e}", path)))?;
    Ok(path)
}

impl From<&ProjectRow> for ContextProject {
    fn from(project: &ProjectRow) -> Self {
        Self {
            id: project.id.clone(),
            identifier: project.identifier.clone(),
            name: project.name.clone(),
            description: project.description.clone(),
            repo_url: project.repo_url.clone(),
            is_default: project.is_default,
        }
    }
}

fn upsert_workspace(workspaces: &mut Vec<MachineWorkspace>, workspace: MachineWorkspace) {
    if let Some(existing) = workspaces.iter_mut().find(|w| w.slug == workspace.slug) {
        *existing = workspace;
    } else {
        workspaces.push(workspace);
    }
}

fn now_stamp() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true)
}

// ---------------------------------------------------------------------------
// Rendering
// ---------------------------------------------------------------------------

fn render_project_context(doc: &ProjectDoc) -> String {
    let mut out = format!(
        "---\nscope: \"project\"\nworkspace_slug: {}\ndefault_project_id: {}\ngenerated_at: {}\nprojects:\n",
        yaml_string(&doc.workspace_slug),
        yaml_string(&doc.default_project_id),
        yaml_string(&doc.generated_at),
    );
    for project in &doc.projects {
        render_project_block(&mut out, project, "  ");
    }
    out.push_str(
        "---\n\n# Pi Dash Workspace Context\n\n\
         This working directory is linked to the Pi Dash project above. For the \
         full list of projects in this and other workspaces on this machine, see \
         the machine-level `~/.pidash/context.md`.\n",
    );
    out
}

fn render_machine_context(doc: &MachineDoc) -> String {
    let mut out = format!(
        "---\nscope: \"machine\"\ngenerated_at: {}\nworkspaces:\n",
        yaml_string(&doc.generated_at),
    );
    for workspace in &doc.workspaces {
        out.push_str(&format!(
            "  - slug: {}\n    default_project_id: {}\n    projects:\n",
            yaml_string(&workspace.slug),
            yaml_string(&workspace.default_project_id),
        ));
        for project in &workspace.projects {
            render_project_block(&mut out, project, "      ");
        }
    }
    out.push_str(
        "---\n\n# Pi Dash Machine Context\n\n\
         Every Pi Dash project in each workspace this machine is bound to. Use it \
         to look up projects other than the one in the current working directory's \
         `.pidash/context.md`.\n",
    );
    out
}

/// Render one project as a YAML list item. `item_indent` is the indentation of
/// the opening `- id:` line; fields are indented two spaces further.
fn render_project_block(out: &mut String, project: &ContextProject, item_indent: &str) {
    let field_indent = format!("{item_indent}  ");
    out.push_str(&format!(
        "{item_indent}- id: {}\n",
        yaml_string(&project.id)
    ));
    out.push_str(&format!(
        "{field_indent}identifier: {}\n",
        yaml_string(&project.identifier)
    ));
    out.push_str(&format!(
        "{field_indent}name: {}\n",
        yaml_string(&project.name)
    ));
    out.push_str(&format!(
        "{field_indent}description: {}\n",
        yaml_string(&project.description)
    ));
    out.push_str(&format!(
        "{field_indent}repo_url: {}\n",
        yaml_string(&project.repo_url)
    ));
    out.push_str(&format!(
        "{field_indent}is_default: {}\n",
        project.is_default
    ));
}

// ---------------------------------------------------------------------------
// Parsing — one shared, indentation-aware line scanner (D4)
// ---------------------------------------------------------------------------

/// One meaningful line of the YAML front-matter, decomposed. Nesting is
/// unambiguous from `is_list_item` + `key`: `- slug:` opens a workspace,
/// `- id:` opens a project.
struct ScanLine {
    is_list_item: bool,
    key: String,
    value: String,
}

/// Scan the `---`-fenced front matter into `(is_list_item, key, value)` tuples.
/// Returns `None` when the body does not open with a `---` fence.
fn scan_front_matter(body: &str) -> Option<Vec<ScanLine>> {
    let mut lines = body.lines();
    if lines.next()? != "---" {
        return None;
    }
    let mut out = Vec::new();
    for line in lines {
        if line == "---" {
            break;
        }
        let trimmed = line.trim_start();
        if trimmed.is_empty() {
            continue;
        }
        let (rest, is_list_item) = match trimmed.strip_prefix("- ") {
            Some(after) => (after, true),
            None => (trimmed, false),
        };
        // Split on the first colon only: a value like a repo URL contains
        // colons, but the key/value boundary is always the first one and the
        // value is rendered quoted.
        let (key, value) = match rest.split_once(':') {
            Some((k, v)) => (k.trim().to_string(), parse_scalar(v)),
            None => (rest.trim().to_string(), String::new()),
        };
        out.push(ScanLine {
            is_list_item,
            key,
            value,
        });
    }
    Some(out)
}

/// Symmetric reader for the working-dir document. The working-dir write
/// replaces the file wholesale (D3), so nothing in the runner reads it back
/// today; this parser exists to round-trip the format in tests and for any
/// future reader. Kept single-sourced with the machine parser via
/// [`scan_front_matter`] / [`apply_project_field`] (D4).
#[cfg(test)]
fn parse_project_context(body: &str) -> ProjectDoc {
    let Some(lines) = scan_front_matter(body) else {
        return ProjectDoc::default();
    };
    let mut doc = ProjectDoc::default();
    let mut current: Option<ContextProject> = None;

    for line in lines {
        if line.is_list_item && line.key == "id" {
            if let Some(project) = current.take() {
                doc.projects.push(project);
            }
            current = Some(ContextProject {
                id: line.value,
                ..Default::default()
            });
        } else if let Some(project) = current.as_mut() {
            apply_project_field(project, &line.key, &line.value);
        } else {
            match line.key.as_str() {
                "workspace_slug" => doc.workspace_slug = line.value,
                "default_project_id" => doc.default_project_id = line.value,
                "generated_at" => doc.generated_at = line.value,
                _ => {}
            }
        }
    }
    if let Some(project) = current {
        doc.projects.push(project);
    }
    doc
}

fn parse_machine_context(body: &str) -> MachineDoc {
    let Some(lines) = scan_front_matter(body) else {
        return MachineDoc::default();
    };
    let mut doc = MachineDoc::default();
    let mut workspace: Option<MachineWorkspace> = None;
    let mut project: Option<ContextProject> = None;

    // Close the open project (if any) into the open workspace.
    fn flush_project(
        workspace: &mut Option<MachineWorkspace>,
        project: &mut Option<ContextProject>,
    ) {
        if let (Some(p), Some(w)) = (project.take(), workspace.as_mut()) {
            w.projects.push(p);
        }
    }

    for line in lines {
        if line.is_list_item && line.key == "slug" {
            flush_project(&mut workspace, &mut project);
            if let Some(w) = workspace.take() {
                doc.workspaces.push(w);
            }
            workspace = Some(MachineWorkspace {
                slug: line.value,
                ..Default::default()
            });
        } else if line.is_list_item && line.key == "id" {
            flush_project(&mut workspace, &mut project);
            project = Some(ContextProject {
                id: line.value,
                ..Default::default()
            });
        } else if let Some(p) = project.as_mut() {
            apply_project_field(p, &line.key, &line.value);
        } else if let Some(w) = workspace.as_mut() {
            if line.key == "default_project_id" {
                w.default_project_id = line.value;
            }
        } else if line.key == "generated_at" {
            doc.generated_at = line.value;
        }
    }
    flush_project(&mut workspace, &mut project);
    if let Some(w) = workspace {
        doc.workspaces.push(w);
    }
    doc
}

/// Apply one scalar field to a project. The single place a new project field is
/// wired into parsing.
fn apply_project_field(project: &mut ContextProject, key: &str, value: &str) {
    match key {
        "identifier" => project.identifier = value.to_string(),
        "name" => project.name = value.to_string(),
        "description" => project.description = value.to_string(),
        "repo_url" => project.repo_url = value.to_string(),
        "is_default" => project.is_default = value.eq_ignore_ascii_case("true"),
        _ => {}
    }
}

fn parse_scalar(value: &str) -> String {
    let value = value.trim();
    if !(value.starts_with('"') && value.ends_with('"') && value.len() >= 2) {
        return value.to_string();
    }
    serde_json::from_str::<String>(value).unwrap_or_else(|_| value[1..value.len() - 1].to_string())
}

fn yaml_string(value: &str) -> String {
    serde_json::to_string(value).expect("serializing YAML scalar")
}

// ---------------------------------------------------------------------------
// Git exclude (D5)
// ---------------------------------------------------------------------------

/// Append `.pidash/` to `$GIT_DIR/info/exclude` when `workspace_dir` is a git
/// repo, so the context file never shows up in the repo's diff. Deliberately
/// avoids `.gitignore` — that is a tracked file in someone else's repo. A no-op
/// when the directory is not a git repo or the line is already present.
fn exclude_pidash_from_git(workspace_dir: &Path) -> std::io::Result<()> {
    let Some(git_dir) = resolve_git_dir(workspace_dir) else {
        return Ok(());
    };
    let info_dir = git_dir.join("info");
    fs::create_dir_all(&info_dir)?;
    let exclude_path = info_dir.join("exclude");
    let existing = fs::read_to_string(&exclude_path).unwrap_or_default();
    if existing.lines().any(|l| l.trim() == ".pidash/") {
        return Ok(());
    }
    let mut body = existing;
    if !body.is_empty() && !body.ends_with('\n') {
        body.push('\n');
    }
    body.push_str(".pidash/\n");
    fs::write(&exclude_path, body)
}

/// Resolve the git directory for `workspace_dir`, handling both a `.git`
/// directory and a `.git` *file* (worktree / submodule gitdir pointer).
fn resolve_git_dir(workspace_dir: &Path) -> Option<PathBuf> {
    let dot_git = workspace_dir.join(".git");
    let meta = fs::metadata(&dot_git).ok()?;
    if meta.is_dir() {
        return Some(dot_git);
    }
    if meta.is_file() {
        // A `.git` file points at the real gitdir: `gitdir: <path>`.
        let contents = fs::read_to_string(&dot_git).ok()?;
        let target = contents.lines().find_map(|l| l.strip_prefix("gitdir:"))?;
        let target = PathBuf::from(target.trim());
        let resolved = if target.is_absolute() {
            target
        } else {
            workspace_dir.join(target)
        };
        return Some(resolved);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project(id: &str, identifier: &str, name: &str) -> ContextProject {
        ContextProject {
            id: id.to_string(),
            identifier: identifier.to_string(),
            name: name.to_string(),
            description: String::new(),
            repo_url: format!("https://github.com/acme/{identifier}"),
            is_default: false,
        }
    }

    #[test]
    fn project_doc_round_trips() {
        let doc = ProjectDoc {
            workspace_slug: "acme".to_string(),
            default_project_id: "p1".to_string(),
            generated_at: "2026-09-12T20:38:38Z".to_string(),
            projects: vec![project("p1", "WEB", "Website")],
        };

        let parsed = parse_project_context(&render_project_context(&doc));

        assert_eq!(parsed.workspace_slug, "acme");
        assert_eq!(parsed.default_project_id, "p1");
        assert_eq!(parsed.generated_at, "2026-09-12T20:38:38Z");
        assert_eq!(parsed.projects.len(), 1);
        assert_eq!(parsed.projects[0].identifier, "WEB");
        assert_eq!(
            parsed.projects[0].repo_url,
            "https://github.com/acme/WEB"
        );
    }

    #[test]
    fn project_doc_round_trips_escaped_strings() {
        let mut p = project("p1", "WEB", "Website");
        p.description = "line 1\nline \"2\" \\ path: with colon".to_string();
        let doc = ProjectDoc {
            workspace_slug: "acme\nworkspace".to_string(),
            default_project_id: "p1".to_string(),
            generated_at: "2026-09-12T20:38:38Z".to_string(),
            projects: vec![p],
        };

        let parsed = parse_project_context(&render_project_context(&doc));

        assert_eq!(parsed.workspace_slug, "acme\nworkspace");
        assert_eq!(
            parsed.projects[0].description,
            "line 1\nline \"2\" \\ path: with colon"
        );
    }

    #[test]
    fn machine_doc_round_trips_multiple_workspaces() {
        let doc = MachineDoc {
            generated_at: "2026-09-12T20:38:38Z".to_string(),
            workspaces: vec![
                MachineWorkspace {
                    slug: "acme".to_string(),
                    default_project_id: "p2".to_string(),
                    projects: vec![project("p1", "WEB", "Website"), project("p2", "API", "API")],
                },
                MachineWorkspace {
                    slug: "other".to_string(),
                    default_project_id: "p3".to_string(),
                    projects: vec![project("p3", "OPS", "Ops")],
                },
            ],
        };

        let parsed = parse_machine_context(&render_machine_context(&doc));

        assert_eq!(parsed.generated_at, "2026-09-12T20:38:38Z");
        assert_eq!(parsed.workspaces.len(), 2);
        assert_eq!(parsed.workspaces[0].slug, "acme");
        assert_eq!(parsed.workspaces[0].default_project_id, "p2");
        assert_eq!(parsed.workspaces[0].projects.len(), 2);
        assert_eq!(parsed.workspaces[0].projects[1].identifier, "API");
        assert_eq!(parsed.workspaces[1].slug, "other");
        assert_eq!(parsed.workspaces[1].projects[0].identifier, "OPS");
    }

    #[test]
    fn parses_legacy_project_file_without_scope_generated_at_or_repo_url() {
        // The shape written before this change: no `scope`, no `generated_at`,
        // no `repo_url`, `projects` a list.
        let legacy = "---\nworkspace_slug: \"acme\"\ndefault_project_id: \"p1\"\nprojects:\n  - id: \"p1\"\n    identifier: \"WEB\"\n    name: \"Website\"\n    description: \"\"\n    is_default: true\n---\n\n# Pi Dash Workspace Context\n";

        let parsed = parse_project_context(legacy);

        assert_eq!(parsed.workspace_slug, "acme");
        assert_eq!(parsed.default_project_id, "p1");
        assert_eq!(parsed.generated_at, "");
        assert_eq!(parsed.projects.len(), 1);
        assert_eq!(parsed.projects[0].identifier, "WEB");
        assert_eq!(parsed.projects[0].repo_url, "");
        assert!(parsed.projects[0].is_default);
    }

    #[test]
    fn upsert_workspace_replaces_by_slug_without_dropping_others() {
        let mut workspaces = vec![
            MachineWorkspace {
                slug: "acme".to_string(),
                default_project_id: "p1".to_string(),
                projects: vec![project("p1", "WEB", "Website")],
            },
            MachineWorkspace {
                slug: "other".to_string(),
                default_project_id: "p3".to_string(),
                projects: vec![project("p3", "OPS", "Ops")],
            },
        ];

        upsert_workspace(
            &mut workspaces,
            MachineWorkspace {
                slug: "acme".to_string(),
                default_project_id: "p2".to_string(),
                projects: vec![project("p2", "API", "API")],
            },
        );

        assert_eq!(workspaces.len(), 2);
        assert_eq!(workspaces[0].slug, "acme");
        assert_eq!(workspaces[0].default_project_id, "p2");
        assert_eq!(workspaces[0].projects.len(), 1);
        assert_eq!(workspaces[0].projects[0].identifier, "API");
        assert_eq!(workspaces[1].slug, "other");
    }

    #[test]
    fn write_project_context_replaces_previous_project() {
        let tmp = tempfile::tempdir().unwrap();
        let row_a = ProjectRow {
            id: "p1".to_string(),
            identifier: "WEB".to_string(),
            name: "Website".to_string(),
            description: String::new(),
            repo_url: "https://github.com/acme/web".to_string(),
            is_default: true,
        };
        let row_b = ProjectRow {
            id: "p2".to_string(),
            identifier: "API".to_string(),
            name: "API".to_string(),
            description: String::new(),
            repo_url: "https://github.com/acme/api".to_string(),
            is_default: false,
        };

        write_project_context_file(tmp.path(), "acme", &row_a).unwrap();
        write_project_context_file(tmp.path(), "acme", &row_b).unwrap();

        let body = fs::read_to_string(tmp.path().join(".pidash/context.md")).unwrap();
        let parsed = parse_project_context(&body);
        // The second write replaced the first — the working dir belongs to one
        // project, so nothing accumulates.
        assert_eq!(parsed.projects.len(), 1);
        assert_eq!(parsed.projects[0].identifier, "API");
        assert_eq!(parsed.default_project_id, "p2");
    }

    #[test]
    fn exclude_added_to_git_dir_info_exclude_once() {
        let tmp = tempfile::tempdir().unwrap();
        // Simulate a `.git` directory.
        fs::create_dir_all(tmp.path().join(".git")).unwrap();

        exclude_pidash_from_git(tmp.path()).unwrap();
        exclude_pidash_from_git(tmp.path()).unwrap();

        let exclude = fs::read_to_string(tmp.path().join(".git/info/exclude")).unwrap();
        assert_eq!(exclude.matches(".pidash/").count(), 1, "added exactly once");
    }

    #[test]
    fn exclude_handles_dot_git_file_worktree_pointer() {
        let tmp = tempfile::tempdir().unwrap();
        let real_git = tmp.path().join("real-gitdir");
        fs::create_dir_all(&real_git).unwrap();
        fs::write(
            tmp.path().join(".git"),
            format!("gitdir: {}\n", real_git.display()),
        )
        .unwrap();

        exclude_pidash_from_git(tmp.path()).unwrap();

        let exclude = fs::read_to_string(real_git.join("info/exclude")).unwrap();
        assert!(exclude.contains(".pidash/"));
    }

    #[test]
    fn exclude_is_noop_when_not_a_git_repo() {
        let tmp = tempfile::tempdir().unwrap();
        // No `.git` at all — must not error and must not create anything.
        exclude_pidash_from_git(tmp.path()).unwrap();
        assert!(!tmp.path().join(".git").exists());
    }
}
