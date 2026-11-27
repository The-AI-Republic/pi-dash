// Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `pidash project …` subcommands.

use clap::{Args, Subcommand};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::api_client::{
    ApiClient, CliEnv, CliError, EXIT_INVALID, EXIT_NOT_FOUND, EXIT_UNKNOWN, report_error,
};

use super::issue::{resolve_project_ref, runners_list_path};
use super::resolve::{looks_like_uuid, resolve_issue};

#[derive(Debug, Args)]
pub struct ProjectArgs {
    #[command(subcommand)]
    pub command: ProjectCommand,
}

#[derive(Debug, Subcommand)]
pub enum ProjectCommand {
    /// List projects in the active workspace. Prints JSON.
    List,
    /// List a project's runners from the cloud. Prints JSON. With
    /// `--assignable-for <issue-id|new>`, each row carries the
    /// per-issue eligibility read: `assignable`, `not_assignable_code`,
    /// `is_busy` and `queue_depth` (dedicated-runner design §7.4).
    Runners(RunnersArgs),
}

#[derive(Debug, Args)]
pub struct RunnersArgs {
    /// Project identifier (slug like `ENG`) or project UUID. Defaults the
    /// same way as `issue create --project`.
    #[arg(long)]
    pub project: Option<String>,

    /// Evaluate assignability for this issue (UUID or `PROJ-123`
    /// identifier) or for a new issue (`new`). Without it, rows carry only
    /// the plain runner fields; with it, each row also carries `assignable`,
    /// `not_assignable_code`, `is_busy` and `queue_depth`.
    #[arg(long = "assignable-for")]
    pub assignable_for: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectRow {
    pub id: String,
    pub identifier: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    /// Bound git remote for the project, if any. Lets an agent match a local
    /// checkout to a project. Empty when the project has no repository bound.
    #[serde(default)]
    pub repo_url: String,
    #[serde(default)]
    pub is_default: bool,
}

pub async fn run(args: ProjectArgs, paths: &crate::util::paths::Paths) -> i32 {
    let env = match CliEnv::resolve(paths) {
        Ok(e) => e,
        Err(e) => return report_error(&e),
    };
    let client = match ApiClient::new(env) {
        Ok(c) => c,
        Err(e) => return report_error(&CliError::new(EXIT_UNKNOWN, format!("{e}"))),
    };

    let result = match args.command {
        ProjectCommand::List => cmd_list(&client).await,
        ProjectCommand::Runners(a) => cmd_runners(&client, paths, a).await,
    };
    match result {
        Ok(()) => 0,
        Err(e) => report_error(&e),
    }
}

pub async fn list_projects(client: &ApiClient) -> Result<Vec<ProjectRow>, CliError> {
    let path = format!("workspaces/{}/projects/", client.env.workspace_slug);
    let resp = client.get(&path).await?;
    project_rows_from_value(resp)
}

pub async fn resolve_project(
    client: &ApiClient,
    project_ref: &str,
) -> Result<ProjectRow, CliError> {
    let needle = project_ref.trim();
    let projects = list_projects(client).await?;
    projects
        .into_iter()
        .find(|p| project_matches(p, needle))
        .ok_or_else(|| CliError::new(EXIT_NOT_FOUND, format!("project {needle:?} not found")))
}

/// Whether `needle` names `project`: its identifier (case-insensitive) or
/// its UUID in any spelling the server accepts (any case, with or without
/// hyphens), compared as parsed UUIDs rather than strings.
fn project_matches(project: &ProjectRow, needle: &str) -> bool {
    if project.identifier.to_uppercase() == needle.to_uppercase() || project.id == needle {
        return true;
    }
    match (
        uuid::Uuid::parse_str(&project.id),
        uuid::Uuid::parse_str(needle),
    ) {
        (Ok(id), Ok(wanted)) => id == wanted,
        _ => false,
    }
}

pub async fn default_project(client: &ApiClient) -> Result<Option<ProjectRow>, CliError> {
    Ok(list_projects(client)
        .await?
        .into_iter()
        .find(|p| p.is_default))
}

async fn cmd_runners(
    client: &ApiClient,
    paths: &crate::util::paths::Paths,
    args: RunnersArgs,
) -> Result<(), CliError> {
    let project_ref = resolve_project_ref(client, paths, args.project.as_deref()).await?;
    // `--assignable-for` accepts an issue identifier (`ENG-42`) as well as
    // a UUID or `new`; resolve identifiers to UUIDs the same way `issue`
    // commands do, since the server expects an issue id.
    let assignable_for = match args.assignable_for.as_deref() {
        Some(value) => {
            let value = value.trim();
            if value.is_empty() {
                return Err(CliError::new(
                    EXIT_INVALID,
                    "--assignable-for must not be empty",
                ));
            }
            if value == "new" || looks_like_uuid(value) {
                Some(value.to_string())
            } else {
                Some(resolve_issue(client, value).await?.id)
            }
        }
        None => None,
    };
    let resp = list_runners(client, &project_ref, assignable_for.as_deref()).await?;
    println!(
        "{}",
        serde_json::to_string(&resp).expect("serialize JSON value")
    );
    Ok(())
}

/// `GET workspaces/{slug}/runners/?project=<id>[&assignable_for=...]` —
/// the token-accessible v1 runners list. Shared with the `project runners`
/// subcommand and the issue `--runner` name resolution.
pub async fn list_runners(
    client: &ApiClient,
    project_id: &str,
    assignable_for: Option<&str>,
) -> Result<Value, CliError> {
    let path = runners_list_path(&client.env.workspace_slug, project_id, assignable_for);
    client.get(&path).await
}

async fn cmd_list(client: &ApiClient) -> Result<(), CliError> {
    let projects = list_projects(client).await?;
    println!(
        "{}",
        serde_json::to_string(&projects).expect("serialize JSON value")
    );
    Ok(())
}

fn project_rows_from_value(resp: Value) -> Result<Vec<ProjectRow>, CliError> {
    if let Some(results) = resp.get("results") {
        return serde_json::from_value(results.clone())
            .map_err(|e| CliError::new(EXIT_UNKNOWN, format!("invalid project list JSON: {e}")));
    }
    serde_json::from_value(resp)
        .map_err(|e| CliError::new(EXIT_UNKNOWN, format!("invalid project list JSON: {e}")))
}

#[cfg(test)]
mod project_match_tests {
    use super::*;

    fn row(id: &str, identifier: &str) -> ProjectRow {
        serde_json::from_value(serde_json::json!({
            "id": id, "identifier": identifier, "name": "n"
        }))
        .unwrap()
    }

    #[test]
    fn project_matches_identifier_case_insensitively() {
        let p = row("00000000-0000-0000-0000-0000000000aa", "ENG");
        assert!(project_matches(&p, "eng"));
        assert!(project_matches(&p, "ENG"));
        assert!(!project_matches(&p, "OPS"));
    }

    #[test]
    fn project_matches_unicode_identifier_case_insensitively() {
        let p = row("00000000-0000-0000-0000-0000000000aa", "ÉQUIPE");
        assert!(project_matches(&p, "équipe"));
        assert!(project_matches(&p, "Équipe"));
    }

    #[test]
    fn project_matches_any_uuid_spelling() {
        let p = row("0000000a-0000-0000-0000-0000000000aa", "ENG");
        assert!(project_matches(&p, "0000000a-0000-0000-0000-0000000000aa"));
        assert!(project_matches(&p, "0000000A-0000-0000-0000-0000000000AA"));
        let unhyphenated = "0000000A-0000-0000-0000-0000000000AA".replace('-', "");
        assert!(project_matches(&p, &unhyphenated));
        assert!(!project_matches(&p, "not-a-uuid"));
        assert!(!project_matches(&p, "0000000b-0000-0000-0000-0000000000aa"));
    }
}
