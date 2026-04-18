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

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum Incoming {
    Response {
        jsonrpc: String,
        id: u64,
        #[serde(default)]
        result: Option<serde_json::Value>,
        #[serde(default)]
        error: Option<RpcError>,
    },
    Notification {
        jsonrpc: String,
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
