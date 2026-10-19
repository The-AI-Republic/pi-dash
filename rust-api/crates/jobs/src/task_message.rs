#![forbid(unsafe_code)]

//! The job envelope a ported call site builds before it enqueues onto the
//! Postgres queue.
//!
//! Mirror of Django's `task.delay(*args, **kwargs)` call shape without the
//! broker: the Celery task name with its positional args and keyword kwargs,
//! carried unchanged into [`crate::queue::NewJob`] (the frozen job-payload
//! contract). A call site that ports a `.delay()` builds one of these and
//! calls its local `enqueue_message` helper for the transaction.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// One task invocation: task name, positional args, keyword kwargs.
///
/// Field-for-field this is the `.delay(*args, **kwargs)` call without the
/// transport. `new` mirrors `.delay()` (fresh id, first attempt); the id is
/// informational only — the Postgres queue mints its own row id.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TaskMessage {
    pub id: String,
    pub task: String,
    pub args: Vec<Value>,
    pub kwargs: Map<String, Value>,
}

impl TaskMessage {
    /// Create a first-attempt message with a fresh id (the `.delay()` path).
    pub fn new(task: impl Into<String>, args: Vec<Value>, kwargs: Map<String, Value>) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            task: task.into(),
            args,
            kwargs,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn new_assigns_unique_ids() {
        let first = TaskMessage::new("t", vec![], Map::new());
        let second = TaskMessage::new("t", vec![json!(1)], Map::new());
        assert_ne!(first.id, second.id);
        assert_eq!(first.task, "t");
        assert_eq!(second.args, vec![json!(1)]);
        assert!(second.kwargs.is_empty());
    }
}
