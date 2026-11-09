#![forbid(unsafe_code)]

//! Instance-admin catalog CRUD (NEWPIDASH-169).
//!
//! `GET /api/instances/runner-models/` — every row (including disabled
//! ones, so the god-mode table can toggle them back), grouped by agent
//! in sort order.
//! `POST /api/instances/runner-models/` — validate, then insert, `201`.
//! `PATCH /api/instances/runner-models/<id>/` — partial update (label,
//! model, efforts, sort order, enabled, preselected), `200`.
//! `DELETE /api/instances/runner-models/<id>/` — hard delete, `204`.
//!
//! Every handler is gated on the instance-admin check (role >= 15 for
//! the actor's user on the first instance row); anonymous is 401, a
//! signed-in non-admin is 403. Setting `is_preselected` clears it on
//! the agent's other rows in the same transaction. Deleting a model
//! never touches runners (their model lives in their own config).

use axum::body::Bytes;
use axum::extract::{Extension, Path, State};
use axum::http::{HeaderMap, Method, Request, StatusCode, Uri};
use axum::response::Response;
use axum::routing::{get, patch};
use axum::Router;
use chrono::Utc;
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, EntityTrait, PaginatorTrait, QueryFilter,
    QueryOrder, QuerySelect,
};
use uuid::Uuid;

use pidash_db::entity::rust_runner_model_catalog as catalog_table;
use pidash_db::orm_tx::TxConn;

use crate::edge;
use crate::middleware::SessionHandle;
use crate::state::AppState;
use crate::uuid_path::is_canonical_uuid;

use super::{
    all_rows, authed_actor, json_response, parse_json_body, pools, render_grouped, render_row,
    require_instance_admin, Denial,
};

/// `GET`/`POST /api/instances/runner-models/`.
pub const LIST_PATH: &str = "/api/instances/runner-models/";
/// `PATCH`/`DELETE /api/instances/runner-models/<id>/`.
pub const DETAIL_PATH: &str = "/api/instances/runner-models/{id}/";

/// Register the admin paths (cutover granularity: owned methods serve
/// from Rust, everything else proxies to Django's own 404/405).
pub fn routes() -> Router<AppState> {
    Router::new()
        .route(
            LIST_PATH,
            get(list_models)
                .post(create_model)
                .put(edge::proxy)
                .patch(edge::proxy)
                .delete(edge::proxy)
                .head(edge::proxy)
                .options(edge::proxy),
        )
        .route(
            DETAIL_PATH,
            patch(patch_model)
                .delete(delete_model)
                .get(edge::proxy)
                .post(edge::proxy)
                .put(edge::proxy)
                .head(edge::proxy)
                .options(edge::proxy),
        )
}

async fn actor_admin(
    state: &AppState,
    extension: Option<Extension<SessionHandle>>,
) -> Result<(Uuid, sqlx::PgPool), Denial> {
    let pool = pools(state)?;
    let actor = authed_actor(state, &pool, extension).await?;
    require_instance_admin(&pool, &actor).await?;
    Ok((actor, pool))
}

async fn list_models(
    State(state): State<AppState>,
    extension: Option<Extension<SessionHandle>>,
) -> Result<Response, Denial> {
    let (_, pool) = actor_admin(&state, extension).await?;
    let rows = all_rows(&pool, false).await?;
    Ok(json_response(StatusCode::OK, &render_grouped(&rows)))
}

async fn create_model(
    State(state): State<AppState>,
    extension: Option<Extension<SessionHandle>>,
    body: Bytes,
) -> Result<Response, Denial> {
    let (_, pool) = actor_admin(&state, extension).await?;
    let parsed = parse_json_body(&body)?;
    let validated = super::guards::validate_create(&parsed).map_err(Denial::BadRequest)?;
    let db = pidash_db::sea_orm_connection(&pool);

    // Duplicate (agent, model): the unique index would answer 500, so
    // check first and answer the validation code. The catalog is tiny
    // (tens of rows), so fetch the agent's rows and compare in Rust.
    let existing = catalog_table::Entity::find()
        .filter(catalog_table::Column::Agent.eq(validated.agent.clone()))
        .all(&db)
        .await
        .map_err(|_| Denial::ServerError)?;
    let refs: Vec<(&str, &str)> = existing
        .iter()
        .map(|row| (row.agent.as_str(), row.model.as_str()))
        .collect();
    if super::guards::is_duplicate(&refs, &validated.agent, &validated.model) {
        return Err(Denial::BadRequest("duplicate_model"));
    }

    let now = Utc::now().fixed_offset();
    let sort_order = match validated.sort_order {
        Some(sort_order) => sort_order,
        None => {
            let max = catalog_table::Entity::find()
                .filter(catalog_table::Column::Agent.eq(validated.agent.clone()))
                .order_by_desc(catalog_table::Column::SortOrder)
                .limit(1)
                .one(&db)
                .await
                .map_err(|_| Denial::ServerError)?;
            max.map(|row| row.sort_order.saturating_add(1)).unwrap_or(0)
        }
    };
    let preselected = validated.is_preselected.unwrap_or(false);

    let new_id = Uuid::new_v4();
    let mut tx = pool.begin().await.map_err(|_| Denial::ServerError)?;
    {
        let conn = TxConn::new(&mut tx);
        if preselected {
            clear_preselected(&validated.agent, None, &conn).await?;
        }
        let active = catalog_table::ActiveModel {
            id: Set(new_id),
            agent: Set(validated.agent),
            model: Set(validated.model),
            label: Set(validated.label),
            reasoning_efforts: Set(validated.reasoning_efforts),
            sort_order: Set(sort_order),
            is_enabled: Set(validated.is_enabled.unwrap_or(true)),
            is_preselected: Set(preselected),
            created_at: Set(now),
            updated_at: Set(now),
        };
        catalog_table::Entity::insert(active)
            .exec(&conn)
            .await
            .map_err(|_| Denial::ServerError)?;
    };
    tx.commit().await.map_err(|_| Denial::ServerError)?;
    let created = catalog_table::Entity::find_by_id(new_id)
        .one(&db)
        .await
        .map_err(|_| Denial::ServerError)?
        .ok_or(Denial::ServerError)?;
    Ok(json_response(StatusCode::CREATED, &render_row(&created)))
}

async fn patch_model(
    State(state): State<AppState>,
    extension: Option<Extension<SessionHandle>>,
    Path(id): Path<String>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, Denial> {
    let row_id = match id.parse::<Uuid>() {
        Ok(row_id) if is_canonical_uuid(&id) => row_id,
        _ => return proxy_pass(state, method, uri, headers, &body).await,
    };
    let (_, pool) = actor_admin(&state, extension).await?;
    let parsed = parse_json_body(&body)?;
    let validated = super::guards::validate_patch(&parsed).map_err(Denial::BadRequest)?;

    let db = pidash_db::sea_orm_connection(&pool);
    let existing = catalog_table::Entity::find_by_id(row_id)
        .one(&db)
        .await
        .map_err(|_| Denial::ServerError)?;
    let Some(existing) = existing else {
        return Err(Denial::NotFound);
    };

    // Duplicate check on a model rename (same agent, another row's model).
    if let Some(model) = &validated.model {
        let clash = catalog_table::Entity::find()
            .filter(catalog_table::Column::Agent.eq(existing.agent.clone()))
            .filter(catalog_table::Column::Model.eq(model.clone()))
            .filter(catalog_table::Column::Id.ne(row_id))
            .exists(&db)
            .await
            .map_err(|_| Denial::ServerError)?;
        if clash {
            return Err(Denial::BadRequest("duplicate_model"));
        }
    }

    let preselect = validated.is_preselected.unwrap_or(existing.is_preselected);
    let mut tx = pool.begin().await.map_err(|_| Denial::ServerError)?;
    let updated = {
        let conn = TxConn::new(&mut tx);
        if preselect {
            clear_preselected(&existing.agent, Some(row_id), &conn).await?;
        }
        let mut active: catalog_table::ActiveModel = existing.clone().into();
        if let Some(model) = validated.model {
            active.model = Set(model);
        }
        if let Some(label) = validated.label {
            active.label = Set(label);
        }
        if let Some(efforts) = validated.reasoning_efforts {
            active.reasoning_efforts = Set(efforts);
        }
        if let Some(sort_order) = validated.sort_order {
            active.sort_order = Set(sort_order);
        }
        if let Some(enabled) = validated.is_enabled {
            active.is_enabled = Set(enabled);
        }
        active.is_preselected = Set(preselect);
        active.updated_at = Set(Utc::now().fixed_offset());
        active
            .update(&conn)
            .await
            .map_err(|_| Denial::ServerError)?
    };
    tx.commit().await.map_err(|_| Denial::ServerError)?;
    Ok(json_response(StatusCode::OK, &render_row(&updated)))
}

async fn delete_model(
    State(state): State<AppState>,
    extension: Option<Extension<SessionHandle>>,
    Path(id): Path<String>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
) -> Result<Response, Denial> {
    let row_id = match id.parse::<Uuid>() {
        Ok(row_id) if is_canonical_uuid(&id) => row_id,
        _ => return proxy_pass(state, method, uri, headers, &[]).await,
    };
    let (_, pool) = actor_admin(&state, extension).await?;
    let db = pidash_db::sea_orm_connection(&pool);
    let result = catalog_table::Entity::delete_by_id(row_id)
        .exec(&db)
        .await
        .map_err(|_| Denial::ServerError)?;
    if result.rows_affected == 0 {
        return Err(Denial::NotFound);
    }
    Ok(Response::builder()
        .status(StatusCode::NO_CONTENT)
        .body(axum::body::Body::empty())
        .expect("empty 204 response"))
}

/// Clear `is_preselected` on the agent's other rows (within an open
/// transaction, so the set is atomic with the insert/update). `keep`
/// excludes the row being patched; create passes `None`.
async fn clear_preselected(
    agent: &str,
    keep: Option<Uuid>,
    conn: &TxConn<'_>,
) -> Result<(), Denial> {
    let mut update = catalog_table::Entity::update_many()
        .col_expr(
            catalog_table::Column::IsPreselected,
            sea_orm::sea_query::Expr::value(false),
        )
        .filter(catalog_table::Column::Agent.eq(agent.to_owned()))
        .filter(catalog_table::Column::IsPreselected.eq(true));
    if let Some(keep) = keep {
        update = update.filter(catalog_table::Column::Id.ne(keep));
    }
    update.exec(conn).await.map_err(|_| Denial::ServerError)?;
    Ok(())
}

/// Non-canonical id segments proxy to Django for its resolver 404
/// (same shape as `crate::r#loop::admin::proxy_pass`).
async fn proxy_pass(
    state: AppState,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: &[u8],
) -> Result<Response, Denial> {
    let mut builder = Request::builder().method(method).uri(uri);
    for (name, value) in headers.iter() {
        builder = builder.header(name, value);
    }
    let req = builder
        .body(axum::body::Body::from(body.to_vec()))
        .map_err(|_| Denial::ServerError)?;
    Ok(edge::proxy(State(state), req).await)
}
