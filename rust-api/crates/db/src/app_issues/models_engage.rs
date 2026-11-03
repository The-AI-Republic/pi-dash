//! Issue engagement table models (D-26, stage 5).
//!
//! Ports `apps/api/pi_dash/db/models/issue.py:550-667` (`IssueComment`),
//! `:700-724` (`IssueSubscriber`), `:726-751` (`IssueReaction`),
//! `:753-778` (`CommentReaction`) and `:780-801` (`IssueVote`), adopting
//! the Django-owned schema column-for-column; migrations are not ported —
//! Django stays schema owner until switchover. Fixture source of truth:
//! `rust-api/fixtures/app_issues/models/FX-ISS-08.engage.json` (recorded by
//! PIDASHCONV-637); the `#[cfg(test)]` suite replays it section by
//! section.
//!
//! Column order in each `*_COLUMNS` const follows the fixture: the 8
//! inherited audit/project columns first (`id`, `created_at`,
//! `updated_at`, `created_by_id`, `updated_by_id`, `deleted_at`, then
//! `project_id`, `workspace_id` from `ProjectBaseModel` at
//! `db/models/project.py:302-311`), then the model fields in declaration
//! order. FK entries use the Django attnames (`project_id`, `issue_id`,
//! `comment_id`, `subscriber_id`, `parent_id`, …). Every
//! application-level default below is Django-side (the live tables carry
//! no `column_default` in `information_schema`, as established for D-01);
//! Rust inserts must supply these values explicitly.
//!
//! # Reads are soft-delete scoped
//!
//! All five tables inherit the soft-delete marker (`deleted_at`, from
//! `SoftDeleteModel` in `pi_dash/db/mixins.py:61-67`) and the default
//! manager filters `deleted_at IS NULL` (`objects = SoftDeletionManager`,
//! `mixins.py:66`; `all_objects` is the plain unscoped manager,
//! `mixins.py:67`). Every read built from these tables must apply
//! [`crate::soft_delete::active_condition`]; the tests pin this by
//! rendering a scoped `SELECT` per table.
//!
//! # Writes backfill the workspace
//!
//! `ProjectBaseModel.save()` (`db/models/project.py:309-311`) sets
//! `workspace` from `project.workspace` on every save: Rust inserts and
//! updates of all five tables must resolve `workspace_id` from the
//! `project_id` row explicitly.
//!
//! # No `delete()` override
//!
//! None of the five models defines `delete()`; deletion is
//! `SoftDeleteModel.delete` (`mixins.py:72-78`), which stamps
//! `deleted_at` and calls `save()` — so destroy DOES emit
//! `pre_save`/`post_save`. There is no code to port here (shared kernel
//! behavior); the write layers must not assume "no signal on destroy".
//!
//! # Overlap with merged defs (deliberate local ports)
//!
//! Four of the five tables also appear in D-02 `space::columns` (columns
//! only, plus the two unique-constraint names, `ACCESS_DEFAULT` and
//! `VOTE_DEFAULT`): `space::columns::{issue_comment, issue_reaction,
//! comment_reaction, issue_vote}`. The comment full-text-search index
//! also appears in D-32 `app_views_search::issue_comment_ref` (table,
//! read columns, FTS consts, and the `issue.py:606` strip rule as
//! `sync_comment_stripped`). This module still ports all five units in
//! full: D-26 owns the activity/comments/reactions/subscribers endpoints
//! (PIDASHCONV-654) and their querysets (PIDASHCONV-649), and the fixture
//! records these five as first-class units. Cross-domain column
//! duplication is established practice (`v1_assets` and `app_assets`
//! carry identical `file_assets` `COLUMNS`); each domain's models layer is
//! the contract its own queries layer builds against. The tests assert
//! this module's consts equal the merged defs where they overlap, so a
//! divergence fails loudly instead of forking silently.
//!
//! # Foreign reads: pinned literals
//!
//! The columns D-26 reads on the foreign `descriptions` table (fixture
//! `foreign_reads`) are pinned as SQL literals in [`foreign`]; query
//! builders copy those literals verbatim and the tests assert they equal
//! the fixture. `Description` belongs to the models-D sub-issue
//! (PIDASHCONV-647, FX-ISS-10), which has not merged yet, so there is no
//! merged def to reuse — the fixture instructs "SQL literal pinned here".
//!
//! # Ported quirks (translate as-is)
//!
//! * `CommentReaction.__str__` (`issue.py:776-777`) dereferences
//!   `self.issue`, but the model has no `issue` field (only `comment`,
//!   `:759`) — stringifying any comment reaction raises
//!   `AttributeError`. Ported as [`comment_reaction::CommentReaction`]
//!   `Display` rendering `"{comment_id} {actor_id}"`: the row carries
//!   only the FKs, so the ids stand in for the joined values (same
//!   substitution as the D-32 intake display ports), and the port does
//!   not raise (same non-raising treatment as the `IssueView` ported bug
//!   in `app_views_search::models`).
//! * `IssueComment.comment_stripped` declares no `default=` (migration
//!   `0001_initial.py` state: `TextField(blank=True)`); the fixture's
//!   `default ''` is the `save()`-computed floor (`:606`: `""` when
//!   `comment_html == ""`, else `strip_tags(html)`). There is no
//!   `COMMENT_STRIPPED_DEFAULT` const — Rust inserts must compute the
//!   value through the save rule.
//! * `IssueComment.save()` (`:598-647`) captures `description_defaults`
//!   (`:610-618`) BEFORE `super().save()` runs the workspace backfill,
//!   so the linked `Description` row is created with the instance's
//!   pre-backfill `workspace_id`. The write layer must reproduce that
//!   order, not "fix" it by backfilling first.
//! * The save update path (`:628-647`) propagates only the changed
//!   tracked fields through `COMMENT_TO_DESCRIPTION`, plus
//!   `updated_by_id`/`updated_at`, via a queryset `.update()` —
//!   `Description` signals never fire. And `is_creating or not
//!   self.description_id` (`:623`) means an update whose
//!   `description_id` is `None` creates a fresh `Description` row.
//! * `strip_tags` is `pi_dash.utils.html_processor.strip_tags`
//!   (`issue.py:21`, MLStripper-based), not Django's. The strip rule
//!   itself is merged in
//!   `app_views_search::issue_comment_ref::sync_comment_stripped`; this
//!   module ports only the pure decision halves ([`issue_comment`]
//!   `TRACKED_FIELDS`, `COMMENT_TO_DESCRIPTION`, `DESCRIPTION_*`,
//!   `creates_description_on_save`, `description_field_for`).
//! * Every reaction/vote/subscriber table keeps BOTH the legacy
//!   `unique_together` (which includes `deleted_at`, so active-row
//!   `NULL`s never collide) and the partial `UniqueConstraint WHERE
//!   deleted_at IS NULL` (which does the real dedupe). Ported as
//!   `UNIQUE_TOGETHER` plus a `NAME`/`COLUMNS`/`WHERE` triple each.
//! * `IssueComment.access` accepts `EXTERNAL` at the model level;
//!   space-list filtering and create-time forcing of `EXTERNAL` live in
//!   the space views, not here. The model stores whatever it is given.
//! * The `labels` array (`size=8`, 32-char elements) carries behavior
//!   markers such as `fold`, which is opt-in (`:564-566`): Pi Dash never
//!   infers it from the body or speaker. No inference here.
//! * `edited_at` is set by the comment view iff `comment_html` changed
//!   (`app/views/issue/comment.py:124-127`), never by the model. No
//!   auto-stamp here.

use super::models_core::OnDelete;

/// `issue_comments` table (`issue.py:550-667`).
pub mod issue_comment {
    use super::OnDelete;
    use serde::{Deserialize, Serialize};

    /// Physical table (`Meta.db_table`, `issue.py:652`).
    pub const TABLE: &str = "issue_comments";
    /// Default ordering (`Meta.ordering`, `issue.py:653`).
    pub const ORDERING: &str = "-created_at";
    /// `verbose_name` (`issue.py:650`).
    pub const VERBOSE_NAME: &str = "Issue Comment";
    /// `verbose_name_plural` (`issue.py:651`).
    pub const VERBOSE_NAME_PLURAL: &str = "Issue Comments";

    /// Columns in fixture FX-ISS-08 order: 8 inherited audit/project
    /// columns, then the 16 declared fields in order (`issue.py:557-594`).
    /// FK columns use the Django attnames.
    pub const COLUMNS: &[&str] = &[
        "id",
        "created_at",
        "updated_at",
        "created_by_id",
        "updated_by_id",
        "deleted_at",
        "project_id",
        "workspace_id",
        "comment_stripped",
        "comment_json",
        "comment_html",
        "description_id",
        "attachments",
        "labels",
        "issue_id",
        "actor_id",
        "access",
        "external_source",
        "external_id",
        "speaker_type",
        "speaker_label",
        "speaker_agent_run_id",
        "edited_at",
        "parent_id",
    ];

    /// `SpeakerType` values + labels in declaration order
    /// (`issue.py:551-555`).
    pub const SPEAKER_TYPE_CHOICES: &[(&str, &str)] = &[
        ("human", "Human"),
        ("agent", "Agent"),
        ("system", "System"),
        ("integration", "Integration"),
    ];
    /// `speaker_type` default (`issue.py:587`, `SpeakerType.HUMAN`).
    pub const SPEAKER_TYPE_DEFAULT: &str = "human";
    /// `speaker_type` bound (`issue.py:585`, `CharField(max_length=32)`).
    pub const SPEAKER_TYPE_MAX_LENGTH: usize = 32;
    /// `speaker_label` bound (`issue.py:589`, `CharField(max_length=128,
    /// blank=True)`).
    pub const SPEAKER_LABEL_MAX_LENGTH: usize = 128;
    /// `speaker_label` Django-side default (`issue.py:589`, `default=""`).
    pub const SPEAKER_LABEL_DEFAULT: &str = "";

    /// `access` choices (`issue.py:576`).
    pub const ACCESS_CHOICES: &[(&str, &str)] =
        &[("INTERNAL", "INTERNAL"), ("EXTERNAL", "EXTERNAL")];
    /// `access` default (`issue.py:577`).
    pub const ACCESS_DEFAULT: &str = "INTERNAL";
    /// `access` bound (`issue.py:578`, `CharField(max_length=100)`).
    pub const ACCESS_MAX_LENGTH: usize = 100;

    /// `external_source` bound (`issue.py:580`, `CharField(max_length=255,
    /// null=True, blank=True)`).
    pub const EXTERNAL_SOURCE_MAX_LENGTH: usize = 255;
    /// `external_id` bound (`issue.py:581`, `CharField(max_length=255,
    /// blank=True, null=True)`).
    pub const EXTERNAL_ID_MAX_LENGTH: usize = 255;

    /// `comment_html` Django-side default (`issue.py:559`,
    /// `default="<p></p>"`).
    pub const COMMENT_HTML_DEFAULT: &str = "<p></p>";

    /// `attachments` array bound (`issue.py:563`, `ArrayField(URLField,
    /// size=10)`).
    pub const ATTACHMENTS_SIZE: usize = 10;
    /// `labels` array bound (`issue.py:566`, `ArrayField(CharField, size=8)`).
    pub const LABELS_SIZE: usize = 8;
    /// `labels` element bound (`issue.py:566`,
    /// `CharField(max_length=32)`).
    pub const LABEL_ELEMENT_MAX_LENGTH: usize = 32;

    /// `comment_json` Django-side default (`issue.py:558`,
    /// `default=dict`). Fresh value per call, matching the Python
    /// callable returning a new dict each time.
    pub fn default_comment_json() -> serde_json::Value {
        serde_json::json!({})
    }

    /// `attachments` Django-side default (`issue.py:563`,
    /// `default=list`). Fresh value per call.
    pub fn default_attachments() -> Vec<String> {
        Vec::new()
    }

    /// `labels` Django-side default (`issue.py:566`, `default=list`).
    /// Fresh value per call.
    pub fn default_labels() -> Vec<String> {
        Vec::new()
    }

    /// Fields whose changes propagate to the linked `Description`
    /// (`issue.py:596`, `TRACKED_FIELDS`).
    pub const TRACKED_FIELDS: &[&str] = &["comment_stripped", "comment_json", "comment_html"];

    /// Comment-field → description-field mapping in `field_mapping` order
    /// (`issue.py:629-633`).
    pub const COMMENT_TO_DESCRIPTION: &[(&str, &str)] = &[
        ("comment_html", "description_html"),
        ("comment_stripped", "description_stripped"),
        ("comment_json", "description_json"),
    ];

    /// Keys of `description_defaults` in order (`issue.py:610-618`): what
    /// the create path supplies to `Description.objects.create`. Note
    /// these are captured BEFORE `super().save()` backfills the
    /// workspace (see the module docs).
    pub const DESCRIPTION_DEFAULT_KEYS: &[&str] = &[
        "workspace_id",
        "project_id",
        "created_by_id",
        "updated_by_id",
        "description_stripped",
        "description_json",
        "description_html",
    ];

    /// Columns the save update path always stamps alongside the changed
    /// tracked fields (`issue.py:646`, `updated_by_id` + `updated_at`).
    pub const DESCRIPTION_UPDATE_TOUCHED: &[&str] = &["updated_by_id", "updated_at"];

    /// Pure create-branch condition of `IssueComment.save`
    /// (`issue.py:623`): `is_creating or not self.description_id` — an
    /// update whose `description_id` is `None` also creates a fresh
    /// `Description` row.
    pub fn creates_description_on_save(
        is_creating: bool,
        description_id: Option<uuid::Uuid>,
    ) -> bool {
        is_creating || description_id.is_none()
    }

    /// Pure field-map lookup of the save update path (`issue.py:629-641`):
    /// the `Description` column a changed comment field propagates to, or
    /// `None` for untracked fields (Python skips them via the
    /// `if comment_field in self._changes_on_save` guard; the caller owns
    /// the emptiness check `if changed_fields and self.description_id`).
    pub fn description_field_for(comment_field: &str) -> Option<&str> {
        COMMENT_TO_DESCRIPTION
            .iter()
            .find(|(comment, _)| *comment == comment_field)
            .map(|(_, description)| *description)
    }

    /// Full-text-search index name (`issue.py:660`).
    pub const FTS_INDEX_NAME: &str = "issue_comments_fts_idx";
    /// Full-text config (`SearchVector(..., config="english")`,
    /// `issue.py:659`).
    pub const FTS_CONFIG: &str = "english";
    /// Source columns of the search vector (`issue.py:659`).
    pub const FTS_SOURCE_COLUMNS: &[&str] = &["comment_stripped"];

    /// No `unique_together`, no partial-unique constraint (`issue.py:649-662`
    /// declares `Meta` without either): comments dedupe nowhere at the
    /// DB level. Ported as-is.
    pub const HAS_PARTIAL_UNIQUE: bool = false;

    /// `description` one-to-one: `CASCADE`, nullable (`issue.py:560-562`).
    pub const DESCRIPTION_ON_DELETE: OnDelete = OnDelete::Cascade;
    /// `description` one-to-one `related_name` (`issue.py:561`).
    pub const DESCRIPTION_RELATED_NAME: &str = "issue_comment_description";
    /// `issue` FK: `CASCADE` (`issue.py:567`).
    pub const ISSUE_ON_DELETE: OnDelete = OnDelete::Cascade;
    /// `issue` FK `related_name` (`issue.py:567`).
    pub const ISSUE_RELATED_NAME: &str = "issue_comments";
    /// `actor` FK: `CASCADE`, nullable — the system can also create
    /// comments (`issue.py:568-574`).
    pub const ACTOR_ON_DELETE: OnDelete = OnDelete::Cascade;
    /// `actor` FK `related_name` (`issue.py:572`).
    pub const ACTOR_RELATED_NAME: &str = "comments";
    /// `parent` self FK: `CASCADE`, nullable (`issue.py:592-594`).
    pub const PARENT_ON_DELETE: OnDelete = OnDelete::Cascade;
    /// `parent` self FK `related_name` (`issue.py:593`).
    pub const PARENT_RELATED_NAME: &str = "parent_issue_comment";
    /// `project` FK: `CASCADE` (`project.py:303`).
    pub const PROJECT_ON_DELETE: OnDelete = OnDelete::Cascade;
    /// `workspace` FK: `CASCADE` (`project.py:304`).
    pub const WORKSPACE_ON_DELETE: OnDelete = OnDelete::Cascade;
    /// `created_by` FK: `SET_NULL`, nullable (`mixins.py:29-35`).
    pub const CREATED_BY_ON_DELETE: OnDelete = OnDelete::SetNull;
    /// `updated_by` FK: `SET_NULL`, nullable (`mixins.py:36-42`).
    pub const UPDATED_BY_ON_DELETE: OnDelete = OnDelete::SetNull;

    /// One issue-comment row. `comment_json` stores `{}`, never `NULL`
    /// (`JSONField(default=dict)`); `attachments`/`labels` store `[]`,
    /// never `NULL` (`default=list`); `comment_stripped` is `NOT NULL`
    /// with no field default (the save rule always computes it).
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    pub struct IssueComment {
        pub id: uuid::Uuid,
        pub created_at: chrono::DateTime<chrono::Utc>,
        pub updated_at: chrono::DateTime<chrono::Utc>,
        pub created_by_id: Option<uuid::Uuid>,
        pub updated_by_id: Option<uuid::Uuid>,
        pub deleted_at: Option<chrono::DateTime<chrono::Utc>>,
        pub project_id: uuid::Uuid,
        pub workspace_id: uuid::Uuid,
        pub comment_stripped: String,
        pub comment_json: serde_json::Value,
        pub comment_html: String,
        pub description_id: Option<uuid::Uuid>,
        pub attachments: Vec<String>,
        pub labels: Vec<String>,
        pub issue_id: uuid::Uuid,
        pub actor_id: Option<uuid::Uuid>,
        pub access: String,
        pub external_source: Option<String>,
        pub external_id: Option<String>,
        pub speaker_type: String,
        pub speaker_label: String,
        pub speaker_agent_run_id: Option<uuid::Uuid>,
        pub edited_at: Option<chrono::DateTime<chrono::Utc>>,
        pub parent_id: Option<uuid::Uuid>,
    }

    impl IssueComment {
        /// `__str__` (`issue.py:664-666`): `str(self.issue)` — the label
        /// IS the joined issue's own display (see
        /// `super::super::models_core::issue::Issue` `Display`). Rust
        /// holds only the issue FK, so the label takes the already
        /// rendered issue string; the join is owned by the queries layer.
        pub fn label(issue_str: &str) -> String {
            issue_str.to_string()
        }
    }
}

/// `issue_subscribers` table (`issue.py:700-724`).
pub mod issue_subscriber {
    use super::OnDelete;
    use serde::{Deserialize, Serialize};

    /// Physical table (`Meta.db_table`, `issue.py:719`).
    pub const TABLE: &str = "issue_subscribers";
    /// Default ordering (`Meta.ordering`, `issue.py:720`).
    pub const ORDERING: &str = "-created_at";
    /// `verbose_name` (`issue.py:717`).
    pub const VERBOSE_NAME: &str = "Issue Subscriber";
    /// `verbose_name_plural` (`issue.py:718`).
    pub const VERBOSE_NAME_PLURAL: &str = "Issue Subscribers";

    /// Columns in fixture FX-ISS-08 order: 8 inherited audit/project
    /// columns, then `issue_id` (`issue.py:701`) and `subscriber_id`
    /// (`:702-706`). FK columns use the Django attnames.
    pub const COLUMNS: &[&str] = &[
        "id",
        "created_at",
        "updated_at",
        "created_by_id",
        "updated_by_id",
        "deleted_at",
        "project_id",
        "workspace_id",
        "issue_id",
        "subscriber_id",
    ];

    /// `Meta.unique_together` (`issue.py:709`).
    pub const UNIQUE_TOGETHER: &[&str] = &["issue", "subscriber", "deleted_at"];
    /// Active-unique constraint name (`issue.py:714`).
    pub const UNIQUE_ISSUE_SUBSCRIBER_NAME: &str =
        "issue_subscriber_unique_issue_subscriber_when_deleted_at_null";
    /// Active-unique constraint columns (`issue.py:712`), as attnames.
    pub const UNIQUE_ISSUE_SUBSCRIBER_COLUMNS: &[&str] = &["issue_id", "subscriber_id"];
    /// Active-unique constraint condition (`issue.py:713`,
    /// `Q(deleted_at__isnull=True)`), unquoted semantic form.
    pub const UNIQUE_ISSUE_SUBSCRIBER_WHERE: &str = "deleted_at IS NULL";

    /// `issue` FK: `CASCADE` (`issue.py:701`).
    pub const ISSUE_ON_DELETE: OnDelete = OnDelete::Cascade;
    /// `issue` FK `related_name` (`issue.py:701`).
    pub const ISSUE_RELATED_NAME: &str = "issue_subscribers";
    /// `subscriber` FK: `CASCADE` (`issue.py:702-706`).
    pub const SUBSCRIBER_ON_DELETE: OnDelete = OnDelete::Cascade;
    /// `subscriber` FK `related_name` (`issue.py:705`).
    pub const SUBSCRIBER_RELATED_NAME: &str = "issue_subscribers";
    /// `project` FK: `CASCADE` (`project.py:303`).
    pub const PROJECT_ON_DELETE: OnDelete = OnDelete::Cascade;
    /// `workspace` FK: `CASCADE` (`project.py:304`).
    pub const WORKSPACE_ON_DELETE: OnDelete = OnDelete::Cascade;
    /// `created_by` FK: `SET_NULL`, nullable (`mixins.py:29-35`).
    pub const CREATED_BY_ON_DELETE: OnDelete = OnDelete::SetNull;
    /// `updated_by` FK: `SET_NULL`, nullable (`mixins.py:36-42`).
    pub const UPDATED_BY_ON_DELETE: OnDelete = OnDelete::SetNull;

    /// One issue-subscriber row.
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    pub struct IssueSubscriber {
        pub id: uuid::Uuid,
        pub created_at: chrono::DateTime<chrono::Utc>,
        pub updated_at: chrono::DateTime<chrono::Utc>,
        pub created_by_id: Option<uuid::Uuid>,
        pub updated_by_id: Option<uuid::Uuid>,
        pub deleted_at: Option<chrono::DateTime<chrono::Utc>>,
        pub project_id: uuid::Uuid,
        pub workspace_id: uuid::Uuid,
        pub issue_id: uuid::Uuid,
        pub subscriber_id: uuid::Uuid,
    }

    impl IssueSubscriber {
        /// `__str__` (`issue.py:722-723`):
        /// `"{issue.name} {subscriber.email}"`. Rust holds only the
        /// FKs, so the label takes the joined names; the joins are owned
        /// by the queries layer.
        pub fn label(issue_name: &str, subscriber_email: &str) -> String {
            format!("{issue_name} {subscriber_email}")
        }
    }
}

/// `issue_reactions` table (`issue.py:726-751`).
pub mod issue_reaction {
    use super::OnDelete;
    use serde::{Deserialize, Serialize};

    /// Physical table (`Meta.db_table`, `issue.py:746`).
    pub const TABLE: &str = "issue_reactions";
    /// Default ordering (`Meta.ordering`, `issue.py:747`).
    pub const ORDERING: &str = "-created_at";
    /// `verbose_name` (`issue.py:744`).
    pub const VERBOSE_NAME: &str = "Issue Reaction";
    /// `verbose_name_plural` (`issue.py:745`).
    pub const VERBOSE_NAME_PLURAL: &str = "Issue Reactions";

    /// Columns in fixture FX-ISS-08 order: 8 inherited audit/project
    /// columns, then `actor_id` (`issue.py:727-731`), `issue_id` (`:732`)
    /// and `reaction` (`:733`). FK columns use the Django attnames.
    pub const COLUMNS: &[&str] = &[
        "id",
        "created_at",
        "updated_at",
        "created_by_id",
        "updated_by_id",
        "deleted_at",
        "project_id",
        "workspace_id",
        "actor_id",
        "issue_id",
        "reaction",
    ];

    /// `Meta.unique_together` (`issue.py:736`).
    pub const UNIQUE_TOGETHER: &[&str] = &["issue", "actor", "reaction", "deleted_at"];
    /// Active-unique constraint name (`issue.py:741`).
    pub const UNIQUE_ISSUE_ACTOR_NAME: &str =
        "issue_reaction_unique_issue_actor_reaction_when_deleted_at_null";
    /// Active-unique constraint columns (`issue.py:739`), as attnames.
    pub const UNIQUE_ISSUE_ACTOR_COLUMNS: &[&str] = &["issue_id", "actor_id", "reaction"];
    /// Active-unique constraint condition (`issue.py:740`,
    /// `Q(deleted_at__isnull=True)`), unquoted semantic form.
    pub const UNIQUE_ISSUE_ACTOR_WHERE: &str = "deleted_at IS NULL";

    /// `actor` FK: `CASCADE` (`issue.py:727-731`).
    pub const ACTOR_ON_DELETE: OnDelete = OnDelete::Cascade;
    /// `actor` FK `related_name` (`issue.py:730`).
    pub const ACTOR_RELATED_NAME: &str = "issue_reactions";
    /// `issue` FK: `CASCADE` (`issue.py:732`).
    pub const ISSUE_ON_DELETE: OnDelete = OnDelete::Cascade;
    /// `issue` FK `related_name` (`issue.py:732`).
    pub const ISSUE_RELATED_NAME: &str = "issue_reactions";
    /// `project` FK: `CASCADE` (`project.py:303`).
    pub const PROJECT_ON_DELETE: OnDelete = OnDelete::Cascade;
    /// `workspace` FK: `CASCADE` (`project.py:304`).
    pub const WORKSPACE_ON_DELETE: OnDelete = OnDelete::Cascade;
    /// `created_by` FK: `SET_NULL`, nullable (`mixins.py:29-35`).
    pub const CREATED_BY_ON_DELETE: OnDelete = OnDelete::SetNull;
    /// `updated_by` FK: `SET_NULL`, nullable (`mixins.py:36-42`).
    pub const UPDATED_BY_ON_DELETE: OnDelete = OnDelete::SetNull;

    /// One issue-reaction row. `reaction` is a required `TextField` (no
    /// max length).
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    pub struct IssueReaction {
        pub id: uuid::Uuid,
        pub created_at: chrono::DateTime<chrono::Utc>,
        pub updated_at: chrono::DateTime<chrono::Utc>,
        pub created_by_id: Option<uuid::Uuid>,
        pub updated_by_id: Option<uuid::Uuid>,
        pub deleted_at: Option<chrono::DateTime<chrono::Utc>>,
        pub project_id: uuid::Uuid,
        pub workspace_id: uuid::Uuid,
        pub actor_id: uuid::Uuid,
        pub issue_id: uuid::Uuid,
        pub reaction: String,
    }

    impl IssueReaction {
        /// `__str__` (`issue.py:749-750`):
        /// `"{issue.name} {actor.email}"`. Rust holds only the FKs, so
        /// the label takes the joined values; the joins are owned by the
        /// queries layer.
        pub fn label(issue_name: &str, actor_email: &str) -> String {
            format!("{issue_name} {actor_email}")
        }
    }
}

/// `comment_reactions` table (`issue.py:753-778`).
pub mod comment_reaction {
    use super::OnDelete;
    use serde::{Deserialize, Serialize};

    /// Physical table (`Meta.db_table`, `issue.py:773`).
    pub const TABLE: &str = "comment_reactions";
    /// Default ordering (`Meta.ordering`, `issue.py:774`).
    pub const ORDERING: &str = "-created_at";
    /// `verbose_name` (`issue.py:771`).
    pub const VERBOSE_NAME: &str = "Comment Reaction";
    /// `verbose_name_plural` (`issue.py:772`).
    pub const VERBOSE_NAME_PLURAL: &str = "Comment Reactions";

    /// Columns in fixture FX-ISS-08 order: 8 inherited audit/project
    /// columns, then `actor_id` (`issue.py:754-758`), `comment_id` (`:759`)
    /// and `reaction` (`:760`). FK columns use the Django attnames.
    pub const COLUMNS: &[&str] = &[
        "id",
        "created_at",
        "updated_at",
        "created_by_id",
        "updated_by_id",
        "deleted_at",
        "project_id",
        "workspace_id",
        "actor_id",
        "comment_id",
        "reaction",
    ];

    /// `Meta.unique_together` (`issue.py:763`).
    pub const UNIQUE_TOGETHER: &[&str] = &["comment", "actor", "reaction", "deleted_at"];
    /// Active-unique constraint name (`issue.py:768`).
    pub const UNIQUE_COMMENT_ACTOR_NAME: &str =
        "comment_reaction_unique_comment_actor_reaction_when_deleted_at_null";
    /// Active-unique constraint columns (`issue.py:766`), as attnames.
    pub const UNIQUE_COMMENT_ACTOR_COLUMNS: &[&str] = &["comment_id", "actor_id", "reaction"];
    /// Active-unique constraint condition (`issue.py:767`,
    /// `Q(deleted_at__isnull=True)`), unquoted semantic form.
    pub const UNIQUE_COMMENT_ACTOR_WHERE: &str = "deleted_at IS NULL";

    /// `actor` FK: `CASCADE` (`issue.py:754-758`).
    pub const ACTOR_ON_DELETE: OnDelete = OnDelete::Cascade;
    /// `actor` FK `related_name` (`issue.py:757`).
    pub const ACTOR_RELATED_NAME: &str = "comment_reactions";
    /// `comment` FK: `CASCADE` (`issue.py:759`).
    pub const COMMENT_ON_DELETE: OnDelete = OnDelete::Cascade;
    /// `comment` FK `related_name` (`issue.py:759`).
    pub const COMMENT_RELATED_NAME: &str = "comment_reactions";
    /// `project` FK: `CASCADE` (`project.py:303`).
    pub const PROJECT_ON_DELETE: OnDelete = OnDelete::Cascade;
    /// `workspace` FK: `CASCADE` (`project.py:304`).
    pub const WORKSPACE_ON_DELETE: OnDelete = OnDelete::Cascade;
    /// `created_by` FK: `SET_NULL`, nullable (`mixins.py:29-35`).
    pub const CREATED_BY_ON_DELETE: OnDelete = OnDelete::SetNull;
    /// `updated_by` FK: `SET_NULL`, nullable (`mixins.py:36-42`).
    pub const UPDATED_BY_ON_DELETE: OnDelete = OnDelete::SetNull;

    /// One comment-reaction row. `reaction` is a required `TextField` (no
    /// max length).
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    pub struct CommentReaction {
        pub id: uuid::Uuid,
        pub created_at: chrono::DateTime<chrono::Utc>,
        pub updated_at: chrono::DateTime<chrono::Utc>,
        pub created_by_id: Option<uuid::Uuid>,
        pub updated_by_id: Option<uuid::Uuid>,
        pub deleted_at: Option<chrono::DateTime<chrono::Utc>>,
        pub project_id: uuid::Uuid,
        pub workspace_id: uuid::Uuid,
        pub actor_id: uuid::Uuid,
        pub comment_id: uuid::Uuid,
        pub reaction: String,
    }

    impl std::fmt::Display for CommentReaction {
        /// `__str__` (`issue.py:776-777`,
        /// `"{self.issue.name} {self.actor.email}"`). Ported bug: the
        /// model has no `issue` field, so Python raises `AttributeError`
        /// on every call (see the module docs). The row carries only the
        /// FKs, so this renders the ids instead of raising.
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "{} {}", self.comment_id, self.actor_id)
        }
    }
}

/// `issue_votes` table (`issue.py:780-801`).
pub mod issue_vote {
    use super::OnDelete;
    use serde::{Deserialize, Serialize};

    /// Physical table (`Meta.db_table`, `issue.py:796`).
    pub const TABLE: &str = "issue_votes";
    /// Default ordering (`Meta.ordering`, `issue.py:797`).
    pub const ORDERING: &str = "-created_at";
    /// `verbose_name` (`issue.py:794`).
    pub const VERBOSE_NAME: &str = "Issue Vote";
    /// `verbose_name_plural` (`issue.py:795`).
    pub const VERBOSE_NAME_PLURAL: &str = "Issue Votes";

    /// Columns in fixture FX-ISS-08 order: 8 inherited audit/project
    /// columns, then `issue_id` (`issue.py:781`), `actor_id` (`:782`) and
    /// `vote` (`:783`). FK columns use the Django attnames.
    pub const COLUMNS: &[&str] = &[
        "id",
        "created_at",
        "updated_at",
        "created_by_id",
        "updated_by_id",
        "deleted_at",
        "project_id",
        "workspace_id",
        "issue_id",
        "actor_id",
        "vote",
    ];

    /// `vote` choices (`issue.py:783`, `IntegerField(choices=((-1,
    /// "DOWNVOTE"), (1, "UPVOTE")))`).
    pub const VOTE_CHOICES: &[(i32, &str)] = &[(-1, "DOWNVOTE"), (1, "UPVOTE")];
    /// `vote` default (`issue.py:783`, `default=1`).
    pub const VOTE_DEFAULT: i32 = 1;

    /// `Meta.unique_together` (`issue.py:786`).
    pub const UNIQUE_TOGETHER: &[&str] = &["issue", "actor", "deleted_at"];
    /// Active-unique constraint name (`issue.py:791`).
    pub const UNIQUE_ISSUE_ACTOR_NAME: &str = "issue_vote_unique_issue_actor_when_deleted_at_null";
    /// Active-unique constraint columns (`issue.py:789`), as attnames.
    pub const UNIQUE_ISSUE_ACTOR_COLUMNS: &[&str] = &["issue_id", "actor_id"];
    /// Active-unique constraint condition (`issue.py:790`,
    /// `Q(deleted_at__isnull=True)`), unquoted semantic form.
    pub const UNIQUE_ISSUE_ACTOR_WHERE: &str = "deleted_at IS NULL";

    /// `issue` FK: `CASCADE` (`issue.py:781`).
    pub const ISSUE_ON_DELETE: OnDelete = OnDelete::Cascade;
    /// `issue` FK `related_name` (`issue.py:781`).
    pub const ISSUE_RELATED_NAME: &str = "votes";
    /// `actor` FK: `CASCADE` (`issue.py:782`).
    pub const ACTOR_ON_DELETE: OnDelete = OnDelete::Cascade;
    /// `actor` FK `related_name` (`issue.py:782`).
    pub const ACTOR_RELATED_NAME: &str = "votes";
    /// `project` FK: `CASCADE` (`project.py:303`).
    pub const PROJECT_ON_DELETE: OnDelete = OnDelete::Cascade;
    /// `workspace` FK: `CASCADE` (`project.py:304`).
    pub const WORKSPACE_ON_DELETE: OnDelete = OnDelete::Cascade;
    /// `created_by` FK: `SET_NULL`, nullable (`mixins.py:29-35`).
    pub const CREATED_BY_ON_DELETE: OnDelete = OnDelete::SetNull;
    /// `updated_by` FK: `SET_NULL`, nullable (`mixins.py:36-42`).
    pub const UPDATED_BY_ON_DELETE: OnDelete = OnDelete::SetNull;

    /// One issue-vote row.
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    pub struct IssueVote {
        pub id: uuid::Uuid,
        pub created_at: chrono::DateTime<chrono::Utc>,
        pub updated_at: chrono::DateTime<chrono::Utc>,
        pub created_by_id: Option<uuid::Uuid>,
        pub updated_by_id: Option<uuid::Uuid>,
        pub deleted_at: Option<chrono::DateTime<chrono::Utc>>,
        pub project_id: uuid::Uuid,
        pub workspace_id: uuid::Uuid,
        pub issue_id: uuid::Uuid,
        pub actor_id: uuid::Uuid,
        pub vote: i32,
    }

    impl IssueVote {
        /// `__str__` (`issue.py:799-800`):
        /// `"{issue.name} {actor.email}"`. Rust holds only the FKs, so
        /// the label takes the joined values; the joins are owned by the
        /// queries layer.
        pub fn label(issue_name: &str, actor_email: &str) -> String {
            format!("{issue_name} {actor_email}")
        }
    }
}

/// Foreign-table columns D-26 reads that have no merged db def yet
/// (fixture FX-ISS-08 `foreign_reads`).
///
/// `IssueComment.save()` creates and updates rows on `descriptions`
/// (`issue.py:609-647`); the queries layer reads the columns below. The
/// owning `Description` port belongs to PIDASHCONV-647 (FX-ISS-10), so
/// the table name and read columns are pinned here as SQL literals; query
/// builders copy these literals verbatim.
pub mod foreign {
    /// Physical `descriptions` table (`db/models/description.py:10-20`,
    /// `Description(WorkspaceBaseModel)`).
    pub const DESCRIPTION_TABLE: &str = "descriptions";
    /// Columns D-26 reads on [`DESCRIPTION_TABLE`], fixture order.
    pub const DESCRIPTION_COLUMNS_READ: &[&str] = &[
        "id",
        "workspace_id",
        "project_id",
        "description_json",
        "description_html",
        "description_binary",
        "description_stripped",
    ];
}

#[cfg(test)]
mod tests {
    use super::comment_reaction;
    use super::foreign;
    use super::issue_comment;
    use super::issue_reaction;
    use super::issue_subscriber;
    use super::issue_vote;
    use super::OnDelete;
    use crate::app_issues::models_core;
    use crate::app_views_search::models::issue_comment_ref;
    use crate::soft_delete::active_condition;
    use crate::space::columns as space_columns;
    use sea_query::{Alias, PostgresQueryBuilder, Query};

    fn fixtures_dir() -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/app_issues/models")
    }

    fn fixture() -> serde_json::Value {
        let path = fixtures_dir().join("FX-ISS-08.engage.json");
        let body = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read FX-ISS-08: {e}"));
        serde_json::from_str(&body).expect("FX-ISS-08.engage.json is valid JSON")
    }

    /// Copy a const into an owned vec so assertions compare two runtime
    /// values (clippy denies asserting on constants directly).
    fn owned(cols: &[&str]) -> Vec<String> {
        cols.iter().map(|c| (*c).to_string()).collect()
    }

    /// `issue_comment.columns[].name` in order (the object-shaped entries;
    /// combined `"<a>/<b>"` names expand like the compact string heads).
    fn object_column_names(value: &serde_json::Value) -> Vec<String> {
        value["issue_comment"]["columns"]
            .as_array()
            .expect("issue_comment has columns array")
            .iter()
            .flat_map(|c| {
                let name = c["name"].as_str().expect("column entry has name");
                name.split('/').map(str::to_string).collect::<Vec<_>>()
            })
            .collect()
    }

    /// Nullable column names from the object-shaped entries: an entry is
    /// nullable when it carries `"null": true` or its `type` marks `NULL`
    /// (vs `NOT NULL` or the bare `PK`).
    fn object_nullable(value: &serde_json::Value) -> Vec<String> {
        value["issue_comment"]["columns"]
            .as_array()
            .expect("issue_comment has columns array")
            .iter()
            .flat_map(|c| {
                let name = c["name"].as_str().expect("column entry has name");
                let flag = c["null"].as_bool().unwrap_or(false);
                let text = c["type"].as_str().expect("column entry has type");
                let marked = text.contains("NULL") && !text.contains("NOT NULL");
                if flag || marked {
                    name.split('/').map(str::to_string).collect::<Vec<_>>()
                } else {
                    Vec::new()
                }
            })
            .collect()
    }

    /// Find an `issue_comment.columns` entry by (possibly combined) name.
    fn object_entry<'a>(value: &'a serde_json::Value, name: &str) -> &'a serde_json::Value {
        value["issue_comment"]["columns"]
            .as_array()
            .expect("issue_comment has columns array")
            .iter()
            .find(|c| c["name"].as_str() == Some(name))
            .unwrap_or_else(|| panic!("issue_comment fixture has column {name}"))
    }

    /// Expand the compact string-list entries (`"<a>/<b> <type> …"`)
    /// into column names in order.
    fn string_column_names(value: &serde_json::Value, model: &str) -> Vec<String> {
        value[model]["columns"]
            .as_array()
            .unwrap_or_else(|| panic!("{model} has columns array"))
            .iter()
            .flat_map(|c| {
                let s = c
                    .as_str()
                    .unwrap_or_else(|| panic!("{model} column entry is str"));
                let head = s.split_whitespace().next().expect("entry has head");
                head.split('/').map(str::to_string).collect::<Vec<_>>()
            })
            .collect()
    }

    /// Nullable column names from the compact string-list entries: an
    /// entry marks `NULL` (vs `NOT NULL` or the bare `PK`).
    fn string_nullable(value: &serde_json::Value, model: &str) -> Vec<String> {
        value[model]["columns"]
            .as_array()
            .unwrap_or_else(|| panic!("{model} has columns array"))
            .iter()
            .flat_map(|c| {
                let s = c.as_str().expect("column entry is str");
                let nullable = s.contains("NULL") && !s.contains("NOT NULL");
                let head = s.split_whitespace().next().expect("entry has head");
                if nullable {
                    head.split('/').map(str::to_string).collect::<Vec<_>>()
                } else {
                    Vec::new()
                }
            })
            .collect()
    }

    fn constraint_strings(value: &serde_json::Value, model: &str) -> Vec<String> {
        value[model]["constraints"]
            .as_array()
            .unwrap_or_else(|| panic!("{model} has constraints"))
            .iter()
            .map(|c| c.as_str().expect("constraint is str").to_string())
            .collect()
    }

    fn comment_constraint_strings(value: &serde_json::Value) -> Vec<String> {
        value["issue_comment"]["constraints_indexes"]
            .as_array()
            .expect("issue_comment has constraints_indexes")
            .iter()
            .map(|c| c.as_str().expect("entry is str").to_string())
            .collect()
    }

    fn foreign_columns_read(value: &serde_json::Value, table: &str) -> Vec<String> {
        value["foreign_reads"][table]["columns_read"]
            .as_array()
            .unwrap_or_else(|| panic!("foreign_reads has {table}"))
            .iter()
            .map(|c| c.as_str().expect("columns_read entry is str").to_string())
            .collect()
    }

    #[test]
    fn fixture_loads() {
        let v = fixture();
        assert_eq!(v["_fixture"].as_str().unwrap(), "FX-ISS-08");
        assert_eq!(
            v["_source"]["consumers"][0].as_str().unwrap(),
            "PIDASHCONV-646 (models_engage.rs)"
        );
    }

    #[test]
    fn issue_comment_columns_match_fixture() {
        let v = fixture();
        assert_eq!(owned(issue_comment::COLUMNS), object_column_names(&v));
        assert_eq!(issue_comment::COLUMNS.len(), 24);
        // Same 8 inherited audit/project columns as the core issue table.
        assert_eq!(
            owned(&issue_comment::COLUMNS[..8]),
            owned(&models_core::issue::COLUMNS[..8])
        );
        assert_eq!(
            owned(&issue_comment::COLUMNS[8..]),
            vec![
                "comment_stripped",
                "comment_json",
                "comment_html",
                "description_id",
                "attachments",
                "labels",
                "issue_id",
                "actor_id",
                "access",
                "external_source",
                "external_id",
                "speaker_type",
                "speaker_label",
                "speaker_agent_run_id",
                "edited_at",
                "parent_id",
            ]
        );
        let table: &str = issue_comment::TABLE;
        assert_eq!(table, v["issue_comment"]["table"].as_str().unwrap());
        assert_eq!(table, "issue_comments");
        assert_eq!(issue_comment::ORDERING, "-created_at");
        assert_eq!(issue_comment::VERBOSE_NAME, "Issue Comment");
        assert_eq!(issue_comment::VERBOSE_NAME_PLURAL, "Issue Comments");
        assert_eq!(
            object_nullable(&v),
            vec![
                "created_by_id",
                "updated_by_id",
                "deleted_at",
                "description_id",
                "actor_id",
                "external_source",
                "external_id",
                "speaker_agent_run_id",
                "edited_at",
                "parent_id",
            ]
        );
        // Choices, bounds, defaults.
        assert_eq!(
            issue_comment::SPEAKER_TYPE_CHOICES,
            &[
                ("human", "Human"),
                ("agent", "Agent"),
                ("system", "System"),
                ("integration", "Integration"),
            ]
        );
        assert_eq!(issue_comment::SPEAKER_TYPE_DEFAULT, "human");
        assert_eq!(issue_comment::SPEAKER_TYPE_MAX_LENGTH, 32);
        assert_eq!(issue_comment::SPEAKER_LABEL_MAX_LENGTH, 128);
        assert_eq!(issue_comment::SPEAKER_LABEL_DEFAULT, "");
        assert_eq!(
            issue_comment::ACCESS_CHOICES,
            &[("INTERNAL", "INTERNAL"), ("EXTERNAL", "EXTERNAL")]
        );
        assert_eq!(issue_comment::ACCESS_DEFAULT, "INTERNAL");
        assert_eq!(issue_comment::ACCESS_MAX_LENGTH, 100);
        assert_eq!(issue_comment::EXTERNAL_SOURCE_MAX_LENGTH, 255);
        assert_eq!(issue_comment::EXTERNAL_ID_MAX_LENGTH, 255);
        assert_eq!(issue_comment::COMMENT_HTML_DEFAULT, "<p></p>");
        assert_eq!(issue_comment::ATTACHMENTS_SIZE, 10);
        assert_eq!(issue_comment::LABELS_SIZE, 8);
        assert_eq!(issue_comment::LABEL_ELEMENT_MAX_LENGTH, 32);
        assert_eq!(issue_comment::default_comment_json(), serde_json::json!({}));
        let attachments: Vec<String> = issue_comment::default_attachments();
        assert!(attachments.is_empty());
        let labels: Vec<String> = issue_comment::default_labels();
        assert!(labels.is_empty());
        // The fixture corroborates the attachment/label shapes.
        let attachments_text = object_entry(&v, "attachments")["type"].as_str().unwrap();
        assert!(attachments_text.contains("varchar[]"));
        assert!(attachments_text.contains("size 10"));
        let labels_text = object_entry(&v, "labels")["type"].as_str().unwrap();
        assert!(labels_text.contains("varchar(32)[]"));
        assert!(labels_text.contains("size 8"));
        // comment_stripped: NOT NULL with no field default — the fixture's
        // `default ''` is the save()-computed floor, not a `default=`.
        let stripped_text = object_entry(&v, "comment_stripped")["type"]
            .as_str()
            .unwrap();
        assert!(stripped_text.contains("NOT NULL"));
        let save_text = v["issue_comment"]["save"].as_str().unwrap();
        assert!(save_text.contains("strip comment_stripped"));
        assert!(save_text.contains("TRACKED_FIELDS"));
        // Save halves.
        assert_eq!(
            issue_comment::TRACKED_FIELDS,
            &["comment_stripped", "comment_json", "comment_html"]
        );
        assert_eq!(
            issue_comment::COMMENT_TO_DESCRIPTION,
            &[
                ("comment_html", "description_html"),
                ("comment_stripped", "description_stripped"),
                ("comment_json", "description_json"),
            ]
        );
        assert_eq!(
            issue_comment::DESCRIPTION_DEFAULT_KEYS,
            &[
                "workspace_id",
                "project_id",
                "created_by_id",
                "updated_by_id",
                "description_stripped",
                "description_json",
                "description_html",
            ]
        );
        assert_eq!(
            issue_comment::DESCRIPTION_UPDATE_TOUCHED,
            &["updated_by_id", "updated_at"]
        );
        assert!(issue_comment::creates_description_on_save(true, None));
        assert!(issue_comment::creates_description_on_save(
            true,
            Some(uuid::Uuid::nil())
        ));
        assert!(issue_comment::creates_description_on_save(false, None));
        assert!(!issue_comment::creates_description_on_save(
            false,
            Some(uuid::Uuid::nil())
        ));
        assert_eq!(
            issue_comment::description_field_for("comment_html"),
            Some("description_html")
        );
        assert_eq!(
            issue_comment::description_field_for("comment_stripped"),
            Some("description_stripped")
        );
        assert_eq!(
            issue_comment::description_field_for("comment_json"),
            Some("description_json")
        );
        assert_eq!(issue_comment::description_field_for("access"), None);
        // The strip rule itself is the merged D-32 port; pin its goldens.
        assert_eq!(issue_comment_ref::sync_comment_stripped(""), "");
        assert_eq!(issue_comment_ref::sync_comment_stripped("<p>hi</p>"), "hi");
        // Index + ordering, no uniqueness.
        let constraints = comment_constraint_strings(&v);
        assert_eq!(constraints.len(), 2);
        assert!(constraints[0].contains("issue_comments_fts_idx"));
        assert!(constraints[0].contains("comment_stripped"));
        assert!(constraints[1].contains("-created_at"));
        assert_eq!(issue_comment::FTS_INDEX_NAME, "issue_comments_fts_idx");
        assert_eq!(issue_comment::FTS_CONFIG, "english");
        assert_eq!(issue_comment::FTS_SOURCE_COLUMNS, &["comment_stripped"]);
        let has_partial_unique: bool = issue_comment::HAS_PARTIAL_UNIQUE;
        assert!(!has_partial_unique);
        assert_eq!(issue_comment::DESCRIPTION_ON_DELETE, OnDelete::Cascade);
        assert_eq!(
            issue_comment::DESCRIPTION_RELATED_NAME,
            "issue_comment_description"
        );
        assert_eq!(issue_comment::ISSUE_ON_DELETE, OnDelete::Cascade);
        assert_eq!(issue_comment::ISSUE_RELATED_NAME, "issue_comments");
        assert_eq!(issue_comment::ACTOR_ON_DELETE, OnDelete::Cascade);
        assert_eq!(issue_comment::ACTOR_RELATED_NAME, "comments");
        assert_eq!(issue_comment::PARENT_ON_DELETE, OnDelete::Cascade);
        assert_eq!(issue_comment::PARENT_RELATED_NAME, "parent_issue_comment");
        assert_eq!(issue_comment::PROJECT_ON_DELETE, OnDelete::Cascade);
        assert_eq!(issue_comment::WORKSPACE_ON_DELETE, OnDelete::Cascade);
        assert_eq!(issue_comment::CREATED_BY_ON_DELETE, OnDelete::SetNull);
        assert_eq!(issue_comment::UPDATED_BY_ON_DELETE, OnDelete::SetNull);
    }

    #[test]
    fn issue_subscriber_columns_match_fixture() {
        let v = fixture();
        assert_eq!(
            owned(issue_subscriber::COLUMNS),
            string_column_names(&v, "issue_subscriber")
        );
        assert_eq!(issue_subscriber::COLUMNS.len(), 10);
        assert_eq!(
            owned(&issue_subscriber::COLUMNS[..8]),
            owned(&models_core::issue::COLUMNS[..8])
        );
        assert_eq!(
            owned(&issue_subscriber::COLUMNS[8..]),
            vec!["issue_id", "subscriber_id"]
        );
        let table: &str = issue_subscriber::TABLE;
        assert_eq!(table, v["issue_subscriber"]["table"].as_str().unwrap());
        assert_eq!(table, "issue_subscribers");
        assert_eq!(issue_subscriber::ORDERING, "-created_at");
        assert_eq!(issue_subscriber::VERBOSE_NAME, "Issue Subscriber");
        assert_eq!(issue_subscriber::VERBOSE_NAME_PLURAL, "Issue Subscribers");
        assert_eq!(
            string_nullable(&v, "issue_subscriber"),
            vec!["created_by_id", "updated_by_id", "deleted_at"]
        );
        let constraints = constraint_strings(&v, "issue_subscriber");
        assert_eq!(constraints.len(), 1);
        assert!(constraints[0].contains("(issue,subscriber)"));
        assert!(constraints[0].contains("deleted_at IS NULL"));
        assert_eq!(
            issue_subscriber::UNIQUE_TOGETHER,
            &["issue", "subscriber", "deleted_at"]
        );
        assert_eq!(
            issue_subscriber::UNIQUE_ISSUE_SUBSCRIBER_NAME,
            "issue_subscriber_unique_issue_subscriber_when_deleted_at_null"
        );
        assert_eq!(
            issue_subscriber::UNIQUE_ISSUE_SUBSCRIBER_COLUMNS,
            &["issue_id", "subscriber_id"]
        );
        assert_eq!(
            issue_subscriber::UNIQUE_ISSUE_SUBSCRIBER_WHERE,
            "deleted_at IS NULL"
        );
        assert_eq!(issue_subscriber::ISSUE_ON_DELETE, OnDelete::Cascade);
        assert_eq!(issue_subscriber::ISSUE_RELATED_NAME, "issue_subscribers");
        assert_eq!(issue_subscriber::SUBSCRIBER_ON_DELETE, OnDelete::Cascade);
        assert_eq!(
            issue_subscriber::SUBSCRIBER_RELATED_NAME,
            "issue_subscribers"
        );
        assert_eq!(issue_subscriber::PROJECT_ON_DELETE, OnDelete::Cascade);
        assert_eq!(issue_subscriber::WORKSPACE_ON_DELETE, OnDelete::Cascade);
        assert_eq!(issue_subscriber::CREATED_BY_ON_DELETE, OnDelete::SetNull);
        assert_eq!(issue_subscriber::UPDATED_BY_ON_DELETE, OnDelete::SetNull);
    }

    #[test]
    fn issue_reaction_columns_match_fixture() {
        let v = fixture();
        assert_eq!(
            owned(issue_reaction::COLUMNS),
            string_column_names(&v, "issue_reaction")
        );
        assert_eq!(issue_reaction::COLUMNS.len(), 11);
        assert_eq!(
            owned(&issue_reaction::COLUMNS[..8]),
            owned(&models_core::issue::COLUMNS[..8])
        );
        assert_eq!(
            owned(&issue_reaction::COLUMNS[8..]),
            vec!["actor_id", "issue_id", "reaction"]
        );
        let table: &str = issue_reaction::TABLE;
        assert_eq!(table, v["issue_reaction"]["table"].as_str().unwrap());
        assert_eq!(table, "issue_reactions");
        assert_eq!(issue_reaction::ORDERING, "-created_at");
        assert_eq!(issue_reaction::VERBOSE_NAME, "Issue Reaction");
        assert_eq!(issue_reaction::VERBOSE_NAME_PLURAL, "Issue Reactions");
        assert_eq!(
            string_nullable(&v, "issue_reaction"),
            vec!["created_by_id", "updated_by_id", "deleted_at"]
        );
        let constraints = constraint_strings(&v, "issue_reaction");
        assert_eq!(constraints.len(), 1);
        assert!(constraints[0].contains("(issue,actor,reaction)"));
        assert!(constraints[0].contains("deleted_at IS NULL"));
        assert_eq!(
            issue_reaction::UNIQUE_TOGETHER,
            &["issue", "actor", "reaction", "deleted_at"]
        );
        assert_eq!(
            issue_reaction::UNIQUE_ISSUE_ACTOR_NAME,
            "issue_reaction_unique_issue_actor_reaction_when_deleted_at_null"
        );
        assert_eq!(
            issue_reaction::UNIQUE_ISSUE_ACTOR_COLUMNS,
            &["issue_id", "actor_id", "reaction"]
        );
        assert_eq!(
            issue_reaction::UNIQUE_ISSUE_ACTOR_WHERE,
            "deleted_at IS NULL"
        );
        assert_eq!(issue_reaction::ACTOR_ON_DELETE, OnDelete::Cascade);
        assert_eq!(issue_reaction::ACTOR_RELATED_NAME, "issue_reactions");
        assert_eq!(issue_reaction::ISSUE_ON_DELETE, OnDelete::Cascade);
        assert_eq!(issue_reaction::ISSUE_RELATED_NAME, "issue_reactions");
        assert_eq!(issue_reaction::PROJECT_ON_DELETE, OnDelete::Cascade);
        assert_eq!(issue_reaction::WORKSPACE_ON_DELETE, OnDelete::Cascade);
        assert_eq!(issue_reaction::CREATED_BY_ON_DELETE, OnDelete::SetNull);
        assert_eq!(issue_reaction::UPDATED_BY_ON_DELETE, OnDelete::SetNull);
    }

    #[test]
    fn comment_reaction_columns_match_fixture() {
        let v = fixture();
        assert_eq!(
            owned(comment_reaction::COLUMNS),
            string_column_names(&v, "comment_reaction")
        );
        assert_eq!(comment_reaction::COLUMNS.len(), 11);
        assert_eq!(
            owned(&comment_reaction::COLUMNS[..8]),
            owned(&models_core::issue::COLUMNS[..8])
        );
        assert_eq!(
            owned(&comment_reaction::COLUMNS[8..]),
            vec!["actor_id", "comment_id", "reaction"]
        );
        let table: &str = comment_reaction::TABLE;
        assert_eq!(table, v["comment_reaction"]["table"].as_str().unwrap());
        assert_eq!(table, "comment_reactions");
        assert_eq!(comment_reaction::ORDERING, "-created_at");
        assert_eq!(comment_reaction::VERBOSE_NAME, "Comment Reaction");
        assert_eq!(comment_reaction::VERBOSE_NAME_PLURAL, "Comment Reactions");
        assert_eq!(
            string_nullable(&v, "comment_reaction"),
            vec!["created_by_id", "updated_by_id", "deleted_at"]
        );
        let constraints = constraint_strings(&v, "comment_reaction");
        assert_eq!(constraints.len(), 1);
        assert!(constraints[0].contains("(comment,actor,reaction)"));
        assert!(constraints[0].contains("deleted_at IS NULL"));
        // The duplicate-create view mapping is handler-layer behavior; the
        // models layer pins only that the constraint behind it exists.
        assert!(constraints[0].contains("Reaction already exists"));
        assert_eq!(
            comment_reaction::UNIQUE_TOGETHER,
            &["comment", "actor", "reaction", "deleted_at"]
        );
        assert_eq!(
            comment_reaction::UNIQUE_COMMENT_ACTOR_NAME,
            "comment_reaction_unique_comment_actor_reaction_when_deleted_at_null"
        );
        assert_eq!(
            comment_reaction::UNIQUE_COMMENT_ACTOR_COLUMNS,
            &["comment_id", "actor_id", "reaction"]
        );
        assert_eq!(
            comment_reaction::UNIQUE_COMMENT_ACTOR_WHERE,
            "deleted_at IS NULL"
        );
        assert_eq!(comment_reaction::ACTOR_ON_DELETE, OnDelete::Cascade);
        assert_eq!(comment_reaction::ACTOR_RELATED_NAME, "comment_reactions");
        assert_eq!(comment_reaction::COMMENT_ON_DELETE, OnDelete::Cascade);
        assert_eq!(comment_reaction::COMMENT_RELATED_NAME, "comment_reactions");
        assert_eq!(comment_reaction::PROJECT_ON_DELETE, OnDelete::Cascade);
        assert_eq!(comment_reaction::WORKSPACE_ON_DELETE, OnDelete::Cascade);
        assert_eq!(comment_reaction::CREATED_BY_ON_DELETE, OnDelete::SetNull);
        assert_eq!(comment_reaction::UPDATED_BY_ON_DELETE, OnDelete::SetNull);
    }

    #[test]
    fn issue_vote_columns_match_fixture() {
        let v = fixture();
        assert_eq!(
            owned(issue_vote::COLUMNS),
            string_column_names(&v, "issue_vote")
        );
        assert_eq!(issue_vote::COLUMNS.len(), 11);
        assert_eq!(
            owned(&issue_vote::COLUMNS[..8]),
            owned(&models_core::issue::COLUMNS[..8])
        );
        assert_eq!(
            owned(&issue_vote::COLUMNS[8..]),
            vec!["issue_id", "actor_id", "vote"]
        );
        let table: &str = issue_vote::TABLE;
        assert_eq!(table, v["issue_vote"]["table"].as_str().unwrap());
        assert_eq!(table, "issue_votes");
        assert_eq!(issue_vote::ORDERING, "-created_at");
        assert_eq!(issue_vote::VERBOSE_NAME, "Issue Vote");
        assert_eq!(issue_vote::VERBOSE_NAME_PLURAL, "Issue Votes");
        assert_eq!(
            string_nullable(&v, "issue_vote"),
            vec!["created_by_id", "updated_by_id", "deleted_at"]
        );
        assert_eq!(issue_vote::VOTE_CHOICES, &[(-1, "DOWNVOTE"), (1, "UPVOTE")]);
        assert_eq!(issue_vote::VOTE_DEFAULT, 1);
        let constraints = constraint_strings(&v, "issue_vote");
        assert_eq!(constraints.len(), 1);
        assert!(constraints[0].contains("(issue,actor)"));
        assert!(constraints[0].contains("deleted_at IS NULL"));
        assert_eq!(
            issue_vote::UNIQUE_TOGETHER,
            &["issue", "actor", "deleted_at"]
        );
        assert_eq!(
            issue_vote::UNIQUE_ISSUE_ACTOR_NAME,
            "issue_vote_unique_issue_actor_when_deleted_at_null"
        );
        assert_eq!(
            issue_vote::UNIQUE_ISSUE_ACTOR_COLUMNS,
            &["issue_id", "actor_id"]
        );
        assert_eq!(issue_vote::UNIQUE_ISSUE_ACTOR_WHERE, "deleted_at IS NULL");
        assert_eq!(issue_vote::ISSUE_ON_DELETE, OnDelete::Cascade);
        assert_eq!(issue_vote::ISSUE_RELATED_NAME, "votes");
        assert_eq!(issue_vote::ACTOR_ON_DELETE, OnDelete::Cascade);
        assert_eq!(issue_vote::ACTOR_RELATED_NAME, "votes");
        assert_eq!(issue_vote::PROJECT_ON_DELETE, OnDelete::Cascade);
        assert_eq!(issue_vote::WORKSPACE_ON_DELETE, OnDelete::Cascade);
        assert_eq!(issue_vote::CREATED_BY_ON_DELETE, OnDelete::SetNull);
        assert_eq!(issue_vote::UPDATED_BY_ON_DELETE, OnDelete::SetNull);
    }

    #[test]
    fn overlap_matches_merged_defs() {
        // D-02 space columns: same tables, same column order, same
        // ordering, same unique names and scalar defaults.
        assert_eq!(issue_comment::TABLE, space_columns::issue_comment::TABLE);
        assert_eq!(
            issue_comment::COLUMNS,
            space_columns::issue_comment::COLUMNS
        );
        assert_eq!(
            issue_comment::ORDERING,
            space_columns::issue_comment::ORDERING[0]
        );
        assert_eq!(
            issue_comment::ACCESS_DEFAULT,
            space_columns::issue_comment::ACCESS_DEFAULT
        );
        assert_eq!(issue_reaction::TABLE, space_columns::issue_reaction::TABLE);
        assert_eq!(
            issue_reaction::COLUMNS,
            space_columns::issue_reaction::COLUMNS
        );
        assert_eq!(
            issue_reaction::ORDERING,
            space_columns::issue_reaction::ORDERING[0]
        );
        assert_eq!(
            issue_reaction::UNIQUE_TOGETHER,
            space_columns::issue_reaction::UNIQUE_TOGETHER
        );
        assert_eq!(
            issue_reaction::UNIQUE_ISSUE_ACTOR_NAME,
            space_columns::issue_reaction::UNIQUE_CONSTRAINT
        );
        assert_eq!(
            comment_reaction::TABLE,
            space_columns::comment_reaction::TABLE
        );
        assert_eq!(
            comment_reaction::COLUMNS,
            space_columns::comment_reaction::COLUMNS
        );
        assert_eq!(
            comment_reaction::ORDERING,
            space_columns::comment_reaction::ORDERING[0]
        );
        assert_eq!(
            comment_reaction::UNIQUE_TOGETHER,
            space_columns::comment_reaction::UNIQUE_TOGETHER
        );
        assert_eq!(
            comment_reaction::UNIQUE_COMMENT_ACTOR_NAME,
            space_columns::comment_reaction::UNIQUE_CONSTRAINT
        );
        assert_eq!(issue_vote::TABLE, space_columns::issue_vote::TABLE);
        assert_eq!(issue_vote::COLUMNS, space_columns::issue_vote::COLUMNS);
        assert_eq!(issue_vote::ORDERING, space_columns::issue_vote::ORDERING[0]);
        assert_eq!(
            issue_vote::UNIQUE_TOGETHER,
            space_columns::issue_vote::UNIQUE_TOGETHER
        );
        assert_eq!(
            issue_vote::UNIQUE_ISSUE_ACTOR_NAME,
            space_columns::issue_vote::UNIQUE_CONSTRAINT
        );
        assert_eq!(
            issue_vote::VOTE_DEFAULT,
            space_columns::issue_vote::VOTE_DEFAULT
        );
        // D-32 search reference: same table, same FTS consts; every read
        // column it pins exists in this module's column list.
        assert_eq!(issue_comment::TABLE, issue_comment_ref::TABLE);
        assert_eq!(
            issue_comment::FTS_INDEX_NAME,
            issue_comment_ref::FTS_INDEX_NAME
        );
        assert_eq!(issue_comment::FTS_CONFIG, issue_comment_ref::FTS_CONFIG);
        assert_eq!(
            issue_comment::FTS_SOURCE_COLUMNS,
            issue_comment_ref::FTS_SOURCE_COLUMNS
        );
        assert_eq!(
            issue_comment::ACCESS_DEFAULT,
            issue_comment_ref::DEFAULT_ACCESS
        );
        for read in issue_comment_ref::READ_COLUMNS {
            assert!(
                issue_comment::COLUMNS.contains(read),
                "read column {read} is a comment column"
            );
        }
    }

    #[test]
    fn foreign_reads_replay_fixture() {
        let v = fixture();
        assert_eq!(
            owned(foreign::DESCRIPTION_COLUMNS_READ),
            foreign_columns_read(&v, "description")
        );
        assert_eq!(
            foreign::DESCRIPTION_COLUMNS_READ,
            &[
                "id",
                "workspace_id",
                "project_id",
                "description_json",
                "description_html",
                "description_binary",
                "description_stripped",
            ]
        );
        let table: &str = foreign::DESCRIPTION_TABLE;
        assert_eq!(table, "descriptions");
        // Reuse note: owned by the models-D port (PIDASHCONV-647), SQL
        // literal pinned here until it merges.
        assert!(v["foreign_reads"]["description"]["reuse"]
            .as_str()
            .unwrap()
            .contains("description.py"));
        assert!(v["foreign_reads"]["description"]["reuse"]
            .as_str()
            .unwrap()
            .contains("SQL literal pinned here"));
        // The writes note matches this module's save-half consts: creates
        // on comment create (:609-627), `.update` propagation (:629-647).
        assert!(v["foreign_reads"]["description"]["creates"]
            .as_str()
            .unwrap()
            .contains("Description row on comment create"));
        assert!(v["foreign_reads"]["description"]["writes"]
            .as_str()
            .unwrap()
            .contains(".update"));
        assert!(v["foreign_reads"]["description"]["writes"]
            .as_str()
            .unwrap()
            .contains("no Description signals"));
    }

    #[test]
    fn reads_are_soft_delete_scoped() {
        for table in [
            issue_comment::TABLE,
            issue_subscriber::TABLE,
            issue_reaction::TABLE,
            comment_reaction::TABLE,
            issue_vote::TABLE,
        ] {
            let mut select = Query::select();
            select
                .column(Alias::new("id"))
                .from(Alias::new(table))
                .cond_where(active_condition());
            let sql = select.to_string(PostgresQueryBuilder);
            assert_eq!(
                sql,
                format!(r#"SELECT "id" FROM "{table}" WHERE "deleted_at" IS NULL"#)
            );
        }
    }

    #[test]
    fn display_matches_python_str() {
        let epoch = chrono::DateTime::from_timestamp(0, 0).unwrap();
        let issue_id = uuid::Uuid::nil();
        // `IssueComment.__str__` delegates to the joined issue's display.
        let rendered_issue = models_core::issue::Issue {
            id: issue_id,
            created_at: epoch,
            updated_at: epoch,
            created_by_id: None,
            updated_by_id: None,
            deleted_at: None,
            project_id: uuid::Uuid::nil(),
            workspace_id: uuid::Uuid::nil(),
            parent_id: None,
            state_id: None,
            point: None,
            estimate_point_id: None,
            name: "Ship it".to_string(),
            description_json: serde_json::json!({}),
            description_html: String::new(),
            description_stripped: None,
            description_binary: None,
            priority: "high".to_string(),
            complexity_score: 0,
            start_date: None,
            target_date: None,
            sequence_id: 1,
            sort_order: 0.0,
            completed_at: None,
            archived_at: None,
            is_draft: false,
            external_source: None,
            external_id: None,
            type_id: None,
            git_work_branch: String::new(),
            workpad: String::new(),
            created_via: None,
            assigned_pod_id: None,
            agent_executor: None,
        };
        assert_eq!(
            issue_comment::IssueComment::label(&rendered_issue.to_string()),
            format!("Ship it <{}>", uuid::Uuid::nil())
        );
        assert_eq!(
            issue_subscriber::IssueSubscriber::label("Ship it", "sam@example.com"),
            "Ship it sam@example.com"
        );
        assert_eq!(
            issue_reaction::IssueReaction::label("Ship it", "sam@example.com"),
            "Ship it sam@example.com"
        );
        assert_eq!(
            issue_vote::IssueVote::label("Ship it", "sam@example.com"),
            "Ship it sam@example.com"
        );
        // Ported bug: Python raises AttributeError (no `issue` field);
        // the port renders the two FK ids instead of raising.
        let reaction = comment_reaction::CommentReaction {
            id: uuid::Uuid::nil(),
            created_at: epoch,
            updated_at: epoch,
            created_by_id: None,
            updated_by_id: None,
            deleted_at: None,
            project_id: uuid::Uuid::nil(),
            workspace_id: uuid::Uuid::nil(),
            actor_id: uuid::Uuid::nil(),
            comment_id: uuid::Uuid::nil(),
            reaction: "+1".to_string(),
        };
        assert_eq!(
            reaction.to_string(),
            format!("{} {}", uuid::Uuid::nil(), uuid::Uuid::nil())
        );
        // A full comment row constructs (all 24 columns, FK-heavy shape).
        let comment = issue_comment::IssueComment {
            id: uuid::Uuid::nil(),
            created_at: epoch,
            updated_at: epoch,
            created_by_id: None,
            updated_by_id: None,
            deleted_at: None,
            project_id: uuid::Uuid::nil(),
            workspace_id: uuid::Uuid::nil(),
            comment_stripped: "hi".to_string(),
            comment_json: serde_json::json!({}),
            comment_html: "<p>hi</p>".to_string(),
            description_id: None,
            attachments: Vec::new(),
            labels: vec!["fold".to_string()],
            issue_id,
            actor_id: None,
            access: issue_comment::ACCESS_DEFAULT.to_string(),
            external_source: None,
            external_id: None,
            speaker_type: issue_comment::SPEAKER_TYPE_DEFAULT.to_string(),
            speaker_label: String::new(),
            speaker_agent_run_id: None,
            edited_at: None,
            parent_id: None,
        };
        assert_eq!(comment.labels, vec!["fold".to_string()]);
        assert_eq!(comment.access, "INTERNAL");
    }
}
