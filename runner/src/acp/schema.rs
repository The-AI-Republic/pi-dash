//! Outbound ACP JSON-RPC message builders for the shared ACP bridge, plus
//! the small helpers used to interpret the responses an ACP server sends
//! back. Inbound frames are parsed with OpenClaw's shared
//! [`crate::openclaw::schema::AcpMessage`] — this module only covers the
//! client → server direction.
//!
//! ACP is JSON-RPC 2.0 (<https://agentclientprotocol.com>). The handshake is:
//! `initialize` (capability negotiation) → `session/new` (returns a
//! `sessionId` and, optionally, the session's config options) →
//! `session/prompt` (the turn; the server streams `session/update`
//! notifications then answers with a `stopReason`). While a turn is in flight
//! the server may send a `session/request_permission` request, which the
//! client must answer by selecting one of the offered options;
//! [`select_allow_option`] implements the MVP auto-approve choice.
//!
//! Resuming reuses a known session without replaying its history via
//! `session/resume` when the server advertises it under
//! `agentCapabilities.sessionCapabilities.resume`, else `session/load` when
//! it advertises `agentCapabilities.loadSession`, else the id is kept only
//! as the run's thread-id seed (no prior state is restored).

use serde_json::{Value, json};

/// The ACP protocol major version this client advertises in `initialize`.
/// ACP is currently at major version 1; a server that speaks a newer version
/// negotiates down in its `initialize` response.
pub const ACP_PROTOCOL_VERSION: u64 = 1;

/// JSON-RPC error code an ACP server returns when the request needs
/// authentication the client has not completed (`Authentication required`).
/// Observed on a real `agy acp` server: `session/new` without cached
/// credentials answers
/// `{"jsonrpc":"2.0","error":{"code":-32000,"message":"Authentication required"},"id":2}`.
pub const AUTH_REQUIRED_CODE: i64 = -32000;

/// Build the `initialize` request line. We advertise a minimal client: no
/// filesystem or terminal capabilities, so the server does its own I/O and we
/// stay a thin driver. The `clientInfo` mirrors what the Codex bridge sends
/// so servers attribute the session to pidash.
pub fn initialize_request(id: u64) -> String {
    rpc_request(
        id,
        "initialize",
        json!({
            "protocolVersion": ACP_PROTOCOL_VERSION,
            "clientCapabilities": { "fs": { "readTextFile": false, "writeTextFile": false } },
            "clientInfo": { "name": "pidash", "version": crate::RUNNER_VERSION },
        }),
    )
}

/// Build the `session/new` request line. `cwd` is the absolute working
/// directory the agent operates in; `model`, when set, rides as an extra
/// field (Grok reads it to pick the model — unknown fields are ignored by
/// spec-conforming ACP servers, so this is safe where the server ignores
/// it). Antigravity ignores it and is instead driven through the session
/// config options it advertises (see [`config_options_from_result`] and
/// [`session_set_config_option_request`]).
pub fn session_new_request(id: u64, cwd: &str, model: Option<&str>) -> String {
    let mut params = json!({ "cwd": cwd, "mcpServers": [] });
    if let Some(m) = model {
        params["model"] = Value::String(m.to_string());
    }
    rpc_request(id, "session/new", params)
}

/// Build the `session/resume` request line: reopen `session_id` in `cwd`
/// without replaying its history. Only valid when the server advertised
/// `agentCapabilities.sessionCapabilities.resume` (see
/// [`resume_support_from_initialize`]).
pub fn session_resume_request(id: u64, session_id: &str, cwd: &str) -> String {
    rpc_request(
        id,
        "session/resume",
        json!({ "sessionId": session_id, "cwd": cwd, "mcpServers": [] }),
    )
}

/// Build the `session/load` request line: reload `session_id` in `cwd`,
/// replaying its history. Only valid when the server advertised
/// `agentCapabilities.loadSession` (see [`resume_support_from_initialize`]).
pub fn session_load_request(id: u64, session_id: &str, cwd: &str) -> String {
    rpc_request(
        id,
        "session/load",
        json!({ "sessionId": session_id, "cwd": cwd, "mcpServers": [] }),
    )
}

/// Build the `session/set_config_option` request line, which applies one
/// advertised session option (e.g. Antigravity's Model picker) mid-session.
/// `config_id` and `value` must come from [`select_config_value`], which
/// matches them against the options the server advertised — servers reject
/// unknown values.
pub fn session_set_config_option_request(
    id: u64,
    session_id: &str,
    config_id: &str,
    value: &str,
) -> String {
    rpc_request(
        id,
        "session/set_config_option",
        json!({ "sessionId": session_id, "configId": config_id, "value": value }),
    )
}

/// Build the `session/cancel` *notification* (no `id`: per the ACP schema it
/// is a `CancelNotification`, not a request). The server aborts the live turn
/// and answers the in-flight `session/prompt` with `stopReason: "cancelled"`.
pub fn session_cancel_notification(session_id: &str) -> String {
    json!({
        "jsonrpc": "2.0",
        "method": "session/cancel",
        "params": { "sessionId": session_id },
    })
    .to_string()
}

/// Build the `session/prompt` request line — the turn itself. The prompt is a
/// single text content block, matching how the Codex bridge sends turn input.
pub fn session_prompt_request(id: u64, session_id: &str, prompt: &str) -> String {
    rpc_request(
        id,
        "session/prompt",
        json!({
            "sessionId": session_id,
            "prompt": [ { "type": "text", "text": prompt } ],
        }),
    )
}

/// Build the response to a `session/request_permission` request, selecting the
/// given option id (see [`select_allow_option`]). ACP expects the outcome to
/// name the chosen option.
pub fn permission_selected_response(id: &Value, option_id: &str) -> String {
    let msg = json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": { "outcome": { "outcome": "selected", "optionId": option_id } },
    });
    msg.to_string()
}

/// Pick the option id to auto-approve from a `session/request_permission`
/// request's `params.options` array. Prefers a persistent "allow always" over
/// a one-shot "allow once"; falls back to the first option so we always send
/// *something* rather than deadlocking the turn. Returns `None` only when no
/// options are offered at all.
pub fn select_allow_option(params: &Value) -> Option<String> {
    let options = params.get("options").and_then(|o| o.as_array())?;
    let id_of = |opt: &Value| {
        opt.get("optionId")
            .and_then(|v| v.as_str())
            .map(ToOwned::to_owned)
    };
    let by_kind = |want: &str| {
        options
            .iter()
            .find(|o| o.get("kind").and_then(|k| k.as_str()) == Some(want))
            .and_then(id_of)
    };
    by_kind("allow_always")
        .or_else(|| by_kind("allow_once"))
        .or_else(|| options.first().and_then(id_of))
}

/// Extract the `sessionId` from a `session/new` (or `session/resume` /
/// `session/load`) response `result`. ACP puts it at `result.sessionId`;
/// tolerate a snake_case spelling defensively.
pub fn session_id_from_result(result: &Value) -> Option<String> {
    result
        .get("sessionId")
        .or_else(|| result.get("session_id"))
        .and_then(|v| v.as_str())
        .map(ToOwned::to_owned)
}

/// How a known session id can be revived on this server, read off the
/// `initialize` response `result`. `sessionCapabilities.resume` (or its
/// snake_case twin) selects history-free `session/resume`; else
/// `loadSession` (likewise tolerant) selects history-replaying
/// `session/load`; else the id is kept as a thread-id seed only.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResumeSupport {
    Resume,
    Load,
    SeedOnly,
}

pub fn resume_support_from_initialize(result: &Value) -> ResumeSupport {
    let caps = result.get("agentCapabilities");
    let session_caps = caps
        .and_then(|c| c.get("sessionCapabilities"))
        .or_else(|| caps.and_then(|c| c.get("session_capabilities")));
    let resume = session_caps
        .and_then(|c| c.get("resume"))
        .is_some_and(|v| !v.is_null());
    if resume {
        return ResumeSupport::Resume;
    }
    let load = caps
        .and_then(|c| c.get("loadSession"))
        .or_else(|| caps.and_then(|c| c.get("load_session")))
        .is_some_and(|v| v.as_bool().unwrap_or(!v.is_null()));
    if load {
        ResumeSupport::Load
    } else {
        ResumeSupport::SeedOnly
    }
}

/// One advertised session config option, from a `session/new` (or
/// `session/resume` / `session/load`) response `result.configOptions` entry.
/// Only the fields the bridge needs are kept: the option `id` (used when
/// setting), the semantic `category`, the current value, and the flat list of
/// selectable value ids (groups are flattened; see [`config_options_from_result`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigOption {
    pub id: String,
    pub name: String,
    pub category: Option<String>,
    pub current_value: String,
    pub values: Vec<String>,
}

/// Read the advertised session config options off a `session/new` (or
/// resume / load) response `result`. Returns an empty vec when the server
/// advertises none — Antigravity advertises its Model and thinking-level
/// pickers here; servers without options (e.g. Grok) yield none and the
/// caller leaves the server default in place. Both flat `options` arrays and
/// grouped option arrays are accepted; entries without a usable value id are
/// skipped so a half-understood schema can never poison option setting.
pub fn config_options_from_result(result: &Value) -> Vec<ConfigOption> {
    let Some(list) = result.get("configOptions").and_then(|v| v.as_array()) else {
        return Vec::new();
    };
    list.iter()
        .filter_map(|entry| {
            let id = entry.get("id")?.as_str()?;
            let current_value = entry.get("currentValue")?.as_str()?;
            let options = entry.get("options")?.as_array()?;
            let mut values = Vec::new();
            for opt in options {
                if let Some(value) = opt.get("value").and_then(|v| v.as_str()) {
                    values.push(value.to_string());
                } else if let Some(group) = opt.get("options").and_then(|v| v.as_array()) {
                    for sub in group {
                        if let Some(value) = sub.get("value").and_then(|v| v.as_str()) {
                            values.push(value.to_string());
                        }
                    }
                }
            }
            Some(ConfigOption {
                id: id.to_string(),
                name: entry
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or(id)
                    .to_string(),
                category: entry
                    .get("category")
                    .and_then(|v| v.as_str())
                    .map(ToOwned::to_owned),
                current_value: current_value.to_string(),
                values,
            })
        })
        .collect()
}

/// Match a configured model slug against the advertised session config
/// options and return the `(config_id, value)` pair to pass to
/// [`session_set_config_option_request`]. The model option is the entry whose
/// `category` is `"model"`, else whose id contains `"model"`, else whose name
/// contains `"model"`. The slug matches a candidate value (or its display
/// name) case-insensitively; without a match the server default stays and the
/// caller logs the miss. Returns `None` when there is no model option or no
/// advertised value matches.
pub fn select_config_value(options: &[ConfigOption], model: &str) -> Option<(String, String)> {
    let option = options
        .iter()
        .find(|o| o.category.as_deref() == Some("model"))
        .or_else(|| options.iter().find(|o| o.id.contains("model")))
        .or_else(|| {
            options
                .iter()
                .find(|o| o.name.to_ascii_lowercase().contains("model"))
        })?;
    if option.values.is_empty() {
        return None;
    }
    let want = model.trim().to_ascii_lowercase();
    // Prefer an exact value-id hit; fall back to a display-name hit so a UI
    // label ("Gemini 4 Neon") still resolves when the slug differs.
    let hit = option
        .values
        .iter()
        .find(|v| v.to_ascii_lowercase() == want)
        .cloned()
        .or_else(|| {
            option
                .values
                .iter()
                .find(|v| {
                    v.to_ascii_lowercase().contains(&want) || want.contains(&v.to_ascii_lowercase())
                })
                .cloned()
        })?;
    Some((option.id.clone(), hit))
}

/// True when `error` (the `error` member of a JSON-RPC error frame) is the
/// ACP authentication-required failure: code `-32000` with an
/// "Authentication required" message. Code alone decides — the message is
/// matched case-insensitively and tolerates surrounding context — so a
/// server that phrases it differently still trips the safety net while an
/// unrelated `-32000` (should one ever appear) without the auth wording
/// stays a plain crash.
pub fn is_auth_required_error(error: &Value) -> bool {
    let code = error.get("code").and_then(|c| c.as_i64());
    if code != Some(AUTH_REQUIRED_CODE) {
        return false;
    }
    error
        .get("message")
        .and_then(|m| m.as_str())
        .is_some_and(|m| m.to_ascii_lowercase().contains("authentication required"))
}

/// Serialize a JSON-RPC 2.0 request with an integer id.
fn rpc_request(id: u64, method: &str, params: Value) -> String {
    json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initialize_request_carries_protocol_version_and_client_info() {
        let line = initialize_request(1);
        let v: Value = serde_json::from_str(&line).unwrap();
        assert_eq!(v["method"], "initialize");
        assert_eq!(v["id"], 1);
        assert_eq!(v["params"]["protocolVersion"], ACP_PROTOCOL_VERSION);
        assert_eq!(v["params"]["clientInfo"]["name"], "pidash");
    }

    #[test]
    fn session_new_includes_model_only_when_set() {
        let with = session_new_request(2, "/work", Some("grok-4.3"));
        let v: Value = serde_json::from_str(&with).unwrap();
        assert_eq!(v["params"]["cwd"], "/work");
        assert_eq!(v["params"]["model"], "grok-4.3");

        let without = session_new_request(2, "/work", None);
        let v: Value = serde_json::from_str(&without).unwrap();
        assert!(v["params"].get("model").is_none());
    }

    #[test]
    fn session_prompt_wraps_text_content_block() {
        let line = session_prompt_request(3, "sess-1", "do the thing");
        let v: Value = serde_json::from_str(&line).unwrap();
        assert_eq!(v["method"], "session/prompt");
        assert_eq!(v["params"]["sessionId"], "sess-1");
        assert_eq!(v["params"]["prompt"][0]["type"], "text");
        assert_eq!(v["params"]["prompt"][0]["text"], "do the thing");
    }

    #[test]
    fn session_cancel_is_a_notification_without_id() {
        let line = session_cancel_notification("sess-9");
        let v: Value = serde_json::from_str(&line).unwrap();
        assert_eq!(v["method"], "session/cancel");
        assert_eq!(v["params"]["sessionId"], "sess-9");
        assert!(v.get("id").is_none());
    }

    #[test]
    fn session_resume_and_load_carry_session_and_cwd() {
        for (line, method) in [
            (
                session_resume_request(4, "sess-r", "/work"),
                "session/resume",
            ),
            (session_load_request(5, "sess-l", "/work"), "session/load"),
        ] {
            let v: Value = serde_json::from_str(&line).unwrap();
            assert_eq!(v["method"], method);
            assert!(v["params"]["sessionId"].as_str().is_some());
            assert_eq!(v["params"]["cwd"], "/work");
        }
    }

    #[test]
    fn session_set_config_option_names_option_and_value() {
        let line = session_set_config_option_request(6, "sess-1", "model", "gemini-4-neon");
        let v: Value = serde_json::from_str(&line).unwrap();
        assert_eq!(v["method"], "session/set_config_option");
        assert_eq!(v["params"]["sessionId"], "sess-1");
        assert_eq!(v["params"]["configId"], "model");
        assert_eq!(v["params"]["value"], "gemini-4-neon");
    }

    #[test]
    fn select_allow_option_prefers_allow_always() {
        let params = json!({
            "options": [
                { "optionId": "a", "name": "Reject", "kind": "reject_once" },
                { "optionId": "b", "name": "Allow once", "kind": "allow_once" },
                { "optionId": "c", "name": "Always allow", "kind": "allow_always" },
            ]
        });
        assert_eq!(select_allow_option(&params).as_deref(), Some("c"));
    }

    #[test]
    fn select_allow_option_falls_back_to_allow_once_then_first() {
        let allow_once = json!({
            "options": [
                { "optionId": "a", "name": "Reject", "kind": "reject_once" },
                { "optionId": "b", "name": "Allow once", "kind": "allow_once" },
            ]
        });
        assert_eq!(select_allow_option(&allow_once).as_deref(), Some("b"));

        // No kinds we recognize → first option, so we never deadlock the turn.
        let unknown = json!({
            "options": [
                { "optionId": "x", "name": "Whatever" },
                { "optionId": "y", "name": "Other" },
            ]
        });
        assert_eq!(select_allow_option(&unknown).as_deref(), Some("x"));

        // No options at all → None.
        assert_eq!(select_allow_option(&json!({ "options": [] })), None);
        assert_eq!(select_allow_option(&json!({})), None);
    }

    #[test]
    fn permission_response_echoes_request_id_and_option() {
        let line = permission_selected_response(&json!(7), "opt-1");
        let v: Value = serde_json::from_str(&line).unwrap();
        assert_eq!(v["id"], 7);
        assert_eq!(v["result"]["outcome"]["outcome"], "selected");
        assert_eq!(v["result"]["outcome"]["optionId"], "opt-1");
        // A string id must round-trip too (ACP ids may be strings).
        let line = permission_selected_response(&json!("req-9"), "opt-2");
        let v: Value = serde_json::from_str(&line).unwrap();
        assert_eq!(v["id"], "req-9");
    }

    #[test]
    fn session_id_extracted_from_result() {
        assert_eq!(
            session_id_from_result(&json!({ "sessionId": "s1" })).as_deref(),
            Some("s1")
        );
        assert_eq!(
            session_id_from_result(&json!({ "session_id": "s2" })).as_deref(),
            Some("s2")
        );
        assert_eq!(session_id_from_result(&json!({})), None);
    }

    /// The real `agy acp` 1.5.15 `initialize` result (recorded unauthenticated
    /// on the dev box): resume support must be detected, as must the
    /// snake_case twins the server also emits.
    #[test]
    fn resume_support_detects_real_agy_initialize() {
        let result = json!({
            "protocolVersion": 1,
            "agentCapabilities": {
                "loadSession": true,
                "load_session": true,
                "sessionCapabilities": { "resume": {}, "close": {}, "load": {}, "list": {} },
                "session_capabilities": { "resume": {}, "close": {}, "load": {}, "list": {} },
                "auth": { "logout": {} },
            },
            "agentInfo": { "name": "Antigravity ACP Server", "version": "1.5.15" },
            "authMethods": [ { "id": "oauth-personal" } ],
        });
        assert_eq!(
            resume_support_from_initialize(&result),
            ResumeSupport::Resume
        );

        let load_only = json!({ "agentCapabilities": { "loadSession": true } });
        assert_eq!(
            resume_support_from_initialize(&load_only),
            ResumeSupport::Load
        );

        let legacy = json!({ "agentCapabilities": { "load_session": true } });
        assert_eq!(resume_support_from_initialize(&legacy), ResumeSupport::Load);

        assert_eq!(
            resume_support_from_initialize(&json!({ "agentCapabilities": {} })),
            ResumeSupport::SeedOnly
        );
        assert_eq!(
            resume_support_from_initialize(&json!({})),
            ResumeSupport::SeedOnly
        );
    }

    #[test]
    fn config_options_parsed_from_session_new_result() {
        let result = json!({
            "sessionId": "sess-1",
            "configOptions": [
                {
                    "id": "model",
                    "name": "Model",
                    "category": "model",
                    "type": "select",
                    "currentValue": "gemini-4-hydrogen",
                    "options": [
                        { "value": "gemini-4-hydrogen", "name": "Gemini 4 Hydrogen" },
                        { "value": "gemini-4-neon", "name": "Gemini 4 Neon" },
                    ],
                },
                {
                    "id": "thinking",
                    "name": "Thinking level",
                    "type": "select",
                    "currentValue": "Medium",
                    "options": [
                        { "value": "Low", "name": "Low" },
                        { "value": "Medium", "name": "Medium" },
                    ],
                },
            ],
        });
        let options = config_options_from_result(&result);
        assert_eq!(options.len(), 2);
        assert_eq!(options[0].id, "model");
        assert_eq!(options[0].category.as_deref(), Some("model"));
        assert_eq!(
            options[0].values,
            vec!["gemini-4-hydrogen", "gemini-4-neon"]
        );

        // Exact slug hit.
        assert_eq!(
            select_config_value(&options, "gemini-4-neon"),
            Some(("model".to_string(), "gemini-4-neon".to_string()))
        );
        // Case-insensitive hit.
        assert_eq!(
            select_config_value(&options, "GEMINI-4-NEON"),
            Some(("model".to_string(), "gemini-4-neon".to_string()))
        );
        // Unknown slug → server default stays.
        assert_eq!(select_config_value(&options, "gpt-99"), None);

        // No configOptions at all → no options, no selection.
        let bare = config_options_from_result(&json!({ "sessionId": "s" }));
        assert!(bare.is_empty());
        assert_eq!(select_config_value(&bare, "gemini-4-neon"), None);
    }

    #[test]
    fn config_options_accept_grouped_values() {
        let result = json!({
            "configOptions": [
                {
                    "id": "model",
                    "name": "Model",
                    "category": "model",
                    "type": "select",
                    "currentValue": "a",
                    "options": [
                        { "name": "Fast", "options": [ { "value": "a" }, { "value": "b" } ] },
                    ],
                },
            ],
        });
        let options = config_options_from_result(&result);
        assert_eq!(options[0].values, vec!["a", "b"]);
        assert_eq!(
            select_config_value(&options, "b"),
            Some(("model".to_string(), "b".to_string()))
        );
    }

    #[test]
    fn auth_required_detected_by_code_with_message_check() {
        // The exact frame a real `agy acp` returns for `session/new`
        // without cached credentials.
        let error = json!({ "code": -32000, "message": "Authentication required" });
        assert!(is_auth_required_error(&error));

        // Same code, same wording with surrounding context → still the net.
        assert!(is_auth_required_error(
            &json!({ "code": -32000, "message": "Error: Authentication required; sign in again" })
        ));
        // Same code, different wording → not auth (stays a plain crash, so
        // a non-auth server error is never misreported as auth-missing and
        // re-queued).
        assert!(!is_auth_required_error(
            &json!({ "code": -32000, "message": "auth required: sign in again" })
        ));
        // Right wording, wrong code → not auth (stays a plain crash).
        assert!(!is_auth_required_error(
            &json!({ "code": -32603, "message": "Authentication required" })
        ));
        // Unrelated error → not auth.
        assert!(!is_auth_required_error(
            &json!({ "code": -32603, "message": "invalid api key" })
        ));
        assert!(!is_auth_required_error(&json!({})));
    }
}
