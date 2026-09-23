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

// Note: there is no `resolve_project` helper. Project-scoped REST routes
// accept either a UUID or the workspace-scoped slug ("ENG") in the URL path,
// so callers pass the user-supplied `--project` value straight through.

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
    // Current API returns the paginated envelope (`{count, results: [...]}`,
    // default page size 1000); older deployments return a bare list. Accept
    // both so the CLI works against either server.
    let states = body
        .get("results")
        .and_then(Value::as_array)
        .or_else(|| body.as_array())
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

/// One label of a project, as the label list endpoint returns it.
#[derive(Debug, Clone)]
pub struct ProjectLabel {
    pub id: String,
    pub name: String,
}

/// Fetch every label of a project, walking the paginated envelope so a project
/// with more labels than one page still resolves. Older deployments answer
/// with a bare list; both shapes are accepted, like `resolve_state_name`.
pub async fn fetch_project_labels(
    client: &ApiClient,
    project: &str,
) -> Result<Vec<ProjectLabel>, CliError> {
    let mut labels = Vec::new();
    let mut cursor: Option<String> = None;
    loop {
        let query = match cursor.as_deref() {
            Some(c) => format!("?cursor={c}"),
            None => String::new(),
        };
        let path = format!(
            "workspaces/{}/projects/{}/labels/{query}",
            client.env.workspace_slug, project
        );
        let body = client.get(&path).await?;
        let page = body
            .get("results")
            .and_then(Value::as_array)
            .or_else(|| body.as_array())
            .ok_or_else(|| CliError::new(EXIT_SERVER, "labels response is not a list"))?;
        for l in page {
            if let (Some(id), Some(name)) = (
                l.get("id").and_then(Value::as_str),
                l.get("name").and_then(Value::as_str),
            ) {
                labels.push(ProjectLabel {
                    id: id.to_string(),
                    name: name.to_string(),
                });
            }
        }
        // `next_cursor` is present but null/empty on the last page.
        match body.get("next_cursor").and_then(Value::as_str) {
            Some(next) if !next.is_empty() && Some(next) != cursor.as_deref() => {
                cursor = Some(next.to_string());
            }
            _ => return Ok(labels),
        }
    }
}

/// Match a label name (case-insensitive) against an already-fetched project
/// label list and return the UUID. Pure so the matching rules — and the
/// "available:" detail an agent needs to correct itself — are unit-testable.
pub fn match_label_name(labels: &[ProjectLabel], name: &str) -> Result<String, CliError> {
    let needle = name.trim().to_lowercase();
    let mut matches = labels
        .iter()
        .filter(|l| l.name.trim().to_lowercase() == needle);
    let Some(first) = matches.next() else {
        let available: Vec<&str> = labels.iter().map(|l| l.name.as_str()).collect();
        return Err(
            CliError::new(EXIT_NOT_FOUND, format!("label '{name}' not found")).with_detail(
                if available.is_empty() {
                    "the project has no labels yet; create one with `pidash label create`"
                        .to_string()
                } else {
                    format!("available: {}", available.join(", "))
                },
            ),
        );
    };
    // The DB holds a unique (project, name) constraint, so a second match means
    // the server is in a state the CLI should not silently pick a winner from.
    if matches.next().is_some() {
        return Err(CliError::new(
            EXIT_INVALID,
            format!("label name '{name}' matched more than one label; disambiguate with the UUID"),
        ));
    }
    Ok(first.id.clone())
}

/// Resolve a comma-separated `--label`-style value into label UUIDs. Entries
/// that already look like UUIDs pass through untouched; names are matched
/// against the project's labels with a **single** list fetch, so
/// `--label bug,frontend,api` costs one request rather than three. Duplicates
/// collapse while keeping first-seen order, since the server replaces the
/// whole set and a repeated id buys nothing.
pub async fn resolve_label_refs(
    client: &ApiClient,
    project: &str,
    refs: &str,
) -> Result<Vec<String>, CliError> {
    let mut groups = resolve_label_ref_groups(client, project, &[refs]).await?;
    Ok(groups.remove(0))
}

/// Resolve several ref-lists — `--add-label` and `--remove-label`, say —
/// against **one** label list fetch, returning one UUID list per input in the
/// same order. An empty input yields an empty list without being rejected, so
/// callers can pass a flag that wasn't given.
pub async fn resolve_label_ref_groups(
    client: &ApiClient,
    project: &str,
    groups: &[&str],
) -> Result<Vec<Vec<String>>, CliError> {
    let entries: Vec<Vec<String>> = groups
        .iter()
        .map(|refs| {
            if refs.trim().is_empty() {
                Ok(Vec::new())
            } else {
                split_refs(refs, "label")
            }
        })
        .collect::<Result<_, _>>()?;

    // Only pay for the list fetch when at least one entry anywhere is a name.
    let labels = if entries
        .iter()
        .flatten()
        .any(|entry| !looks_like_uuid(entry))
    {
        fetch_project_labels(client, project).await?
    } else {
        Vec::new()
    };

    entries
        .into_iter()
        .map(|group| resolve_group(&labels, group))
        .collect()
}

/// Map one group's entries to UUIDs against an already-fetched label list,
/// collapsing duplicates while keeping first-seen order: the server replaces
/// the whole set, so a repeated id buys nothing.
fn resolve_group(labels: &[ProjectLabel], entries: Vec<String>) -> Result<Vec<String>, CliError> {
    let mut ids: Vec<String> = Vec::with_capacity(entries.len());
    for entry in entries {
        let id = if looks_like_uuid(&entry) {
            entry
        } else {
            match_label_name(labels, &entry)?
        };
        if !ids.contains(&id) {
            ids.push(id);
        }
    }
    Ok(ids)
}

/// Split a comma-separated list of references, rejecting empty entries. An
/// empty entry is nearly always a stray comma or an unexpanded shell variable,
/// and silently dropping it would mutate a different set than the caller meant.
pub fn split_refs(refs: &str, noun: &str) -> Result<Vec<String>, CliError> {
    if refs.trim().is_empty() {
        return Err(CliError::new(
            EXIT_INVALID,
            format!("{noun} list must not be empty"),
        ));
    }
    refs.split(',')
        .map(|entry| {
            let trimmed = entry.trim();
            if trimmed.is_empty() {
                Err(CliError::new(
                    EXIT_INVALID,
                    format!("{noun} list has an empty entry: '{refs}'"),
                ))
            } else {
                Ok(trimmed.to_string())
            }
        })
        .collect()
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

#[cfg(test)]
mod tests {
    use super::{ProjectLabel, match_label_name, split_refs};
    use crate::api_client::{EXIT_INVALID, EXIT_NOT_FOUND};

    fn labels() -> Vec<ProjectLabel> {
        [("l-bug", "bug"), ("l-fe", "Frontend")]
            .into_iter()
            .map(|(id, name)| ProjectLabel {
                id: id.to_string(),
                name: name.to_string(),
            })
            .collect()
    }

    #[test]
    fn match_label_name_is_case_insensitive() {
        assert_eq!(match_label_name(&labels(), "FRONTEND").unwrap(), "l-fe");
        assert_eq!(match_label_name(&labels(), " bug ").unwrap(), "l-bug");
    }

    #[test]
    fn match_label_name_lists_available_labels_when_missing() {
        let err = match_label_name(&labels(), "chore").expect_err("unknown label");
        assert_eq!(err.exit_code, EXIT_NOT_FOUND);
        assert_eq!(err.message, "label 'chore' not found");
        assert_eq!(err.detail.as_deref(), Some("available: bug, Frontend"));
    }

    #[test]
    fn match_label_name_points_at_create_when_the_project_has_none() {
        let err = match_label_name(&[], "bug").expect_err("no labels");
        assert!(
            err.detail
                .as_deref()
                .is_some_and(|d| d.contains("pidash label create")),
            "detail should point at label create, got {:?}",
            err.detail,
        );
    }

    #[test]
    fn match_label_name_refuses_to_guess_between_duplicates() {
        let dupes = vec![
            ProjectLabel {
                id: "a".into(),
                name: "bug".into(),
            },
            ProjectLabel {
                id: "b".into(),
                name: "BUG".into(),
            },
        ];
        let err = match_label_name(&dupes, "bug").expect_err("ambiguous");
        assert_eq!(err.exit_code, EXIT_INVALID);
        assert!(err.message.contains("disambiguate with the UUID"));
    }

    #[test]
    fn split_refs_trims_entries() {
        assert_eq!(
            split_refs(" bug , frontend ", "label").unwrap(),
            vec!["bug".to_string(), "frontend".to_string()],
        );
    }

    #[test]
    fn split_refs_rejects_empty_input_and_empty_entries() {
        let err = split_refs("   ", "label").expect_err("empty");
        assert_eq!(err.message, "label list must not be empty");
        let err = split_refs("bug,,frontend", "label").expect_err("empty entry");
        assert_eq!(err.exit_code, EXIT_INVALID);
        assert!(err.message.contains("empty entry"));
    }
}
