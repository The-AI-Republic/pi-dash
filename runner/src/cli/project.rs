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
    /// Active projects only by default; archived projects are excluded.
    /// Pass `--all` to include archived projects too.
    List(ListArgs),
    /// Get one project by identifier (slug like `ENG`) or project UUID.
    /// Works for active and archived projects. Prints JSON.
    Get(GetArgs),
    /// List a project's runners from the cloud. Prints JSON. With
    /// `--assignable-for <issue-id|new>`, each row carries the
    /// per-issue eligibility read: `assignable`, `not_assignable_code`,
    /// `is_busy` and `queue_depth` (dedicated-runner design §7.4).
    Runners(RunnersArgs),
}

#[derive(Debug, Args)]
pub struct ListArgs {
    /// Include archived projects. Without this flag archived projects are
    /// excluded entirely; only active projects are listed.
    #[arg(long)]
    pub all: bool,
}

#[derive(Debug, Args)]
pub struct GetArgs {
    /// Project identifier (slug like `ENG`) or project UUID.
    pub project: String,
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
        ProjectCommand::List(args) => cmd_list(&client, args).await,
        ProjectCommand::Get(args) => cmd_get(&client, &args.project).await,
        ProjectCommand::Runners(a) => cmd_runners(&client, paths, a).await,
    };
    match result {
        Ok(()) => 0,
        Err(e) => report_error(&e),
    }
}

/// Every visible project (active and archived). Callers that resolve a
/// project by identifier (`resolve_project`, `default_project`) and the
/// machine-context writer (`context.rs`) need the unfiltered set so an
/// archived identifier still resolves; `cmd_list` applies the active-only
/// default itself (the list endpoint has no archive filter).
pub async fn list_projects(client: &ApiClient) -> Result<Vec<ProjectRow>, CliError> {
    Ok(fetch_project_views(client)
        .await?
        .into_iter()
        .map(|view| view.row)
        .collect())
}

async fn fetch_project_views(client: &ApiClient) -> Result<Vec<ProjectView>, CliError> {
    let path = format!("workspaces/{}/projects/", client.env.workspace_slug);
    let resp = client.get(&path).await?;
    project_views_from_value(resp)
}

/// Read one project by identifier or UUID through `GET projects/{ref}/`.
/// The server resolves `project_ref` exactly as it does for the work-item
/// routes (identifier in any case, UUID in any spelling), independent of
/// list paging and of archived filtering. An unknown project is a 404
/// (exit 4).
pub async fn get_project(client: &ApiClient, project_ref: &str) -> Result<ProjectRow, CliError> {
    let path = format!(
        "workspaces/{}/projects/{}/",
        client.env.workspace_slug,
        project_ref.trim()
    );
    let resp = client.get(&path).await?;
    serde_json::from_value(resp)
        .map_err(|e| CliError::new(EXIT_UNKNOWN, format!("invalid project JSON: {e}")))
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

async fn cmd_list(client: &ApiClient, args: ListArgs) -> Result<(), CliError> {
    let mut views = fetch_project_views(client).await?;
    if !args.all {
        views.retain(ProjectView::is_active);
    }
    print_views(&views);
    Ok(())
}

async fn cmd_get(client: &ApiClient, project: &str) -> Result<(), CliError> {
    let path = format!(
        "workspaces/{}/projects/{}/",
        client.env.workspace_slug, project
    );
    let resp = client.get(&path).await?;
    let view = project_view_from_value(&resp)?;
    println!(
        "{}",
        serde_json::to_string(&view.to_json()).expect("serialize JSON value")
    );
    Ok(())
}

fn print_views(views: &[ProjectView]) {
    let printed: Vec<Value> = views.iter().map(ProjectView::to_json).collect();
    println!(
        "{}",
        serde_json::to_string(&printed).expect("serialize JSON value")
    );
}

/// One project as parsed from the list (or detail) response, with the
/// archive state the CLI needs for `status` and the active-only default.
/// `ProjectRow` itself is unchanged — the machine-context code in
/// `context.rs` constructs it exhaustively — so the archive fields ride
/// along here instead.
#[derive(Debug, Clone)]
pub struct ProjectView {
    pub row: ProjectRow,
    pub archived_at: Option<String>,
}

impl ProjectView {
    /// `active` when `archived_at` is null, `archived` otherwise — the same
    /// derivation as the MCP `status` field (DIFF-46).
    pub fn status(&self) -> &'static str {
        if self.archived_at.is_some() {
            "archived"
        } else {
            "active"
        }
    }

    pub fn is_active(&self) -> bool {
        self.archived_at.is_none()
    }

    /// Printed JSON: every `ProjectRow` field plus `status` and `archived_at`
    /// (null when active).
    pub fn to_json(&self) -> Value {
        let mut value = serde_json::to_value(&self.row).expect("serialize project row");
        if let Some(map) = value.as_object_mut() {
            map.insert(
                "status".to_string(),
                Value::String(self.status().to_string()),
            );
            map.insert(
                "archived_at".to_string(),
                match &self.archived_at {
                    Some(ts) => Value::String(ts.clone()),
                    None => Value::Null,
                },
            );
        }
        value
    }
}

fn project_views_from_value(resp: Value) -> Result<Vec<ProjectView>, CliError> {
    let items: Vec<Value> = if let Some(results) = resp.get("results") {
        results.as_array().cloned().ok_or_else(|| {
            CliError::new(
                EXIT_UNKNOWN,
                "invalid project list JSON: 'results' is not a list",
            )
        })?
    } else if let Some(array) = resp.as_array() {
        array.clone()
    } else {
        return Err(CliError::new(
            EXIT_UNKNOWN,
            "invalid project list JSON: expected a list or an envelope with 'results'",
        ));
    };
    items.iter().map(project_view_from_value).collect()
}

fn project_view_from_value(value: &Value) -> Result<ProjectView, CliError> {
    let row: ProjectRow = serde_json::from_value(value.clone())
        .map_err(|e| CliError::new(EXIT_UNKNOWN, format!("invalid project list JSON: {e}")))?;
    let archived_at = value
        .get("archived_at")
        .and_then(Value::as_str)
        .map(str::to_string);
    Ok(ProjectView { row, archived_at })
}

#[cfg(test)]
mod tests {
    use super::*;

    const LIST_ENVELOPE: &str = r#"{"results":[
        {"id":"11111111-1111-1111-1111-111111111111","identifier":"ENG","name":"Engineering","description":"","repo_url":"","is_default":true,"archived_at":null},
        {"id":"22222222-2222-2222-2222-222222222222","identifier":"OLD","name":"Old Project","description":"","repo_url":"","is_default":false,"archived_at":"2026-11-13T18:57:37Z"}
    ]}"#;

    fn views() -> Vec<ProjectView> {
        let resp: Value = serde_json::from_str(LIST_ENVELOPE).unwrap();
        project_views_from_value(resp).expect("parse list envelope")
    }

    #[test]
    fn default_filter_keeps_only_the_active_project() {
        let mut views = views();
        views.retain(ProjectView::is_active);
        assert_eq!(views.len(), 1);
        assert_eq!(views[0].row.identifier, "ENG");
        assert_eq!(views[0].status(), "active");
    }

    #[test]
    fn all_keeps_active_and_archived_with_derived_status() {
        let views = views();
        assert_eq!(views.len(), 2);
        assert_eq!(views[0].status(), "active");
        assert!(views[0].is_active());
        assert_eq!(views[1].status(), "archived");
        assert!(!views[1].is_active());
        assert_eq!(
            views[1].archived_at.as_deref(),
            Some("2026-11-13T18:57:37Z")
        );
    }

    #[test]
    fn printed_json_carries_status_and_archived_at() {
        let views = views();
        let active = views[0].to_json();
        assert_eq!(active["status"], "active");
        assert_eq!(active["archived_at"], Value::Null);
        assert_eq!(active["identifier"], "ENG");
        let archived = views[1].to_json();
        assert_eq!(archived["status"], "archived");
        assert_eq!(archived["archived_at"], "2026-11-13T18:57:37Z");
        assert_eq!(archived["identifier"], "OLD");
    }

    #[test]
    fn detail_object_parses_the_same_way_as_a_list_row() {
        let active: Value = serde_json::from_str(
            r#"{"id":"11111111-1111-1111-1111-111111111111","identifier":"ENG","name":"Engineering","archived_at":null}"#,
        )
        .unwrap();
        let view = project_view_from_value(&active).expect("detail active");
        assert_eq!(view.status(), "active");
        assert_eq!(view.to_json()["archived_at"], Value::Null);

        let archived: Value = serde_json::from_str(
            r#"{"id":"22222222-2222-2222-2222-222222222222","identifier":"OLD","name":"Old Project","archived_at":"2026-11-13T18:57:37Z"}"#,
        )
        .unwrap();
        let view = project_view_from_value(&archived).expect("detail archived");
        assert_eq!(view.status(), "archived");
        assert_eq!(view.to_json()["archived_at"], "2026-11-13T18:57:37Z");
    }

    #[test]
    fn envelope_without_results_or_array_is_invalid_json() {
        let resp: Value = serde_json::from_str(r#"{"count":0}"#).unwrap();
        assert!(project_views_from_value(resp).is_err());
    }

    #[test]
    fn non_object_row_is_invalid_json() {
        let resp: Value = serde_json::from_str(r#"{"results":["nope"]}"#).unwrap();
        assert!(project_views_from_value(resp).is_err());
    }
}
