// Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `pidash context …` subcommands.

use std::fs;
use std::path::{Path, PathBuf};

use clap::{Args, Subcommand};
use serde::Serialize;

use crate::api_client::{ApiClient, CliEnv, CliError, EXIT_INVALID, EXIT_UNKNOWN, report_error};

use super::project::{ProjectRow, resolve_project};

#[derive(Debug, Args)]
pub struct ContextArgs {
    #[command(subcommand)]
    pub command: ContextCommand,
}

#[derive(Debug, Subcommand)]
pub enum ContextCommand {
    /// Write .pidash/context.md for this workspace.
    Init(InitArgs),
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

#[derive(Debug, Clone, Serialize)]
struct ContextProject {
    id: String,
    identifier: String,
    name: String,
    description: String,
    is_default: bool,
}

#[derive(Debug, Clone, Default)]
struct ContextDoc {
    workspace_slug: String,
    default_project_id: String,
    projects: Vec<ContextProject>,
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
    };
    match result {
        Ok(()) => 0,
        Err(e) => report_error(&e),
    }
}

pub async fn write_context_for_project(
    paths: &crate::util::paths::Paths,
    workspace_dir: &Path,
    project_ref: &str,
) -> Result<PathBuf, CliError> {
    let env = CliEnv::resolve(paths)?;
    let client = ApiClient::new(env).map_err(|e| CliError::new(EXIT_UNKNOWN, format!("{e}")))?;
    let project = resolve_project(&client, project_ref).await?;
    write_context_file(workspace_dir, &client.env.workspace_slug, &project)
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
    let path = write_context_file(&workspace, &client.env.workspace_slug, &project)?;
    println!("{}", serde_json::json!({"path": path, "project": project}));
    Ok(())
}

fn write_context_file(
    workspace_dir: &Path,
    workspace_slug: &str,
    project: &ProjectRow,
) -> Result<PathBuf, CliError> {
    let pidash_dir = workspace_dir.join(".pidash");
    fs::create_dir_all(&pidash_dir)
        .map_err(|e| CliError::new(EXIT_UNKNOWN, format!("creating {:?}: {e}", pidash_dir)))?;
    let path = pidash_dir.join("context.md");
    let mut doc = fs::read_to_string(&path)
        .ok()
        .and_then(|body| parse_context(&body))
        .unwrap_or_default();
    doc.workspace_slug = workspace_slug.to_string();
    doc.default_project_id = project.id.clone();
    upsert_project(&mut doc.projects, ContextProject::from(project));
    let body = render_context(&doc);
    fs::write(&path, body)
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
            is_default: project.is_default,
        }
    }
}

fn upsert_project(projects: &mut Vec<ContextProject>, project: ContextProject) {
    if let Some(existing) = projects.iter_mut().find(|p| p.id == project.id) {
        *existing = project;
    } else {
        projects.push(project);
    }
}

fn render_context(doc: &ContextDoc) -> String {
    let mut out = format!(
        "---\nworkspace_slug: {}\ndefault_project_id: {}\nprojects:\n",
        yaml_string(&doc.workspace_slug),
        yaml_string(&doc.default_project_id),
    );
    for project in &doc.projects {
        out.push_str(&format!(
            "  - id: {}\n    identifier: {}\n    name: {}\n    description: {}\n    is_default: {}\n",
            yaml_string(&project.id),
            yaml_string(&project.identifier),
            yaml_string(&project.name),
            yaml_string(&project.description),
            project.is_default,
        ));
    }
    out.push_str(
        "---\n\n# Pi Dash Workspace Context\n\nThis workspace is linked to the Pi Dash projects above.\n",
    );
    out
}

fn parse_context(body: &str) -> Option<ContextDoc> {
    let mut lines = body.lines();
    if lines.next()? != "---" {
        return None;
    }

    let mut doc = ContextDoc::default();
    let mut current: Option<ContextProject> = None;

    for line in lines {
        if line == "---" {
            break;
        }
        let trimmed = line.trim_start();
        if let Some(raw) = trimmed.strip_prefix("workspace_slug:") {
            doc.workspace_slug = parse_scalar(raw);
        } else if let Some(raw) = trimmed.strip_prefix("default_project_id:") {
            doc.default_project_id = parse_scalar(raw);
        } else if let Some(raw) = trimmed.strip_prefix("- id:") {
            if let Some(project) = current.take() {
                doc.projects.push(project);
            }
            current = Some(ContextProject {
                id: parse_scalar(raw),
                identifier: String::new(),
                name: String::new(),
                description: String::new(),
                is_default: false,
            });
        } else if let Some(project) = current.as_mut() {
            if let Some(raw) = trimmed.strip_prefix("identifier:") {
                project.identifier = parse_scalar(raw);
            } else if let Some(raw) = trimmed.strip_prefix("name:") {
                project.name = parse_scalar(raw);
            } else if let Some(raw) = trimmed.strip_prefix("description:") {
                project.description = parse_scalar(raw);
            } else if let Some(raw) = trimmed.strip_prefix("is_default:") {
                project.is_default = parse_scalar(raw).eq_ignore_ascii_case("true");
            }
        }
    }
    if let Some(project) = current {
        doc.projects.push(project);
    }
    Some(doc)
}

fn parse_scalar(value: &str) -> String {
    let value = value.trim();
    if !(value.starts_with('"') && value.ends_with('"') && value.len() >= 2) {
        return value.to_string();
    }

    let mut out = String::new();
    let mut escaped = false;
    for ch in value[1..value.len() - 1].chars() {
        if escaped {
            out.push(ch);
            escaped = false;
        } else if ch == '\\' {
            escaped = true;
        } else {
            out.push(ch);
        }
    }
    out
}

fn yaml_string(value: &str) -> String {
    let escaped = value.replace('\\', "\\\\").replace('"', "\\\"");
    format!("\"{escaped}\"")
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
            is_default: false,
        }
    }

    #[test]
    fn render_parse_round_trips_multiple_projects() {
        let doc = ContextDoc {
            workspace_slug: "acme".to_string(),
            default_project_id: "p2".to_string(),
            projects: vec![project("p1", "WEB", "Website"), project("p2", "API", "API")],
        };

        let parsed = parse_context(&render_context(&doc)).expect("context parses");

        assert_eq!(parsed.workspace_slug, "acme");
        assert_eq!(parsed.default_project_id, "p2");
        assert_eq!(parsed.projects.len(), 2);
        assert_eq!(parsed.projects[0].identifier, "WEB");
        assert_eq!(parsed.projects[1].identifier, "API");
    }

    #[test]
    fn upsert_project_replaces_existing_project_without_dropping_others() {
        let mut projects = vec![project("p1", "WEB", "Website"), project("p2", "API", "API")];

        upsert_project(&mut projects, project("p1", "APP", "App"));

        assert_eq!(projects.len(), 2);
        assert_eq!(projects[0].identifier, "APP");
        assert_eq!(projects[1].identifier, "API");
    }
}
