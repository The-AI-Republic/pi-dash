// Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `pidash page …` subcommands.
//!
//! Project pages are a project-scoped wiki: decisions, conventions, stage
//! plans. These commands wrap the `/api/v1/` page endpoints so an agent can
//! look up shared knowledge while it works (`list`, `get`) and record it
//! (`create`, `update`, `archive`, `unarchive`).
//!
//! JSON on stdout, JSON on stderr for errors, exit codes per
//! `api_client::EXIT_*` — same contract as `issue` and `workpad`.

use std::io::Read;
use std::path::{Path, PathBuf};

use clap::{Args, Subcommand, ValueEnum};
use serde_json::{Map, Value};

use crate::api_client::{ApiClient, CliEnv, CliError, EXIT_INVALID, EXIT_SERVER, EXIT_UNKNOWN, report_error};

use super::issue::build_query_string;
use super::resolve::looks_like_uuid;

#[derive(Debug, Args)]
pub struct PageArgs {
    #[command(subcommand)]
    pub command: PageCommand,
}

#[derive(Debug, Subcommand)]
pub enum PageCommand {
    /// List the pages of a project. Returns the server's paginated envelope
    /// (`{count, next_cursor, prev_cursor, results: [...]}`) carrying page
    /// metadata only — read a single page to get its body.
    List(ListArgs),
    /// Fetch one page, including `description_markdown`.
    Get(GetArgs),
    /// Create a page. Prints the created page.
    Create(CreateArgs),
    /// Change a page's title, body, parent, or access. Only the fields you
    /// pass are sent. Prints the updated page.
    Update(UpdateArgs),
    /// Archive a page. Prints the archived page.
    Archive(ArchiveArgs),
    /// Restore an archived page. Prints the restored page.
    Unarchive(ArchiveArgs),
}

/// Who can see a page. Sent to the server as the integer `access` field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum PageAccess {
    /// Visible to every project member.
    Public,
    /// Visible to the page owner only.
    Private,
}

impl PageAccess {
    fn as_api(self) -> u8 {
        match self {
            PageAccess::Public => 0,
            PageAccess::Private => 1,
        }
    }
}

/// The page body source: inline or from a file (`-` = stdin). Neither is
/// required; `update` omits the body when both are absent.
#[derive(Debug, Args)]
pub struct PageBodyArgs {
    /// Page body as markdown.
    #[arg(long, conflicts_with = "body_file")]
    pub body: Option<String>,

    /// Read the markdown body from a file; `-` reads it from stdin.
    #[arg(long = "body-file", value_name = "PATH")]
    pub body_file: Option<PathBuf>,
}

impl PageBodyArgs {
    fn is_set(&self) -> bool {
        self.body.is_some() || self.body_file.is_some()
    }
}

#[derive(Debug, Args)]
pub struct CreateArgs {
    /// Project identifier (slug like `ENG`) or project UUID.
    #[arg(long)]
    pub project: String,

    /// Page title.
    #[arg(long)]
    pub title: String,

    #[command(flatten)]
    pub body: PageBodyArgs,

    /// Parent page UUID, to nest the new page under it.
    #[arg(long)]
    pub parent: Option<String>,

    /// Page visibility. Server default (public) applies if omitted.
    #[arg(long, value_enum)]
    pub access: Option<PageAccess>,
}

#[derive(Debug, Args)]
pub struct UpdateArgs {
    /// Page UUID, as listed by `pidash page list`.
    pub page_id: String,

    /// Project identifier (slug like `ENG`) or project UUID.
    #[arg(long)]
    pub project: String,

    /// New page title.
    #[arg(long)]
    pub title: Option<String>,

    /// Replacement body. The whole body is overwritten.
    #[command(flatten)]
    pub body: PageBodyArgs,

    /// New parent page UUID. Mutually exclusive with `--clear-parent`.
    #[arg(long, conflicts_with = "clear_parent")]
    pub parent: Option<String>,

    /// Make the page top-level (sends `parent: null`).
    #[arg(long)]
    pub clear_parent: bool,

    /// New page visibility.
    #[arg(long, value_enum)]
    pub access: Option<PageAccess>,
}

#[derive(Debug, Args)]
pub struct ArchiveArgs {
    /// Page UUID, as listed by `pidash page list`.
    pub page_id: String,

    /// Project identifier (slug like `ENG`) or project UUID.
    #[arg(long)]
    pub project: String,
}

#[derive(Debug, Args)]
pub struct ListArgs {
    /// Project identifier (slug like `ENG`) or project UUID.
    #[arg(long)]
    pub project: String,

    /// Pagination cursor returned from a prior page (the `next_cursor` field).
    #[arg(long)]
    pub cursor: Option<String>,

    /// Items per page. Server-side default applies if omitted.
    #[arg(long)]
    pub per_page: Option<u32>,

    /// Also list archived pages. Excluded by default.
    #[arg(long)]
    pub include_archived: bool,
}

#[derive(Debug, Args)]
pub struct GetArgs {
    /// Page UUID, as listed by `pidash page list`.
    pub page_id: String,

    /// Project identifier (slug like `ENG`) or project UUID.
    #[arg(long)]
    pub project: String,

    /// Print only the page body as markdown, so it can be redirected to a
    /// file. Without it the whole JSON envelope is printed.
    #[arg(long)]
    pub body_only: bool,
}

pub async fn run(args: PageArgs, paths: &crate::util::paths::Paths) -> i32 {
    let env = match CliEnv::resolve(paths) {
        Ok(e) => e,
        Err(e) => return report_error(&e),
    };
    let client = match ApiClient::new(env) {
        Ok(c) => c,
        Err(e) => return report_error(&CliError::new(EXIT_UNKNOWN, format!("{e}"))),
    };

    let result = match args.command {
        PageCommand::List(args) => cmd_list(&client, args).await,
        PageCommand::Get(args) => cmd_get(&client, args).await,
        PageCommand::Create(args) => cmd_create(&client, args).await,
        PageCommand::Update(args) => cmd_update(&client, args).await,
        PageCommand::Archive(args) => cmd_archive(&client, args).await,
        PageCommand::Unarchive(args) => cmd_unarchive(&client, args).await,
    };
    match result {
        Ok(()) => 0,
        Err(e) => report_error(&e),
    }
}

pub async fn cmd_list(client: &ApiClient, args: ListArgs) -> Result<(), CliError> {
    let project_ref = validated_project(&args.project)?;
    let query = build_query_string(&list_params(&args));
    // Project-scoped REST routes accept either a UUID or the workspace-scoped
    // slug in the path, so `--project` goes through unresolved (see
    // `cli::resolve`).
    let path = format!(
        "workspaces/{}/projects/{project_ref}/pages/{query}",
        client.env.workspace_slug
    );
    let resp = client.get(&path).await?;
    println!(
        "{}",
        serde_json::to_string(&resp).expect("serialize JSON value")
    );
    Ok(())
}

pub async fn cmd_get(client: &ApiClient, args: GetArgs) -> Result<(), CliError> {
    let project_ref = validated_project(&args.project)?;
    let page_id = validated_page_id(&args.page_id)?;
    let path = format!(
        "workspaces/{}/projects/{project_ref}/pages/{page_id}/",
        client.env.workspace_slug
    );
    let resp = client.get(&path).await?;
    println!("{}", render_get(&resp, args.body_only)?);
    Ok(())
}

pub async fn cmd_create(client: &ApiClient, args: CreateArgs) -> Result<(), CliError> {
    let project_ref = validated_project(&args.project)?;
    let payload = create_payload(&args, std::io::stdin())?;
    let path = format!(
        "workspaces/{}/projects/{project_ref}/pages/",
        client.env.workspace_slug
    );
    let resp = client.post(&path, &Value::Object(payload)).await?;
    print_json(&resp);
    Ok(())
}

pub async fn cmd_update(client: &ApiClient, args: UpdateArgs) -> Result<(), CliError> {
    let project_ref = validated_project(&args.project)?;
    let page_id = validated_page_id(&args.page_id)?;
    let payload = update_payload(&args, std::io::stdin())?;
    let resp = client
        .patch(&page_path(client, project_ref, page_id), &Value::Object(payload))
        .await?;
    print_json(&resp);
    Ok(())
}

pub async fn cmd_archive(client: &ApiClient, args: ArchiveArgs) -> Result<(), CliError> {
    let project_ref = validated_project(&args.project)?;
    let page_id = validated_page_id(&args.page_id)?;
    let path = format!("{}archive/", page_path(client, project_ref, page_id));
    let resp = client.post(&path, &Value::Object(Map::new())).await?;
    print_json(&resp);
    Ok(())
}

pub async fn cmd_unarchive(client: &ApiClient, args: ArchiveArgs) -> Result<(), CliError> {
    let project_ref = validated_project(&args.project)?;
    let page_id = validated_page_id(&args.page_id)?;
    let path = format!("{}archive/", page_path(client, project_ref, page_id));
    let resp = client.delete(&path).await?;
    print_json(&resp);
    Ok(())
}

fn page_path(client: &ApiClient, project_ref: &str, page_id: &str) -> String {
    format!(
        "workspaces/{}/projects/{project_ref}/pages/{page_id}/",
        client.env.workspace_slug
    )
}

fn print_json(resp: &Value) {
    println!(
        "{}",
        serde_json::to_string(resp).expect("serialize JSON value")
    );
}

/// The POST body for `pidash page create`. `stdin` is only read for
/// `--body-file -`.
fn create_payload(args: &CreateArgs, stdin: impl Read) -> Result<Map<String, Value>, CliError> {
    let mut payload = Map::new();
    payload.insert("name".into(), Value::String(validated_title(&args.title)?));
    if let Some(parent) = args.parent.as_deref() {
        payload.insert("parent".into(), Value::String(validated_parent(parent)?));
    }
    if let Some(access) = args.access {
        payload.insert("access".into(), Value::from(access.as_api()));
    }
    if let Some(body) = load_body(&args.body, stdin)? {
        payload.insert("description_markdown".into(), Value::String(body));
    }
    Ok(payload)
}

/// The PATCH body for `pidash page update`: only the keys the caller asked
/// to change. Refuses an empty update so nothing is sent for a no-op.
fn update_payload(args: &UpdateArgs, stdin: impl Read) -> Result<Map<String, Value>, CliError> {
    if args.title.is_none()
        && !args.body.is_set()
        && args.parent.is_none()
        && !args.clear_parent
        && args.access.is_none()
    {
        return Err(CliError::new(
            EXIT_INVALID,
            "nothing to update: pass at least one of --title, --body, --body-file, \
             --parent, --clear-parent, --access",
        ));
    }
    let mut payload = Map::new();
    if let Some(title) = args.title.as_deref() {
        payload.insert("name".into(), Value::String(validated_title(title)?));
    }
    if args.clear_parent {
        payload.insert("parent".into(), Value::Null);
    } else if let Some(parent) = args.parent.as_deref() {
        payload.insert("parent".into(), Value::String(validated_parent(parent)?));
    }
    if let Some(access) = args.access {
        payload.insert("access".into(), Value::from(access.as_api()));
    }
    if let Some(body) = load_body(&args.body, stdin)? {
        payload.insert("description_markdown".into(), Value::String(body));
    }
    Ok(payload)
}

/// Resolve the body source. `--body-file -` reads `stdin`; `None` when no
/// body flag was given. An empty body is legitimate (it clears the page).
fn load_body(args: &PageBodyArgs, mut stdin: impl Read) -> Result<Option<String>, CliError> {
    match (&args.body, &args.body_file) {
        (Some(body), _) => Ok(Some(body.clone())),
        (None, Some(path)) if path == Path::new("-") => {
            let mut buf = String::new();
            stdin.read_to_string(&mut buf).map_err(|e| {
                CliError::new(EXIT_UNKNOWN, format!("failed reading page body from stdin: {e}"))
            })?;
            Ok(Some(buf))
        }
        (None, Some(path)) => std::fs::read_to_string(path).map(Some).map_err(|e| {
            CliError::new(
                EXIT_INVALID,
                format!("failed reading page body file {}: {e}", path.display()),
            )
        }),
        (None, None) => Ok(None),
    }
}

fn validated_title(title: &str) -> Result<String, CliError> {
    let trimmed = title.trim();
    if trimmed.is_empty() {
        return Err(CliError::new(EXIT_INVALID, "--title must not be empty"));
    }
    Ok(trimmed.to_string())
}

fn validated_parent(parent: &str) -> Result<String, CliError> {
    let trimmed = parent.trim();
    if !looks_like_uuid(trimmed) {
        return Err(CliError::new(
            EXIT_INVALID,
            format!("--parent must be a page UUID, got {trimmed:?}"),
        ));
    }
    Ok(trimmed.to_string())
}

/// What `pidash page get` writes to stdout: the JSON envelope by default, or
/// the bare markdown body under `--body-only` so an agent can redirect it
/// straight into a file.
fn render_get(resp: &Value, body_only: bool) -> Result<String, CliError> {
    if body_only {
        return Ok(markdown_body(resp)?.to_string());
    }
    Ok(serde_json::to_string(resp).expect("serialize JSON value"))
}

/// Collect the list query parameters in a stable order so the resulting
/// query string is predictable (and assertable in tests) without a
/// network round-trip.
fn list_params(args: &ListArgs) -> Vec<(&'static str, String)> {
    let mut params: Vec<(&'static str, String)> = Vec::new();
    if let Some(c) = args.cursor.as_ref() {
        params.push(("cursor", c.clone()));
    }
    if let Some(n) = args.per_page {
        params.push(("per_page", n.to_string()));
    }
    if args.include_archived {
        params.push(("include_archived", "true".to_string()));
    }
    params
}

fn validated_project(project: &str) -> Result<&str, CliError> {
    let trimmed = project.trim();
    if trimmed.is_empty() {
        return Err(CliError::new(EXIT_INVALID, "--project must not be empty"));
    }
    Ok(trimmed)
}

/// The page route binds `<uuid:page_id>`, so a non-UUID would 404 with a
/// routing error rather than a useful message. Reject it locally instead.
fn validated_page_id(page_id: &str) -> Result<&str, CliError> {
    let trimmed = page_id.trim();
    if !looks_like_uuid(trimmed) {
        return Err(CliError::new(
            EXIT_INVALID,
            format!("page id must be a UUID, got {trimmed:?}"),
        ));
    }
    Ok(trimmed)
}

/// Pull `description_markdown` out of a page payload for `--body-only`.
///
/// A page with an empty body is legitimate and prints nothing; a payload
/// missing the field entirely means the server is older than this CLI, which
/// is worth an error rather than silent emptiness.
fn markdown_body(resp: &Value) -> Result<&str, CliError> {
    resp.get("description_markdown")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            CliError::new(
                EXIT_SERVER,
                "response missing 'description_markdown'; the Pi Dash server may predate page markdown",
            )
        })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{
        CreateArgs, GetArgs, ListArgs, PageAccess, PageBodyArgs, UpdateArgs, create_payload,
        list_params, load_body, markdown_body, render_get, update_payload, validated_page_id,
        validated_project,
    };
    use crate::api_client::{EXIT_INVALID, EXIT_SERVER};
    use crate::cli::issue::build_query_string;

    const PAGE_UUID: &str = "550e8400-e29b-41d4-a716-446655440000";

    fn list_args() -> ListArgs {
        ListArgs {
            project: "ENG".to_string(),
            cursor: None,
            per_page: None,
            include_archived: false,
        }
    }

    fn get_args() -> GetArgs {
        GetArgs {
            page_id: PAGE_UUID.to_string(),
            project: "ENG".to_string(),
            body_only: false,
        }
    }

    #[test]
    fn list_without_options_sends_no_query_string() {
        assert_eq!(build_query_string(&list_params(&list_args())), "");
    }

    #[test]
    fn list_include_archived_sets_the_flag() {
        let args = ListArgs {
            include_archived: true,
            ..list_args()
        };

        assert_eq!(
            build_query_string(&list_params(&args)),
            "?include_archived=true"
        );
    }

    #[test]
    fn list_default_omits_include_archived_so_the_server_default_applies() {
        let params = list_params(&list_args());

        assert!(!params.iter().any(|(k, _)| *k == "include_archived"));
    }

    #[test]
    fn list_pagination_params_are_encoded() {
        let args = ListArgs {
            cursor: Some("20:1:0".to_string()),
            per_page: Some(50),
            include_archived: true,
            ..list_args()
        };

        assert_eq!(
            build_query_string(&list_params(&args)),
            "?cursor=20%3A1%3A0&per_page=50&include_archived=true"
        );
    }

    #[test]
    fn empty_project_is_rejected() {
        let err = validated_project("   ").expect_err("empty project must be invalid");

        assert_eq!(err.exit_code, EXIT_INVALID);
    }

    #[test]
    fn project_is_passed_through_untouched() {
        assert_eq!(validated_project(" ENG ").expect("project"), "ENG");
    }

    #[test]
    fn page_id_must_be_a_uuid() {
        let err = validated_page_id("release-checklist").expect_err("slug must be invalid");

        assert_eq!(err.exit_code, EXIT_INVALID);
        assert!(err.message.contains("must be a UUID"));
    }

    #[test]
    fn page_id_accepts_a_uuid() {
        assert_eq!(validated_page_id(PAGE_UUID).expect("page id"), PAGE_UUID);
    }

    #[test]
    fn body_only_extracts_the_markdown() {
        let resp = json!({"id": PAGE_UUID, "description_markdown": "# Title\n\nbody"});

        assert_eq!(markdown_body(&resp).expect("body"), "# Title\n\nbody");
    }

    #[test]
    fn body_only_accepts_an_empty_body() {
        let resp = json!({"id": PAGE_UUID, "description_markdown": ""});

        assert_eq!(markdown_body(&resp).expect("body"), "");
    }

    #[test]
    fn body_only_errors_when_the_server_omits_the_field() {
        let resp = json!({"id": PAGE_UUID, "description_html": "<p>x</p>"});

        let err = markdown_body(&resp).expect_err("missing field must error");

        assert_eq!(err.exit_code, EXIT_SERVER);
        assert!(err.message.contains("description_markdown"));
    }

    #[test]
    fn get_defaults_to_the_full_envelope() {
        let args = get_args();
        let resp = json!({"id": PAGE_UUID, "description_markdown": "# Title"});

        assert!(!args.body_only);
        assert_eq!(
            render_get(&resp, args.body_only).expect("rendered"),
            serde_json::to_string(&resp).expect("json")
        );
    }

    #[test]
    fn body_only_prints_just_the_markdown() {
        let resp = json!({"id": PAGE_UUID, "name": "Conventions", "description_markdown": "# Title\n\nbody"});

        assert_eq!(render_get(&resp, true).expect("rendered"), "# Title\n\nbody");
    }

    fn no_body() -> PageBodyArgs {
        PageBodyArgs {
            body: None,
            body_file: None,
        }
    }

    fn create_args() -> CreateArgs {
        CreateArgs {
            project: "ENG".to_string(),
            title: "Conventions".to_string(),
            body: no_body(),
            parent: None,
            access: None,
        }
    }

    fn update_args() -> UpdateArgs {
        UpdateArgs {
            page_id: PAGE_UUID.to_string(),
            project: "ENG".to_string(),
            title: None,
            body: no_body(),
            parent: None,
            clear_parent: false,
            access: None,
        }
    }

    const NO_STDIN: &[u8] = b"";

    #[test]
    fn create_sends_only_the_title_by_default() {
        let payload = create_payload(&create_args(), NO_STDIN).expect("payload");

        assert_eq!(serde_json::Value::Object(payload), json!({"name": "Conventions"}));
    }

    #[test]
    fn create_maps_every_field() {
        let args = CreateArgs {
            body: PageBodyArgs {
                body: Some("# Rules".to_string()),
                body_file: None,
            },
            parent: Some(PAGE_UUID.to_string()),
            access: Some(PageAccess::Private),
            ..create_args()
        };

        let payload = create_payload(&args, NO_STDIN).expect("payload");

        assert_eq!(
            serde_json::Value::Object(payload),
            json!({"name": "Conventions", "description_markdown": "# Rules", "parent": PAGE_UUID, "access": 1})
        );
    }

    #[test]
    fn create_rejects_an_empty_title() {
        let args = CreateArgs {
            title: "  ".to_string(),
            ..create_args()
        };

        let err = create_payload(&args, NO_STDIN).expect_err("empty title");

        assert_eq!(err.exit_code, EXIT_INVALID);
    }

    #[test]
    fn create_rejects_a_non_uuid_parent() {
        let args = CreateArgs {
            parent: Some("conventions".to_string()),
            ..create_args()
        };

        let err = create_payload(&args, NO_STDIN).expect_err("slug parent");

        assert_eq!(err.exit_code, EXIT_INVALID);
    }

    #[test]
    fn access_maps_to_the_server_integers() {
        assert_eq!(PageAccess::Public.as_api(), 0);
        assert_eq!(PageAccess::Private.as_api(), 1);
    }

    #[test]
    fn body_file_dash_reads_stdin() {
        let args = PageBodyArgs {
            body: None,
            body_file: Some("-".into()),
        };

        let body = load_body(&args, &b"# From stdin\n"[..]).expect("body");

        assert_eq!(body.as_deref(), Some("# From stdin\n"));
    }

    #[test]
    fn body_file_reads_the_named_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("page.md");
        std::fs::write(&path, "# From file").expect("write");
        let args = PageBodyArgs {
            body: None,
            body_file: Some(path),
        };

        assert_eq!(
            load_body(&args, NO_STDIN).expect("body").as_deref(),
            Some("# From file")
        );
    }

    #[test]
    fn missing_body_file_is_invalid() {
        let args = PageBodyArgs {
            body: None,
            body_file: Some("/nonexistent/pidash/page.md".into()),
        };

        let err = load_body(&args, NO_STDIN).expect_err("missing file");

        assert_eq!(err.exit_code, EXIT_INVALID);
    }

    #[test]
    fn update_without_fields_is_rejected() {
        let err = update_payload(&update_args(), NO_STDIN).expect_err("no-op update");

        assert_eq!(err.exit_code, EXIT_INVALID);
        assert!(err.message.contains("--title"));
    }

    #[test]
    fn update_sends_only_the_provided_keys() {
        let args = UpdateArgs {
            access: Some(PageAccess::Public),
            ..update_args()
        };

        let payload = update_payload(&args, NO_STDIN).expect("payload");

        assert_eq!(serde_json::Value::Object(payload), json!({"access": 0}));
    }

    #[test]
    fn update_clear_parent_sends_null() {
        let args = UpdateArgs {
            clear_parent: true,
            ..update_args()
        };

        let payload = update_payload(&args, NO_STDIN).expect("payload");

        assert_eq!(serde_json::Value::Object(payload), json!({"parent": null}));
    }

    #[test]
    fn update_accepts_an_empty_body_to_clear_the_page() {
        let args = UpdateArgs {
            body: PageBodyArgs {
                body: Some(String::new()),
                body_file: None,
            },
            ..update_args()
        };

        let payload = update_payload(&args, NO_STDIN).expect("payload");

        assert_eq!(serde_json::Value::Object(payload), json!({"description_markdown": ""}));
    }

    /// `UpdateArgs` derives `Args`, not `Parser`; wrap it to exercise the
    /// clap relationships the way they are enforced at runtime.
    #[derive(Debug, clap::Parser)]
    struct UpdateHarness {
        #[command(flatten)]
        args: UpdateArgs,
    }

    #[test]
    fn body_and_body_file_conflict() {
        use clap::Parser;
        let err = UpdateHarness::try_parse_from([
            "update", PAGE_UUID, "--project", "ENG", "--body", "x", "--body-file", "-",
        ])
        .expect_err("--body and --body-file are mutually exclusive");

        assert_eq!(err.kind(), clap::error::ErrorKind::ArgumentConflict);
    }

    #[test]
    fn parent_and_clear_parent_conflict() {
        use clap::Parser;
        let err = UpdateHarness::try_parse_from([
            "update", PAGE_UUID, "--project", "ENG", "--parent", PAGE_UUID, "--clear-parent",
        ])
        .expect_err("--parent and --clear-parent are mutually exclusive");

        assert_eq!(err.kind(), clap::error::ErrorKind::ArgumentConflict);
    }

    #[test]
    fn access_parses_by_name() {
        use clap::Parser;
        let parsed = UpdateHarness::try_parse_from([
            "update", PAGE_UUID, "--project", "ENG", "--access", "private",
        ])
        .expect("parse");

        assert_eq!(parsed.args.access, Some(PageAccess::Private));
    }
}
