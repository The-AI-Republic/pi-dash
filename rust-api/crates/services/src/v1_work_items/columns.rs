//! D-18 models layer: column-verification record + nullable-column consts.
//!
//! D-18 (`api-v1: work items`) owns no tables, so this layer ports no
//! models. Instead it verifies that the db crate already exposes every
//! column the domain needs, and publishes the one shape the db crate
//! does not expose as data: per-table nullable-column lists for the
//! queries layer (which columns admit `NULL`, in fixture ordinal order).
//!
//! Source of truth: `rust-api/fixtures/v1_work_items/models/`
//! `F18-05.columns.json` (recorded by PIDASHCONV-659 from
//! `information_schema.columns` in ordinal order, cross-checked against
//! the Django model `_meta`). The `#[cfg(test)]` suite replays it table
//! by table against the db-crate `TABLE`/`COLUMNS` consts below.
//!
//! Verified mapping (fixture model → physical table → primary db module;
//! the `#[cfg(test)]` suite pins the secondaries too):
//!
//! | Fixture | Table | Primary db module | Secondary defs (also match) |
//! | --- | --- | --- | --- |
//! | `Issue` | `issues` | `app_issues::models_core::issue` | — |
//! | `Label` | `labels` | `app_issues::models_core::label` | — |
//! | `IssueLink` | `issue_links` | `app_issues::models_links::issue_link` | `space::columns::issue_link` (D-02, columns only) |
//! | `IssueComment` | `issue_comments` | `app_issues::models_engage::issue_comment` | — |
//! | `IssueActivity` | `issue_activities` | `app_issues::models_read::issue_activity` | — |
//! | `FileAsset` | `file_assets` | `v1_assets::model::file_asset` | `app_assets::columns` (D-31, columns + helpers) |
//! | `IssueRelation` | `issue_relations` | `app_issues::models_links::issue_relation` | `space::columns::issue_relation` (D-02, columns only) |
//! | `GithubPullRequestLink` | `github_pull_request_links` | `app_issues::models_links::github_pull_request_link` (D-26, full port) | `integrations::github_models::github_pull_request_link` (D-05) |
//! | `GitCodeReviewLink` | `git_code_review_links` | `app_issues::models_links::git_code_review_link` (D-26, full port) | `integrations::git_models::git_code_review_link` (D-05) |
//! | `Page` | `pages` | `app_pages::page` | — |
//! | `ProjectPage` | `project_pages` | `app_pages::project_page` | — |
//!
//! The D-26/D-05 overlap on the PR/review links and the D-02 columns-only
//! overlap on link/relation are deliberate cross-domain duplication
//! (established practice, documented on
//! `app_issues::models_links`): each domain's models layer is the
//! contract its own queries layer builds against. Both defs match F18-05
//! byte for byte on names and nullability; neither needed a change, so
//! no follow-up issue was filed (foundation crates are read-only).
//!
//! # Method
//!
//! Names are compared as sets: db-crate `COLUMNS` consts run in Django
//! model-declaration order while F18-05 runs in Postgres ordinal order,
//! so order differs by design and is not asserted. Nullability is
//! `Option<..>` ⟺ `nullable` on all 216 columns, pinned two ways: the
//! `NULLABLE` consts below replay the fixture exactly (in ordinal
//! order), and the `sample_*` constructors build one row per struct with
//! `None` for every nullable field — a mistyped field fails compilation.
//!
//! Ported from `01a93e17216faea7bfc156b0f864cbbe420d1c52`.

/// `issues` nullable columns (`app_issues::models_core::issue`, F18-05
/// `Issue`, `db/models/issue.py`), in fixture ordinal order.
pub mod issue {
    /// Columns whose F18-05 entry has `nullable: true` (19 of 34).
    pub const NULLABLE: &[&str] = &[
        "start_date",
        "target_date",
        "created_by_id",
        "parent_id",
        "state_id",
        "updated_by_id",
        "description_stripped",
        "completed_at",
        "point",
        "archived_at",
        "external_id",
        "external_source",
        "description_binary",
        "estimate_point_id",
        "type_id",
        "deleted_at",
        "assigned_pod_id",
        "created_via",
        "agent_executor",
    ];
}

/// `labels` nullable columns (`app_issues::models_core::label`, F18-05
/// `Label`, `db/models/label.py`), in fixture ordinal order.
pub mod label {
    /// Columns whose F18-05 entry has `nullable: true` (7 of 15).
    pub const NULLABLE: &[&str] = &[
        "created_by_id",
        "project_id",
        "updated_by_id",
        "parent_id",
        "external_id",
        "external_source",
        "deleted_at",
    ];
}

/// `issue_links` nullable columns
/// (`app_issues::models_links::issue_link`, F18-05 `IssueLink`,
/// `db/models/issue.py:471-484`), in fixture ordinal order.
pub mod issue_link {
    /// Columns whose F18-05 entry has `nullable: true` (4 of 12).
    pub const NULLABLE: &[&str] = &["title", "created_by_id", "updated_by_id", "deleted_at"];
}

/// `issue_comments` nullable columns
/// (`app_issues::models_engage::issue_comment`, F18-05 `IssueComment`,
/// `db/models/comment.py`), in fixture ordinal order.
pub mod issue_comment {
    /// Columns whose F18-05 entry has `nullable: true` (10 of 24).
    pub const NULLABLE: &[&str] = &[
        "created_by_id",
        "updated_by_id",
        "actor_id",
        "external_id",
        "external_source",
        "deleted_at",
        "edited_at",
        "description_id",
        "parent_id",
        "speaker_agent_run_id",
    ];
}

/// `issue_activities` nullable columns
/// (`app_issues::models_read::issue_activity`, F18-05 `IssueActivity`,
/// `db/models/activity.py`), in fixture ordinal order.
pub mod issue_activity {
    /// Columns whose F18-05 entry has `nullable: true` (12 of 20).
    pub const NULLABLE: &[&str] = &[
        "field",
        "old_value",
        "new_value",
        "created_by_id",
        "issue_id",
        "issue_comment_id",
        "updated_by_id",
        "actor_id",
        "new_identifier",
        "old_identifier",
        "epoch",
        "deleted_at",
    ];
}

/// `file_assets` nullable columns (`v1_assets::model::file_asset`,
/// F18-05 `FileAsset`, `db/models/asset.py`), in fixture ordinal order.
pub mod file_asset {
    /// Columns whose F18-05 entry has `nullable: true` (15 of 24).
    pub const NULLABLE: &[&str] = &[
        "created_by_id",
        "updated_by_id",
        "workspace_id",
        "deleted_at",
        "comment_id",
        "entity_type",
        "external_id",
        "external_source",
        "issue_id",
        "page_id",
        "project_id",
        "storage_metadata",
        "user_id",
        "draft_issue_id",
        "entity_identifier",
    ];
}

/// `issue_relations` nullable columns
/// (`app_issues::models_links::issue_relation`, F18-05 `IssueRelation`,
/// `db/models/relation.py`), in fixture ordinal order.
pub mod issue_relation {
    /// Columns whose F18-05 entry has `nullable: true` (3 of 11).
    pub const NULLABLE: &[&str] = &["created_by_id", "updated_by_id", "deleted_at"];
}

/// `github_pull_request_links` nullable columns
/// (`app_issues::models_links::github_pull_request_link`, F18-05
/// `GithubPullRequestLink`, `db/models/github_pr.py`), in fixture ordinal
/// order.
pub mod github_pull_request_link {
    /// Columns whose F18-05 entry has `nullable: true` (4 of 18).
    pub const NULLABLE: &[&str] = &[
        "deleted_at",
        "pr_updated_at",
        "created_by_id",
        "updated_by_id",
    ];
}

/// `git_code_review_links` nullable columns
/// (`app_issues::models_links::git_code_review_link`, F18-05
/// `GitCodeReviewLink`, `db/models/code_review.py`), in fixture ordinal
/// order.
pub mod git_code_review_link {
    /// Columns whose F18-05 entry has `nullable: true` (4 of 23).
    pub const NULLABLE: &[&str] = &[
        "deleted_at",
        "remote_updated_at",
        "created_by_id",
        "updated_by_id",
    ];
}

/// `pages` nullable columns (`app_pages::page`, F18-05 `Page`,
/// `db/models/page.py`), in fixture ordinal order.
pub mod page {
    /// Columns whose F18-05 entry has `nullable: true` (11 of 26).
    pub const NULLABLE: &[&str] = &[
        "description_stripped",
        "created_by_id",
        "updated_by_id",
        "archived_at",
        "parent_id",
        "description_binary",
        "deleted_at",
        "moved_to_page",
        "moved_to_project",
        "external_id",
        "external_source",
    ];
}

/// `project_pages` nullable columns (`app_pages::project_page`, F18-05
/// `ProjectPage`, `db/models/page.py`), in fixture ordinal order.
pub mod project_page {
    /// Columns whose F18-05 entry has `nullable: true` (3 of 9).
    pub const NULLABLE: &[&str] = &["created_by_id", "updated_by_id", "deleted_at"];
}

#[cfg(test)]
mod tests {
    use super::{
        file_asset, git_code_review_link, github_pull_request_link, issue, issue_activity,
        issue_comment, issue_link, issue_relation, label, page, project_page,
    };
    use pidash_db::{app_assets, app_issues, app_pages, integrations, space, v1_assets};
    use serde_json::Value;

    fn fixture() -> Value {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/v1_work_items/models/F18-05.columns.json");
        let body = std::fs::read_to_string(&path).expect("read F18-05.columns.json");
        serde_json::from_str(&body).expect("F18-05 is valid JSON")
    }

    /// `(db_table, column names in ordinal order, nullable names in
    /// ordinal order)` for one F18-05 `tables.<Model>` entry.
    fn table(value: &Value, model: &str) -> (String, Vec<String>, Vec<String>) {
        let entry = &value["tables"][model];
        assert!(entry.is_object(), "{model} has a tables entry");
        let db_table = entry["db_table"]
            .as_str()
            .expect("db_table is str")
            .to_string();
        let columns = entry["columns"].as_array().expect("columns is an array");
        assert_eq!(
            columns.len(),
            entry["column_count"].as_u64().expect("column_count is u64") as usize,
            "{model} column_count matches the listed columns"
        );
        let mut names = Vec::with_capacity(columns.len());
        let mut nullable = Vec::new();
        for column in columns {
            let name = column["name"]
                .as_str()
                .expect("column has name")
                .to_string();
            if column["nullable"].as_bool().expect("column has nullable") {
                nullable.push(name.clone());
            }
            names.push(name);
        }
        (db_table, names, nullable)
    }

    /// Copy a const into an owned vec so assertions compare two runtime
    /// values (clippy denies asserting on constants directly).
    fn owned(cols: &[&str]) -> Vec<String> {
        cols.iter().map(|c| (*c).to_string()).collect()
    }

    /// Replay one F18-05 table against a db-crate module: `TABLE`
    /// equals `db_table`, `COLUMNS` covers exactly the fixture names
    /// (as sets — declaration order vs ordinal order differs by
    /// design), and this module's `NULLABLE` equals the fixture
    /// nullable names in ordinal order.
    fn assert_table(model: &str, db_table: &str, db_columns: &[&str], nullable: &[&str]) {
        let fx = fixture();
        let (want_table, want_names, want_nullable) = table(&fx, model);
        assert_eq!(db_table.to_string(), want_table, "{model} TABLE");
        let mut got = owned(db_columns);
        got.sort();
        let mut want = want_names;
        want.sort();
        assert_eq!(got, want, "{model} COLUMNS names");
        assert_eq!(owned(nullable), want_nullable, "{model} NULLABLE");
        for name in nullable {
            assert!(
                db_columns.contains(name),
                "{model} NULLABLE {name} is a column"
            );
        }
    }

    /// Replay one F18-05 table against a columns-only secondary def
    /// (`TABLE` + `COLUMNS` names, no row struct to pin).
    fn assert_secondary(model: &str, db_table: &str, db_columns: &[&str]) {
        let fx = fixture();
        let (want_table, want_names, _) = table(&fx, model);
        assert_eq!(db_table.to_string(), want_table, "{model} secondary TABLE");
        let mut got = owned(db_columns);
        got.sort();
        let mut want = want_names;
        want.sort();
        assert_eq!(got, want, "{model} secondary COLUMNS names");
    }

    #[test]
    fn issue_matches_fixture() {
        assert_table(
            "Issue",
            app_issues::models_core::issue::TABLE,
            app_issues::models_core::issue::COLUMNS,
            issue::NULLABLE,
        );
        let row = sample_issue();
        assert!(row.deleted_at.is_none());
        assert!(row.state_id.is_none());
    }

    #[test]
    fn label_matches_fixture() {
        assert_table(
            "Label",
            app_issues::models_core::label::TABLE,
            app_issues::models_core::label::COLUMNS,
            label::NULLABLE,
        );
        let row = sample_label();
        assert!(row.project_id.is_none());
        assert!(row.deleted_at.is_none());
    }

    #[test]
    fn issue_link_matches_fixture() {
        assert_table(
            "IssueLink",
            app_issues::models_links::issue_link::TABLE,
            app_issues::models_links::issue_link::COLUMNS,
            issue_link::NULLABLE,
        );
        let row = sample_issue_link();
        assert!(row.title.is_none());
    }

    #[test]
    fn issue_comment_matches_fixture() {
        assert_table(
            "IssueComment",
            app_issues::models_engage::issue_comment::TABLE,
            app_issues::models_engage::issue_comment::COLUMNS,
            issue_comment::NULLABLE,
        );
        let row = sample_issue_comment();
        assert!(row.actor_id.is_none());
        assert!(row.edited_at.is_none());
    }

    #[test]
    fn issue_activity_matches_fixture() {
        assert_table(
            "IssueActivity",
            app_issues::models_read::issue_activity::TABLE,
            app_issues::models_read::issue_activity::COLUMNS,
            issue_activity::NULLABLE,
        );
        let row = sample_issue_activity();
        assert!(row.issue_id.is_none());
        assert!(row.epoch.is_none());
    }

    #[test]
    fn file_asset_matches_fixture() {
        assert_table(
            "FileAsset",
            v1_assets::model::file_asset::TABLE,
            v1_assets::model::file_asset::COLUMNS,
            file_asset::NULLABLE,
        );
        let row = sample_file_asset();
        assert!(row.workspace_id.is_none());
        assert!(row.storage_metadata.is_none());
    }

    #[test]
    fn issue_relation_matches_fixture() {
        assert_table(
            "IssueRelation",
            app_issues::models_links::issue_relation::TABLE,
            app_issues::models_links::issue_relation::COLUMNS,
            issue_relation::NULLABLE,
        );
        let row = sample_issue_relation();
        assert!(row.deleted_at.is_none());
    }

    #[test]
    fn github_pull_request_link_matches_fixture() {
        assert_table(
            "GithubPullRequestLink",
            app_issues::models_links::github_pull_request_link::TABLE,
            app_issues::models_links::github_pull_request_link::COLUMNS,
            github_pull_request_link::NULLABLE,
        );
        let row = sample_github_pull_request_link();
        assert!(row.pr_updated_at.is_none());
    }

    #[test]
    fn git_code_review_link_matches_fixture() {
        assert_table(
            "GitCodeReviewLink",
            app_issues::models_links::git_code_review_link::TABLE,
            app_issues::models_links::git_code_review_link::COLUMNS,
            git_code_review_link::NULLABLE,
        );
        let row = sample_git_code_review_link();
        assert!(row.remote_updated_at.is_none());
    }

    #[test]
    fn page_matches_fixture() {
        assert_table(
            "Page",
            app_pages::page::TABLE,
            app_pages::page::COLUMNS,
            page::NULLABLE,
        );
        let row = sample_page();
        assert!(row.parent_id.is_none());
        assert!(row.moved_to_page.is_none());
    }

    #[test]
    fn project_page_matches_fixture() {
        assert_table(
            "ProjectPage",
            app_pages::project_page::TABLE,
            app_pages::project_page::COLUMNS,
            project_page::NULLABLE,
        );
        let row = sample_project_page();
        assert!(row.deleted_at.is_none());
    }

    #[test]
    fn secondary_defs_match_fixture() {
        // D-02 columns-only defs.
        assert_secondary(
            "IssueLink",
            space::columns::issue_link::TABLE,
            space::columns::issue_link::COLUMNS,
        );
        assert_secondary(
            "IssueRelation",
            space::columns::issue_relation::TABLE,
            space::columns::issue_relation::COLUMNS,
        );
        // D-31 file-asset columns + helpers.
        assert_secondary(
            "FileAsset",
            app_assets::columns::TABLE,
            app_assets::columns::COLUMNS,
        );
        // D-05 PR/review defs (full structs, pinned below too).
        assert_secondary(
            "GithubPullRequestLink",
            integrations::github_models::github_pull_request_link::TABLE,
            integrations::github_models::github_pull_request_link::COLUMNS,
        );
        assert_secondary(
            "GitCodeReviewLink",
            integrations::git_models::git_code_review_link::TABLE,
            integrations::git_models::git_code_review_link::COLUMNS,
        );
        let pr = sample_d05_github_pull_request_link();
        assert!(pr.pr_updated_at.is_none());
        let review = sample_d05_git_code_review_link();
        assert!(review.remote_updated_at.is_none());
    }

    // Sample-row constructors: one row per verified struct with `None`
    // for every nullable field and dummy values elsewhere. The
    // construction itself is the nullability pin — a field whose
    // `Option` does not match F18-05 fails compilation here.

    fn uid() -> uuid::Uuid {
        uuid::Uuid::nil()
    }

    fn ts() -> chrono::DateTime<chrono::Utc> {
        chrono::DateTime::<chrono::Utc>::MIN_UTC
    }

    fn sample_issue() -> app_issues::models_core::issue::Issue {
        app_issues::models_core::issue::Issue {
            id: uid(),
            created_at: ts(),
            updated_at: ts(),
            created_by_id: None,
            updated_by_id: None,
            deleted_at: None,
            project_id: uid(),
            workspace_id: uid(),
            parent_id: None,
            state_id: None,
            point: None,
            estimate_point_id: None,
            name: String::new(),
            description_json: Value::Null,
            description_html: String::new(),
            description_stripped: None,
            description_binary: None,
            priority: String::new(),
            complexity_score: 0,
            start_date: None,
            target_date: None,
            sequence_id: 0,
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
        }
    }

    fn sample_label() -> app_issues::models_core::label::Label {
        app_issues::models_core::label::Label {
            id: uid(),
            created_at: ts(),
            updated_at: ts(),
            created_by_id: None,
            updated_by_id: None,
            deleted_at: None,
            workspace_id: uid(),
            project_id: None,
            parent_id: None,
            name: String::new(),
            description: String::new(),
            color: String::new(),
            sort_order: 0.0,
            external_source: None,
            external_id: None,
        }
    }

    fn sample_issue_link() -> app_issues::models_links::issue_link::IssueLink {
        app_issues::models_links::issue_link::IssueLink {
            id: uid(),
            created_at: ts(),
            updated_at: ts(),
            created_by_id: None,
            updated_by_id: None,
            deleted_at: None,
            project_id: uid(),
            workspace_id: uid(),
            title: None,
            url: String::new(),
            issue_id: uid(),
            metadata: Value::Null,
        }
    }

    fn sample_issue_comment() -> app_issues::models_engage::issue_comment::IssueComment {
        app_issues::models_engage::issue_comment::IssueComment {
            id: uid(),
            created_at: ts(),
            updated_at: ts(),
            created_by_id: None,
            updated_by_id: None,
            deleted_at: None,
            project_id: uid(),
            workspace_id: uid(),
            comment_stripped: String::new(),
            comment_json: Value::Null,
            comment_html: String::new(),
            description_id: None,
            attachments: Vec::new(),
            labels: Vec::new(),
            issue_id: uid(),
            actor_id: None,
            access: String::new(),
            external_source: None,
            external_id: None,
            speaker_type: String::new(),
            speaker_label: String::new(),
            speaker_agent_run_id: None,
            edited_at: None,
            parent_id: None,
        }
    }

    fn sample_issue_activity() -> app_issues::models_read::issue_activity::IssueActivity {
        app_issues::models_read::issue_activity::IssueActivity {
            id: uid(),
            created_at: ts(),
            updated_at: ts(),
            created_by_id: None,
            updated_by_id: None,
            deleted_at: None,
            project_id: uid(),
            workspace_id: uid(),
            issue_id: None,
            verb: String::new(),
            field: None,
            old_value: None,
            new_value: None,
            comment: String::new(),
            attachments: Vec::new(),
            issue_comment_id: None,
            actor_id: None,
            old_identifier: None,
            new_identifier: None,
            epoch: None,
        }
    }

    fn sample_file_asset() -> v1_assets::model::file_asset::FileAsset {
        v1_assets::model::file_asset::FileAsset {
            id: uid(),
            created_at: ts(),
            updated_at: ts(),
            created_by_id: None,
            updated_by_id: None,
            deleted_at: None,
            attributes: Value::Null,
            asset: String::new(),
            user_id: None,
            workspace_id: None,
            draft_issue_id: None,
            project_id: None,
            issue_id: None,
            comment_id: None,
            page_id: None,
            entity_type: None,
            entity_identifier: None,
            is_deleted: false,
            is_archived: false,
            external_id: None,
            external_source: None,
            size: 0.0,
            is_uploaded: false,
            storage_metadata: None,
        }
    }

    fn sample_issue_relation() -> app_issues::models_links::issue_relation::IssueRelation {
        app_issues::models_links::issue_relation::IssueRelation {
            id: uid(),
            created_at: ts(),
            updated_at: ts(),
            created_by_id: None,
            updated_by_id: None,
            deleted_at: None,
            project_id: uid(),
            workspace_id: uid(),
            issue_id: uid(),
            related_issue_id: uid(),
            relation_type: String::new(),
        }
    }

    fn sample_github_pull_request_link(
    ) -> app_issues::models_links::github_pull_request_link::GithubPullRequestLink {
        app_issues::models_links::github_pull_request_link::GithubPullRequestLink {
            id: uid(),
            created_at: ts(),
            updated_at: ts(),
            created_by_id: None,
            updated_by_id: None,
            deleted_at: None,
            project_id: uid(),
            workspace_id: uid(),
            issue_id: uid(),
            repo_owner: String::new(),
            repo_name: String::new(),
            pr_number: 0,
            url: String::new(),
            title: String::new(),
            state: String::new(),
            merged: false,
            draft: false,
            pr_updated_at: None,
        }
    }

    fn sample_git_code_review_link(
    ) -> app_issues::models_links::git_code_review_link::GitCodeReviewLink {
        app_issues::models_links::git_code_review_link::GitCodeReviewLink {
            id: uid(),
            created_at: ts(),
            updated_at: ts(),
            created_by_id: None,
            updated_by_id: None,
            deleted_at: None,
            project_id: uid(),
            workspace_id: uid(),
            issue_id: uid(),
            provider: String::new(),
            host_url: String::new(),
            namespace: String::new(),
            repo_name: String::new(),
            repo_external_id: String::new(),
            external_id: String::new(),
            external_iid: String::new(),
            url: String::new(),
            title: String::new(),
            state: String::new(),
            merged: false,
            draft: false,
            remote_updated_at: None,
            metadata: Value::Null,
        }
    }

    fn sample_page() -> app_pages::page::Page {
        app_pages::page::Page {
            id: uid(),
            created_at: ts(),
            updated_at: ts(),
            created_by_id: None,
            updated_by_id: None,
            deleted_at: None,
            workspace_id: uid(),
            name: String::new(),
            description_json: Value::Null,
            description_binary: None,
            description_html: String::new(),
            description_stripped: None,
            owned_by_id: uid(),
            access: 0,
            color: String::new(),
            parent_id: None,
            archived_at: None,
            is_locked: false,
            view_props: Value::Null,
            logo_props: Value::Null,
            is_global: false,
            moved_to_page: None,
            moved_to_project: None,
            sort_order: 0.0,
            external_id: None,
            external_source: None,
        }
    }

    fn sample_project_page() -> app_pages::project_page::ProjectPage {
        app_pages::project_page::ProjectPage {
            id: uid(),
            created_at: ts(),
            updated_at: ts(),
            created_by_id: None,
            updated_by_id: None,
            deleted_at: None,
            project_id: uid(),
            page_id: uid(),
            workspace_id: uid(),
        }
    }

    fn sample_d05_github_pull_request_link(
    ) -> integrations::github_models::github_pull_request_link::GithubPullRequestLink {
        integrations::github_models::github_pull_request_link::GithubPullRequestLink {
            id: uid(),
            created_at: ts(),
            updated_at: ts(),
            created_by_id: None,
            updated_by_id: None,
            deleted_at: None,
            project_id: uid(),
            workspace_id: uid(),
            issue_id: uid(),
            repo_owner: String::new(),
            repo_name: String::new(),
            pr_number: 0,
            url: String::new(),
            title: String::new(),
            state: String::new(),
            merged: false,
            draft: false,
            pr_updated_at: None,
        }
    }

    fn sample_d05_git_code_review_link(
    ) -> integrations::git_models::git_code_review_link::GitCodeReviewLink {
        integrations::git_models::git_code_review_link::GitCodeReviewLink {
            id: uid(),
            created_at: ts(),
            updated_at: ts(),
            created_by_id: None,
            updated_by_id: None,
            deleted_at: None,
            project_id: uid(),
            workspace_id: uid(),
            issue_id: uid(),
            provider: String::new(),
            host_url: String::new(),
            namespace: String::new(),
            repo_name: String::new(),
            repo_external_id: String::new(),
            external_id: String::new(),
            external_iid: String::new(),
            url: String::new(),
            title: String::new(),
            state: String::new(),
            merged: false,
            draft: false,
            remote_updated_at: None,
            metadata: Value::Null,
        }
    }
}
