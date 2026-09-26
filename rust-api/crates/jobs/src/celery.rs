//! Celery protocol v2 publisher format.
//!
//! A published task is a JSON body of `[args, kwargs, embed]` sent with
//! headers carrying `id`, `task`, `lang`, and `retries`. Python workers
//! consume these messages unchanged while the Rust worker loop (F-09) is
//! being built, so this shape is load-bearing: any drift breaks coexistence.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// A task message ready to publish to the broker.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CeleryTaskMessage {
    pub id: String,
    pub task: String,
    pub args: Vec<Value>,
    pub kwargs: Map<String, Value>,
    pub retries: u32,
}

impl CeleryTaskMessage {
    /// Create a first-attempt message with a fresh id.
    pub fn new(task: impl Into<String>, args: Vec<Value>, kwargs: Map<String, Value>) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            task: task.into(),
            args,
            kwargs,
            retries: 0,
        }
    }

    /// AMQP headers Celery workers expect.
    pub fn headers(&self) -> Map<String, Value> {
        let mut headers = Map::new();
        headers.insert("id".to_owned(), Value::String(self.id.clone()));
        headers.insert("task".to_owned(), Value::String(self.task.clone()));
        headers.insert("lang".to_owned(), Value::String("py".to_owned()));
        headers.insert("retries".to_owned(), Value::Number(self.retries.into()));
        headers
    }

    /// Protocol v2 body: `[args, kwargs, embed]`.
    pub fn body(&self) -> Value {
        Value::Array(vec![
            Value::Array(self.args.clone()),
            Value::Object(self.kwargs.clone()),
            Value::Object(Map::new()),
        ])
    }

    /// The full wire payload: headers plus the serialized body.
    pub fn to_wire(&self) -> (Map<String, Value>, Vec<u8>) {
        let body = serde_json::to_vec(&self.body()).expect("body is JSON");
        (self.headers(), body)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn message() -> CeleryTaskMessage {
        CeleryTaskMessage {
            id: "task-id-1".to_owned(),
            task: "pi_dash.jobs.ping".to_owned(),
            args: vec![json!(1)],
            kwargs: Map::new(),
            retries: 0,
        }
    }

    #[test]
    fn headers_carry_identity_and_lang() {
        let headers = message().headers();
        assert_eq!(headers["id"], "task-id-1");
        assert_eq!(headers["task"], "pi_dash.jobs.ping");
        assert_eq!(headers["lang"], "py");
    }

    #[test]
    fn body_is_args_kwargs_embed_triple() {
        let body = message().body();
        assert_eq!(body, json!([[1], {}, {}]));
    }

    #[test]
    fn wire_body_round_trips() {
        let (headers, bytes) = message().to_wire();
        assert_eq!(headers["id"], "task-id-1");
        let body: Value = serde_json::from_slice(&bytes).expect("json");
        assert_eq!(body, json!([[1], {}, {}]));
    }

    #[test]
    fn new_assigns_unique_ids() {
        let first = CeleryTaskMessage::new("t", vec![], Map::new());
        let second = CeleryTaskMessage::new("t", vec![], Map::new());
        assert_ne!(first.id, second.id);
        assert_eq!(first.retries, 0);
    }
}
