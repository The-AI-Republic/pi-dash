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

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Router;
use serde_json::Value;

use crate::app_cycles::gates;
use crate::app_issues::Denial;
use crate::state::AppState;

use super::handlers_cycle_issues::{
    actor_user_id, check_gate, empty_response, fetch_allow_facts, parse_uuid_or_invalid, pool_of,
    resolve_project_id, HandlerResult, INVALID_DETAIL_MSG,
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

/// Coerce `request.data.get("cycle")` to the `entity_identifier` column:
/// absent/`null` stays NULL; strings must parse as UUIDs
/// (`ValidationError` → 400); any other JSON type is a `ValidationError`
/// too, matching the `UUIDField` coercion.
fn parse_entity_identifier(value: Option<&Value>) -> Result<Option<uuid::Uuid>, Denial> {
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(raw)) => parse_uuid_or_invalid(raw).map(Some),
        Some(_) => Err(Denial::BadError(INVALID_DETAIL_MSG.to_owned())),
    }
}

/// `CycleFavoriteViewSet.create` (`base.py:571-579`).
async fn favorite_create(
    State(state): State<AppState>,
    Path((slug, project_raw)): Path<(String, String)>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
    body: axum::Json<Value>,
) -> HandlerResult {
    let pool = pool_of(&state)?;
    let user_id = actor_user_id(extension)?;
    let project_id = resolve_project_id(&pool, &slug, &project_raw).await?;
    let facts = fetch_allow_facts(&pool, &slug, &project_id, &user_id).await?;
    check_gate(gate_for_create(), &slug, &facts)?;
    let entity_identifier = match body.0.as_object() {
        Some(_) => parse_entity_identifier(body.0.get("cycle"))?,
        // Non-object bodies cannot `.get` (AttributeError → 500).
        None => return Err(Denial::ServerError),
    };
    // The view passes no workspace; `WorkspaceBaseModel.save` derives it
    // from the project (`db/models/workspace.py:192-195`).
    let workspace_id: Option<(uuid::Uuid,)> = sqlx::query_as(
        r#"SELECT p.workspace_id FROM projects p
           WHERE p.id = $1 AND p.deleted_at IS NULL"#,
    )
    .bind(project_id)
    .fetch_optional(&pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    let (workspace_id,) = workspace_id.ok_or(Denial::ServerError)?;
    // `UserFavorite.save`: `Max(sequence) + 10000` over the workspace's
    // live favorites, default 65535 when empty (`favorite.py:52-63`).
    let largest: Option<(Option<f64>,)> = sqlx::query_as(
        r#"SELECT MAX(sequence) FROM user_favorites
           WHERE workspace_id = $1 AND deleted_at IS NULL"#,
    )
    .bind(workspace_id)
    .fetch_optional(&pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    let sequence = largest
        .and_then(|(max,)| max)
        .map(|max| max + 10000.0)
        .unwrap_or(FAVORITE_SEQUENCE_DEFAULT);
    // `is_folder` is `boolean NOT NULL` with no DB default; Django supplies
    // it Python-side (`db/models/favorite.py:23`: `default=False`).
    sqlx::query(
        r#"INSERT INTO user_favorites
           (id, workspace_id, project_id, user_id, entity_type, entity_identifier,
            is_folder, sequence, created_by_id, updated_by_id, created_at, updated_at)
           VALUES (gen_random_uuid(), $1, $2, $3, $4, $5, FALSE, $6, $7, NULL, now(), now())"#,
    )
    .bind(workspace_id)
    .bind(project_id)
    .bind(user_id)
    .bind(FAVORITE_ENTITY_TYPE)
    .bind(entity_identifier)
    .bind(sequence)
    .bind(user_id)
    .execute(&pool)
    .await
    .map_err(|error| match error {
        // Partial-unique violation (`favorite.py:39-44`) → IntegrityError.
        sqlx::Error::Database(db) if db.code().as_deref() == Some("23505") => {
            Denial::BadError("The payload is not valid".to_owned())
        }
        _ => Denial::ServerError,
    })?;
    // The view returns an empty 204 (`:579`); no activity is published.
    Ok(empty_response(StatusCode::NO_CONTENT))
}

/// `CycleFavoriteViewSet.destroy` (`base.py:581-591`).
async fn favorite_destroy(
    State(state): State<AppState>,
    Path((slug, project_raw, cycle_raw)): Path<(String, String, String)>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
) -> HandlerResult {
    let pool = pool_of(&state)?;
    let user_id = actor_user_id(extension)?;
    let project_id = resolve_project_id(&pool, &slug, &project_raw).await?;
    let facts = fetch_allow_facts(&pool, &slug, &project_id, &user_id).await?;
    check_gate(gate_for_destroy(), &slug, &facts)?;
    let cycle_id = parse_uuid_or_invalid(&cycle_raw)?;
    let rows: Vec<(uuid::Uuid,)> = sqlx::query_as(
        r#"SELECT uf.id FROM user_favorites uf
           JOIN workspaces w ON w.id = uf.workspace_id
           WHERE uf.project_id = $1 AND uf.entity_type = $2 AND uf.user_id = $3
             AND w.slug = $4 AND uf.entity_identifier = $5
             AND uf.deleted_at IS NULL"#,
    )
    .bind(project_id)
    .bind(FAVORITE_ENTITY_TYPE)
    .bind(user_id)
    .bind(&slug)
    .bind(cycle_id)
    .fetch_all(&pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    // `.get(...)` semantics: zero rows → `DoesNotExist` 404; several →
    // `MultipleObjectsReturned` → generic 500.
    let [(favorite_id,)] = rows.as_slice() else {
        if rows.is_empty() {
            return Err(Denial::NotFound);
        }
        return Err(Denial::ServerError);
    };
    // Hard delete (`soft=False`, `:590`).
    sqlx::query(r#"DELETE FROM user_favorites WHERE id = $1"#)
        .bind(*favorite_id)
        .execute(&pool)
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
        assert!(parse_entity_identifier(Some(&json!("nope"))).is_err());
        assert!(parse_entity_identifier(Some(&json!(7))).is_err());
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
