//! `core::comments::{create, update}` (NEWPIDASH-190 §4.3): the
//! shared comment write over a caller-supplied
//! [`WriteTxn`](crate::domain::WriteTxn).
//!
//! - [`CommentCreate`] + [`Speaker`]: `operations::comments::create`
//!   validates the request shape and speaker fields, and calls this
//!   for the three domain row writes; the agent
//!   `op_add_current_issue_comment` calls it with build-derived
//!   values. Rows, stamps and the description link are byte-equal
//!   to the two inlined transactions they replace.
//! - [`CommentUpdate`]: `operations::comments::update` merges the
//!   PATCH into the before image and calls this; it keeps the
//!   rendering, dup guard and activity.
use chrono::{DateTime, Utc};
use serde_json::Value;
use uuid::Uuid;

use crate::domain::comments as comment_domain;
use crate::domain::{WriteCtx, WriteTxn};

use super::CoreError;

/// A stored speaker: the columns as written, typed, not a body map.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Speaker {
    /// An agent speaker (`speaker_type: "agent"`): the cloud-agent
    /// run that wrote the comment.
    Agent {
        /// The owning `agent_run`.
        run_id: Uuid,
    },
    /// Any other speaker (the operations callers' validated values):
    /// a free type and label from the request body (default `"human"`
    /// / `""`), with an optional run id.
    User {
        /// The validated speaker type (`validated.speaker_type`).
        speaker_type: String,
        /// The validated speaker label (`validated.speaker_label`).
        label: String,
        /// The optional run id (`validated.speaker_agent_run_id`).
        run_id: Option<Uuid>,
    },
}

impl Speaker {
    /// The stored columns as written on create.
    fn fields(&self) -> (String, String, Option<Uuid>) {
        match self {
            Self::Agent { run_id } => (
                "agent".to_owned(),
                "Pi Dash Cloud Agent".to_owned(),
                Some(*run_id),
            ),
            Self::User {
                speaker_type,
                label,
                run_id,
            } => (speaker_type.clone(), label.clone(), *run_id),
        }
    }
}

/// The external duplicação identity carried on the row.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CommentExternal {
    /// The validated external id.
    pub id: Option<String>,
    /// The validated external source.
    pub source: Option<String>,
}

/// A comment create across both wrappers.
#[derive(Debug, Clone)]
pub struct CommentCreate {
    /// The commented issue.
    pub issue_id: Uuid,
    /// The owning project.
    pub project_id: Uuid,
    /// The workspace (both wrappers resolve it before the write).
    pub workspace_id: Uuid,
    /// The sanitized comment html (models shape: `comment_html`), owned
    pub html: String,
    /// The writer: the agent's cloud run or the request's speaker.
    pub speaker: Speaker,
    /// The comment access (`INTERNAL` on both paths today).
    pub access: Option<String>,
    /// The external duplicate identity.
    pub external: CommentExternal,
    /// The row's `created_by` override (operations-only: the
    /// `CreatedAtResolver`'s callers pass the resolved value; `None`
    /// is an explicit null).
    pub created_by: Option<Uuid>,
    /// The row's `created_at` (the operations resolver's db value,
    /// or the wrapper's stamp when nothing was supplied).
    pub created_at: DateTime<Utc>,
    /// The comment json (the models' `dict` shape; `None` runs the
    /// prior `{}` default).
    pub json: Option<Value>,
    /// The labels (the agent path writes none).
    pub labels: Option<Vec<String>>,
}

/// The return of a landed comment create.
#[derive(Debug, Clone)]
pub struct CommentCreated {
    /// The inserted comment id.
    pub id: Uuid,
    /// The linked description id (the `description:` response field).
    pub description_id: Uuid,
    /// The stored `created_at` (the override when set, else now).
    pub created_at: DateTime<Utc>,
}

/// A comment update across both wrappers (today only the operations
/// PATCH drives it; the agent wraps it in phase 4).
#[derive(Debug, Clone)]
pub struct CommentUpdate {
    /// The patched comment id.
    pub id: Uuid,
    /// The new comment html.
    pub html: String,
    /// The stored stripped text for the comment (recomputed verbatim
    /// on save when untouched by the writer: `issue.py:606`).
    pub stripped: String,
    /// The new comment json (`None` keeps nothing new: partial semantics
    /// stay with the wrapper's merge).
    pub json: Option<Value>,
    /// The new speaker (the wrapper merges validated over before).
    pub speaker: Speaker,
    /// The new access (`validated.access`, the merged default stays
    /// with the wrapper).
    pub access: Option<String>,
    /// The labels (the wrapper merges over before).
    pub labels: Option<Vec<String>>,
    /// The external duplicate identity (the wrapper merges over
    /// before).
    pub external: CommentExternal,
    /// The comment's `description_id` (the sync-or-mint discriminator);
    /// registration requires `Some`, as the description side of the
    /// previous save requires.
    pub description_id: Option<Uuid>,
    /// The workspace (the description side carries it).
    pub workspace_id: Uuid,
    /// The project (the description side carries it).
    pub project_id: Uuid,
    /// The merge's `new_stripped` value for the description-side
    /// changedness computation (`stripped_changed`/spot-sync).
    pub is_stripped_changed: bool,
    /// Whether `html`/`json` changed (the changedness of the
    /// description subset; `is_stripped_changed` reads separately
    /// since Python compares values, not presence).
    pub are_html_json_changed: bool,
}

/// The return of a landed comment update.
#[derive(Debug, Clone)]
pub struct CommentUpdated {
    /// The updated comment id.
    pub id: Uuid,
    /// The stored `updated_at` (the now stamp this save wrote).
    pub updated_at: DateTime<Utc>,
}

/// `core::comments::create(txn, ctx, CommentCreate { … }) ->
/// CommentCreated { id, created_at }` (§4.3). Runs the three domain
/// row writes on the caller's transaction: comment INSERT,
/// description INSERT, link update. `ctx` stamps the actor (the
/// response decode of `created_by` reads the override from the
/// wrapper, as today).
pub async fn create(
    txn: &mut WriteTxn,
    ctx: &WriteCtx,
    input: CommentCreate,
    _effects: Option<&[crate::domain::issues::ChainEffect]>,
) -> Result<CommentCreated, CoreError> {
    let comment_id = Uuid::new_v4();
    let description_id = Uuid::new_v4();
    let now = crate::operations::common::now_utc();
    let empty_json = empty_json_inner().clone();
    let json = input.json.clone().unwrap_or(empty_json);
    let stripped = strip_comment_html(&input.html);
    let desc_stripped = crate::operations::comments::description_stripped_for_create(&input.html);
    let labels: Vec<String> = input.labels.clone().unwrap_or_default();
    let access = input.access.as_deref().unwrap_or("INTERNAL");
    let (speaker_type, speaker_label, speaker_run_id) = input.speaker.fields();
    let created_at = input.created_at;
    let empty_list: Vec<String> = Vec::new();
    let rows = comment_domain::CommentRows {
        id: comment_id,
        description_id,
        created_at,
        updated_at: now,
        desc_created_at: crate::operations::common::now_utc(),
        desc_updated_at: crate::operations::common::now_utc(),
        workspace_id: input.workspace_id,
        project_id: input.project_id,
        issue_id: input.issue_id,
        actor_id: Some(ctx.actor.user_id),
        created_by: Some(input.created_by.unwrap_or(ctx.actor.user_id)),
        updated_by: None,
        deleted_at: None,
        html: &input.html,
        json: &json,
        stripped: &stripped,
        desc_stripped: desc_stripped.as_deref(),
        access,
        speaker_type: &speaker_type,
        speaker_label: &speaker_label,
        speaker_agent_run_id: speaker_run_id,
        external_id: input.external.id.as_deref(),
        external_source: input.external.source.as_deref(),
        parent_id: None,
        attachments: &empty_list,
        labels: &labels,
        edited_at: None,
    };
    comment_domain::insert_comment_row(&txn.connection(), &rows).await?;
    comment_domain::insert_description_row(&txn.connection(), &rows).await?;
    comment_domain::link_comment_description(&txn.connection(), rows.id, rows.description_id)
        .await?;
    Ok(CommentCreated {
        id: comment_id,
        description_id,
        created_at,
    })
}

/// `core::comments::update(txn, ctx, CommentUpdate { … }) ->
/// CommentUpdated { id, updated_at }` (§4.3). Runs the `UPDATE` plus
/// the description sync-or-mint inside the caller's transaction
/// (mirrors `save`'s journal space exactly); the wrapper keeps the
/// activity and webhook emissions.
pub async fn update(
    txn: &mut WriteTxn,
    ctx: &WriteCtx,
    input: CommentUpdate,
    _effects: Option<&[crate::domain::issues::ChainEffect]>,
) -> Result<CommentUpdated, CoreError> {
    let now = crate::operations::common::now_utc();
    let actor_id = ctx.actor.user_id;
    let (speaker_type, speaker_label, speaker_run_id) = input.speaker.fields();
    comment_domain::save_v1_comment_update(
        &txn.connection(),
        &comment_domain::V1CommentUpdate {
            id: input.id,
            json: &input
                .json
                .clone()
                .unwrap_or_else(|| empty_json_inner().clone()),
            html: &input.html.clone(),
            stripped: &input.stripped.clone(),
            access: input.access.as_deref().unwrap_or("INTERNAL"),
            external_source: input.external.source.as_deref(),
            external_id: input.external.id.as_deref(),
            labels: input.labels.as_deref().unwrap_or(&[]),
            speaker_type: &speaker_type,
            speaker_label: &speaker_label,
            speaker_agent_run_id: speaker_run_id,
            updated_at: now,
            updated_by: actor_id,
        },
    )
    .await?;
    if let Some(description_id) = input.description_id {
        if input.are_html_json_changed || input.is_stripped_changed {
            let sync_html = &input.html;
            let sync_stripped = &input.stripped;
            let sync_json = input
                .json
                .clone()
                .unwrap_or_else(|| empty_json_inner().clone());
            comment_domain::sync_v1_description_subset(
                &txn.connection(),
                &comment_domain::V1DescriptionSync {
                    id: description_id,
                    html: input.are_html_json_changed.then_some(sync_html.as_str()),
                    stripped: input.is_stripped_changed.then_some(sync_stripped.as_str()),
                    json: input.are_html_json_changed.then_some(&sync_json),
                    updated_by: actor_id,
                    updated_at: now,
                },
            )
            .await?;
        }
    } else {
        // Unlinked legacy row: mints over `description_defaults`
        // (`:607-627`) via the same `Description.save`.
        let desc_stripped =
            crate::operations::comments::description_stripped_for_create(&input.html);
        let json = input.json.clone().unwrap_or(empty_json_inner().clone());
        let desc = comment_domain::DescriptionValues {
            id: Uuid::new_v4(),
            created_at: crate::operations::common::now_utc(),
            updated_at: crate::operations::common::now_utc(),
            created_by: Some(actor_id),
            updated_by: None,
            workspace_id: input.workspace_id,
            project_id: input.project_id,
            json: &json,
            html: &input.html,
            stripped: desc_stripped.as_deref(),
        };
        let desc_id = desc.id;
        comment_domain::insert_description_values(&txn.connection(), &desc).await?;
        comment_domain::link_comment_description(&txn.connection(), input.id, desc_id).await?;
    }
    Ok(CommentUpdated {
        id: input.id,
        updated_at: now,
    })
}

fn strip_comment_html(html: &str) -> String {
    crate::operations::comments::strip_comment_html(html)
}

fn empty_json_inner() -> &'static Value {
    static VALUE: std::sync::OnceLock<Value> = std::sync::OnceLock::new();
    VALUE.get_or_init(|| serde_json::json!({}))
}
