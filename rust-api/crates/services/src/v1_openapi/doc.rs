#![forbid(unsafe_code)]

//! D-23 OpenAPI 3.0.3 document assembly + YAML/JSON rendering (stage 5).
//!
//! Builds the served api-v1 document from static data: the golden route
//! table ([`super::routes`]) selects path × method, the per-operation
//! essentials below supply `operationId`/`tags`/`summary`/`parameters`/
//! `responses`, and the doc frame (`openapi` version, `info`, `servers`,
//! `tags`, `ApiKeyAuthentication` scheme) comes from [`super::meta`].
//! Both schema hooks from [`super::hooks`] are applied: the preprocess
//! filter over the route table and the project-id dual-form postprocessor
//! over the built document.
//!
//! Scope is the pinned subset from `FX-OPENAPI-04`: per-operation
//! `description`/`security`/`requestBody`/`externalDocs`, which
//! drf-spectacular emits conditionally, are neither captured nor pinned and
//! are omitted (minimal disposition: route coverage, not byte-identity).
//! `parameters: null` in the fixture means Django omitted the key
//! (`openapi.py:93-94` `if parameters:`), so the builder omits it too.
//!
//! Renderers: [`render_json`] (4-space pretty, like DRF's `JSONRenderer`
//! with `OpenApiJsonRenderer.get_indent or 4`) and [`render_yaml`] (minimal
//! block-style emitter; every string double-quoted with JSON escaping so
//! PyYAML's implicit resolution cannot mistype values — see
//! `yaml_quotes_every_string`). No utoipa dependency: utoipa 5/6 serialize
//! only `openapi` 3.1/3.2 while the contract pins exactly `3.0.3`, and
//! utoipa 4's `Parameter` has no `examples` field while the contract pins
//! per-parameter examples.
//!
//! Ported from `01a93e17216faea7bfc156b0f864cbbe420d1c52`.

use serde::Serialize;
use serde_json::{Map, Value};

use super::{hooks, meta, routes};

/// Assemble the full OpenAPI 3.0.3 document as a JSON value.
///
/// Paths/methods come from [`routes::ROUTES`] filtered through
/// [`hooks::endpoint_kept`]; per-operation bodies come from the embedded
/// [`OPERATIONS_JSON`] essentials; the frame comes from [`meta`]; then
/// [`hooks::postprocess_project_id_dual_form`] is applied.
pub fn build_document() -> Value {
    let essentials = essentials();
    let essentials = essentials.as_object().expect("essentials is a map");
    let mut paths = Map::new();
    for (path, methods) in routes::ROUTES {
        let mut item = Map::new();
        for method in *methods {
            if !hooks::endpoint_kept(path, method) {
                continue;
            }
            let mut op = essentials
                .get(*path)
                .and_then(|ops| ops.get(*method))
                .cloned()
                .expect("route table matches essentials");
            if op.get("parameters").is_some_and(Value::is_null) {
                op.as_object_mut()
                    .expect("operation is a map")
                    .remove("parameters");
            }
            item.insert((*method).to_string(), op);
        }
        paths.insert((*path).to_string(), Value::Object(item));
    }
    let servers: Vec<Value> = meta::SERVERS
        .iter()
        .map(|server| serde_json::json!({"url": server.url, "description": server.description}))
        .collect();
    let tags: Vec<Value> = meta::TAGS
        .iter()
        .map(|tag| serde_json::json!({"name": tag.name, "description": tag.description}))
        .collect();
    let mut security_schemes = Map::new();
    security_schemes.insert(
        meta::API_KEY_AUTH_NAME.to_string(),
        meta::api_key_security_definition(),
    );
    let mut doc = serde_json::json!({
        "openapi": meta::OPENAPI_VERSION,
        "info": {
            "title": meta::TITLE,
            "version": meta::VERSION,
            "description": meta::DESCRIPTION,
            "contact": {
                "name": meta::CONTACT.name,
                "url": meta::CONTACT.url,
                "email": meta::CONTACT.email,
            },
            "license": {
                "name": meta::LICENSE.name,
                "url": meta::LICENSE.url,
            },
        },
        "servers": servers,
        "tags": tags,
        "paths": Value::Object(paths),
        "components": {"securitySchemes": Value::Object(security_schemes)},
    });
    hooks::postprocess_project_id_dual_form(&mut doc);
    doc
}

/// Render the document as JSON (the `?format=json` body), 4-space pretty.
pub fn render_json() -> String {
    let doc = build_document();
    let mut buf = Vec::new();
    let mut ser = serde_json::ser::Serializer::with_formatter(
        &mut buf,
        serde_json::ser::PrettyFormatter::with_indent(b"    "),
    );
    doc.serialize(&mut ser)
        .expect("JSON serialization cannot fail");
    String::from_utf8(buf).expect("JSON is UTF-8")
}

/// Render the document as YAML (the default `/api/schema/` body).
///
/// Minimal block-style emitter: mappings and sequences in block form, empty
/// collections inline (`[]`/`{}`), numbers/bools/null as literals, and
/// every string double-quoted with JSON escaping (a subset of YAML
/// double-quote escapes, so any YAML 1.1/1.2 parser reads values back
/// exactly — verified against PyYAML, see the workpad).
pub fn render_yaml() -> String {
    let doc = build_document();
    let map = doc.as_object().expect("document is a mapping");
    let mut out = String::with_capacity(512 * 1024);
    for (key, value) in map {
        emit_string(key, &mut out);
        out.push(':');
        emit_child(value, 2, &mut out);
        out.push('\n');
    }
    out
}

/// Emit a block child after its `-`/`key:` prefix: scalars (and empty
/// collections) on the current line, non-empty collections on following
/// lines at `child_indent`.
fn emit_child(value: &Value, child_indent: usize, out: &mut String) {
    match value {
        Value::Array(items) if !items.is_empty() => {
            for item in items {
                out.push('\n');
                push_indent(child_indent, out);
                out.push('-');
                emit_child(item, child_indent + 2, out);
            }
        }
        Value::Object(map) if !map.is_empty() => {
            for (key, val) in map {
                out.push('\n');
                push_indent(child_indent, out);
                emit_string(key, out);
                out.push(':');
                emit_child(val, child_indent + 2, out);
            }
        }
        _ => {
            out.push(' ');
            emit_inline(value, out);
        }
    }
}

/// Emit a scalar or empty collection inline.
fn emit_inline(value: &Value, out: &mut String) {
    match value {
        Value::Null => out.push_str("null"),
        Value::Bool(true) => out.push_str("true"),
        Value::Bool(false) => out.push_str("false"),
        Value::Number(number) => out.push_str(&number.to_string()),
        Value::String(text) => emit_string(text, out),
        Value::Array(items) => {
            assert!(items.is_empty(), "non-empty array must use block form");
            out.push_str("[]");
        }
        Value::Object(map) => {
            assert!(map.is_empty(), "non-empty object must use block form");
            out.push_str("{}");
        }
    }
}

/// Emit a string double-quoted with JSON escaping.
fn emit_string(text: &str, out: &mut String) {
    out.push_str(&serde_json::to_string(text).expect("string serializes"));
}

fn push_indent(indent: usize, out: &mut String) {
    for _ in 0..indent {
        out.push(' ');
    }
}

/// Per-operation essentials: `{path: {method: {operationId, tags, summary,
/// parameters, responses}}}` for all 189 golden operations, pretty JSON
/// extracted from the `operations` key of
/// `rust-api/fixtures/v1_openapi/FX-OPENAPI-04.doc_essentials.json`.
///
/// Provenance + regen: when a re-sync re-records `FX-OPENAPI-04`,
/// re-extract with `python3 -c "import json;
/// fx = json.load(open('rust-api/fixtures/v1_openapi/FX-OPENAPI-04.doc_essentials.json'));
/// print(json.dumps(fx['operations'], indent=2, ensure_ascii=False))"`,
/// replace this literal, and review the diff (never blanket-update — the
/// tests pin this const to the fixture parsed-equal).
const OPERATIONS_JSON: &str = r##"
{
  "/api/v1/assets/user-assets/": {
    "post": {
      "operationId": "create_user_asset_upload",
      "tags": [
        "Assets"
      ],
      "summary": "Generate presigned URL for user asset upload",
      "parameters": null,
      "responses": {
        "401": {
          "description": "Authentication credentials were not provided or are invalid."
        },
        "403": {
          "description": "Permission denied. User lacks required permissions."
        },
        "200": {
          "description": "Presigned URL generated successfully"
        },
        "400": {
          "description": "Validation error occurred with the provided data."
        }
      }
    }
  },
  "/api/v1/assets/user-assets/{asset_id}/": {
    "delete": {
      "operationId": "delete_user_asset",
      "tags": [
        "Assets"
      ],
      "summary": "Delete user asset",
      "parameters": [
        {
          "in": "path",
          "name": "asset_id",
          "schema": {
            "type": "string",
            "format": "uuid"
          },
          "description": "Asset ID",
          "required": true,
          "examples": {
            "ExampleAssetID": {
              "value": "550e8400-e29b-41d4-a716-446655440000",
              "summary": "Example asset ID",
              "description": "A typical asset UUID"
            }
          }
        }
      ],
      "responses": {
        "401": {
          "description": "Authentication credentials were not provided or are invalid."
        },
        "403": {
          "description": "Permission denied. User lacks required permissions."
        },
        "204": {
          "description": "Asset deleted successfully"
        },
        "404": {
          "description": "The requested resource was not found."
        }
      }
    },
    "patch": {
      "operationId": "update_user_asset",
      "tags": [
        "Assets"
      ],
      "summary": "Mark user asset as uploaded",
      "parameters": [
        {
          "in": "path",
          "name": "asset_id",
          "schema": {
            "type": "string",
            "format": "uuid"
          },
          "description": "Asset ID",
          "required": true,
          "examples": {
            "ExampleAssetID": {
              "value": "550e8400-e29b-41d4-a716-446655440000",
              "summary": "Example asset ID",
              "description": "A typical asset UUID"
            }
          }
        }
      ],
      "responses": {
        "401": {
          "description": "Authentication credentials were not provided or are invalid."
        },
        "403": {
          "description": "Permission denied. User lacks required permissions."
        },
        "204": {
          "description": "Asset updated successfully"
        },
        "404": {
          "description": "The requested resource was not found."
        }
      }
    }
  },
  "/api/v1/auth/device/approve/": {
    "post": {
      "operationId": "auth_device_approve_create",
      "tags": [
        "auth"
      ],
      "summary": null,
      "parameters": null,
      "responses": {
        "200": {
          "description": "No response body"
        }
      }
    }
  },
  "/api/v1/auth/device/start/": {
    "post": {
      "operationId": "auth_device_start_create",
      "tags": [
        "auth"
      ],
      "summary": null,
      "parameters": null,
      "responses": {
        "200": {
          "description": "No response body"
        }
      }
    }
  },
  "/api/v1/auth/device/token/": {
    "post": {
      "operationId": "auth_device_token_create",
      "tags": [
        "auth"
      ],
      "summary": null,
      "parameters": null,
      "responses": {
        "200": {
          "description": "No response body"
        }
      }
    }
  },
  "/api/v1/auth/machine-token/": {
    "post": {
      "operationId": "auth_machine_token_create",
      "tags": [
        "auth"
      ],
      "summary": null,
      "parameters": null,
      "responses": {
        "200": {
          "description": "No response body"
        }
      }
    }
  },
  "/api/v1/auth/revoke/": {
    "post": {
      "operationId": "auth_revoke_create",
      "tags": [
        "auth"
      ],
      "summary": null,
      "parameters": null,
      "responses": {
        "200": {
          "description": "No response body"
        }
      }
    }
  },
  "/api/v1/auth/workspaces/": {
    "get": {
      "operationId": "auth_workspaces_retrieve",
      "tags": [
        "auth"
      ],
      "summary": null,
      "parameters": null,
      "responses": {
        "200": {
          "description": "No response body"
        }
      }
    }
  },
  "/api/v1/runner/chat/sessions/{session_id}/approvals/": {
    "post": {
      "operationId": "runner_chat_sessions_approvals_create",
      "tags": [
        "runner"
      ],
      "summary": null,
      "parameters": [
        {
          "in": "path",
          "name": "session_id",
          "schema": {
            "type": "string",
            "format": "uuid"
          },
          "required": true
        }
      ],
      "responses": {
        "200": {
          "description": "No response body"
        }
      }
    }
  },
  "/api/v1/runner/chat/sessions/{session_id}/closed/": {
    "post": {
      "operationId": "runner_chat_sessions_closed_create",
      "tags": [
        "runner"
      ],
      "summary": null,
      "parameters": [
        {
          "in": "path",
          "name": "session_id",
          "schema": {
            "type": "string",
            "format": "uuid"
          },
          "required": true
        }
      ],
      "responses": {
        "200": {
          "description": "No response body"
        }
      }
    }
  },
  "/api/v1/runner/chat/sessions/{session_id}/events/": {
    "post": {
      "operationId": "runner_chat_sessions_events_create",
      "tags": [
        "runner"
      ],
      "summary": null,
      "parameters": [
        {
          "in": "path",
          "name": "session_id",
          "schema": {
            "type": "string",
            "format": "uuid"
          },
          "required": true
        }
      ],
      "responses": {
        "200": {
          "description": "No response body"
        }
      }
    }
  },
  "/api/v1/runner/chat/sessions/{session_id}/failed/": {
    "post": {
      "operationId": "runner_chat_sessions_failed_create",
      "tags": [
        "runner"
      ],
      "summary": null,
      "parameters": [
        {
          "in": "path",
          "name": "session_id",
          "schema": {
            "type": "string",
            "format": "uuid"
          },
          "required": true
        }
      ],
      "responses": {
        "200": {
          "description": "No response body"
        }
      }
    }
  },
  "/api/v1/runner/chat/sessions/{session_id}/messages/{message_id}/complete/": {
    "post": {
      "operationId": "runner_chat_sessions_messages_complete_create",
      "tags": [
        "runner"
      ],
      "summary": null,
      "parameters": [
        {
          "in": "path",
          "name": "message_id",
          "schema": {
            "type": "string",
            "format": "uuid"
          },
          "required": true
        },
        {
          "in": "path",
          "name": "session_id",
          "schema": {
            "type": "string",
            "format": "uuid"
          },
          "required": true
        }
      ],
      "responses": {
        "200": {
          "description": "No response body"
        }
      }
    }
  },
  "/api/v1/runner/chat/sessions/{session_id}/messages/{message_id}/started/": {
    "post": {
      "operationId": "runner_chat_sessions_messages_started_create",
      "tags": [
        "runner"
      ],
      "summary": null,
      "parameters": [
        {
          "in": "path",
          "name": "message_id",
          "schema": {
            "type": "string",
            "format": "uuid"
          },
          "required": true
        },
        {
          "in": "path",
          "name": "session_id",
          "schema": {
            "type": "string",
            "format": "uuid"
          },
          "required": true
        }
      ],
      "responses": {
        "200": {
          "description": "No response body"
        }
      }
    }
  },
  "/api/v1/runner/chat/sessions/{session_id}/started/": {
    "post": {
      "operationId": "runner_chat_sessions_started_create",
      "tags": [
        "runner"
      ],
      "summary": null,
      "parameters": [
        {
          "in": "path",
          "name": "session_id",
          "schema": {
            "type": "string",
            "format": "uuid"
          },
          "required": true
        }
      ],
      "responses": {
        "200": {
          "description": "No response body"
        }
      }
    }
  },
  "/api/v1/runner/dev-machines/desktop-enroll/": {
    "delete": {
      "operationId": "runner_dev_machines_desktop_enroll_destroy",
      "tags": [
        "runner"
      ],
      "summary": null,
      "parameters": null,
      "responses": {
        "204": {
          "description": "No response body"
        }
      }
    },
    "post": {
      "operationId": "runner_dev_machines_desktop_enroll_create",
      "tags": [
        "runner"
      ],
      "summary": null,
      "parameters": null,
      "responses": {
        "200": {
          "description": "No response body"
        }
      }
    }
  },
  "/api/v1/runner/dev-machines/{dev_machine_id}/commands/{request_id}/result/": {
    "post": {
      "operationId": "runner_dev_machines_commands_result_create",
      "tags": [
        "runner"
      ],
      "summary": null,
      "parameters": [
        {
          "in": "path",
          "name": "dev_machine_id",
          "schema": {
            "type": "string",
            "format": "uuid"
          },
          "required": true
        },
        {
          "in": "path",
          "name": "request_id",
          "schema": {
            "type": "string",
            "format": "uuid"
          },
          "required": true
        }
      ],
      "responses": {
        "200": {
          "description": "No response body"
        }
      }
    }
  },
  "/api/v1/runner/dev-machines/{dev_machine_id}/sessions/": {
    "post": {
      "operationId": "runner_dev_machines_sessions_create",
      "tags": [
        "runner"
      ],
      "summary": null,
      "parameters": [
        {
          "in": "path",
          "name": "dev_machine_id",
          "schema": {
            "type": "string",
            "format": "uuid"
          },
          "required": true
        }
      ],
      "responses": {
        "200": {
          "description": "No response body"
        }
      }
    }
  },
  "/api/v1/runner/dev-machines/{dev_machine_id}/sessions/{sid}/": {
    "delete": {
      "operationId": "runner_dev_machines_sessions_destroy",
      "tags": [
        "runner"
      ],
      "summary": null,
      "parameters": [
        {
          "in": "path",
          "name": "dev_machine_id",
          "schema": {
            "type": "string",
            "format": "uuid"
          },
          "required": true
        },
        {
          "in": "path",
          "name": "sid",
          "schema": {
            "type": "string",
            "format": "uuid"
          },
          "required": true
        }
      ],
      "responses": {
        "204": {
          "description": "No response body"
        }
      }
    }
  },
  "/api/v1/runner/health/": {
    "get": {
      "operationId": "runner_health_retrieve",
      "tags": [
        "runner"
      ],
      "summary": null,
      "parameters": null,
      "responses": {
        "200": {
          "description": "No response body"
        }
      }
    }
  },
  "/api/v1/runner/machine-tokens/": {
    "post": {
      "operationId": "runner_machine_tokens_create",
      "tags": [
        "runner"
      ],
      "summary": null,
      "parameters": null,
      "responses": {
        "200": {
          "description": "No response body"
        }
      }
    }
  },
  "/api/v1/runner/metrics/": {
    "get": {
      "operationId": "runner_metrics_retrieve",
      "tags": [
        "runner"
      ],
      "summary": null,
      "parameters": null,
      "responses": {
        "200": {
          "description": "No response body"
        }
      }
    }
  },
  "/api/v1/runner/projects/": {
    "get": {
      "operationId": "runner_projects_retrieve",
      "tags": [
        "runner"
      ],
      "summary": null,
      "parameters": null,
      "responses": {
        "200": {
          "description": "No response body"
        }
      }
    }
  },
  "/api/v1/runner/runners/": {
    "post": {
      "operationId": "runner_runners_create",
      "tags": [
        "runner"
      ],
      "summary": null,
      "parameters": null,
      "responses": {
        "200": {
          "description": "No response body"
        }
      }
    }
  },
  "/api/v1/runner/runners/enroll/": {
    "post": {
      "operationId": "runner_runners_enroll_create",
      "tags": [
        "runner"
      ],
      "summary": null,
      "parameters": null,
      "responses": {
        "200": {
          "description": "No response body"
        }
      }
    }
  },
  "/api/v1/runner/runners/{runner_id}/": {
    "delete": {
      "operationId": "runner_runners_destroy",
      "tags": [
        "runner"
      ],
      "summary": null,
      "parameters": [
        {
          "in": "path",
          "name": "runner_id",
          "schema": {
            "type": "string",
            "format": "uuid"
          },
          "required": true
        }
      ],
      "responses": {
        "204": {
          "description": "No response body"
        }
      }
    }
  },
  "/api/v1/runner/runners/{runner_id}/refresh/": {
    "post": {
      "operationId": "runner_runners_refresh_create",
      "tags": [
        "runner"
      ],
      "summary": null,
      "parameters": [
        {
          "in": "path",
          "name": "runner_id",
          "schema": {
            "type": "string",
            "format": "uuid"
          },
          "required": true
        }
      ],
      "responses": {
        "200": {
          "description": "No response body"
        }
      }
    }
  },
  "/api/v1/runner/runners/{runner_id}/sessions/": {
    "post": {
      "operationId": "runner_runners_sessions_create",
      "tags": [
        "runner"
      ],
      "summary": null,
      "parameters": [
        {
          "in": "path",
          "name": "runner_id",
          "schema": {
            "type": "string",
            "format": "uuid"
          },
          "required": true
        }
      ],
      "responses": {
        "200": {
          "description": "No response body"
        }
      }
    }
  },
  "/api/v1/runner/runners/{runner_id}/sessions/{sid}/": {
    "delete": {
      "operationId": "runner_runners_sessions_destroy",
      "tags": [
        "runner"
      ],
      "summary": null,
      "parameters": [
        {
          "in": "path",
          "name": "runner_id",
          "schema": {
            "type": "string",
            "format": "uuid"
          },
          "required": true
        },
        {
          "in": "path",
          "name": "sid",
          "schema": {
            "type": "string",
            "format": "uuid"
          },
          "required": true
        }
      ],
      "responses": {
        "204": {
          "description": "No response body"
        }
      }
    }
  },
  "/api/v1/runner/runs/{run_id}/accept/": {
    "post": {
      "operationId": "runner_runs_accept_create",
      "tags": [
        "runner"
      ],
      "summary": null,
      "parameters": [
        {
          "in": "path",
          "name": "run_id",
          "schema": {
            "type": "string",
            "format": "uuid"
          },
          "required": true
        }
      ],
      "responses": {
        "200": {
          "description": "No response body"
        }
      }
    }
  },
  "/api/v1/runner/runs/{run_id}/approvals/": {
    "post": {
      "operationId": "runner_runs_approvals_create",
      "tags": [
        "runner"
      ],
      "summary": null,
      "parameters": [
        {
          "in": "path",
          "name": "run_id",
          "schema": {
            "type": "string",
            "format": "uuid"
          },
          "required": true
        }
      ],
      "responses": {
        "200": {
          "description": "No response body"
        }
      }
    }
  },
  "/api/v1/runner/runs/{run_id}/awaiting-reauth/": {
    "post": {
      "operationId": "runner_runs_awaiting_reauth_create",
      "tags": [
        "runner"
      ],
      "summary": null,
      "parameters": [
        {
          "in": "path",
          "name": "run_id",
          "schema": {
            "type": "string",
            "format": "uuid"
          },
          "required": true
        }
      ],
      "responses": {
        "200": {
          "description": "No response body"
        }
      }
    }
  },
  "/api/v1/runner/runs/{run_id}/cancelled/": {
    "post": {
      "operationId": "runner_runs_cancelled_create",
      "tags": [
        "runner"
      ],
      "summary": null,
      "parameters": [
        {
          "in": "path",
          "name": "run_id",
          "schema": {
            "type": "string",
            "format": "uuid"
          },
          "required": true
        }
      ],
      "responses": {
        "200": {
          "description": "No response body"
        }
      }
    }
  },
  "/api/v1/runner/runs/{run_id}/complete/": {
    "post": {
      "operationId": "runner_runs_complete_create",
      "tags": [
        "runner"
      ],
      "summary": null,
      "parameters": [
        {
          "in": "path",
          "name": "run_id",
          "schema": {
            "type": "string",
            "format": "uuid"
          },
          "required": true
        }
      ],
      "responses": {
        "200": {
          "description": "No response body"
        }
      }
    }
  },
  "/api/v1/runner/runs/{run_id}/events/": {
    "post": {
      "operationId": "runner_runs_events_create",
      "tags": [
        "runner"
      ],
      "summary": null,
      "parameters": [
        {
          "in": "path",
          "name": "run_id",
          "schema": {
            "type": "string",
            "format": "uuid"
          },
          "required": true
        }
      ],
      "responses": {
        "200": {
          "description": "No response body"
        }
      }
    }
  },
  "/api/v1/runner/runs/{run_id}/fail/": {
    "post": {
      "operationId": "runner_runs_fail_create",
      "tags": [
        "runner"
      ],
      "summary": null,
      "parameters": [
        {
          "in": "path",
          "name": "run_id",
          "schema": {
            "type": "string",
            "format": "uuid"
          },
          "required": true
        }
      ],
      "responses": {
        "200": {
          "description": "No response body"
        }
      }
    }
  },
  "/api/v1/runner/runs/{run_id}/pause/": {
    "post": {
      "operationId": "runner_runs_pause_create",
      "tags": [
        "runner"
      ],
      "summary": null,
      "parameters": [
        {
          "in": "path",
          "name": "run_id",
          "schema": {
            "type": "string",
            "format": "uuid"
          },
          "required": true
        }
      ],
      "responses": {
        "200": {
          "description": "No response body"
        }
      }
    }
  },
  "/api/v1/runner/runs/{run_id}/queued/": {
    "post": {
      "operationId": "runner_runs_queued_create",
      "tags": [
        "runner"
      ],
      "summary": null,
      "parameters": [
        {
          "in": "path",
          "name": "run_id",
          "schema": {
            "type": "string",
            "format": "uuid"
          },
          "required": true
        }
      ],
      "responses": {
        "200": {
          "description": "No response body"
        }
      }
    }
  },
  "/api/v1/runner/runs/{run_id}/resumed/": {
    "post": {
      "operationId": "runner_runs_resumed_create",
      "tags": [
        "runner"
      ],
      "summary": null,
      "parameters": [
        {
          "in": "path",
          "name": "run_id",
          "schema": {
            "type": "string",
            "format": "uuid"
          },
          "required": true
        }
      ],
      "responses": {
        "200": {
          "description": "No response body"
        }
      }
    }
  },
  "/api/v1/runner/runs/{run_id}/started/": {
    "post": {
      "operationId": "runner_runs_started_create",
      "tags": [
        "runner"
      ],
      "summary": null,
      "parameters": [
        {
          "in": "path",
          "name": "run_id",
          "schema": {
            "type": "string",
            "format": "uuid"
          },
          "required": true
        }
      ],
      "responses": {
        "200": {
          "description": "No response body"
        }
      }
    }
  },
  "/api/v1/runner/runs/{run_id}/stream/upgrade/": {
    "post": {
      "operationId": "runner_runs_stream_upgrade_create",
      "tags": [
        "runner"
      ],
      "summary": null,
      "parameters": [
        {
          "in": "path",
          "name": "run_id",
          "schema": {
            "type": "string",
            "format": "uuid"
          },
          "required": true
        }
      ],
      "responses": {
        "200": {
          "description": "No response body"
        }
      }
    }
  },
  "/api/v1/runners/{runner_id}/": {
    "delete": {
      "operationId": "runners_destroy",
      "tags": [
        "runners"
      ],
      "summary": null,
      "parameters": [
        {
          "in": "path",
          "name": "runner_id",
          "schema": {
            "type": "string",
            "format": "uuid"
          },
          "required": true
        }
      ],
      "responses": {
        "204": {
          "description": "No response body"
        }
      }
    }
  },
  "/api/v1/users/me/": {
    "get": {
      "operationId": "get_current_user",
      "tags": [
        "Users"
      ],
      "summary": "Get current user",
      "parameters": null,
      "responses": {
        "401": {
          "description": "Authentication credentials were not provided or are invalid."
        },
        "200": {
          "content": {
            "application/json": {
              "schema": {
                "$ref": "#/components/schemas/UserLite"
              },
              "examples": {
                "User": {
                  "value": {
                    "id": "550e8400-e29b-41d4-a716-446655440000",
                    "first_name": "John",
                    "last_name": "Doe",
                    "email": "john.doe@example.com",
                    "avatar": "https://example.com/avatar.jpg",
                    "avatar_url": "https://example.com/avatar.jpg",
                    "display_name": "John Doe"
                  }
                }
              }
            }
          },
          "description": "Current user profile"
        }
      }
    }
  },
  "/api/v1/workspaces/{slug}/agent-runs/{run_id}/yield/": {
    "post": {
      "operationId": "workspaces_agent_runs_yield_create",
      "tags": [
        "workspaces"
      ],
      "summary": null,
      "parameters": [
        {
          "in": "path",
          "name": "run_id",
          "schema": {
            "type": "string",
            "format": "uuid"
          },
          "required": true
        },
        {
          "in": "path",
          "name": "slug",
          "schema": {
            "type": "string"
          },
          "required": true
        }
      ],
      "responses": {
        "200": {
          "description": "No response body"
        }
      }
    }
  },
  "/api/v1/workspaces/{slug}/assets/": {
    "post": {
      "operationId": "create_generic_asset_upload",
      "tags": [
        "Assets"
      ],
      "summary": "Generate presigned URL for generic asset upload",
      "parameters": [
        {
          "in": "path",
          "name": "slug",
          "schema": {
            "type": "string"
          },
          "description": "Workspace slug",
          "required": true,
          "examples": {
            "ExampleWorkspace": {
              "value": "my-workspace",
              "summary": "Example workspace",
              "description": "A typical workspace slug"
            }
          }
        }
      ],
      "responses": {
        "401": {
          "description": "Authentication credentials were not provided or are invalid."
        },
        "403": {
          "description": "Permission denied. User lacks required permissions."
        },
        "200": {
          "description": "Presigned URL generated successfully"
        },
        "400": {
          "description": "Validation error"
        },
        "404": {
          "description": "The requested resource was not found."
        },
        "409": {
          "description": "Asset with same external ID already exists"
        }
      }
    }
  },
  "/api/v1/workspaces/{slug}/assets/{asset_id}/": {
    "get": {
      "operationId": "get_generic_asset",
      "tags": [
        "Assets"
      ],
      "summary": "Get presigned URL for asset download",
      "parameters": [
        {
          "in": "path",
          "name": "asset_id",
          "schema": {
            "type": "string",
            "format": "uuid"
          },
          "required": true
        },
        {
          "in": "path",
          "name": "slug",
          "schema": {
            "type": "string"
          },
          "description": "Workspace slug",
          "required": true,
          "examples": {
            "ExampleWorkspace": {
              "value": "my-workspace",
              "summary": "Example workspace",
              "description": "A typical workspace slug"
            }
          }
        }
      ],
      "responses": {
        "401": {
          "description": "Authentication credentials were not provided or are invalid."
        },
        "403": {
          "description": "Permission denied. User lacks required permissions."
        },
        "200": {
          "description": "Presigned download URL generated successfully"
        },
        "400": {
          "description": "Bad request"
        },
        "404": {
          "description": "Asset not found"
        }
      }
    },
    "patch": {
      "operationId": "update_generic_asset",
      "tags": [
        "Assets"
      ],
      "summary": "Update generic asset after upload completion",
      "parameters": [
        {
          "in": "path",
          "name": "asset_id",
          "schema": {
            "type": "string",
            "format": "uuid"
          },
          "description": "Asset ID",
          "required": true,
          "examples": {
            "ExampleAssetID": {
              "value": "550e8400-e29b-41d4-a716-446655440000",
              "summary": "Example asset ID",
              "description": "A typical asset UUID"
            }
          }
        },
        {
          "in": "path",
          "name": "slug",
          "schema": {
            "type": "string"
          },
          "description": "Workspace slug",
          "required": true,
          "examples": {
            "ExampleWorkspace": {
              "value": "my-workspace",
              "summary": "Example workspace",
              "description": "A typical workspace slug"
            }
          }
        }
      ],
      "responses": {
        "401": {
          "description": "Authentication credentials were not provided or are invalid."
        },
        "403": {
          "description": "Permission denied. User lacks required permissions."
        },
        "204": {
          "description": "Asset updated successfully"
        },
        "404": {
          "description": "Asset not found"
        }
      }
    }
  },
  "/api/v1/workspaces/{slug}/invitations/": {
    "get": {
      "operationId": "workspaces_invitations_list",
      "tags": [
        "workspaces"
      ],
      "summary": "List workspace invites",
      "parameters": [
        {
          "in": "path",
          "name": "slug",
          "schema": {
            "type": "string"
          },
          "description": "Workspace slug",
          "required": true,
          "examples": {
            "ExampleWorkspace": {
              "value": "my-workspace",
              "summary": "Example workspace",
              "description": "A typical workspace slug"
            }
          }
        }
      ],
      "responses": {
        "200": {
          "content": {
            "application/json": {
              "schema": {
                "type": "array",
                "items": {
                  "$ref": "#/components/schemas/WorkspaceInvite"
                }
              }
            }
          },
          "description": "Workspace invites"
        }
      }
    },
    "post": {
      "operationId": "workspaces_invitations_create",
      "tags": [
        "workspaces"
      ],
      "summary": "Create workspace invite",
      "parameters": [
        {
          "in": "path",
          "name": "slug",
          "schema": {
            "type": "string"
          },
          "description": "Workspace slug",
          "required": true,
          "examples": {
            "ExampleWorkspace": {
              "value": "my-workspace",
              "summary": "Example workspace",
              "description": "A typical workspace slug"
            }
          }
        }
      ],
      "responses": {
        "201": {
          "content": {
            "application/json": {
              "schema": {
                "$ref": "#/components/schemas/WorkspaceInvite"
              }
            }
          },
          "description": "Workspace invite"
        }
      }
    }
  },
  "/api/v1/workspaces/{slug}/invitations/{pk}/": {
    "delete": {
      "operationId": "workspaces_invitations_destroy",
      "tags": [
        "workspaces"
      ],
      "summary": "Delete workspace invite",
      "parameters": [
        {
          "in": "path",
          "name": "pk",
          "schema": {
            "type": "string",
            "format": "uuid"
          },
          "description": "Workspace invite ID",
          "required": true
        },
        {
          "in": "path",
          "name": "slug",
          "schema": {
            "type": "string"
          },
          "description": "Workspace slug",
          "required": true,
          "examples": {
            "ExampleWorkspace": {
              "value": "my-workspace",
              "summary": "Example workspace",
              "description": "A typical workspace slug"
            }
          }
        }
      ],
      "responses": {
        "204": {
          "description": "Workspace invite deleted"
        }
      }
    },
    "get": {
      "operationId": "workspaces_invitations_retrieve",
      "tags": [
        "workspaces"
      ],
      "summary": "Get workspace invite",
      "parameters": [
        {
          "in": "path",
          "name": "pk",
          "schema": {
            "type": "string",
            "format": "uuid"
          },
          "description": "Workspace invite ID",
          "required": true
        },
        {
          "in": "path",
          "name": "slug",
          "schema": {
            "type": "string"
          },
          "description": "Workspace slug",
          "required": true,
          "examples": {
            "ExampleWorkspace": {
              "value": "my-workspace",
              "summary": "Example workspace",
              "description": "A typical workspace slug"
            }
          }
        }
      ],
      "responses": {
        "200": {
          "content": {
            "application/json": {
              "schema": {
                "$ref": "#/components/schemas/WorkspaceInvite"
              }
            }
          },
          "description": "Workspace invite"
        }
      }
    },
    "patch": {
      "operationId": "workspaces_invitations_partial_update",
      "tags": [
        "workspaces"
      ],
      "summary": "Update workspace invite",
      "parameters": [
        {
          "in": "path",
          "name": "pk",
          "schema": {
            "type": "string",
            "format": "uuid"
          },
          "description": "Workspace invite ID",
          "required": true
        },
        {
          "in": "path",
          "name": "slug",
          "schema": {
            "type": "string"
          },
          "description": "Workspace slug",
          "required": true,
          "examples": {
            "ExampleWorkspace": {
              "value": "my-workspace",
              "summary": "Example workspace",
              "description": "A typical workspace slug"
            }
          }
        }
      ],
      "responses": {
        "200": {
          "content": {
            "application/json": {
              "schema": {
                "$ref": "#/components/schemas/WorkspaceInvite"
              }
            }
          },
          "description": "Workspace invite"
        }
      }
    }
  },
  "/api/v1/workspaces/{slug}/issues/search/": {
    "get": {
      "operationId": "search_work_items",
      "tags": [
        "Work Items"
      ],
      "summary": null,
      "parameters": [
        {
          "in": "query",
          "name": "limit",
          "schema": {
            "type": "integer"
          },
          "description": "Maximum number of results to return",
          "examples": {
            "Default": {
              "value": 10
            },
            "MoreResults": {
              "value": 50,
              "summary": "More results"
            }
          }
        },
        {
          "in": "query",
          "name": "project_id",
          "schema": {
            "type": "string",
            "format": "uuid"
          },
          "description": "Project ID for filtering results within a specific project",
          "examples": {
            "ExampleProjectID": {
              "value": "550e8400-e29b-41d4-a716-446655440000",
              "summary": "Example project ID",
              "description": "Filter results for this project"
            }
          }
        },
        {
          "in": "query",
          "name": "search",
          "schema": {
            "type": "string"
          },
          "description": "Search query to filter results by name, description, or identifier",
          "required": true,
          "examples": {
            "NameSearch": {
              "value": "bug fix",
              "summary": "Name search",
              "description": "Search for items containing 'bug fix'"
            },
            "SequenceID": {
              "value": "123",
              "summary": "Sequence ID",
              "description": "Search by sequence ID number"
            }
          }
        },
        {
          "in": "path",
          "name": "slug",
          "schema": {
            "type": "string"
          },
          "description": "Workspace slug",
          "required": true,
          "examples": {
            "ExampleWorkspace": {
              "value": "my-workspace",
              "summary": "Example workspace",
              "description": "A typical workspace slug"
            }
          }
        },
        {
          "in": "query",
          "name": "workspace_search",
          "schema": {
            "type": "string"
          },
          "description": "Whether to search across entire workspace or within specific project",
          "examples": {
            "ProjectOnly": {
              "value": "false",
              "summary": "Project only",
              "description": "Search within specific project only"
            },
            "WorkspaceWide": {
              "value": "true",
              "summary": "Workspace wide",
              "description": "Search across entire workspace"
            }
          }
        }
      ],
      "responses": {
        "200": {
          "content": {
            "application/json": {
              "schema": {
                "$ref": "#/components/schemas/IssueSearch"
              },
              "examples": {
                "IssueSearchResults": {
                  "value": {
                    "issues": [
                      {
                        "id": "550e8400-e29b-41d4-a716-446655440000",
                        "name": "Fix authentication bug in user login",
                        "sequence_id": 123,
                        "project__identifier": "MAB",
                        "project_id": "550e8400-e29b-41d4-a716-446655440001",
                        "workspace__slug": "my-workspace"
                      },
                      {
                        "id": "550e8400-e29b-41d4-a716-446655440002",
                        "name": "Add authentication middleware",
                        "sequence_id": 124,
                        "project__identifier": "MAB",
                        "project_id": "550e8400-e29b-41d4-a716-446655440001",
                        "workspace__slug": "my-workspace"
                      }
                    ]
                  }
                }
              }
            }
          },
          "description": "Work item search results"
        },
        "400": {
          "description": "Bad request - invalid search parameters"
        },
        "401": {
          "description": "Authentication credentials were not provided or are invalid."
        },
        "403": {
          "description": "Permission denied. User lacks required permissions."
        },
        "404": {
          "description": "Workspace not found"
        }
      }
    }
  },
  "/api/v1/workspaces/{slug}/issues/{project_identifier}-{issue_identifier}/": {
    "get": {
      "operationId": "get_workspace_work_item",
      "tags": [
        "Work Items"
      ],
      "summary": "Retrieve work item by identifiers",
      "parameters": [
        {
          "in": "path",
          "name": "issue_identifier",
          "schema": {
            "type": "integer"
          },
          "description": "Issue sequence ID (numeric identifier within project)",
          "required": true,
          "examples": {
            "ExampleIssueIdentifier": {
              "value": 123,
              "summary": "Example issue identifier",
              "description": "A typical issue sequence ID"
            }
          }
        },
        {
          "in": "path",
          "name": "project_identifier",
          "schema": {
            "type": "string"
          },
          "description": "Project identifier (unique string within workspace)",
          "required": true,
          "examples": {
            "ExampleProjectIdentifier": {
              "value": "PROJ",
              "summary": "Example project identifier",
              "description": "A typical project identifier"
            }
          }
        },
        {
          "in": "path",
          "name": "slug",
          "schema": {
            "type": "string"
          },
          "description": "Workspace slug",
          "required": true,
          "examples": {
            "ExampleWorkspace": {
              "value": "my-workspace",
              "summary": "Example workspace",
              "description": "A typical workspace slug"
            }
          }
        }
      ],
      "responses": {
        "200": {
          "content": {
            "application/json": {
              "schema": {
                "$ref": "#/components/schemas/Issue"
              },
              "examples": {
                "Issue": {
                  "value": {
                    "id": "550e8400-e29b-41d4-a716-446655440000",
                    "name": "Implement user authentication",
                    "description": "Add OAuth 2.0 authentication flow",
                    "sequence_id": 1,
                    "priority": "high",
                    "assignees": [
                      "550e8400-e29b-41d4-a716-446655440001"
                    ],
                    "labels": [
                      "550e8400-e29b-41d4-a716-446655440002"
                    ],
                    "created_at": "2024-01-15T10:30:00Z",
                    "updated_at": "2024-01-15T10:30:00Z"
                  }
                }
              }
            }
          },
          "description": "Work item details"
        },
        "404": {
          "description": "Work item not found"
        }
      }
    }
  },
  "/api/v1/workspaces/{slug}/members/": {
    "get": {
      "operationId": "get_workspace_members",
      "tags": [
        "Members"
      ],
      "summary": "List workspace members",
      "parameters": [
        {
          "in": "path",
          "name": "slug",
          "schema": {
            "type": "string"
          },
          "description": "Workspace slug",
          "required": true,
          "examples": {
            "ExampleWorkspace": {
              "value": "my-workspace",
              "summary": "Example workspace",
              "description": "A typical workspace slug"
            }
          }
        }
      ],
      "responses": {
        "200": {
          "content": {
            "application/json": {
              "schema": {
                "type": "array",
                "items": {
                  "allOf": [
                    {
                      "$ref": "#/components/schemas/UserLite"
                    },
                    {
                      "type": "object",
                      "properties": {
                        "role": {
                          "type": "integer",
                          "description": "Member role in the workspace"
                        }
                      }
                    }
                  ]
                }
              },
              "examples": {
                "WorkspaceMembers": {
                  "value": [
                    {
                      "id": "550e8400-e29b-41d4-a716-446655440000",
                      "first_name": "John",
                      "last_name": "Doe",
                      "display_name": "John Doe",
                      "email": "john.doe@example.com",
                      "avatar": "https://example.com/avatar.jpg",
                      "role": 20
                    },
                    {
                      "id": "550e8400-e29b-41d4-a716-446655440001",
                      "first_name": "Jane",
                      "last_name": "Smith",
                      "display_name": "Jane Smith",
                      "email": "jane.smith@example.com",
                      "avatar": "https://example.com/avatar2.jpg",
                      "role": 15
                    }
                  ]
                }
              }
            }
          },
          "description": "List of workspace members with their roles"
        },
        "401": {
          "description": "Authentication credentials were not provided or are invalid."
        },
        "403": {
          "description": "Permission denied. User lacks required permissions."
        },
        "404": {
          "description": "Workspace not found"
        }
      }
    }
  },
  "/api/v1/workspaces/{slug}/projects/": {
    "get": {
      "operationId": "list_projects",
      "tags": [
        "Projects"
      ],
      "summary": "List or retrieve projects",
      "parameters": [
        {
          "in": "query",
          "name": "cursor",
          "schema": {
            "type": "string"
          },
          "description": "Pagination cursor for getting next set of results",
          "examples": {
            "NextPageCursor": {
              "value": "20:1:0",
              "summary": "Next page cursor",
              "description": "Cursor format: 'page_size:page_number:offset'"
            }
          }
        },
        {
          "in": "query",
          "name": "expand",
          "schema": {
            "type": "string"
          },
          "description": "Comma-separated list of related fields to expand in response",
          "examples": {
            "ExpandAssignees": {
              "value": "assignees",
              "summary": "Expand assignees",
              "description": "Include full assignee details"
            },
            "MultipleExpansions": {
              "value": "assignees,labels,state",
              "summary": "Multiple expansions",
              "description": "Include details for multiple relations"
            }
          }
        },
        {
          "in": "query",
          "name": "fields",
          "schema": {
            "type": "string"
          },
          "description": "Comma-separated list of fields to include in response",
          "examples": {
            "BasicFields": {
              "value": "id,name,description",
              "summary": "Basic fields",
              "description": "Include only basic fields"
            },
            "WithRelations": {
              "value": "id,name,assignees,state",
              "summary": "With relations",
              "description": "Include fields with relationships"
            }
          }
        },
        {
          "in": "query",
          "name": "order_by",
          "schema": {
            "type": "string"
          },
          "description": "Field to order results by. Prefix with '-' for descending order",
          "examples": {
            "CreatedDateDescending": {
              "value": "-created_at",
              "summary": "Created date descending",
              "description": "Most recent items first"
            },
            "PriorityAscending": {
              "value": "priority",
              "summary": "Priority ascending",
              "description": "Order by priority (urgent, high, medium, low, none)"
            },
            "StateGroup": {
              "value": "state__group",
              "summary": "State group",
              "description": "Order by state group (backlog, unstarted, started, review, test, completed, cancelled)"
            },
            "AssigneeName": {
              "value": "assignees__first_name",
              "summary": "Assignee name",
              "description": "Order by assignee first name"
            }
          }
        },
        {
          "in": "query",
          "name": "per_page",
          "schema": {
            "type": "integer"
          },
          "description": "Number of results per page (default: 20, max: 100)",
          "examples": {
            "Default": {
              "value": 20
            },
            "Maximum": {
              "value": 100
            }
          }
        },
        {
          "in": "path",
          "name": "slug",
          "schema": {
            "type": "string"
          },
          "description": "Workspace slug",
          "required": true,
          "examples": {
            "ExampleWorkspace": {
              "value": "my-workspace",
              "summary": "Example workspace",
              "description": "A typical workspace slug"
            }
          }
        }
      ],
      "responses": {
        "401": {
          "description": "Authentication credentials were not provided or are invalid."
        },
        "403": {
          "description": "Permission denied. User lacks required permissions."
        },
        "404": {
          "description": "Project not found"
        },
        "200": {
          "content": {
            "application/json": {
              "schema": {
                "$ref": "#/components/schemas/PaginatedProjectResponse"
              },
              "examples": {
                "PaginatedProjects": {
                  "value": {
                    "grouped_by": "state",
                    "sub_grouped_by": "priority",
                    "total_count": 150,
                    "next_cursor": "20:1:0",
                    "prev_cursor": "20:0:0",
                    "next_page_results": true,
                    "prev_page_results": false,
                    "count": 20,
                    "total_pages": 8,
                    "total_results": 150,
                    "extra_stats": null,
                    "results": [
                      {
                        "id": "550e8400-e29b-41d4-a716-446655440000",
                        "name": "Mobile App Backend",
                        "description": "Backend services for the mobile application",
                        "identifier": "MAB",
                        "network": 2
                      }
                    ]
                  },
                  "summary": "Paginated Projects"
                }
              }
            }
          },
          "description": "Paginated list of projects"
        }
      }
    },
    "post": {
      "operationId": "create_project",
      "tags": [
        "Projects"
      ],
      "summary": "Create project",
      "parameters": [
        {
          "in": "path",
          "name": "slug",
          "schema": {
            "type": "string"
          },
          "description": "Workspace slug",
          "required": true,
          "examples": {
            "ExampleWorkspace": {
              "value": "my-workspace",
              "summary": "Example workspace",
              "description": "A typical workspace slug"
            }
          }
        }
      ],
      "responses": {
        "401": {
          "description": "Authentication credentials were not provided or are invalid."
        },
        "403": {
          "description": "Permission denied. User lacks required permissions."
        },
        "404": {
          "description": "Workspace not found"
        },
        "201": {
          "content": {
            "application/json": {
              "schema": {
                "$ref": "#/components/schemas/Project"
              },
              "examples": {
                "Project": {
                  "value": {
                    "id": "550e8400-e29b-41d4-a716-446655440000",
                    "name": "Mobile App Development",
                    "description": "Development of the mobile application",
                    "identifier": "MAD",
                    "network": 2,
                    "project_lead": "550e8400-e29b-41d4-a716-446655440001",
                    "created_at": "2024-01-15T10:30:00Z",
                    "updated_at": "2024-01-15T10:30:00Z"
                  }
                }
              }
            }
          },
          "description": "Project created successfully"
        },
        "409": {
          "description": "Project name already taken"
        }
      }
    }
  },
  "/api/v1/workspaces/{slug}/projects/{pk}/": {
    "delete": {
      "operationId": "delete_project",
      "tags": [
        "Projects"
      ],
      "summary": "Delete project",
      "parameters": [
        {
          "in": "path",
          "name": "pk",
          "schema": {
            "type": "string"
          },
          "description": "Either the project's UUID or its workspace-scoped identifier (e.g. ``ENG``). Identifier matching is case-insensitive.",
          "required": true,
          "examples": {
            "uuid": {
              "value": "00000000-0000-0000-0000-000000000000",
              "summary": "UUID form"
            },
            "slug": {
              "value": "ENG",
              "summary": "Workspace-scoped identifier"
            }
          }
        },
        {
          "in": "path",
          "name": "slug",
          "schema": {
            "type": "string"
          },
          "description": "Workspace slug",
          "required": true,
          "examples": {
            "ExampleWorkspace": {
              "value": "my-workspace",
              "summary": "Example workspace",
              "description": "A typical workspace slug"
            }
          }
        }
      ],
      "responses": {
        "401": {
          "description": "Authentication credentials were not provided or are invalid."
        },
        "403": {
          "description": "Permission denied. User lacks required permissions."
        },
        "404": {
          "description": "The requested resource was not found."
        },
        "204": {
          "description": "Resource deleted successfully"
        }
      }
    },
    "get": {
      "operationId": "retrieve_project",
      "tags": [
        "Projects"
      ],
      "summary": "Retrieve project",
      "parameters": [
        {
          "in": "path",
          "name": "pk",
          "schema": {
            "type": "string"
          },
          "description": "Either the project's UUID or its workspace-scoped identifier (e.g. ``ENG``). Identifier matching is case-insensitive.",
          "required": true,
          "examples": {
            "uuid": {
              "value": "00000000-0000-0000-0000-000000000000",
              "summary": "UUID form"
            },
            "slug": {
              "value": "ENG",
              "summary": "Workspace-scoped identifier"
            }
          }
        },
        {
          "in": "path",
          "name": "slug",
          "schema": {
            "type": "string"
          },
          "description": "Workspace slug",
          "required": true,
          "examples": {
            "ExampleWorkspace": {
              "value": "my-workspace",
              "summary": "Example workspace",
              "description": "A typical workspace slug"
            }
          }
        }
      ],
      "responses": {
        "401": {
          "description": "Authentication credentials were not provided or are invalid."
        },
        "403": {
          "description": "Permission denied. User lacks required permissions."
        },
        "404": {
          "description": "Project not found"
        },
        "200": {
          "content": {
            "application/json": {
              "schema": {
                "$ref": "#/components/schemas/Project"
              },
              "examples": {
                "Project": {
                  "value": {
                    "id": "550e8400-e29b-41d4-a716-446655440000",
                    "name": "Mobile App Development",
                    "description": "Development of the mobile application",
                    "identifier": "MAD",
                    "network": 2,
                    "project_lead": "550e8400-e29b-41d4-a716-446655440001",
                    "created_at": "2024-01-15T10:30:00Z",
                    "updated_at": "2024-01-15T10:30:00Z"
                  }
                }
              }
            }
          },
          "description": "Project details"
        }
      }
    },
    "patch": {
      "operationId": "update_project",
      "tags": [
        "Projects"
      ],
      "summary": "Update project",
      "parameters": [
        {
          "in": "path",
          "name": "pk",
          "schema": {
            "type": "string"
          },
          "description": "Either the project's UUID or its workspace-scoped identifier (e.g. ``ENG``). Identifier matching is case-insensitive.",
          "required": true,
          "examples": {
            "uuid": {
              "value": "00000000-0000-0000-0000-000000000000",
              "summary": "UUID form"
            },
            "slug": {
              "value": "ENG",
              "summary": "Workspace-scoped identifier"
            }
          }
        },
        {
          "in": "path",
          "name": "slug",
          "schema": {
            "type": "string"
          },
          "description": "Workspace slug",
          "required": true,
          "examples": {
            "ExampleWorkspace": {
              "value": "my-workspace",
              "summary": "Example workspace",
              "description": "A typical workspace slug"
            }
          }
        }
      ],
      "responses": {
        "401": {
          "description": "Authentication credentials were not provided or are invalid."
        },
        "403": {
          "description": "Permission denied. User lacks required permissions."
        },
        "404": {
          "description": "Project not found"
        },
        "200": {
          "content": {
            "application/json": {
              "schema": {
                "$ref": "#/components/schemas/Project"
              },
              "examples": {
                "Project": {
                  "value": {
                    "id": "550e8400-e29b-41d4-a716-446655440000",
                    "name": "Mobile App Development",
                    "description": "Development of the mobile application",
                    "identifier": "MAD",
                    "network": 2,
                    "project_lead": "550e8400-e29b-41d4-a716-446655440001",
                    "created_at": "2024-01-15T10:30:00Z",
                    "updated_at": "2024-01-15T10:30:00Z"
                  }
                }
              }
            }
          },
          "description": "Project updated successfully"
        },
        "409": {
          "description": "Project name already taken"
        }
      }
    }
  },
  "/api/v1/workspaces/{slug}/projects/{project_id}/archive/": {
    "delete": {
      "operationId": "unarchive_project",
      "tags": [
        "Projects"
      ],
      "summary": "Unarchive project",
      "parameters": [
        {
          "in": "path",
          "name": "project_id",
          "schema": {
            "type": "string"
          },
          "description": "Either the project's UUID or its workspace-scoped identifier (e.g. ``ENG``). Identifier matching is case-insensitive.",
          "required": true,
          "examples": {
            "uuid": {
              "value": "00000000-0000-0000-0000-000000000000",
              "summary": "UUID form"
            },
            "slug": {
              "value": "ENG",
              "summary": "Workspace-scoped identifier"
            }
          }
        },
        {
          "in": "path",
          "name": "slug",
          "schema": {
            "type": "string"
          },
          "description": "Workspace slug",
          "required": true,
          "examples": {
            "ExampleWorkspace": {
              "value": "my-workspace",
              "summary": "Example workspace",
              "description": "A typical workspace slug"
            }
          }
        }
      ],
      "responses": {
        "401": {
          "description": "Authentication credentials were not provided or are invalid."
        },
        "403": {
          "description": "Permission denied. User lacks required permissions."
        },
        "404": {
          "description": "The requested resource was not found."
        },
        "204": {
          "description": "Resource unarchived successfully"
        }
      }
    },
    "post": {
      "operationId": "archive_project",
      "tags": [
        "Projects"
      ],
      "summary": "Archive project",
      "parameters": [
        {
          "in": "path",
          "name": "project_id",
          "schema": {
            "type": "string"
          },
          "description": "Either the project's UUID or its workspace-scoped identifier (e.g. ``ENG``). Identifier matching is case-insensitive.",
          "required": true,
          "examples": {
            "uuid": {
              "value": "00000000-0000-0000-0000-000000000000",
              "summary": "UUID form"
            },
            "slug": {
              "value": "ENG",
              "summary": "Workspace-scoped identifier"
            }
          }
        },
        {
          "in": "path",
          "name": "slug",
          "schema": {
            "type": "string"
          },
          "description": "Workspace slug",
          "required": true,
          "examples": {
            "ExampleWorkspace": {
              "value": "my-workspace",
              "summary": "Example workspace",
              "description": "A typical workspace slug"
            }
          }
        }
      ],
      "responses": {
        "401": {
          "description": "Authentication credentials were not provided or are invalid."
        },
        "403": {
          "description": "Permission denied. User lacks required permissions."
        },
        "404": {
          "description": "The requested resource was not found."
        },
        "204": {
          "description": "Resource archived successfully"
        }
      }
    }
  },
  "/api/v1/workspaces/{slug}/projects/{project_id}/archived-cycles/": {
    "get": {
      "operationId": "list_archived_cycles",
      "tags": [
        "Cycles"
      ],
      "summary": "List archived cycles",
      "parameters": [
        {
          "in": "query",
          "name": "cursor",
          "schema": {
            "type": "string"
          },
          "description": "Pagination cursor for getting next set of results",
          "examples": {
            "NextPageCursor": {
              "value": "20:1:0",
              "summary": "Next page cursor",
              "description": "Cursor format: 'page_size:page_number:offset'"
            }
          }
        },
        {
          "in": "query",
          "name": "per_page",
          "schema": {
            "type": "integer"
          },
          "description": "Number of results per page (default: 20, max: 100)",
          "examples": {
            "Default": {
              "value": 20
            },
            "Maximum": {
              "value": 100
            }
          }
        },
        {
          "in": "path",
          "name": "project_id",
          "schema": {
            "type": "string"
          },
          "description": "Either the project's UUID or its workspace-scoped identifier (e.g. ``ENG``). Identifier matching is case-insensitive.",
          "required": true,
          "examples": {
            "uuid": {
              "value": "00000000-0000-0000-0000-000000000000",
              "summary": "UUID form"
            },
            "slug": {
              "value": "ENG",
              "summary": "Workspace-scoped identifier"
            }
          }
        },
        {
          "in": "path",
          "name": "slug",
          "schema": {
            "type": "string"
          },
          "description": "Workspace slug",
          "required": true,
          "examples": {
            "ExampleWorkspace": {
              "value": "my-workspace",
              "summary": "Example workspace",
              "description": "A typical workspace slug"
            }
          }
        }
      ],
      "responses": {
        "401": {
          "description": "Authentication credentials were not provided or are invalid."
        },
        "403": {
          "description": "Permission denied. User lacks required permissions."
        },
        "404": {
          "description": "The requested resource was not found."
        },
        "200": {
          "content": {
            "application/json": {
              "schema": {
                "$ref": "#/components/schemas/PaginatedArchivedCycleResponse"
              },
              "examples": {
                "PaginatedArchivedCycles": {
                  "value": {
                    "grouped_by": "state",
                    "sub_grouped_by": "priority",
                    "total_count": 150,
                    "next_cursor": "20:1:0",
                    "prev_cursor": "20:0:0",
                    "next_page_results": true,
                    "prev_page_results": false,
                    "count": 20,
                    "total_pages": 8,
                    "total_results": 150,
                    "extra_stats": null,
                    "results": [
                      {
                        "id": "550e8400-e29b-41d4-a716-446655440000",
                        "name": "Sample Item",
                        "created_at": "2024-01-15T12:00:00Z"
                      }
                    ]
                  },
                  "summary": "Paginated Archived Cycles"
                }
              }
            }
          },
          "description": "Paginated list of archived cycles"
        }
      }
    }
  },
  "/api/v1/workspaces/{slug}/projects/{project_id}/archived-cycles/{cycle_id}/unarchive/": {
    "delete": {
      "operationId": "unarchive_cycle",
      "tags": [
        "Cycles"
      ],
      "summary": "Unarchive cycle",
      "parameters": [
        {
          "in": "path",
          "name": "cycle_id",
          "schema": {
            "type": "string",
            "format": "uuid"
          },
          "required": true
        },
        {
          "in": "path",
          "name": "project_id",
          "schema": {
            "type": "string"
          },
          "description": "Either the project's UUID or its workspace-scoped identifier (e.g. ``ENG``). Identifier matching is case-insensitive.",
          "required": true,
          "examples": {
            "uuid": {
              "value": "00000000-0000-0000-0000-000000000000",
              "summary": "UUID form"
            },
            "slug": {
              "value": "ENG",
              "summary": "Workspace-scoped identifier"
            }
          }
        },
        {
          "in": "path",
          "name": "slug",
          "schema": {
            "type": "string"
          },
          "description": "Workspace slug",
          "required": true,
          "examples": {
            "ExampleWorkspace": {
              "value": "my-workspace",
              "summary": "Example workspace",
              "description": "A typical workspace slug"
            }
          }
        }
      ],
      "responses": {
        "401": {
          "description": "Authentication credentials were not provided or are invalid."
        },
        "403": {
          "description": "Permission denied. User lacks required permissions."
        },
        "404": {
          "description": "The requested resource was not found."
        },
        "204": {
          "description": "Resource unarchived successfully"
        }
      }
    }
  },
  "/api/v1/workspaces/{slug}/projects/{project_id}/archived-modules/": {
    "get": {
      "operationId": "list_archived_modules",
      "tags": [
        "Modules"
      ],
      "summary": "List archived modules",
      "parameters": [
        {
          "in": "query",
          "name": "cursor",
          "schema": {
            "type": "string"
          },
          "description": "Pagination cursor for getting next set of results",
          "examples": {
            "NextPageCursor": {
              "value": "20:1:0",
              "summary": "Next page cursor",
              "description": "Cursor format: 'page_size:page_number:offset'"
            }
          }
        },
        {
          "in": "query",
          "name": "expand",
          "schema": {
            "type": "string"
          },
          "description": "Comma-separated list of related fields to expand in response",
          "examples": {
            "ExpandAssignees": {
              "value": "assignees",
              "summary": "Expand assignees",
              "description": "Include full assignee details"
            },
            "MultipleExpansions": {
              "value": "assignees,labels,state",
              "summary": "Multiple expansions",
              "description": "Include details for multiple relations"
            }
          }
        },
        {
          "in": "query",
          "name": "fields",
          "schema": {
            "type": "string"
          },
          "description": "Comma-separated list of fields to include in response",
          "examples": {
            "BasicFields": {
              "value": "id,name,description",
              "summary": "Basic fields",
              "description": "Include only basic fields"
            },
            "WithRelations": {
              "value": "id,name,assignees,state",
              "summary": "With relations",
              "description": "Include fields with relationships"
            }
          }
        },
        {
          "in": "query",
          "name": "order_by",
          "schema": {
            "type": "string"
          },
          "description": "Field to order results by. Prefix with '-' for descending order",
          "examples": {
            "CreatedDateDescending": {
              "value": "-created_at",
              "summary": "Created date descending",
              "description": "Most recent items first"
            },
            "PriorityAscending": {
              "value": "priority",
              "summary": "Priority ascending",
              "description": "Order by priority (urgent, high, medium, low, none)"
            },
            "StateGroup": {
              "value": "state__group",
              "summary": "State group",
              "description": "Order by state group (backlog, unstarted, started, review, test, completed, cancelled)"
            },
            "AssigneeName": {
              "value": "assignees__first_name",
              "summary": "Assignee name",
              "description": "Order by assignee first name"
            }
          }
        },
        {
          "in": "query",
          "name": "per_page",
          "schema": {
            "type": "integer"
          },
          "description": "Number of results per page (default: 20, max: 100)",
          "examples": {
            "Default": {
              "value": 20
            },
            "Maximum": {
              "value": 100
            }
          }
        },
        {
          "in": "path",
          "name": "project_id",
          "schema": {
            "type": "string"
          },
          "description": "Either the project's UUID or its workspace-scoped identifier (e.g. ``ENG``). Identifier matching is case-insensitive.",
          "required": true,
          "examples": {
            "uuid": {
              "value": "00000000-0000-0000-0000-000000000000",
              "summary": "UUID form"
            },
            "slug": {
              "value": "ENG",
              "summary": "Workspace-scoped identifier"
            }
          }
        },
        {
          "in": "path",
          "name": "slug",
          "schema": {
            "type": "string"
          },
          "description": "Workspace slug",
          "required": true,
          "examples": {
            "ExampleWorkspace": {
              "value": "my-workspace",
              "summary": "Example workspace",
              "description": "A typical workspace slug"
            }
          }
        }
      ],
      "responses": {
        "401": {
          "description": "Authentication credentials were not provided or are invalid."
        },
        "403": {
          "description": "Permission denied. User lacks required permissions."
        },
        "404": {
          "description": "Project not found"
        },
        "200": {
          "content": {
            "application/json": {
              "schema": {
                "$ref": "#/components/schemas/PaginatedArchivedModuleResponse"
              },
              "examples": {
                "PaginatedArchivedModules": {
                  "value": {
                    "grouped_by": "state",
                    "sub_grouped_by": "priority",
                    "total_count": 150,
                    "next_cursor": "20:1:0",
                    "prev_cursor": "20:0:0",
                    "next_page_results": true,
                    "prev_page_results": false,
                    "count": 20,
                    "total_pages": 8,
                    "total_results": 150,
                    "extra_stats": null,
                    "results": [
                      {
                        "id": "550e8400-e29b-41d4-a716-446655440000",
                        "name": "Sample Item",
                        "created_at": "2024-01-15T12:00:00Z"
                      }
                    ]
                  },
                  "summary": "Paginated Archived Modules"
                }
              }
            }
          },
          "description": "Paginated list of archived modules"
        }
      }
    }
  },
  "/api/v1/workspaces/{slug}/projects/{project_id}/archived-modules/{pk}/unarchive/": {
    "delete": {
      "operationId": "unarchive_module",
      "tags": [
        "Modules"
      ],
      "summary": "Unarchive module",
      "parameters": [
        {
          "in": "path",
          "name": "pk",
          "schema": {
            "type": "string",
            "format": "uuid"
          },
          "description": "Module ID",
          "required": true,
          "examples": {
            "ExampleModuleID": {
              "value": "550e8400-e29b-41d4-a716-446655440000",
              "summary": "Example module ID",
              "description": "A typical module UUID"
            }
          }
        },
        {
          "in": "path",
          "name": "project_id",
          "schema": {
            "type": "string"
          },
          "description": "Either the project's UUID or its workspace-scoped identifier (e.g. ``ENG``). Identifier matching is case-insensitive.",
          "required": true,
          "examples": {
            "uuid": {
              "value": "00000000-0000-0000-0000-000000000000",
              "summary": "UUID form"
            },
            "slug": {
              "value": "ENG",
              "summary": "Workspace-scoped identifier"
            }
          }
        },
        {
          "in": "path",
          "name": "slug",
          "schema": {
            "type": "string"
          },
          "description": "Workspace slug",
          "required": true,
          "examples": {
            "ExampleWorkspace": {
              "value": "my-workspace",
              "summary": "Example workspace",
              "description": "A typical workspace slug"
            }
          }
        }
      ],
      "responses": {
        "401": {
          "description": "Authentication credentials were not provided or are invalid."
        },
        "403": {
          "description": "Permission denied. User lacks required permissions."
        },
        "404": {
          "description": "Module not found"
        },
        "204": {
          "description": "No response body"
        }
      }
    }
  },
  "/api/v1/workspaces/{slug}/projects/{project_id}/cycles/": {
    "get": {
      "operationId": "list_cycles",
      "tags": [
        "Cycles"
      ],
      "summary": "List cycles",
      "parameters": [
        {
          "in": "query",
          "name": "cursor",
          "schema": {
            "type": "string"
          },
          "description": "Pagination cursor for getting next set of results",
          "examples": {
            "NextPageCursor": {
              "value": "20:1:0",
              "summary": "Next page cursor",
              "description": "Cursor format: 'page_size:page_number:offset'"
            }
          }
        },
        {
          "in": "query",
          "name": "cycle_view",
          "schema": {
            "type": "string"
          },
          "description": "Filter cycles by status",
          "examples": {
            "AllCycles": {
              "value": "all",
              "summary": "All cycles"
            },
            "CurrentCycles": {
              "value": "current",
              "summary": "Current cycles"
            },
            "UpcomingCycles": {
              "value": "upcoming",
              "summary": "Upcoming cycles"
            },
            "CompletedCycles": {
              "value": "completed",
              "summary": "Completed cycles"
            },
            "DraftCycles": {
              "value": "draft",
              "summary": "Draft cycles"
            },
            "IncompleteCycles": {
              "value": "incomplete",
              "summary": "Incomplete cycles"
            }
          }
        },
        {
          "in": "query",
          "name": "expand",
          "schema": {
            "type": "string"
          },
          "description": "Comma-separated list of related fields to expand in response",
          "examples": {
            "ExpandAssignees": {
              "value": "assignees",
              "summary": "Expand assignees",
              "description": "Include full assignee details"
            },
            "MultipleExpansions": {
              "value": "assignees,labels,state",
              "summary": "Multiple expansions",
              "description": "Include details for multiple relations"
            }
          }
        },
        {
          "in": "query",
          "name": "fields",
          "schema": {
            "type": "string"
          },
          "description": "Comma-separated list of fields to include in response",
          "examples": {
            "BasicFields": {
              "value": "id,name,description",
              "summary": "Basic fields",
              "description": "Include only basic fields"
            },
            "WithRelations": {
              "value": "id,name,assignees,state",
              "summary": "With relations",
              "description": "Include fields with relationships"
            }
          }
        },
        {
          "in": "query",
          "name": "order_by",
          "schema": {
            "type": "string"
          },
          "description": "Field to order results by. Prefix with '-' for descending order",
          "examples": {
            "CreatedDateDescending": {
              "value": "-created_at",
              "summary": "Created date descending",
              "description": "Most recent items first"
            },
            "PriorityAscending": {
              "value": "priority",
              "summary": "Priority ascending",
              "description": "Order by priority (urgent, high, medium, low, none)"
            },
            "StateGroup": {
              "value": "state__group",
              "summary": "State group",
              "description": "Order by state group (backlog, unstarted, started, review, test, completed, cancelled)"
            },
            "AssigneeName": {
              "value": "assignees__first_name",
              "summary": "Assignee name",
              "description": "Order by assignee first name"
            }
          }
        },
        {
          "in": "query",
          "name": "per_page",
          "schema": {
            "type": "integer"
          },
          "description": "Number of results per page (default: 20, max: 100)",
          "examples": {
            "Default": {
              "value": 20
            },
            "Maximum": {
              "value": 100
            }
          }
        },
        {
          "in": "path",
          "name": "project_id",
          "schema": {
            "type": "string"
          },
          "description": "Either the project's UUID or its workspace-scoped identifier (e.g. ``ENG``). Identifier matching is case-insensitive.",
          "required": true,
          "examples": {
            "uuid": {
              "value": "00000000-0000-0000-0000-000000000000",
              "summary": "UUID form"
            },
            "slug": {
              "value": "ENG",
              "summary": "Workspace-scoped identifier"
            }
          }
        },
        {
          "in": "path",
          "name": "slug",
          "schema": {
            "type": "string"
          },
          "description": "Workspace slug",
          "required": true,
          "examples": {
            "ExampleWorkspace": {
              "value": "my-workspace",
              "summary": "Example workspace",
              "description": "A typical workspace slug"
            }
          }
        }
      ],
      "responses": {
        "401": {
          "description": "Authentication credentials were not provided or are invalid."
        },
        "403": {
          "description": "Permission denied. User lacks required permissions."
        },
        "404": {
          "description": "The requested resource was not found."
        },
        "200": {
          "content": {
            "application/json": {
              "schema": {
                "$ref": "#/components/schemas/PaginatedCycleResponse"
              },
              "examples": {
                "PaginatedCycles": {
                  "value": {
                    "grouped_by": "state",
                    "sub_grouped_by": "priority",
                    "total_count": 150,
                    "next_cursor": "20:1:0",
                    "prev_cursor": "20:0:0",
                    "next_page_results": true,
                    "prev_page_results": false,
                    "count": 20,
                    "total_pages": 8,
                    "total_results": 150,
                    "extra_stats": null,
                    "results": [
                      {
                        "id": "550e8400-e29b-41d4-a716-446655440000",
                        "name": "Sprint 1 - Q1 2024",
                        "description": "First sprint of the quarter focusing on core features",
                        "start_date": "2024-01-01",
                        "end_date": "2024-01-14",
                        "status": "current"
                      }
                    ]
                  },
                  "summary": "Paginated Cycles"
                }
              }
            }
          },
          "description": "Paginated list of cycles"
        }
      }
    },
    "post": {
      "operationId": "create_cycle",
      "tags": [
        "Cycles"
      ],
      "summary": "Create cycle",
      "parameters": [
        {
          "in": "path",
          "name": "project_id",
          "schema": {
            "type": "string"
          },
          "description": "Either the project's UUID or its workspace-scoped identifier (e.g. ``ENG``). Identifier matching is case-insensitive.",
          "required": true,
          "examples": {
            "uuid": {
              "value": "00000000-0000-0000-0000-000000000000",
              "summary": "UUID form"
            },
            "slug": {
              "value": "ENG",
              "summary": "Workspace-scoped identifier"
            }
          }
        },
        {
          "in": "path",
          "name": "slug",
          "schema": {
            "type": "string"
          },
          "description": "Workspace slug",
          "required": true,
          "examples": {
            "ExampleWorkspace": {
              "value": "my-workspace",
              "summary": "Example workspace",
              "description": "A typical workspace slug"
            }
          }
        }
      ],
      "responses": {
        "401": {
          "description": "Authentication credentials were not provided or are invalid."
        },
        "403": {
          "description": "Permission denied. User lacks required permissions."
        },
        "404": {
          "description": "The requested resource was not found."
        },
        "201": {
          "content": {
            "application/json": {
              "schema": {
                "$ref": "#/components/schemas/Cycle"
              },
              "examples": {
                "Cycle": {
                  "value": {
                    "id": "550e8400-e29b-41d4-a716-446655440000",
                    "name": "Sprint 1 - Q1 2024",
                    "description": "First sprint of the quarter focusing on core features",
                    "start_date": "2024-01-01",
                    "end_date": "2024-01-14",
                    "status": "current",
                    "total_issues": 15,
                    "completed_issues": 8,
                    "cancelled_issues": 1,
                    "started_issues": 4,
                    "unstarted_issues": 2,
                    "backlog_issues": 0,
                    "created_at": "2024-01-01T10:30:00Z",
                    "updated_at": "2024-01-10T15:45:00Z"
                  }
                }
              }
            }
          },
          "description": "Cycle created"
        }
      }
    }
  },
  "/api/v1/workspaces/{slug}/projects/{project_id}/cycles/{cycle_id}/archive/": {
    "post": {
      "operationId": "archive_cycle",
      "tags": [
        "Cycles"
      ],
      "summary": "Archive cycle",
      "parameters": [
        {
          "in": "path",
          "name": "cycle_id",
          "schema": {
            "type": "string",
            "format": "uuid"
          },
          "required": true
        },
        {
          "in": "path",
          "name": "project_id",
          "schema": {
            "type": "string"
          },
          "description": "Either the project's UUID or its workspace-scoped identifier (e.g. ``ENG``). Identifier matching is case-insensitive.",
          "required": true,
          "examples": {
            "uuid": {
              "value": "00000000-0000-0000-0000-000000000000",
              "summary": "UUID form"
            },
            "slug": {
              "value": "ENG",
              "summary": "Workspace-scoped identifier"
            }
          }
        },
        {
          "in": "path",
          "name": "slug",
          "schema": {
            "type": "string"
          },
          "description": "Workspace slug",
          "required": true,
          "examples": {
            "ExampleWorkspace": {
              "value": "my-workspace",
              "summary": "Example workspace",
              "description": "A typical workspace slug"
            }
          }
        }
      ],
      "responses": {
        "401": {
          "description": "Authentication credentials were not provided or are invalid."
        },
        "403": {
          "description": "Permission denied. User lacks required permissions."
        },
        "404": {
          "description": "The requested resource was not found."
        },
        "204": {
          "description": "Resource archived successfully"
        },
        "400": {
          "description": "Cycle cannot be archived"
        }
      }
    }
  },
  "/api/v1/workspaces/{slug}/projects/{project_id}/cycles/{cycle_id}/cycle-issues/": {
    "get": {
      "operationId": "list_cycle_work_items",
      "tags": [
        "Cycles"
      ],
      "summary": "List cycle work items",
      "parameters": [
        {
          "in": "query",
          "name": "cursor",
          "schema": {
            "type": "string"
          },
          "description": "Pagination cursor for getting next set of results",
          "examples": {
            "NextPageCursor": {
              "value": "20:1:0",
              "summary": "Next page cursor",
              "description": "Cursor format: 'page_size:page_number:offset'"
            }
          }
        },
        {
          "in": "path",
          "name": "cycle_id",
          "schema": {
            "type": "string",
            "format": "uuid"
          },
          "required": true
        },
        {
          "in": "query",
          "name": "per_page",
          "schema": {
            "type": "integer"
          },
          "description": "Number of results per page (default: 20, max: 100)",
          "examples": {
            "Default": {
              "value": 20
            },
            "Maximum": {
              "value": 100
            }
          }
        },
        {
          "in": "path",
          "name": "project_id",
          "schema": {
            "type": "string"
          },
          "description": "Either the project's UUID or its workspace-scoped identifier (e.g. ``ENG``). Identifier matching is case-insensitive.",
          "required": true,
          "examples": {
            "uuid": {
              "value": "00000000-0000-0000-0000-000000000000",
              "summary": "UUID form"
            },
            "slug": {
              "value": "ENG",
              "summary": "Workspace-scoped identifier"
            }
          }
        },
        {
          "in": "path",
          "name": "slug",
          "schema": {
            "type": "string"
          },
          "description": "Workspace slug",
          "required": true,
          "examples": {
            "ExampleWorkspace": {
              "value": "my-workspace",
              "summary": "Example workspace",
              "description": "A typical workspace slug"
            }
          }
        }
      ],
      "responses": {
        "401": {
          "description": "Authentication credentials were not provided or are invalid."
        },
        "403": {
          "description": "Permission denied. User lacks required permissions."
        },
        "404": {
          "description": "The requested resource was not found."
        },
        "200": {
          "content": {
            "application/json": {
              "schema": {
                "$ref": "#/components/schemas/PaginatedCycleIssueResponse"
              },
              "examples": {
                "PaginatedCycleWorkItems": {
                  "value": {
                    "grouped_by": "state",
                    "sub_grouped_by": "priority",
                    "total_count": 150,
                    "next_cursor": "20:1:0",
                    "prev_cursor": "20:0:0",
                    "next_page_results": true,
                    "prev_page_results": false,
                    "count": 20,
                    "total_pages": 8,
                    "total_results": 150,
                    "extra_stats": null,
                    "results": [
                      {
                        "id": "550e8400-e29b-41d4-a716-446655440000",
                        "cycle": "550e8400-e29b-41d4-a716-446655440001",
                        "issue": "550e8400-e29b-41d4-a716-446655440002",
                        "sub_issues_count": 3,
                        "created_at": "2024-01-01T10:30:00Z"
                      }
                    ]
                  },
                  "summary": "Paginated Cycle Work Items"
                }
              }
            }
          },
          "description": "Paginated list of cycle work items"
        }
      }
    },
    "post": {
      "operationId": "add_cycle_work_items",
      "tags": [
        "Cycles"
      ],
      "summary": "Add Work Items to Cycle",
      "parameters": [
        {
          "in": "path",
          "name": "cycle_id",
          "schema": {
            "type": "string",
            "format": "uuid"
          },
          "required": true
        },
        {
          "in": "path",
          "name": "project_id",
          "schema": {
            "type": "string"
          },
          "description": "Either the project's UUID or its workspace-scoped identifier (e.g. ``ENG``). Identifier matching is case-insensitive.",
          "required": true,
          "examples": {
            "uuid": {
              "value": "00000000-0000-0000-0000-000000000000",
              "summary": "UUID form"
            },
            "slug": {
              "value": "ENG",
              "summary": "Workspace-scoped identifier"
            }
          }
        },
        {
          "in": "path",
          "name": "slug",
          "schema": {
            "type": "string"
          },
          "description": "Workspace slug",
          "required": true,
          "examples": {
            "ExampleWorkspace": {
              "value": "my-workspace",
              "summary": "Example workspace",
              "description": "A typical workspace slug"
            }
          }
        }
      ],
      "responses": {
        "401": {
          "description": "Authentication credentials were not provided or are invalid."
        },
        "403": {
          "description": "Permission denied. User lacks required permissions."
        },
        "404": {
          "description": "The requested resource was not found."
        },
        "200": {
          "content": {
            "application/json": {
              "schema": {
                "$ref": "#/components/schemas/CycleIssue"
              },
              "examples": {
                "CycleIssue": {
                  "value": {
                    "id": "550e8400-e29b-41d4-a716-446655440000",
                    "cycle": "550e8400-e29b-41d4-a716-446655440001",
                    "issue": "550e8400-e29b-41d4-a716-446655440002",
                    "sub_issues_count": 3,
                    "created_at": "2024-01-01T10:30:00Z",
                    "updated_at": "2024-01-10T15:45:00Z"
                  }
                }
              }
            }
          },
          "description": "Cycle work items added"
        },
        "400": {
          "description": "Required fields are missing"
        }
      }
    }
  },
  "/api/v1/workspaces/{slug}/projects/{project_id}/cycles/{cycle_id}/cycle-issues/{issue_id}/": {
    "delete": {
      "operationId": "delete_cycle_work_item",
      "tags": [
        "Cycles"
      ],
      "summary": "Delete cycle work item",
      "parameters": [
        {
          "in": "path",
          "name": "cycle_id",
          "schema": {
            "type": "string",
            "format": "uuid"
          },
          "required": true
        },
        {
          "in": "path",
          "name": "issue_id",
          "schema": {
            "type": "string",
            "format": "uuid"
          },
          "required": true
        },
        {
          "in": "path",
          "name": "project_id",
          "schema": {
            "type": "string"
          },
          "description": "Either the project's UUID or its workspace-scoped identifier (e.g. ``ENG``). Identifier matching is case-insensitive.",
          "required": true,
          "examples": {
            "uuid": {
              "value": "00000000-0000-0000-0000-000000000000",
              "summary": "UUID form"
            },
            "slug": {
              "value": "ENG",
              "summary": "Workspace-scoped identifier"
            }
          }
        },
        {
          "in": "path",
          "name": "slug",
          "schema": {
            "type": "string"
          },
          "description": "Workspace slug",
          "required": true,
          "examples": {
            "ExampleWorkspace": {
              "value": "my-workspace",
              "summary": "Example workspace",
              "description": "A typical workspace slug"
            }
          }
        }
      ],
      "responses": {
        "401": {
          "description": "Authentication credentials were not provided or are invalid."
        },
        "403": {
          "description": "Permission denied. User lacks required permissions."
        },
        "404": {
          "description": "The requested resource was not found."
        },
        "204": {
          "description": "Resource deleted successfully"
        }
      }
    },
    "get": {
      "operationId": "retrieve_cycle_work_item",
      "tags": [
        "Cycles"
      ],
      "summary": "Retrieve cycle work item",
      "parameters": [
        {
          "in": "path",
          "name": "cycle_id",
          "schema": {
            "type": "string",
            "format": "uuid"
          },
          "required": true
        },
        {
          "in": "path",
          "name": "issue_id",
          "schema": {
            "type": "string",
            "format": "uuid"
          },
          "required": true
        },
        {
          "in": "path",
          "name": "project_id",
          "schema": {
            "type": "string"
          },
          "description": "Either the project's UUID or its workspace-scoped identifier (e.g. ``ENG``). Identifier matching is case-insensitive.",
          "required": true,
          "examples": {
            "uuid": {
              "value": "00000000-0000-0000-0000-000000000000",
              "summary": "UUID form"
            },
            "slug": {
              "value": "ENG",
              "summary": "Workspace-scoped identifier"
            }
          }
        },
        {
          "in": "path",
          "name": "slug",
          "schema": {
            "type": "string"
          },
          "description": "Workspace slug",
          "required": true,
          "examples": {
            "ExampleWorkspace": {
              "value": "my-workspace",
              "summary": "Example workspace",
              "description": "A typical workspace slug"
            }
          }
        }
      ],
      "responses": {
        "401": {
          "description": "Authentication credentials were not provided or are invalid."
        },
        "403": {
          "description": "Permission denied. User lacks required permissions."
        },
        "404": {
          "description": "The requested resource was not found."
        },
        "200": {
          "content": {
            "application/json": {
              "schema": {
                "$ref": "#/components/schemas/CycleIssue"
              },
              "examples": {
                "CycleIssue": {
                  "value": {
                    "id": "550e8400-e29b-41d4-a716-446655440000",
                    "cycle": "550e8400-e29b-41d4-a716-446655440001",
                    "issue": "550e8400-e29b-41d4-a716-446655440002",
                    "sub_issues_count": 3,
                    "created_at": "2024-01-01T10:30:00Z",
                    "updated_at": "2024-01-10T15:45:00Z"
                  }
                }
              }
            }
          },
          "description": "Cycle work items"
        }
      }
    }
  },
  "/api/v1/workspaces/{slug}/projects/{project_id}/cycles/{cycle_id}/transfer-issues/": {
    "post": {
      "operationId": "transfer_cycle_work_items",
      "tags": [
        "Cycles"
      ],
      "summary": "Transfer cycle work items",
      "parameters": [
        {
          "in": "path",
          "name": "cycle_id",
          "schema": {
            "type": "string",
            "format": "uuid"
          },
          "required": true
        },
        {
          "in": "path",
          "name": "project_id",
          "schema": {
            "type": "string"
          },
          "description": "Either the project's UUID or its workspace-scoped identifier (e.g. ``ENG``). Identifier matching is case-insensitive.",
          "required": true,
          "examples": {
            "uuid": {
              "value": "00000000-0000-0000-0000-000000000000",
              "summary": "UUID form"
            },
            "slug": {
              "value": "ENG",
              "summary": "Workspace-scoped identifier"
            }
          }
        },
        {
          "in": "path",
          "name": "slug",
          "schema": {
            "type": "string"
          },
          "description": "Workspace slug",
          "required": true,
          "examples": {
            "ExampleWorkspace": {
              "value": "my-workspace",
              "summary": "Example workspace",
              "description": "A typical workspace slug"
            }
          }
        }
      ],
      "responses": {
        "401": {
          "description": "Authentication credentials were not provided or are invalid."
        },
        "403": {
          "description": "Permission denied. User lacks required permissions."
        },
        "404": {
          "description": "The requested resource was not found."
        },
        "200": {
          "content": {
            "application/json": {
              "schema": {
                "type": "object",
                "properties": {
                  "message": {
                    "type": "string",
                    "description": "Success message",
                    "example": "Success"
                  }
                }
              },
              "examples": {
                "TransferCycleIssueSuccess": {
                  "value": {
                    "message": "Success"
                  },
                  "summary": "Transfer Cycle Issue Success",
                  "description": "Successful transfer of cycle issues to new cycle"
                }
              }
            }
          },
          "description": "Work items transferred successfully"
        },
        "400": {
          "content": {
            "application/json": {
              "schema": {
                "type": "object",
                "properties": {
                  "error": {
                    "type": "string",
                    "description": "Error message",
                    "example": "New Cycle Id is required"
                  }
                }
              },
              "examples": {
                "TransferCycleIssueError": {
                  "value": {
                    "error": "New Cycle Id is required"
                  },
                  "summary": "Transfer Cycle Issue Error",
                  "description": "Error when required cycle ID is missing"
                },
                "TransferToCompletedCycleError": {
                  "value": {
                    "error": "The cycle where the issues are transferred is already completed"
                  },
                  "summary": "Transfer to Completed Cycle Error",
                  "description": "Error when trying to transfer to a completed cycle"
                }
              }
            }
          },
          "description": "Bad request"
        }
      }
    }
  },
  "/api/v1/workspaces/{slug}/projects/{project_id}/cycles/{pk}/": {
    "delete": {
      "operationId": "delete_cycle",
      "tags": [
        "Cycles"
      ],
      "summary": "Delete cycle",
      "parameters": [
        {
          "in": "path",
          "name": "pk",
          "schema": {
            "type": "string",
            "format": "uuid"
          },
          "required": true
        },
        {
          "in": "path",
          "name": "project_id",
          "schema": {
            "type": "string"
          },
          "description": "Either the project's UUID or its workspace-scoped identifier (e.g. ``ENG``). Identifier matching is case-insensitive.",
          "required": true,
          "examples": {
            "uuid": {
              "value": "00000000-0000-0000-0000-000000000000",
              "summary": "UUID form"
            },
            "slug": {
              "value": "ENG",
              "summary": "Workspace-scoped identifier"
            }
          }
        },
        {
          "in": "path",
          "name": "slug",
          "schema": {
            "type": "string"
          },
          "description": "Workspace slug",
          "required": true,
          "examples": {
            "ExampleWorkspace": {
              "value": "my-workspace",
              "summary": "Example workspace",
              "description": "A typical workspace slug"
            }
          }
        }
      ],
      "responses": {
        "401": {
          "description": "Authentication credentials were not provided or are invalid."
        },
        "403": {
          "description": "Permission denied. User lacks required permissions."
        },
        "404": {
          "description": "The requested resource was not found."
        },
        "204": {
          "description": "Resource deleted successfully"
        }
      }
    },
    "get": {
      "operationId": "retrieve_cycle",
      "tags": [
        "Cycles"
      ],
      "summary": "Retrieve cycle",
      "parameters": [
        {
          "in": "path",
          "name": "pk",
          "schema": {
            "type": "string",
            "format": "uuid"
          },
          "required": true
        },
        {
          "in": "path",
          "name": "project_id",
          "schema": {
            "type": "string"
          },
          "description": "Either the project's UUID or its workspace-scoped identifier (e.g. ``ENG``). Identifier matching is case-insensitive.",
          "required": true,
          "examples": {
            "uuid": {
              "value": "00000000-0000-0000-0000-000000000000",
              "summary": "UUID form"
            },
            "slug": {
              "value": "ENG",
              "summary": "Workspace-scoped identifier"
            }
          }
        },
        {
          "in": "path",
          "name": "slug",
          "schema": {
            "type": "string"
          },
          "description": "Workspace slug",
          "required": true,
          "examples": {
            "ExampleWorkspace": {
              "value": "my-workspace",
              "summary": "Example workspace",
              "description": "A typical workspace slug"
            }
          }
        }
      ],
      "responses": {
        "401": {
          "description": "Authentication credentials were not provided or are invalid."
        },
        "403": {
          "description": "Permission denied. User lacks required permissions."
        },
        "404": {
          "description": "The requested resource was not found."
        },
        "200": {
          "content": {
            "application/json": {
              "schema": {
                "$ref": "#/components/schemas/Cycle"
              },
              "examples": {
                "Cycle": {
                  "value": {
                    "id": "550e8400-e29b-41d4-a716-446655440000",
                    "name": "Sprint 1 - Q1 2024",
                    "description": "First sprint of the quarter focusing on core features",
                    "start_date": "2024-01-01",
                    "end_date": "2024-01-14",
                    "status": "current",
                    "total_issues": 15,
                    "completed_issues": 8,
                    "cancelled_issues": 1,
                    "started_issues": 4,
                    "unstarted_issues": 2,
                    "backlog_issues": 0,
                    "created_at": "2024-01-01T10:30:00Z",
                    "updated_at": "2024-01-10T15:45:00Z"
                  }
                }
              }
            }
          },
          "description": "Cycles"
        }
      }
    },
    "patch": {
      "operationId": "update_cycle",
      "tags": [
        "Cycles"
      ],
      "summary": "Update cycle",
      "parameters": [
        {
          "in": "path",
          "name": "pk",
          "schema": {
            "type": "string",
            "format": "uuid"
          },
          "required": true
        },
        {
          "in": "path",
          "name": "project_id",
          "schema": {
            "type": "string"
          },
          "description": "Either the project's UUID or its workspace-scoped identifier (e.g. ``ENG``). Identifier matching is case-insensitive.",
          "required": true,
          "examples": {
            "uuid": {
              "value": "00000000-0000-0000-0000-000000000000",
              "summary": "UUID form"
            },
            "slug": {
              "value": "ENG",
              "summary": "Workspace-scoped identifier"
            }
          }
        },
        {
          "in": "path",
          "name": "slug",
          "schema": {
            "type": "string"
          },
          "description": "Workspace slug",
          "required": true,
          "examples": {
            "ExampleWorkspace": {
              "value": "my-workspace",
              "summary": "Example workspace",
              "description": "A typical workspace slug"
            }
          }
        }
      ],
      "responses": {
        "401": {
          "description": "Authentication credentials were not provided or are invalid."
        },
        "403": {
          "description": "Permission denied. User lacks required permissions."
        },
        "404": {
          "description": "The requested resource was not found."
        },
        "200": {
          "content": {
            "application/json": {
              "schema": {
                "$ref": "#/components/schemas/Cycle"
              },
              "examples": {
                "Cycle": {
                  "value": {
                    "id": "550e8400-e29b-41d4-a716-446655440000",
                    "name": "Sprint 1 - Q1 2024",
                    "description": "First sprint of the quarter focusing on core features",
                    "start_date": "2024-01-01",
                    "end_date": "2024-01-14",
                    "status": "current",
                    "total_issues": 15,
                    "completed_issues": 8,
                    "cancelled_issues": 1,
                    "started_issues": 4,
                    "unstarted_issues": 2,
                    "backlog_issues": 0,
                    "created_at": "2024-01-01T10:30:00Z",
                    "updated_at": "2024-01-10T15:45:00Z"
                  }
                }
              }
            }
          },
          "description": "Cycle updated"
        }
      }
    }
  },
  "/api/v1/workspaces/{slug}/projects/{project_id}/intake-issues/": {
    "get": {
      "operationId": "get_intake_work_items_list",
      "tags": [
        "Intake"
      ],
      "summary": "List intake work items",
      "parameters": [
        {
          "in": "query",
          "name": "cursor",
          "schema": {
            "type": "string"
          },
          "description": "Pagination cursor for getting next set of results",
          "examples": {
            "NextPageCursor": {
              "value": "20:1:0",
              "summary": "Next page cursor",
              "description": "Cursor format: 'page_size:page_number:offset'"
            }
          }
        },
        {
          "in": "query",
          "name": "expand",
          "schema": {
            "type": "string"
          },
          "description": "Comma-separated list of related fields to expand in response",
          "examples": {
            "ExpandAssignees": {
              "value": "assignees",
              "summary": "Expand assignees",
              "description": "Include full assignee details"
            },
            "MultipleExpansions": {
              "value": "assignees,labels,state",
              "summary": "Multiple expansions",
              "description": "Include details for multiple relations"
            }
          }
        },
        {
          "in": "query",
          "name": "fields",
          "schema": {
            "type": "string"
          },
          "description": "Comma-separated list of fields to include in response",
          "examples": {
            "BasicFields": {
              "value": "id,name,description",
              "summary": "Basic fields",
              "description": "Include only basic fields"
            },
            "WithRelations": {
              "value": "id,name,assignees,state",
              "summary": "With relations",
              "description": "Include fields with relationships"
            }
          }
        },
        {
          "in": "query",
          "name": "per_page",
          "schema": {
            "type": "integer"
          },
          "description": "Number of results per page (default: 20, max: 100)",
          "examples": {
            "Default": {
              "value": 20
            },
            "Maximum": {
              "value": 100
            }
          }
        },
        {
          "in": "path",
          "name": "project_id",
          "schema": {
            "type": "string"
          },
          "description": "Either the project's UUID or its workspace-scoped identifier (e.g. ``ENG``). Identifier matching is case-insensitive.",
          "required": true,
          "examples": {
            "uuid": {
              "value": "00000000-0000-0000-0000-000000000000",
              "summary": "UUID form"
            },
            "slug": {
              "value": "ENG",
              "summary": "Workspace-scoped identifier"
            }
          }
        },
        {
          "in": "path",
          "name": "slug",
          "schema": {
            "type": "string"
          },
          "description": "Workspace slug",
          "required": true,
          "examples": {
            "ExampleWorkspace": {
              "value": "my-workspace",
              "summary": "Example workspace",
              "description": "A typical workspace slug"
            }
          }
        }
      ],
      "responses": {
        "401": {
          "description": "Authentication credentials were not provided or are invalid."
        },
        "403": {
          "description": "Permission denied. User lacks required permissions."
        },
        "404": {
          "description": "The requested resource was not found."
        },
        "200": {
          "content": {
            "application/json": {
              "schema": {
                "$ref": "#/components/schemas/PaginatedIntakeIssueResponse"
              },
              "examples": {
                "PaginatedIntakeWorkItems": {
                  "value": {
                    "grouped_by": "state",
                    "sub_grouped_by": "priority",
                    "total_count": 150,
                    "next_cursor": "20:1:0",
                    "prev_cursor": "20:0:0",
                    "next_page_results": true,
                    "prev_page_results": false,
                    "count": 20,
                    "total_pages": 8,
                    "total_results": 150,
                    "extra_stats": null,
                    "results": [
                      {
                        "id": "550e8400-e29b-41d4-a716-446655440000",
                        "name": "Sample Item",
                        "created_at": "2024-01-15T12:00:00Z"
                      }
                    ]
                  },
                  "summary": "Paginated Intake Work Items"
                }
              }
            }
          },
          "description": "Paginated list of intake work items"
        }
      }
    },
    "post": {
      "operationId": "create_intake_work_item",
      "tags": [
        "Intake"
      ],
      "summary": "Create intake work item",
      "parameters": [
        {
          "in": "path",
          "name": "project_id",
          "schema": {
            "type": "string"
          },
          "description": "Either the project's UUID or its workspace-scoped identifier (e.g. ``ENG``). Identifier matching is case-insensitive.",
          "required": true,
          "examples": {
            "uuid": {
              "value": "00000000-0000-0000-0000-000000000000",
              "summary": "UUID form"
            },
            "slug": {
              "value": "ENG",
              "summary": "Workspace-scoped identifier"
            }
          }
        },
        {
          "in": "path",
          "name": "slug",
          "schema": {
            "type": "string"
          },
          "description": "Workspace slug",
          "required": true,
          "examples": {
            "ExampleWorkspace": {
              "value": "my-workspace",
              "summary": "Example workspace",
              "description": "A typical workspace slug"
            }
          }
        }
      ],
      "responses": {
        "401": {
          "description": "Authentication credentials were not provided or are invalid."
        },
        "403": {
          "description": "Permission denied. User lacks required permissions."
        },
        "404": {
          "description": "The requested resource was not found."
        },
        "201": {
          "content": {
            "application/json": {
              "schema": {
                "$ref": "#/components/schemas/IntakeIssue"
              },
              "examples": {
                "IntakeIssue": {
                  "value": {
                    "id": "550e8400-e29b-41d4-a716-446655440000",
                    "status": 0,
                    "source": "in_app",
                    "issue": {
                      "id": "550e8400-e29b-41d4-a716-446655440001",
                      "name": "Feature request: Dark mode",
                      "description": "Add dark mode support to the application",
                      "priority": "medium",
                      "sequence_id": 124
                    },
                    "created_at": "2024-01-01T10:30:00Z",
                    "updated_at": "2024-01-10T15:45:00Z"
                  }
                }
              }
            }
          },
          "description": "Intake work item created"
        },
        "400": {
          "description": "Invalid request data provided"
        }
      }
    }
  },
  "/api/v1/workspaces/{slug}/projects/{project_id}/intake-issues/{issue_id}/": {
    "delete": {
      "operationId": "delete_intake_work_item",
      "tags": [
        "Intake"
      ],
      "summary": "Delete intake work item",
      "parameters": [
        {
          "in": "path",
          "name": "issue_id",
          "schema": {
            "type": "string",
            "format": "uuid"
          },
          "description": "Issue ID",
          "required": true,
          "examples": {
            "ExampleIssueID": {
              "value": "550e8400-e29b-41d4-a716-446655440000",
              "summary": "Example issue ID",
              "description": "A typical issue UUID"
            }
          }
        },
        {
          "in": "path",
          "name": "project_id",
          "schema": {
            "type": "string"
          },
          "description": "Either the project's UUID or its workspace-scoped identifier (e.g. ``ENG``). Identifier matching is case-insensitive.",
          "required": true,
          "examples": {
            "uuid": {
              "value": "00000000-0000-0000-0000-000000000000",
              "summary": "UUID form"
            },
            "slug": {
              "value": "ENG",
              "summary": "Workspace-scoped identifier"
            }
          }
        },
        {
          "in": "path",
          "name": "slug",
          "schema": {
            "type": "string"
          },
          "description": "Workspace slug",
          "required": true,
          "examples": {
            "ExampleWorkspace": {
              "value": "my-workspace",
              "summary": "Example workspace",
              "description": "A typical workspace slug"
            }
          }
        }
      ],
      "responses": {
        "401": {
          "description": "Authentication credentials were not provided or are invalid."
        },
        "403": {
          "description": "Permission denied. User lacks required permissions."
        },
        "404": {
          "description": "The requested resource was not found."
        },
        "204": {
          "description": "Resource deleted successfully"
        }
      }
    },
    "get": {
      "operationId": "retrieve_intake_work_item",
      "tags": [
        "Intake"
      ],
      "summary": "Retrieve intake work item",
      "parameters": [
        {
          "in": "path",
          "name": "issue_id",
          "schema": {
            "type": "string",
            "format": "uuid"
          },
          "description": "Issue ID",
          "required": true,
          "examples": {
            "ExampleIssueID": {
              "value": "550e8400-e29b-41d4-a716-446655440000",
              "summary": "Example issue ID",
              "description": "A typical issue UUID"
            }
          }
        },
        {
          "in": "path",
          "name": "project_id",
          "schema": {
            "type": "string"
          },
          "description": "Either the project's UUID or its workspace-scoped identifier (e.g. ``ENG``). Identifier matching is case-insensitive.",
          "required": true,
          "examples": {
            "uuid": {
              "value": "00000000-0000-0000-0000-000000000000",
              "summary": "UUID form"
            },
            "slug": {
              "value": "ENG",
              "summary": "Workspace-scoped identifier"
            }
          }
        },
        {
          "in": "path",
          "name": "slug",
          "schema": {
            "type": "string"
          },
          "description": "Workspace slug",
          "required": true,
          "examples": {
            "ExampleWorkspace": {
              "value": "my-workspace",
              "summary": "Example workspace",
              "description": "A typical workspace slug"
            }
          }
        }
      ],
      "responses": {
        "401": {
          "description": "Authentication credentials were not provided or are invalid."
        },
        "403": {
          "description": "Permission denied. User lacks required permissions."
        },
        "404": {
          "description": "The requested resource was not found."
        },
        "200": {
          "content": {
            "application/json": {
              "schema": {
                "$ref": "#/components/schemas/IntakeIssue"
              },
              "examples": {
                "IntakeIssue": {
                  "value": {
                    "id": "550e8400-e29b-41d4-a716-446655440000",
                    "status": 0,
                    "source": "in_app",
                    "issue": {
                      "id": "550e8400-e29b-41d4-a716-446655440001",
                      "name": "Feature request: Dark mode",
                      "description": "Add dark mode support to the application",
                      "priority": "medium",
                      "sequence_id": 124
                    },
                    "created_at": "2024-01-01T10:30:00Z",
                    "updated_at": "2024-01-10T15:45:00Z"
                  }
                }
              }
            }
          },
          "description": "Intake work item"
        }
      }
    },
    "patch": {
      "operationId": "update_intake_work_item",
      "tags": [
        "Intake"
      ],
      "summary": "Update intake work item",
      "parameters": [
        {
          "in": "path",
          "name": "issue_id",
          "schema": {
            "type": "string",
            "format": "uuid"
          },
          "description": "Issue ID",
          "required": true,
          "examples": {
            "ExampleIssueID": {
              "value": "550e8400-e29b-41d4-a716-446655440000",
              "summary": "Example issue ID",
              "description": "A typical issue UUID"
            }
          }
        },
        {
          "in": "path",
          "name": "project_id",
          "schema": {
            "type": "string"
          },
          "description": "Either the project's UUID or its workspace-scoped identifier (e.g. ``ENG``). Identifier matching is case-insensitive.",
          "required": true,
          "examples": {
            "uuid": {
              "value": "00000000-0000-0000-0000-000000000000",
              "summary": "UUID form"
            },
            "slug": {
              "value": "ENG",
              "summary": "Workspace-scoped identifier"
            }
          }
        },
        {
          "in": "path",
          "name": "slug",
          "schema": {
            "type": "string"
          },
          "description": "Workspace slug",
          "required": true,
          "examples": {
            "ExampleWorkspace": {
              "value": "my-workspace",
              "summary": "Example workspace",
              "description": "A typical workspace slug"
            }
          }
        }
      ],
      "responses": {
        "401": {
          "description": "Authentication credentials were not provided or are invalid."
        },
        "403": {
          "description": "Permission denied. User lacks required permissions."
        },
        "404": {
          "description": "The requested resource was not found."
        },
        "200": {
          "content": {
            "application/json": {
              "schema": {
                "$ref": "#/components/schemas/IntakeIssue"
              },
              "examples": {
                "IntakeIssue": {
                  "value": {
                    "id": "550e8400-e29b-41d4-a716-446655440000",
                    "status": 0,
                    "source": "in_app",
                    "issue": {
                      "id": "550e8400-e29b-41d4-a716-446655440001",
                      "name": "Feature request: Dark mode",
                      "description": "Add dark mode support to the application",
                      "priority": "medium",
                      "sequence_id": 124
                    },
                    "created_at": "2024-01-01T10:30:00Z",
                    "updated_at": "2024-01-10T15:45:00Z"
                  }
                }
              }
            }
          },
          "description": "Intake work item updated"
        },
        "400": {
          "description": "Invalid request data provided"
        }
      }
    }
  },
  "/api/v1/workspaces/{slug}/projects/{project_id}/issues/": {
    "get": {
      "operationId": "list_work_items",
      "tags": [
        "Work Items"
      ],
      "summary": "List work items",
      "parameters": [
        {
          "in": "query",
          "name": "assignees",
          "schema": {
            "type": "string"
          },
          "description": "Filter by assignee. Comma-separated user UUIDs."
        },
        {
          "in": "query",
          "name": "cursor",
          "schema": {
            "type": "string"
          },
          "description": "Pagination cursor for getting next set of results",
          "examples": {
            "NextPageCursor": {
              "value": "20:1:0",
              "summary": "Next page cursor",
              "description": "Cursor format: 'page_size:page_number:offset'"
            }
          }
        },
        {
          "in": "query",
          "name": "expand",
          "schema": {
            "type": "string"
          },
          "description": "Comma-separated list of related fields to expand in response",
          "examples": {
            "ExpandAssignees": {
              "value": "assignees",
              "summary": "Expand assignees",
              "description": "Include full assignee details"
            },
            "MultipleExpansions": {
              "value": "assignees,labels,state",
              "summary": "Multiple expansions",
              "description": "Include details for multiple relations"
            }
          }
        },
        {
          "in": "query",
          "name": "external_id",
          "schema": {
            "type": "string"
          },
          "description": "External system identifier for filtering or lookup",
          "examples": {
            "GitHubIssue": {
              "value": "1234567890",
              "summary": "GitHub Issue",
              "description": "GitHub issue number"
            }
          }
        },
        {
          "in": "query",
          "name": "external_source",
          "schema": {
            "type": "string"
          },
          "description": "External system source name for filtering or lookup",
          "examples": {
            "GitHub": {
              "value": "github",
              "description": "GitHub integration source"
            },
            "Jira": {
              "value": "jira",
              "description": "Jira integration source"
            }
          }
        },
        {
          "in": "query",
          "name": "fields",
          "schema": {
            "type": "string"
          },
          "description": "Comma-separated list of fields to include in response",
          "examples": {
            "BasicFields": {
              "value": "id,name,description",
              "summary": "Basic fields",
              "description": "Include only basic fields"
            },
            "WithRelations": {
              "value": "id,name,assignees,state",
              "summary": "With relations",
              "description": "Include fields with relationships"
            }
          }
        },
        {
          "in": "query",
          "name": "labels",
          "schema": {
            "type": "string"
          },
          "description": "Filter by label. Comma-separated label UUIDs and/or label names (case-insensitive).",
          "examples": {
            "ByName": {
              "value": "bug,frontend",
              "summary": "By name"
            }
          }
        },
        {
          "in": "query",
          "name": "order_by",
          "schema": {
            "type": "string"
          },
          "description": "Field to order results by. Prefix with '-' for descending order",
          "examples": {
            "CreatedDateDescending": {
              "value": "-created_at",
              "summary": "Created date descending",
              "description": "Most recent items first"
            },
            "PriorityAscending": {
              "value": "priority",
              "summary": "Priority ascending",
              "description": "Order by priority (urgent, high, medium, low, none)"
            },
            "StateGroup": {
              "value": "state__group",
              "summary": "State group",
              "description": "Order by state group (backlog, unstarted, started, review, test, completed, cancelled)"
            },
            "AssigneeName": {
              "value": "assignees__first_name",
              "summary": "Assignee name",
              "description": "Order by assignee first name"
            }
          }
        },
        {
          "in": "query",
          "name": "parent",
          "schema": {
            "type": "string"
          },
          "description": "Filter by parent work item. Comma-separated parent UUIDs and/or identifiers (e.g. PROJ-123). Pass `null` to return only top-level work items.",
          "examples": {
            "ChildrenOfAnEpic": {
              "value": "PROJ-123",
              "summary": "Children of an epic"
            },
            "Top-levelOnly": {
              "value": "null",
              "summary": "Top-level only"
            }
          }
        },
        {
          "in": "query",
          "name": "per_page",
          "schema": {
            "type": "integer"
          },
          "description": "Number of results per page (default: 20, max: 100)",
          "examples": {
            "Default": {
              "value": 20
            },
            "Maximum": {
              "value": 100
            }
          }
        },
        {
          "in": "query",
          "name": "priority",
          "schema": {
            "type": "string"
          },
          "description": "Filter by priority. Comma-separated values from: urgent, high, medium, low, none.",
          "examples": {
            "Hot": {
              "value": "urgent,high"
            }
          }
        },
        {
          "in": "path",
          "name": "project_id",
          "schema": {
            "type": "string"
          },
          "description": "Either the project's UUID or its workspace-scoped identifier (e.g. ``ENG``). Identifier matching is case-insensitive.",
          "required": true,
          "examples": {
            "uuid": {
              "value": "00000000-0000-0000-0000-000000000000",
              "summary": "UUID form"
            },
            "slug": {
              "value": "ENG",
              "summary": "Workspace-scoped identifier"
            }
          }
        },
        {
          "in": "path",
          "name": "slug",
          "schema": {
            "type": "string"
          },
          "description": "Workspace slug",
          "required": true,
          "examples": {
            "ExampleWorkspace": {
              "value": "my-workspace",
              "summary": "Example workspace",
              "description": "A typical workspace slug"
            }
          }
        },
        {
          "in": "query",
          "name": "state",
          "schema": {
            "type": "string"
          },
          "description": "Filter by state. Comma-separated state UUIDs and/or state names (case-insensitive exact match within the project). An unknown name returns 400 listing the valid state names.",
          "examples": {
            "ByName": {
              "value": "Backlog,Todo",
              "summary": "By name"
            },
            "ById": {
              "value": "550e8400-e29b-41d4-a716-446655440000",
              "summary": "By id"
            }
          }
        },
        {
          "in": "query",
          "name": "state_group",
          "schema": {
            "type": "string"
          },
          "description": "Filter by state group. Comma-separated values from: backlog, unstarted, started, review, test, completed, cancelled.",
          "examples": {
            "OpenWork": {
              "value": "unstarted,started",
              "summary": "Open work"
            }
          }
        }
      ],
      "responses": {
        "401": {
          "description": "Authentication credentials were not provided or are invalid."
        },
        "403": {
          "description": "Permission denied. User lacks required permissions."
        },
        "404": {
          "description": "Project not found"
        },
        "200": {
          "content": {
            "application/json": {
              "schema": {
                "$ref": "#/components/schemas/PaginatedWorkItemResponse"
              },
              "examples": {
                "PaginatedWorkItems": {
                  "value": {
                    "grouped_by": "state",
                    "sub_grouped_by": "priority",
                    "total_count": 150,
                    "next_cursor": "20:1:0",
                    "prev_cursor": "20:0:0",
                    "next_page_results": true,
                    "prev_page_results": false,
                    "count": 20,
                    "total_pages": 8,
                    "total_results": 150,
                    "extra_stats": null,
                    "results": [
                      {
                        "id": "550e8400-e29b-41d4-a716-446655440000",
                        "name": "Fix authentication bug in user login",
                        "description": "Users are unable to log in due to authentication service timeout",
                        "priority": "high",
                        "sequence_id": 123,
                        "state": {
                          "id": "550e8400-e29b-41d4-a716-446655440001",
                          "name": "In Progress",
                          "group": "started"
                        },
                        "assignees": [],
                        "labels": [],
                        "created_at": "2024-01-15T10:30:00Z"
                      }
                    ]
                  },
                  "summary": "Paginated Work Items"
                }
              }
            }
          },
          "description": "Paginated list of work items"
        },
        "400": {
          "description": "Invalid request data provided"
        }
      }
    },
    "post": {
      "operationId": "create_work_item",
      "tags": [
        "Work Items"
      ],
      "summary": "Create work item",
      "parameters": [
        {
          "in": "path",
          "name": "project_id",
          "schema": {
            "type": "string"
          },
          "description": "Either the project's UUID or its workspace-scoped identifier (e.g. ``ENG``). Identifier matching is case-insensitive.",
          "required": true,
          "examples": {
            "uuid": {
              "value": "00000000-0000-0000-0000-000000000000",
              "summary": "UUID form"
            },
            "slug": {
              "value": "ENG",
              "summary": "Workspace-scoped identifier"
            }
          }
        },
        {
          "in": "path",
          "name": "slug",
          "schema": {
            "type": "string"
          },
          "description": "Workspace slug",
          "required": true,
          "examples": {
            "ExampleWorkspace": {
              "value": "my-workspace",
              "summary": "Example workspace",
              "description": "A typical workspace slug"
            }
          }
        }
      ],
      "responses": {
        "401": {
          "description": "Authentication credentials were not provided or are invalid."
        },
        "403": {
          "description": "Permission denied. User lacks required permissions."
        },
        "404": {
          "description": "Project not found"
        },
        "201": {
          "content": {
            "application/json": {
              "schema": {
                "$ref": "#/components/schemas/Issue"
              },
              "examples": {
                "Issue": {
                  "value": {
                    "id": "550e8400-e29b-41d4-a716-446655440000",
                    "name": "Implement user authentication",
                    "description": "Add OAuth 2.0 authentication flow",
                    "sequence_id": 1,
                    "priority": "high",
                    "assignees": [
                      "550e8400-e29b-41d4-a716-446655440001"
                    ],
                    "labels": [
                      "550e8400-e29b-41d4-a716-446655440002"
                    ],
                    "created_at": "2024-01-15T10:30:00Z",
                    "updated_at": "2024-01-15T10:30:00Z"
                  }
                }
              }
            }
          },
          "description": "Work Item created successfully"
        },
        "400": {
          "description": "Invalid request data provided"
        },
        "409": {
          "description": "Resource with same external ID already exists"
        }
      }
    }
  },
  "/api/v1/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/activities/": {
    "get": {
      "operationId": "list_work_item_activities",
      "tags": [
        "Work Item Activity"
      ],
      "summary": null,
      "parameters": [
        {
          "in": "query",
          "name": "cursor",
          "schema": {
            "type": "string"
          },
          "description": "Pagination cursor for getting next set of results",
          "examples": {
            "NextPageCursor": {
              "value": "20:1:0",
              "summary": "Next page cursor",
              "description": "Cursor format: 'page_size:page_number:offset'"
            }
          }
        },
        {
          "in": "query",
          "name": "expand",
          "schema": {
            "type": "string"
          },
          "description": "Comma-separated list of related fields to expand in response",
          "examples": {
            "ExpandAssignees": {
              "value": "assignees",
              "summary": "Expand assignees",
              "description": "Include full assignee details"
            },
            "MultipleExpansions": {
              "value": "assignees,labels,state",
              "summary": "Multiple expansions",
              "description": "Include details for multiple relations"
            }
          }
        },
        {
          "in": "query",
          "name": "fields",
          "schema": {
            "type": "string"
          },
          "description": "Comma-separated list of fields to include in response",
          "examples": {
            "BasicFields": {
              "value": "id,name,description",
              "summary": "Basic fields",
              "description": "Include only basic fields"
            },
            "WithRelations": {
              "value": "id,name,assignees,state",
              "summary": "With relations",
              "description": "Include fields with relationships"
            }
          }
        },
        {
          "in": "path",
          "name": "issue_id",
          "schema": {
            "type": "string",
            "format": "uuid"
          },
          "description": "Issue ID",
          "required": true,
          "examples": {
            "ExampleIssueID": {
              "value": "550e8400-e29b-41d4-a716-446655440000",
              "summary": "Example issue ID",
              "description": "A typical issue UUID"
            }
          }
        },
        {
          "in": "query",
          "name": "order_by",
          "schema": {
            "type": "string"
          },
          "description": "Field to order results by. Prefix with '-' for descending order",
          "examples": {
            "CreatedDateDescending": {
              "value": "-created_at",
              "summary": "Created date descending",
              "description": "Most recent items first"
            },
            "PriorityAscending": {
              "value": "priority",
              "summary": "Priority ascending",
              "description": "Order by priority (urgent, high, medium, low, none)"
            },
            "StateGroup": {
              "value": "state__group",
              "summary": "State group",
              "description": "Order by state group (backlog, unstarted, started, review, test, completed, cancelled)"
            },
            "AssigneeName": {
              "value": "assignees__first_name",
              "summary": "Assignee name",
              "description": "Order by assignee first name"
            }
          }
        },
        {
          "in": "query",
          "name": "per_page",
          "schema": {
            "type": "integer"
          },
          "description": "Number of results per page (default: 20, max: 100)",
          "examples": {
            "Default": {
              "value": 20
            },
            "Maximum": {
              "value": 100
            }
          }
        },
        {
          "in": "path",
          "name": "project_id",
          "schema": {
            "type": "string"
          },
          "description": "Either the project's UUID or its workspace-scoped identifier (e.g. ``ENG``). Identifier matching is case-insensitive.",
          "required": true,
          "examples": {
            "uuid": {
              "value": "00000000-0000-0000-0000-000000000000",
              "summary": "UUID form"
            },
            "slug": {
              "value": "ENG",
              "summary": "Workspace-scoped identifier"
            }
          }
        },
        {
          "in": "path",
          "name": "slug",
          "schema": {
            "type": "string"
          },
          "description": "Workspace slug",
          "required": true,
          "examples": {
            "ExampleWorkspace": {
              "value": "my-workspace",
              "summary": "Example workspace",
              "description": "A typical workspace slug"
            }
          }
        }
      ],
      "responses": {
        "401": {
          "description": "Authentication credentials were not provided or are invalid."
        },
        "403": {
          "description": "Permission denied. User lacks required permissions."
        },
        "404": {
          "description": "Issue not found"
        },
        "200": {
          "content": {
            "application/json": {
              "schema": {
                "$ref": "#/components/schemas/PaginatedIssueActivityResponse"
              },
              "examples": {
                "PaginatedIssueActivities": {
                  "value": {
                    "grouped_by": "state",
                    "sub_grouped_by": "priority",
                    "total_count": 150,
                    "next_cursor": "20:1:0",
                    "prev_cursor": "20:0:0",
                    "next_page_results": true,
                    "prev_page_results": false,
                    "count": 20,
                    "total_pages": 8,
                    "total_results": 150,
                    "extra_stats": null,
                    "results": [
                      {
                        "id": "550e8400-e29b-41d4-a716-446655440000",
                        "name": "Sample Item",
                        "created_at": "2024-01-15T12:00:00Z"
                      }
                    ]
                  },
                  "summary": "Paginated Issue Activities"
                }
              }
            }
          },
          "description": "Paginated list of issue activities"
        },
        "400": {
          "description": "Invalid request data provided"
        }
      }
    }
  },
  "/api/v1/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/activities/{pk}/": {
    "get": {
      "operationId": "retrieve_work_item_activity",
      "tags": [
        "Work Item Activity"
      ],
      "summary": null,
      "parameters": [
        {
          "in": "query",
          "name": "cursor",
          "schema": {
            "type": "string"
          },
          "description": "Pagination cursor for getting next set of results",
          "examples": {
            "NextPageCursor": {
              "value": "20:1:0",
              "summary": "Next page cursor",
              "description": "Cursor format: 'page_size:page_number:offset'"
            }
          }
        },
        {
          "in": "query",
          "name": "expand",
          "schema": {
            "type": "string"
          },
          "description": "Comma-separated list of related fields to expand in response",
          "examples": {
            "ExpandAssignees": {
              "value": "assignees",
              "summary": "Expand assignees",
              "description": "Include full assignee details"
            },
            "MultipleExpansions": {
              "value": "assignees,labels,state",
              "summary": "Multiple expansions",
              "description": "Include details for multiple relations"
            }
          }
        },
        {
          "in": "query",
          "name": "fields",
          "schema": {
            "type": "string"
          },
          "description": "Comma-separated list of fields to include in response",
          "examples": {
            "BasicFields": {
              "value": "id,name,description",
              "summary": "Basic fields",
              "description": "Include only basic fields"
            },
            "WithRelations": {
              "value": "id,name,assignees,state",
              "summary": "With relations",
              "description": "Include fields with relationships"
            }
          }
        },
        {
          "in": "path",
          "name": "issue_id",
          "schema": {
            "type": "string",
            "format": "uuid"
          },
          "description": "Issue ID",
          "required": true,
          "examples": {
            "ExampleIssueID": {
              "value": "550e8400-e29b-41d4-a716-446655440000",
              "summary": "Example issue ID",
              "description": "A typical issue UUID"
            }
          }
        },
        {
          "in": "query",
          "name": "order_by",
          "schema": {
            "type": "string"
          },
          "description": "Field to order results by. Prefix with '-' for descending order",
          "examples": {
            "CreatedDateDescending": {
              "value": "-created_at",
              "summary": "Created date descending",
              "description": "Most recent items first"
            },
            "PriorityAscending": {
              "value": "priority",
              "summary": "Priority ascending",
              "description": "Order by priority (urgent, high, medium, low, none)"
            },
            "StateGroup": {
              "value": "state__group",
              "summary": "State group",
              "description": "Order by state group (backlog, unstarted, started, review, test, completed, cancelled)"
            },
            "AssigneeName": {
              "value": "assignees__first_name",
              "summary": "Assignee name",
              "description": "Order by assignee first name"
            }
          }
        },
        {
          "in": "query",
          "name": "per_page",
          "schema": {
            "type": "integer"
          },
          "description": "Number of results per page (default: 20, max: 100)",
          "examples": {
            "Default": {
              "value": 20
            },
            "Maximum": {
              "value": 100
            }
          }
        },
        {
          "in": "path",
          "name": "pk",
          "schema": {
            "type": "string",
            "format": "uuid"
          },
          "description": "Activity ID",
          "required": true,
          "examples": {
            "ExampleActivityID": {
              "value": "550e8400-e29b-41d4-a716-446655440000",
              "summary": "Example activity ID",
              "description": "A typical activity UUID"
            }
          }
        },
        {
          "in": "path",
          "name": "project_id",
          "schema": {
            "type": "string"
          },
          "description": "Either the project's UUID or its workspace-scoped identifier (e.g. ``ENG``). Identifier matching is case-insensitive.",
          "required": true,
          "examples": {
            "uuid": {
              "value": "00000000-0000-0000-0000-000000000000",
              "summary": "UUID form"
            },
            "slug": {
              "value": "ENG",
              "summary": "Workspace-scoped identifier"
            }
          }
        },
        {
          "in": "path",
          "name": "slug",
          "schema": {
            "type": "string"
          },
          "description": "Workspace slug",
          "required": true,
          "examples": {
            "ExampleWorkspace": {
              "value": "my-workspace",
              "summary": "Example workspace",
              "description": "A typical workspace slug"
            }
          }
        }
      ],
      "responses": {
        "401": {
          "description": "Authentication credentials were not provided or are invalid."
        },
        "403": {
          "description": "Permission denied. User lacks required permissions."
        },
        "404": {
          "description": "Issue not found"
        },
        "200": {
          "content": {
            "application/json": {
              "schema": {
                "$ref": "#/components/schemas/PaginatedIssueActivityDetailResponse"
              },
              "examples": {
                "WorkItemActivityDetails": {
                  "value": {
                    "grouped_by": "state",
                    "sub_grouped_by": "priority",
                    "total_count": 150,
                    "next_cursor": "20:1:0",
                    "prev_cursor": "20:0:0",
                    "next_page_results": true,
                    "prev_page_results": false,
                    "count": 20,
                    "total_pages": 8,
                    "total_results": 150,
                    "extra_stats": null,
                    "results": [
                      {
                        "id": "550e8400-e29b-41d4-a716-446655440000",
                        "name": "Sample Item",
                        "created_at": "2024-01-15T12:00:00Z"
                      }
                    ]
                  },
                  "summary": "Work Item Activity Details"
                }
              }
            }
          },
          "description": "Paginated list of work item activities"
        },
        "400": {
          "description": "Invalid request data provided"
        }
      }
    }
  },
  "/api/v1/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/comments/": {
    "get": {
      "operationId": "list_work_item_comments",
      "tags": [
        "Work Item Comments"
      ],
      "summary": null,
      "parameters": [
        {
          "in": "query",
          "name": "cursor",
          "schema": {
            "type": "string"
          },
          "description": "Pagination cursor for getting next set of results",
          "examples": {
            "NextPageCursor": {
              "value": "20:1:0",
              "summary": "Next page cursor",
              "description": "Cursor format: 'page_size:page_number:offset'"
            }
          }
        },
        {
          "in": "query",
          "name": "expand",
          "schema": {
            "type": "string"
          },
          "description": "Comma-separated list of related fields to expand in response",
          "examples": {
            "ExpandAssignees": {
              "value": "assignees",
              "summary": "Expand assignees",
              "description": "Include full assignee details"
            },
            "MultipleExpansions": {
              "value": "assignees,labels,state",
              "summary": "Multiple expansions",
              "description": "Include details for multiple relations"
            }
          }
        },
        {
          "in": "query",
          "name": "fields",
          "schema": {
            "type": "string"
          },
          "description": "Comma-separated list of fields to include in response",
          "examples": {
            "BasicFields": {
              "value": "id,name,description",
              "summary": "Basic fields",
              "description": "Include only basic fields"
            },
            "WithRelations": {
              "value": "id,name,assignees,state",
              "summary": "With relations",
              "description": "Include fields with relationships"
            }
          }
        },
        {
          "in": "path",
          "name": "issue_id",
          "schema": {
            "type": "string",
            "format": "uuid"
          },
          "description": "Issue ID",
          "required": true,
          "examples": {
            "ExampleIssueID": {
              "value": "550e8400-e29b-41d4-a716-446655440000",
              "summary": "Example issue ID",
              "description": "A typical issue UUID"
            }
          }
        },
        {
          "in": "query",
          "name": "order_by",
          "schema": {
            "type": "string"
          },
          "description": "Field to order results by. Prefix with '-' for descending order",
          "examples": {
            "CreatedDateDescending": {
              "value": "-created_at",
              "summary": "Created date descending",
              "description": "Most recent items first"
            },
            "PriorityAscending": {
              "value": "priority",
              "summary": "Priority ascending",
              "description": "Order by priority (urgent, high, medium, low, none)"
            },
            "StateGroup": {
              "value": "state__group",
              "summary": "State group",
              "description": "Order by state group (backlog, unstarted, started, review, test, completed, cancelled)"
            },
            "AssigneeName": {
              "value": "assignees__first_name",
              "summary": "Assignee name",
              "description": "Order by assignee first name"
            }
          }
        },
        {
          "in": "query",
          "name": "per_page",
          "schema": {
            "type": "integer"
          },
          "description": "Number of results per page (default: 20, max: 100)",
          "examples": {
            "Default": {
              "value": 20
            },
            "Maximum": {
              "value": 100
            }
          }
        },
        {
          "in": "path",
          "name": "project_id",
          "schema": {
            "type": "string"
          },
          "description": "Either the project's UUID or its workspace-scoped identifier (e.g. ``ENG``). Identifier matching is case-insensitive.",
          "required": true,
          "examples": {
            "uuid": {
              "value": "00000000-0000-0000-0000-000000000000",
              "summary": "UUID form"
            },
            "slug": {
              "value": "ENG",
              "summary": "Workspace-scoped identifier"
            }
          }
        },
        {
          "in": "path",
          "name": "slug",
          "schema": {
            "type": "string"
          },
          "description": "Workspace slug",
          "required": true,
          "examples": {
            "ExampleWorkspace": {
              "value": "my-workspace",
              "summary": "Example workspace",
              "description": "A typical workspace slug"
            }
          }
        }
      ],
      "responses": {
        "401": {
          "description": "Authentication credentials were not provided or are invalid."
        },
        "403": {
          "description": "Permission denied. User lacks required permissions."
        },
        "404": {
          "description": "Issue not found"
        },
        "200": {
          "content": {
            "application/json": {
              "schema": {
                "$ref": "#/components/schemas/PaginatedIssueCommentResponse"
              },
              "examples": {
                "PaginatedWorkItemComments": {
                  "value": {
                    "grouped_by": "state",
                    "sub_grouped_by": "priority",
                    "total_count": 150,
                    "next_cursor": "20:1:0",
                    "prev_cursor": "20:0:0",
                    "next_page_results": true,
                    "prev_page_results": false,
                    "count": 20,
                    "total_pages": 8,
                    "total_results": 150,
                    "extra_stats": null,
                    "results": [
                      {
                        "id": "550e8400-e29b-41d4-a716-446655440000",
                        "name": "Sample Item",
                        "created_at": "2024-01-15T12:00:00Z"
                      }
                    ]
                  },
                  "summary": "Paginated Work Item Comments"
                }
              }
            }
          },
          "description": "Paginated list of work item comments"
        }
      }
    },
    "post": {
      "operationId": "create_work_item_comment",
      "tags": [
        "Work Item Comments"
      ],
      "summary": null,
      "parameters": [
        {
          "in": "path",
          "name": "issue_id",
          "schema": {
            "type": "string",
            "format": "uuid"
          },
          "description": "Issue ID",
          "required": true,
          "examples": {
            "ExampleIssueID": {
              "value": "550e8400-e29b-41d4-a716-446655440000",
              "summary": "Example issue ID",
              "description": "A typical issue UUID"
            }
          }
        },
        {
          "in": "path",
          "name": "project_id",
          "schema": {
            "type": "string"
          },
          "description": "Either the project's UUID or its workspace-scoped identifier (e.g. ``ENG``). Identifier matching is case-insensitive.",
          "required": true,
          "examples": {
            "uuid": {
              "value": "00000000-0000-0000-0000-000000000000",
              "summary": "UUID form"
            },
            "slug": {
              "value": "ENG",
              "summary": "Workspace-scoped identifier"
            }
          }
        },
        {
          "in": "path",
          "name": "slug",
          "schema": {
            "type": "string"
          },
          "description": "Workspace slug",
          "required": true,
          "examples": {
            "ExampleWorkspace": {
              "value": "my-workspace",
              "summary": "Example workspace",
              "description": "A typical workspace slug"
            }
          }
        }
      ],
      "responses": {
        "401": {
          "description": "Authentication credentials were not provided or are invalid."
        },
        "403": {
          "description": "Permission denied. User lacks required permissions."
        },
        "404": {
          "description": "Issue not found"
        },
        "201": {
          "content": {
            "application/json": {
              "schema": {
                "$ref": "#/components/schemas/IssueComment"
              },
              "examples": {
                "IssueComment": {
                  "value": {
                    "id": "550e8400-e29b-41d4-a716-446655440000",
                    "comment_html": "<p>This issue has been resolved by implementing OAuth 2.0 flow.</p>",
                    "labels": [],
                    "comment_json": {
                      "type": "doc",
                      "content": [
                        {
                          "type": "paragraph",
                          "content": [
                            {
                              "type": "text",
                              "text": "This issue has been resolved by implementing OAuth 2.0 flow."
                            }
                          ]
                        }
                      ]
                    },
                    "actor": {
                      "id": "550e8400-e29b-41d4-a716-446655440001",
                      "first_name": "John",
                      "last_name": "Doe",
                      "display_name": "John Doe",
                      "avatar": "https://example.com/avatar.jpg"
                    },
                    "created_at": "2024-01-01T10:30:00Z",
                    "updated_at": "2024-01-10T15:45:00Z"
                  }
                }
              }
            }
          },
          "description": "Work item comment created successfully"
        },
        "400": {
          "description": "Invalid request data provided"
        },
        "409": {
          "description": "Resource with same external ID already exists"
        }
      }
    }
  },
  "/api/v1/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/comments/{pk}/": {
    "delete": {
      "operationId": "delete_work_item_comment",
      "tags": [
        "Work Item Comments"
      ],
      "summary": null,
      "parameters": [
        {
          "in": "path",
          "name": "issue_id",
          "schema": {
            "type": "string",
            "format": "uuid"
          },
          "description": "Issue ID",
          "required": true,
          "examples": {
            "ExampleIssueID": {
              "value": "550e8400-e29b-41d4-a716-446655440000",
              "summary": "Example issue ID",
              "description": "A typical issue UUID"
            }
          }
        },
        {
          "in": "path",
          "name": "pk",
          "schema": {
            "type": "string",
            "format": "uuid"
          },
          "description": "Comment ID",
          "required": true,
          "examples": {
            "ExampleCommentID": {
              "value": "550e8400-e29b-41d4-a716-446655440000",
              "summary": "Example comment ID",
              "description": "A typical comment UUID"
            }
          }
        },
        {
          "in": "path",
          "name": "project_id",
          "schema": {
            "type": "string"
          },
          "description": "Either the project's UUID or its workspace-scoped identifier (e.g. ``ENG``). Identifier matching is case-insensitive.",
          "required": true,
          "examples": {
            "uuid": {
              "value": "00000000-0000-0000-0000-000000000000",
              "summary": "UUID form"
            },
            "slug": {
              "value": "ENG",
              "summary": "Workspace-scoped identifier"
            }
          }
        },
        {
          "in": "path",
          "name": "slug",
          "schema": {
            "type": "string"
          },
          "description": "Workspace slug",
          "required": true,
          "examples": {
            "ExampleWorkspace": {
              "value": "my-workspace",
              "summary": "Example workspace",
              "description": "A typical workspace slug"
            }
          }
        }
      ],
      "responses": {
        "401": {
          "description": "Authentication credentials were not provided or are invalid."
        },
        "403": {
          "description": "Permission denied. User lacks required permissions."
        },
        "404": {
          "description": "Comment not found"
        },
        "204": {
          "description": "Work item comment deleted successfully"
        }
      }
    },
    "get": {
      "operationId": "retrieve_work_item_comment",
      "tags": [
        "Work Item Comments"
      ],
      "summary": null,
      "parameters": [
        {
          "in": "path",
          "name": "issue_id",
          "schema": {
            "type": "string",
            "format": "uuid"
          },
          "description": "Issue ID",
          "required": true,
          "examples": {
            "ExampleIssueID": {
              "value": "550e8400-e29b-41d4-a716-446655440000",
              "summary": "Example issue ID",
              "description": "A typical issue UUID"
            }
          }
        },
        {
          "in": "path",
          "name": "pk",
          "schema": {
            "type": "string",
            "format": "uuid"
          },
          "description": "Comment ID",
          "required": true,
          "examples": {
            "ExampleCommentID": {
              "value": "550e8400-e29b-41d4-a716-446655440000",
              "summary": "Example comment ID",
              "description": "A typical comment UUID"
            }
          }
        },
        {
          "in": "path",
          "name": "project_id",
          "schema": {
            "type": "string"
          },
          "description": "Either the project's UUID or its workspace-scoped identifier (e.g. ``ENG``). Identifier matching is case-insensitive.",
          "required": true,
          "examples": {
            "uuid": {
              "value": "00000000-0000-0000-0000-000000000000",
              "summary": "UUID form"
            },
            "slug": {
              "value": "ENG",
              "summary": "Workspace-scoped identifier"
            }
          }
        },
        {
          "in": "path",
          "name": "slug",
          "schema": {
            "type": "string"
          },
          "description": "Workspace slug",
          "required": true,
          "examples": {
            "ExampleWorkspace": {
              "value": "my-workspace",
              "summary": "Example workspace",
              "description": "A typical workspace slug"
            }
          }
        }
      ],
      "responses": {
        "401": {
          "description": "Authentication credentials were not provided or are invalid."
        },
        "403": {
          "description": "Permission denied. User lacks required permissions."
        },
        "404": {
          "description": "Issue not found"
        },
        "200": {
          "content": {
            "application/json": {
              "schema": {
                "$ref": "#/components/schemas/IssueComment"
              },
              "examples": {
                "IssueComment": {
                  "value": {
                    "id": "550e8400-e29b-41d4-a716-446655440000",
                    "comment_html": "<p>This issue has been resolved by implementing OAuth 2.0 flow.</p>",
                    "labels": [],
                    "comment_json": {
                      "type": "doc",
                      "content": [
                        {
                          "type": "paragraph",
                          "content": [
                            {
                              "type": "text",
                              "text": "This issue has been resolved by implementing OAuth 2.0 flow."
                            }
                          ]
                        }
                      ]
                    },
                    "actor": {
                      "id": "550e8400-e29b-41d4-a716-446655440001",
                      "first_name": "John",
                      "last_name": "Doe",
                      "display_name": "John Doe",
                      "avatar": "https://example.com/avatar.jpg"
                    },
                    "created_at": "2024-01-01T10:30:00Z",
                    "updated_at": "2024-01-10T15:45:00Z"
                  }
                }
              }
            }
          },
          "description": "Work item comments"
        },
        "400": {
          "description": "Invalid request data provided"
        }
      }
    },
    "patch": {
      "operationId": "update_work_item_comment",
      "tags": [
        "Work Item Comments"
      ],
      "summary": null,
      "parameters": [
        {
          "in": "path",
          "name": "issue_id",
          "schema": {
            "type": "string",
            "format": "uuid"
          },
          "description": "Issue ID",
          "required": true,
          "examples": {
            "ExampleIssueID": {
              "value": "550e8400-e29b-41d4-a716-446655440000",
              "summary": "Example issue ID",
              "description": "A typical issue UUID"
            }
          }
        },
        {
          "in": "path",
          "name": "pk",
          "schema": {
            "type": "string",
            "format": "uuid"
          },
          "description": "Comment ID",
          "required": true,
          "examples": {
            "ExampleCommentID": {
              "value": "550e8400-e29b-41d4-a716-446655440000",
              "summary": "Example comment ID",
              "description": "A typical comment UUID"
            }
          }
        },
        {
          "in": "path",
          "name": "project_id",
          "schema": {
            "type": "string"
          },
          "description": "Either the project's UUID or its workspace-scoped identifier (e.g. ``ENG``). Identifier matching is case-insensitive.",
          "required": true,
          "examples": {
            "uuid": {
              "value": "00000000-0000-0000-0000-000000000000",
              "summary": "UUID form"
            },
            "slug": {
              "value": "ENG",
              "summary": "Workspace-scoped identifier"
            }
          }
        },
        {
          "in": "path",
          "name": "slug",
          "schema": {
            "type": "string"
          },
          "description": "Workspace slug",
          "required": true,
          "examples": {
            "ExampleWorkspace": {
              "value": "my-workspace",
              "summary": "Example workspace",
              "description": "A typical workspace slug"
            }
          }
        }
      ],
      "responses": {
        "401": {
          "description": "Authentication credentials were not provided or are invalid."
        },
        "403": {
          "description": "Permission denied. User lacks required permissions."
        },
        "404": {
          "description": "Comment not found"
        },
        "200": {
          "content": {
            "application/json": {
              "schema": {
                "$ref": "#/components/schemas/IssueComment"
              },
              "examples": {
                "IssueComment": {
                  "value": {
                    "id": "550e8400-e29b-41d4-a716-446655440000",
                    "comment_html": "<p>This issue has been resolved by implementing OAuth 2.0 flow.</p>",
                    "labels": [],
                    "comment_json": {
                      "type": "doc",
                      "content": [
                        {
                          "type": "paragraph",
                          "content": [
                            {
                              "type": "text",
                              "text": "This issue has been resolved by implementing OAuth 2.0 flow."
                            }
                          ]
                        }
                      ]
                    },
                    "actor": {
                      "id": "550e8400-e29b-41d4-a716-446655440001",
                      "first_name": "John",
                      "last_name": "Doe",
                      "display_name": "John Doe",
                      "avatar": "https://example.com/avatar.jpg"
                    },
                    "created_at": "2024-01-01T10:30:00Z",
                    "updated_at": "2024-01-10T15:45:00Z"
                  }
                }
              }
            }
          },
          "description": "Work item comment updated successfully"
        },
        "400": {
          "description": "Invalid request data provided"
        },
        "409": {
          "description": "Resource with same external ID already exists"
        }
      }
    }
  },
  "/api/v1/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/issue-attachments/": {
    "get": {
      "operationId": "list_work_item_attachments",
      "tags": [
        "Work Item Attachments"
      ],
      "summary": null,
      "parameters": [
        {
          "in": "path",
          "name": "issue_id",
          "schema": {
            "type": "string",
            "format": "uuid"
          },
          "description": "Issue ID",
          "required": true,
          "examples": {
            "ExampleIssueID": {
              "value": "550e8400-e29b-41d4-a716-446655440000",
              "summary": "Example issue ID",
              "description": "A typical issue UUID"
            }
          }
        },
        {
          "in": "path",
          "name": "project_id",
          "schema": {
            "type": "string"
          },
          "description": "Either the project's UUID or its workspace-scoped identifier (e.g. ``ENG``). Identifier matching is case-insensitive.",
          "required": true,
          "examples": {
            "uuid": {
              "value": "00000000-0000-0000-0000-000000000000",
              "summary": "UUID form"
            },
            "slug": {
              "value": "ENG",
              "summary": "Workspace-scoped identifier"
            }
          }
        },
        {
          "in": "path",
          "name": "slug",
          "schema": {
            "type": "string"
          },
          "description": "Workspace slug",
          "required": true,
          "examples": {
            "ExampleWorkspace": {
              "value": "my-workspace",
              "summary": "Example workspace",
              "description": "A typical workspace slug"
            }
          }
        }
      ],
      "responses": {
        "401": {
          "description": "Authentication credentials were not provided or are invalid."
        },
        "403": {
          "description": "Permission denied. User lacks required permissions."
        },
        "404": {
          "description": "Attachment not found"
        },
        "200": {
          "content": {
            "application/json": {
              "schema": {
                "$ref": "#/components/schemas/IssueAttachment"
              },
              "examples": {
                "IssueAttachment": {
                  "value": {
                    "id": "550e8400-e29b-41d4-a716-446655440000",
                    "name": "screenshot.png",
                    "size": 1024000,
                    "asset_url": "https://s3.amazonaws.com/bucket/screenshot.png?signed-url",
                    "attributes": {
                      "name": "screenshot.png",
                      "type": "image/png",
                      "size": 1024000
                    },
                    "created_at": "2024-01-01T10:30:00Z",
                    "updated_at": "2024-01-10T15:45:00Z"
                  }
                }
              }
            }
          },
          "description": "Work item attachment"
        },
        "400": {
          "description": "Invalid request data provided"
        }
      }
    },
    "post": {
      "operationId": "create_work_item_attachment",
      "tags": [
        "Work Item Attachments"
      ],
      "summary": null,
      "parameters": [
        {
          "in": "path",
          "name": "issue_id",
          "schema": {
            "type": "string",
            "format": "uuid"
          },
          "description": "Issue ID",
          "required": true,
          "examples": {
            "ExampleIssueID": {
              "value": "550e8400-e29b-41d4-a716-446655440000",
              "summary": "Example issue ID",
              "description": "A typical issue UUID"
            }
          }
        },
        {
          "in": "path",
          "name": "project_id",
          "schema": {
            "type": "string"
          },
          "description": "Either the project's UUID or its workspace-scoped identifier (e.g. ``ENG``). Identifier matching is case-insensitive.",
          "required": true,
          "examples": {
            "uuid": {
              "value": "00000000-0000-0000-0000-000000000000",
              "summary": "UUID form"
            },
            "slug": {
              "value": "ENG",
              "summary": "Workspace-scoped identifier"
            }
          }
        },
        {
          "in": "path",
          "name": "slug",
          "schema": {
            "type": "string"
          },
          "description": "Workspace slug",
          "required": true,
          "examples": {
            "ExampleWorkspace": {
              "value": "my-workspace",
              "summary": "Example workspace",
              "description": "A typical workspace slug"
            }
          }
        }
      ],
      "responses": {
        "401": {
          "description": "Authentication credentials were not provided or are invalid."
        },
        "403": {
          "description": "Permission denied. User lacks required permissions."
        },
        "404": {
          "description": "Issue or Project or Workspace not found"
        },
        "200": {
          "description": "Presigned download URL generated successfully"
        },
        "400": {
          "description": "Validation error"
        }
      }
    }
  },
  "/api/v1/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/issue-attachments/{pk}/": {
    "delete": {
      "operationId": "delete_work_item_attachment",
      "tags": [
        "Work Item Attachments"
      ],
      "summary": null,
      "parameters": [
        {
          "in": "path",
          "name": "issue_id",
          "schema": {
            "type": "string",
            "format": "uuid"
          },
          "required": true
        },
        {
          "in": "path",
          "name": "pk",
          "schema": {
            "type": "string",
            "format": "uuid"
          },
          "description": "Attachment ID",
          "required": true,
          "examples": {
            "ExampleAttachmentID": {
              "value": "550e8400-e29b-41d4-a716-446655440000",
              "summary": "Example attachment ID",
              "description": "A typical attachment UUID"
            }
          }
        },
        {
          "in": "path",
          "name": "project_id",
          "schema": {
            "type": "string"
          },
          "description": "Either the project's UUID or its workspace-scoped identifier (e.g. ``ENG``). Identifier matching is case-insensitive.",
          "required": true,
          "examples": {
            "uuid": {
              "value": "00000000-0000-0000-0000-000000000000",
              "summary": "UUID form"
            },
            "slug": {
              "value": "ENG",
              "summary": "Workspace-scoped identifier"
            }
          }
        },
        {
          "in": "path",
          "name": "slug",
          "schema": {
            "type": "string"
          },
          "description": "Workspace slug",
          "required": true,
          "examples": {
            "ExampleWorkspace": {
              "value": "my-workspace",
              "summary": "Example workspace",
              "description": "A typical workspace slug"
            }
          }
        }
      ],
      "responses": {
        "401": {
          "description": "Authentication credentials were not provided or are invalid."
        },
        "403": {
          "description": "Permission denied. User lacks required permissions."
        },
        "404": {
          "description": "Attachment not found"
        },
        "204": {
          "description": "Work item attachment deleted successfully"
        }
      }
    },
    "get": {
      "operationId": "retrieve_work_item_attachment",
      "tags": [
        "Work Item Attachments"
      ],
      "summary": null,
      "parameters": [
        {
          "in": "path",
          "name": "issue_id",
          "schema": {
            "type": "string",
            "format": "uuid"
          },
          "required": true
        },
        {
          "in": "path",
          "name": "pk",
          "schema": {
            "type": "string",
            "format": "uuid"
          },
          "description": "Attachment ID",
          "required": true,
          "examples": {
            "ExampleAttachmentID": {
              "value": "550e8400-e29b-41d4-a716-446655440000",
              "summary": "Example attachment ID",
              "description": "A typical attachment UUID"
            }
          }
        },
        {
          "in": "path",
          "name": "project_id",
          "schema": {
            "type": "string"
          },
          "description": "Either the project's UUID or its workspace-scoped identifier (e.g. ``ENG``). Identifier matching is case-insensitive.",
          "required": true,
          "examples": {
            "uuid": {
              "value": "00000000-0000-0000-0000-000000000000",
              "summary": "UUID form"
            },
            "slug": {
              "value": "ENG",
              "summary": "Workspace-scoped identifier"
            }
          }
        },
        {
          "in": "path",
          "name": "slug",
          "schema": {
            "type": "string"
          },
          "description": "Workspace slug",
          "required": true,
          "examples": {
            "ExampleWorkspace": {
              "value": "my-workspace",
              "summary": "Example workspace",
              "description": "A typical workspace slug"
            }
          }
        }
      ],
      "responses": {
        "401": {
          "description": "Authentication credentials were not provided or are invalid."
        },
        "403": {
          "description": "Permission denied. User lacks required permissions."
        },
        "404": {
          "description": "Attachment not found"
        },
        "302": {
          "description": "Redirect to presigned download URL"
        },
        "400": {
          "content": {
            "application/json": {
              "schema": {
                "type": "object",
                "properties": {
                  "error": {
                    "type": "string",
                    "description": "Error message",
                    "example": "The asset is not uploaded."
                  },
                  "status": {
                    "type": "boolean",
                    "description": "Request status",
                    "example": false
                  }
                }
              },
              "examples": {
                "IssueAttachmentNotUploaded": {
                  "value": {
                    "error": "The asset is not uploaded.",
                    "status": false
                  },
                  "summary": "Issue Attachment Not Uploaded",
                  "description": "Error when trying to download an attachment that hasn't been uploaded yet"
                }
              }
            }
          },
          "description": "Asset not uploaded"
        }
      }
    },
    "patch": {
      "operationId": "upload_work_item_attachment",
      "tags": [
        "Work Item Attachments"
      ],
      "summary": null,
      "parameters": [
        {
          "in": "path",
          "name": "issue_id",
          "schema": {
            "type": "string",
            "format": "uuid"
          },
          "required": true
        },
        {
          "in": "path",
          "name": "pk",
          "schema": {
            "type": "string",
            "format": "uuid"
          },
          "description": "Attachment ID",
          "required": true,
          "examples": {
            "ExampleAttachmentID": {
              "value": "550e8400-e29b-41d4-a716-446655440000",
              "summary": "Example attachment ID",
              "description": "A typical attachment UUID"
            }
          }
        },
        {
          "in": "path",
          "name": "project_id",
          "schema": {
            "type": "string"
          },
          "description": "Either the project's UUID or its workspace-scoped identifier (e.g. ``ENG``). Identifier matching is case-insensitive.",
          "required": true,
          "examples": {
            "uuid": {
              "value": "00000000-0000-0000-0000-000000000000",
              "summary": "UUID form"
            },
            "slug": {
              "value": "ENG",
              "summary": "Workspace-scoped identifier"
            }
          }
        },
        {
          "in": "path",
          "name": "slug",
          "schema": {
            "type": "string"
          },
          "description": "Workspace slug",
          "required": true,
          "examples": {
            "ExampleWorkspace": {
              "value": "my-workspace",
              "summary": "Example workspace",
              "description": "A typical workspace slug"
            }
          }
        }
      ],
      "responses": {
        "401": {
          "description": "Authentication credentials were not provided or are invalid."
        },
        "403": {
          "description": "Permission denied. User lacks required permissions."
        },
        "404": {
          "description": "Attachment not found"
        },
        "204": {
          "description": "Work item attachment uploaded successfully"
        },
        "400": {
          "description": "Invalid request data provided"
        }
      }
    }
  },
  "/api/v1/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/links/": {
    "get": {
      "operationId": "list_work_item_links",
      "tags": [
        "Work Item Links"
      ],
      "summary": null,
      "parameters": [
        {
          "in": "query",
          "name": "cursor",
          "schema": {
            "type": "string"
          },
          "description": "Pagination cursor for getting next set of results",
          "examples": {
            "NextPageCursor": {
              "value": "20:1:0",
              "summary": "Next page cursor",
              "description": "Cursor format: 'page_size:page_number:offset'"
            }
          }
        },
        {
          "in": "query",
          "name": "expand",
          "schema": {
            "type": "string"
          },
          "description": "Comma-separated list of related fields to expand in response",
          "examples": {
            "ExpandAssignees": {
              "value": "assignees",
              "summary": "Expand assignees",
              "description": "Include full assignee details"
            },
            "MultipleExpansions": {
              "value": "assignees,labels,state",
              "summary": "Multiple expansions",
              "description": "Include details for multiple relations"
            }
          }
        },
        {
          "in": "query",
          "name": "fields",
          "schema": {
            "type": "string"
          },
          "description": "Comma-separated list of fields to include in response",
          "examples": {
            "BasicFields": {
              "value": "id,name,description",
              "summary": "Basic fields",
              "description": "Include only basic fields"
            },
            "WithRelations": {
              "value": "id,name,assignees,state",
              "summary": "With relations",
              "description": "Include fields with relationships"
            }
          }
        },
        {
          "in": "path",
          "name": "issue_id",
          "schema": {
            "type": "string",
            "format": "uuid"
          },
          "description": "Issue ID",
          "required": true,
          "examples": {
            "ExampleIssueID": {
              "value": "550e8400-e29b-41d4-a716-446655440000",
              "summary": "Example issue ID",
              "description": "A typical issue UUID"
            }
          }
        },
        {
          "in": "query",
          "name": "order_by",
          "schema": {
            "type": "string"
          },
          "description": "Field to order results by. Prefix with '-' for descending order",
          "examples": {
            "CreatedDateDescending": {
              "value": "-created_at",
              "summary": "Created date descending",
              "description": "Most recent items first"
            },
            "PriorityAscending": {
              "value": "priority",
              "summary": "Priority ascending",
              "description": "Order by priority (urgent, high, medium, low, none)"
            },
            "StateGroup": {
              "value": "state__group",
              "summary": "State group",
              "description": "Order by state group (backlog, unstarted, started, review, test, completed, cancelled)"
            },
            "AssigneeName": {
              "value": "assignees__first_name",
              "summary": "Assignee name",
              "description": "Order by assignee first name"
            }
          }
        },
        {
          "in": "query",
          "name": "per_page",
          "schema": {
            "type": "integer"
          },
          "description": "Number of results per page (default: 20, max: 100)",
          "examples": {
            "Default": {
              "value": 20
            },
            "Maximum": {
              "value": 100
            }
          }
        },
        {
          "in": "path",
          "name": "project_id",
          "schema": {
            "type": "string"
          },
          "description": "Either the project's UUID or its workspace-scoped identifier (e.g. ``ENG``). Identifier matching is case-insensitive.",
          "required": true,
          "examples": {
            "uuid": {
              "value": "00000000-0000-0000-0000-000000000000",
              "summary": "UUID form"
            },
            "slug": {
              "value": "ENG",
              "summary": "Workspace-scoped identifier"
            }
          }
        },
        {
          "in": "path",
          "name": "slug",
          "schema": {
            "type": "string"
          },
          "description": "Workspace slug",
          "required": true,
          "examples": {
            "ExampleWorkspace": {
              "value": "my-workspace",
              "summary": "Example workspace",
              "description": "A typical workspace slug"
            }
          }
        }
      ],
      "responses": {
        "401": {
          "description": "Authentication credentials were not provided or are invalid."
        },
        "403": {
          "description": "Permission denied. User lacks required permissions."
        },
        "404": {
          "description": "Issue not found"
        },
        "200": {
          "content": {
            "application/json": {
              "schema": {
                "$ref": "#/components/schemas/PaginatedIssueLinkResponse"
              },
              "examples": {
                "PaginatedWorkItemLinks": {
                  "value": {
                    "grouped_by": "state",
                    "sub_grouped_by": "priority",
                    "total_count": 150,
                    "next_cursor": "20:1:0",
                    "prev_cursor": "20:0:0",
                    "next_page_results": true,
                    "prev_page_results": false,
                    "count": 20,
                    "total_pages": 8,
                    "total_results": 150,
                    "extra_stats": null,
                    "results": [
                      {
                        "id": "550e8400-e29b-41d4-a716-446655440000",
                        "name": "Sample Item",
                        "created_at": "2024-01-15T12:00:00Z"
                      }
                    ]
                  },
                  "summary": "Paginated Work Item Links"
                }
              }
            }
          },
          "description": "Paginated list of work item links"
        },
        "400": {
          "description": "Invalid request data provided"
        }
      }
    },
    "post": {
      "operationId": "create_work_item_link",
      "tags": [
        "Work Item Links"
      ],
      "summary": null,
      "parameters": [
        {
          "in": "path",
          "name": "issue_id",
          "schema": {
            "type": "string",
            "format": "uuid"
          },
          "description": "Issue ID",
          "required": true,
          "examples": {
            "ExampleIssueID": {
              "value": "550e8400-e29b-41d4-a716-446655440000",
              "summary": "Example issue ID",
              "description": "A typical issue UUID"
            }
          }
        },
        {
          "in": "path",
          "name": "project_id",
          "schema": {
            "type": "string"
          },
          "description": "Either the project's UUID or its workspace-scoped identifier (e.g. ``ENG``). Identifier matching is case-insensitive.",
          "required": true,
          "examples": {
            "uuid": {
              "value": "00000000-0000-0000-0000-000000000000",
              "summary": "UUID form"
            },
            "slug": {
              "value": "ENG",
              "summary": "Workspace-scoped identifier"
            }
          }
        },
        {
          "in": "path",
          "name": "slug",
          "schema": {
            "type": "string"
          },
          "description": "Workspace slug",
          "required": true,
          "examples": {
            "ExampleWorkspace": {
              "value": "my-workspace",
              "summary": "Example workspace",
              "description": "A typical workspace slug"
            }
          }
        }
      ],
      "responses": {
        "401": {
          "description": "Authentication credentials were not provided or are invalid."
        },
        "403": {
          "description": "Permission denied. User lacks required permissions."
        },
        "404": {
          "description": "Issue not found"
        },
        "201": {
          "content": {
            "application/json": {
              "schema": {
                "$ref": "#/components/schemas/IssueLink"
              },
              "examples": {
                "IssueLink": {
                  "value": {
                    "id": "550e8400-e29b-41d4-a716-446655440000",
                    "url": "https://github.com/example/repo/pull/123",
                    "title": "Fix authentication bug",
                    "metadata": {
                      "title": "Fix authentication bug",
                      "description": "Pull request to fix authentication timeout issue",
                      "image": "https://github.com/example/repo/avatar.png"
                    },
                    "created_at": "2024-01-01T10:30:00Z",
                    "updated_at": "2024-01-10T15:45:00Z"
                  }
                }
              }
            }
          },
          "description": "Work item link created successfully"
        },
        "400": {
          "description": "Invalid request data provided"
        }
      }
    }
  },
  "/api/v1/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/links/{pk}/": {
    "delete": {
      "operationId": "delete_work_item_link",
      "tags": [
        "Work Item Links"
      ],
      "summary": null,
      "parameters": [
        {
          "in": "path",
          "name": "issue_id",
          "schema": {
            "type": "string",
            "format": "uuid"
          },
          "description": "Issue ID",
          "required": true,
          "examples": {
            "ExampleIssueID": {
              "value": "550e8400-e29b-41d4-a716-446655440000",
              "summary": "Example issue ID",
              "description": "A typical issue UUID"
            }
          }
        },
        {
          "in": "path",
          "name": "pk",
          "schema": {
            "type": "string",
            "format": "uuid"
          },
          "description": "Link ID",
          "required": true,
          "examples": {
            "ExampleLinkID": {
              "value": "550e8400-e29b-41d4-a716-446655440000",
              "summary": "Example link ID",
              "description": "A typical link UUID"
            }
          }
        },
        {
          "in": "path",
          "name": "project_id",
          "schema": {
            "type": "string"
          },
          "description": "Either the project's UUID or its workspace-scoped identifier (e.g. ``ENG``). Identifier matching is case-insensitive.",
          "required": true,
          "examples": {
            "uuid": {
              "value": "00000000-0000-0000-0000-000000000000",
              "summary": "UUID form"
            },
            "slug": {
              "value": "ENG",
              "summary": "Workspace-scoped identifier"
            }
          }
        },
        {
          "in": "path",
          "name": "slug",
          "schema": {
            "type": "string"
          },
          "description": "Workspace slug",
          "required": true,
          "examples": {
            "ExampleWorkspace": {
              "value": "my-workspace",
              "summary": "Example workspace",
              "description": "A typical workspace slug"
            }
          }
        }
      ],
      "responses": {
        "401": {
          "description": "Authentication credentials were not provided or are invalid."
        },
        "403": {
          "description": "Permission denied. User lacks required permissions."
        },
        "404": {
          "description": "Work item link not found"
        },
        "204": {
          "description": "Work item link deleted successfully"
        }
      }
    },
    "get": {
      "operationId": "retrieve_work_item_link",
      "tags": [
        "Work Item Links"
      ],
      "summary": null,
      "parameters": [
        {
          "in": "query",
          "name": "cursor",
          "schema": {
            "type": "string"
          },
          "description": "Pagination cursor for getting next set of results",
          "examples": {
            "NextPageCursor": {
              "value": "20:1:0",
              "summary": "Next page cursor",
              "description": "Cursor format: 'page_size:page_number:offset'"
            }
          }
        },
        {
          "in": "query",
          "name": "expand",
          "schema": {
            "type": "string"
          },
          "description": "Comma-separated list of related fields to expand in response",
          "examples": {
            "ExpandAssignees": {
              "value": "assignees",
              "summary": "Expand assignees",
              "description": "Include full assignee details"
            },
            "MultipleExpansions": {
              "value": "assignees,labels,state",
              "summary": "Multiple expansions",
              "description": "Include details for multiple relations"
            }
          }
        },
        {
          "in": "query",
          "name": "fields",
          "schema": {
            "type": "string"
          },
          "description": "Comma-separated list of fields to include in response",
          "examples": {
            "BasicFields": {
              "value": "id,name,description",
              "summary": "Basic fields",
              "description": "Include only basic fields"
            },
            "WithRelations": {
              "value": "id,name,assignees,state",
              "summary": "With relations",
              "description": "Include fields with relationships"
            }
          }
        },
        {
          "in": "path",
          "name": "issue_id",
          "schema": {
            "type": "string",
            "format": "uuid"
          },
          "description": "Issue ID",
          "required": true,
          "examples": {
            "ExampleIssueID": {
              "value": "550e8400-e29b-41d4-a716-446655440000",
              "summary": "Example issue ID",
              "description": "A typical issue UUID"
            }
          }
        },
        {
          "in": "query",
          "name": "per_page",
          "schema": {
            "type": "integer"
          },
          "description": "Number of results per page (default: 20, max: 100)",
          "examples": {
            "Default": {
              "value": 20
            },
            "Maximum": {
              "value": 100
            }
          }
        },
        {
          "in": "path",
          "name": "pk",
          "schema": {
            "type": "string",
            "format": "uuid"
          },
          "description": "Link ID",
          "required": true,
          "examples": {
            "ExampleLinkID": {
              "value": "550e8400-e29b-41d4-a716-446655440000",
              "summary": "Example link ID",
              "description": "A typical link UUID"
            }
          }
        },
        {
          "in": "path",
          "name": "project_id",
          "schema": {
            "type": "string"
          },
          "description": "Either the project's UUID or its workspace-scoped identifier (e.g. ``ENG``). Identifier matching is case-insensitive.",
          "required": true,
          "examples": {
            "uuid": {
              "value": "00000000-0000-0000-0000-000000000000",
              "summary": "UUID form"
            },
            "slug": {
              "value": "ENG",
              "summary": "Workspace-scoped identifier"
            }
          }
        },
        {
          "in": "path",
          "name": "slug",
          "schema": {
            "type": "string"
          },
          "description": "Workspace slug",
          "required": true,
          "examples": {
            "ExampleWorkspace": {
              "value": "my-workspace",
              "summary": "Example workspace",
              "description": "A typical workspace slug"
            }
          }
        }
      ],
      "responses": {
        "401": {
          "description": "Authentication credentials were not provided or are invalid."
        },
        "403": {
          "description": "Permission denied. User lacks required permissions."
        },
        "404": {
          "description": "Issue not found"
        },
        "200": {
          "content": {
            "application/json": {
              "schema": {
                "$ref": "#/components/schemas/PaginatedIssueLinkDetailResponse"
              },
              "examples": {
                "WorkItemLinkDetails": {
                  "value": {
                    "grouped_by": "state",
                    "sub_grouped_by": "priority",
                    "total_count": 150,
                    "next_cursor": "20:1:0",
                    "prev_cursor": "20:0:0",
                    "next_page_results": true,
                    "prev_page_results": false,
                    "count": 20,
                    "total_pages": 8,
                    "total_results": 150,
                    "extra_stats": null,
                    "results": [
                      {
                        "id": "550e8400-e29b-41d4-a716-446655440000",
                        "name": "Sample Item",
                        "created_at": "2024-01-15T12:00:00Z"
                      }
                    ]
                  },
                  "summary": "Work Item Link Details"
                }
              }
            }
          },
          "description": "Work item link details or paginated list"
        }
      }
    },
    "patch": {
      "operationId": "update_issue_link",
      "tags": [
        "Work Item Links"
      ],
      "summary": null,
      "parameters": [
        {
          "in": "path",
          "name": "issue_id",
          "schema": {
            "type": "string",
            "format": "uuid"
          },
          "description": "Issue ID",
          "required": true,
          "examples": {
            "ExampleIssueID": {
              "value": "550e8400-e29b-41d4-a716-446655440000",
              "summary": "Example issue ID",
              "description": "A typical issue UUID"
            }
          }
        },
        {
          "in": "path",
          "name": "pk",
          "schema": {
            "type": "string",
            "format": "uuid"
          },
          "description": "Link ID",
          "required": true,
          "examples": {
            "ExampleLinkID": {
              "value": "550e8400-e29b-41d4-a716-446655440000",
              "summary": "Example link ID",
              "description": "A typical link UUID"
            }
          }
        },
        {
          "in": "path",
          "name": "project_id",
          "schema": {
            "type": "string"
          },
          "description": "Either the project's UUID or its workspace-scoped identifier (e.g. ``ENG``). Identifier matching is case-insensitive.",
          "required": true,
          "examples": {
            "uuid": {
              "value": "00000000-0000-0000-0000-000000000000",
              "summary": "UUID form"
            },
            "slug": {
              "value": "ENG",
              "summary": "Workspace-scoped identifier"
            }
          }
        },
        {
          "in": "path",
          "name": "slug",
          "schema": {
            "type": "string"
          },
          "description": "Workspace slug",
          "required": true,
          "examples": {
            "ExampleWorkspace": {
              "value": "my-workspace",
              "summary": "Example workspace",
              "description": "A typical workspace slug"
            }
          }
        }
      ],
      "responses": {
        "401": {
          "description": "Authentication credentials were not provided or are invalid."
        },
        "403": {
          "description": "Permission denied. User lacks required permissions."
        },
        "404": {
          "description": "Link not found"
        },
        "200": {
          "content": {
            "application/json": {
              "schema": {
                "$ref": "#/components/schemas/IssueLink"
              },
              "examples": {
                "IssueLink": {
                  "value": {
                    "id": "550e8400-e29b-41d4-a716-446655440000",
                    "url": "https://github.com/example/repo/pull/123",
                    "title": "Fix authentication bug",
                    "metadata": {
                      "title": "Fix authentication bug",
                      "description": "Pull request to fix authentication timeout issue",
                      "image": "https://github.com/example/repo/avatar.png"
                    },
                    "created_at": "2024-01-01T10:30:00Z",
                    "updated_at": "2024-01-10T15:45:00Z"
                  }
                }
              }
            }
          },
          "description": "Issue link updated successfully"
        },
        "400": {
          "description": "Invalid request data provided"
        }
      }
    }
  },
  "/api/v1/workspaces/{slug}/projects/{project_id}/issues/{pk}/": {
    "delete": {
      "operationId": "delete_work_item",
      "tags": [
        "Work Items"
      ],
      "summary": "Delete work item",
      "parameters": [
        {
          "in": "path",
          "name": "pk",
          "schema": {
            "type": "string",
            "format": "uuid"
          },
          "required": true
        },
        {
          "in": "path",
          "name": "project_id",
          "schema": {
            "type": "string"
          },
          "description": "Either the project's UUID or its workspace-scoped identifier (e.g. ``ENG``). Identifier matching is case-insensitive.",
          "required": true,
          "examples": {
            "uuid": {
              "value": "00000000-0000-0000-0000-000000000000",
              "summary": "UUID form"
            },
            "slug": {
              "value": "ENG",
              "summary": "Workspace-scoped identifier"
            }
          }
        },
        {
          "in": "path",
          "name": "slug",
          "schema": {
            "type": "string"
          },
          "description": "Workspace slug",
          "required": true,
          "examples": {
            "ExampleWorkspace": {
              "value": "my-workspace",
              "summary": "Example workspace",
              "description": "A typical workspace slug"
            }
          }
        }
      ],
      "responses": {
        "401": {
          "description": "Authentication credentials were not provided or are invalid."
        },
        "403": {
          "description": "Only admin or creator can perform this action"
        },
        "404": {
          "description": "Work item not found"
        },
        "204": {
          "description": "Resource deleted successfully"
        }
      }
    },
    "get": {
      "operationId": "retrieve_work_item",
      "tags": [
        "Work Items"
      ],
      "summary": "Retrieve work item",
      "parameters": [
        {
          "in": "query",
          "name": "expand",
          "schema": {
            "type": "string"
          },
          "description": "Comma-separated list of related fields to expand in response",
          "examples": {
            "ExpandAssignees": {
              "value": "assignees",
              "summary": "Expand assignees",
              "description": "Include full assignee details"
            },
            "MultipleExpansions": {
              "value": "assignees,labels,state",
              "summary": "Multiple expansions",
              "description": "Include details for multiple relations"
            }
          }
        },
        {
          "in": "query",
          "name": "external_id",
          "schema": {
            "type": "string"
          },
          "description": "External system identifier for filtering or lookup",
          "examples": {
            "GitHubIssue": {
              "value": "1234567890",
              "summary": "GitHub Issue",
              "description": "GitHub issue number"
            }
          }
        },
        {
          "in": "query",
          "name": "external_source",
          "schema": {
            "type": "string"
          },
          "description": "External system source name for filtering or lookup",
          "examples": {
            "GitHub": {
              "value": "github",
              "description": "GitHub integration source"
            },
            "Jira": {
              "value": "jira",
              "description": "Jira integration source"
            }
          }
        },
        {
          "in": "query",
          "name": "fields",
          "schema": {
            "type": "string"
          },
          "description": "Comma-separated list of fields to include in response",
          "examples": {
            "BasicFields": {
              "value": "id,name,description",
              "summary": "Basic fields",
              "description": "Include only basic fields"
            },
            "WithRelations": {
              "value": "id,name,assignees,state",
              "summary": "With relations",
              "description": "Include fields with relationships"
            }
          }
        },
        {
          "in": "query",
          "name": "order_by",
          "schema": {
            "type": "string"
          },
          "description": "Field to order results by. Prefix with '-' for descending order",
          "examples": {
            "CreatedDateDescending": {
              "value": "-created_at",
              "summary": "Created date descending",
              "description": "Most recent items first"
            },
            "PriorityAscending": {
              "value": "priority",
              "summary": "Priority ascending",
              "description": "Order by priority (urgent, high, medium, low, none)"
            },
            "StateGroup": {
              "value": "state__group",
              "summary": "State group",
              "description": "Order by state group (backlog, unstarted, started, review, test, completed, cancelled)"
            },
            "AssigneeName": {
              "value": "assignees__first_name",
              "summary": "Assignee name",
              "description": "Order by assignee first name"
            }
          }
        },
        {
          "in": "path",
          "name": "pk",
          "schema": {
            "type": "string",
            "format": "uuid"
          },
          "required": true
        },
        {
          "in": "path",
          "name": "project_id",
          "schema": {
            "type": "string"
          },
          "description": "Either the project's UUID or its workspace-scoped identifier (e.g. ``ENG``). Identifier matching is case-insensitive.",
          "required": true,
          "examples": {
            "uuid": {
              "value": "00000000-0000-0000-0000-000000000000",
              "summary": "UUID form"
            },
            "slug": {
              "value": "ENG",
              "summary": "Workspace-scoped identifier"
            }
          }
        },
        {
          "in": "path",
          "name": "slug",
          "schema": {
            "type": "string"
          },
          "description": "Workspace slug",
          "required": true,
          "examples": {
            "ExampleWorkspace": {
              "value": "my-workspace",
              "summary": "Example workspace",
              "description": "A typical workspace slug"
            }
          }
        }
      ],
      "responses": {
        "401": {
          "description": "Authentication credentials were not provided or are invalid."
        },
        "403": {
          "description": "Permission denied. User lacks required permissions."
        },
        "404": {
          "description": "Work item not found"
        },
        "200": {
          "content": {
            "application/json": {
              "schema": {
                "$ref": "#/components/schemas/Issue"
              },
              "examples": {
                "Issue": {
                  "value": {
                    "id": "550e8400-e29b-41d4-a716-446655440000",
                    "name": "Implement user authentication",
                    "description": "Add OAuth 2.0 authentication flow",
                    "sequence_id": 1,
                    "priority": "high",
                    "assignees": [
                      "550e8400-e29b-41d4-a716-446655440001"
                    ],
                    "labels": [
                      "550e8400-e29b-41d4-a716-446655440002"
                    ],
                    "created_at": "2024-01-15T10:30:00Z",
                    "updated_at": "2024-01-15T10:30:00Z"
                  }
                }
              }
            }
          },
          "description": "List of issues or issue details"
        },
        "400": {
          "description": "Invalid request data provided"
        }
      }
    },
    "patch": {
      "operationId": "update_work_item",
      "tags": [
        "Work Items"
      ],
      "summary": "Partially update work item",
      "parameters": [
        {
          "in": "path",
          "name": "pk",
          "schema": {
            "type": "string",
            "format": "uuid"
          },
          "required": true
        },
        {
          "in": "path",
          "name": "project_id",
          "schema": {
            "type": "string"
          },
          "description": "Either the project's UUID or its workspace-scoped identifier (e.g. ``ENG``). Identifier matching is case-insensitive.",
          "required": true,
          "examples": {
            "uuid": {
              "value": "00000000-0000-0000-0000-000000000000",
              "summary": "UUID form"
            },
            "slug": {
              "value": "ENG",
              "summary": "Workspace-scoped identifier"
            }
          }
        },
        {
          "in": "path",
          "name": "slug",
          "schema": {
            "type": "string"
          },
          "description": "Workspace slug",
          "required": true,
          "examples": {
            "ExampleWorkspace": {
              "value": "my-workspace",
              "summary": "Example workspace",
              "description": "A typical workspace slug"
            }
          }
        }
      ],
      "responses": {
        "401": {
          "description": "Authentication credentials were not provided or are invalid."
        },
        "403": {
          "description": "Permission denied. User lacks required permissions."
        },
        "404": {
          "description": "Work item not found"
        },
        "200": {
          "content": {
            "application/json": {
              "schema": {
                "$ref": "#/components/schemas/Issue"
              },
              "examples": {
                "Issue": {
                  "value": {
                    "id": "550e8400-e29b-41d4-a716-446655440000",
                    "name": "Implement user authentication",
                    "description": "Add OAuth 2.0 authentication flow",
                    "sequence_id": 1,
                    "priority": "high",
                    "assignees": [
                      "550e8400-e29b-41d4-a716-446655440001"
                    ],
                    "labels": [
                      "550e8400-e29b-41d4-a716-446655440002"
                    ],
                    "created_at": "2024-01-15T10:30:00Z",
                    "updated_at": "2024-01-15T10:30:00Z"
                  }
                }
              }
            }
          },
          "description": "Work Item patched successfully"
        },
        "400": {
          "description": "Invalid request data provided"
        },
        "409": {
          "description": "Resource with same external ID already exists"
        }
      }
    }
  },
  "/api/v1/workspaces/{slug}/projects/{project_id}/labels/": {
    "get": {
      "operationId": "list_labels",
      "tags": [
        "Labels"
      ],
      "summary": null,
      "parameters": [
        {
          "in": "query",
          "name": "cursor",
          "schema": {
            "type": "string"
          },
          "description": "Pagination cursor for getting next set of results",
          "examples": {
            "NextPageCursor": {
              "value": "20:1:0",
              "summary": "Next page cursor",
              "description": "Cursor format: 'page_size:page_number:offset'"
            }
          }
        },
        {
          "in": "query",
          "name": "expand",
          "schema": {
            "type": "string"
          },
          "description": "Comma-separated list of related fields to expand in response",
          "examples": {
            "ExpandAssignees": {
              "value": "assignees",
              "summary": "Expand assignees",
              "description": "Include full assignee details"
            },
            "MultipleExpansions": {
              "value": "assignees,labels,state",
              "summary": "Multiple expansions",
              "description": "Include details for multiple relations"
            }
          }
        },
        {
          "in": "query",
          "name": "fields",
          "schema": {
            "type": "string"
          },
          "description": "Comma-separated list of fields to include in response",
          "examples": {
            "BasicFields": {
              "value": "id,name,description",
              "summary": "Basic fields",
              "description": "Include only basic fields"
            },
            "WithRelations": {
              "value": "id,name,assignees,state",
              "summary": "With relations",
              "description": "Include fields with relationships"
            }
          }
        },
        {
          "in": "query",
          "name": "order_by",
          "schema": {
            "type": "string"
          },
          "description": "Field to order results by. Prefix with '-' for descending order",
          "examples": {
            "CreatedDateDescending": {
              "value": "-created_at",
              "summary": "Created date descending",
              "description": "Most recent items first"
            },
            "PriorityAscending": {
              "value": "priority",
              "summary": "Priority ascending",
              "description": "Order by priority (urgent, high, medium, low, none)"
            },
            "StateGroup": {
              "value": "state__group",
              "summary": "State group",
              "description": "Order by state group (backlog, unstarted, started, review, test, completed, cancelled)"
            },
            "AssigneeName": {
              "value": "assignees__first_name",
              "summary": "Assignee name",
              "description": "Order by assignee first name"
            }
          }
        },
        {
          "in": "query",
          "name": "per_page",
          "schema": {
            "type": "integer"
          },
          "description": "Number of results per page (default: 20, max: 100)",
          "examples": {
            "Default": {
              "value": 20
            },
            "Maximum": {
              "value": 100
            }
          }
        },
        {
          "in": "path",
          "name": "project_id",
          "schema": {
            "type": "string"
          },
          "description": "Either the project's UUID or its workspace-scoped identifier (e.g. ``ENG``). Identifier matching is case-insensitive.",
          "required": true,
          "examples": {
            "uuid": {
              "value": "00000000-0000-0000-0000-000000000000",
              "summary": "UUID form"
            },
            "slug": {
              "value": "ENG",
              "summary": "Workspace-scoped identifier"
            }
          }
        },
        {
          "in": "path",
          "name": "slug",
          "schema": {
            "type": "string"
          },
          "description": "Workspace slug",
          "required": true,
          "examples": {
            "ExampleWorkspace": {
              "value": "my-workspace",
              "summary": "Example workspace",
              "description": "A typical workspace slug"
            }
          }
        }
      ],
      "responses": {
        "401": {
          "description": "Authentication credentials were not provided or are invalid."
        },
        "403": {
          "description": "Permission denied. User lacks required permissions."
        },
        "404": {
          "description": "Project not found"
        },
        "200": {
          "content": {
            "application/json": {
              "schema": {
                "$ref": "#/components/schemas/PaginatedLabelResponse"
              },
              "examples": {
                "PaginatedLabels": {
                  "value": {
                    "grouped_by": "state",
                    "sub_grouped_by": "priority",
                    "total_count": 150,
                    "next_cursor": "20:1:0",
                    "prev_cursor": "20:0:0",
                    "next_page_results": true,
                    "prev_page_results": false,
                    "count": 20,
                    "total_pages": 8,
                    "total_results": 150,
                    "extra_stats": null,
                    "results": [
                      {
                        "id": "550e8400-e29b-41d4-a716-446655440000",
                        "name": "bug",
                        "color": "#ff4444",
                        "description": "Issues that represent bugs in the system"
                      }
                    ]
                  },
                  "summary": "Paginated Labels"
                }
              }
            }
          },
          "description": "Paginated list of labels"
        },
        "400": {
          "description": "Invalid request data provided"
        }
      }
    },
    "post": {
      "operationId": "create_label",
      "tags": [
        "Labels"
      ],
      "summary": null,
      "parameters": [
        {
          "in": "path",
          "name": "project_id",
          "schema": {
            "type": "string"
          },
          "description": "Either the project's UUID or its workspace-scoped identifier (e.g. ``ENG``). Identifier matching is case-insensitive.",
          "required": true,
          "examples": {
            "uuid": {
              "value": "00000000-0000-0000-0000-000000000000",
              "summary": "UUID form"
            },
            "slug": {
              "value": "ENG",
              "summary": "Workspace-scoped identifier"
            }
          }
        },
        {
          "in": "path",
          "name": "slug",
          "schema": {
            "type": "string"
          },
          "description": "Workspace slug",
          "required": true,
          "examples": {
            "ExampleWorkspace": {
              "value": "my-workspace",
              "summary": "Example workspace",
              "description": "A typical workspace slug"
            }
          }
        }
      ],
      "responses": {
        "401": {
          "description": "Authentication credentials were not provided or are invalid."
        },
        "403": {
          "description": "Permission denied. User lacks required permissions."
        },
        "404": {
          "description": "The requested resource was not found."
        },
        "201": {
          "content": {
            "application/json": {
              "schema": {
                "$ref": "#/components/schemas/Label"
              },
              "examples": {
                "Label": {
                  "value": {
                    "id": "550e8400-e29b-41d4-a716-446655440000",
                    "name": "bug",
                    "color": "#ff4444",
                    "description": "Issues that represent bugs in the system",
                    "created_at": "2024-01-01T10:30:00Z",
                    "updated_at": "2024-01-10T15:45:00Z"
                  }
                }
              }
            }
          },
          "description": "Label created successfully"
        },
        "400": {
          "description": "Invalid request data provided"
        },
        "409": {
          "description": "Label with the same name already exists"
        }
      }
    }
  },
  "/api/v1/workspaces/{slug}/projects/{project_id}/labels/{pk}/": {
    "delete": {
      "operationId": "delete_label",
      "tags": [
        "Labels"
      ],
      "summary": null,
      "parameters": [
        {
          "in": "path",
          "name": "pk",
          "schema": {
            "type": "string",
            "format": "uuid"
          },
          "description": "Label ID",
          "required": true,
          "examples": {
            "ExampleLabelID": {
              "value": "550e8400-e29b-41d4-a716-446655440000",
              "summary": "Example label ID",
              "description": "A typical label UUID"
            }
          }
        },
        {
          "in": "path",
          "name": "project_id",
          "schema": {
            "type": "string"
          },
          "description": "Either the project's UUID or its workspace-scoped identifier (e.g. ``ENG``). Identifier matching is case-insensitive.",
          "required": true,
          "examples": {
            "uuid": {
              "value": "00000000-0000-0000-0000-000000000000",
              "summary": "UUID form"
            },
            "slug": {
              "value": "ENG",
              "summary": "Workspace-scoped identifier"
            }
          }
        },
        {
          "in": "path",
          "name": "slug",
          "schema": {
            "type": "string"
          },
          "description": "Workspace slug",
          "required": true,
          "examples": {
            "ExampleWorkspace": {
              "value": "my-workspace",
              "summary": "Example workspace",
              "description": "A typical workspace slug"
            }
          }
        }
      ],
      "responses": {
        "401": {
          "description": "Authentication credentials were not provided or are invalid."
        },
        "403": {
          "description": "Permission denied. User lacks required permissions."
        },
        "404": {
          "description": "Label not found"
        },
        "204": {
          "description": "Resource deleted successfully"
        }
      }
    },
    "get": {
      "operationId": "get_labels",
      "tags": [
        "Labels"
      ],
      "summary": null,
      "parameters": [
        {
          "in": "path",
          "name": "pk",
          "schema": {
            "type": "string",
            "format": "uuid"
          },
          "description": "Label ID",
          "required": true,
          "examples": {
            "ExampleLabelID": {
              "value": "550e8400-e29b-41d4-a716-446655440000",
              "summary": "Example label ID",
              "description": "A typical label UUID"
            }
          }
        },
        {
          "in": "path",
          "name": "project_id",
          "schema": {
            "type": "string"
          },
          "description": "Either the project's UUID or its workspace-scoped identifier (e.g. ``ENG``). Identifier matching is case-insensitive.",
          "required": true,
          "examples": {
            "uuid": {
              "value": "00000000-0000-0000-0000-000000000000",
              "summary": "UUID form"
            },
            "slug": {
              "value": "ENG",
              "summary": "Workspace-scoped identifier"
            }
          }
        },
        {
          "in": "path",
          "name": "slug",
          "schema": {
            "type": "string"
          },
          "description": "Workspace slug",
          "required": true,
          "examples": {
            "ExampleWorkspace": {
              "value": "my-workspace",
              "summary": "Example workspace",
              "description": "A typical workspace slug"
            }
          }
        }
      ],
      "responses": {
        "401": {
          "description": "Authentication credentials were not provided or are invalid."
        },
        "403": {
          "description": "Permission denied. User lacks required permissions."
        },
        "404": {
          "description": "Label not found"
        },
        "200": {
          "content": {
            "application/json": {
              "schema": {
                "$ref": "#/components/schemas/Label"
              },
              "examples": {
                "Label": {
                  "value": {
                    "id": "550e8400-e29b-41d4-a716-446655440000",
                    "name": "bug",
                    "color": "#ff4444",
                    "description": "Issues that represent bugs in the system",
                    "created_at": "2024-01-01T10:30:00Z",
                    "updated_at": "2024-01-10T15:45:00Z"
                  }
                }
              }
            }
          },
          "description": "Labels"
        }
      }
    },
    "patch": {
      "operationId": "update_label",
      "tags": [
        "Labels"
      ],
      "summary": null,
      "parameters": [
        {
          "in": "path",
          "name": "pk",
          "schema": {
            "type": "string",
            "format": "uuid"
          },
          "description": "Label ID",
          "required": true,
          "examples": {
            "ExampleLabelID": {
              "value": "550e8400-e29b-41d4-a716-446655440000",
              "summary": "Example label ID",
              "description": "A typical label UUID"
            }
          }
        },
        {
          "in": "path",
          "name": "project_id",
          "schema": {
            "type": "string"
          },
          "description": "Either the project's UUID or its workspace-scoped identifier (e.g. ``ENG``). Identifier matching is case-insensitive.",
          "required": true,
          "examples": {
            "uuid": {
              "value": "00000000-0000-0000-0000-000000000000",
              "summary": "UUID form"
            },
            "slug": {
              "value": "ENG",
              "summary": "Workspace-scoped identifier"
            }
          }
        },
        {
          "in": "path",
          "name": "slug",
          "schema": {
            "type": "string"
          },
          "description": "Workspace slug",
          "required": true,
          "examples": {
            "ExampleWorkspace": {
              "value": "my-workspace",
              "summary": "Example workspace",
              "description": "A typical workspace slug"
            }
          }
        }
      ],
      "responses": {
        "401": {
          "description": "Authentication credentials were not provided or are invalid."
        },
        "403": {
          "description": "Permission denied. User lacks required permissions."
        },
        "404": {
          "description": "Label not found"
        },
        "200": {
          "content": {
            "application/json": {
              "schema": {
                "$ref": "#/components/schemas/Label"
              },
              "examples": {
                "Label": {
                  "value": {
                    "id": "550e8400-e29b-41d4-a716-446655440000",
                    "name": "bug",
                    "color": "#ff4444",
                    "description": "Issues that represent bugs in the system",
                    "created_at": "2024-01-01T10:30:00Z",
                    "updated_at": "2024-01-10T15:45:00Z"
                  }
                }
              }
            }
          },
          "description": "Label updated successfully"
        },
        "400": {
          "description": "Invalid request data provided"
        },
        "409": {
          "description": "Resource with same external ID already exists"
        }
      }
    }
  },
  "/api/v1/workspaces/{slug}/projects/{project_id}/members/": {
    "get": {
      "operationId": "get_project_members",
      "tags": [
        "Members"
      ],
      "summary": "List project members",
      "parameters": [
        {
          "in": "path",
          "name": "project_id",
          "schema": {
            "type": "string"
          },
          "description": "Either the project's UUID or its workspace-scoped identifier (e.g. ``ENG``). Identifier matching is case-insensitive.",
          "required": true,
          "examples": {
            "uuid": {
              "value": "00000000-0000-0000-0000-000000000000",
              "summary": "UUID form"
            },
            "slug": {
              "value": "ENG",
              "summary": "Workspace-scoped identifier"
            }
          }
        },
        {
          "in": "path",
          "name": "slug",
          "schema": {
            "type": "string"
          },
          "description": "Workspace slug",
          "required": true,
          "examples": {
            "ExampleWorkspace": {
              "value": "my-workspace",
              "summary": "Example workspace",
              "description": "A typical workspace slug"
            }
          }
        }
      ],
      "responses": {
        "200": {
          "content": {
            "application/json": {
              "schema": {
                "$ref": "#/components/schemas/UserLite"
              },
              "examples": {
                "ProjectMembers": {
                  "value": [
                    {
                      "id": "550e8400-e29b-41d4-a716-446655440000",
                      "first_name": "John",
                      "last_name": "Doe",
                      "display_name": "John Doe",
                      "email": "john.doe@example.com",
                      "avatar": "https://example.com/avatar.jpg"
                    },
                    {
                      "id": "550e8400-e29b-41d4-a716-446655440001",
                      "first_name": "Jane",
                      "last_name": "Smith",
                      "display_name": "Jane Smith",
                      "email": "jane.smith@example.com",
                      "avatar": "https://example.com/avatar2.jpg"
                    }
                  ]
                }
              }
            }
          },
          "description": "List of project members with their roles"
        },
        "401": {
          "description": "Authentication credentials were not provided or are invalid."
        },
        "403": {
          "description": "Permission denied. User lacks required permissions."
        },
        "404": {
          "description": "Project not found"
        }
      }
    },
    "post": {
      "operationId": "create_project_member",
      "tags": [
        "Members"
      ],
      "summary": "Create project member",
      "parameters": [
        {
          "in": "path",
          "name": "project_id",
          "schema": {
            "type": "string"
          },
          "description": "Either the project's UUID or its workspace-scoped identifier (e.g. ``ENG``). Identifier matching is case-insensitive.",
          "required": true,
          "examples": {
            "uuid": {
              "value": "00000000-0000-0000-0000-000000000000",
              "summary": "UUID form"
            },
            "slug": {
              "value": "ENG",
              "summary": "Workspace-scoped identifier"
            }
          }
        },
        {
          "in": "path",
          "name": "slug",
          "schema": {
            "type": "string"
          },
          "description": "Workspace slug",
          "required": true,
          "examples": {
            "ExampleWorkspace": {
              "value": "my-workspace",
              "summary": "Example workspace",
              "description": "A typical workspace slug"
            }
          }
        }
      ],
      "responses": {
        "201": {
          "content": {
            "application/json": {
              "schema": {
                "$ref": "#/components/schemas/ProjectMember"
              }
            }
          },
          "description": "Project member created"
        }
      }
    }
  },
  "/api/v1/workspaces/{slug}/projects/{project_id}/members/{pk}/": {
    "delete": {
      "operationId": "delete_project_member",
      "tags": [
        "Members"
      ],
      "summary": "Delete project member",
      "parameters": [
        {
          "in": "path",
          "name": "pk",
          "schema": {
            "type": "string",
            "format": "uuid"
          },
          "required": true
        },
        {
          "in": "path",
          "name": "project_id",
          "schema": {
            "type": "string"
          },
          "description": "Either the project's UUID or its workspace-scoped identifier (e.g. ``ENG``). Identifier matching is case-insensitive.",
          "required": true,
          "examples": {
            "uuid": {
              "value": "00000000-0000-0000-0000-000000000000",
              "summary": "UUID form"
            },
            "slug": {
              "value": "ENG",
              "summary": "Workspace-scoped identifier"
            }
          }
        },
        {
          "in": "path",
          "name": "slug",
          "schema": {
            "type": "string"
          },
          "description": "Workspace slug",
          "required": true,
          "examples": {
            "ExampleWorkspace": {
              "value": "my-workspace",
              "summary": "Example workspace",
              "description": "A typical workspace slug"
            }
          }
        }
      ],
      "responses": {
        "204": {
          "description": "Project member deleted"
        }
      }
    },
    "get": {
      "operationId": "get_project_member",
      "tags": [
        "Members"
      ],
      "summary": "Get project member",
      "parameters": [
        {
          "in": "path",
          "name": "pk",
          "schema": {
            "type": "string",
            "format": "uuid"
          },
          "required": true
        },
        {
          "in": "path",
          "name": "project_id",
          "schema": {
            "type": "string"
          },
          "description": "Either the project's UUID or its workspace-scoped identifier (e.g. ``ENG``). Identifier matching is case-insensitive.",
          "required": true,
          "examples": {
            "uuid": {
              "value": "00000000-0000-0000-0000-000000000000",
              "summary": "UUID form"
            },
            "slug": {
              "value": "ENG",
              "summary": "Workspace-scoped identifier"
            }
          }
        },
        {
          "in": "path",
          "name": "slug",
          "schema": {
            "type": "string"
          },
          "description": "Workspace slug",
          "required": true,
          "examples": {
            "ExampleWorkspace": {
              "value": "my-workspace",
              "summary": "Example workspace",
              "description": "A typical workspace slug"
            }
          }
        }
      ],
      "responses": {
        "200": {
          "content": {
            "application/json": {
              "schema": {
                "$ref": "#/components/schemas/ProjectMember"
              }
            }
          },
          "description": "Project member"
        },
        "401": {
          "description": "Authentication credentials were not provided or are invalid."
        },
        "403": {
          "description": "Permission denied. User lacks required permissions."
        },
        "404": {
          "description": "Project not found"
        }
      }
    },
    "patch": {
      "operationId": "update_project_member",
      "tags": [
        "Members"
      ],
      "summary": "Update project member",
      "parameters": [
        {
          "in": "path",
          "name": "pk",
          "schema": {
            "type": "string",
            "format": "uuid"
          },
          "required": true
        },
        {
          "in": "path",
          "name": "project_id",
          "schema": {
            "type": "string"
          },
          "description": "Either the project's UUID or its workspace-scoped identifier (e.g. ``ENG``). Identifier matching is case-insensitive.",
          "required": true,
          "examples": {
            "uuid": {
              "value": "00000000-0000-0000-0000-000000000000",
              "summary": "UUID form"
            },
            "slug": {
              "value": "ENG",
              "summary": "Workspace-scoped identifier"
            }
          }
        },
        {
          "in": "path",
          "name": "slug",
          "schema": {
            "type": "string"
          },
          "description": "Workspace slug",
          "required": true,
          "examples": {
            "ExampleWorkspace": {
              "value": "my-workspace",
              "summary": "Example workspace",
              "description": "A typical workspace slug"
            }
          }
        }
      ],
      "responses": {
        "200": {
          "content": {
            "application/json": {
              "schema": {
                "$ref": "#/components/schemas/ProjectMember"
              }
            }
          },
          "description": "Project member updated"
        }
      }
    }
  },
  "/api/v1/workspaces/{slug}/projects/{project_id}/modules/": {
    "get": {
      "operationId": "list_modules",
      "tags": [
        "Modules"
      ],
      "summary": "List modules",
      "parameters": [
        {
          "in": "query",
          "name": "cursor",
          "schema": {
            "type": "string"
          },
          "description": "Pagination cursor for getting next set of results",
          "examples": {
            "NextPageCursor": {
              "value": "20:1:0",
              "summary": "Next page cursor",
              "description": "Cursor format: 'page_size:page_number:offset'"
            }
          }
        },
        {
          "in": "query",
          "name": "expand",
          "schema": {
            "type": "string"
          },
          "description": "Comma-separated list of related fields to expand in response",
          "examples": {
            "ExpandAssignees": {
              "value": "assignees",
              "summary": "Expand assignees",
              "description": "Include full assignee details"
            },
            "MultipleExpansions": {
              "value": "assignees,labels,state",
              "summary": "Multiple expansions",
              "description": "Include details for multiple relations"
            }
          }
        },
        {
          "in": "query",
          "name": "fields",
          "schema": {
            "type": "string"
          },
          "description": "Comma-separated list of fields to include in response",
          "examples": {
            "BasicFields": {
              "value": "id,name,description",
              "summary": "Basic fields",
              "description": "Include only basic fields"
            },
            "WithRelations": {
              "value": "id,name,assignees,state",
              "summary": "With relations",
              "description": "Include fields with relationships"
            }
          }
        },
        {
          "in": "query",
          "name": "order_by",
          "schema": {
            "type": "string"
          },
          "description": "Field to order results by. Prefix with '-' for descending order",
          "examples": {
            "CreatedDateDescending": {
              "value": "-created_at",
              "summary": "Created date descending",
              "description": "Most recent items first"
            },
            "PriorityAscending": {
              "value": "priority",
              "summary": "Priority ascending",
              "description": "Order by priority (urgent, high, medium, low, none)"
            },
            "StateGroup": {
              "value": "state__group",
              "summary": "State group",
              "description": "Order by state group (backlog, unstarted, started, review, test, completed, cancelled)"
            },
            "AssigneeName": {
              "value": "assignees__first_name",
              "summary": "Assignee name",
              "description": "Order by assignee first name"
            }
          }
        },
        {
          "in": "query",
          "name": "per_page",
          "schema": {
            "type": "integer"
          },
          "description": "Number of results per page (default: 20, max: 100)",
          "examples": {
            "Default": {
              "value": 20
            },
            "Maximum": {
              "value": 100
            }
          }
        },
        {
          "in": "path",
          "name": "project_id",
          "schema": {
            "type": "string"
          },
          "description": "Either the project's UUID or its workspace-scoped identifier (e.g. ``ENG``). Identifier matching is case-insensitive.",
          "required": true,
          "examples": {
            "uuid": {
              "value": "00000000-0000-0000-0000-000000000000",
              "summary": "UUID form"
            },
            "slug": {
              "value": "ENG",
              "summary": "Workspace-scoped identifier"
            }
          }
        },
        {
          "in": "path",
          "name": "slug",
          "schema": {
            "type": "string"
          },
          "description": "Workspace slug",
          "required": true,
          "examples": {
            "ExampleWorkspace": {
              "value": "my-workspace",
              "summary": "Example workspace",
              "description": "A typical workspace slug"
            }
          }
        }
      ],
      "responses": {
        "401": {
          "description": "Authentication credentials were not provided or are invalid."
        },
        "403": {
          "description": "Permission denied. User lacks required permissions."
        },
        "404": {
          "description": "Module not found"
        },
        "200": {
          "content": {
            "application/json": {
              "schema": {
                "$ref": "#/components/schemas/PaginatedModuleResponse"
              },
              "examples": {
                "PaginatedModules": {
                  "value": {
                    "grouped_by": "state",
                    "sub_grouped_by": "priority",
                    "total_count": 150,
                    "next_cursor": "20:1:0",
                    "prev_cursor": "20:0:0",
                    "next_page_results": true,
                    "prev_page_results": false,
                    "count": 20,
                    "total_pages": 8,
                    "total_results": 150,
                    "extra_stats": null,
                    "results": [
                      {
                        "id": "550e8400-e29b-41d4-a716-446655440000",
                        "name": "Authentication Module",
                        "description": "User authentication and authorization features",
                        "start_date": "2024-01-01",
                        "target_date": "2024-02-15",
                        "status": "in_progress"
                      }
                    ]
                  },
                  "summary": "Paginated Modules"
                }
              }
            }
          },
          "description": "Paginated list of modules"
        }
      }
    },
    "post": {
      "operationId": "create_module",
      "tags": [
        "Modules"
      ],
      "summary": "Create module",
      "parameters": [
        {
          "in": "path",
          "name": "project_id",
          "schema": {
            "type": "string"
          },
          "description": "Either the project's UUID or its workspace-scoped identifier (e.g. ``ENG``). Identifier matching is case-insensitive.",
          "required": true,
          "examples": {
            "uuid": {
              "value": "00000000-0000-0000-0000-000000000000",
              "summary": "UUID form"
            },
            "slug": {
              "value": "ENG",
              "summary": "Workspace-scoped identifier"
            }
          }
        },
        {
          "in": "path",
          "name": "slug",
          "schema": {
            "type": "string"
          },
          "description": "Workspace slug",
          "required": true,
          "examples": {
            "ExampleWorkspace": {
              "value": "my-workspace",
              "summary": "Example workspace",
              "description": "A typical workspace slug"
            }
          }
        }
      ],
      "responses": {
        "401": {
          "description": "Authentication credentials were not provided or are invalid."
        },
        "403": {
          "description": "Permission denied. User lacks required permissions."
        },
        "404": {
          "description": "Project not found"
        },
        "201": {
          "content": {
            "application/json": {
              "schema": {
                "$ref": "#/components/schemas/Module"
              },
              "examples": {
                "Module": {
                  "value": {
                    "id": "550e8400-e29b-41d4-a716-446655440000",
                    "name": "Authentication Module",
                    "description": "User authentication and authorization features",
                    "start_date": "2024-01-01",
                    "target_date": "2024-02-15",
                    "status": "in-progress",
                    "total_issues": 12,
                    "completed_issues": 5,
                    "cancelled_issues": 0,
                    "started_issues": 4,
                    "unstarted_issues": 3,
                    "backlog_issues": 0,
                    "created_at": "2024-01-01T10:30:00Z",
                    "updated_at": "2024-01-10T15:45:00Z"
                  }
                }
              }
            }
          },
          "description": "Module created"
        },
        "400": {
          "description": "Invalid request data provided"
        },
        "409": {
          "description": "Resource with same external ID already exists"
        }
      }
    }
  },
  "/api/v1/workspaces/{slug}/projects/{project_id}/modules/{module_id}/module-issues/": {
    "get": {
      "operationId": "list_module_work_items",
      "tags": [
        "Modules"
      ],
      "summary": "List module work items",
      "parameters": [
        {
          "in": "query",
          "name": "cursor",
          "schema": {
            "type": "string"
          },
          "description": "Pagination cursor for getting next set of results",
          "examples": {
            "NextPageCursor": {
              "value": "20:1:0",
              "summary": "Next page cursor",
              "description": "Cursor format: 'page_size:page_number:offset'"
            }
          }
        },
        {
          "in": "query",
          "name": "expand",
          "schema": {
            "type": "string"
          },
          "description": "Comma-separated list of related fields to expand in response",
          "examples": {
            "ExpandAssignees": {
              "value": "assignees",
              "summary": "Expand assignees",
              "description": "Include full assignee details"
            },
            "MultipleExpansions": {
              "value": "assignees,labels,state",
              "summary": "Multiple expansions",
              "description": "Include details for multiple relations"
            }
          }
        },
        {
          "in": "query",
          "name": "fields",
          "schema": {
            "type": "string"
          },
          "description": "Comma-separated list of fields to include in response",
          "examples": {
            "BasicFields": {
              "value": "id,name,description",
              "summary": "Basic fields",
              "description": "Include only basic fields"
            },
            "WithRelations": {
              "value": "id,name,assignees,state",
              "summary": "With relations",
              "description": "Include fields with relationships"
            }
          }
        },
        {
          "in": "path",
          "name": "module_id",
          "schema": {
            "type": "string",
            "format": "uuid"
          },
          "description": "Module ID",
          "required": true,
          "examples": {
            "ExampleModuleID": {
              "value": "550e8400-e29b-41d4-a716-446655440000",
              "summary": "Example module ID",
              "description": "A typical module UUID"
            }
          }
        },
        {
          "in": "query",
          "name": "order_by",
          "schema": {
            "type": "string"
          },
          "description": "Field to order results by. Prefix with '-' for descending order",
          "examples": {
            "CreatedDateDescending": {
              "value": "-created_at",
              "summary": "Created date descending",
              "description": "Most recent items first"
            },
            "PriorityAscending": {
              "value": "priority",
              "summary": "Priority ascending",
              "description": "Order by priority (urgent, high, medium, low, none)"
            },
            "StateGroup": {
              "value": "state__group",
              "summary": "State group",
              "description": "Order by state group (backlog, unstarted, started, review, test, completed, cancelled)"
            },
            "AssigneeName": {
              "value": "assignees__first_name",
              "summary": "Assignee name",
              "description": "Order by assignee first name"
            }
          }
        },
        {
          "in": "query",
          "name": "per_page",
          "schema": {
            "type": "integer"
          },
          "description": "Number of results per page (default: 20, max: 100)",
          "examples": {
            "Default": {
              "value": 20
            },
            "Maximum": {
              "value": 100
            }
          }
        },
        {
          "in": "path",
          "name": "project_id",
          "schema": {
            "type": "string"
          },
          "description": "Either the project's UUID or its workspace-scoped identifier (e.g. ``ENG``). Identifier matching is case-insensitive.",
          "required": true,
          "examples": {
            "uuid": {
              "value": "00000000-0000-0000-0000-000000000000",
              "summary": "UUID form"
            },
            "slug": {
              "value": "ENG",
              "summary": "Workspace-scoped identifier"
            }
          }
        },
        {
          "in": "path",
          "name": "slug",
          "schema": {
            "type": "string"
          },
          "description": "Workspace slug",
          "required": true,
          "examples": {
            "ExampleWorkspace": {
              "value": "my-workspace",
              "summary": "Example workspace",
              "description": "A typical workspace slug"
            }
          }
        }
      ],
      "responses": {
        "401": {
          "description": "Authentication credentials were not provided or are invalid."
        },
        "403": {
          "description": "Permission denied. User lacks required permissions."
        },
        "404": {
          "description": "Module not found"
        },
        "200": {
          "content": {
            "application/json": {
              "schema": {
                "$ref": "#/components/schemas/PaginatedModuleIssueResponse"
              },
              "examples": {
                "PaginatedModuleWorkItems": {
                  "value": {
                    "grouped_by": "state",
                    "sub_grouped_by": "priority",
                    "total_count": 150,
                    "next_cursor": "20:1:0",
                    "prev_cursor": "20:0:0",
                    "next_page_results": true,
                    "prev_page_results": false,
                    "count": 20,
                    "total_pages": 8,
                    "total_results": 150,
                    "extra_stats": null,
                    "results": [
                      {
                        "id": "550e8400-e29b-41d4-a716-446655440000",
                        "name": "Sample Item",
                        "created_at": "2024-01-15T12:00:00Z"
                      }
                    ]
                  },
                  "summary": "Paginated Module Work Items"
                }
              }
            }
          },
          "description": "Paginated list of module work items"
        }
      }
    },
    "post": {
      "operationId": "add_module_work_items",
      "tags": [
        "Modules"
      ],
      "summary": "Add Work Items to Module",
      "parameters": [
        {
          "in": "path",
          "name": "module_id",
          "schema": {
            "type": "string",
            "format": "uuid"
          },
          "description": "Module ID",
          "required": true,
          "examples": {
            "ExampleModuleID": {
              "value": "550e8400-e29b-41d4-a716-446655440000",
              "summary": "Example module ID",
              "description": "A typical module UUID"
            }
          }
        },
        {
          "in": "path",
          "name": "project_id",
          "schema": {
            "type": "string"
          },
          "description": "Either the project's UUID or its workspace-scoped identifier (e.g. ``ENG``). Identifier matching is case-insensitive.",
          "required": true,
          "examples": {
            "uuid": {
              "value": "00000000-0000-0000-0000-000000000000",
              "summary": "UUID form"
            },
            "slug": {
              "value": "ENG",
              "summary": "Workspace-scoped identifier"
            }
          }
        },
        {
          "in": "path",
          "name": "slug",
          "schema": {
            "type": "string"
          },
          "description": "Workspace slug",
          "required": true,
          "examples": {
            "ExampleWorkspace": {
              "value": "my-workspace",
              "summary": "Example workspace",
              "description": "A typical workspace slug"
            }
          }
        }
      ],
      "responses": {
        "401": {
          "description": "Authentication credentials were not provided or are invalid."
        },
        "403": {
          "description": "Permission denied. User lacks required permissions."
        },
        "404": {
          "description": "Module not found"
        },
        "200": {
          "content": {
            "application/json": {
              "schema": {
                "$ref": "#/components/schemas/ModuleIssue"
              },
              "examples": {
                "ModuleIssue": {
                  "value": {
                    "id": "550e8400-e29b-41d4-a716-446655440000",
                    "module": "550e8400-e29b-41d4-a716-446655440001",
                    "issue": "550e8400-e29b-41d4-a716-446655440002",
                    "sub_issues_count": 2,
                    "created_at": "2024-01-01T10:30:00Z",
                    "updated_at": "2024-01-10T15:45:00Z"
                  }
                }
              }
            }
          },
          "description": "Module issues added"
        },
        "400": {
          "description": "Required fields are missing"
        }
      }
    }
  },
  "/api/v1/workspaces/{slug}/projects/{project_id}/modules/{module_id}/module-issues/{issue_id}/": {
    "delete": {
      "operationId": "delete_module_work_item",
      "tags": [
        "Modules"
      ],
      "summary": "Delete module work item",
      "parameters": [
        {
          "in": "path",
          "name": "issue_id",
          "schema": {
            "type": "string",
            "format": "uuid"
          },
          "description": "Issue ID",
          "required": true,
          "examples": {
            "ExampleIssueID": {
              "value": "550e8400-e29b-41d4-a716-446655440000",
              "summary": "Example issue ID",
              "description": "A typical issue UUID"
            }
          }
        },
        {
          "in": "path",
          "name": "module_id",
          "schema": {
            "type": "string",
            "format": "uuid"
          },
          "description": "Module ID",
          "required": true,
          "examples": {
            "ExampleModuleID": {
              "value": "550e8400-e29b-41d4-a716-446655440000",
              "summary": "Example module ID",
              "description": "A typical module UUID"
            }
          }
        },
        {
          "in": "path",
          "name": "project_id",
          "schema": {
            "type": "string"
          },
          "description": "Either the project's UUID or its workspace-scoped identifier (e.g. ``ENG``). Identifier matching is case-insensitive.",
          "required": true,
          "examples": {
            "uuid": {
              "value": "00000000-0000-0000-0000-000000000000",
              "summary": "UUID form"
            },
            "slug": {
              "value": "ENG",
              "summary": "Workspace-scoped identifier"
            }
          }
        },
        {
          "in": "path",
          "name": "slug",
          "schema": {
            "type": "string"
          },
          "description": "Workspace slug",
          "required": true,
          "examples": {
            "ExampleWorkspace": {
              "value": "my-workspace",
              "summary": "Example workspace",
              "description": "A typical workspace slug"
            }
          }
        }
      ],
      "responses": {
        "401": {
          "description": "Authentication credentials were not provided or are invalid."
        },
        "403": {
          "description": "Permission denied. User lacks required permissions."
        },
        "404": {
          "description": "Module issue not found"
        },
        "204": {
          "description": "Resource deleted successfully"
        }
      }
    }
  },
  "/api/v1/workspaces/{slug}/projects/{project_id}/modules/{pk}/": {
    "delete": {
      "operationId": "delete_module",
      "tags": [
        "Modules"
      ],
      "summary": "Delete module",
      "parameters": [
        {
          "in": "path",
          "name": "pk",
          "schema": {
            "type": "string",
            "format": "uuid"
          },
          "description": "Module ID",
          "required": true,
          "examples": {
            "ExampleModuleID": {
              "value": "550e8400-e29b-41d4-a716-446655440000",
              "summary": "Example module ID",
              "description": "A typical module UUID"
            }
          }
        },
        {
          "in": "path",
          "name": "project_id",
          "schema": {
            "type": "string"
          },
          "description": "Either the project's UUID or its workspace-scoped identifier (e.g. ``ENG``). Identifier matching is case-insensitive.",
          "required": true,
          "examples": {
            "uuid": {
              "value": "00000000-0000-0000-0000-000000000000",
              "summary": "UUID form"
            },
            "slug": {
              "value": "ENG",
              "summary": "Workspace-scoped identifier"
            }
          }
        },
        {
          "in": "path",
          "name": "slug",
          "schema": {
            "type": "string"
          },
          "description": "Workspace slug",
          "required": true,
          "examples": {
            "ExampleWorkspace": {
              "value": "my-workspace",
              "summary": "Example workspace",
              "description": "A typical workspace slug"
            }
          }
        }
      ],
      "responses": {
        "401": {
          "description": "Authentication credentials were not provided or are invalid."
        },
        "403": {
          "description": "Only admin or creator can perform this action"
        },
        "404": {
          "description": "Module not found"
        },
        "204": {
          "description": "Resource deleted successfully"
        }
      }
    },
    "get": {
      "operationId": "retrieve_module",
      "tags": [
        "Modules"
      ],
      "summary": "Retrieve module",
      "parameters": [
        {
          "in": "path",
          "name": "pk",
          "schema": {
            "type": "string",
            "format": "uuid"
          },
          "description": "Module ID",
          "required": true,
          "examples": {
            "ExampleModuleID": {
              "value": "550e8400-e29b-41d4-a716-446655440000",
              "summary": "Example module ID",
              "description": "A typical module UUID"
            }
          }
        },
        {
          "in": "path",
          "name": "project_id",
          "schema": {
            "type": "string"
          },
          "description": "Either the project's UUID or its workspace-scoped identifier (e.g. ``ENG``). Identifier matching is case-insensitive.",
          "required": true,
          "examples": {
            "uuid": {
              "value": "00000000-0000-0000-0000-000000000000",
              "summary": "UUID form"
            },
            "slug": {
              "value": "ENG",
              "summary": "Workspace-scoped identifier"
            }
          }
        },
        {
          "in": "path",
          "name": "slug",
          "schema": {
            "type": "string"
          },
          "description": "Workspace slug",
          "required": true,
          "examples": {
            "ExampleWorkspace": {
              "value": "my-workspace",
              "summary": "Example workspace",
              "description": "A typical workspace slug"
            }
          }
        }
      ],
      "responses": {
        "401": {
          "description": "Authentication credentials were not provided or are invalid."
        },
        "403": {
          "description": "Permission denied. User lacks required permissions."
        },
        "404": {
          "description": "Module not found"
        },
        "200": {
          "content": {
            "application/json": {
              "schema": {
                "$ref": "#/components/schemas/Module"
              },
              "examples": {
                "Module": {
                  "value": {
                    "id": "550e8400-e29b-41d4-a716-446655440000",
                    "name": "Authentication Module",
                    "description": "User authentication and authorization features",
                    "start_date": "2024-01-01",
                    "target_date": "2024-02-15",
                    "status": "in-progress",
                    "total_issues": 12,
                    "completed_issues": 5,
                    "cancelled_issues": 0,
                    "started_issues": 4,
                    "unstarted_issues": 3,
                    "backlog_issues": 0,
                    "created_at": "2024-01-01T10:30:00Z",
                    "updated_at": "2024-01-10T15:45:00Z"
                  }
                }
              }
            }
          },
          "description": "Module"
        }
      }
    },
    "patch": {
      "operationId": "update_module",
      "tags": [
        "Modules"
      ],
      "summary": "Update module",
      "parameters": [
        {
          "in": "path",
          "name": "pk",
          "schema": {
            "type": "string",
            "format": "uuid"
          },
          "description": "Module ID",
          "required": true,
          "examples": {
            "ExampleModuleID": {
              "value": "550e8400-e29b-41d4-a716-446655440000",
              "summary": "Example module ID",
              "description": "A typical module UUID"
            }
          }
        },
        {
          "in": "path",
          "name": "project_id",
          "schema": {
            "type": "string"
          },
          "description": "Either the project's UUID or its workspace-scoped identifier (e.g. ``ENG``). Identifier matching is case-insensitive.",
          "required": true,
          "examples": {
            "uuid": {
              "value": "00000000-0000-0000-0000-000000000000",
              "summary": "UUID form"
            },
            "slug": {
              "value": "ENG",
              "summary": "Workspace-scoped identifier"
            }
          }
        },
        {
          "in": "path",
          "name": "slug",
          "schema": {
            "type": "string"
          },
          "description": "Workspace slug",
          "required": true,
          "examples": {
            "ExampleWorkspace": {
              "value": "my-workspace",
              "summary": "Example workspace",
              "description": "A typical workspace slug"
            }
          }
        }
      ],
      "responses": {
        "401": {
          "description": "Authentication credentials were not provided or are invalid."
        },
        "403": {
          "description": "Permission denied. User lacks required permissions."
        },
        "404": {
          "description": "Module not found"
        },
        "200": {
          "content": {
            "application/json": {
              "schema": {
                "$ref": "#/components/schemas/Module"
              },
              "examples": {
                "Module": {
                  "value": {
                    "id": "550e8400-e29b-41d4-a716-446655440000",
                    "name": "Authentication Module",
                    "description": "User authentication and authorization features",
                    "start_date": "2024-01-01",
                    "target_date": "2024-02-15",
                    "status": "in-progress",
                    "total_issues": 12,
                    "completed_issues": 5,
                    "cancelled_issues": 0,
                    "started_issues": 4,
                    "unstarted_issues": 3,
                    "backlog_issues": 0,
                    "created_at": "2024-01-01T10:30:00Z",
                    "updated_at": "2024-01-10T15:45:00Z"
                  }
                }
              }
            }
          },
          "description": "Module updated successfully"
        },
        "400": {
          "content": {
            "application/json": {
              "schema": {
                "$ref": "#/components/schemas/Module"
              },
              "examples": {
                "ModuleUpdateSerializer": {
                  "value": {
                    "name": "Updated Module",
                    "description": "Updated module description",
                    "start_date": "2021-01-01",
                    "end_date": "2021-01-31",
                    "external_id": "1234567890",
                    "external_source": "github"
                  },
                  "description": "Example request for updating a module"
                }
              }
            }
          },
          "description": "Invalid request data"
        },
        "409": {
          "description": "Module with same external ID already exists"
        }
      }
    }
  },
  "/api/v1/workspaces/{slug}/projects/{project_id}/modules/{pk}/archive/": {
    "post": {
      "operationId": "archive_module",
      "tags": [
        "Modules"
      ],
      "summary": "Archive module",
      "parameters": [
        {
          "in": "path",
          "name": "pk",
          "schema": {
            "type": "string",
            "format": "uuid"
          },
          "description": "Module ID",
          "required": true,
          "examples": {
            "ExampleModuleID": {
              "value": "550e8400-e29b-41d4-a716-446655440000",
              "summary": "Example module ID",
              "description": "A typical module UUID"
            }
          }
        },
        {
          "in": "path",
          "name": "project_id",
          "schema": {
            "type": "string"
          },
          "description": "Either the project's UUID or its workspace-scoped identifier (e.g. ``ENG``). Identifier matching is case-insensitive.",
          "required": true,
          "examples": {
            "uuid": {
              "value": "00000000-0000-0000-0000-000000000000",
              "summary": "UUID form"
            },
            "slug": {
              "value": "ENG",
              "summary": "Workspace-scoped identifier"
            }
          }
        },
        {
          "in": "path",
          "name": "slug",
          "schema": {
            "type": "string"
          },
          "description": "Workspace slug",
          "required": true,
          "examples": {
            "ExampleWorkspace": {
              "value": "my-workspace",
              "summary": "Example workspace",
              "description": "A typical workspace slug"
            }
          }
        }
      ],
      "responses": {
        "401": {
          "description": "Authentication credentials were not provided or are invalid."
        },
        "403": {
          "description": "Permission denied. User lacks required permissions."
        },
        "404": {
          "description": "Module not found"
        },
        "204": {
          "description": "No response body"
        },
        "400": {
          "description": "Resource cannot be archived in current state"
        }
      }
    }
  },
  "/api/v1/workspaces/{slug}/projects/{project_id}/pages/": {
    "get": {
      "operationId": "list_pages",
      "tags": [
        "Pages"
      ],
      "summary": "List pages",
      "parameters": [
        {
          "in": "query",
          "name": "cursor",
          "schema": {
            "type": "string"
          },
          "description": "Pagination cursor for getting next set of results",
          "examples": {
            "NextPageCursor": {
              "value": "20:1:0",
              "summary": "Next page cursor",
              "description": "Cursor format: 'page_size:page_number:offset'"
            }
          }
        },
        {
          "in": "query",
          "name": "expand",
          "schema": {
            "type": "string"
          },
          "description": "Comma-separated list of related fields to expand in response",
          "examples": {
            "ExpandAssignees": {
              "value": "assignees",
              "summary": "Expand assignees",
              "description": "Include full assignee details"
            },
            "MultipleExpansions": {
              "value": "assignees,labels,state",
              "summary": "Multiple expansions",
              "description": "Include details for multiple relations"
            }
          }
        },
        {
          "in": "query",
          "name": "fields",
          "schema": {
            "type": "string"
          },
          "description": "Comma-separated list of fields to include in response",
          "examples": {
            "BasicFields": {
              "value": "id,name,description",
              "summary": "Basic fields",
              "description": "Include only basic fields"
            },
            "WithRelations": {
              "value": "id,name,assignees,state",
              "summary": "With relations",
              "description": "Include fields with relationships"
            }
          }
        },
        {
          "in": "query",
          "name": "include_archived",
          "schema": {
            "type": "boolean"
          },
          "description": "Include archived records in the results. Excluded by default.",
          "examples": {
            "IncludeArchived": {
              "value": true,
              "summary": "Include archived",
              "description": "Return archived records alongside active ones"
            }
          }
        },
        {
          "in": "query",
          "name": "per_page",
          "schema": {
            "type": "integer"
          },
          "description": "Number of results per page (default: 20, max: 100)",
          "examples": {
            "Default": {
              "value": 20
            },
            "Maximum": {
              "value": 100
            }
          }
        },
        {
          "in": "path",
          "name": "project_id",
          "schema": {
            "type": "string"
          },
          "description": "Either the project's UUID or its workspace-scoped identifier (e.g. ``ENG``). Identifier matching is case-insensitive.",
          "required": true,
          "examples": {
            "uuid": {
              "value": "00000000-0000-0000-0000-000000000000",
              "summary": "UUID form"
            },
            "slug": {
              "value": "ENG",
              "summary": "Workspace-scoped identifier"
            }
          }
        },
        {
          "in": "path",
          "name": "slug",
          "schema": {
            "type": "string"
          },
          "description": "Workspace slug",
          "required": true,
          "examples": {
            "ExampleWorkspace": {
              "value": "my-workspace",
              "summary": "Example workspace",
              "description": "A typical workspace slug"
            }
          }
        }
      ],
      "responses": {
        "401": {
          "description": "Authentication credentials were not provided or are invalid."
        },
        "403": {
          "description": "Permission denied. User lacks required permissions."
        },
        "404": {
          "description": "The requested resource was not found."
        },
        "200": {
          "content": {
            "application/json": {
              "schema": {
                "$ref": "#/components/schemas/PaginatedPageResponse"
              },
              "examples": {
                "PaginatedPages": {
                  "value": {
                    "grouped_by": "state",
                    "sub_grouped_by": "priority",
                    "total_count": 150,
                    "next_cursor": "20:1:0",
                    "prev_cursor": "20:0:0",
                    "next_page_results": true,
                    "prev_page_results": false,
                    "count": 20,
                    "total_pages": 8,
                    "total_results": 150,
                    "extra_stats": null,
                    "results": [
                      {
                        "id": "550e8400-e29b-41d4-a716-446655440000",
                        "name": "Release checklist",
                        "parent": null,
                        "owned_by": "3fa85f64-5717-4562-b3fc-2c963f66afa6",
                        "access": 0,
                        "is_locked": false,
                        "archived_at": null,
                        "created_at": "2024-01-01T10:30:00Z",
                        "updated_at": "2024-01-10T15:45:00Z"
                      }
                    ]
                  },
                  "summary": "Paginated Pages"
                }
              }
            }
          },
          "description": "Paginated list of pages"
        }
      }
    },
    "post": {
      "operationId": "create_page",
      "tags": [
        "Pages"
      ],
      "summary": "Create page",
      "parameters": [
        {
          "in": "path",
          "name": "project_id",
          "schema": {
            "type": "string"
          },
          "description": "Either the project's UUID or its workspace-scoped identifier (e.g. ``ENG``). Identifier matching is case-insensitive.",
          "required": true,
          "examples": {
            "uuid": {
              "value": "00000000-0000-0000-0000-000000000000",
              "summary": "UUID form"
            },
            "slug": {
              "value": "ENG",
              "summary": "Workspace-scoped identifier"
            }
          }
        },
        {
          "in": "path",
          "name": "slug",
          "schema": {
            "type": "string"
          },
          "description": "Workspace slug",
          "required": true,
          "examples": {
            "ExampleWorkspace": {
              "value": "my-workspace",
              "summary": "Example workspace",
              "description": "A typical workspace slug"
            }
          }
        }
      ],
      "responses": {
        "401": {
          "description": "Authentication credentials were not provided or are invalid."
        },
        "403": {
          "description": "Permission denied. User lacks required permissions."
        },
        "404": {
          "description": "The requested resource was not found."
        },
        "201": {
          "content": {
            "application/json": {
              "schema": {
                "$ref": "#/components/schemas/PageDetail"
              },
              "examples": {
                "PageDetail": {
                  "value": {
                    "id": "550e8400-e29b-41d4-a716-446655440000",
                    "name": "Release checklist",
                    "parent": null,
                    "owned_by": "3fa85f64-5717-4562-b3fc-2c963f66afa6",
                    "access": 0,
                    "is_locked": false,
                    "archived_at": null,
                    "created_at": "2024-01-01T10:30:00Z",
                    "updated_at": "2024-01-10T15:45:00Z",
                    "description_html": "<h1>Release checklist</h1><p>Tag the release <strong>first</strong>.</p>",
                    "description_stripped": "Release checklistTag the release first.",
                    "description_markdown": "# Release checklist\n\nTag the release **first**."
                  },
                  "summary": "Page detail"
                }
              }
            }
          },
          "description": "Page created"
        },
        "400": {
          "description": "Invalid request body or parent"
        },
        "409": {
          "description": "The project is archived"
        },
        "503": {
          "description": "The live document service is unavailable; nothing was written"
        }
      }
    }
  },
  "/api/v1/workspaces/{slug}/projects/{project_id}/pages/{page_id}/": {
    "get": {
      "operationId": "retrieve_page",
      "tags": [
        "Pages"
      ],
      "summary": "Retrieve page",
      "parameters": [
        {
          "in": "query",
          "name": "expand",
          "schema": {
            "type": "string"
          },
          "description": "Comma-separated list of related fields to expand in response",
          "examples": {
            "ExpandAssignees": {
              "value": "assignees",
              "summary": "Expand assignees",
              "description": "Include full assignee details"
            },
            "MultipleExpansions": {
              "value": "assignees,labels,state",
              "summary": "Multiple expansions",
              "description": "Include details for multiple relations"
            }
          }
        },
        {
          "in": "query",
          "name": "fields",
          "schema": {
            "type": "string"
          },
          "description": "Comma-separated list of fields to include in response",
          "examples": {
            "BasicFields": {
              "value": "id,name,description",
              "summary": "Basic fields",
              "description": "Include only basic fields"
            },
            "WithRelations": {
              "value": "id,name,assignees,state",
              "summary": "With relations",
              "description": "Include fields with relationships"
            }
          }
        },
        {
          "in": "path",
          "name": "page_id",
          "schema": {
            "type": "string",
            "format": "uuid"
          },
          "description": "Page ID",
          "required": true,
          "examples": {
            "ExamplePageID": {
              "value": "550e8400-e29b-41d4-a716-446655440000",
              "summary": "Example page ID",
              "description": "A typical page UUID"
            }
          }
        },
        {
          "in": "path",
          "name": "project_id",
          "schema": {
            "type": "string"
          },
          "description": "Either the project's UUID or its workspace-scoped identifier (e.g. ``ENG``). Identifier matching is case-insensitive.",
          "required": true,
          "examples": {
            "uuid": {
              "value": "00000000-0000-0000-0000-000000000000",
              "summary": "UUID form"
            },
            "slug": {
              "value": "ENG",
              "summary": "Workspace-scoped identifier"
            }
          }
        },
        {
          "in": "path",
          "name": "slug",
          "schema": {
            "type": "string"
          },
          "description": "Workspace slug",
          "required": true,
          "examples": {
            "ExampleWorkspace": {
              "value": "my-workspace",
              "summary": "Example workspace",
              "description": "A typical workspace slug"
            }
          }
        }
      ],
      "responses": {
        "401": {
          "description": "Authentication credentials were not provided or are invalid."
        },
        "403": {
          "description": "Permission denied. User lacks required permissions."
        },
        "404": {
          "description": "The requested resource was not found."
        },
        "200": {
          "content": {
            "application/json": {
              "schema": {
                "$ref": "#/components/schemas/PageDetail"
              },
              "examples": {
                "PageDetail": {
                  "value": {
                    "id": "550e8400-e29b-41d4-a716-446655440000",
                    "name": "Release checklist",
                    "parent": null,
                    "owned_by": "3fa85f64-5717-4562-b3fc-2c963f66afa6",
                    "access": 0,
                    "is_locked": false,
                    "archived_at": null,
                    "created_at": "2024-01-01T10:30:00Z",
                    "updated_at": "2024-01-10T15:45:00Z",
                    "description_html": "<h1>Release checklist</h1><p>Tag the release <strong>first</strong>.</p>",
                    "description_stripped": "Release checklistTag the release first.",
                    "description_markdown": "# Release checklist\n\nTag the release **first**."
                  },
                  "summary": "Page detail"
                }
              }
            }
          },
          "description": "Page retrieved"
        }
      }
    },
    "patch": {
      "operationId": "update_page",
      "tags": [
        "Pages"
      ],
      "summary": "Update page",
      "parameters": [
        {
          "in": "path",
          "name": "page_id",
          "schema": {
            "type": "string",
            "format": "uuid"
          },
          "description": "Page ID",
          "required": true,
          "examples": {
            "ExamplePageID": {
              "value": "550e8400-e29b-41d4-a716-446655440000",
              "summary": "Example page ID",
              "description": "A typical page UUID"
            }
          }
        },
        {
          "in": "path",
          "name": "project_id",
          "schema": {
            "type": "string"
          },
          "description": "Either the project's UUID or its workspace-scoped identifier (e.g. ``ENG``). Identifier matching is case-insensitive.",
          "required": true,
          "examples": {
            "uuid": {
              "value": "00000000-0000-0000-0000-000000000000",
              "summary": "UUID form"
            },
            "slug": {
              "value": "ENG",
              "summary": "Workspace-scoped identifier"
            }
          }
        },
        {
          "in": "path",
          "name": "slug",
          "schema": {
            "type": "string"
          },
          "description": "Workspace slug",
          "required": true,
          "examples": {
            "ExampleWorkspace": {
              "value": "my-workspace",
              "summary": "Example workspace",
              "description": "A typical workspace slug"
            }
          }
        }
      ],
      "responses": {
        "401": {
          "description": "Authentication credentials were not provided or are invalid."
        },
        "403": {
          "description": "Permission denied. User lacks required permissions."
        },
        "404": {
          "description": "The requested resource was not found."
        },
        "200": {
          "content": {
            "application/json": {
              "schema": {
                "$ref": "#/components/schemas/PageDetail"
              },
              "examples": {
                "PageDetail": {
                  "value": {
                    "id": "550e8400-e29b-41d4-a716-446655440000",
                    "name": "Release checklist",
                    "parent": null,
                    "owned_by": "3fa85f64-5717-4562-b3fc-2c963f66afa6",
                    "access": 0,
                    "is_locked": false,
                    "archived_at": null,
                    "created_at": "2024-01-01T10:30:00Z",
                    "updated_at": "2024-01-10T15:45:00Z",
                    "description_html": "<h1>Release checklist</h1><p>Tag the release <strong>first</strong>.</p>",
                    "description_stripped": "Release checklistTag the release first.",
                    "description_markdown": "# Release checklist\n\nTag the release **first**."
                  },
                  "summary": "Page detail"
                }
              }
            }
          },
          "description": "Page updated"
        },
        "400": {
          "description": "Invalid request body or parent"
        },
        "409": {
          "description": "The page is locked (`PAGE_LOCKED`) or archived (`PAGE_ARCHIVED`)"
        },
        "503": {
          "description": "The live document service is unavailable; nothing was written"
        }
      }
    }
  },
  "/api/v1/workspaces/{slug}/projects/{project_id}/pages/{page_id}/archive/": {
    "delete": {
      "operationId": "unarchive_page",
      "tags": [
        "Pages"
      ],
      "summary": "Unarchive page",
      "parameters": [
        {
          "in": "path",
          "name": "page_id",
          "schema": {
            "type": "string",
            "format": "uuid"
          },
          "description": "Page ID",
          "required": true,
          "examples": {
            "ExamplePageID": {
              "value": "550e8400-e29b-41d4-a716-446655440000",
              "summary": "Example page ID",
              "description": "A typical page UUID"
            }
          }
        },
        {
          "in": "path",
          "name": "project_id",
          "schema": {
            "type": "string"
          },
          "description": "Either the project's UUID or its workspace-scoped identifier (e.g. ``ENG``). Identifier matching is case-insensitive.",
          "required": true,
          "examples": {
            "uuid": {
              "value": "00000000-0000-0000-0000-000000000000",
              "summary": "UUID form"
            },
            "slug": {
              "value": "ENG",
              "summary": "Workspace-scoped identifier"
            }
          }
        },
        {
          "in": "path",
          "name": "slug",
          "schema": {
            "type": "string"
          },
          "description": "Workspace slug",
          "required": true,
          "examples": {
            "ExampleWorkspace": {
              "value": "my-workspace",
              "summary": "Example workspace",
              "description": "A typical workspace slug"
            }
          }
        }
      ],
      "responses": {
        "401": {
          "description": "Authentication credentials were not provided or are invalid."
        },
        "403": {
          "description": "Permission denied. User lacks required permissions."
        },
        "404": {
          "description": "The requested resource was not found."
        },
        "200": {
          "content": {
            "application/json": {
              "schema": {
                "$ref": "#/components/schemas/PageDetail"
              }
            }
          },
          "description": "Page unarchived"
        },
        "409": {
          "description": "The page is locked (`PAGE_LOCKED`)"
        }
      }
    },
    "post": {
      "operationId": "archive_page",
      "tags": [
        "Pages"
      ],
      "summary": "Archive page",
      "parameters": [
        {
          "in": "path",
          "name": "page_id",
          "schema": {
            "type": "string",
            "format": "uuid"
          },
          "description": "Page ID",
          "required": true,
          "examples": {
            "ExamplePageID": {
              "value": "550e8400-e29b-41d4-a716-446655440000",
              "summary": "Example page ID",
              "description": "A typical page UUID"
            }
          }
        },
        {
          "in": "path",
          "name": "project_id",
          "schema": {
            "type": "string"
          },
          "description": "Either the project's UUID or its workspace-scoped identifier (e.g. ``ENG``). Identifier matching is case-insensitive.",
          "required": true,
          "examples": {
            "uuid": {
              "value": "00000000-0000-0000-0000-000000000000",
              "summary": "UUID form"
            },
            "slug": {
              "value": "ENG",
              "summary": "Workspace-scoped identifier"
            }
          }
        },
        {
          "in": "path",
          "name": "slug",
          "schema": {
            "type": "string"
          },
          "description": "Workspace slug",
          "required": true,
          "examples": {
            "ExampleWorkspace": {
              "value": "my-workspace",
              "summary": "Example workspace",
              "description": "A typical workspace slug"
            }
          }
        }
      ],
      "responses": {
        "401": {
          "description": "Authentication credentials were not provided or are invalid."
        },
        "403": {
          "description": "Permission denied. User lacks required permissions."
        },
        "404": {
          "description": "The requested resource was not found."
        },
        "200": {
          "content": {
            "application/json": {
              "schema": {
                "$ref": "#/components/schemas/PageDetail"
              }
            }
          },
          "description": "Page archived"
        },
        "409": {
          "description": "The page is locked (`PAGE_LOCKED`)"
        }
      }
    }
  },
  "/api/v1/workspaces/{slug}/projects/{project_id}/project-members/": {
    "get": {
      "operationId": "get_project_members_2",
      "tags": [
        "Members"
      ],
      "summary": "List project members",
      "parameters": [
        {
          "in": "path",
          "name": "project_id",
          "schema": {
            "type": "string"
          },
          "description": "Either the project's UUID or its workspace-scoped identifier (e.g. ``ENG``). Identifier matching is case-insensitive.",
          "required": true,
          "examples": {
            "uuid": {
              "value": "00000000-0000-0000-0000-000000000000",
              "summary": "UUID form"
            },
            "slug": {
              "value": "ENG",
              "summary": "Workspace-scoped identifier"
            }
          }
        },
        {
          "in": "path",
          "name": "slug",
          "schema": {
            "type": "string"
          },
          "description": "Workspace slug",
          "required": true,
          "examples": {
            "ExampleWorkspace": {
              "value": "my-workspace",
              "summary": "Example workspace",
              "description": "A typical workspace slug"
            }
          }
        }
      ],
      "responses": {
        "200": {
          "content": {
            "application/json": {
              "schema": {
                "$ref": "#/components/schemas/UserLite"
              },
              "examples": {
                "ProjectMembers": {
                  "value": [
                    {
                      "id": "550e8400-e29b-41d4-a716-446655440000",
                      "first_name": "John",
                      "last_name": "Doe",
                      "display_name": "John Doe",
                      "email": "john.doe@example.com",
                      "avatar": "https://example.com/avatar.jpg"
                    },
                    {
                      "id": "550e8400-e29b-41d4-a716-446655440001",
                      "first_name": "Jane",
                      "last_name": "Smith",
                      "display_name": "Jane Smith",
                      "email": "jane.smith@example.com",
                      "avatar": "https://example.com/avatar2.jpg"
                    }
                  ]
                }
              }
            }
          },
          "description": "List of project members with their roles"
        },
        "401": {
          "description": "Authentication credentials were not provided or are invalid."
        },
        "403": {
          "description": "Permission denied. User lacks required permissions."
        },
        "404": {
          "description": "Project not found"
        }
      }
    },
    "post": {
      "operationId": "create_project_member_2",
      "tags": [
        "Members"
      ],
      "summary": "Create project member",
      "parameters": [
        {
          "in": "path",
          "name": "project_id",
          "schema": {
            "type": "string"
          },
          "description": "Either the project's UUID or its workspace-scoped identifier (e.g. ``ENG``). Identifier matching is case-insensitive.",
          "required": true,
          "examples": {
            "uuid": {
              "value": "00000000-0000-0000-0000-000000000000",
              "summary": "UUID form"
            },
            "slug": {
              "value": "ENG",
              "summary": "Workspace-scoped identifier"
            }
          }
        },
        {
          "in": "path",
          "name": "slug",
          "schema": {
            "type": "string"
          },
          "description": "Workspace slug",
          "required": true,
          "examples": {
            "ExampleWorkspace": {
              "value": "my-workspace",
              "summary": "Example workspace",
              "description": "A typical workspace slug"
            }
          }
        }
      ],
      "responses": {
        "201": {
          "content": {
            "application/json": {
              "schema": {
                "$ref": "#/components/schemas/ProjectMember"
              }
            }
          },
          "description": "Project member created"
        }
      }
    }
  },
  "/api/v1/workspaces/{slug}/projects/{project_id}/project-members/{pk}/": {
    "delete": {
      "operationId": "delete_project_member_2",
      "tags": [
        "Members"
      ],
      "summary": "Delete project member",
      "parameters": [
        {
          "in": "path",
          "name": "pk",
          "schema": {
            "type": "string",
            "format": "uuid"
          },
          "required": true
        },
        {
          "in": "path",
          "name": "project_id",
          "schema": {
            "type": "string"
          },
          "description": "Either the project's UUID or its workspace-scoped identifier (e.g. ``ENG``). Identifier matching is case-insensitive.",
          "required": true,
          "examples": {
            "uuid": {
              "value": "00000000-0000-0000-0000-000000000000",
              "summary": "UUID form"
            },
            "slug": {
              "value": "ENG",
              "summary": "Workspace-scoped identifier"
            }
          }
        },
        {
          "in": "path",
          "name": "slug",
          "schema": {
            "type": "string"
          },
          "description": "Workspace slug",
          "required": true,
          "examples": {
            "ExampleWorkspace": {
              "value": "my-workspace",
              "summary": "Example workspace",
              "description": "A typical workspace slug"
            }
          }
        }
      ],
      "responses": {
        "204": {
          "description": "Project member deleted"
        }
      }
    },
    "get": {
      "operationId": "get_project_member_2",
      "tags": [
        "Members"
      ],
      "summary": "Get project member",
      "parameters": [
        {
          "in": "path",
          "name": "pk",
          "schema": {
            "type": "string",
            "format": "uuid"
          },
          "required": true
        },
        {
          "in": "path",
          "name": "project_id",
          "schema": {
            "type": "string"
          },
          "description": "Either the project's UUID or its workspace-scoped identifier (e.g. ``ENG``). Identifier matching is case-insensitive.",
          "required": true,
          "examples": {
            "uuid": {
              "value": "00000000-0000-0000-0000-000000000000",
              "summary": "UUID form"
            },
            "slug": {
              "value": "ENG",
              "summary": "Workspace-scoped identifier"
            }
          }
        },
        {
          "in": "path",
          "name": "slug",
          "schema": {
            "type": "string"
          },
          "description": "Workspace slug",
          "required": true,
          "examples": {
            "ExampleWorkspace": {
              "value": "my-workspace",
              "summary": "Example workspace",
              "description": "A typical workspace slug"
            }
          }
        }
      ],
      "responses": {
        "200": {
          "content": {
            "application/json": {
              "schema": {
                "$ref": "#/components/schemas/ProjectMember"
              }
            }
          },
          "description": "Project member"
        },
        "401": {
          "description": "Authentication credentials were not provided or are invalid."
        },
        "403": {
          "description": "Permission denied. User lacks required permissions."
        },
        "404": {
          "description": "Project not found"
        }
      }
    },
    "patch": {
      "operationId": "update_project_member_2",
      "tags": [
        "Members"
      ],
      "summary": "Update project member",
      "parameters": [
        {
          "in": "path",
          "name": "pk",
          "schema": {
            "type": "string",
            "format": "uuid"
          },
          "required": true
        },
        {
          "in": "path",
          "name": "project_id",
          "schema": {
            "type": "string"
          },
          "description": "Either the project's UUID or its workspace-scoped identifier (e.g. ``ENG``). Identifier matching is case-insensitive.",
          "required": true,
          "examples": {
            "uuid": {
              "value": "00000000-0000-0000-0000-000000000000",
              "summary": "UUID form"
            },
            "slug": {
              "value": "ENG",
              "summary": "Workspace-scoped identifier"
            }
          }
        },
        {
          "in": "path",
          "name": "slug",
          "schema": {
            "type": "string"
          },
          "description": "Workspace slug",
          "required": true,
          "examples": {
            "ExampleWorkspace": {
              "value": "my-workspace",
              "summary": "Example workspace",
              "description": "A typical workspace slug"
            }
          }
        }
      ],
      "responses": {
        "200": {
          "content": {
            "application/json": {
              "schema": {
                "$ref": "#/components/schemas/ProjectMember"
              }
            }
          },
          "description": "Project member updated"
        }
      }
    }
  },
  "/api/v1/workspaces/{slug}/projects/{project_id}/states/": {
    "get": {
      "operationId": "list_states",
      "tags": [
        "States"
      ],
      "summary": "List states",
      "parameters": [
        {
          "in": "query",
          "name": "cursor",
          "schema": {
            "type": "string"
          },
          "description": "Pagination cursor for getting next set of results",
          "examples": {
            "NextPageCursor": {
              "value": "20:1:0",
              "summary": "Next page cursor",
              "description": "Cursor format: 'page_size:page_number:offset'"
            }
          }
        },
        {
          "in": "query",
          "name": "expand",
          "schema": {
            "type": "string"
          },
          "description": "Comma-separated list of related fields to expand in response",
          "examples": {
            "ExpandAssignees": {
              "value": "assignees",
              "summary": "Expand assignees",
              "description": "Include full assignee details"
            },
            "MultipleExpansions": {
              "value": "assignees,labels,state",
              "summary": "Multiple expansions",
              "description": "Include details for multiple relations"
            }
          }
        },
        {
          "in": "query",
          "name": "fields",
          "schema": {
            "type": "string"
          },
          "description": "Comma-separated list of fields to include in response",
          "examples": {
            "BasicFields": {
              "value": "id,name,description",
              "summary": "Basic fields",
              "description": "Include only basic fields"
            },
            "WithRelations": {
              "value": "id,name,assignees,state",
              "summary": "With relations",
              "description": "Include fields with relationships"
            }
          }
        },
        {
          "in": "query",
          "name": "per_page",
          "schema": {
            "type": "integer"
          },
          "description": "Number of results per page (default: 20, max: 100)",
          "examples": {
            "Default": {
              "value": 20
            },
            "Maximum": {
              "value": 100
            }
          }
        },
        {
          "in": "path",
          "name": "project_id",
          "schema": {
            "type": "string"
          },
          "description": "Either the project's UUID or its workspace-scoped identifier (e.g. ``ENG``). Identifier matching is case-insensitive.",
          "required": true,
          "examples": {
            "uuid": {
              "value": "00000000-0000-0000-0000-000000000000",
              "summary": "UUID form"
            },
            "slug": {
              "value": "ENG",
              "summary": "Workspace-scoped identifier"
            }
          }
        },
        {
          "in": "path",
          "name": "slug",
          "schema": {
            "type": "string"
          },
          "description": "Workspace slug",
          "required": true,
          "examples": {
            "ExampleWorkspace": {
              "value": "my-workspace",
              "summary": "Example workspace",
              "description": "A typical workspace slug"
            }
          }
        }
      ],
      "responses": {
        "401": {
          "description": "Authentication credentials were not provided or are invalid."
        },
        "403": {
          "description": "Permission denied. User lacks required permissions."
        },
        "404": {
          "description": "The requested resource was not found."
        },
        "200": {
          "content": {
            "application/json": {
              "schema": {
                "$ref": "#/components/schemas/PaginatedStateResponse"
              },
              "examples": {
                "PaginatedStates": {
                  "value": {
                    "grouped_by": "state",
                    "sub_grouped_by": "priority",
                    "total_count": 150,
                    "next_cursor": "20:1:0",
                    "prev_cursor": "20:0:0",
                    "next_page_results": true,
                    "prev_page_results": false,
                    "count": 20,
                    "total_pages": 8,
                    "total_results": 150,
                    "extra_stats": null,
                    "results": [
                      {
                        "id": "550e8400-e29b-41d4-a716-446655440000",
                        "name": "In Progress",
                        "color": "#ffa500",
                        "group": "started",
                        "sequence": 2
                      }
                    ]
                  },
                  "summary": "Paginated States"
                }
              }
            }
          },
          "description": "Paginated list of states"
        }
      }
    },
    "post": {
      "operationId": "create_state",
      "tags": [
        "States"
      ],
      "summary": "Create state",
      "parameters": [
        {
          "in": "path",
          "name": "project_id",
          "schema": {
            "type": "string"
          },
          "description": "Either the project's UUID or its workspace-scoped identifier (e.g. ``ENG``). Identifier matching is case-insensitive.",
          "required": true,
          "examples": {
            "uuid": {
              "value": "00000000-0000-0000-0000-000000000000",
              "summary": "UUID form"
            },
            "slug": {
              "value": "ENG",
              "summary": "Workspace-scoped identifier"
            }
          }
        },
        {
          "in": "path",
          "name": "slug",
          "schema": {
            "type": "string"
          },
          "description": "Workspace slug",
          "required": true,
          "examples": {
            "ExampleWorkspace": {
              "value": "my-workspace",
              "summary": "Example workspace",
              "description": "A typical workspace slug"
            }
          }
        }
      ],
      "responses": {
        "401": {
          "description": "Authentication credentials were not provided or are invalid."
        },
        "403": {
          "description": "Permission denied. User lacks required permissions."
        },
        "404": {
          "description": "The requested resource was not found."
        },
        "200": {
          "content": {
            "application/json": {
              "schema": {
                "$ref": "#/components/schemas/State"
              },
              "examples": {
                "State": {
                  "value": {
                    "id": "550e8400-e29b-41d4-a716-446655440000",
                    "name": "In Progress",
                    "color": "#f39c12",
                    "group": "started",
                    "sequence": 2,
                    "default": false,
                    "created_at": "2024-01-01T10:30:00Z",
                    "updated_at": "2024-01-10T15:45:00Z"
                  }
                }
              }
            }
          },
          "description": "State created"
        },
        "400": {
          "description": "Invalid request data provided"
        },
        "409": {
          "description": "State with the same name already exists"
        }
      }
    }
  },
  "/api/v1/workspaces/{slug}/projects/{project_id}/states/{state_id}/": {
    "delete": {
      "operationId": "delete_state",
      "tags": [
        "States"
      ],
      "summary": "Delete state",
      "parameters": [
        {
          "in": "path",
          "name": "project_id",
          "schema": {
            "type": "string"
          },
          "description": "Either the project's UUID or its workspace-scoped identifier (e.g. ``ENG``). Identifier matching is case-insensitive.",
          "required": true,
          "examples": {
            "uuid": {
              "value": "00000000-0000-0000-0000-000000000000",
              "summary": "UUID form"
            },
            "slug": {
              "value": "ENG",
              "summary": "Workspace-scoped identifier"
            }
          }
        },
        {
          "in": "path",
          "name": "slug",
          "schema": {
            "type": "string"
          },
          "description": "Workspace slug",
          "required": true,
          "examples": {
            "ExampleWorkspace": {
              "value": "my-workspace",
              "summary": "Example workspace",
              "description": "A typical workspace slug"
            }
          }
        },
        {
          "in": "path",
          "name": "state_id",
          "schema": {
            "type": "string",
            "format": "uuid"
          },
          "description": "State ID",
          "required": true,
          "examples": {
            "ExampleStateID": {
              "value": "550e8400-e29b-41d4-a716-446655440000",
              "summary": "Example state ID",
              "description": "A typical state UUID"
            }
          }
        }
      ],
      "responses": {
        "401": {
          "description": "Authentication credentials were not provided or are invalid."
        },
        "403": {
          "description": "Permission denied. User lacks required permissions."
        },
        "404": {
          "description": "The requested resource was not found."
        },
        "204": {
          "description": "Resource deleted successfully"
        },
        "400": {
          "description": "State cannot be deleted"
        }
      }
    },
    "get": {
      "operationId": "retrieve_state",
      "tags": [
        "States"
      ],
      "summary": "Retrieve state",
      "parameters": [
        {
          "in": "path",
          "name": "project_id",
          "schema": {
            "type": "string"
          },
          "description": "Either the project's UUID or its workspace-scoped identifier (e.g. ``ENG``). Identifier matching is case-insensitive.",
          "required": true,
          "examples": {
            "uuid": {
              "value": "00000000-0000-0000-0000-000000000000",
              "summary": "UUID form"
            },
            "slug": {
              "value": "ENG",
              "summary": "Workspace-scoped identifier"
            }
          }
        },
        {
          "in": "path",
          "name": "slug",
          "schema": {
            "type": "string"
          },
          "description": "Workspace slug",
          "required": true,
          "examples": {
            "ExampleWorkspace": {
              "value": "my-workspace",
              "summary": "Example workspace",
              "description": "A typical workspace slug"
            }
          }
        },
        {
          "in": "path",
          "name": "state_id",
          "schema": {
            "type": "string",
            "format": "uuid"
          },
          "description": "State ID",
          "required": true,
          "examples": {
            "ExampleStateID": {
              "value": "550e8400-e29b-41d4-a716-446655440000",
              "summary": "Example state ID",
              "description": "A typical state UUID"
            }
          }
        }
      ],
      "responses": {
        "401": {
          "description": "Authentication credentials were not provided or are invalid."
        },
        "403": {
          "description": "Permission denied. User lacks required permissions."
        },
        "404": {
          "description": "The requested resource was not found."
        },
        "200": {
          "content": {
            "application/json": {
              "schema": {
                "$ref": "#/components/schemas/State"
              },
              "examples": {
                "State": {
                  "value": {
                    "id": "550e8400-e29b-41d4-a716-446655440000",
                    "name": "In Progress",
                    "color": "#f39c12",
                    "group": "started",
                    "sequence": 2,
                    "default": false,
                    "created_at": "2024-01-01T10:30:00Z",
                    "updated_at": "2024-01-10T15:45:00Z"
                  }
                }
              }
            }
          },
          "description": "State retrieved"
        }
      }
    },
    "patch": {
      "operationId": "update_state",
      "tags": [
        "States"
      ],
      "summary": "Update state",
      "parameters": [
        {
          "in": "path",
          "name": "project_id",
          "schema": {
            "type": "string"
          },
          "description": "Either the project's UUID or its workspace-scoped identifier (e.g. ``ENG``). Identifier matching is case-insensitive.",
          "required": true,
          "examples": {
            "uuid": {
              "value": "00000000-0000-0000-0000-000000000000",
              "summary": "UUID form"
            },
            "slug": {
              "value": "ENG",
              "summary": "Workspace-scoped identifier"
            }
          }
        },
        {
          "in": "path",
          "name": "slug",
          "schema": {
            "type": "string"
          },
          "description": "Workspace slug",
          "required": true,
          "examples": {
            "ExampleWorkspace": {
              "value": "my-workspace",
              "summary": "Example workspace",
              "description": "A typical workspace slug"
            }
          }
        },
        {
          "in": "path",
          "name": "state_id",
          "schema": {
            "type": "string",
            "format": "uuid"
          },
          "description": "State ID",
          "required": true,
          "examples": {
            "ExampleStateID": {
              "value": "550e8400-e29b-41d4-a716-446655440000",
              "summary": "Example state ID",
              "description": "A typical state UUID"
            }
          }
        }
      ],
      "responses": {
        "401": {
          "description": "Authentication credentials were not provided or are invalid."
        },
        "403": {
          "description": "Permission denied. User lacks required permissions."
        },
        "404": {
          "description": "The requested resource was not found."
        },
        "200": {
          "content": {
            "application/json": {
              "schema": {
                "$ref": "#/components/schemas/State"
              },
              "examples": {
                "State": {
                  "value": {
                    "id": "550e8400-e29b-41d4-a716-446655440000",
                    "name": "In Progress",
                    "color": "#f39c12",
                    "group": "started",
                    "sequence": 2,
                    "default": false,
                    "created_at": "2024-01-01T10:30:00Z",
                    "updated_at": "2024-01-10T15:45:00Z"
                  }
                }
              }
            }
          },
          "description": "State updated"
        },
        "400": {
          "description": "Invalid request data provided"
        },
        "409": {
          "description": "Resource with same external ID already exists"
        }
      }
    }
  },
  "/api/v1/workspaces/{slug}/projects/{project_id}/summary/": {
    "get": {
      "operationId": "workspaces_projects_summary_retrieve",
      "tags": [
        "workspaces"
      ],
      "summary": null,
      "parameters": [
        {
          "in": "path",
          "name": "project_id",
          "schema": {
            "type": "string"
          },
          "required": true,
          "description": "Either the project's UUID or its workspace-scoped identifier (e.g. ``ENG``). Identifier matching is case-insensitive.",
          "examples": {
            "uuid": {
              "value": "00000000-0000-0000-0000-000000000000",
              "summary": "UUID form"
            },
            "slug": {
              "value": "ENG",
              "summary": "Workspace-scoped identifier"
            }
          }
        },
        {
          "in": "path",
          "name": "slug",
          "schema": {
            "type": "string"
          },
          "required": true
        }
      ],
      "responses": {
        "200": {
          "description": "No response body"
        }
      }
    }
  },
  "/api/v1/workspaces/{slug}/projects/{project_id}/work-items/": {
    "get": {
      "operationId": "list_work_items_2",
      "tags": [
        "Work Items"
      ],
      "summary": "List work items",
      "parameters": [
        {
          "in": "query",
          "name": "assignees",
          "schema": {
            "type": "string"
          },
          "description": "Filter by assignee. Comma-separated user UUIDs."
        },
        {
          "in": "query",
          "name": "cursor",
          "schema": {
            "type": "string"
          },
          "description": "Pagination cursor for getting next set of results",
          "examples": {
            "NextPageCursor": {
              "value": "20:1:0",
              "summary": "Next page cursor",
              "description": "Cursor format: 'page_size:page_number:offset'"
            }
          }
        },
        {
          "in": "query",
          "name": "expand",
          "schema": {
            "type": "string"
          },
          "description": "Comma-separated list of related fields to expand in response",
          "examples": {
            "ExpandAssignees": {
              "value": "assignees",
              "summary": "Expand assignees",
              "description": "Include full assignee details"
            },
            "MultipleExpansions": {
              "value": "assignees,labels,state",
              "summary": "Multiple expansions",
              "description": "Include details for multiple relations"
            }
          }
        },
        {
          "in": "query",
          "name": "external_id",
          "schema": {
            "type": "string"
          },
          "description": "External system identifier for filtering or lookup",
          "examples": {
            "GitHubIssue": {
              "value": "1234567890",
              "summary": "GitHub Issue",
              "description": "GitHub issue number"
            }
          }
        },
        {
          "in": "query",
          "name": "external_source",
          "schema": {
            "type": "string"
          },
          "description": "External system source name for filtering or lookup",
          "examples": {
            "GitHub": {
              "value": "github",
              "description": "GitHub integration source"
            },
            "Jira": {
              "value": "jira",
              "description": "Jira integration source"
            }
          }
        },
        {
          "in": "query",
          "name": "fields",
          "schema": {
            "type": "string"
          },
          "description": "Comma-separated list of fields to include in response",
          "examples": {
            "BasicFields": {
              "value": "id,name,description",
              "summary": "Basic fields",
              "description": "Include only basic fields"
            },
            "WithRelations": {
              "value": "id,name,assignees,state",
              "summary": "With relations",
              "description": "Include fields with relationships"
            }
          }
        },
        {
          "in": "query",
          "name": "labels",
          "schema": {
            "type": "string"
          },
          "description": "Filter by label. Comma-separated label UUIDs and/or label names (case-insensitive).",
          "examples": {
            "ByName": {
              "value": "bug,frontend",
              "summary": "By name"
            }
          }
        },
        {
          "in": "query",
          "name": "order_by",
          "schema": {
            "type": "string"
          },
          "description": "Field to order results by. Prefix with '-' for descending order",
          "examples": {
            "CreatedDateDescending": {
              "value": "-created_at",
              "summary": "Created date descending",
              "description": "Most recent items first"
            },
            "PriorityAscending": {
              "value": "priority",
              "summary": "Priority ascending",
              "description": "Order by priority (urgent, high, medium, low, none)"
            },
            "StateGroup": {
              "value": "state__group",
              "summary": "State group",
              "description": "Order by state group (backlog, unstarted, started, review, test, completed, cancelled)"
            },
            "AssigneeName": {
              "value": "assignees__first_name",
              "summary": "Assignee name",
              "description": "Order by assignee first name"
            }
          }
        },
        {
          "in": "query",
          "name": "parent",
          "schema": {
            "type": "string"
          },
          "description": "Filter by parent work item. Comma-separated parent UUIDs and/or identifiers (e.g. PROJ-123). Pass `null` to return only top-level work items.",
          "examples": {
            "ChildrenOfAnEpic": {
              "value": "PROJ-123",
              "summary": "Children of an epic"
            },
            "Top-levelOnly": {
              "value": "null",
              "summary": "Top-level only"
            }
          }
        },
        {
          "in": "query",
          "name": "per_page",
          "schema": {
            "type": "integer"
          },
          "description": "Number of results per page (default: 20, max: 100)",
          "examples": {
            "Default": {
              "value": 20
            },
            "Maximum": {
              "value": 100
            }
          }
        },
        {
          "in": "query",
          "name": "priority",
          "schema": {
            "type": "string"
          },
          "description": "Filter by priority. Comma-separated values from: urgent, high, medium, low, none.",
          "examples": {
            "Hot": {
              "value": "urgent,high"
            }
          }
        },
        {
          "in": "path",
          "name": "project_id",
          "schema": {
            "type": "string"
          },
          "description": "Either the project's UUID or its workspace-scoped identifier (e.g. ``ENG``). Identifier matching is case-insensitive.",
          "required": true,
          "examples": {
            "uuid": {
              "value": "00000000-0000-0000-0000-000000000000",
              "summary": "UUID form"
            },
            "slug": {
              "value": "ENG",
              "summary": "Workspace-scoped identifier"
            }
          }
        },
        {
          "in": "path",
          "name": "slug",
          "schema": {
            "type": "string"
          },
          "description": "Workspace slug",
          "required": true,
          "examples": {
            "ExampleWorkspace": {
              "value": "my-workspace",
              "summary": "Example workspace",
              "description": "A typical workspace slug"
            }
          }
        },
        {
          "in": "query",
          "name": "state",
          "schema": {
            "type": "string"
          },
          "description": "Filter by state. Comma-separated state UUIDs and/or state names (case-insensitive exact match within the project). An unknown name returns 400 listing the valid state names.",
          "examples": {
            "ByName": {
              "value": "Backlog,Todo",
              "summary": "By name"
            },
            "ById": {
              "value": "550e8400-e29b-41d4-a716-446655440000",
              "summary": "By id"
            }
          }
        },
        {
          "in": "query",
          "name": "state_group",
          "schema": {
            "type": "string"
          },
          "description": "Filter by state group. Comma-separated values from: backlog, unstarted, started, review, test, completed, cancelled.",
          "examples": {
            "OpenWork": {
              "value": "unstarted,started",
              "summary": "Open work"
            }
          }
        }
      ],
      "responses": {
        "401": {
          "description": "Authentication credentials were not provided or are invalid."
        },
        "403": {
          "description": "Permission denied. User lacks required permissions."
        },
        "404": {
          "description": "Project not found"
        },
        "200": {
          "content": {
            "application/json": {
              "schema": {
                "$ref": "#/components/schemas/PaginatedWorkItemResponse"
              },
              "examples": {
                "PaginatedWorkItems": {
                  "value": {
                    "grouped_by": "state",
                    "sub_grouped_by": "priority",
                    "total_count": 150,
                    "next_cursor": "20:1:0",
                    "prev_cursor": "20:0:0",
                    "next_page_results": true,
                    "prev_page_results": false,
                    "count": 20,
                    "total_pages": 8,
                    "total_results": 150,
                    "extra_stats": null,
                    "results": [
                      {
                        "id": "550e8400-e29b-41d4-a716-446655440000",
                        "name": "Fix authentication bug in user login",
                        "description": "Users are unable to log in due to authentication service timeout",
                        "priority": "high",
                        "sequence_id": 123,
                        "state": {
                          "id": "550e8400-e29b-41d4-a716-446655440001",
                          "name": "In Progress",
                          "group": "started"
                        },
                        "assignees": [],
                        "labels": [],
                        "created_at": "2024-01-15T10:30:00Z"
                      }
                    ]
                  },
                  "summary": "Paginated Work Items"
                }
              }
            }
          },
          "description": "Paginated list of work items"
        },
        "400": {
          "description": "Invalid request data provided"
        }
      }
    },
    "post": {
      "operationId": "create_work_item_2",
      "tags": [
        "Work Items"
      ],
      "summary": "Create work item",
      "parameters": [
        {
          "in": "path",
          "name": "project_id",
          "schema": {
            "type": "string"
          },
          "description": "Either the project's UUID or its workspace-scoped identifier (e.g. ``ENG``). Identifier matching is case-insensitive.",
          "required": true,
          "examples": {
            "uuid": {
              "value": "00000000-0000-0000-0000-000000000000",
              "summary": "UUID form"
            },
            "slug": {
              "value": "ENG",
              "summary": "Workspace-scoped identifier"
            }
          }
        },
        {
          "in": "path",
          "name": "slug",
          "schema": {
            "type": "string"
          },
          "description": "Workspace slug",
          "required": true,
          "examples": {
            "ExampleWorkspace": {
              "value": "my-workspace",
              "summary": "Example workspace",
              "description": "A typical workspace slug"
            }
          }
        }
      ],
      "responses": {
        "401": {
          "description": "Authentication credentials were not provided or are invalid."
        },
        "403": {
          "description": "Permission denied. User lacks required permissions."
        },
        "404": {
          "description": "Project not found"
        },
        "201": {
          "content": {
            "application/json": {
              "schema": {
                "$ref": "#/components/schemas/Issue"
              },
              "examples": {
                "Issue": {
                  "value": {
                    "id": "550e8400-e29b-41d4-a716-446655440000",
                    "name": "Implement user authentication",
                    "description": "Add OAuth 2.0 authentication flow",
                    "sequence_id": 1,
                    "priority": "high",
                    "assignees": [
                      "550e8400-e29b-41d4-a716-446655440001"
                    ],
                    "labels": [
                      "550e8400-e29b-41d4-a716-446655440002"
                    ],
                    "created_at": "2024-01-15T10:30:00Z",
                    "updated_at": "2024-01-15T10:30:00Z"
                  }
                }
              }
            }
          },
          "description": "Work Item created successfully"
        },
        "400": {
          "description": "Invalid request data provided"
        },
        "409": {
          "description": "Resource with same external ID already exists"
        }
      }
    }
  },
  "/api/v1/workspaces/{slug}/projects/{project_id}/work-items/{issue_id}/activities/": {
    "get": {
      "operationId": "list_work_item_activities_2",
      "tags": [
        "Work Item Activity"
      ],
      "summary": null,
      "parameters": [
        {
          "in": "query",
          "name": "cursor",
          "schema": {
            "type": "string"
          },
          "description": "Pagination cursor for getting next set of results",
          "examples": {
            "NextPageCursor": {
              "value": "20:1:0",
              "summary": "Next page cursor",
              "description": "Cursor format: 'page_size:page_number:offset'"
            }
          }
        },
        {
          "in": "query",
          "name": "expand",
          "schema": {
            "type": "string"
          },
          "description": "Comma-separated list of related fields to expand in response",
          "examples": {
            "ExpandAssignees": {
              "value": "assignees",
              "summary": "Expand assignees",
              "description": "Include full assignee details"
            },
            "MultipleExpansions": {
              "value": "assignees,labels,state",
              "summary": "Multiple expansions",
              "description": "Include details for multiple relations"
            }
          }
        },
        {
          "in": "query",
          "name": "fields",
          "schema": {
            "type": "string"
          },
          "description": "Comma-separated list of fields to include in response",
          "examples": {
            "BasicFields": {
              "value": "id,name,description",
              "summary": "Basic fields",
              "description": "Include only basic fields"
            },
            "WithRelations": {
              "value": "id,name,assignees,state",
              "summary": "With relations",
              "description": "Include fields with relationships"
            }
          }
        },
        {
          "in": "path",
          "name": "issue_id",
          "schema": {
            "type": "string",
            "format": "uuid"
          },
          "description": "Issue ID",
          "required": true,
          "examples": {
            "ExampleIssueID": {
              "value": "550e8400-e29b-41d4-a716-446655440000",
              "summary": "Example issue ID",
              "description": "A typical issue UUID"
            }
          }
        },
        {
          "in": "query",
          "name": "order_by",
          "schema": {
            "type": "string"
          },
          "description": "Field to order results by. Prefix with '-' for descending order",
          "examples": {
            "CreatedDateDescending": {
              "value": "-created_at",
              "summary": "Created date descending",
              "description": "Most recent items first"
            },
            "PriorityAscending": {
              "value": "priority",
              "summary": "Priority ascending",
              "description": "Order by priority (urgent, high, medium, low, none)"
            },
            "StateGroup": {
              "value": "state__group",
              "summary": "State group",
              "description": "Order by state group (backlog, unstarted, started, review, test, completed, cancelled)"
            },
            "AssigneeName": {
              "value": "assignees__first_name",
              "summary": "Assignee name",
              "description": "Order by assignee first name"
            }
          }
        },
        {
          "in": "query",
          "name": "per_page",
          "schema": {
            "type": "integer"
          },
          "description": "Number of results per page (default: 20, max: 100)",
          "examples": {
            "Default": {
              "value": 20
            },
            "Maximum": {
              "value": 100
            }
          }
        },
        {
          "in": "path",
          "name": "project_id",
          "schema": {
            "type": "string"
          },
          "description": "Either the project's UUID or its workspace-scoped identifier (e.g. ``ENG``). Identifier matching is case-insensitive.",
          "required": true,
          "examples": {
            "uuid": {
              "value": "00000000-0000-0000-0000-000000000000",
              "summary": "UUID form"
            },
            "slug": {
              "value": "ENG",
              "summary": "Workspace-scoped identifier"
            }
          }
        },
        {
          "in": "path",
          "name": "slug",
          "schema": {
            "type": "string"
          },
          "description": "Workspace slug",
          "required": true,
          "examples": {
            "ExampleWorkspace": {
              "value": "my-workspace",
              "summary": "Example workspace",
              "description": "A typical workspace slug"
            }
          }
        }
      ],
      "responses": {
        "401": {
          "description": "Authentication credentials were not provided or are invalid."
        },
        "403": {
          "description": "Permission denied. User lacks required permissions."
        },
        "404": {
          "description": "Issue not found"
        },
        "200": {
          "content": {
            "application/json": {
              "schema": {
                "$ref": "#/components/schemas/PaginatedIssueActivityResponse"
              },
              "examples": {
                "PaginatedIssueActivities": {
                  "value": {
                    "grouped_by": "state",
                    "sub_grouped_by": "priority",
                    "total_count": 150,
                    "next_cursor": "20:1:0",
                    "prev_cursor": "20:0:0",
                    "next_page_results": true,
                    "prev_page_results": false,
                    "count": 20,
                    "total_pages": 8,
                    "total_results": 150,
                    "extra_stats": null,
                    "results": [
                      {
                        "id": "550e8400-e29b-41d4-a716-446655440000",
                        "name": "Sample Item",
                        "created_at": "2024-01-15T12:00:00Z"
                      }
                    ]
                  },
                  "summary": "Paginated Issue Activities"
                }
              }
            }
          },
          "description": "Paginated list of issue activities"
        },
        "400": {
          "description": "Invalid request data provided"
        }
      }
    }
  },
  "/api/v1/workspaces/{slug}/projects/{project_id}/work-items/{issue_id}/activities/{pk}/": {
    "get": {
      "operationId": "retrieve_work_item_activity_2",
      "tags": [
        "Work Item Activity"
      ],
      "summary": null,
      "parameters": [
        {
          "in": "query",
          "name": "cursor",
          "schema": {
            "type": "string"
          },
          "description": "Pagination cursor for getting next set of results",
          "examples": {
            "NextPageCursor": {
              "value": "20:1:0",
              "summary": "Next page cursor",
              "description": "Cursor format: 'page_size:page_number:offset'"
            }
          }
        },
        {
          "in": "query",
          "name": "expand",
          "schema": {
            "type": "string"
          },
          "description": "Comma-separated list of related fields to expand in response",
          "examples": {
            "ExpandAssignees": {
              "value": "assignees",
              "summary": "Expand assignees",
              "description": "Include full assignee details"
            },
            "MultipleExpansions": {
              "value": "assignees,labels,state",
              "summary": "Multiple expansions",
              "description": "Include details for multiple relations"
            }
          }
        },
        {
          "in": "query",
          "name": "fields",
          "schema": {
            "type": "string"
          },
          "description": "Comma-separated list of fields to include in response",
          "examples": {
            "BasicFields": {
              "value": "id,name,description",
              "summary": "Basic fields",
              "description": "Include only basic fields"
            },
            "WithRelations": {
              "value": "id,name,assignees,state",
              "summary": "With relations",
              "description": "Include fields with relationships"
            }
          }
        },
        {
          "in": "path",
          "name": "issue_id",
          "schema": {
            "type": "string",
            "format": "uuid"
          },
          "description": "Issue ID",
          "required": true,
          "examples": {
            "ExampleIssueID": {
              "value": "550e8400-e29b-41d4-a716-446655440000",
              "summary": "Example issue ID",
              "description": "A typical issue UUID"
            }
          }
        },
        {
          "in": "query",
          "name": "order_by",
          "schema": {
            "type": "string"
          },
          "description": "Field to order results by. Prefix with '-' for descending order",
          "examples": {
            "CreatedDateDescending": {
              "value": "-created_at",
              "summary": "Created date descending",
              "description": "Most recent items first"
            },
            "PriorityAscending": {
              "value": "priority",
              "summary": "Priority ascending",
              "description": "Order by priority (urgent, high, medium, low, none)"
            },
            "StateGroup": {
              "value": "state__group",
              "summary": "State group",
              "description": "Order by state group (backlog, unstarted, started, review, test, completed, cancelled)"
            },
            "AssigneeName": {
              "value": "assignees__first_name",
              "summary": "Assignee name",
              "description": "Order by assignee first name"
            }
          }
        },
        {
          "in": "query",
          "name": "per_page",
          "schema": {
            "type": "integer"
          },
          "description": "Number of results per page (default: 20, max: 100)",
          "examples": {
            "Default": {
              "value": 20
            },
            "Maximum": {
              "value": 100
            }
          }
        },
        {
          "in": "path",
          "name": "pk",
          "schema": {
            "type": "string",
            "format": "uuid"
          },
          "description": "Activity ID",
          "required": true,
          "examples": {
            "ExampleActivityID": {
              "value": "550e8400-e29b-41d4-a716-446655440000",
              "summary": "Example activity ID",
              "description": "A typical activity UUID"
            }
          }
        },
        {
          "in": "path",
          "name": "project_id",
          "schema": {
            "type": "string"
          },
          "description": "Either the project's UUID or its workspace-scoped identifier (e.g. ``ENG``). Identifier matching is case-insensitive.",
          "required": true,
          "examples": {
            "uuid": {
              "value": "00000000-0000-0000-0000-000000000000",
              "summary": "UUID form"
            },
            "slug": {
              "value": "ENG",
              "summary": "Workspace-scoped identifier"
            }
          }
        },
        {
          "in": "path",
          "name": "slug",
          "schema": {
            "type": "string"
          },
          "description": "Workspace slug",
          "required": true,
          "examples": {
            "ExampleWorkspace": {
              "value": "my-workspace",
              "summary": "Example workspace",
              "description": "A typical workspace slug"
            }
          }
        }
      ],
      "responses": {
        "401": {
          "description": "Authentication credentials were not provided or are invalid."
        },
        "403": {
          "description": "Permission denied. User lacks required permissions."
        },
        "404": {
          "description": "Issue not found"
        },
        "200": {
          "content": {
            "application/json": {
              "schema": {
                "$ref": "#/components/schemas/PaginatedIssueActivityDetailResponse"
              },
              "examples": {
                "WorkItemActivityDetails": {
                  "value": {
                    "grouped_by": "state",
                    "sub_grouped_by": "priority",
                    "total_count": 150,
                    "next_cursor": "20:1:0",
                    "prev_cursor": "20:0:0",
                    "next_page_results": true,
                    "prev_page_results": false,
                    "count": 20,
                    "total_pages": 8,
                    "total_results": 150,
                    "extra_stats": null,
                    "results": [
                      {
                        "id": "550e8400-e29b-41d4-a716-446655440000",
                        "name": "Sample Item",
                        "created_at": "2024-01-15T12:00:00Z"
                      }
                    ]
                  },
                  "summary": "Work Item Activity Details"
                }
              }
            }
          },
          "description": "Paginated list of work item activities"
        },
        "400": {
          "description": "Invalid request data provided"
        }
      }
    }
  },
  "/api/v1/workspaces/{slug}/projects/{project_id}/work-items/{issue_id}/attachments/": {
    "get": {
      "operationId": "list_work_item_attachments_2",
      "tags": [
        "Work Item Attachments"
      ],
      "summary": null,
      "parameters": [
        {
          "in": "path",
          "name": "issue_id",
          "schema": {
            "type": "string",
            "format": "uuid"
          },
          "description": "Issue ID",
          "required": true,
          "examples": {
            "ExampleIssueID": {
              "value": "550e8400-e29b-41d4-a716-446655440000",
              "summary": "Example issue ID",
              "description": "A typical issue UUID"
            }
          }
        },
        {
          "in": "path",
          "name": "project_id",
          "schema": {
            "type": "string"
          },
          "description": "Either the project's UUID or its workspace-scoped identifier (e.g. ``ENG``). Identifier matching is case-insensitive.",
          "required": true,
          "examples": {
            "uuid": {
              "value": "00000000-0000-0000-0000-000000000000",
              "summary": "UUID form"
            },
            "slug": {
              "value": "ENG",
              "summary": "Workspace-scoped identifier"
            }
          }
        },
        {
          "in": "path",
          "name": "slug",
          "schema": {
            "type": "string"
          },
          "description": "Workspace slug",
          "required": true,
          "examples": {
            "ExampleWorkspace": {
              "value": "my-workspace",
              "summary": "Example workspace",
              "description": "A typical workspace slug"
            }
          }
        }
      ],
      "responses": {
        "401": {
          "description": "Authentication credentials were not provided or are invalid."
        },
        "403": {
          "description": "Permission denied. User lacks required permissions."
        },
        "404": {
          "description": "Attachment not found"
        },
        "200": {
          "content": {
            "application/json": {
              "schema": {
                "$ref": "#/components/schemas/IssueAttachment"
              },
              "examples": {
                "IssueAttachment": {
                  "value": {
                    "id": "550e8400-e29b-41d4-a716-446655440000",
                    "name": "screenshot.png",
                    "size": 1024000,
                    "asset_url": "https://s3.amazonaws.com/bucket/screenshot.png?signed-url",
                    "attributes": {
                      "name": "screenshot.png",
                      "type": "image/png",
                      "size": 1024000
                    },
                    "created_at": "2024-01-01T10:30:00Z",
                    "updated_at": "2024-01-10T15:45:00Z"
                  }
                }
              }
            }
          },
          "description": "Work item attachment"
        },
        "400": {
          "description": "Invalid request data provided"
        }
      }
    },
    "post": {
      "operationId": "create_work_item_attachment_2",
      "tags": [
        "Work Item Attachments"
      ],
      "summary": null,
      "parameters": [
        {
          "in": "path",
          "name": "issue_id",
          "schema": {
            "type": "string",
            "format": "uuid"
          },
          "description": "Issue ID",
          "required": true,
          "examples": {
            "ExampleIssueID": {
              "value": "550e8400-e29b-41d4-a716-446655440000",
              "summary": "Example issue ID",
              "description": "A typical issue UUID"
            }
          }
        },
        {
          "in": "path",
          "name": "project_id",
          "schema": {
            "type": "string"
          },
          "description": "Either the project's UUID or its workspace-scoped identifier (e.g. ``ENG``). Identifier matching is case-insensitive.",
          "required": true,
          "examples": {
            "uuid": {
              "value": "00000000-0000-0000-0000-000000000000",
              "summary": "UUID form"
            },
            "slug": {
              "value": "ENG",
              "summary": "Workspace-scoped identifier"
            }
          }
        },
        {
          "in": "path",
          "name": "slug",
          "schema": {
            "type": "string"
          },
          "description": "Workspace slug",
          "required": true,
          "examples": {
            "ExampleWorkspace": {
              "value": "my-workspace",
              "summary": "Example workspace",
              "description": "A typical workspace slug"
            }
          }
        }
      ],
      "responses": {
        "401": {
          "description": "Authentication credentials were not provided or are invalid."
        },
        "403": {
          "description": "Permission denied. User lacks required permissions."
        },
        "404": {
          "description": "Issue or Project or Workspace not found"
        },
        "200": {
          "description": "Presigned download URL generated successfully"
        },
        "400": {
          "description": "Validation error"
        }
      }
    }
  },
  "/api/v1/workspaces/{slug}/projects/{project_id}/work-items/{issue_id}/attachments/{pk}/": {
    "delete": {
      "operationId": "delete_work_item_attachment_2",
      "tags": [
        "Work Item Attachments"
      ],
      "summary": null,
      "parameters": [
        {
          "in": "path",
          "name": "issue_id",
          "schema": {
            "type": "string",
            "format": "uuid"
          },
          "required": true
        },
        {
          "in": "path",
          "name": "pk",
          "schema": {
            "type": "string",
            "format": "uuid"
          },
          "description": "Attachment ID",
          "required": true,
          "examples": {
            "ExampleAttachmentID": {
              "value": "550e8400-e29b-41d4-a716-446655440000",
              "summary": "Example attachment ID",
              "description": "A typical attachment UUID"
            }
          }
        },
        {
          "in": "path",
          "name": "project_id",
          "schema": {
            "type": "string"
          },
          "description": "Either the project's UUID or its workspace-scoped identifier (e.g. ``ENG``). Identifier matching is case-insensitive.",
          "required": true,
          "examples": {
            "uuid": {
              "value": "00000000-0000-0000-0000-000000000000",
              "summary": "UUID form"
            },
            "slug": {
              "value": "ENG",
              "summary": "Workspace-scoped identifier"
            }
          }
        },
        {
          "in": "path",
          "name": "slug",
          "schema": {
            "type": "string"
          },
          "description": "Workspace slug",
          "required": true,
          "examples": {
            "ExampleWorkspace": {
              "value": "my-workspace",
              "summary": "Example workspace",
              "description": "A typical workspace slug"
            }
          }
        }
      ],
      "responses": {
        "401": {
          "description": "Authentication credentials were not provided or are invalid."
        },
        "403": {
          "description": "Permission denied. User lacks required permissions."
        },
        "404": {
          "description": "Attachment not found"
        },
        "204": {
          "description": "Work item attachment deleted successfully"
        }
      }
    },
    "get": {
      "operationId": "retrieve_work_item_attachment_2",
      "tags": [
        "Work Item Attachments"
      ],
      "summary": null,
      "parameters": [
        {
          "in": "path",
          "name": "issue_id",
          "schema": {
            "type": "string",
            "format": "uuid"
          },
          "required": true
        },
        {
          "in": "path",
          "name": "pk",
          "schema": {
            "type": "string",
            "format": "uuid"
          },
          "description": "Attachment ID",
          "required": true,
          "examples": {
            "ExampleAttachmentID": {
              "value": "550e8400-e29b-41d4-a716-446655440000",
              "summary": "Example attachment ID",
              "description": "A typical attachment UUID"
            }
          }
        },
        {
          "in": "path",
          "name": "project_id",
          "schema": {
            "type": "string"
          },
          "description": "Either the project's UUID or its workspace-scoped identifier (e.g. ``ENG``). Identifier matching is case-insensitive.",
          "required": true,
          "examples": {
            "uuid": {
              "value": "00000000-0000-0000-0000-000000000000",
              "summary": "UUID form"
            },
            "slug": {
              "value": "ENG",
              "summary": "Workspace-scoped identifier"
            }
          }
        },
        {
          "in": "path",
          "name": "slug",
          "schema": {
            "type": "string"
          },
          "description": "Workspace slug",
          "required": true,
          "examples": {
            "ExampleWorkspace": {
              "value": "my-workspace",
              "summary": "Example workspace",
              "description": "A typical workspace slug"
            }
          }
        }
      ],
      "responses": {
        "401": {
          "description": "Authentication credentials were not provided or are invalid."
        },
        "403": {
          "description": "Permission denied. User lacks required permissions."
        },
        "404": {
          "description": "Attachment not found"
        },
        "302": {
          "description": "Redirect to presigned download URL"
        },
        "400": {
          "content": {
            "application/json": {
              "schema": {
                "type": "object",
                "properties": {
                  "error": {
                    "type": "string",
                    "description": "Error message",
                    "example": "The asset is not uploaded."
                  },
                  "status": {
                    "type": "boolean",
                    "description": "Request status",
                    "example": false
                  }
                }
              },
              "examples": {
                "IssueAttachmentNotUploaded": {
                  "value": {
                    "error": "The asset is not uploaded.",
                    "status": false
                  },
                  "summary": "Issue Attachment Not Uploaded",
                  "description": "Error when trying to download an attachment that hasn't been uploaded yet"
                }
              }
            }
          },
          "description": "Asset not uploaded"
        }
      }
    },
    "patch": {
      "operationId": "upload_work_item_attachment_2",
      "tags": [
        "Work Item Attachments"
      ],
      "summary": null,
      "parameters": [
        {
          "in": "path",
          "name": "issue_id",
          "schema": {
            "type": "string",
            "format": "uuid"
          },
          "required": true
        },
        {
          "in": "path",
          "name": "pk",
          "schema": {
            "type": "string",
            "format": "uuid"
          },
          "description": "Attachment ID",
          "required": true,
          "examples": {
            "ExampleAttachmentID": {
              "value": "550e8400-e29b-41d4-a716-446655440000",
              "summary": "Example attachment ID",
              "description": "A typical attachment UUID"
            }
          }
        },
        {
          "in": "path",
          "name": "project_id",
          "schema": {
            "type": "string"
          },
          "description": "Either the project's UUID or its workspace-scoped identifier (e.g. ``ENG``). Identifier matching is case-insensitive.",
          "required": true,
          "examples": {
            "uuid": {
              "value": "00000000-0000-0000-0000-000000000000",
              "summary": "UUID form"
            },
            "slug": {
              "value": "ENG",
              "summary": "Workspace-scoped identifier"
            }
          }
        },
        {
          "in": "path",
          "name": "slug",
          "schema": {
            "type": "string"
          },
          "description": "Workspace slug",
          "required": true,
          "examples": {
            "ExampleWorkspace": {
              "value": "my-workspace",
              "summary": "Example workspace",
              "description": "A typical workspace slug"
            }
          }
        }
      ],
      "responses": {
        "401": {
          "description": "Authentication credentials were not provided or are invalid."
        },
        "403": {
          "description": "Permission denied. User lacks required permissions."
        },
        "404": {
          "description": "Attachment not found"
        },
        "204": {
          "description": "Work item attachment uploaded successfully"
        },
        "400": {
          "description": "Invalid request data provided"
        }
      }
    }
  },
  "/api/v1/workspaces/{slug}/projects/{project_id}/work-items/{issue_id}/code-reviews/": {
    "get": {
      "operationId": "workspaces_projects_work_items_code_reviews_retrieve",
      "tags": [
        "workspaces"
      ],
      "summary": null,
      "parameters": [
        {
          "in": "path",
          "name": "issue_id",
          "schema": {
            "type": "string",
            "format": "uuid"
          },
          "required": true
        },
        {
          "in": "path",
          "name": "project_id",
          "schema": {
            "type": "string"
          },
          "required": true,
          "description": "Either the project's UUID or its workspace-scoped identifier (e.g. ``ENG``). Identifier matching is case-insensitive.",
          "examples": {
            "uuid": {
              "value": "00000000-0000-0000-0000-000000000000",
              "summary": "UUID form"
            },
            "slug": {
              "value": "ENG",
              "summary": "Workspace-scoped identifier"
            }
          }
        },
        {
          "in": "path",
          "name": "slug",
          "schema": {
            "type": "string"
          },
          "required": true
        }
      ],
      "responses": {
        "200": {
          "content": {
            "application/json": {
              "schema": {
                "$ref": "#/components/schemas/GitCodeReviewLink"
              }
            }
          },
          "description": ""
        }
      }
    },
    "post": {
      "operationId": "workspaces_projects_work_items_code_reviews_create",
      "tags": [
        "workspaces"
      ],
      "summary": null,
      "parameters": [
        {
          "in": "path",
          "name": "issue_id",
          "schema": {
            "type": "string",
            "format": "uuid"
          },
          "required": true
        },
        {
          "in": "path",
          "name": "project_id",
          "schema": {
            "type": "string"
          },
          "required": true,
          "description": "Either the project's UUID or its workspace-scoped identifier (e.g. ``ENG``). Identifier matching is case-insensitive.",
          "examples": {
            "uuid": {
              "value": "00000000-0000-0000-0000-000000000000",
              "summary": "UUID form"
            },
            "slug": {
              "value": "ENG",
              "summary": "Workspace-scoped identifier"
            }
          }
        },
        {
          "in": "path",
          "name": "slug",
          "schema": {
            "type": "string"
          },
          "required": true
        }
      ],
      "responses": {
        "200": {
          "content": {
            "application/json": {
              "schema": {
                "$ref": "#/components/schemas/GitCodeReviewLink"
              }
            }
          },
          "description": ""
        }
      }
    }
  },
  "/api/v1/workspaces/{slug}/projects/{project_id}/work-items/{issue_id}/code-reviews/{pk}/": {
    "delete": {
      "operationId": "workspaces_projects_work_items_code_reviews_destroy",
      "tags": [
        "workspaces"
      ],
      "summary": null,
      "parameters": [
        {
          "in": "path",
          "name": "issue_id",
          "schema": {
            "type": "string",
            "format": "uuid"
          },
          "required": true
        },
        {
          "in": "path",
          "name": "pk",
          "schema": {
            "type": "string",
            "format": "uuid"
          },
          "required": true
        },
        {
          "in": "path",
          "name": "project_id",
          "schema": {
            "type": "string"
          },
          "required": true,
          "description": "Either the project's UUID or its workspace-scoped identifier (e.g. ``ENG``). Identifier matching is case-insensitive.",
          "examples": {
            "uuid": {
              "value": "00000000-0000-0000-0000-000000000000",
              "summary": "UUID form"
            },
            "slug": {
              "value": "ENG",
              "summary": "Workspace-scoped identifier"
            }
          }
        },
        {
          "in": "path",
          "name": "slug",
          "schema": {
            "type": "string"
          },
          "required": true
        }
      ],
      "responses": {
        "204": {
          "description": "No response body"
        }
      }
    }
  },
  "/api/v1/workspaces/{slug}/projects/{project_id}/work-items/{issue_id}/comments/": {
    "get": {
      "operationId": "list_work_item_comments_2",
      "tags": [
        "Work Item Comments"
      ],
      "summary": null,
      "parameters": [
        {
          "in": "query",
          "name": "cursor",
          "schema": {
            "type": "string"
          },
          "description": "Pagination cursor for getting next set of results",
          "examples": {
            "NextPageCursor": {
              "value": "20:1:0",
              "summary": "Next page cursor",
              "description": "Cursor format: 'page_size:page_number:offset'"
            }
          }
        },
        {
          "in": "query",
          "name": "expand",
          "schema": {
            "type": "string"
          },
          "description": "Comma-separated list of related fields to expand in response",
          "examples": {
            "ExpandAssignees": {
              "value": "assignees",
              "summary": "Expand assignees",
              "description": "Include full assignee details"
            },
            "MultipleExpansions": {
              "value": "assignees,labels,state",
              "summary": "Multiple expansions",
              "description": "Include details for multiple relations"
            }
          }
        },
        {
          "in": "query",
          "name": "fields",
          "schema": {
            "type": "string"
          },
          "description": "Comma-separated list of fields to include in response",
          "examples": {
            "BasicFields": {
              "value": "id,name,description",
              "summary": "Basic fields",
              "description": "Include only basic fields"
            },
            "WithRelations": {
              "value": "id,name,assignees,state",
              "summary": "With relations",
              "description": "Include fields with relationships"
            }
          }
        },
        {
          "in": "path",
          "name": "issue_id",
          "schema": {
            "type": "string",
            "format": "uuid"
          },
          "description": "Issue ID",
          "required": true,
          "examples": {
            "ExampleIssueID": {
              "value": "550e8400-e29b-41d4-a716-446655440000",
              "summary": "Example issue ID",
              "description": "A typical issue UUID"
            }
          }
        },
        {
          "in": "query",
          "name": "order_by",
          "schema": {
            "type": "string"
          },
          "description": "Field to order results by. Prefix with '-' for descending order",
          "examples": {
            "CreatedDateDescending": {
              "value": "-created_at",
              "summary": "Created date descending",
              "description": "Most recent items first"
            },
            "PriorityAscending": {
              "value": "priority",
              "summary": "Priority ascending",
              "description": "Order by priority (urgent, high, medium, low, none)"
            },
            "StateGroup": {
              "value": "state__group",
              "summary": "State group",
              "description": "Order by state group (backlog, unstarted, started, review, test, completed, cancelled)"
            },
            "AssigneeName": {
              "value": "assignees__first_name",
              "summary": "Assignee name",
              "description": "Order by assignee first name"
            }
          }
        },
        {
          "in": "query",
          "name": "per_page",
          "schema": {
            "type": "integer"
          },
          "description": "Number of results per page (default: 20, max: 100)",
          "examples": {
            "Default": {
              "value": 20
            },
            "Maximum": {
              "value": 100
            }
          }
        },
        {
          "in": "path",
          "name": "project_id",
          "schema": {
            "type": "string"
          },
          "description": "Either the project's UUID or its workspace-scoped identifier (e.g. ``ENG``). Identifier matching is case-insensitive.",
          "required": true,
          "examples": {
            "uuid": {
              "value": "00000000-0000-0000-0000-000000000000",
              "summary": "UUID form"
            },
            "slug": {
              "value": "ENG",
              "summary": "Workspace-scoped identifier"
            }
          }
        },
        {
          "in": "path",
          "name": "slug",
          "schema": {
            "type": "string"
          },
          "description": "Workspace slug",
          "required": true,
          "examples": {
            "ExampleWorkspace": {
              "value": "my-workspace",
              "summary": "Example workspace",
              "description": "A typical workspace slug"
            }
          }
        }
      ],
      "responses": {
        "401": {
          "description": "Authentication credentials were not provided or are invalid."
        },
        "403": {
          "description": "Permission denied. User lacks required permissions."
        },
        "404": {
          "description": "Issue not found"
        },
        "200": {
          "content": {
            "application/json": {
              "schema": {
                "$ref": "#/components/schemas/PaginatedIssueCommentResponse"
              },
              "examples": {
                "PaginatedWorkItemComments": {
                  "value": {
                    "grouped_by": "state",
                    "sub_grouped_by": "priority",
                    "total_count": 150,
                    "next_cursor": "20:1:0",
                    "prev_cursor": "20:0:0",
                    "next_page_results": true,
                    "prev_page_results": false,
                    "count": 20,
                    "total_pages": 8,
                    "total_results": 150,
                    "extra_stats": null,
                    "results": [
                      {
                        "id": "550e8400-e29b-41d4-a716-446655440000",
                        "name": "Sample Item",
                        "created_at": "2024-01-15T12:00:00Z"
                      }
                    ]
                  },
                  "summary": "Paginated Work Item Comments"
                }
              }
            }
          },
          "description": "Paginated list of work item comments"
        }
      }
    },
    "post": {
      "operationId": "create_work_item_comment_2",
      "tags": [
        "Work Item Comments"
      ],
      "summary": null,
      "parameters": [
        {
          "in": "path",
          "name": "issue_id",
          "schema": {
            "type": "string",
            "format": "uuid"
          },
          "description": "Issue ID",
          "required": true,
          "examples": {
            "ExampleIssueID": {
              "value": "550e8400-e29b-41d4-a716-446655440000",
              "summary": "Example issue ID",
              "description": "A typical issue UUID"
            }
          }
        },
        {
          "in": "path",
          "name": "project_id",
          "schema": {
            "type": "string"
          },
          "description": "Either the project's UUID or its workspace-scoped identifier (e.g. ``ENG``). Identifier matching is case-insensitive.",
          "required": true,
          "examples": {
            "uuid": {
              "value": "00000000-0000-0000-0000-000000000000",
              "summary": "UUID form"
            },
            "slug": {
              "value": "ENG",
              "summary": "Workspace-scoped identifier"
            }
          }
        },
        {
          "in": "path",
          "name": "slug",
          "schema": {
            "type": "string"
          },
          "description": "Workspace slug",
          "required": true,
          "examples": {
            "ExampleWorkspace": {
              "value": "my-workspace",
              "summary": "Example workspace",
              "description": "A typical workspace slug"
            }
          }
        }
      ],
      "responses": {
        "401": {
          "description": "Authentication credentials were not provided or are invalid."
        },
        "403": {
          "description": "Permission denied. User lacks required permissions."
        },
        "404": {
          "description": "Issue not found"
        },
        "201": {
          "content": {
            "application/json": {
              "schema": {
                "$ref": "#/components/schemas/IssueComment"
              },
              "examples": {
                "IssueComment": {
                  "value": {
                    "id": "550e8400-e29b-41d4-a716-446655440000",
                    "comment_html": "<p>This issue has been resolved by implementing OAuth 2.0 flow.</p>",
                    "labels": [],
                    "comment_json": {
                      "type": "doc",
                      "content": [
                        {
                          "type": "paragraph",
                          "content": [
                            {
                              "type": "text",
                              "text": "This issue has been resolved by implementing OAuth 2.0 flow."
                            }
                          ]
                        }
                      ]
                    },
                    "actor": {
                      "id": "550e8400-e29b-41d4-a716-446655440001",
                      "first_name": "John",
                      "last_name": "Doe",
                      "display_name": "John Doe",
                      "avatar": "https://example.com/avatar.jpg"
                    },
                    "created_at": "2024-01-01T10:30:00Z",
                    "updated_at": "2024-01-10T15:45:00Z"
                  }
                }
              }
            }
          },
          "description": "Work item comment created successfully"
        },
        "400": {
          "description": "Invalid request data provided"
        },
        "409": {
          "description": "Resource with same external ID already exists"
        }
      }
    }
  },
  "/api/v1/workspaces/{slug}/projects/{project_id}/work-items/{issue_id}/comments/{pk}/": {
    "delete": {
      "operationId": "delete_work_item_comment_2",
      "tags": [
        "Work Item Comments"
      ],
      "summary": null,
      "parameters": [
        {
          "in": "path",
          "name": "issue_id",
          "schema": {
            "type": "string",
            "format": "uuid"
          },
          "description": "Issue ID",
          "required": true,
          "examples": {
            "ExampleIssueID": {
              "value": "550e8400-e29b-41d4-a716-446655440000",
              "summary": "Example issue ID",
              "description": "A typical issue UUID"
            }
          }
        },
        {
          "in": "path",
          "name": "pk",
          "schema": {
            "type": "string",
            "format": "uuid"
          },
          "description": "Comment ID",
          "required": true,
          "examples": {
            "ExampleCommentID": {
              "value": "550e8400-e29b-41d4-a716-446655440000",
              "summary": "Example comment ID",
              "description": "A typical comment UUID"
            }
          }
        },
        {
          "in": "path",
          "name": "project_id",
          "schema": {
            "type": "string"
          },
          "description": "Either the project's UUID or its workspace-scoped identifier (e.g. ``ENG``). Identifier matching is case-insensitive.",
          "required": true,
          "examples": {
            "uuid": {
              "value": "00000000-0000-0000-0000-000000000000",
              "summary": "UUID form"
            },
            "slug": {
              "value": "ENG",
              "summary": "Workspace-scoped identifier"
            }
          }
        },
        {
          "in": "path",
          "name": "slug",
          "schema": {
            "type": "string"
          },
          "description": "Workspace slug",
          "required": true,
          "examples": {
            "ExampleWorkspace": {
              "value": "my-workspace",
              "summary": "Example workspace",
              "description": "A typical workspace slug"
            }
          }
        }
      ],
      "responses": {
        "401": {
          "description": "Authentication credentials were not provided or are invalid."
        },
        "403": {
          "description": "Permission denied. User lacks required permissions."
        },
        "404": {
          "description": "Comment not found"
        },
        "204": {
          "description": "Work item comment deleted successfully"
        }
      }
    },
    "get": {
      "operationId": "retrieve_work_item_comment_2",
      "tags": [
        "Work Item Comments"
      ],
      "summary": null,
      "parameters": [
        {
          "in": "path",
          "name": "issue_id",
          "schema": {
            "type": "string",
            "format": "uuid"
          },
          "description": "Issue ID",
          "required": true,
          "examples": {
            "ExampleIssueID": {
              "value": "550e8400-e29b-41d4-a716-446655440000",
              "summary": "Example issue ID",
              "description": "A typical issue UUID"
            }
          }
        },
        {
          "in": "path",
          "name": "pk",
          "schema": {
            "type": "string",
            "format": "uuid"
          },
          "description": "Comment ID",
          "required": true,
          "examples": {
            "ExampleCommentID": {
              "value": "550e8400-e29b-41d4-a716-446655440000",
              "summary": "Example comment ID",
              "description": "A typical comment UUID"
            }
          }
        },
        {
          "in": "path",
          "name": "project_id",
          "schema": {
            "type": "string"
          },
          "description": "Either the project's UUID or its workspace-scoped identifier (e.g. ``ENG``). Identifier matching is case-insensitive.",
          "required": true,
          "examples": {
            "uuid": {
              "value": "00000000-0000-0000-0000-000000000000",
              "summary": "UUID form"
            },
            "slug": {
              "value": "ENG",
              "summary": "Workspace-scoped identifier"
            }
          }
        },
        {
          "in": "path",
          "name": "slug",
          "schema": {
            "type": "string"
          },
          "description": "Workspace slug",
          "required": true,
          "examples": {
            "ExampleWorkspace": {
              "value": "my-workspace",
              "summary": "Example workspace",
              "description": "A typical workspace slug"
            }
          }
        }
      ],
      "responses": {
        "401": {
          "description": "Authentication credentials were not provided or are invalid."
        },
        "403": {
          "description": "Permission denied. User lacks required permissions."
        },
        "404": {
          "description": "Issue not found"
        },
        "200": {
          "content": {
            "application/json": {
              "schema": {
                "$ref": "#/components/schemas/IssueComment"
              },
              "examples": {
                "IssueComment": {
                  "value": {
                    "id": "550e8400-e29b-41d4-a716-446655440000",
                    "comment_html": "<p>This issue has been resolved by implementing OAuth 2.0 flow.</p>",
                    "labels": [],
                    "comment_json": {
                      "type": "doc",
                      "content": [
                        {
                          "type": "paragraph",
                          "content": [
                            {
                              "type": "text",
                              "text": "This issue has been resolved by implementing OAuth 2.0 flow."
                            }
                          ]
                        }
                      ]
                    },
                    "actor": {
                      "id": "550e8400-e29b-41d4-a716-446655440001",
                      "first_name": "John",
                      "last_name": "Doe",
                      "display_name": "John Doe",
                      "avatar": "https://example.com/avatar.jpg"
                    },
                    "created_at": "2024-01-01T10:30:00Z",
                    "updated_at": "2024-01-10T15:45:00Z"
                  }
                }
              }
            }
          },
          "description": "Work item comments"
        },
        "400": {
          "description": "Invalid request data provided"
        }
      }
    },
    "patch": {
      "operationId": "update_work_item_comment_2",
      "tags": [
        "Work Item Comments"
      ],
      "summary": null,
      "parameters": [
        {
          "in": "path",
          "name": "issue_id",
          "schema": {
            "type": "string",
            "format": "uuid"
          },
          "description": "Issue ID",
          "required": true,
          "examples": {
            "ExampleIssueID": {
              "value": "550e8400-e29b-41d4-a716-446655440000",
              "summary": "Example issue ID",
              "description": "A typical issue UUID"
            }
          }
        },
        {
          "in": "path",
          "name": "pk",
          "schema": {
            "type": "string",
            "format": "uuid"
          },
          "description": "Comment ID",
          "required": true,
          "examples": {
            "ExampleCommentID": {
              "value": "550e8400-e29b-41d4-a716-446655440000",
              "summary": "Example comment ID",
              "description": "A typical comment UUID"
            }
          }
        },
        {
          "in": "path",
          "name": "project_id",
          "schema": {
            "type": "string"
          },
          "description": "Either the project's UUID or its workspace-scoped identifier (e.g. ``ENG``). Identifier matching is case-insensitive.",
          "required": true,
          "examples": {
            "uuid": {
              "value": "00000000-0000-0000-0000-000000000000",
              "summary": "UUID form"
            },
            "slug": {
              "value": "ENG",
              "summary": "Workspace-scoped identifier"
            }
          }
        },
        {
          "in": "path",
          "name": "slug",
          "schema": {
            "type": "string"
          },
          "description": "Workspace slug",
          "required": true,
          "examples": {
            "ExampleWorkspace": {
              "value": "my-workspace",
              "summary": "Example workspace",
              "description": "A typical workspace slug"
            }
          }
        }
      ],
      "responses": {
        "401": {
          "description": "Authentication credentials were not provided or are invalid."
        },
        "403": {
          "description": "Permission denied. User lacks required permissions."
        },
        "404": {
          "description": "Comment not found"
        },
        "200": {
          "content": {
            "application/json": {
              "schema": {
                "$ref": "#/components/schemas/IssueComment"
              },
              "examples": {
                "IssueComment": {
                  "value": {
                    "id": "550e8400-e29b-41d4-a716-446655440000",
                    "comment_html": "<p>This issue has been resolved by implementing OAuth 2.0 flow.</p>",
                    "labels": [],
                    "comment_json": {
                      "type": "doc",
                      "content": [
                        {
                          "type": "paragraph",
                          "content": [
                            {
                              "type": "text",
                              "text": "This issue has been resolved by implementing OAuth 2.0 flow."
                            }
                          ]
                        }
                      ]
                    },
                    "actor": {
                      "id": "550e8400-e29b-41d4-a716-446655440001",
                      "first_name": "John",
                      "last_name": "Doe",
                      "display_name": "John Doe",
                      "avatar": "https://example.com/avatar.jpg"
                    },
                    "created_at": "2024-01-01T10:30:00Z",
                    "updated_at": "2024-01-10T15:45:00Z"
                  }
                }
              }
            }
          },
          "description": "Work item comment updated successfully"
        },
        "400": {
          "description": "Invalid request data provided"
        },
        "409": {
          "description": "Resource with same external ID already exists"
        }
      }
    }
  },
  "/api/v1/workspaces/{slug}/projects/{project_id}/work-items/{issue_id}/github/pull-requests/": {
    "get": {
      "operationId": "workspaces_projects_work_items_github_pull_requests_retrieve",
      "tags": [
        "workspaces"
      ],
      "summary": null,
      "parameters": [
        {
          "in": "path",
          "name": "issue_id",
          "schema": {
            "type": "string",
            "format": "uuid"
          },
          "required": true
        },
        {
          "in": "path",
          "name": "project_id",
          "schema": {
            "type": "string"
          },
          "required": true,
          "description": "Either the project's UUID or its workspace-scoped identifier (e.g. ``ENG``). Identifier matching is case-insensitive.",
          "examples": {
            "uuid": {
              "value": "00000000-0000-0000-0000-000000000000",
              "summary": "UUID form"
            },
            "slug": {
              "value": "ENG",
              "summary": "Workspace-scoped identifier"
            }
          }
        },
        {
          "in": "path",
          "name": "slug",
          "schema": {
            "type": "string"
          },
          "required": true
        }
      ],
      "responses": {
        "200": {
          "content": {
            "application/json": {
              "schema": {
                "$ref": "#/components/schemas/GithubPullRequestLink"
              }
            }
          },
          "description": ""
        }
      }
    },
    "post": {
      "operationId": "workspaces_projects_work_items_github_pull_requests_create",
      "tags": [
        "workspaces"
      ],
      "summary": null,
      "parameters": [
        {
          "in": "path",
          "name": "issue_id",
          "schema": {
            "type": "string",
            "format": "uuid"
          },
          "required": true
        },
        {
          "in": "path",
          "name": "project_id",
          "schema": {
            "type": "string"
          },
          "required": true,
          "description": "Either the project's UUID or its workspace-scoped identifier (e.g. ``ENG``). Identifier matching is case-insensitive.",
          "examples": {
            "uuid": {
              "value": "00000000-0000-0000-0000-000000000000",
              "summary": "UUID form"
            },
            "slug": {
              "value": "ENG",
              "summary": "Workspace-scoped identifier"
            }
          }
        },
        {
          "in": "path",
          "name": "slug",
          "schema": {
            "type": "string"
          },
          "required": true
        }
      ],
      "responses": {
        "200": {
          "content": {
            "application/json": {
              "schema": {
                "$ref": "#/components/schemas/GithubPullRequestLink"
              }
            }
          },
          "description": ""
        }
      }
    }
  },
  "/api/v1/workspaces/{slug}/projects/{project_id}/work-items/{issue_id}/github/pull-requests/{pk}/": {
    "delete": {
      "operationId": "workspaces_projects_work_items_github_pull_requests_destroy",
      "tags": [
        "workspaces"
      ],
      "summary": null,
      "parameters": [
        {
          "in": "path",
          "name": "issue_id",
          "schema": {
            "type": "string",
            "format": "uuid"
          },
          "required": true
        },
        {
          "in": "path",
          "name": "pk",
          "schema": {
            "type": "string",
            "format": "uuid"
          },
          "required": true
        },
        {
          "in": "path",
          "name": "project_id",
          "schema": {
            "type": "string"
          },
          "required": true,
          "description": "Either the project's UUID or its workspace-scoped identifier (e.g. ``ENG``). Identifier matching is case-insensitive.",
          "examples": {
            "uuid": {
              "value": "00000000-0000-0000-0000-000000000000",
              "summary": "UUID form"
            },
            "slug": {
              "value": "ENG",
              "summary": "Workspace-scoped identifier"
            }
          }
        },
        {
          "in": "path",
          "name": "slug",
          "schema": {
            "type": "string"
          },
          "required": true
        }
      ],
      "responses": {
        "204": {
          "description": "No response body"
        }
      }
    }
  },
  "/api/v1/workspaces/{slug}/projects/{project_id}/work-items/{issue_id}/links/": {
    "get": {
      "operationId": "list_work_item_links_2",
      "tags": [
        "Work Item Links"
      ],
      "summary": null,
      "parameters": [
        {
          "in": "query",
          "name": "cursor",
          "schema": {
            "type": "string"
          },
          "description": "Pagination cursor for getting next set of results",
          "examples": {
            "NextPageCursor": {
              "value": "20:1:0",
              "summary": "Next page cursor",
              "description": "Cursor format: 'page_size:page_number:offset'"
            }
          }
        },
        {
          "in": "query",
          "name": "expand",
          "schema": {
            "type": "string"
          },
          "description": "Comma-separated list of related fields to expand in response",
          "examples": {
            "ExpandAssignees": {
              "value": "assignees",
              "summary": "Expand assignees",
              "description": "Include full assignee details"
            },
            "MultipleExpansions": {
              "value": "assignees,labels,state",
              "summary": "Multiple expansions",
              "description": "Include details for multiple relations"
            }
          }
        },
        {
          "in": "query",
          "name": "fields",
          "schema": {
            "type": "string"
          },
          "description": "Comma-separated list of fields to include in response",
          "examples": {
            "BasicFields": {
              "value": "id,name,description",
              "summary": "Basic fields",
              "description": "Include only basic fields"
            },
            "WithRelations": {
              "value": "id,name,assignees,state",
              "summary": "With relations",
              "description": "Include fields with relationships"
            }
          }
        },
        {
          "in": "path",
          "name": "issue_id",
          "schema": {
            "type": "string",
            "format": "uuid"
          },
          "description": "Issue ID",
          "required": true,
          "examples": {
            "ExampleIssueID": {
              "value": "550e8400-e29b-41d4-a716-446655440000",
              "summary": "Example issue ID",
              "description": "A typical issue UUID"
            }
          }
        },
        {
          "in": "query",
          "name": "order_by",
          "schema": {
            "type": "string"
          },
          "description": "Field to order results by. Prefix with '-' for descending order",
          "examples": {
            "CreatedDateDescending": {
              "value": "-created_at",
              "summary": "Created date descending",
              "description": "Most recent items first"
            },
            "PriorityAscending": {
              "value": "priority",
              "summary": "Priority ascending",
              "description": "Order by priority (urgent, high, medium, low, none)"
            },
            "StateGroup": {
              "value": "state__group",
              "summary": "State group",
              "description": "Order by state group (backlog, unstarted, started, review, test, completed, cancelled)"
            },
            "AssigneeName": {
              "value": "assignees__first_name",
              "summary": "Assignee name",
              "description": "Order by assignee first name"
            }
          }
        },
        {
          "in": "query",
          "name": "per_page",
          "schema": {
            "type": "integer"
          },
          "description": "Number of results per page (default: 20, max: 100)",
          "examples": {
            "Default": {
              "value": 20
            },
            "Maximum": {
              "value": 100
            }
          }
        },
        {
          "in": "path",
          "name": "project_id",
          "schema": {
            "type": "string"
          },
          "description": "Either the project's UUID or its workspace-scoped identifier (e.g. ``ENG``). Identifier matching is case-insensitive.",
          "required": true,
          "examples": {
            "uuid": {
              "value": "00000000-0000-0000-0000-000000000000",
              "summary": "UUID form"
            },
            "slug": {
              "value": "ENG",
              "summary": "Workspace-scoped identifier"
            }
          }
        },
        {
          "in": "path",
          "name": "slug",
          "schema": {
            "type": "string"
          },
          "description": "Workspace slug",
          "required": true,
          "examples": {
            "ExampleWorkspace": {
              "value": "my-workspace",
              "summary": "Example workspace",
              "description": "A typical workspace slug"
            }
          }
        }
      ],
      "responses": {
        "401": {
          "description": "Authentication credentials were not provided or are invalid."
        },
        "403": {
          "description": "Permission denied. User lacks required permissions."
        },
        "404": {
          "description": "Issue not found"
        },
        "200": {
          "content": {
            "application/json": {
              "schema": {
                "$ref": "#/components/schemas/PaginatedIssueLinkResponse"
              },
              "examples": {
                "PaginatedWorkItemLinks": {
                  "value": {
                    "grouped_by": "state",
                    "sub_grouped_by": "priority",
                    "total_count": 150,
                    "next_cursor": "20:1:0",
                    "prev_cursor": "20:0:0",
                    "next_page_results": true,
                    "prev_page_results": false,
                    "count": 20,
                    "total_pages": 8,
                    "total_results": 150,
                    "extra_stats": null,
                    "results": [
                      {
                        "id": "550e8400-e29b-41d4-a716-446655440000",
                        "name": "Sample Item",
                        "created_at": "2024-01-15T12:00:00Z"
                      }
                    ]
                  },
                  "summary": "Paginated Work Item Links"
                }
              }
            }
          },
          "description": "Paginated list of work item links"
        },
        "400": {
          "description": "Invalid request data provided"
        }
      }
    },
    "post": {
      "operationId": "create_work_item_link_2",
      "tags": [
        "Work Item Links"
      ],
      "summary": null,
      "parameters": [
        {
          "in": "path",
          "name": "issue_id",
          "schema": {
            "type": "string",
            "format": "uuid"
          },
          "description": "Issue ID",
          "required": true,
          "examples": {
            "ExampleIssueID": {
              "value": "550e8400-e29b-41d4-a716-446655440000",
              "summary": "Example issue ID",
              "description": "A typical issue UUID"
            }
          }
        },
        {
          "in": "path",
          "name": "project_id",
          "schema": {
            "type": "string"
          },
          "description": "Either the project's UUID or its workspace-scoped identifier (e.g. ``ENG``). Identifier matching is case-insensitive.",
          "required": true,
          "examples": {
            "uuid": {
              "value": "00000000-0000-0000-0000-000000000000",
              "summary": "UUID form"
            },
            "slug": {
              "value": "ENG",
              "summary": "Workspace-scoped identifier"
            }
          }
        },
        {
          "in": "path",
          "name": "slug",
          "schema": {
            "type": "string"
          },
          "description": "Workspace slug",
          "required": true,
          "examples": {
            "ExampleWorkspace": {
              "value": "my-workspace",
              "summary": "Example workspace",
              "description": "A typical workspace slug"
            }
          }
        }
      ],
      "responses": {
        "401": {
          "description": "Authentication credentials were not provided or are invalid."
        },
        "403": {
          "description": "Permission denied. User lacks required permissions."
        },
        "404": {
          "description": "Issue not found"
        },
        "201": {
          "content": {
            "application/json": {
              "schema": {
                "$ref": "#/components/schemas/IssueLink"
              },
              "examples": {
                "IssueLink": {
                  "value": {
                    "id": "550e8400-e29b-41d4-a716-446655440000",
                    "url": "https://github.com/example/repo/pull/123",
                    "title": "Fix authentication bug",
                    "metadata": {
                      "title": "Fix authentication bug",
                      "description": "Pull request to fix authentication timeout issue",
                      "image": "https://github.com/example/repo/avatar.png"
                    },
                    "created_at": "2024-01-01T10:30:00Z",
                    "updated_at": "2024-01-10T15:45:00Z"
                  }
                }
              }
            }
          },
          "description": "Work item link created successfully"
        },
        "400": {
          "description": "Invalid request data provided"
        }
      }
    }
  },
  "/api/v1/workspaces/{slug}/projects/{project_id}/work-items/{issue_id}/links/{pk}/": {
    "delete": {
      "operationId": "delete_work_item_link_2",
      "tags": [
        "Work Item Links"
      ],
      "summary": null,
      "parameters": [
        {
          "in": "path",
          "name": "issue_id",
          "schema": {
            "type": "string",
            "format": "uuid"
          },
          "description": "Issue ID",
          "required": true,
          "examples": {
            "ExampleIssueID": {
              "value": "550e8400-e29b-41d4-a716-446655440000",
              "summary": "Example issue ID",
              "description": "A typical issue UUID"
            }
          }
        },
        {
          "in": "path",
          "name": "pk",
          "schema": {
            "type": "string",
            "format": "uuid"
          },
          "description": "Link ID",
          "required": true,
          "examples": {
            "ExampleLinkID": {
              "value": "550e8400-e29b-41d4-a716-446655440000",
              "summary": "Example link ID",
              "description": "A typical link UUID"
            }
          }
        },
        {
          "in": "path",
          "name": "project_id",
          "schema": {
            "type": "string"
          },
          "description": "Either the project's UUID or its workspace-scoped identifier (e.g. ``ENG``). Identifier matching is case-insensitive.",
          "required": true,
          "examples": {
            "uuid": {
              "value": "00000000-0000-0000-0000-000000000000",
              "summary": "UUID form"
            },
            "slug": {
              "value": "ENG",
              "summary": "Workspace-scoped identifier"
            }
          }
        },
        {
          "in": "path",
          "name": "slug",
          "schema": {
            "type": "string"
          },
          "description": "Workspace slug",
          "required": true,
          "examples": {
            "ExampleWorkspace": {
              "value": "my-workspace",
              "summary": "Example workspace",
              "description": "A typical workspace slug"
            }
          }
        }
      ],
      "responses": {
        "401": {
          "description": "Authentication credentials were not provided or are invalid."
        },
        "403": {
          "description": "Permission denied. User lacks required permissions."
        },
        "404": {
          "description": "Work item link not found"
        },
        "204": {
          "description": "Work item link deleted successfully"
        }
      }
    },
    "get": {
      "operationId": "retrieve_work_item_link_2",
      "tags": [
        "Work Item Links"
      ],
      "summary": null,
      "parameters": [
        {
          "in": "query",
          "name": "cursor",
          "schema": {
            "type": "string"
          },
          "description": "Pagination cursor for getting next set of results",
          "examples": {
            "NextPageCursor": {
              "value": "20:1:0",
              "summary": "Next page cursor",
              "description": "Cursor format: 'page_size:page_number:offset'"
            }
          }
        },
        {
          "in": "query",
          "name": "expand",
          "schema": {
            "type": "string"
          },
          "description": "Comma-separated list of related fields to expand in response",
          "examples": {
            "ExpandAssignees": {
              "value": "assignees",
              "summary": "Expand assignees",
              "description": "Include full assignee details"
            },
            "MultipleExpansions": {
              "value": "assignees,labels,state",
              "summary": "Multiple expansions",
              "description": "Include details for multiple relations"
            }
          }
        },
        {
          "in": "query",
          "name": "fields",
          "schema": {
            "type": "string"
          },
          "description": "Comma-separated list of fields to include in response",
          "examples": {
            "BasicFields": {
              "value": "id,name,description",
              "summary": "Basic fields",
              "description": "Include only basic fields"
            },
            "WithRelations": {
              "value": "id,name,assignees,state",
              "summary": "With relations",
              "description": "Include fields with relationships"
            }
          }
        },
        {
          "in": "path",
          "name": "issue_id",
          "schema": {
            "type": "string",
            "format": "uuid"
          },
          "description": "Issue ID",
          "required": true,
          "examples": {
            "ExampleIssueID": {
              "value": "550e8400-e29b-41d4-a716-446655440000",
              "summary": "Example issue ID",
              "description": "A typical issue UUID"
            }
          }
        },
        {
          "in": "query",
          "name": "per_page",
          "schema": {
            "type": "integer"
          },
          "description": "Number of results per page (default: 20, max: 100)",
          "examples": {
            "Default": {
              "value": 20
            },
            "Maximum": {
              "value": 100
            }
          }
        },
        {
          "in": "path",
          "name": "pk",
          "schema": {
            "type": "string",
            "format": "uuid"
          },
          "description": "Link ID",
          "required": true,
          "examples": {
            "ExampleLinkID": {
              "value": "550e8400-e29b-41d4-a716-446655440000",
              "summary": "Example link ID",
              "description": "A typical link UUID"
            }
          }
        },
        {
          "in": "path",
          "name": "project_id",
          "schema": {
            "type": "string"
          },
          "description": "Either the project's UUID or its workspace-scoped identifier (e.g. ``ENG``). Identifier matching is case-insensitive.",
          "required": true,
          "examples": {
            "uuid": {
              "value": "00000000-0000-0000-0000-000000000000",
              "summary": "UUID form"
            },
            "slug": {
              "value": "ENG",
              "summary": "Workspace-scoped identifier"
            }
          }
        },
        {
          "in": "path",
          "name": "slug",
          "schema": {
            "type": "string"
          },
          "description": "Workspace slug",
          "required": true,
          "examples": {
            "ExampleWorkspace": {
              "value": "my-workspace",
              "summary": "Example workspace",
              "description": "A typical workspace slug"
            }
          }
        }
      ],
      "responses": {
        "401": {
          "description": "Authentication credentials were not provided or are invalid."
        },
        "403": {
          "description": "Permission denied. User lacks required permissions."
        },
        "404": {
          "description": "Issue not found"
        },
        "200": {
          "content": {
            "application/json": {
              "schema": {
                "$ref": "#/components/schemas/PaginatedIssueLinkDetailResponse"
              },
              "examples": {
                "WorkItemLinkDetails": {
                  "value": {
                    "grouped_by": "state",
                    "sub_grouped_by": "priority",
                    "total_count": 150,
                    "next_cursor": "20:1:0",
                    "prev_cursor": "20:0:0",
                    "next_page_results": true,
                    "prev_page_results": false,
                    "count": 20,
                    "total_pages": 8,
                    "total_results": 150,
                    "extra_stats": null,
                    "results": [
                      {
                        "id": "550e8400-e29b-41d4-a716-446655440000",
                        "name": "Sample Item",
                        "created_at": "2024-01-15T12:00:00Z"
                      }
                    ]
                  },
                  "summary": "Work Item Link Details"
                }
              }
            }
          },
          "description": "Work item link details or paginated list"
        }
      }
    },
    "patch": {
      "operationId": "update_issue_link_2",
      "tags": [
        "Work Item Links"
      ],
      "summary": null,
      "parameters": [
        {
          "in": "path",
          "name": "issue_id",
          "schema": {
            "type": "string",
            "format": "uuid"
          },
          "description": "Issue ID",
          "required": true,
          "examples": {
            "ExampleIssueID": {
              "value": "550e8400-e29b-41d4-a716-446655440000",
              "summary": "Example issue ID",
              "description": "A typical issue UUID"
            }
          }
        },
        {
          "in": "path",
          "name": "pk",
          "schema": {
            "type": "string",
            "format": "uuid"
          },
          "description": "Link ID",
          "required": true,
          "examples": {
            "ExampleLinkID": {
              "value": "550e8400-e29b-41d4-a716-446655440000",
              "summary": "Example link ID",
              "description": "A typical link UUID"
            }
          }
        },
        {
          "in": "path",
          "name": "project_id",
          "schema": {
            "type": "string"
          },
          "description": "Either the project's UUID or its workspace-scoped identifier (e.g. ``ENG``). Identifier matching is case-insensitive.",
          "required": true,
          "examples": {
            "uuid": {
              "value": "00000000-0000-0000-0000-000000000000",
              "summary": "UUID form"
            },
            "slug": {
              "value": "ENG",
              "summary": "Workspace-scoped identifier"
            }
          }
        },
        {
          "in": "path",
          "name": "slug",
          "schema": {
            "type": "string"
          },
          "description": "Workspace slug",
          "required": true,
          "examples": {
            "ExampleWorkspace": {
              "value": "my-workspace",
              "summary": "Example workspace",
              "description": "A typical workspace slug"
            }
          }
        }
      ],
      "responses": {
        "401": {
          "description": "Authentication credentials were not provided or are invalid."
        },
        "403": {
          "description": "Permission denied. User lacks required permissions."
        },
        "404": {
          "description": "Link not found"
        },
        "200": {
          "content": {
            "application/json": {
              "schema": {
                "$ref": "#/components/schemas/IssueLink"
              },
              "examples": {
                "IssueLink": {
                  "value": {
                    "id": "550e8400-e29b-41d4-a716-446655440000",
                    "url": "https://github.com/example/repo/pull/123",
                    "title": "Fix authentication bug",
                    "metadata": {
                      "title": "Fix authentication bug",
                      "description": "Pull request to fix authentication timeout issue",
                      "image": "https://github.com/example/repo/avatar.png"
                    },
                    "created_at": "2024-01-01T10:30:00Z",
                    "updated_at": "2024-01-10T15:45:00Z"
                  }
                }
              }
            }
          },
          "description": "Issue link updated successfully"
        },
        "400": {
          "description": "Invalid request data provided"
        }
      }
    }
  },
  "/api/v1/workspaces/{slug}/projects/{project_id}/work-items/{issue_id}/relations/": {
    "get": {
      "operationId": "list_work_item_relations",
      "tags": [
        "Work Item Relations"
      ],
      "summary": "List work item relations",
      "parameters": [
        {
          "in": "query",
          "name": "cursor",
          "schema": {
            "type": "string"
          },
          "description": "Pagination cursor for getting next set of results",
          "examples": {
            "NextPageCursor": {
              "value": "20:1:0",
              "summary": "Next page cursor",
              "description": "Cursor format: 'page_size:page_number:offset'"
            }
          }
        },
        {
          "in": "query",
          "name": "expand",
          "schema": {
            "type": "string"
          },
          "description": "Comma-separated list of related fields to expand in response",
          "examples": {
            "ExpandAssignees": {
              "value": "assignees",
              "summary": "Expand assignees",
              "description": "Include full assignee details"
            },
            "MultipleExpansions": {
              "value": "assignees,labels,state",
              "summary": "Multiple expansions",
              "description": "Include details for multiple relations"
            }
          }
        },
        {
          "in": "query",
          "name": "fields",
          "schema": {
            "type": "string"
          },
          "description": "Comma-separated list of fields to include in response",
          "examples": {
            "BasicFields": {
              "value": "id,name,description",
              "summary": "Basic fields",
              "description": "Include only basic fields"
            },
            "WithRelations": {
              "value": "id,name,assignees,state",
              "summary": "With relations",
              "description": "Include fields with relationships"
            }
          }
        },
        {
          "in": "path",
          "name": "issue_id",
          "schema": {
            "type": "string",
            "format": "uuid"
          },
          "description": "Issue ID",
          "required": true,
          "examples": {
            "ExampleIssueID": {
              "value": "550e8400-e29b-41d4-a716-446655440000",
              "summary": "Example issue ID",
              "description": "A typical issue UUID"
            }
          }
        },
        {
          "in": "query",
          "name": "order_by",
          "schema": {
            "type": "string"
          },
          "description": "Field to order results by. Prefix with '-' for descending order",
          "examples": {
            "CreatedDateDescending": {
              "value": "-created_at",
              "summary": "Created date descending",
              "description": "Most recent items first"
            },
            "PriorityAscending": {
              "value": "priority",
              "summary": "Priority ascending",
              "description": "Order by priority (urgent, high, medium, low, none)"
            },
            "StateGroup": {
              "value": "state__group",
              "summary": "State group",
              "description": "Order by state group (backlog, unstarted, started, review, test, completed, cancelled)"
            },
            "AssigneeName": {
              "value": "assignees__first_name",
              "summary": "Assignee name",
              "description": "Order by assignee first name"
            }
          }
        },
        {
          "in": "query",
          "name": "per_page",
          "schema": {
            "type": "integer"
          },
          "description": "Number of results per page (default: 20, max: 100)",
          "examples": {
            "Default": {
              "value": 20
            },
            "Maximum": {
              "value": 100
            }
          }
        },
        {
          "in": "path",
          "name": "project_id",
          "schema": {
            "type": "string"
          },
          "description": "Either the project's UUID or its workspace-scoped identifier (e.g. ``ENG``). Identifier matching is case-insensitive.",
          "required": true,
          "examples": {
            "uuid": {
              "value": "00000000-0000-0000-0000-000000000000",
              "summary": "UUID form"
            },
            "slug": {
              "value": "ENG",
              "summary": "Workspace-scoped identifier"
            }
          }
        },
        {
          "in": "path",
          "name": "slug",
          "schema": {
            "type": "string"
          },
          "description": "Workspace slug",
          "required": true,
          "examples": {
            "ExampleWorkspace": {
              "value": "my-workspace",
              "summary": "Example workspace",
              "description": "A typical workspace slug"
            }
          }
        }
      ],
      "responses": {
        "401": {
          "description": "Authentication credentials were not provided or are invalid."
        },
        "403": {
          "description": "Permission denied. User lacks required permissions."
        },
        "404": {
          "description": "Issue not found"
        },
        "200": {
          "content": {
            "application/json": {
              "schema": {
                "$ref": "#/components/schemas/IssueRelationResponse"
              },
              "examples": {
                "WorkItemRelationsResponse": {
                  "value": {
                    "blocking": [
                      "550e8400-e29b-41d4-a716-446655440000",
                      "550e8400-e29b-41d4-a716-446655440001"
                    ],
                    "blocked_by": [
                      "550e8400-e29b-41d4-a716-446655440002"
                    ],
                    "duplicate": [],
                    "relates_to": [
                      "550e8400-e29b-41d4-a716-446655440003"
                    ],
                    "start_after": [],
                    "start_before": [
                      "550e8400-e29b-41d4-a716-446655440004"
                    ],
                    "finish_after": [],
                    "finish_before": []
                  },
                  "summary": "Work Item Relations Response"
                }
              }
            }
          },
          "description": "Work item relations grouped by relation type"
        },
        "400": {
          "description": "Invalid request data provided"
        }
      }
    },
    "post": {
      "operationId": "create_work_item_relation",
      "tags": [
        "Work Item Relations"
      ],
      "summary": "Create work item relation",
      "parameters": [
        {
          "in": "path",
          "name": "issue_id",
          "schema": {
            "type": "string",
            "format": "uuid"
          },
          "description": "Issue ID",
          "required": true,
          "examples": {
            "ExampleIssueID": {
              "value": "550e8400-e29b-41d4-a716-446655440000",
              "summary": "Example issue ID",
              "description": "A typical issue UUID"
            }
          }
        },
        {
          "in": "path",
          "name": "project_id",
          "schema": {
            "type": "string"
          },
          "description": "Either the project's UUID or its workspace-scoped identifier (e.g. ``ENG``). Identifier matching is case-insensitive.",
          "required": true,
          "examples": {
            "uuid": {
              "value": "00000000-0000-0000-0000-000000000000",
              "summary": "UUID form"
            },
            "slug": {
              "value": "ENG",
              "summary": "Workspace-scoped identifier"
            }
          }
        },
        {
          "in": "path",
          "name": "slug",
          "schema": {
            "type": "string"
          },
          "description": "Workspace slug",
          "required": true,
          "examples": {
            "ExampleWorkspace": {
              "value": "my-workspace",
              "summary": "Example workspace",
              "description": "A typical workspace slug"
            }
          }
        }
      ],
      "responses": {
        "401": {
          "description": "Authentication credentials were not provided or are invalid."
        },
        "403": {
          "description": "Permission denied. User lacks required permissions."
        },
        "404": {
          "description": "Issue not found"
        },
        "201": {
          "content": {
            "application/json": {
              "schema": {
                "type": "array",
                "items": {
                  "$ref": "#/components/schemas/IssueRelation"
                }
              },
              "examples": {
                "RelationsCreated": {
                  "value": [
                    [
                      {
                        "id": "550e8400-e29b-41d4-a716-446655440000",
                        "name": "Fix authentication bug",
                        "sequence_id": 42,
                        "project_id": "550e8400-e29b-41d4-a716-446655440001",
                        "relation_type": "blocked_by",
                        "state_id": "550e8400-e29b-41d4-a716-446655440002",
                        "priority": "high",
                        "created_at": "2024-01-15T10:00:00Z",
                        "updated_at": "2024-01-15T10:00:00Z",
                        "created_by": "550e8400-e29b-41d4-a716-446655440004",
                        "updated_by": "550e8400-e29b-41d4-a716-446655440004"
                      }
                    ]
                  ],
                  "summary": "Relations created"
                }
              }
            }
          },
          "description": "Work item relations created successfully"
        },
        "400": {
          "description": "Invalid request data provided"
        }
      }
    }
  },
  "/api/v1/workspaces/{slug}/projects/{project_id}/work-items/{issue_id}/relations/grouped/": {
    "get": {
      "operationId": "list_work_item_relations_grouped",
      "tags": [
        "Work Item Relations"
      ],
      "summary": "List work item relations with details",
      "parameters": [
        {
          "in": "path",
          "name": "issue_id",
          "schema": {
            "type": "string",
            "format": "uuid"
          },
          "description": "Issue ID",
          "required": true,
          "examples": {
            "ExampleIssueID": {
              "value": "550e8400-e29b-41d4-a716-446655440000",
              "summary": "Example issue ID",
              "description": "A typical issue UUID"
            }
          }
        },
        {
          "in": "path",
          "name": "project_id",
          "schema": {
            "type": "string"
          },
          "description": "Either the project's UUID or its workspace-scoped identifier (e.g. ``ENG``). Identifier matching is case-insensitive.",
          "required": true,
          "examples": {
            "uuid": {
              "value": "00000000-0000-0000-0000-000000000000",
              "summary": "UUID form"
            },
            "slug": {
              "value": "ENG",
              "summary": "Workspace-scoped identifier"
            }
          }
        },
        {
          "in": "path",
          "name": "slug",
          "schema": {
            "type": "string"
          },
          "description": "Workspace slug",
          "required": true,
          "examples": {
            "ExampleWorkspace": {
              "value": "my-workspace",
              "summary": "Example workspace",
              "description": "A typical workspace slug"
            }
          }
        }
      ],
      "responses": {
        "401": {
          "description": "Authentication credentials were not provided or are invalid."
        },
        "403": {
          "description": "Permission denied. User lacks required permissions."
        },
        "404": {
          "description": "Issue not found"
        },
        "200": {
          "description": "Grouped relations"
        }
      }
    }
  },
  "/api/v1/workspaces/{slug}/projects/{project_id}/work-items/{issue_id}/relations/relate/": {
    "post": {
      "operationId": "relate_work_items",
      "tags": [
        "Work Item Relations"
      ],
      "summary": "Relate work items (idempotent)",
      "parameters": [
        {
          "in": "path",
          "name": "issue_id",
          "schema": {
            "type": "string",
            "format": "uuid"
          },
          "description": "Issue ID",
          "required": true,
          "examples": {
            "ExampleIssueID": {
              "value": "550e8400-e29b-41d4-a716-446655440000",
              "summary": "Example issue ID",
              "description": "A typical issue UUID"
            }
          }
        },
        {
          "in": "path",
          "name": "project_id",
          "schema": {
            "type": "string"
          },
          "description": "Either the project's UUID or its workspace-scoped identifier (e.g. ``ENG``). Identifier matching is case-insensitive.",
          "required": true,
          "examples": {
            "uuid": {
              "value": "00000000-0000-0000-0000-000000000000",
              "summary": "UUID form"
            },
            "slug": {
              "value": "ENG",
              "summary": "Workspace-scoped identifier"
            }
          }
        },
        {
          "in": "path",
          "name": "slug",
          "schema": {
            "type": "string"
          },
          "description": "Workspace slug",
          "required": true,
          "examples": {
            "ExampleWorkspace": {
              "value": "my-workspace",
              "summary": "Example workspace",
              "description": "A typical workspace slug"
            }
          }
        }
      ],
      "responses": {
        "401": {
          "description": "Authentication credentials were not provided or are invalid."
        },
        "403": {
          "description": "Permission denied. User lacks required permissions."
        },
        "404": {
          "description": "Issue not found"
        },
        "200": {
          "description": "created / unchanged / conflicts plus grouped relations"
        },
        "400": {
          "description": "Invalid request data provided"
        }
      }
    }
  },
  "/api/v1/workspaces/{slug}/projects/{project_id}/work-items/{issue_id}/relations/unrelate/": {
    "post": {
      "operationId": "unrelate_work_items",
      "tags": [
        "Work Item Relations"
      ],
      "summary": "Remove work item relations (idempotent)",
      "parameters": [
        {
          "in": "path",
          "name": "issue_id",
          "schema": {
            "type": "string",
            "format": "uuid"
          },
          "description": "Issue ID",
          "required": true,
          "examples": {
            "ExampleIssueID": {
              "value": "550e8400-e29b-41d4-a716-446655440000",
              "summary": "Example issue ID",
              "description": "A typical issue UUID"
            }
          }
        },
        {
          "in": "path",
          "name": "project_id",
          "schema": {
            "type": "string"
          },
          "description": "Either the project's UUID or its workspace-scoped identifier (e.g. ``ENG``). Identifier matching is case-insensitive.",
          "required": true,
          "examples": {
            "uuid": {
              "value": "00000000-0000-0000-0000-000000000000",
              "summary": "UUID form"
            },
            "slug": {
              "value": "ENG",
              "summary": "Workspace-scoped identifier"
            }
          }
        },
        {
          "in": "path",
          "name": "slug",
          "schema": {
            "type": "string"
          },
          "description": "Workspace slug",
          "required": true,
          "examples": {
            "ExampleWorkspace": {
              "value": "my-workspace",
              "summary": "Example workspace",
              "description": "A typical workspace slug"
            }
          }
        }
      ],
      "responses": {
        "401": {
          "description": "Authentication credentials were not provided or are invalid."
        },
        "403": {
          "description": "Permission denied. User lacks required permissions."
        },
        "404": {
          "description": "Issue not found"
        },
        "200": {
          "description": "removed / not_related plus grouped relations"
        },
        "400": {
          "description": "Invalid request data provided"
        }
      }
    }
  },
  "/api/v1/workspaces/{slug}/projects/{project_id}/work-items/{issue_id}/workpad/": {
    "get": {
      "operationId": "workspaces_projects_work_items_workpad_retrieve",
      "tags": [
        "workspaces"
      ],
      "summary": null,
      "parameters": [
        {
          "in": "path",
          "name": "issue_id",
          "schema": {
            "type": "string",
            "format": "uuid"
          },
          "required": true
        },
        {
          "in": "path",
          "name": "project_id",
          "schema": {
            "type": "string"
          },
          "required": true,
          "description": "Either the project's UUID or its workspace-scoped identifier (e.g. ``ENG``). Identifier matching is case-insensitive.",
          "examples": {
            "uuid": {
              "value": "00000000-0000-0000-0000-000000000000",
              "summary": "UUID form"
            },
            "slug": {
              "value": "ENG",
              "summary": "Workspace-scoped identifier"
            }
          }
        },
        {
          "in": "path",
          "name": "slug",
          "schema": {
            "type": "string"
          },
          "required": true
        }
      ],
      "responses": {
        "200": {
          "content": {
            "application/json": {
              "schema": {
                "$ref": "#/components/schemas/IssueWorkpad"
              }
            }
          },
          "description": ""
        }
      }
    },
    "patch": {
      "operationId": "workspaces_projects_work_items_workpad_partial_update",
      "tags": [
        "workspaces"
      ],
      "summary": null,
      "parameters": [
        {
          "in": "path",
          "name": "issue_id",
          "schema": {
            "type": "string",
            "format": "uuid"
          },
          "required": true
        },
        {
          "in": "path",
          "name": "project_id",
          "schema": {
            "type": "string"
          },
          "required": true,
          "description": "Either the project's UUID or its workspace-scoped identifier (e.g. ``ENG``). Identifier matching is case-insensitive.",
          "examples": {
            "uuid": {
              "value": "00000000-0000-0000-0000-000000000000",
              "summary": "UUID form"
            },
            "slug": {
              "value": "ENG",
              "summary": "Workspace-scoped identifier"
            }
          }
        },
        {
          "in": "path",
          "name": "slug",
          "schema": {
            "type": "string"
          },
          "required": true
        }
      ],
      "responses": {
        "200": {
          "content": {
            "application/json": {
              "schema": {
                "$ref": "#/components/schemas/IssueWorkpad"
              }
            }
          },
          "description": ""
        }
      }
    }
  },
  "/api/v1/workspaces/{slug}/projects/{project_id}/work-items/{pk}/": {
    "delete": {
      "operationId": "delete_work_item_2",
      "tags": [
        "Work Items"
      ],
      "summary": "Delete work item",
      "parameters": [
        {
          "in": "path",
          "name": "pk",
          "schema": {
            "type": "string",
            "format": "uuid"
          },
          "required": true
        },
        {
          "in": "path",
          "name": "project_id",
          "schema": {
            "type": "string"
          },
          "description": "Either the project's UUID or its workspace-scoped identifier (e.g. ``ENG``). Identifier matching is case-insensitive.",
          "required": true,
          "examples": {
            "uuid": {
              "value": "00000000-0000-0000-0000-000000000000",
              "summary": "UUID form"
            },
            "slug": {
              "value": "ENG",
              "summary": "Workspace-scoped identifier"
            }
          }
        },
        {
          "in": "path",
          "name": "slug",
          "schema": {
            "type": "string"
          },
          "description": "Workspace slug",
          "required": true,
          "examples": {
            "ExampleWorkspace": {
              "value": "my-workspace",
              "summary": "Example workspace",
              "description": "A typical workspace slug"
            }
          }
        }
      ],
      "responses": {
        "401": {
          "description": "Authentication credentials were not provided or are invalid."
        },
        "403": {
          "description": "Only admin or creator can perform this action"
        },
        "404": {
          "description": "Work item not found"
        },
        "204": {
          "description": "Resource deleted successfully"
        }
      }
    },
    "get": {
      "operationId": "retrieve_work_item_2",
      "tags": [
        "Work Items"
      ],
      "summary": "Retrieve work item",
      "parameters": [
        {
          "in": "query",
          "name": "expand",
          "schema": {
            "type": "string"
          },
          "description": "Comma-separated list of related fields to expand in response",
          "examples": {
            "ExpandAssignees": {
              "value": "assignees",
              "summary": "Expand assignees",
              "description": "Include full assignee details"
            },
            "MultipleExpansions": {
              "value": "assignees,labels,state",
              "summary": "Multiple expansions",
              "description": "Include details for multiple relations"
            }
          }
        },
        {
          "in": "query",
          "name": "external_id",
          "schema": {
            "type": "string"
          },
          "description": "External system identifier for filtering or lookup",
          "examples": {
            "GitHubIssue": {
              "value": "1234567890",
              "summary": "GitHub Issue",
              "description": "GitHub issue number"
            }
          }
        },
        {
          "in": "query",
          "name": "external_source",
          "schema": {
            "type": "string"
          },
          "description": "External system source name for filtering or lookup",
          "examples": {
            "GitHub": {
              "value": "github",
              "description": "GitHub integration source"
            },
            "Jira": {
              "value": "jira",
              "description": "Jira integration source"
            }
          }
        },
        {
          "in": "query",
          "name": "fields",
          "schema": {
            "type": "string"
          },
          "description": "Comma-separated list of fields to include in response",
          "examples": {
            "BasicFields": {
              "value": "id,name,description",
              "summary": "Basic fields",
              "description": "Include only basic fields"
            },
            "WithRelations": {
              "value": "id,name,assignees,state",
              "summary": "With relations",
              "description": "Include fields with relationships"
            }
          }
        },
        {
          "in": "query",
          "name": "order_by",
          "schema": {
            "type": "string"
          },
          "description": "Field to order results by. Prefix with '-' for descending order",
          "examples": {
            "CreatedDateDescending": {
              "value": "-created_at",
              "summary": "Created date descending",
              "description": "Most recent items first"
            },
            "PriorityAscending": {
              "value": "priority",
              "summary": "Priority ascending",
              "description": "Order by priority (urgent, high, medium, low, none)"
            },
            "StateGroup": {
              "value": "state__group",
              "summary": "State group",
              "description": "Order by state group (backlog, unstarted, started, review, test, completed, cancelled)"
            },
            "AssigneeName": {
              "value": "assignees__first_name",
              "summary": "Assignee name",
              "description": "Order by assignee first name"
            }
          }
        },
        {
          "in": "path",
          "name": "pk",
          "schema": {
            "type": "string",
            "format": "uuid"
          },
          "required": true
        },
        {
          "in": "path",
          "name": "project_id",
          "schema": {
            "type": "string"
          },
          "description": "Either the project's UUID or its workspace-scoped identifier (e.g. ``ENG``). Identifier matching is case-insensitive.",
          "required": true,
          "examples": {
            "uuid": {
              "value": "00000000-0000-0000-0000-000000000000",
              "summary": "UUID form"
            },
            "slug": {
              "value": "ENG",
              "summary": "Workspace-scoped identifier"
            }
          }
        },
        {
          "in": "path",
          "name": "slug",
          "schema": {
            "type": "string"
          },
          "description": "Workspace slug",
          "required": true,
          "examples": {
            "ExampleWorkspace": {
              "value": "my-workspace",
              "summary": "Example workspace",
              "description": "A typical workspace slug"
            }
          }
        }
      ],
      "responses": {
        "401": {
          "description": "Authentication credentials were not provided or are invalid."
        },
        "403": {
          "description": "Permission denied. User lacks required permissions."
        },
        "404": {
          "description": "Work item not found"
        },
        "200": {
          "content": {
            "application/json": {
              "schema": {
                "$ref": "#/components/schemas/Issue"
              },
              "examples": {
                "Issue": {
                  "value": {
                    "id": "550e8400-e29b-41d4-a716-446655440000",
                    "name": "Implement user authentication",
                    "description": "Add OAuth 2.0 authentication flow",
                    "sequence_id": 1,
                    "priority": "high",
                    "assignees": [
                      "550e8400-e29b-41d4-a716-446655440001"
                    ],
                    "labels": [
                      "550e8400-e29b-41d4-a716-446655440002"
                    ],
                    "created_at": "2024-01-15T10:30:00Z",
                    "updated_at": "2024-01-15T10:30:00Z"
                  }
                }
              }
            }
          },
          "description": "List of issues or issue details"
        },
        "400": {
          "description": "Invalid request data provided"
        }
      }
    },
    "patch": {
      "operationId": "update_work_item_2",
      "tags": [
        "Work Items"
      ],
      "summary": "Partially update work item",
      "parameters": [
        {
          "in": "path",
          "name": "pk",
          "schema": {
            "type": "string",
            "format": "uuid"
          },
          "required": true
        },
        {
          "in": "path",
          "name": "project_id",
          "schema": {
            "type": "string"
          },
          "description": "Either the project's UUID or its workspace-scoped identifier (e.g. ``ENG``). Identifier matching is case-insensitive.",
          "required": true,
          "examples": {
            "uuid": {
              "value": "00000000-0000-0000-0000-000000000000",
              "summary": "UUID form"
            },
            "slug": {
              "value": "ENG",
              "summary": "Workspace-scoped identifier"
            }
          }
        },
        {
          "in": "path",
          "name": "slug",
          "schema": {
            "type": "string"
          },
          "description": "Workspace slug",
          "required": true,
          "examples": {
            "ExampleWorkspace": {
              "value": "my-workspace",
              "summary": "Example workspace",
              "description": "A typical workspace slug"
            }
          }
        }
      ],
      "responses": {
        "401": {
          "description": "Authentication credentials were not provided or are invalid."
        },
        "403": {
          "description": "Permission denied. User lacks required permissions."
        },
        "404": {
          "description": "Work item not found"
        },
        "200": {
          "content": {
            "application/json": {
              "schema": {
                "$ref": "#/components/schemas/Issue"
              },
              "examples": {
                "Issue": {
                  "value": {
                    "id": "550e8400-e29b-41d4-a716-446655440000",
                    "name": "Implement user authentication",
                    "description": "Add OAuth 2.0 authentication flow",
                    "sequence_id": 1,
                    "priority": "high",
                    "assignees": [
                      "550e8400-e29b-41d4-a716-446655440001"
                    ],
                    "labels": [
                      "550e8400-e29b-41d4-a716-446655440002"
                    ],
                    "created_at": "2024-01-15T10:30:00Z",
                    "updated_at": "2024-01-15T10:30:00Z"
                  }
                }
              }
            }
          },
          "description": "Work Item patched successfully"
        },
        "400": {
          "description": "Invalid request data provided"
        },
        "409": {
          "description": "Resource with same external ID already exists"
        }
      }
    }
  },
  "/api/v1/workspaces/{slug}/projects/{project_id}/work-items/{pk}/move/": {
    "post": {
      "operationId": "workspaces_projects_work_items_move_create",
      "tags": [
        "workspaces"
      ],
      "summary": null,
      "parameters": [
        {
          "in": "path",
          "name": "pk",
          "schema": {
            "type": "string",
            "format": "uuid"
          },
          "required": true
        },
        {
          "in": "path",
          "name": "project_id",
          "schema": {
            "type": "string"
          },
          "required": true,
          "description": "Either the project's UUID or its workspace-scoped identifier (e.g. ``ENG``). Identifier matching is case-insensitive.",
          "examples": {
            "uuid": {
              "value": "00000000-0000-0000-0000-000000000000",
              "summary": "UUID form"
            },
            "slug": {
              "value": "ENG",
              "summary": "Workspace-scoped identifier"
            }
          }
        },
        {
          "in": "path",
          "name": "slug",
          "schema": {
            "type": "string"
          },
          "required": true
        }
      ],
      "responses": {
        "200": {
          "content": {
            "application/json": {
              "schema": {
                "$ref": "#/components/schemas/Issue"
              }
            }
          },
          "description": ""
        }
      }
    }
  },
  "/api/v1/workspaces/{slug}/projects/{project_id}/work-items/{pk}/re-tick/": {
    "post": {
      "operationId": "workspaces_projects_work_items_re_tick_create",
      "tags": [
        "workspaces"
      ],
      "summary": null,
      "parameters": [
        {
          "in": "path",
          "name": "pk",
          "schema": {
            "type": "string",
            "format": "uuid"
          },
          "required": true
        },
        {
          "in": "path",
          "name": "project_id",
          "schema": {
            "type": "string"
          },
          "required": true,
          "description": "Either the project's UUID or its workspace-scoped identifier (e.g. ``ENG``). Identifier matching is case-insensitive.",
          "examples": {
            "uuid": {
              "value": "00000000-0000-0000-0000-000000000000",
              "summary": "UUID form"
            },
            "slug": {
              "value": "ENG",
              "summary": "Workspace-scoped identifier"
            }
          }
        },
        {
          "in": "path",
          "name": "slug",
          "schema": {
            "type": "string"
          },
          "required": true
        }
      ],
      "responses": {
        "200": {
          "description": "No response body"
        }
      }
    }
  },
  "/api/v1/workspaces/{slug}/projects/{project_id}/work-items/{pk}/run-ai/": {
    "post": {
      "operationId": "run_ai_work_item",
      "tags": [
        "Work Items"
      ],
      "summary": "Run AI on a work item",
      "parameters": [
        {
          "in": "path",
          "name": "issue_id",
          "schema": {
            "type": "string",
            "format": "uuid"
          },
          "description": "Issue ID",
          "required": true,
          "examples": {
            "ExampleIssueID": {
              "value": "550e8400-e29b-41d4-a716-446655440000",
              "summary": "Example issue ID",
              "description": "A typical issue UUID"
            }
          }
        },
        {
          "in": "path",
          "name": "pk",
          "schema": {
            "type": "string",
            "format": "uuid"
          },
          "required": true
        },
        {
          "in": "path",
          "name": "project_id",
          "schema": {
            "type": "string"
          },
          "description": "Either the project's UUID or its workspace-scoped identifier (e.g. ``ENG``). Identifier matching is case-insensitive.",
          "required": true,
          "examples": {
            "uuid": {
              "value": "00000000-0000-0000-0000-000000000000",
              "summary": "UUID form"
            },
            "slug": {
              "value": "ENG",
              "summary": "Workspace-scoped identifier"
            }
          }
        },
        {
          "in": "path",
          "name": "slug",
          "schema": {
            "type": "string"
          },
          "description": "Workspace slug",
          "required": true,
          "examples": {
            "ExampleWorkspace": {
              "value": "my-workspace",
              "summary": "Example workspace",
              "description": "A typical workspace slug"
            }
          }
        }
      ],
      "responses": {
        "201": {
          "description": "Run dispatched"
        },
        "403": {
          "description": "Permission denied. User lacks required permissions."
        },
        "404": {
          "description": "Work item not found"
        },
        "409": {
          "description": "No run could be dispatched (see `reason`)"
        }
      }
    }
  },
  "/api/v1/workspaces/{slug}/projects/{project_id}/work-items/{pk}/wait/": {
    "post": {
      "operationId": "wait_work_item",
      "tags": [
        "Work Items"
      ],
      "summary": "Wait on a blocker",
      "parameters": [
        {
          "in": "path",
          "name": "issue_id",
          "schema": {
            "type": "string",
            "format": "uuid"
          },
          "description": "Issue ID",
          "required": true,
          "examples": {
            "ExampleIssueID": {
              "value": "550e8400-e29b-41d4-a716-446655440000",
              "summary": "Example issue ID",
              "description": "A typical issue UUID"
            }
          }
        },
        {
          "in": "path",
          "name": "pk",
          "schema": {
            "type": "string",
            "format": "uuid"
          },
          "required": true
        },
        {
          "in": "path",
          "name": "project_id",
          "schema": {
            "type": "string"
          },
          "description": "Either the project's UUID or its workspace-scoped identifier (e.g. ``ENG``). Identifier matching is case-insensitive.",
          "required": true,
          "examples": {
            "uuid": {
              "value": "00000000-0000-0000-0000-000000000000",
              "summary": "UUID form"
            },
            "slug": {
              "value": "ENG",
              "summary": "Workspace-scoped identifier"
            }
          }
        },
        {
          "in": "path",
          "name": "slug",
          "schema": {
            "type": "string"
          },
          "description": "Workspace slug",
          "required": true,
          "examples": {
            "ExampleWorkspace": {
              "value": "my-workspace",
              "summary": "Example workspace",
              "description": "A typical workspace slug"
            }
          }
        }
      ],
      "responses": {
        "200": {
          "description": "Wait applied, or refused with a reason"
        },
        "404": {
          "description": "Work item not found"
        }
      }
    }
  },
  "/api/v1/workspaces/{slug}/stickies/": {
    "get": {
      "operationId": "list_stickies",
      "tags": [
        "Stickies"
      ],
      "summary": "List stickies",
      "parameters": [
        {
          "in": "path",
          "name": "slug",
          "schema": {
            "type": "string"
          },
          "description": "Workspace slug",
          "required": true,
          "examples": {
            "ExampleWorkspace": {
              "value": "my-workspace",
              "summary": "Example workspace",
              "description": "A typical workspace slug"
            }
          }
        }
      ],
      "responses": {
        "401": {
          "description": "Authentication credentials were not provided or are invalid."
        },
        "403": {
          "description": "Permission denied. User lacks required permissions."
        },
        "404": {
          "description": "The requested resource was not found."
        },
        "200": {
          "content": {
            "application/json": {
              "schema": {
                "type": "array",
                "items": {
                  "$ref": "#/components/schemas/Sticky"
                }
              },
              "examples": {
                "ListOfStickies": {
                  "value": [
                    {
                      "grouped_by": "state",
                      "sub_grouped_by": "priority",
                      "total_count": 150,
                      "next_cursor": "20:1:0",
                      "prev_cursor": "20:0:0",
                      "next_page_results": true,
                      "prev_page_results": false,
                      "count": 20,
                      "total_pages": 8,
                      "total_results": 150,
                      "extra_stats": null,
                      "results": [
                        {
                          "id": "550e8400-e29b-41d4-a716-446655440000",
                          "name": "Sticky 1",
                          "description_html": "<p>Sticky 1 description</p>",
                          "created_at": "2024-01-01T10:30:00Z"
                        }
                      ]
                    }
                  ],
                  "summary": "List of stickies"
                }
              }
            }
          },
          "description": "List of stickies"
        }
      }
    },
    "post": {
      "operationId": "create_sticky",
      "tags": [
        "Stickies"
      ],
      "summary": "Create a new sticky",
      "parameters": [
        {
          "in": "path",
          "name": "slug",
          "schema": {
            "type": "string"
          },
          "description": "Workspace slug",
          "required": true,
          "examples": {
            "ExampleWorkspace": {
              "value": "my-workspace",
              "summary": "Example workspace",
              "description": "A typical workspace slug"
            }
          }
        }
      ],
      "responses": {
        "401": {
          "description": "Authentication credentials were not provided or are invalid."
        },
        "403": {
          "description": "Permission denied. User lacks required permissions."
        },
        "404": {
          "description": "The requested resource was not found."
        },
        "201": {
          "content": {
            "application/json": {
              "schema": {
                "$ref": "#/components/schemas/Sticky"
              },
              "examples": {
                "Sticky": {
                  "value": {
                    "id": "550e8400-e29b-41d4-a716-446655440000",
                    "name": "Sticky 1",
                    "description_html": "<p>Sticky 1 description</p>",
                    "created_at": "2024-01-01T10:30:00Z"
                  }
                }
              }
            }
          },
          "description": "Sticky created"
        }
      }
    }
  },
  "/api/v1/workspaces/{slug}/stickies/{pk}/": {
    "delete": {
      "operationId": "delete_sticky",
      "tags": [
        "Stickies"
      ],
      "summary": "Delete a sticky",
      "parameters": [
        {
          "in": "path",
          "name": "pk",
          "schema": {
            "type": "string",
            "format": "uuid"
          },
          "description": "A UUID string identifying this Sticky.",
          "required": true
        },
        {
          "in": "path",
          "name": "slug",
          "schema": {
            "type": "string"
          },
          "description": "Workspace slug",
          "required": true,
          "examples": {
            "ExampleWorkspace": {
              "value": "my-workspace",
              "summary": "Example workspace",
              "description": "A typical workspace slug"
            }
          }
        }
      ],
      "responses": {
        "401": {
          "description": "Authentication credentials were not provided or are invalid."
        },
        "403": {
          "description": "Permission denied. User lacks required permissions."
        },
        "404": {
          "description": "The requested resource was not found."
        },
        "204": {
          "description": "Resource deleted successfully"
        }
      }
    },
    "get": {
      "operationId": "retrieve_sticky",
      "tags": [
        "Stickies"
      ],
      "summary": "Retrieve a sticky",
      "parameters": [
        {
          "in": "path",
          "name": "pk",
          "schema": {
            "type": "string",
            "format": "uuid"
          },
          "description": "A UUID string identifying this Sticky.",
          "required": true
        },
        {
          "in": "path",
          "name": "slug",
          "schema": {
            "type": "string"
          },
          "description": "Workspace slug",
          "required": true,
          "examples": {
            "ExampleWorkspace": {
              "value": "my-workspace",
              "summary": "Example workspace",
              "description": "A typical workspace slug"
            }
          }
        }
      ],
      "responses": {
        "401": {
          "description": "Authentication credentials were not provided or are invalid."
        },
        "403": {
          "description": "Permission denied. User lacks required permissions."
        },
        "404": {
          "description": "The requested resource was not found."
        },
        "200": {
          "content": {
            "application/json": {
              "schema": {
                "$ref": "#/components/schemas/Sticky"
              },
              "examples": {
                "Sticky": {
                  "value": {
                    "id": "550e8400-e29b-41d4-a716-446655440000",
                    "name": "Sticky 1",
                    "description_html": "<p>Sticky 1 description</p>",
                    "created_at": "2024-01-01T10:30:00Z"
                  }
                }
              }
            }
          },
          "description": "Sticky"
        }
      }
    },
    "patch": {
      "operationId": "update_sticky",
      "tags": [
        "Stickies"
      ],
      "summary": "Update a sticky",
      "parameters": [
        {
          "in": "path",
          "name": "pk",
          "schema": {
            "type": "string",
            "format": "uuid"
          },
          "description": "A UUID string identifying this Sticky.",
          "required": true
        },
        {
          "in": "path",
          "name": "slug",
          "schema": {
            "type": "string"
          },
          "description": "Workspace slug",
          "required": true,
          "examples": {
            "ExampleWorkspace": {
              "value": "my-workspace",
              "summary": "Example workspace",
              "description": "A typical workspace slug"
            }
          }
        }
      ],
      "responses": {
        "401": {
          "description": "Authentication credentials were not provided or are invalid."
        },
        "403": {
          "description": "Permission denied. User lacks required permissions."
        },
        "404": {
          "description": "The requested resource was not found."
        },
        "200": {
          "content": {
            "application/json": {
              "schema": {
                "$ref": "#/components/schemas/Sticky"
              },
              "examples": {
                "Sticky": {
                  "value": {
                    "id": "550e8400-e29b-41d4-a716-446655440000",
                    "name": "Sticky 1",
                    "description_html": "<p>Sticky 1 description</p>",
                    "created_at": "2024-01-01T10:30:00Z"
                  }
                }
              }
            }
          },
          "description": "Sticky"
        }
      }
    }
  },
  "/api/v1/workspaces/{slug}/work-items/search/": {
    "get": {
      "operationId": "search_work_items_2",
      "tags": [
        "Work Items"
      ],
      "summary": null,
      "parameters": [
        {
          "in": "query",
          "name": "limit",
          "schema": {
            "type": "integer"
          },
          "description": "Maximum number of results to return",
          "examples": {
            "Default": {
              "value": 10
            },
            "MoreResults": {
              "value": 50,
              "summary": "More results"
            }
          }
        },
        {
          "in": "query",
          "name": "project_id",
          "schema": {
            "type": "string",
            "format": "uuid"
          },
          "description": "Project ID for filtering results within a specific project",
          "examples": {
            "ExampleProjectID": {
              "value": "550e8400-e29b-41d4-a716-446655440000",
              "summary": "Example project ID",
              "description": "Filter results for this project"
            }
          }
        },
        {
          "in": "query",
          "name": "search",
          "schema": {
            "type": "string"
          },
          "description": "Search query to filter results by name, description, or identifier",
          "required": true,
          "examples": {
            "NameSearch": {
              "value": "bug fix",
              "summary": "Name search",
              "description": "Search for items containing 'bug fix'"
            },
            "SequenceID": {
              "value": "123",
              "summary": "Sequence ID",
              "description": "Search by sequence ID number"
            }
          }
        },
        {
          "in": "path",
          "name": "slug",
          "schema": {
            "type": "string"
          },
          "description": "Workspace slug",
          "required": true,
          "examples": {
            "ExampleWorkspace": {
              "value": "my-workspace",
              "summary": "Example workspace",
              "description": "A typical workspace slug"
            }
          }
        },
        {
          "in": "query",
          "name": "workspace_search",
          "schema": {
            "type": "string"
          },
          "description": "Whether to search across entire workspace or within specific project",
          "examples": {
            "ProjectOnly": {
              "value": "false",
              "summary": "Project only",
              "description": "Search within specific project only"
            },
            "WorkspaceWide": {
              "value": "true",
              "summary": "Workspace wide",
              "description": "Search across entire workspace"
            }
          }
        }
      ],
      "responses": {
        "200": {
          "content": {
            "application/json": {
              "schema": {
                "$ref": "#/components/schemas/IssueSearch"
              },
              "examples": {
                "IssueSearchResults": {
                  "value": {
                    "issues": [
                      {
                        "id": "550e8400-e29b-41d4-a716-446655440000",
                        "name": "Fix authentication bug in user login",
                        "sequence_id": 123,
                        "project__identifier": "MAB",
                        "project_id": "550e8400-e29b-41d4-a716-446655440001",
                        "workspace__slug": "my-workspace"
                      },
                      {
                        "id": "550e8400-e29b-41d4-a716-446655440002",
                        "name": "Add authentication middleware",
                        "sequence_id": 124,
                        "project__identifier": "MAB",
                        "project_id": "550e8400-e29b-41d4-a716-446655440001",
                        "workspace__slug": "my-workspace"
                      }
                    ]
                  }
                }
              }
            }
          },
          "description": "Work item search results"
        },
        "400": {
          "description": "Bad request - invalid search parameters"
        },
        "401": {
          "description": "Authentication credentials were not provided or are invalid."
        },
        "403": {
          "description": "Permission denied. User lacks required permissions."
        },
        "404": {
          "description": "Workspace not found"
        }
      }
    }
  },
  "/api/v1/workspaces/{slug}/work-items/search/advanced/": {
    "get": {
      "operationId": "search_work_items_advanced",
      "tags": [
        "Work Items"
      ],
      "summary": null,
      "parameters": [
        {
          "in": "path",
          "name": "slug",
          "schema": {
            "type": "string"
          },
          "required": true
        }
      ],
      "responses": {
        "200": {
          "content": {
            "application/json": {
              "schema": {
                "$ref": "#/components/schemas/IssueAdvancedSearchResponse"
              }
            }
          },
          "description": "Ranked work item search results."
        },
        "400": {
          "description": "Invalid query parameter."
        }
      }
    }
  },
  "/api/v1/workspaces/{slug}/work-items/{project_identifier}-{issue_identifier}/": {
    "get": {
      "operationId": "get_workspace_work_item_2",
      "tags": [
        "Work Items"
      ],
      "summary": "Retrieve work item by identifiers",
      "parameters": [
        {
          "in": "path",
          "name": "issue_identifier",
          "schema": {
            "type": "integer"
          },
          "description": "Issue sequence ID (numeric identifier within project)",
          "required": true,
          "examples": {
            "ExampleIssueIdentifier": {
              "value": 123,
              "summary": "Example issue identifier",
              "description": "A typical issue sequence ID"
            }
          }
        },
        {
          "in": "path",
          "name": "project_identifier",
          "schema": {
            "type": "string"
          },
          "description": "Project identifier (unique string within workspace)",
          "required": true,
          "examples": {
            "ExampleProjectIdentifier": {
              "value": "PROJ",
              "summary": "Example project identifier",
              "description": "A typical project identifier"
            }
          }
        },
        {
          "in": "path",
          "name": "slug",
          "schema": {
            "type": "string"
          },
          "description": "Workspace slug",
          "required": true,
          "examples": {
            "ExampleWorkspace": {
              "value": "my-workspace",
              "summary": "Example workspace",
              "description": "A typical workspace slug"
            }
          }
        }
      ],
      "responses": {
        "200": {
          "content": {
            "application/json": {
              "schema": {
                "$ref": "#/components/schemas/Issue"
              },
              "examples": {
                "Issue": {
                  "value": {
                    "id": "550e8400-e29b-41d4-a716-446655440000",
                    "name": "Implement user authentication",
                    "description": "Add OAuth 2.0 authentication flow",
                    "sequence_id": 1,
                    "priority": "high",
                    "assignees": [
                      "550e8400-e29b-41d4-a716-446655440001"
                    ],
                    "labels": [
                      "550e8400-e29b-41d4-a716-446655440002"
                    ],
                    "created_at": "2024-01-15T10:30:00Z",
                    "updated_at": "2024-01-15T10:30:00Z"
                  }
                }
              }
            }
          },
          "description": "Work item details"
        },
        "404": {
          "description": "Work item not found"
        }
      }
    }
  }
}
"##;

/// Parse the embedded essentials. The literal is pinned parsed-equal to the
/// fixture by `essentials_match_fixture`, so this cannot fail.
fn essentials() -> Value {
    serde_json::from_str(OPERATIONS_JSON).expect("embedded operations parse")
}

#[cfg(test)]
mod tests {
    use super::*;

    const FX04: &str =
        include_str!("../../../../fixtures/v1_openapi/FX-OPENAPI-04.doc_essentials.json");

    fn fixture() -> Value {
        serde_json::from_str(FX04).expect("fixture parses")
    }

    #[test]
    fn essentials_match_fixture() {
        assert_eq!(essentials(), fixture()["operations"]);
    }

    #[test]
    fn route_table_matches_essentials() {
        let essentials = essentials();
        let essentials = essentials.as_object().unwrap();
        assert_eq!(essentials.len(), routes::ROUTE_COUNT);
        for (path, methods) in routes::ROUTES {
            let ops = essentials.get(*path).unwrap_or_else(|| panic!("{path}"));
            let ops = ops.as_object().unwrap();
            let mut got: Vec<&str> = ops.keys().map(String::as_str).collect();
            got.sort_unstable();
            assert_eq!(&got, methods, "{path}");
        }
    }

    #[test]
    fn every_op_has_operation_id_and_responses() {
        let doc = build_document();
        let paths = doc["paths"].as_object().unwrap();
        assert_eq!(paths.len(), routes::ROUTE_COUNT);
        let mut ops = 0;
        for (path, item) in paths {
            for (method, op) in item.as_object().unwrap() {
                assert!(op.is_object(), "{path} {method}");
                assert!(
                    !op["operationId"].as_str().unwrap_or("").is_empty(),
                    "{path} {method}"
                );
                assert!(
                    !op["responses"].as_object().unwrap().is_empty(),
                    "{path} {method}"
                );
                ops += 1;
            }
        }
        assert_eq!(ops, 189);
    }

    #[test]
    fn doc_meta_matches_vocab_and_live_capture() {
        let doc = build_document();
        assert_eq!(doc["openapi"].as_str().unwrap(), meta::OPENAPI_VERSION);
        let info = &doc["info"];
        assert_eq!(info["title"].as_str().unwrap(), meta::TITLE);
        assert_eq!(info["version"].as_str().unwrap(), meta::VERSION);
        assert_eq!(info["description"].as_str().unwrap(), meta::DESCRIPTION);
        assert_eq!(
            info["contact"]["name"].as_str().unwrap(),
            meta::CONTACT.name
        );
        assert_eq!(info["contact"]["url"].as_str().unwrap(), meta::CONTACT.url);
        assert_eq!(
            info["contact"]["email"].as_str().unwrap(),
            meta::CONTACT.email
        );
        assert_eq!(
            info["license"]["name"].as_str().unwrap(),
            meta::LICENSE.name
        );
        assert_eq!(info["license"]["url"].as_str().unwrap(), meta::LICENSE.url);
        let servers = doc["servers"].as_array().unwrap();
        assert_eq!(servers.len(), meta::SERVERS.len());
        for (got, want) in servers.iter().zip(meta::SERVERS.iter()) {
            assert_eq!(got["url"].as_str().unwrap(), want.url);
            assert_eq!(got["description"].as_str().unwrap(), want.description);
        }
        let tags = doc["tags"].as_array().unwrap();
        assert_eq!(tags.len(), meta::TAGS.len());
        for (got, want) in tags.iter().zip(meta::TAGS.iter()) {
            assert_eq!(got["name"].as_str().unwrap(), want.name);
            assert_eq!(got["description"].as_str().unwrap(), want.description);
        }
        let scheme = &doc["components"]["securitySchemes"][meta::API_KEY_AUTH_NAME];
        assert_eq!(*scheme, meta::api_key_security_definition());
        assert_eq!(
            *scheme,
            fixture()["security_schemes"]["ApiKeyAuthentication"]
        );
    }

    #[test]
    fn contract_shape_pins_hold() {
        let doc = build_document();
        let paths = doc["paths"].as_object().unwrap();
        for (path, item) in paths {
            assert!(path.starts_with("/api/v1/"), "{path}");
            assert!(!path.to_lowercase().contains("server"), "{path}");
            for (method, _) in item.as_object().unwrap() {
                assert!(
                    matches!(method.as_str(), "get" | "post" | "patch" | "delete"),
                    "{path} {method}"
                );
            }
        }
        let live: Map<String, Value> = paths
            .iter()
            .map(|(path, item)| {
                let mut methods: Vec<Value> = item
                    .as_object()
                    .unwrap()
                    .keys()
                    .map(|method| Value::String(method.clone()))
                    .collect();
                methods.sort_by(|a, b| a.as_str().cmp(&b.as_str()));
                (path.clone(), Value::Array(methods))
            })
            .collect();
        assert_eq!(
            Value::Object(live),
            Value::Object(
                routes::route_map()
                    .into_iter()
                    .map(|(path, methods)| (
                        path.to_string(),
                        Value::Array(
                            methods
                                .into_iter()
                                .map(|name| Value::String(name.to_string()))
                                .collect(),
                        ),
                    ))
                    .collect()
            )
        );
    }

    #[test]
    fn dual_form_params_match_contract() {
        let doc = build_document();
        let mut seen = 0;
        for (path, item) in doc["paths"].as_object().unwrap() {
            if !path.contains("{project_id}") && !path.contains("/projects/{pk}/") {
                continue;
            }
            for (method, op) in item.as_object().unwrap() {
                let Some(params) = op.get("parameters").and_then(Value::as_array) else {
                    continue;
                };
                for param in params {
                    if param.get("in").and_then(Value::as_str) != Some("path") {
                        continue;
                    }
                    let name = param.get("name").and_then(Value::as_str);
                    let is_project_id = name == Some("project_id");
                    let is_pk = name == Some("pk") && path.contains("/projects/{pk}/");
                    if !is_project_id && !is_pk {
                        continue;
                    }
                    seen += 1;
                    let description = param["description"].as_str().unwrap_or("");
                    assert!(description.contains("UUID"), "{path} {method}");
                    assert_eq!(description, hooks::DUAL_FORM_DESCRIPTION, "{path} {method}");
                    let mut examples: Vec<&str> = param["examples"]
                        .as_object()
                        .unwrap()
                        .keys()
                        .map(String::as_str)
                        .collect();
                    examples.sort_unstable();
                    assert_eq!(examples, ["slug", "uuid"], "{path} {method}");
                    assert_eq!(param["schema"]["type"].as_str().unwrap(), "string");
                    assert!(param["schema"].get("format").is_none(), "{path} {method}");
                }
            }
        }
        assert!(seen > 0);
    }

    #[test]
    fn null_parameters_omitted() {
        let essentials = essentials();
        let essentials = essentials.as_object().unwrap();
        let doc = build_document();
        let paths = doc["paths"].as_object().unwrap();
        let mut omitted = 0;
        for (path, item) in paths {
            for (method, op) in item.as_object().unwrap() {
                let essential = essentials
                    .get(path.as_str())
                    .and_then(|ops| ops.get(method.as_str()))
                    .unwrap_or_else(|| panic!("{path} {method}"));
                if essential.get("parameters").is_some_and(Value::is_null) {
                    assert!(op.get("parameters").is_none(), "{path} {method}");
                    omitted += 1;
                } else {
                    assert_eq!(op["parameters"], essential["parameters"], "{path} {method}");
                }
            }
        }
        assert_eq!(omitted, 16);
    }

    #[test]
    fn yaml_and_json_parse_to_equal_documents() {
        let yaml_text = render_yaml();
        let json_text = render_json();
        let from_json: Value = serde_json::from_str(&json_text).expect("JSON parses");
        assert_eq!(from_json, build_document());
        let from_yaml = parse_yaml_subset(&yaml_text);
        assert_eq!(from_yaml, from_json);
    }

    #[test]
    fn yaml_quotes_every_string() {
        let yaml_text = render_yaml();
        let mut strings = Vec::new();
        collect_strings(&build_document(), &mut strings);
        assert!(!strings.is_empty());
        for text in &strings {
            let quoted = serde_json::to_string(text).unwrap();
            assert!(yaml_text.contains(&quoted), "{text:?}");
        }
    }

    fn collect_strings(value: &Value, out: &mut Vec<String>) {
        match value {
            Value::String(text) => out.push(text.clone()),
            Value::Array(items) => {
                for item in items {
                    collect_strings(item, out);
                }
            }
            Value::Object(map) => {
                for (key, val) in map {
                    out.push(key.clone());
                    collect_strings(val, out);
                }
            }
            _ => {}
        }
    }

    /// Strict parser for exactly the grammar [`render_yaml`] emits: block
    /// mappings/sequences at 2-space indents, `-`/`"key":` prefixes, inline
    /// scalars that are double-quoted strings, `null`/`true`/`false`,
    /// JSON numbers, or empty `[]`/`{}`. Anything else — including an
    /// unquoted string — is an error, so a successful round-trip proves the
    /// emitter quoted every string.
    fn parse_yaml_subset(text: &str) -> Value {
        let lines: Vec<&str> = text.lines().collect();
        assert!(!lines.is_empty());
        let (value, next) = parse_block(&lines, 0, 0);
        assert_eq!(next, lines.len(), "trailing lines");
        value
    }

    fn parse_block(lines: &[&str], mut i: usize, indent: usize) -> (Value, usize) {
        assert!(i < lines.len(), "missing block");
        assert_eq!(count_indent(lines[i]), indent, "bad indent: {:?}", lines[i]);
        if lines[i][indent..].starts_with('"') {
            let mut map = Map::new();
            while i < lines.len()
                && count_indent(lines[i]) == indent
                && lines[i][indent..].starts_with('"')
            {
                let (key, rest) = split_entry(&lines[i][indent..]);
                i = parse_child(lines, i, indent, rest, &mut map, key);
            }
            (Value::Object(map), i)
        } else {
            assert!(
                &lines[i][indent..] == "-" || lines[i][indent..].starts_with("- "),
                "expected sequence item: {:?}",
                lines[i]
            );
            let mut items = Vec::new();
            while i < lines.len()
                && count_indent(lines[i]) == indent
                && (&lines[i][indent..] == "-" || lines[i][indent..].starts_with("- "))
            {
                let rest = &lines[i][indent + 1..];
                i = parse_seq_child(lines, i, indent, rest, &mut items);
            }
            (Value::Array(items), i)
        }
    }

    fn parse_child(
        lines: &[&str],
        i: usize,
        indent: usize,
        rest: &str,
        map: &mut Map<String, Value>,
        key: String,
    ) -> usize {
        if rest.is_empty() {
            let (child, next) = parse_block(lines, i + 1, indent + 2);
            map.insert(key, child);
            next
        } else {
            let token = rest.strip_prefix(' ').expect("one space after colon");
            assert!(!token.starts_with(' '), "one space after colon");
            map.insert(key, parse_inline(token));
            i + 1
        }
    }

    fn parse_seq_child(
        lines: &[&str],
        i: usize,
        indent: usize,
        rest: &str,
        items: &mut Vec<Value>,
    ) -> usize {
        if rest.is_empty() {
            let (child, next) = parse_block(lines, i + 1, indent + 2);
            items.push(child);
            next
        } else {
            let token = rest.strip_prefix(' ').expect("one space after dash");
            assert!(!token.starts_with(' '), "one space after dash");
            items.push(parse_inline(token));
            i + 1
        }
    }

    /// Split `"key":` + rest off a mapping line. The key scan mirrors JSON
    /// string escaping (backslash escapes the next byte).
    fn split_entry(line: &str) -> (String, &str) {
        let bytes = line.as_bytes();
        let mut i = 1;
        let mut escaped = false;
        while i < bytes.len() {
            let byte = bytes[i];
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                break;
            }
            i += 1;
        }
        assert!(i < bytes.len(), "unterminated key: {line:?}");
        let key: String = serde_json::from_str(&line[..=i]).expect("key has valid escapes");
        let rest = line[i + 1..].strip_prefix(':').expect("colon after key");
        (key, rest)
    }

    fn parse_inline(token: &str) -> Value {
        if token.starts_with('"') {
            assert!(
                token.len() >= 2 && token.ends_with('"'),
                "unterminated string"
            );
            return serde_json::from_str(token).expect("string has valid escapes");
        }
        match token {
            "null" => Value::Null,
            "true" => Value::Bool(true),
            "false" => Value::Bool(false),
            "[]" => Value::Array(Vec::new()),
            "{}" => Value::Object(Map::new()),
            _ => {
                if let Ok(n) = token.parse::<i64>() {
                    Value::Number(n.into())
                } else if let Ok(n) = token.parse::<u64>() {
                    Value::Number(n.into())
                } else if let Ok(n) = token.parse::<f64>() {
                    Value::Number(serde_json::Number::from_f64(n).expect("finite float"))
                } else {
                    panic!("scalar must be quoted or a JSON literal: {token:?}");
                }
            }
        }
    }

    fn count_indent(line: &str) -> usize {
        line.len() - line.trim_start_matches(' ').len()
    }
}
