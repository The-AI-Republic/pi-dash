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

use super::issue::{build_query_string, resolve_project_ref, runners_list_path};
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
    /// For the runners configured on this machine, use `pidash runner list`.
    Runners(RunnersArgs),
}

#[derive(Debug, Args)]
pub struct RunnersArgs {
    /// Project identifier (slug like `ENG`) or project UUID. Defaults the
    /// same way as `issue create --project`.
    #[arg(long)]
    pub project: Option<String>,

    /// Project identifier (slug like `ENG`) or project UUID as a
    /// positional (`pidash project runners PDASHOSS01`).
    /// Conflicts with `--project`.
    #[arg(value_name = "PROJECT", conflicts_with = "project")]
    pub project_positional: Option<String>,

    /// Narrow to a single pod (UUID).
    #[arg(long)]
    pub pod: Option<String>,

    /// Include desktop-bundled runners, which are hidden by default.
    #[arg(long)]
    pub include_bundled: bool,

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
    let needle_upper = needle.to_uppercase();
    let projects = list_projects(client).await?;
    projects
        .into_iter()
        .find(|p| p.id == needle || p.identifier.to_uppercase() == needle_upper)
        .ok_or_else(|| CliError::new(EXIT_NOT_FOUND, format!("project {needle:?} not found")))
}

pub async fn default_project(client: &ApiClient) -> Result<Option<ProjectRow>, CliError> {
    Ok(list_projects(client)
        .await?
        .into_iter()
        .find(|p| p.is_default))
}

/// `GET workspaces/<slug>/projects/<uuid>/runners/` — the cloud's view of a
/// project's runners, sharing visibility rules with the web AI Workers panel.
/// The project reference is resolved to a UUID client-side (same three-way
/// id/identifier match as the other project subcommands), so an unknown
/// project fails with `EXIT_NOT_FOUND` before any runners call.
pub async fn list_project_runners(
    client: &ApiClient,
    args: &RunnersArgs,
    project_id: &str,
) -> Result<Value, CliError> {
    let mut params: Vec<(&str, String)> = Vec::new();
    if let Some(pod) = &args.pod {
        params.push(("pod", pod.clone()));
    }
    if args.include_bundled {
        params.push(("include_bundled", "true".to_string()));
    }
    let path = format!(
        "workspaces/{}/projects/{}/runners/{}",
        client.env.workspace_slug,
        project_id,
        build_query_string(&params),
    );
    client.get(&path).await
}

async fn cmd_runners(
    client: &ApiClient,
    paths: &crate::util::paths::Paths,
    args: RunnersArgs,
) -> Result<(), CliError> {
    // Resolve project: explicit --project or positional, else the
    // config/default chain in resolve_project_ref.
    let project_ref = resolve_project_ref(
        client,
        paths,
        args.project.as_deref().or(args.project_positional.as_deref()),
    )
    .await?;

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

    // If assignable_for is requested, use the generic v1 list endpoint which
    // supports per-row eligibility. Otherwise use the project-scoped endpoint
    // which supports pod/include_bundled.
    if let Some(af) = assignable_for {
        let resp = list_runners(client, &project_ref, Some(&af)).await?;
        println!(
            "{}",
            serde_json::to_string(&resp).expect("serialize JSON value")
        );
        return Ok(());
    }

    // Default: project-scoped endpoint with pod/include_bundled.
    let runners = list_project_runners(client, &args, &project_ref).await?;
    println!(
        "{}",
        serde_json::to_string(&runners).expect("serialize JSON value")
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
mod test {
    use super::*;
    #[test]
    fn runners_args_parses() {
        // Just ensure the struct can be constructed; CLI parsing is tested via integration.
        let args = RunnersArgs {
            project: Some("PDASHOSS01".to_string()),
            project_positional: None,
            pod: None,
            include_bundled: false,
            assignable_for: None,
        };
        assert_eq!(args.project.as_deref(), Some("PDASHOSS01"));
    }

    /// Parse `pidash project runners` args in isolation. `RunnersArgs`
    /// derives `Args`, not `Parser`, so wrap it in a throwaway `Parser` to
    /// exercise the positional / `--project` relationship the way clap
    /// enforces it at runtime (PDASHOSS01-245).
    #[derive(Debug, clap::Parser)]
    struct RunnersArgsHarness {
        #[command(flatten)]
        args: RunnersArgs,
    }

    #[test]
    fn runners_positional_project_parses() {
        use clap::Parser;
        let parsed = RunnersArgsHarness::try_parse_from(["runners", "PDASHOSS01"]).unwrap();
        assert_eq!(parsed.args.project_positional.as_deref(), Some("PDASHOSS01"));
        assert_eq!(parsed.args.project, None);
    }

    #[test]
    fn runners_positional_and_project_flag_conflict() {
        use clap::Parser;
        let err = RunnersArgsHarness::try_parse_from(["runners", "PDASHOSS01", "--project", "ENG"])
            .expect_err("positional project and --project are mutually exclusive");
        assert_eq!(err.kind(), clap::error::ErrorKind::ArgumentConflict);
    }
}
