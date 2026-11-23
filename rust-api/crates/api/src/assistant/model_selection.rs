//! Per-surface model selection endpoints (NEWPIDASH-392, OSS-1, §6.1).
//!
//! `GET /api/users/me/ai-assistant/model-selection/` renders every
//! surface's saved entry, effective choice and fallback reason;
//! `PUT .../model-selection/{surface}/` validates one entry through the
//! installed seam and writes that surface atomically;
//! `DELETE .../model-selection/{surface}/` clears the entry (the surface
//! falls back to the default). The `assistant_models` namespace stays
//! out of the settings schema, so the generic profile PATCH refuses it
//! and this module is the only writer.
//!
//! Session auth only, like the other `/api/users/me/ai-assistant/`
//! routes; no throttle class (matching `/config/` PUT).

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Router;
use serde_json::Value;

use pidash_services::assistant::model_selection::{
    self as selection, ChoiceError, ModelChoice, Resolution,
};
use pidash_services::extensions::ChoiceOperational;
use pidash_types::assistant::surface::Surface;

use super::common::{actor, body_get, pool_of, request_content_type, BodyField, BodyMap, Failure};
use crate::state::AppState;

/// `assistant/urls.py` style, under the `api/` include: the collection.
pub const MODEL_SELECTION_PATH: &str = "/api/users/me/ai-assistant/model-selection/";
/// The per-surface item (`{surface}` is `web_assistant`,
/// `desktop_assistant` or `agent`; anything else is `unknown_surface`).
pub const MODEL_SELECTION_SURFACE_PATH: &str =
    "/api/users/me/ai-assistant/model-selection/{surface}/";

/// Owned model-selection routes. Sibling paths stay unmatched and proxy
/// to Django through the fallback; every unowned method on the owned
/// paths also proxies so Django answers its own 405-after-auth and
/// metadata OPTIONS byte for byte (the loop-handlers precedent).
pub fn routes() -> Router<AppState> {
    use axum::routing::get;
    Router::new()
        .route(MODEL_SELECTION_PATH, owned_collection(get(get_selection)))
        .route(
            MODEL_SELECTION_SURFACE_PATH,
            owned_item(axum::routing::put(put_selection).delete(delete_selection)),
        )
}

/// The collection path: GET serves from Rust, everything else falls
/// through to Django.
fn owned_collection(
    owned: axum::routing::MethodRouter<AppState>,
) -> axum::routing::MethodRouter<AppState> {
    owned
        .post(crate::edge::proxy)
        .put(crate::edge::proxy)
        .patch(crate::edge::proxy)
        .delete(crate::edge::proxy)
        .head(crate::edge::proxy)
        .options(crate::edge::proxy)
}

/// The item path: PUT + DELETE serve from Rust, everything else falls
/// through to Django.
fn owned_item(
    owned: axum::routing::MethodRouter<AppState>,
) -> axum::routing::MethodRouter<AppState> {
    owned
        .get(crate::edge::proxy)
        .post(crate::edge::proxy)
        .patch(crate::edge::proxy)
        .head(crate::edge::proxy)
        .options(crate::edge::proxy)
}

// ---------------------------------------------------------------------------
// responses
// ---------------------------------------------------------------------------

/// Render one surface (`§6.1` GET shape): the saved entry (as stored),
/// the effective choice and the fallback reason (null when the saved
/// entry applied).
fn render_surface(saved: Option<ModelChoice>, resolution: &Resolution) -> Value {
    serde_json::json!({
        "saved": saved.map(|choice| choice.to_json()).unwrap_or(Value::Null),
        "effective": resolution
            .effective
            .as_ref()
            .map(|choice| choice.to_json())
            .unwrap_or(Value::Null),
        "fallback_reason": resolution
            .fallback_reason
            .map(|reason| Value::String(reason.as_str().to_owned()))
            .unwrap_or(Value::Null),
    })
}

/// Read one surface's saved entry for display (the resolver already
/// read it for policy; this second read is display-only).
async fn saved_entry(
    pool: &sqlx::PgPool,
    user_id: uuid::Uuid,
    surface: Surface,
) -> Result<Option<ModelChoice>, Response> {
    let db = pidash_db::pool::sea_orm_connection(pool);
    let settings = pidash_services::domain::assistant::read_settings_bag(&db, user_id)
        .await
        .map_err(|_| Failure::server_error().into_response())?;
    Ok(settings
        .as_ref()
        .and_then(|settings| selection::read_entry(settings, surface)))
}

fn operational(error: ChoiceOperational) -> Response {
    let _ = error;
    Failure::server_error().into_response()
}

/// `GET users/me/ai-assistant/model-selection/`.
async fn get_selection(
    State(state): State<AppState>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
) -> Response {
    let pool = match pool_of(&state) {
        Ok(pool) => pool,
        Err(failure) => return failure.into_response(),
    };
    let actor = match actor(&state, &pool, extension).await {
        Ok(actor) => actor,
        Err(failure) => return failure.into_response(),
    };
    let mut surfaces = serde_json::Map::with_capacity(3);
    for surface in Surface::all() {
        let saved = match saved_entry(&pool, actor.id, surface).await {
            Ok(saved) => saved,
            Err(response) => return response,
        };
        let resolution = match state
            .assistant_seam()
            .resolve_choice(actor.id, surface)
            .await
        {
            Ok(resolution) => resolution,
            Err(error) => return operational(error),
        };
        surfaces.insert(
            surface.as_str().to_owned(),
            render_surface(saved, &resolution),
        );
    }
    crate::license::json_response(serde_json::Value::Object(surfaces))
}

// ---------------------------------------------------------------------------
// PUT validation
// ---------------------------------------------------------------------------

/// Parse one PUT body into a [`ModelChoice`]. Shape failures reuse the
/// closed `§6.1` code set: a missing or empty model is
/// `model_not_available`, an unparseable connection id is
/// `connection_not_found`.
fn parse_entry(body: &BodyMap) -> Result<ModelChoice, (StatusCode, String, String)> {
    let bad =
        |code: &str, detail: &str| (StatusCode::BAD_REQUEST, code.to_owned(), detail.to_owned());
    let model = match body_get(body, "model") {
        Some(BodyField::Json(Value::String(model))) => model.trim().to_owned(),
        _ => {
            return Err(bad(
                selection::choice_code::MODEL_NOT_AVAILABLE,
                "A model is required.",
            ));
        }
    };
    if model.is_empty() {
        return Err(bad(
            selection::choice_code::MODEL_NOT_AVAILABLE,
            "A model is required.",
        ));
    }
    let connection_id = match body_get(body, "connection_id") {
        None | Some(BodyField::Json(Value::Null)) => None,
        Some(BodyField::Json(Value::String(id))) => match id.parse::<uuid::Uuid>() {
            Ok(id) => Some(id),
            Err(_) => {
                return Err(bad(
                    selection::choice_code::CONNECTION_NOT_FOUND,
                    "Unknown connection.",
                ));
            }
        },
        _ => {
            return Err(bad(
                selection::choice_code::CONNECTION_NOT_FOUND,
                "Unknown connection.",
            ));
        }
    };
    Ok(ModelChoice {
        model,
        connection_id,
    })
}

/// `PUT users/me/ai-assistant/model-selection/{surface}/`.
async fn put_selection(
    State(state): State<AppState>,
    Path(surface): Path<String>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
    headers: axum::http::HeaderMap,
    body: axum::body::Bytes,
) -> Response {
    let pool = match pool_of(&state) {
        Ok(pool) => pool,
        Err(failure) => return failure.into_response(),
    };
    let actor = match actor(&state, &pool, extension).await {
        Ok(actor) => actor,
        Err(failure) => return failure.into_response(),
    };
    let Some(surface) = Surface::parse(&surface) else {
        return Failure::error_body(
            StatusCode::BAD_REQUEST,
            selection::choice_code::UNKNOWN_SURFACE,
            "Unknown surface.",
        )
        .into_response();
    };
    let data = match super::common::parse_put_body(&body, request_content_type(&headers)) {
        Ok(data) => data,
        Err(failure) => return failure.into_response(),
    };
    let entry = match parse_entry(&data) {
        Ok(entry) => entry,
        Err((status, code, detail)) => {
            return Failure::error_body(status, &code, &detail).into_response();
        }
    };
    let choice = match state
        .assistant_seam()
        .validate_choice(actor.id, surface, &entry)
        .await
    {
        Ok(choice) => choice,
        Err(ChoiceError::Invalid(invalid)) => {
            return Failure::error_body(StatusCode::BAD_REQUEST, invalid.code, &invalid.detail)
                .into_response();
        }
        Err(ChoiceError::Operational(error)) => return operational(error),
    };
    let now = chrono::Utc::now();
    let db = pidash_db::pool::sea_orm_connection(&pool);
    match pidash_services::domain::assistant::save_assistant_model_entry(
        &db,
        actor.id,
        surface,
        &choice.to_json(),
        now,
    )
    .await
    {
        Ok(0) => return Failure::server_error().into_response(),
        Ok(_) => {}
        Err(_) => return Failure::server_error().into_response(),
    }
    let resolution = match state
        .assistant_seam()
        .resolve_choice(actor.id, surface)
        .await
    {
        Ok(resolution) => resolution,
        Err(error) => return operational(error),
    };
    crate::license::json_response(render_surface(Some(choice), &resolution))
}

/// `DELETE users/me/ai-assistant/model-selection/{surface}/`: clear the
/// entry (the surface falls back to the default). Answers 204 with no
/// body, like the sibling config DELETE.
async fn delete_selection(
    State(state): State<AppState>,
    Path(surface): Path<String>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
) -> Response {
    let pool = match pool_of(&state) {
        Ok(pool) => pool,
        Err(failure) => return failure.into_response(),
    };
    let actor = match actor(&state, &pool, extension).await {
        Ok(actor) => actor,
        Err(failure) => return failure.into_response(),
    };
    let Some(surface) = Surface::parse(&surface) else {
        return Failure::error_body(
            StatusCode::BAD_REQUEST,
            selection::choice_code::UNKNOWN_SURFACE,
            "Unknown surface.",
        )
        .into_response();
    };
    let now = chrono::Utc::now();
    let db = pidash_db::pool::sea_orm_connection(&pool);
    if pidash_services::domain::assistant::clear_assistant_model_entry(&db, actor.id, surface, now)
        .await
        .is_err()
    {
        return Failure::server_error().into_response();
    }
    Response::builder()
        .status(StatusCode::NO_CONTENT)
        .header(axum::http::header::CONTENT_TYPE, "application/json")
        .body(axum::body::Body::empty())
        .expect("delete response")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn body(entries: &[(&str, BodyField)]) -> BodyMap {
        entries
            .iter()
            .map(|(key, value)| ((*key).to_owned(), value.clone()))
            .collect()
    }

    #[test]
    fn put_body_parses_model_and_connection() {
        let data = body(&[
            ("model", BodyField::Json(json!("gpt-6"))),
            (
                "connection_id",
                BodyField::Json(json!("6f1c2b3a-9d4e-4f5a-8b6c-7d8e9f0a1b2c")),
            ),
        ]);
        let entry = parse_entry(&data).expect("parses");
        assert_eq!(entry.model, "gpt-6");
        assert_eq!(
            entry.connection_id.expect("connection").to_string(),
            "6f1c2b3a-9d4e-4f5a-8b6c-7d8e9f0a1b2c"
        );
    }

    #[test]
    fn put_body_trims_model_and_drops_null_connection() {
        let data = body(&[
            ("model", BodyField::Json(json!("  gpt-6  "))),
            ("connection_id", BodyField::Json(Value::Null)),
        ]);
        let entry = parse_entry(&data).expect("parses");
        assert_eq!(entry.model, "gpt-6");
        assert_eq!(entry.connection_id, None);
    }

    #[test]
    fn put_body_rejects_missing_and_empty_model() {
        for data in [
            body(&[]),
            body(&[("model", BodyField::Json(json!("   ")))]),
            body(&[("model", BodyField::Json(json!(7)))]),
        ] {
            let (status, code, _) = parse_entry(&data).expect_err("invalid");
            assert_eq!(status, StatusCode::BAD_REQUEST);
            assert_eq!(code, selection::choice_code::MODEL_NOT_AVAILABLE);
        }
    }

    #[test]
    fn put_body_rejects_bad_connection_id() {
        for data in [
            body(&[
                ("model", BodyField::Json(json!("m"))),
                ("connection_id", BodyField::Json(json!("nope"))),
            ]),
            body(&[
                ("model", BodyField::Json(json!("m"))),
                ("connection_id", BodyField::Json(json!(7))),
            ]),
        ] {
            let (_, code, _) = parse_entry(&data).expect_err("invalid");
            assert_eq!(code, selection::choice_code::CONNECTION_NOT_FOUND);
        }
    }

    #[test]
    fn surface_render_matches_section_61() {
        let saved = Some(ModelChoice {
            model: "modelC".to_owned(),
            connection_id: None,
        });
        let resolution = Resolution {
            effective: Some(ModelChoice {
                model: "openhub_free_llm".to_owned(),
                connection_id: None,
            }),
            fallback_reason: Some(selection::FallbackReason::ModelRemoved),
        };
        assert_eq!(
            render_surface(saved, &resolution),
            json!({
                "saved": {"model": "modelC"},
                "effective": {"model": "openhub_free_llm"},
                "fallback_reason": "model_removed",
            })
        );
        let resolution = Resolution {
            effective: None,
            fallback_reason: None,
        };
        assert_eq!(
            render_surface(None, &resolution),
            json!({"saved": null, "effective": null, "fallback_reason": null})
        );
    }

    #[test]
    fn unknown_surface_rejects() {
        assert_eq!(Surface::parse("browser"), None);
        assert_eq!(Surface::parse("web_assistant"), Some(Surface::WebAssistant));
    }
}
