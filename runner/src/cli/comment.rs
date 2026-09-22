// Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `pidash comment …` subcommands.

use std::path::{Path, PathBuf};

use clap::{Args, Subcommand};
use serde_json::{Map, Value, json};

use crate::api_client::{
    ApiClient, CliEnv, CliError, EXIT_INVALID, EXIT_SERVER, EXIT_UNKNOWN, report_error,
};

use super::resolve::resolve_issue;

#[derive(Debug, Args)]
pub struct CommentArgs {
    #[command(subcommand)]
    pub command: CommentCommand,
}

#[derive(Debug, Args)]
#[group(required = false, multiple = false)]
pub struct CommentBodyArgs {
    /// Comment body (plain text or markdown).
    #[arg(long)]
    body: Option<String>,
    /// Path to a file containing the comment body.
    #[arg(long = "body-file", value_name = "PATH")]
    body_file: Option<PathBuf>,
}

#[derive(Debug, Args)]
pub struct CommentSpeakerArgs {
    /// Mark the comment as spoken by an AI agent with this display name.
    #[arg(long = "as-agent", value_name = "NAME")]
    as_agent: Option<String>,
    /// Pi Dash agent run UUID that produced this comment.
    #[arg(long = "agent-run-id", value_name = "UUID")]
    agent_run_id: Option<String>,
}

#[derive(Debug, Subcommand)]
pub enum CommentCommand {
    /// List comments on a work item.
    List {
        /// Work item identifier, e.g. `ENG-42`.
        identifier: String,
    },
    /// Post a new comment on a work item.
    Add {
        /// Work item identifier, e.g. `ENG-42`.
        identifier: String,
        #[command(flatten)]
        comment_body: CommentBodyArgs,
        /// Path to an image to upload and embed in the comment. Repeatable to
        /// attach several images. Requires a body or at least one image.
        #[arg(long = "image", value_name = "PATH")]
        image: Vec<PathBuf>,
        #[command(flatten)]
        speaker: CommentSpeakerArgs,
        /// Fold this low-value status comment in the UI and omit it from future agent prompts.
        #[arg(long)]
        fold: bool,
    },
    /// Edit an existing comment owned by this user. Requires the issue
    /// identifier because the REST URL is project-scoped.
    Update {
        /// Work item identifier the comment lives on, e.g. `ENG-42`.
        identifier: String,
        /// Comment UUID.
        comment_id: String,
        #[command(flatten)]
        comment_body: CommentBodyArgs,
    },
}

pub async fn run(args: CommentArgs, paths: &crate::util::paths::Paths) -> i32 {
    let env = match CliEnv::resolve(paths) {
        Ok(e) => e,
        Err(e) => return report_error(&e),
    };
    let client = match ApiClient::new(env) {
        Ok(c) => c,
        Err(e) => return report_error(&CliError::new(EXIT_UNKNOWN, format!("{e}"))),
    };

    let result = match args.command {
        CommentCommand::List { identifier } => cmd_list(&client, &identifier).await,
        CommentCommand::Add {
            identifier,
            comment_body,
            image,
            speaker,
            fold,
        } => cmd_add(&client, &identifier, comment_body, image, speaker, fold).await,
        CommentCommand::Update {
            identifier,
            comment_id,
            comment_body,
        } => cmd_update(&client, &identifier, &comment_id, comment_body).await,
    };
    match result {
        Ok(()) => 0,
        Err(e) => report_error(&e),
    }
}

async fn cmd_list(client: &ApiClient, identifier: &str) -> Result<(), CliError> {
    let issue = resolve_issue(client, identifier).await?;
    let path = format!(
        "workspaces/{}/projects/{}/work-items/{}/comments/",
        client.env.workspace_slug, issue.project_id, issue.id
    );
    let resp = client.get(&path).await?;
    println!(
        "{}",
        serde_json::to_string(&resp).expect("serialize JSON value")
    );
    Ok(())
}

async fn cmd_add(
    client: &ApiClient,
    identifier: &str,
    comment_body: CommentBodyArgs,
    images: Vec<PathBuf>,
    speaker: CommentSpeakerArgs,
    fold: bool,
) -> Result<(), CliError> {
    let body = load_comment_body_opt(comment_body)?;
    if body.is_none() && images.is_empty() {
        return Err(CliError::new(
            EXIT_INVALID,
            "provide a comment body (--body/--body-file) or at least one --image",
        ));
    }

    // Read and validate every image up front. A bad path or unsupported type
    // anywhere in the list must fail before the *first* upload, otherwise the
    // earlier images are already stored as assets that nothing will ever
    // reference.
    let prepared = images
        .iter()
        .map(|path| PreparedImage::load(path))
        .collect::<Result<Vec<_>, _>>()?;

    let issue = resolve_issue(client, identifier).await?;

    // Upload each image and collect the embed nodes before posting the comment,
    // so a failed upload aborts without leaving a half-written comment.
    let mut image_nodes = String::new();
    for image in prepared {
        let asset_id = upload_image(client, &issue.project_id, image).await?;
        image_nodes.push_str(&image_component_html(&asset_id));
    }

    let comment_html = compose_comment_html(body.as_deref(), &image_nodes);

    let path = format!(
        "workspaces/{}/projects/{}/work-items/{}/comments/",
        client.env.workspace_slug, issue.project_id, issue.id
    );
    let mut payload: Map<String, Value> = Map::new();
    payload.insert("comment_html".into(), Value::String(comment_html));
    add_fold_label(&mut payload, fold);
    add_speaker_metadata(&mut payload, speaker)?;
    let resp = client.post(&path, &Value::Object(payload)).await?;
    println!(
        "{}",
        serde_json::to_string(&resp).expect("serialize JSON value")
    );
    Ok(())
}

/// Join the optional text body with the embedded-image nodes into one
/// `comment_html` string. The text body is inserted verbatim (the CLI has
/// always treated `--body` as raw comment HTML/markdown).
fn compose_comment_html(body: Option<&str>, image_nodes: &str) -> String {
    match body {
        Some(text) => format!("{text}{image_nodes}"),
        None => image_nodes.to_string(),
    }
}

/// An image that has been read off disk and validated, ready to upload.
struct PreparedImage {
    filename: String,
    content_type: &'static str,
    bytes: Vec<u8>,
}

impl std::fmt::Debug for PreparedImage {
    /// Print the byte count rather than the bytes, so a failing assertion
    /// doesn't dump megabytes of binary into the test output.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PreparedImage")
            .field("filename", &self.filename)
            .field("content_type", &self.content_type)
            .field("bytes", &format_args!("{} bytes", self.bytes.len()))
            .finish()
    }
}

impl PreparedImage {
    /// Resolve the file name, map the extension to an image MIME type, and
    /// read the bytes. Every failure mode that can be detected without
    /// talking to the server is detected here.
    fn load(path: &Path) -> Result<Self, CliError> {
        let filename = path
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| {
                CliError::new(
                    EXIT_INVALID,
                    format!("image path has no file name: {}", display_path(path)),
                )
            })?
            .to_string();
        let content_type = image_mime_type(path)?;
        let bytes = std::fs::read(path).map_err(|e| {
            CliError::new(
                EXIT_INVALID,
                format!("failed reading image {}: {e}", display_path(path)),
            )
        })?;
        Ok(Self {
            filename,
            content_type,
            bytes,
        })
    }
}

/// Upload a single prepared image through the API-key asset surface and return
/// its asset UUID. Three steps mirror the web client: create the asset (which
/// returns a presigned POST), push the bytes straight to S3/MinIO, then mark
/// the asset uploaded.
async fn upload_image(
    client: &ApiClient,
    project_id: &str,
    image: PreparedImage,
) -> Result<String, CliError> {
    let PreparedImage {
        filename,
        content_type,
        bytes,
    } = image;
    let size = bytes.len();

    // 1. Create the asset row and get a presigned upload target.
    let create_path = format!("workspaces/{}/assets/", client.env.workspace_slug);
    let create_body = json!({
        "name": filename,
        "type": content_type,
        "size": size,
        "project_id": project_id,
    });
    let created = client.post(&create_path, &create_body).await?;
    let asset_id = created
        .get("asset_id")
        .and_then(Value::as_str)
        .ok_or_else(|| CliError::new(EXIT_SERVER, "asset create response missing 'asset_id'"))?
        .to_string();
    let upload_data = created
        .get("upload_data")
        .ok_or_else(|| CliError::new(EXIT_SERVER, "asset create response missing 'upload_data'"))?;
    let upload_url = upload_data
        .get("url")
        .and_then(Value::as_str)
        .ok_or_else(|| CliError::new(EXIT_SERVER, "upload_data missing 'url'"))?;
    let fields = upload_data
        .get("fields")
        .and_then(Value::as_object)
        .ok_or_else(|| CliError::new(EXIT_SERVER, "upload_data missing 'fields'"))?;

    // 2. Push the bytes to S3/MinIO with the presigned form.
    client
        .post_multipart(upload_url, fields, &filename, content_type, bytes)
        .await?;

    // 3. Mark the asset uploaded so it becomes servable.
    let confirm_path = format!(
        "workspaces/{}/assets/{}/",
        client.env.workspace_slug, asset_id
    );
    client
        .patch(&confirm_path, &json!({ "is_uploaded": true }))
        .await?;

    Ok(asset_id)
}

/// Serialize the editor's custom image node. `src` carries the asset UUID; the
/// web editor resolves it to the asset URL at render time. Width/height/
/// alignment match the editor defaults so the image renders sensibly.
fn image_component_html(asset_id: &str) -> String {
    format!(
        "<image-component src=\"{asset_id}\" width=\"35%\" height=\"auto\" alignment=\"left\"></image-component>"
    )
}

/// Map a file extension to an image MIME type accepted by the asset endpoint.
/// Errors on anything that isn't a supported image, since the embed node only
/// renders images.
fn image_mime_type(path: &Path) -> Result<&'static str, CliError> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase());
    match ext.as_deref() {
        Some("jpg") | Some("jpeg") => Ok("image/jpeg"),
        Some("png") => Ok("image/png"),
        Some("gif") => Ok("image/gif"),
        Some("webp") => Ok("image/webp"),
        Some("svg") => Ok("image/svg+xml"),
        Some("bmp") => Ok("image/bmp"),
        Some("tif") | Some("tiff") => Ok("image/tiff"),
        _ => Err(CliError::new(
            EXIT_INVALID,
            format!(
                "unsupported image type for {}: expected one of jpg, jpeg, png, gif, webp, svg, bmp, tif, tiff",
                display_path(path)
            ),
        )),
    }
}

fn add_fold_label(payload: &mut Map<String, Value>, fold: bool) {
    if fold {
        payload.insert(
            "labels".into(),
            Value::Array(vec![Value::String("fold".into())]),
        );
    }
}

fn add_speaker_metadata(
    payload: &mut Map<String, Value>,
    speaker: CommentSpeakerArgs,
) -> Result<(), CliError> {
    let as_agent = speaker.as_agent.and_then(non_empty);
    let agent_run_id = speaker.agent_run_id.and_then(non_empty);
    match (as_agent, agent_run_id) {
        (Some(label), agent_run_id) => {
            payload.insert("speaker_type".into(), Value::String("agent".into()));
            payload.insert("speaker_label".into(), Value::String(label));
            if let Some(run_id) = agent_run_id {
                payload.insert("speaker_agent_run_id".into(), Value::String(run_id));
            }
        }
        (None, Some(_)) => {
            return Err(CliError::new(
                EXIT_INVALID,
                "--agent-run-id requires --as-agent so Pi Dash can mark the comment speaker",
            ));
        }
        (None, None) => {}
    }
    Ok(())
}

fn non_empty(value: String) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

async fn cmd_update(
    client: &ApiClient,
    identifier: &str,
    comment_id: &str,
    comment_body: CommentBodyArgs,
) -> Result<(), CliError> {
    let body = load_comment_body(comment_body)?;
    let issue = resolve_issue(client, identifier).await?;
    let path = format!(
        "workspaces/{}/projects/{}/work-items/{}/comments/{}/",
        client.env.workspace_slug, issue.project_id, issue.id, comment_id
    );
    let mut payload: Map<String, Value> = Map::new();
    payload.insert("comment_html".into(), Value::String(body));
    let resp = client.patch(&path, &Value::Object(payload)).await?;
    println!(
        "{}",
        serde_json::to_string(&resp).expect("serialize JSON value")
    );
    Ok(())
}

/// Load the comment body, if any. Returns `None` when neither `--body` nor
/// `--body-file` was given (valid for an image-only `comment add`). clap's
/// mutually-exclusive group guarantees at most one source is set.
fn load_comment_body_opt(args: CommentBodyArgs) -> Result<Option<String>, CliError> {
    match (args.body, args.body_file) {
        (Some(body), None) => Ok(Some(body)),
        (None, Some(path)) => std::fs::read_to_string(&path).map(Some).map_err(|e| {
            CliError::new(
                EXIT_UNKNOWN,
                format!(
                    "failed reading comment body file {}: {e}",
                    display_path(&path)
                ),
            )
        }),
        (None, None) => Ok(None),
        (Some(_), Some(_)) => unreachable!("clap enforces at most one comment body source"),
    }
}

/// Load a required comment body, erroring if none was supplied. Used by
/// `comment update`, which must always carry replacement text.
fn load_comment_body(args: CommentBodyArgs) -> Result<String, CliError> {
    load_comment_body_opt(args)?.ok_or_else(|| {
        CliError::new(
            EXIT_INVALID,
            "provide a comment body with --body or --body-file",
        )
    })
}

fn display_path(path: &Path) -> String {
    path.display().to_string()
}

#[cfg(test)]
mod tests {
    use serde_json::{Map, Value};

    use clap::Parser;

    use std::path::Path;

    use super::{
        CommentBodyArgs, CommentSpeakerArgs, PreparedImage, add_fold_label, add_speaker_metadata,
        compose_comment_html, image_component_html, image_mime_type, load_comment_body,
        load_comment_body_opt,
    };

    #[derive(Debug, Parser)]
    struct TestCli {
        #[command(flatten)]
        comments: super::CommentArgs,
    }

    #[test]
    fn load_comment_body_prefers_inline_body() {
        let body = load_comment_body(CommentBodyArgs {
            body: Some("hello".into()),
            body_file: None,
        })
        .expect("inline body");
        assert_eq!(body, "hello");
    }

    #[test]
    fn load_comment_body_reads_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("comment.md");
        std::fs::write(&path, "from file\n").expect("write file");

        let body = load_comment_body(CommentBodyArgs {
            body: None,
            body_file: Some(path),
        })
        .expect("file body");
        assert_eq!(body, "from file\n");
    }

    #[test]
    fn add_speaker_metadata_marks_agent_comment() {
        let mut payload = Map::new();
        add_speaker_metadata(
            &mut payload,
            CommentSpeakerArgs {
                as_agent: Some("Codex".into()),
                agent_run_id: Some("11111111-1111-1111-1111-111111111111".into()),
            },
        )
        .expect("speaker metadata");

        assert_eq!(
            payload.get("speaker_type"),
            Some(&Value::String("agent".into()))
        );
        assert_eq!(
            payload.get("speaker_label"),
            Some(&Value::String("Codex".into()))
        );
        assert_eq!(
            payload.get("speaker_agent_run_id"),
            Some(&Value::String(
                "11111111-1111-1111-1111-111111111111".into()
            ))
        );
    }

    #[test]
    fn add_speaker_metadata_rejects_agent_run_without_agent_label() {
        let mut payload = Map::new();
        let err = add_speaker_metadata(
            &mut payload,
            CommentSpeakerArgs {
                as_agent: None,
                agent_run_id: Some("11111111-1111-1111-1111-111111111111".into()),
            },
        )
        .expect_err("run id without agent label should fail");

        assert_eq!(err.exit_code, crate::api_client::EXIT_INVALID);
        assert!(err.message.contains("--agent-run-id requires --as-agent"));
        assert!(payload.is_empty());
    }

    #[test]
    fn add_accepts_fold_flag() {
        let parsed =
            TestCli::try_parse_from(["pidash", "add", "ENG-42", "--body", "No change", "--fold"])
                .expect("parse folded comment");

        match parsed.comments.command {
            super::CommentCommand::Add { fold, .. } => assert!(fold),
            _ => panic!("expected comment add"),
        }
    }

    #[test]
    fn fold_flag_adds_reserved_label_to_payload() {
        let mut payload = Map::new();
        add_fold_label(&mut payload, true);

        assert_eq!(
            payload.get("labels"),
            Some(&Value::Array(vec![Value::String("fold".into())]))
        );
    }

    #[test]
    fn load_comment_body_opt_none_when_absent() {
        let body = load_comment_body_opt(CommentBodyArgs {
            body: None,
            body_file: None,
        })
        .expect("no body source is allowed");
        assert_eq!(body, None);
    }

    #[test]
    fn image_component_embeds_asset_id_as_src() {
        let html = image_component_html("11111111-1111-1111-1111-111111111111");
        assert!(html.starts_with("<image-component src=\"11111111-1111-1111-1111-111111111111\""));
        assert!(html.ends_with("></image-component>"));
    }

    #[test]
    fn compose_comment_html_joins_body_and_images() {
        let node = image_component_html("abc");
        assert_eq!(compose_comment_html(Some("<p>hi</p>"), &node), format!("<p>hi</p>{node}"));
        assert_eq!(compose_comment_html(None, &node), node);
        assert_eq!(compose_comment_html(Some("only text"), ""), "only text");
    }

    #[test]
    fn image_mime_type_maps_known_extensions() {
        assert_eq!(image_mime_type(Path::new("a.png")).unwrap(), "image/png");
        assert_eq!(image_mime_type(Path::new("a.JPG")).unwrap(), "image/jpeg");
        assert_eq!(image_mime_type(Path::new("a.jpeg")).unwrap(), "image/jpeg");
        assert_eq!(image_mime_type(Path::new("dir/b.WebP")).unwrap(), "image/webp");
        assert_eq!(image_mime_type(Path::new("a.gif")).unwrap(), "image/gif");
    }

    #[test]
    fn image_mime_type_rejects_non_images() {
        let err = image_mime_type(Path::new("notes.txt")).expect_err("txt is not an image");
        assert_eq!(err.exit_code, crate::api_client::EXIT_INVALID);
        let err = image_mime_type(Path::new("noext")).expect_err("missing extension");
        assert_eq!(err.exit_code, crate::api_client::EXIT_INVALID);
    }

    #[test]
    fn prepared_image_load_reads_bytes_and_mime() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("shot.png");
        std::fs::write(&path, b"\x89PNG fake bytes").expect("write fixture");

        let image = PreparedImage::load(&path).expect("png loads");
        assert_eq!(image.filename, "shot.png");
        assert_eq!(image.content_type, "image/png");
        assert_eq!(image.bytes, b"\x89PNG fake bytes");
    }

    #[test]
    fn prepared_image_load_rejects_missing_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let err = PreparedImage::load(&dir.path().join("absent.png"))
            .expect_err("a missing file must not load");
        assert_eq!(err.exit_code, crate::api_client::EXIT_INVALID);
    }

    /// Every image is read and validated before the first upload, so a bad
    /// path late in the list cannot leave earlier images uploaded as orphaned
    /// assets.
    #[test]
    fn prepared_image_load_fails_whole_batch_before_any_upload() {
        let dir = tempfile::tempdir().expect("tempdir");
        let good = dir.path().join("good.png");
        std::fs::write(&good, b"png").expect("write fixture");
        let bad = dir.path().join("notes.txt");
        std::fs::write(&bad, b"text").expect("write fixture");

        let batch: Result<Vec<_>, _> = [good, bad].iter().map(|p| PreparedImage::load(p)).collect();
        let err = batch.expect_err("a .txt anywhere in the batch fails the batch");
        assert_eq!(err.exit_code, crate::api_client::EXIT_INVALID);
        assert!(err.message.contains("unsupported image type"), "{}", err.message);
    }

    #[test]
    fn add_accepts_image_flag_repeatably() {
        let parsed = TestCli::try_parse_from([
            "pidash", "add", "ENG-42", "--image", "a.png", "--image", "b.jpg",
        ])
        .expect("parse image-only comment");

        match parsed.comments.command {
            super::CommentCommand::Add { image, .. } => {
                assert_eq!(image.len(), 2);
            }
            _ => panic!("expected comment add"),
        }
    }
}
