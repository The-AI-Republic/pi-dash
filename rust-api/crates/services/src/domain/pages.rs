//! `domain::pages`: move a page, with its sub-pages, to another project of
//! the same workspace (`apps/api/pi_dash/utils/page_move.py`,
//! PDASHOSS01-347).
//!
//! Both drivers call this: the v1 `PageMoveAPIEndpoint`
//! (`api/views/page.py`, 200 + the page detail under the target) and the
//! app `PageMoveEndpoint` (`app/views/page/base.py`, 204). The drivers own
//! authentication, the `ProjectEntityPermission` gate and the visible-page
//! lookup (404 `{"error":"Page not found"}`); this module owns the body
//! rule ([`parse_target_project`]), the remaining checks in Django's order
//! and the writes ([`move_to_project`]).
//!
//! A page reaches a project through its `project_pages` row, so a move
//! relinks that row; the `pages` row keeps its id, owner, body, labels and
//! versions, and only gains `moved_to_project` (plus a cleared `parent_id`
//! when the moved page had a parent). The subtree moves along; favourites
//! scoped to the source project are soft-deleted; the move is logged as a
//! `page_logs` row (`entity_name="moved_to_project"`).
//!
//! Reads and writes run on the [`WriteTxn`]'s SeaORM transaction through the
//! `pages`/`project_pages`/`user_favorites`/`page_logs` entities. The one
//! raw statement is the recursive subtree CTE ([`SUBTREE_SQL`]): SeaORM 1.1
//! has no recursive-CTE builder (`sea-query` 0.32's `WithClause` cannot
//! drive an entity select), so it stays SQL text, read-only.

use chrono::{DateTime, Utc};
use sea_orm::sea_query::Expr;
use sea_orm::{
    ActiveModelTrait, ActiveValue, ColumnTrait, ConnectionTrait, DbBackend, EntityTrait,
    QueryFilter, QuerySelect, Statement,
};
use serde_json::Value;
use uuid::Uuid;

use pidash_db::entity::{
    page_logs, pages, project_members, project_pages, projects, user_favorites,
};

use super::error::DomainError;
use super::txn::WriteTxn;
use super::WriteCtx;

/// `PageLog.entity_name` of a move (`page_move.py`, `MOVE_LOG_ENTITY_NAME`).
pub const MOVE_LOG_ENTITY_NAME: &str = "moved_to_project";
/// `PageLog.entity_type` of a move (`MOVE_LOG_ENTITY_TYPE`).
pub const MOVE_LOG_ENTITY_TYPE: &str = "project";
/// `UserFavorite.entity_type` of a page favourite.
const FAVORITE_ENTITY_TYPE: &str = "page";

/// `ROLE.ADMIN` / `ROLE.MEMBER` (`app/permissions`).
const ROLE_ADMIN: i16 = 20;
const ROLE_MEMBER: i16 = 15;

/// `_subtree_ids`: the page plus every live descendant (`UNION`, so a
/// parent cycle terminates). The root itself is not liveness-filtered —
/// the caller already resolved it as a visible, live page.
pub const SUBTREE_SQL: &str = "WITH RECURSIVE descendants AS (SELECT id FROM pages WHERE id = $1 UNION SELECT pages.id FROM pages JOIN descendants ON pages.parent_id = descendants.id WHERE pages.deleted_at IS NULL) SELECT id FROM descendants";

/// Why a move is refused (`PageMoveError`): each maps to one status and
/// one exact JSON body. Variants are in check order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MoveRefusal {
    /// `project_id` missing, `null` or `""` (400).
    ProjectIdRequired,
    /// `project_id` is not a UUID (400).
    ProjectIdInvalid,
    /// Caller is not an active admin of the source project (403).
    NotSourceAdmin,
    /// Target is the source project (400).
    SameProject,
    /// Target missing, soft-deleted or in another workspace (400).
    TargetNotInWorkspace,
    /// Target archived (400).
    TargetArchived,
    /// Caller is not an active admin/member of the target (403).
    NoTargetWriteAccess,
    /// Page locked (400 `PAGE_LOCKED`).
    Locked,
}

impl MoveRefusal {
    /// HTTP status of the refusal.
    pub fn status(self) -> u16 {
        match self {
            MoveRefusal::NotSourceAdmin | MoveRefusal::NoTargetWriteAccess => 403,
            _ => 400,
        }
    }

    /// Exact response bytes (`PageMoveError.body`, compact DRF JSON; key
    /// order `error`, `error_code`, `error_message`).
    pub fn body(self) -> &'static str {
        match self {
            MoveRefusal::ProjectIdRequired => r#"{"error":"project_id is required"}"#,
            MoveRefusal::ProjectIdInvalid => r#"{"error":"project_id must be a valid project id"}"#,
            MoveRefusal::NotSourceAdmin => {
                r#"{"error":"Only a project admin can move a page to another project"}"#
            }
            MoveRefusal::SameProject => r#"{"error":"The page is already in this project"}"#,
            MoveRefusal::TargetNotInWorkspace => {
                r#"{"error":"The target project is not in this workspace"}"#
            }
            MoveRefusal::TargetArchived => r#"{"error":"The target project is archived"}"#,
            MoveRefusal::NoTargetWriteAccess => {
                r#"{"error":"You need write access to the target project to move a page into it"}"#
            }
            MoveRefusal::Locked => {
                r#"{"error":"Page is locked","error_code":4701,"error_message":"PAGE_LOCKED"}"#
            }
        }
    }
}

/// Check 1: `data.get("project_id")` then `uuid.UUID(str(raw))`.
///
/// `value` is the `project_id` entry of a JSON-object or form body; a body
/// that is not an object has no `.get` and passes `None`. Missing, `null`
/// and `""` are "required"; any other value is parsed from its string form
/// (numbers by their decimal digits, so only a 32-digit integer could pass;
/// booleans, arrays and objects never do).
pub fn parse_target_project(value: Option<&Value>) -> Result<Uuid, MoveRefusal> {
    let text = match value {
        None | Some(Value::Null) => return Err(MoveRefusal::ProjectIdRequired),
        Some(Value::String(text)) if text.is_empty() => return Err(MoveRefusal::ProjectIdRequired),
        Some(Value::String(text)) => text.clone(),
        Some(Value::Number(number)) => number.to_string(),
        Some(_) => return Err(MoveRefusal::ProjectIdInvalid),
    };
    parse_uuid_text(&text).ok_or(MoveRefusal::ProjectIdInvalid)
}

/// `uuid.UUID(hex)`'s accepted spellings: `urn:`/`uuid:` removed, braces
/// stripped from both ends, dashes removed, then exactly 32 hex digits
/// (either case). CPython's `int(…, 16)` leniency (surrounding whitespace,
/// `_` separators, a sign) is interpreter detail and not reproduced.
fn parse_uuid_text(raw: &str) -> Option<Uuid> {
    let text = raw.replace("urn:", "").replace("uuid:", "");
    let compact: String = text
        .trim_matches(|c| c == '{' || c == '}')
        .chars()
        .filter(|c| *c != '-')
        .collect();
    if compact.len() != 32 || !compact.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    Uuid::parse_str(&compact).ok()
}

/// One move request, after the driver's gate and page lookup.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PageMove {
    /// The visible page (`page.pk`).
    pub page_id: Uuid,
    /// `page.workspace_id`.
    pub workspace_id: Uuid,
    /// The URL project (after the identifier rewrite).
    pub source_project_id: Uuid,
    /// The parsed body `project_id` ([`parse_target_project`]).
    pub target_project_id: Uuid,
}

/// `move_page` checks 2-7 and the writes, in one transaction. Answers the
/// target project id, or the first refusal (nothing written then).
///
/// The actor is `ctx.actor.user_id`; `now` is the single write stamp
/// (`timezone.now()`, microsecond precision).
pub async fn move_to_project(
    ctx: &WriteCtx,
    txn: &mut WriteTxn,
    input: PageMove,
    now: DateTime<Utc>,
) -> Result<Result<Uuid, MoveRefusal>, DomainError> {
    let actor = ctx.actor.user_id;
    let db = txn.connection();
    let PageMove {
        page_id,
        workspace_id,
        source_project_id,
        target_project_id,
    } = input;

    if !is_member(&db, source_project_id, actor, &[ROLE_ADMIN]).await? {
        return Ok(Err(MoveRefusal::NotSourceAdmin));
    }
    if target_project_id == source_project_id {
        return Ok(Err(MoveRefusal::SameProject));
    }
    // `Project.objects` (soft-delete manager) scoped to the page's workspace.
    let target = projects::Entity::find_by_id(target_project_id)
        .filter(projects::Column::WorkspaceId.eq(workspace_id))
        .filter(projects::Column::DeletedAt.is_null())
        .one(&db)
        .await?;
    let Some(target) = target else {
        return Ok(Err(MoveRefusal::TargetNotInWorkspace));
    };
    if target.archived_at.is_some() {
        return Ok(Err(MoveRefusal::TargetArchived));
    }
    if !is_member(&db, target_project_id, actor, &[ROLE_ADMIN, ROLE_MEMBER]).await? {
        return Ok(Err(MoveRefusal::NoTargetWriteAccess));
    }
    // Django checks the lock on the fetched instance, then re-checks under
    // `select_for_update`; the locked read covers both.
    let current = pages::Entity::find_by_id(page_id)
        .filter(pages::Column::DeletedAt.is_null())
        .lock_exclusive()
        .one(&db)
        .await?
        .ok_or_else(|| DomainError::Db(sea_orm::DbErr::RecordNotFound("pages".to_owned())))?;
    if current.is_locked {
        return Ok(Err(MoveRefusal::Locked));
    }

    let stamp: sea_orm::prelude::DateTimeWithTimeZone = now.into();
    let subtree: Vec<Uuid> = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Postgres,
            SUBTREE_SQL,
            [page_id.into()],
        ))
        .await?
        .iter()
        .map(|row| row.try_get::<Uuid>("", "id"))
        .collect::<Result<_, _>>()?;

    let live_links = |project: Uuid, ids: Vec<Uuid>| {
        project_pages::Entity::find()
            .filter(project_pages::Column::ProjectId.eq(project))
            .filter(project_pages::Column::PageId.is_in(ids))
            .filter(project_pages::Column::DeletedAt.is_null())
    };
    let moved: Vec<Uuid> = live_links(source_project_id, subtree.clone())
        .all(&db)
        .await?
        .into_iter()
        .map(|link| link.page_id)
        .collect();
    let already_in_target: Vec<Uuid> = live_links(target_project_id, moved.clone())
        .all(&db)
        .await?
        .into_iter()
        .map(|link| link.page_id)
        .collect();

    // A page already linked to the target keeps that link; its source link
    // is soft-deleted instead of relinked (one live link per pair).
    if !already_in_target.is_empty() {
        project_pages::Entity::update_many()
            .col_expr(project_pages::Column::DeletedAt, Expr::value(stamp))
            .filter(project_pages::Column::ProjectId.eq(source_project_id))
            .filter(project_pages::Column::PageId.is_in(already_in_target))
            .filter(project_pages::Column::DeletedAt.is_null())
            .exec(&db)
            .await?;
    }
    project_pages::Entity::update_many()
        .col_expr(
            project_pages::Column::ProjectId,
            Expr::value(target_project_id),
        )
        .col_expr(project_pages::Column::UpdatedAt, Expr::value(stamp))
        .col_expr(project_pages::Column::UpdatedById, Expr::value(actor))
        .filter(project_pages::Column::ProjectId.eq(source_project_id))
        .filter(project_pages::Column::PageId.is_in(moved.clone()))
        .filter(project_pages::Column::DeletedAt.is_null())
        .exec(&db)
        .await?;

    pages::Entity::update_many()
        .col_expr(
            pages::Column::MovedToProject,
            Expr::value(target_project_id),
        )
        .col_expr(pages::Column::UpdatedAt, Expr::value(stamp))
        .col_expr(pages::Column::UpdatedById, Expr::value(actor))
        .filter(pages::Column::Id.is_in(moved.clone()))
        .filter(pages::Column::DeletedAt.is_null())
        .exec(&db)
        .await?;
    // A page moved on its own leaves its parent behind (a parent must be in
    // the same project). Only `parent_id` is written; children keep theirs.
    if current.parent_id.is_some() {
        pages::Entity::update_many()
            .col_expr(pages::Column::ParentId, Expr::value(Option::<Uuid>::None))
            .filter(pages::Column::Id.eq(page_id))
            .filter(pages::Column::DeletedAt.is_null())
            .exec(&db)
            .await?;
    }

    // Queryset `.delete()` on the soft-delete manager: `deleted_at` only.
    user_favorites::Entity::update_many()
        .col_expr(user_favorites::Column::DeletedAt, Expr::value(stamp))
        .filter(user_favorites::Column::EntityType.eq(FAVORITE_ENTITY_TYPE))
        .filter(user_favorites::Column::EntityIdentifier.is_in(moved))
        .filter(user_favorites::Column::ProjectId.eq(source_project_id))
        .filter(user_favorites::Column::WorkspaceId.eq(workspace_id))
        .filter(user_favorites::Column::DeletedAt.is_null())
        .exec(&db)
        .await?;

    // `PageLog(...).save(created_by_id=user.id)`: `updated_by` stays NULL.
    page_logs::ActiveModel {
        id: ActiveValue::Set(Uuid::new_v4()),
        created_at: ActiveValue::Set(stamp),
        updated_at: ActiveValue::Set(stamp),
        created_by_id: ActiveValue::Set(Some(actor)),
        updated_by_id: ActiveValue::Set(None),
        deleted_at: ActiveValue::Set(None),
        transaction: ActiveValue::Set(Uuid::new_v4()),
        page_id: ActiveValue::Set(page_id),
        entity_identifier: ActiveValue::Set(Some(target_project_id)),
        entity_name: ActiveValue::Set(MOVE_LOG_ENTITY_NAME.to_owned()),
        entity_type: ActiveValue::Set(Some(MOVE_LOG_ENTITY_TYPE.to_owned())),
        workspace_id: ActiveValue::Set(workspace_id),
    }
    .insert(&db)
    .await?;

    Ok(Ok(target_project_id))
}

/// `BasePageReadAPIEndpoint.get_queryset().filter(pk=page_id).first()`
/// (`api/views/page.py:182-213`; the app `PageMoveEndpoint` repeats it):
/// a live page of the `slug` workspace, linked by a live `project_pages`
/// row to `project_id`, whose project is not archived and has an active
/// membership row for `user_id` (one join row — the `Exists` filters share
/// it; the joined tables carry no soft-delete scope), and that is public or
/// owned by the caller.
pub async fn find_visible_page<C: ConnectionTrait>(
    db: &C,
    slug: &str,
    project_id: Uuid,
    user_id: Uuid,
    page_id: Uuid,
) -> Result<Option<pages::Model>, DomainError> {
    use pidash_db::entity::workspaces;
    use sea_orm::sea_query::{Condition, Query};
    let linked = Query::select()
        .expr(Expr::val(1))
        .from(project_pages::Entity)
        .inner_join(
            projects::Entity,
            Expr::col((projects::Entity, projects::Column::Id))
                .equals((project_pages::Entity, project_pages::Column::ProjectId)),
        )
        .inner_join(
            project_members::Entity,
            Expr::col((project_members::Entity, project_members::Column::ProjectId))
                .equals((projects::Entity, projects::Column::Id)),
        )
        .and_where(Expr::col((project_pages::Entity, project_pages::Column::DeletedAt)).is_null())
        .and_where(
            Expr::col((project_pages::Entity, project_pages::Column::PageId))
                .equals((pages::Entity, pages::Column::Id)),
        )
        .and_where(
            Expr::col((project_pages::Entity, project_pages::Column::ProjectId)).eq(project_id),
        )
        .and_where(Expr::col((projects::Entity, projects::Column::ArchivedAt)).is_null())
        .and_where(Expr::col((project_members::Entity, project_members::Column::IsActive)).eq(true))
        .and_where(
            Expr::col((project_members::Entity, project_members::Column::MemberId)).eq(user_id),
        )
        .to_owned();
    let workspace = Query::select()
        .column(workspaces::Column::Id)
        .from(workspaces::Entity)
        .and_where(workspaces::Column::Slug.eq(slug))
        .to_owned();
    let page = pages::Entity::find_by_id(page_id)
        .filter(pages::Column::DeletedAt.is_null())
        .filter(pages::Column::WorkspaceId.in_subquery(workspace))
        .filter(Expr::exists(linked))
        .filter(
            Condition::any()
                .add(pages::Column::Access.eq(0i16))
                .add(pages::Column::OwnedById.eq(user_id)),
        )
        .one(db)
        .await?;
    Ok(page)
}

/// `_is_member`: an active, live `project_members` row with one of `roles`
/// (no workspace scope, as in Django).
async fn is_member<C: ConnectionTrait>(
    db: &C,
    project_id: Uuid,
    user_id: Uuid,
    roles: &[i16],
) -> Result<bool, DomainError> {
    let row = project_members::Entity::find()
        .filter(project_members::Column::ProjectId.eq(project_id))
        .filter(project_members::Column::MemberId.eq(user_id))
        .filter(project_members::Column::IsActive.eq(true))
        .filter(project_members::Column::DeletedAt.is_null())
        .filter(project_members::Column::Role.is_in(roles.iter().copied()))
        .one(db)
        .await?;
    Ok(row.is_some())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const ID: &str = "550e8400-e29b-41d4-a716-446655440000";

    fn id() -> Uuid {
        Uuid::parse_str(ID).unwrap()
    }

    #[test]
    fn missing_null_and_blank_are_required() {
        for value in [None, Some(json!(null)), Some(json!(""))] {
            assert_eq!(
                parse_target_project(value.as_ref()),
                Err(MoveRefusal::ProjectIdRequired),
                "{value:?}"
            );
        }
    }

    #[test]
    fn python_uuid_spellings_parse() {
        for raw in [
            ID.to_owned(),
            ID.to_uppercase(),
            ID.replace('-', ""),
            format!("{{{ID}}}"),
            format!("urn:uuid:{ID}"),
            "550e-8400e29b41d4a716446655440000".to_owned(),
        ] {
            assert_eq!(parse_target_project(Some(&json!(raw))), Ok(id()), "{raw}");
        }
    }

    #[test]
    fn non_uuid_values_are_invalid() {
        for value in [
            json!("nope"),
            json!(" "),
            json!("{}"),
            json!(0),
            json!(12345),
            json!(1.5),
            json!(true),
            json!(false),
            json!([ID]),
            json!({"id": ID}),
            json!(format!("{ID}0")),
        ] {
            assert_eq!(
                parse_target_project(Some(&value)),
                Err(MoveRefusal::ProjectIdInvalid),
                "{value}"
            );
        }
    }

    #[test]
    fn refusal_statuses_and_bodies() {
        use MoveRefusal as R;
        let cases = [
            (R::ProjectIdRequired, 400, "project_id is required"),
            (
                R::ProjectIdInvalid,
                400,
                "project_id must be a valid project id",
            ),
            (
                R::NotSourceAdmin,
                403,
                "Only a project admin can move a page to another project",
            ),
            (R::SameProject, 400, "The page is already in this project"),
            (
                R::TargetNotInWorkspace,
                400,
                "The target project is not in this workspace",
            ),
            (R::TargetArchived, 400, "The target project is archived"),
            (
                R::NoTargetWriteAccess,
                403,
                "You need write access to the target project to move a page into it",
            ),
        ];
        for (refusal, status, message) in cases {
            assert_eq!(refusal.status(), status, "{refusal:?}");
            assert_eq!(
                refusal.body(),
                serde_json::to_string(&json!({ "error": message })).unwrap()
            );
        }
        assert_eq!(R::Locked.status(), 400);
        let locked: Value = serde_json::from_str(R::Locked.body()).unwrap();
        assert_eq!(
            locked,
            json!({"error": "Page is locked", "error_code": 4701, "error_message": "PAGE_LOCKED"})
        );
        assert!(R::Locked.body().starts_with(r#"{"error":"#));
    }

    #[test]
    fn subtree_sql_unions_live_descendants() {
        assert!(SUBTREE_SQL.contains(" UNION SELECT "));
        assert!(!SUBTREE_SQL.contains("UNION ALL"));
        assert!(SUBTREE_SQL.contains("WHERE pages.deleted_at IS NULL"));
    }
}
