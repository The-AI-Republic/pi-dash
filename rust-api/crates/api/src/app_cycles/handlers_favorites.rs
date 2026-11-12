//! Favorite-cycle handlers (D-27, stage 5, PIDASHCONV-323).
//!
//! Ports `CycleFavoriteViewSet` from
//! `apps/api/pi_dash/app/views/cycle/base.py:559-591`:
//!
//! - `create` (`:571-579`): `UserFavorite.objects.create(project_id,
//!   user, entity_type="cycle", entity_identifier=request.data.cycle)` —
//!   no workspace passed explicitly (it derives from the project in
//!   `WorkspaceBaseModel.save`), sequence from the workspace
//!   `Max + 10000` (`db/models/favorite.py:52-63`), 204 empty.
//! - `destroy` (`:581-591`): `.get(project, entity_type="cycle", user,
//!   workspace slug, entity_identifier)` — a miss raises
//!   `DoesNotExist` → 404 — then a hard delete (`soft=False`), 204 empty.
//!
//! The `GET` list falls through to the `ModelViewSet` default over a
//! queryset whose `select_related("cycle__owned_by")` names no FK
//! (`FieldError` → 500 upstream, pinned by the contract); it proxies to
//! Django, which reproduces the 500 itself.
//!
//! Fixture ids: F-C27-07 (MEMBER gates via `super::gates`), F-C27-10
//! (favorite goldens: 204 create, 204 + hard delete destroy).
//!
//! # Ported bugs (translate, don't redesign — also listed in the PR)
//!
//! * `create` sets no workspace explicitly (`:571-579`); the row's
//!   workspace derives from the project at save time.
//! * `destroy` on a missing row 404s through the default
//!   `DoesNotExist` branch, not a detail 404.
//! * A duplicate favorite violates the partial unique constraint
//!   (`favorite.py:39-44`) → `IntegrityError` → 400 "payload not valid".

use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::Router;
use serde_json::Value;

use crate::app_cycles::gates;
use crate::app_issues::Denial;
use crate::state::AppState;

use super::handlers_cycle_issues::{
    actor_user_id, check_gate, coerce_uuid_field, decode_request_body, empty_response,
    fetch_allow_facts, parse_uuid_or_invalid, pool_of, resolve_project_id, HandlerResult,
};

/// Collection path in `app/urls/cycle.py:56-60` form.
pub const FAVORITES_PATH: &str =
    "/api/workspaces/{slug}/projects/{project_id}/user-favorite-cycles/";
/// Detail path in `app/urls/cycle.py:61-65` form.
pub const FAVORITE_PATH: &str =
    "/api/workspaces/{slug}/projects/{project_id}/user-favorite-cycles/{cycle_id}/";

/// `entity_type` for every row this family writes (`base.py:575,584`).
pub const FAVORITE_ENTITY_TYPE: &str = "cycle";

/// `UserFavorite.sequence` default when the workspace has no favorites
/// (`db/models/favorite.py:19`: `FloatField(default=65535)`).
pub const FAVORITE_SEQUENCE_DEFAULT: f64 = 65535.0;

/// `POST` on the collection path and `DELETE` on the detail path are
/// owned; everything else (including the `GET` list) proxies to Django.
pub fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/api/workspaces/{slug}/projects/{project_id}/user-favorite-cycles/",
            axum::routing::post(favorite_create)
                .get(crate::edge::proxy)
                .put(crate::edge::proxy)
                .patch(crate::edge::proxy)
                .delete(crate::edge::proxy)
                .options(crate::edge::proxy),
        )
        .route(
            "/api/workspaces/{slug}/projects/{project_id}/user-favorite-cycles/{cycle_id}/",
            axum::routing::delete(favorite_destroy)
                .get(crate::edge::proxy)
                .put(crate::edge::proxy)
                .patch(crate::edge::proxy)
                .post(crate::edge::proxy)
                .options(crate::edge::proxy),
        )
}

fn gate_for_create() -> &'static gates::Gate {
    &gates::gate_for(
        "POST",
        "workspaces/<slug>/projects/<id>/user-favorite-cycles/",
    )
    .expect("favorites create gate")
    .gate
}

fn gate_for_destroy() -> &'static gates::Gate {
    &gates::gate_for(
        "DELETE",
        "workspaces/<slug>/projects/<id>/user-favorite-cycles/<uuid>/",
    )
    .expect("favorites destroy gate")
    .gate
}

/// The shared SeaORM connection over an sqlx pool (no I/O — it wraps the
/// pool). Reads run on it; the insert below stays handler-side (a direct
/// table write, not a §4 domain table).
fn sea_conn(pool: &sqlx::PgPool) -> sea_orm::DatabaseConnection {
    pidash_db::pool::sea_orm_connection(pool)
}

/// One `MAX(sequence)` row, decoded by name.
#[derive(Debug, Clone, Copy, sea_orm::FromQueryResult)]
struct MaxSequenceRow {
    largest: Option<f64>,
}

/// Coerce `request.data.get("cycle")` to the `entity_identifier` column:
/// absent/`null` stays NULL (the field is `null=True`); everything else
/// goes through `UUIDField.to_python` — a string parses as hex, a bool or
/// a non-negative int below `2**128` becomes `UUID(int=v)`, anything else
/// is a `ValidationError` → 400.
fn parse_entity_identifier(value: Option<&Value>) -> Result<Option<uuid::Uuid>, Denial> {
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(other) => coerce_uuid_field(other).map(Some),
    }
}

/// `CycleFavoriteViewSet.create` (`base.py:571-579`).
async fn favorite_create(
    State(state): State<AppState>,
    Path((slug, project_raw)): Path<(String, String)>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
    headers: HeaderMap,
    body: Bytes,
) -> HandlerResult {
    let pool = pool_of(&state)?;
    let user_id = actor_user_id(extension)?;
    let project_id = resolve_project_id(&pool, &slug, &project_raw).await?;
    let facts = fetch_allow_facts(&pool, &slug, &project_id, &user_id).await?;
    check_gate(gate_for_create(), &slug, &facts)?;
    // `request.data.get("cycle")` (`base.py:578`): decoded after the gate
    // (DRF parses lazily), so body errors never preempt auth/gate.
    let data = decode_request_body(&headers, &body)?;
    let entity_identifier = match data.as_object() {
        Some(_) => parse_entity_identifier(data.get("cycle"))?,
        // Non-object bodies cannot `.get` (AttributeError → 500).
        None => return Err(Denial::ServerError),
    };
    // The view passes no workspace; `WorkspaceBaseModel.save` derives it
    // from the project (`db/models/workspace.py:192-195`).
    use pidash_db::entity::projects as proj_entity;
    use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, QuerySelect};
    let workspace_id: Option<uuid::Uuid> = proj_entity::Entity::find_by_id(project_id)
        .filter(proj_entity::Column::DeletedAt.is_null())
        .select_only()
        .column(proj_entity::Column::WorkspaceId)
        .into_tuple()
        .one(&sea_conn(&pool))
        .await
        .map_err(|_| Denial::ServerError)?;
    let workspace_id = workspace_id.ok_or(Denial::ServerError)?;
    // `UserFavorite.save`: `Max(sequence) + 10000` over the workspace's
    // live favorites, default 65535 when empty (`favorite.py:52-63`).
    use pidash_db::entity::user_favorites as fav_entity;
    use sea_orm::sea_query::{Expr, Func};
    let largest = fav_entity::Entity::find()
        .filter(fav_entity::Column::WorkspaceId.eq(workspace_id))
        .filter(fav_entity::Column::DeletedAt.is_null())
        .select_only()
        .expr_as(
            Func::max(Expr::col(fav_entity::Column::Sequence)),
            "largest",
        )
        .into_model::<MaxSequenceRow>()
        .one(&sea_conn(&pool))
        .await
        .map_err(|_| Denial::ServerError)?;
    let sequence = largest
        .and_then(|row| row.largest)
        .map(|max| max + 10000.0)
        .unwrap_or(FAVORITE_SEQUENCE_DEFAULT);
    // `is_folder` is `boolean NOT NULL` with no DB default; Django supplies
    // it Python-side (`db/models/favorite.py:23`: `default=False`).
    use sea_orm::ActiveModelTrait;
    fav_entity::ActiveModel {
        id: sea_orm::Set(uuid::Uuid::new_v4()),
        workspace_id: sea_orm::Set(workspace_id),
        project_id: sea_orm::Set(Some(project_id)),
        user_id: sea_orm::Set(user_id),
        entity_type: sea_orm::Set(FAVORITE_ENTITY_TYPE.to_owned()),
        entity_identifier: sea_orm::Set(entity_identifier),
        is_folder: sea_orm::Set(false),
        sequence: sea_orm::Set(sequence),
        created_by_id: sea_orm::Set(Some(user_id)),
        updated_by_id: sea_orm::Set(None),
        created_at: sea_orm::Set(chrono::Utc::now().into()),
        updated_at: sea_orm::Set(chrono::Utc::now().into()),
        ..Default::default()
    }
    .insert(&sea_conn(&pool))
    .await
    .map_err(|error| {
        let sqlx_error = pidash_db::orm_tx::into_sqlx(error);
        match sqlx_error {
            // Partial-unique violation (`favorite.py:39-44`) → IntegrityError.
            sqlx::Error::Database(db) if db.code().as_deref() == Some("23505") => {
                Denial::BadError("The payload is not valid".to_owned())
            }
            _ => Denial::ServerError,
        }
    })?;
    // The view returns an empty 204 (`:579`); no activity is published.
    Ok(empty_response(StatusCode::NO_CONTENT))
}

/// `CycleFavoriteViewSet.destroy` (`base.py:581-591`).
async fn favorite_destroy(
    State(state): State<AppState>,
    Path((slug, project_raw, cycle_raw)): Path<(String, String, String)>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
    method: axum::http::Method,
    uri: axum::http::Uri,
    headers: axum::http::HeaderMap,
) -> HandlerResult {
    // Django's `<uuid:cycle_id>` never routes a non-canonical or garbage
    // spelling: the resolver 404 comes before auth, so proxy for everyone
    // rather than answering the view's 400 (member) / auth error (anon).
    if !super::is_canonical_uuid(&cycle_raw) {
        return Ok(super::proxy_segment(state, method, uri, headers).await);
    }
    let pool = pool_of(&state)?;
    let user_id = actor_user_id(extension)?;
    let project_id = resolve_project_id(&pool, &slug, &project_raw).await?;
    let facts = fetch_allow_facts(&pool, &slug, &project_id, &user_id).await?;
    check_gate(gate_for_destroy(), &slug, &facts)?;
    let cycle_id = parse_uuid_or_invalid(&cycle_raw)?;
    use pidash_db::entity::user_favorites as fav_entity;
    use pidash_db::entity::workspaces as ws_entity;
    use sea_orm::{ColumnTrait, EntityTrait, JoinType, QueryFilter, QuerySelect, RelationTrait};
    let rows: Vec<uuid::Uuid> = fav_entity::Entity::find()
        .join(JoinType::InnerJoin, fav_entity::Relation::Workspaces.def())
        .filter(fav_entity::Column::ProjectId.eq(project_id))
        .filter(fav_entity::Column::EntityType.eq(FAVORITE_ENTITY_TYPE))
        .filter(fav_entity::Column::UserId.eq(user_id))
        .filter(ws_entity::Column::Slug.eq(slug))
        .filter(fav_entity::Column::EntityIdentifier.eq(cycle_id))
        .filter(fav_entity::Column::DeletedAt.is_null())
        .select_only()
        .column(fav_entity::Column::Id)
        .into_tuple()
        .all(&sea_conn(&pool))
        .await
        .map_err(|_| Denial::ServerError)?;
    // `.get(...)` semantics: zero rows → `DoesNotExist` 404; several →
    // `MultipleObjectsReturned` → generic 500.
    let [favorite_id] = rows.as_slice() else {
        if rows.is_empty() {
            return Err(Denial::NotFound);
        }
        return Err(Denial::ServerError);
    };
    // Hard delete (`soft=False`, `:590`).
    fav_entity::Entity::delete_by_id(*favorite_id)
        .exec(&sea_conn(&pool))
        .await
        .map_err(|_| Denial::ServerError)?;
    Ok(empty_response(StatusCode::NO_CONTENT))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn gates_cover_both_owned_actions() {
        let create = gates::gate_for(
            "POST",
            "workspaces/<slug>/projects/<id>/user-favorite-cycles/",
        )
        .expect("create gate");
        assert_eq!(create.source, "base.py:571 (CycleFavoriteViewSet.create)");
        let destroy = gates::gate_for(
            "DELETE",
            "workspaces/<slug>/projects/<id>/user-favorite-cycles/<uuid>/",
        )
        .expect("destroy gate");
        assert_eq!(destroy.source, "base.py:581 (CycleFavoriteViewSet.destroy)");
        for row in [create, destroy] {
            assert!(matches!(row.gate, gates::Gate::Project { .. }));
        }
    }

    #[test]
    fn entity_identifier_coercion_matches_the_uuid_field() {
        assert_eq!(parse_entity_identifier(None).unwrap(), None);
        assert_eq!(parse_entity_identifier(Some(&Value::Null)).unwrap(), None);
        let id = uuid::Uuid::new_v4();
        assert_eq!(
            parse_entity_identifier(Some(&json!(id.to_string()))).unwrap(),
            Some(id)
        );
        // `UUIDField.to_python` coerces in-range ints and bools
        // (`input_form = "int"` for a Python `int`), which is exactly what
        // Django writes into `entity_identifier`.
        assert_eq!(
            parse_entity_identifier(Some(&json!(5))).unwrap(),
            Some(uuid::Uuid::from_u128(5))
        );
        assert_eq!(
            parse_entity_identifier(Some(&json!(true))).unwrap(),
            Some(uuid::Uuid::from_u128(1))
        );
        assert_eq!(
            parse_entity_identifier(Some(&json!(false))).unwrap(),
            Some(uuid::Uuid::from_u128(0))
        );
        assert!(parse_entity_identifier(Some(&json!("nope"))).is_err());
        assert!(parse_entity_identifier(Some(&json!(5.5))).is_err());
        assert!(parse_entity_identifier(Some(&json!(-1))).is_err());
        let over_max: Value =
            serde_json::from_str("340282366920938463463374607431768211456").expect("fixture");
        assert!(parse_entity_identifier(Some(&over_max)).is_err());
        assert!(parse_entity_identifier(Some(&json!([1]))).is_err());
        assert!(parse_entity_identifier(Some(&json!({"a": 1}))).is_err());
    }

    #[test]
    fn favorite_consts_match_the_fixtures() {
        // F-C27-10: `entity_type="cycle"`, no workspace on create, 204s.
        assert_eq!(FAVORITE_ENTITY_TYPE, "cycle");
        assert_eq!(FAVORITE_SEQUENCE_DEFAULT, 65535.0);
        assert!(FAVORITES_PATH.contains("user-favorite-cycles/"));
        assert!(FAVORITE_PATH.contains("user-favorite-cycles/{cycle_id}/"));
    }
}
