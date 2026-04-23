// Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Identifier → UUID resolution for CLI subcommands.
//!
//! The REST API exposes the issue-by-identifier route as GET-only
//! (`/workspaces/<slug>/work-items/<PROJ>-<num>/`); every mutating route
//! requires a `project_id` in the URL. The CLI hides that by resolving
//! identifiers itself before issuing the actual mutation.
//!
//! See `.ai_design/make_e2e_ready/implementation-plan.md` §"URL resolution
//! sequence" for the full contract.

use serde_json::Value;

use crate::api_client::{ApiClient, CliError, EXIT_INVALID, EXIT_NOT_FOUND, EXIT_SERVER};

/// Issue resolved from a `<PROJ>-<num>` identifier.
#[derive(Debug, Clone)]
pub struct ResolvedIssue {
    pub id: String,
    pub project_id: String,
    /// Full JSON payload from the by-identifier GET. Callers reuse it for
    /// `pidash issue get` output without re-fetching.
    pub raw: Value,
}

pub async fn resolve_issue(client: &ApiClient, ident: &str) -> Result<ResolvedIssue, CliError> {
    let path = format!(
        "workspaces/{}/work-items/{}/",
        client.env.workspace_slug, ident
    );
    let body = client.get(&path).await?;
    let id = body
        .get("id")
        .and_then(Value::as_str)
        .ok_or_else(|| CliError::new(EXIT_SERVER, "response missing 'id'"))?
        .to_string();
    // The serializer emits `project` as the FK UUID; newer payloads may also
    // carry `project_id`. Accept either so we don't trip on schema drift.
    let project_id = body
        .get("project")
        .and_then(Value::as_str)
        .or_else(|| body.get("project_id").and_then(Value::as_str))
        .ok_or_else(|| CliError::new(EXIT_SERVER, "response missing 'project'"))?
        .to_string();
    Ok(ResolvedIssue {
        id,
        project_id,
        raw: body,
    })
}

/// Match a state name (case-insensitive) against the project's state list and
/// return the UUID. Errors if there are zero or multiple matches, with detail
/// listing what was found so the agent can correct.
pub async fn resolve_state_name(
    client: &ApiClient,
    project_id: &str,
    name: &str,
) -> Result<String, CliError> {
    let path = format!(
        "workspaces/{}/projects/{}/states/",
        client.env.workspace_slug, project_id
    );
    let body = client.get(&path).await?;
    let states = body
        .as_array()
        .ok_or_else(|| CliError::new(EXIT_SERVER, "states response is not a list"))?;
    let needle = name.trim().to_lowercase();
    let mut matches: Vec<(String, String)> = Vec::new();
    for s in states {
        let sname = s.get("name").and_then(Value::as_str).unwrap_or("");
        if sname.trim().to_lowercase() == needle
            && let Some(id) = s.get("id").and_then(Value::as_str)
        {
            matches.push((id.to_string(), sname.to_string()));
        }
    }
    match matches.len() {
        1 => Ok(matches.remove(0).0),
        0 => {
            let available: Vec<String> = states
                .iter()
                .filter_map(|s| s.get("name").and_then(Value::as_str).map(str::to_string))
                .collect();
            Err(
                CliError::new(EXIT_NOT_FOUND, format!("state '{name}' not found"))
                    .with_detail(format!("available: {}", available.join(", "))),
            )
        }
        n => Err(CliError::new(
            EXIT_INVALID,
            format!("state name '{name}' matched {n} states; disambiguate with the UUID"),
        )),
    }
}

/// Returns `true` if the caller-supplied value already looks like a UUID,
/// letting the CLI skip the resolve step for operators who paste UUIDs.
pub fn looks_like_uuid(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 36
        && b[8] == b'-'
        && b[13] == b'-'
        && b[18] == b'-'
        && b[23] == b'-'
        && s.chars().all(|c| c == '-' || c.is_ascii_hexdigit())
}
