//! Usage-table loader (`fixtures/cloud_agent/usage-providers.golden.json`).
//!
//! The chat engine reads only the `usage_providers` section at boot
//! (agent naming convention P2, section 4.4); the `usage_vectors`
//! section backs the usage replay tests. [`load`] requires exactly
//! those two sections: the retired path's schemas live in the
//! legacy `fx-disp-08` golden, which P3 deletes with its tests.
//!
//! The legacy loader ([`Golden`]) is kept in this module until P3:
//! the retired battery tests still read it. Nothing live calls it;
//! [`load`] is the boot path.

use serde_json::Value;

/// The cloud-agent usage fixture: `usage_providers` (run time) and
/// `usage_vectors` (usage replay tests).
const USAGE_GOLDEN: &str =
    include_str!("../../../../fixtures/cloud_agent/usage-providers.golden.json");

/// Usage-table load failure: the embedded fixture is corrupt or has
/// drifted from the shape the engine expects. Fail-closed at boot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UsageTableError(pub String);

impl std::fmt::Display for UsageTableError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "usage table invalid: {}", self.0)
    }
}

impl std::error::Error for UsageTableError {}

/// The loaded usage fixture document ([`load`]).
#[derive(Debug, Clone)]
pub struct UsageDocument {
    document: Value,
}

impl UsageDocument {
    /// The provider rows (`usage_providers`).
    pub fn usage_providers(&self) -> Result<&Vec<Value>, UsageTableError> {
        self.document
            .get("usage_providers")
            .and_then(Value::as_array)
            .ok_or_else(|| UsageTableError("missing `usage_providers`".to_string()))
    }

    /// The replay vectors (`usage_vectors`).
    pub fn usage_vectors(&self) -> Result<&Vec<Value>, UsageTableError> {
        self.document
            .get("usage_vectors")
            .and_then(Value::as_array)
            .ok_or_else(|| UsageTableError("missing `usage_vectors`".to_string()))
    }
}

/// Parse and validate the cloud-agent usage fixture. Requires exactly
/// the two sections the engine and its tests use; anything else is
/// drift and fails closed.
pub fn load() -> Result<UsageDocument, UsageTableError> {
    let document: Value =
        serde_json::from_str(USAGE_GOLDEN).map_err(|err| UsageTableError(err.to_string()))?;
    let doc = UsageDocument { document };
    let _ = doc.usage_providers()?;
    let _ = doc.usage_vectors()?;
    Ok(doc)
}

// ---------------------------------------------------------------------------
// Legacy golden loader (fx-disp-08): retired-path tests only, until P3.
// ---------------------------------------------------------------------------

/// The legacy embedded golden document (D-11 fixture, PIDASHCONV-866).
const LEGACY_GOLDEN: &str =
    include_str!("../../../../fixtures/dispatch/fx-disp-08-model-runtime.golden.json");

/// Legacy golden load failure (P3 deletes this loader with the fixture).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GoldenError(pub String);

impl std::fmt::Display for GoldenError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "model-runtime golden invalid: {}", self.0)
    }
}

impl std::error::Error for GoldenError {}

/// One model-facing tool definition: description + parameter schema.
#[derive(Debug, Clone)]
pub struct ToolSchema {
    /// Tool name (`pidash_search_project_issues`, `final_result`, ...).
    pub name: String,
    /// Docstring-derived description (empty string when absent).
    pub description: String,
    /// The `parameters` / `input_schema` object, document order preserved.
    pub parameters: Value,
}

/// The loaded legacy golden: schema maps plus the usage-extractor table.
#[derive(Debug, Clone)]
pub struct Golden {
    function_schemas: std::collections::HashMap<String, ToolSchema>,
    mcp_schemas: std::collections::HashMap<String, ToolSchema>,
    output_schema: ToolSchema,
    document: Value,
}

impl Golden {
    /// Parse and index the legacy embedded golden (P3 deletes).
    pub fn load() -> Result<Self, GoldenError> {
        let document: Value =
            serde_json::from_str(LEGACY_GOLDEN).map_err(|err| GoldenError(err.to_string()))?;
        let section = |key: &str| {
            document
                .get(key)
                .and_then(Value::as_object)
                .ok_or_else(|| GoldenError(format!("missing object section `{key}`")))
        };
        let mut function_schemas = std::collections::HashMap::new();
        for (name, def) in section("function_schemas")? {
            function_schemas.insert(name.clone(), tool_schema(name, def)?);
        }
        let mut mcp_schemas = std::collections::HashMap::new();
        for (name, def) in section("mcp_schemas")? {
            mcp_schemas.insert(name.clone(), tool_schema(name, def)?);
        }
        let output = document
            .get("output_schema")
            .ok_or_else(|| GoldenError("missing `output_schema`".to_string()))?;
        let output_name = output
            .get("name")
            .and_then(Value::as_str)
            .ok_or_else(|| GoldenError("output schema has no `name`".to_string()))?;
        let output_schema = tool_schema(output_name, output)?;
        if function_schemas.len() != 15 {
            return Err(GoldenError(format!(
                "expected 15 function schemas, found {}",
                function_schemas.len()
            )));
        }
        Ok(Golden {
            function_schemas,
            mcp_schemas,
            output_schema,
            document,
        })
    }

    /// Function-tool schema by name (`None` for unknown names).
    pub fn function_schema(&self, name: &str) -> Option<&ToolSchema> {
        self.function_schemas.get(name)
    }

    /// MCP-leg schema by name (`github_get_file`, ...).
    pub fn mcp_schema(&self, name: &str) -> Option<&ToolSchema> {
        self.mcp_schemas.get(name)
    }

    /// The `final_result` output-tool schema.
    pub fn output_schema(&self) -> &ToolSchema {
        &self.output_schema
    }

    /// The raw golden document (usage table, battery replay).
    pub fn document(&self) -> &Value {
        &self.document
    }
}

fn tool_schema(name: &str, def: &Value) -> Result<ToolSchema, GoldenError> {
    let description = def
        .get("description")
        .and_then(Value::as_str)
        .ok_or_else(|| GoldenError(format!("schema `{name}` has no `description`")))?;
    let parameters = def
        .get("parameters")
        .ok_or_else(|| GoldenError(format!("schema `{name}` has no `parameters`")))?;
    Ok(ToolSchema {
        name: name.to_string(),
        description: description.to_string(),
        parameters: parameters.clone(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn usage_fixture_loads_with_both_sections() {
        let doc = load().expect("usage fixture parses");
        assert_eq!(doc.usage_providers().expect("providers").len(), 43);
        assert_eq!(doc.usage_vectors().expect("vectors").len(), 66);
    }

    #[test]
    fn golden_loads_with_expected_counts() {
        let golden = Golden::load().expect("golden parses");
        assert_eq!(golden.function_schemas.len(), 15);
        assert_eq!(golden.mcp_schemas.len(), 2);
        assert_eq!(golden.output_schema.name, "final_result");
        assert_eq!(
            golden.output_schema.description,
            "The final response which ends this conversation"
        );
    }

    #[test]
    fn parameter_key_order_matches_wire() {
        let golden = Golden::load().expect("golden parses");
        let search = golden
            .function_schema("pidash_search_project_issues")
            .expect("search schema");
        let keys: Vec<&str> = search
            .parameters
            .as_object()
            .expect("parameters object")
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(
            keys,
            vec!["additionalProperties", "properties", "required", "type"],
            "function tools carry additionalProperties first"
        );
        let out_keys: Vec<&str> = golden
            .output_schema
            .parameters
            .as_object()
            .expect("output parameters object")
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(
            out_keys.last(),
            Some(&"additionalProperties"),
            "output tool carries additionalProperties last"
        );
    }

    #[test]
    fn relate_schema_pins_list_shape() {
        let golden = Golden::load().expect("golden parses");
        let relate = golden
            .function_schema("pidash_relate_issues")
            .expect("relate schema");
        let related = &relate.parameters["properties"]["related_issues"];
        assert_eq!(related["type"], Value::String("array".to_string()));
        assert_eq!(
            related["items"]["type"],
            Value::String("string".to_string())
        );
        assert_eq!(
            relate.parameters["required"],
            serde_json::json!(["issue", "relation_type", "related_issues"])
        );
    }
}
