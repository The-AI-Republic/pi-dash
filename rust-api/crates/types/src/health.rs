//! Health DTO returned by `GET /healthz`.

use serde::{Deserialize, Serialize};

/// Liveness report. `version` is the binary's crate version.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HealthStatus {
    pub status: String,
    pub version: String,
}

impl HealthStatus {
    pub fn ok(version: impl Into<String>) -> Self {
        Self {
            status: "ok".to_owned(),
            version: version.into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn health_serializes_to_exact_shape() {
        let json = serde_json::to_string(&HealthStatus::ok("0.1.0")).expect("serialize");
        assert_eq!(json, r#"{"status":"ok","version":"0.1.0"}"#);
    }
}
