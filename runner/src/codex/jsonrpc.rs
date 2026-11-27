use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Request<P> {
    pub jsonrpc: String,
    pub id: u64,
    pub method: String,
    pub params: P,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Notification<P> {
    pub jsonrpc: String,
    pub method: String,
    pub params: P,
}

/// Server request identifiers are opaque: current app-server versions use
/// numeric and string IDs, and replies must preserve both value and type.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(untagged)]
pub enum RpcId {
    Number(i64),
    String(String),
}

#[derive(Debug, Serialize)]
struct Response<R> {
    jsonrpc: &'static str,
    id: RpcId,
    result: R,
}

pub fn response<R: Serialize>(id: RpcId, result: R) -> serde_json::Result<String> {
    serde_json::to_string(&Response {
        jsonrpc: "2.0",
        id,
        result,
    })
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum Incoming {
    // Match before Response: serde otherwise accepts a server request as
    // an empty response and silently discards its method and parameters.
    Request {
        #[serde(default)]
        jsonrpc: Option<String>,
        id: RpcId,
        method: String,
        #[serde(default)]
        params: serde_json::Value,
    },
    Response {
        #[serde(default)]
        jsonrpc: Option<String>,
        id: u64,
        #[serde(default)]
        result: Option<serde_json::Value>,
        #[serde(default)]
        error: Option<RpcError>,
    },
    Notification {
        #[serde(default)]
        jsonrpc: Option<String>,
        method: String,
        #[serde(default)]
        params: serde_json::Value,
    },
}

#[derive(Debug, Clone, Deserialize)]
pub struct RpcError {
    pub code: i64,
    pub message: String,
    #[serde(default)]
    pub data: Option<serde_json::Value>,
}

pub fn request<P: Serialize>(id: u64, method: &str, params: &P) -> serde_json::Result<String> {
    let msg = Request {
        jsonrpc: "2.0".to_string(),
        id,
        method: method.to_string(),
        params,
    };
    serde_json::to_string(&msg)
}

pub fn notification<P: Serialize>(method: &str, params: &P) -> serde_json::Result<String> {
    let msg = Notification {
        jsonrpc: "2.0".to_string(),
        method: method.to_string(),
        params,
    };
    serde_json::to_string(&msg)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn server_requests_are_not_empty_responses() {
        for id in [json!(71), json!("approval:1")] {
            let frame: Incoming = serde_json::from_value(json!({
                "jsonrpc": "2.0", "id": id, "method": "item/commandExecution/requestApproval", "params": {"command": "touch file"}
            })).unwrap();
            assert!(matches!(frame, Incoming::Request { .. }));
        }
        let response: Incoming = serde_json::from_value(json!({"id": 1, "result": {}})).unwrap();
        assert!(matches!(response, Incoming::Response { id: 1, .. }));
        let notification: Incoming =
            serde_json::from_value(json!({"method": "item/started", "params": {}})).unwrap();
        assert!(matches!(notification, Incoming::Notification { .. }));
    }
}
