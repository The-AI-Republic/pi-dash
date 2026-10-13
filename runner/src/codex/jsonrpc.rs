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
    /// A request the server makes of us; it waits for a response carrying the
    /// same id. Listed first: a frame with both `id` and `method` would
    /// otherwise parse as a `Response`.
    Request {
        #[serde(default)]
        jsonrpc: Option<String>,
        id: serde_json::Value,
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

/// Answer a request the server made of us, echoing its id.
pub fn response<R: Serialize>(id: &serde_json::Value, result: &R) -> serde_json::Result<String> {
    #[derive(Serialize)]
    struct Response<'a, R> {
        jsonrpc: &'a str,
        id: &'a serde_json::Value,
        result: &'a R,
    }
    serde_json::to_string(&Response {
        jsonrpc: "2.0",
        id,
        result,
    })
}

/// Refuse a request the server made of us, echoing its id.
pub fn error_response(
    id: &serde_json::Value,
    code: i64,
    message: &str,
) -> serde_json::Result<String> {
    #[derive(Serialize)]
    struct Refusal<'a> {
        jsonrpc: &'a str,
        id: &'a serde_json::Value,
        error: Error<'a>,
    }
    #[derive(Serialize)]
    struct Error<'a> {
        code: i64,
        message: &'a str,
    }
    serde_json::to_string(&Refusal {
        jsonrpc: "2.0",
        id,
        error: Error { code, message },
    })
}
