//! Outbound ACP JSON-RPC message builders for the Grok bridge, plus the
//! small helpers used to interpret the responses grok's ACP server sends
//! back. Inbound frames are parsed with OpenClaw's shared
//! [`crate::openclaw::schema::AcpMessage`] — this module only covers the
//! client → server direction that the OpenClaw bridge never needed (there,
//! `acpx` is the client and drives the handshake for us).
//!
//! ACP is JSON-RPC 2.0 (<https://agentclientprotocol.com>). The handshake is:
//! `initialize` (capability negotiation) → `session/new` (returns a
//! `sessionId`) → `session/prompt` (the turn; the server streams
//! `session/update` notifications then answers with a `stopReason`). While a
//! turn is in flight the server may send a `session/request_permission`
//! request, which the client must answer by selecting one of the offered
//! options; [`select_allow_option`] implements the MVP auto-approve choice.

use serde_json::{Value, json};

/// The ACP protocol major version this client advertises in `initialize`.
/// ACP is currently at major version 1; a server that speaks a newer version
/// negotiates down in its `initialize` response.
pub const ACP_PROTOCOL_VERSION: u64 = 1;

/// Build the `initialize` request line. We advertise a minimal client: no
/// filesystem or terminal capabilities, so the server does its own I/O and we
/// stay a thin driver. The `clientInfo` mirrors what the Codex bridge sends so
/// grok's logs attribute the session to pidash.
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
/// directory the agent operates in; `model`, when set, rides as an extra field
/// (grok reads it to pick the model — unknown fields are ignored by
/// spec-conforming ACP servers, so this is safe if grok ignores it).
pub fn session_new_request(id: u64, cwd: &str, model: Option<&str>) -> String {
    let mut params = json!({ "cwd": cwd, "mcpServers": [] });
    if let Some(m) = model {
        params["model"] = Value::String(m.to_string());
    }
    rpc_request(id, "session/new", params)
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

/// Extract the `sessionId` from a `session/new` response `result`. ACP puts it
/// at `result.sessionId`; tolerate a snake_case spelling defensively.
pub fn session_id_from_result(result: &Value) -> Option<String> {
    result
        .get("sessionId")
        .or_else(|| result.get("session_id"))
        .and_then(|v| v.as_str())
        .map(ToOwned::to_owned)
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
}
