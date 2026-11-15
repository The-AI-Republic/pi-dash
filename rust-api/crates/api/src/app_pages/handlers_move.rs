#![forbid(unsafe_code)]

//! App page move (`PageMoveEndpoint`, `app/views/page/base.py:687-720`,
//! PDASHOSS01-347): `POST .../pages/<page_id>/move/` with
//! `{"project_id": <target>}` moves the page and its sub-pages; 204.
//!
//! Order, as Django runs it: `<uuid:page_id>` routing (a mismatch proxies to
//! Django's resolver 404), session auth (anonymous 401), the project
//! identifier rewrite (404 `{"detail":"Project not found"}`),
//! `ProjectEntityPermission` (POST: an active ADMIN/MEMBER row in this
//! project and workspace, else the DRF-default 403), the visible-page
//! lookup (404 `{"error":"Page not found"}` — NOT the `ProjectPagePermission`
//! gate the other page views use), then `request.data` and the shared
//! rules in [`pidash_services::domain::pages`].

use axum::extract::{Path, Request, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Extension;
use http_body_util::BodyExt;
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
use serde_json::Value;

use pidash_db::entity::{project_members, workspaces};
use pidash_services::domain::{self, pages as page_domain, WriteActor, WriteCtx};

use super::{actor_user_id, parse_page_id, pool_of, resolve_project_id, Denial};
use crate::middleware::SessionHandle;
use crate::state::AppState;
use pidash_compat::response::json_response;

/// `get_page_or_error`-style miss body (`base.py:714-715`).
pub const PAGE_NOT_FOUND_BODY: &str = r#"{"error":"Page not found"}"#;

/// `POST .../pages/<page_id>/move/`.
pub async fn move_page(
    State(state): State<AppState>,
    Path((slug, project_raw, page_raw)): Path<(String, String, String)>,
    extension: Option<Extension<SessionHandle>>,
    req: Request,
) -> Response {
    let Ok(page_id) = parse_page_id(&page_raw) else {
        return crate::edge::proxy(State(state), req).await;
    };
    let user_id = match actor_user_id(extension) {
        Ok(user_id) => user_id,
        Err(denial) => return denial.into_response(),
    };
    let pool = match pool_of(&state) {
        Ok(pool) => pool,
        Err(denial) => return denial.into_response(),
    };
    let project_id = match resolve_project_id(pool, &slug, &project_raw).await {
        Ok(id) => id,
        Err(denial) => return denial.into_response(),
    };
    let Some(orm) = state.orm() else {
        return Denial::ServerError.into_response();
    };
    match entity_write_allowed(orm, &slug, project_id, user_id).await {
        Ok(true) => {}
        Ok(false) => {
            return json_response(
                StatusCode::FORBIDDEN,
                crate::permissions::DEFAULT_DENIED_BODY.to_owned(),
            )
        }
        Err(()) => return Denial::ServerError.into_response(),
    }
    let page = match page_domain::find_visible_page(orm, &slug, project_id, user_id, page_id).await
    {
        Ok(page) => page,
        Err(error) => {
            tracing::warn!(%error, "app page move lookup failed");
            return Denial::ServerError.into_response();
        }
    };
    let Some(page) = page else {
        return json_response(StatusCode::NOT_FOUND, PAGE_NOT_FOUND_BODY.to_owned());
    };
    let (parts, body) = req.into_parts();
    let bytes = match body.collect().await {
        Ok(collected) => collected.to_bytes(),
        Err(_) => return Denial::ServerError.into_response(),
    };
    let data = match request_data(&parts.headers, &bytes) {
        Ok(data) => data,
        Err(response) => return response,
    };
    let target = match page_domain::parse_target_project(data.get("project_id")) {
        Ok(target) => target,
        Err(refusal) => return refusal_response(refusal),
    };
    let ctx = WriteCtx::new(WriteActor {
        user_id,
        timezone: None,
    });
    let input = page_domain::PageMove {
        page_id,
        workspace_id: page.workspace_id,
        source_project_id: project_id,
        target_project_id: target,
    };
    // `timezone.now()`: microsecond precision, like the stored column.
    let now = chrono::Utc::now();
    let now = now - chrono::Duration::nanoseconds(i64::from(now.timestamp_subsec_nanos() % 1000));
    let outcome = domain::run(&ctx, orm, |txn| {
        let ctx = ctx.clone();
        Box::pin(async move { page_domain::move_to_project(&ctx, txn, input, now).await })
    })
    .await;
    match outcome {
        Ok(Ok(_target)) => StatusCode::NO_CONTENT.into_response(),
        Ok(Err(refusal)) => refusal_response(refusal),
        Err(error) => {
            tracing::warn!(%error, "app page move failed");
            Denial::ServerError.into_response()
        }
    }
}

/// `ProjectEntityPermission` for an unsafe method (`app/permissions/
/// project.py`): `ProjectMember.objects.filter(workspace__slug=slug,
/// member=user, role__in=[ADMIN, MEMBER], project_id=project,
/// is_active=True).exists()`.
async fn entity_write_allowed(
    orm: &sea_orm::DatabaseConnection,
    slug: &str,
    project_id: uuid::Uuid,
    user_id: uuid::Uuid,
) -> Result<bool, ()> {
    use sea_orm::sea_query::Query;
    let workspace = Query::select()
        .column(workspaces::Column::Id)
        .from(workspaces::Entity)
        .and_where(workspaces::Column::Slug.eq(slug))
        .to_owned();
    project_members::Entity::find()
        .filter(project_members::Column::WorkspaceId.in_subquery(workspace))
        .filter(project_members::Column::MemberId.eq(user_id))
        .filter(project_members::Column::ProjectId.eq(project_id))
        .filter(project_members::Column::Role.is_in([20i16, 15i16]))
        .filter(project_members::Column::IsActive.eq(true))
        .filter(project_members::Column::DeletedAt.is_null())
        .one(orm)
        .await
        .map(|row| row.is_some())
        .map_err(|error| tracing::warn!(%error, "app page move gate failed"))
}

/// DRF `request.data` for the move body (default JSON/form/multipart
/// parsers): empty bodies are `{}`; malformed JSON answers the 400
/// `ParseError` detail; unknown content types the 415. A non-object body
/// has no `.get`, so it reads as an empty object (→ "project_id is
/// required").
#[allow(clippy::result_large_err)]
fn request_data(
    headers: &HeaderMap,
    body: &[u8],
) -> Result<serde_json::Map<String, Value>, Response> {
    use crate::v1_cycles_modules::body::{negotiate_body, BodyError, BodySpec, NegotiatedBody};
    use crate::v1_cycles_modules::json::{parse_json_text_spans, to_serde_publish_map, JsonFail};
    const SPEC: BodySpec = BodySpec {
        list_fields: &[],
        skip_blank_fields: &[],
    };
    let detail = |status: StatusCode, message: String| {
        let body = serde_json::json!({ "detail": message }).to_string();
        json_response(status, body)
    };
    match negotiate_body(headers, body, &SPEC) {
        Err(BodyError::UnsupportedMediaType(message)) => {
            Err(detail(StatusCode::UNSUPPORTED_MEDIA_TYPE, message))
        }
        Err(BodyError::ParseDetail(message)) => Err(detail(StatusCode::BAD_REQUEST, message)),
        Err(BodyError::ServerError) => Err(Denial::ServerError.into_response()),
        Ok(NegotiatedBody::Empty) => Ok(serde_json::Map::new()),
        Ok(NegotiatedBody::Form { map, .. }) => Ok(map),
        Ok(NegotiatedBody::JsonText { text, surr }) => match parse_json_text_spans(&text, &surr) {
            Err(JsonFail::Message(message)) => Err(detail(
                StatusCode::BAD_REQUEST,
                format!("JSON parse error - {message}"),
            )),
            Err(JsonFail::Recursion) => Err(Denial::ServerError.into_response()),
            Ok(value) => Ok(match value.into_object() {
                Some(object) => to_serde_publish_map(&object),
                None => serde_json::Map::new(),
            }),
        },
    }
}

/// A `PageMoveError` response: its status and exact body.
fn refusal_response(refusal: page_domain::MoveRefusal) -> Response {
    let status = StatusCode::from_u16(refusal.status()).expect("refusal status");
    json_response(status, refusal.body().to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::header;

    /// Headers for `body` as a client sends them: an absent
    /// `Content-Length` reads as an empty body (DRF `_load_stream`).
    fn headers_for(content_type: &str, body: &[u8]) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(header::CONTENT_TYPE, content_type.parse().unwrap());
        headers.insert(header::CONTENT_LENGTH, body.len().into());
        headers
    }

    fn json_headers(body: &[u8]) -> HeaderMap {
        headers_for("application/json", body)
    }

    #[test]
    fn object_body_exposes_project_id() {
        let data = {
            let body = br#"{"project_id":"x"}"#;
            request_data(&json_headers(body), body)
        }
        .unwrap();
        assert_eq!(data.get("project_id"), Some(&Value::from("x")));
    }

    #[test]
    fn non_object_and_empty_bodies_have_no_project_id() {
        for body in [&b"[1]"[..], b"\"s\"", b"null", b"3", b""] {
            let data = request_data(&json_headers(body), body).unwrap();
            assert!(data.get("project_id").is_none(), "{body:?}");
        }
    }

    #[test]
    fn form_body_last_value_wins() {
        let body = b"project_id=a&project_id=b";
        let headers = headers_for("application/x-www-form-urlencoded", body);
        let data = request_data(&headers, body).unwrap();
        assert_eq!(data.get("project_id"), Some(&Value::from("b")));
    }

    #[test]
    fn malformed_json_is_parse_error() {
        let response = request_data(&json_headers(b"{"), b"{").unwrap_err();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }
}
