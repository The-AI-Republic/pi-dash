//! State shared by every handler.

/// Data every handler can read. Extended by later issues (pools under F-04,
/// session keys under F-05); handlers take it by extractor, never globals.
#[derive(Debug, Clone)]
pub struct AppState {
    version: String,
}

impl AppState {
    pub fn new(version: impl Into<String>) -> Self {
        Self {
            version: version.into(),
        }
    }

    pub fn version(&self) -> &str {
        &self.version
    }
}
