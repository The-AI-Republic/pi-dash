// Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `pidash label …` subcommands.
//!
//! Labels are the project's free-form tags — `bug`, `frontend`,
//! `needs-design` — the dimension that states, cycles and modules don't
//! cover. These commands wrap the `/api/v1/` label endpoints so an agent can
//! read the project's vocabulary (`list`) and curate it (`create`, `update`,
//! `delete`); attaching a label to a work item lives on `pidash issue`
//! (`--label` / `--add-label` / `--remove-label`).
//!
//! Everywhere a label is named, the CLI accepts either its name
//! (case-insensitive) or its UUID and resolves names via
//! `resolve::resolve_label_refs`, mirroring how `--state` accepts a state
//! name. A project's labels are unique by name, so a name is an unambiguous
//! handle.
//!
//! JSON on stdout, JSON on stderr for errors, exit codes per
//! `api_client::EXIT_*` — same contract as `issue` and `page`.

use clap::{Args, Subcommand};
use serde_json::{Map, Value};

use crate::api_client::{ApiClient, CliEnv, CliError, EXIT_INVALID, EXIT_UNKNOWN, report_error};

use super::issue::{build_query_string, resolve_project_ref};
use super::resolve::{looks_like_uuid, resolve_label_refs};

#[derive(Debug, Args)]
pub struct LabelArgs {
    #[command(subcommand)]
    pub command: LabelCommand,
}

#[derive(Debug, Subcommand)]
pub enum LabelCommand {
    /// List a project's labels. Returns the server's paginated envelope
    /// (`{count, next_cursor, prev_cursor, results: [...]}`) — pass
    /// `--cursor` from a prior page to walk pages. Read this before adding a
    /// label to a work item, so you reuse the project's vocabulary instead of
    /// coining a near-duplicate.
    List(ListArgs),
    /// Create a label in a project. Names are unique per project; creating one
    /// that already exists is a 409.
    Create(CreateArgs),
    /// Rename a label or change its color, description, or parent. Only the
    /// fields you pass are sent. The edit applies everywhere the label is
    /// already attached.
    Update(UpdateArgs),
    /// Delete a label. This detaches it from every work item and page that
    /// carries it — there is no undo. Prefer renaming a label that is merely
    /// misspelled.
    Delete(DeleteArgs),
}

/// The project a label command operates on. Optional everywhere: it falls
/// back through `PIDASH_PROJECT_ID`, the local config default, and the
/// workspace default project, exactly like `pidash issue create --project`.
#[derive(Debug, Args)]
pub struct ProjectArg {
    /// Project identifier (slug like `ENG`) or project UUID. Defaults to the
    /// current run's project.
    #[arg(long)]
    pub project: Option<String>,
}

#[derive(Debug, Args)]
pub struct ListArgs {
    #[command(flatten)]
    pub project: ProjectArg,

    /// Pagination cursor returned from a prior page (the `next_cursor` field).
    #[arg(long)]
    pub cursor: Option<String>,

    /// Items per page. Server-side default applies if omitted.
    #[arg(long)]
    pub per_page: Option<u32>,
}

#[derive(Debug, Args)]
pub struct CreateArgs {
    #[command(flatten)]
    pub project: ProjectArg,

    /// Label name, e.g. `bug`. Unique within the project.
    #[arg(long)]
    pub name: String,

    /// Display color as a hex string, e.g. `#ff5630`.
    #[arg(long)]
    pub color: Option<String>,

    /// What the label means — worth filling in so later runs pick the right
    /// one instead of coining a synonym.
    #[arg(long)]
    pub description: Option<String>,

    /// Group this label under an existing one — its name or UUID. Label
    /// groups nest one level; they only tidy the settings list.
    #[arg(long)]
    pub parent: Option<String>,
}

#[derive(Debug, Args)]
pub struct UpdateArgs {
    /// The label to change — its name (case-insensitive) or UUID.
    pub label: String,

    #[command(flatten)]
    pub project: ProjectArg,

    /// New name.
    #[arg(long)]
    pub name: Option<String>,

    /// New display color as a hex string, e.g. `#ff5630`.
    #[arg(long)]
    pub color: Option<String>,

    /// New description. `--description ""` clears it.
    #[arg(long)]
    pub description: Option<String>,

    /// New parent label — its name or UUID. Mutually exclusive with
    /// `--clear-parent`.
    #[arg(long, conflicts_with = "clear_parent")]
    pub parent: Option<String>,

    /// Move the label out of its group (sends `parent: null`).
    #[arg(long)]
    pub clear_parent: bool,
}

#[derive(Debug, Args)]
pub struct DeleteArgs {
    /// The label to delete — its name (case-insensitive) or UUID.
    pub label: String,

    #[command(flatten)]
    pub project: ProjectArg,
}

pub async fn run(args: LabelArgs, paths: &crate::util::paths::Paths) -> i32 {
    let env = match CliEnv::resolve(paths) {
        Ok(e) => e,
        Err(e) => return report_error(&e),
    };
    let client = match ApiClient::new(env) {
        Ok(c) => c,
        Err(e) => return report_error(&CliError::new(EXIT_UNKNOWN, format!("{e}"))),
    };

    // Every label subcommand is project-scoped and defaults `--project` the
    // same way, so resolve it once here. That also keeps the `cmd_*` bodies
    // free of `Paths`, which is what lets the contract tests drive them
    // against a fake server.
    let explicit = match &args.command {
        LabelCommand::List(a) => a.project.project.clone(),
        LabelCommand::Create(a) => a.project.project.clone(),
        LabelCommand::Update(a) => a.project.project.clone(),
        LabelCommand::Delete(a) => a.project.project.clone(),
    };
    let project = match resolve_project_ref(&client, paths, explicit.as_deref()).await {
        Ok(p) => p,
        Err(e) => return report_error(&e),
    };

    let result = match args.command {
        LabelCommand::List(a) => cmd_list(&client, &project, a).await,
        LabelCommand::Create(a) => cmd_create(&client, &project, a).await,
        LabelCommand::Update(a) => cmd_update(&client, &project, a).await,
        LabelCommand::Delete(a) => cmd_delete(&client, &project, a).await,
    };
    match result {
        Ok(()) => 0,
        Err(e) => report_error(&e),
    }
}

pub async fn cmd_list(client: &ApiClient, project: &str, args: ListArgs) -> Result<(), CliError> {
    let mut params: Vec<(&str, String)> = Vec::new();
    if let Some(c) = args.cursor.as_ref() {
        params.push(("cursor", c.clone()));
    }
    if let Some(n) = args.per_page {
        params.push(("per_page", n.to_string()));
    }
    let query = build_query_string(&params);
    let resp = client
        .get(&format!("{}{query}", labels_path(client, project)))
        .await?;
    print_json(&resp);
    Ok(())
}

pub async fn cmd_create(
    client: &ApiClient,
    project: &str,
    args: CreateArgs,
) -> Result<(), CliError> {
    let name = validated_name(&args.name)?;
    // Resolve the network-dependent parent first, then hand already-resolved
    // values to the pure body builder so the field contract is unit-testable.
    let parent = match args.parent.as_deref() {
        Some(parent) => Some(resolve_one_label(client, project, parent).await?),
        None => None,
    };
    let body = build_create_body(
        name,
        args.color.as_deref(),
        args.description.as_deref(),
        parent,
    );
    let resp = client
        .post(&labels_path(client, project), &Value::Object(body))
        .await?;
    print_json(&resp);
    Ok(())
}

/// Assemble the label POST body from already-resolved values. Pure (no
/// network) so the flag → field mapping is unit-testable.
fn build_create_body(
    name: String,
    color: Option<&str>,
    description: Option<&str>,
    parent: Option<String>,
) -> Map<String, Value> {
    let mut body = Map::new();
    body.insert("name".into(), Value::String(name));
    if let Some(color) = color {
        body.insert("color".into(), Value::String(color.to_string()));
    }
    if let Some(description) = description {
        body.insert("description".into(), Value::String(description.to_string()));
    }
    if let Some(parent) = parent {
        body.insert("parent".into(), Value::String(parent));
    }
    body
}

pub async fn cmd_update(
    client: &ApiClient,
    project: &str,
    args: UpdateArgs,
) -> Result<(), CliError> {
    let name = args.name.as_deref().map(validated_name).transpose()?;
    let label_id = resolve_one_label(client, project, &args.label).await?;
    let parent = if args.clear_parent {
        ParentPatch::Clear
    } else if let Some(ref parent) = args.parent {
        ParentPatch::Set(resolve_one_label(client, project, parent).await?)
    } else {
        ParentPatch::Unchanged
    };
    let body = build_update_body(
        name,
        args.color.as_deref(),
        args.description.as_deref(),
        parent,
    )?;
    let resp = client
        .patch(
            &label_path(client, project, &label_id),
            &Value::Object(body),
        )
        .await?;
    print_json(&resp);
    Ok(())
}

/// The `parent`-field mutation requested by a `pidash label update`.
///
/// `clap`'s `conflicts_with` guarantees `--parent` and `--clear-parent` are
/// never both present, so these three variants are exhaustive.
enum ParentPatch {
    /// `--parent` given; the resolved parent label UUID.
    Set(String),
    /// `--clear-parent` given; ungroup by sending `parent: null`.
    Clear,
    /// Neither flag given; leave the parent untouched.
    Unchanged,
}

/// Assemble the label PATCH body, enforcing the "at least one mutation"
/// guard. Pure so the body contract — including `parent: null` for
/// `--clear-parent`, and the fact that a parent mutation alone satisfies the
/// guard — is unit-testable.
fn build_update_body(
    name: Option<String>,
    color: Option<&str>,
    description: Option<&str>,
    parent: ParentPatch,
) -> Result<Map<String, Value>, CliError> {
    let mut body = Map::new();
    if let Some(name) = name {
        body.insert("name".into(), Value::String(name));
    }
    if let Some(color) = color {
        body.insert("color".into(), Value::String(color.to_string()));
    }
    if let Some(description) = description {
        body.insert("description".into(), Value::String(description.to_string()));
    }
    match parent {
        ParentPatch::Set(uuid) => {
            body.insert("parent".into(), Value::String(uuid));
        }
        ParentPatch::Clear => {
            body.insert("parent".into(), Value::Null);
        }
        ParentPatch::Unchanged => {}
    }
    if body.is_empty() {
        return Err(CliError::new(
            EXIT_INVALID,
            "at least one of --name/--color/--description/--parent/--clear-parent is required",
        ));
    }
    Ok(body)
}

pub async fn cmd_delete(
    client: &ApiClient,
    project: &str,
    args: DeleteArgs,
) -> Result<(), CliError> {
    let label_id = resolve_one_label(client, project, &args.label).await?;
    let resp = client
        .delete(&label_path(client, project, &label_id))
        .await?;
    print_json(&delete_result(resp, &label_id));
    Ok(())
}

/// The document `pidash label delete` prints. The endpoint answers `204` with
/// no body, which would print a bare `null` and tell an agent nothing — so an
/// empty response becomes an explicit `{"deleted": true, "id": …}`. A server
/// that does return a body is passed through untouched.
fn delete_result(resp: Value, label_id: &str) -> Value {
    if !resp.is_null() {
        return resp;
    }
    let mut out = Map::new();
    out.insert("deleted".into(), Value::Bool(true));
    out.insert("id".into(), Value::String(label_id.to_string()));
    Value::Object(out)
}

/// Resolve a single label reference (name or UUID) to a UUID.
async fn resolve_one_label(
    client: &ApiClient,
    project: &str,
    label: &str,
) -> Result<String, CliError> {
    let trimmed = label.trim();
    if trimmed.is_empty() {
        return Err(CliError::new(EXIT_INVALID, "label must not be empty"));
    }
    if looks_like_uuid(trimmed) {
        return Ok(trimmed.to_string());
    }
    // A comma here means the caller passed a list where one label is wanted;
    // `resolve_label_refs` would happily return several and we'd silently use
    // the first.
    if trimmed.contains(',') {
        return Err(CliError::new(
            EXIT_INVALID,
            format!("expected one label, got a list: '{label}'"),
        ));
    }
    let mut ids = resolve_label_refs(client, project, trimmed).await?;
    Ok(ids.remove(0))
}

fn validated_name(name: &str) -> Result<String, CliError> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err(CliError::new(EXIT_INVALID, "--name must not be empty"));
    }
    Ok(trimmed.to_string())
}

fn labels_path(client: &ApiClient, project: &str) -> String {
    format!(
        "workspaces/{}/projects/{project}/labels/",
        client.env.workspace_slug
    )
}

fn label_path(client: &ApiClient, project: &str, label_id: &str) -> String {
    format!("{}{label_id}/", labels_path(client, project))
}

fn print_json(resp: &Value) {
    println!(
        "{}",
        serde_json::to_string(resp).expect("serialize JSON value")
    );
}

#[cfg(test)]
mod tests {
    use super::{ParentPatch, build_create_body, build_update_body, delete_result, validated_name};
    use crate::api_client::EXIT_INVALID;
    use serde_json::{Value, json};

    #[test]
    fn create_body_sends_only_the_flags_given() {
        let body = build_create_body("bug".into(), None, None, None);
        assert_eq!(Value::Object(body), json!({"name": "bug"}));
    }

    #[test]
    fn create_body_carries_color_description_and_parent() {
        let body = build_create_body(
            "ios".into(),
            Some("#ff5630"),
            Some("Apple client"),
            Some("parent-uuid".into()),
        );
        assert_eq!(
            Value::Object(body),
            json!({
                "name": "ios",
                "color": "#ff5630",
                "description": "Apple client",
                "parent": "parent-uuid",
            })
        );
    }

    #[test]
    fn update_body_requires_at_least_one_mutation() {
        let err =
            build_update_body(None, None, None, ParentPatch::Unchanged).expect_err("empty patch");
        assert_eq!(err.exit_code, EXIT_INVALID);
        assert!(err.message.contains("--name"), "got {}", err.message);
    }

    #[test]
    fn update_body_clears_the_parent_with_null() {
        let body = build_update_body(None, None, None, ParentPatch::Clear).expect("patch");
        assert_eq!(Value::Object(body), json!({"parent": Value::Null}));
    }

    #[test]
    fn update_body_allows_clearing_the_description() {
        // `--description ""` is a real mutation (it blanks the field), unlike
        // an omitted flag — so it must satisfy the guard.
        let body = build_update_body(None, None, Some(""), ParentPatch::Unchanged).expect("patch");
        assert_eq!(Value::Object(body), json!({"description": ""}));
    }

    #[test]
    fn delete_result_synthesizes_a_document_for_an_empty_204() {
        assert_eq!(
            delete_result(Value::Null, "l-1"),
            json!({"deleted": true, "id": "l-1"})
        );
    }

    #[test]
    fn delete_result_passes_a_server_body_through() {
        let body = json!({"message": "gone"});
        assert_eq!(delete_result(body.clone(), "l-1"), body);
    }

    #[test]
    fn validated_name_trims_and_rejects_blank() {
        assert_eq!(validated_name("  bug  ").unwrap(), "bug");
        let err = validated_name("   ").expect_err("blank name");
        assert_eq!(err.message, "--name must not be empty");
    }
}
