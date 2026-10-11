//! Golden tool-schema loader (`fx-disp-08-model-runtime.golden.json`).
//!
//! The model runtime sends the exact tool/parameter schemas pydantic-ai
//! 1.107.0 generates: the golden records them from the real `Agent` tool
//! defs (function tools), a FastMCP `list_tools` capture over
//! AST-asserted replica wrappers (MCP leg), and a wire capture
//! (`final_result`). This module loads the embedded golden and serves
//! schema lookups; [`super::validation`] validates calls against the
//! loaded schemas and [`super::provider`] serializes them onto the wire.
//!
//! Key order inside `parameters` objects is significant on the wire
//! (function tools carry `additionalProperties:false` FIRST, the output
//! tool LAST); the bin's `serde_json` `preserve_order` feature keeps the
//! golden's document order through parse and re-emit.

use serde_json::Value;
use std::collections::HashMap;

/// The embedded golden document (D-11 fixture, PIDASHCONV-866).
const GOLDEN: &str =
    include_str!("../../../../fixtures/dispatch/fx-disp-08-model-runtime.golden.json");

/// Golden load failure: the embedded fixture is corrupt or has drifted
/// from the shape the runtime expects. Fail-closed at worker boot.
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

/// The loaded golden: schema maps plus the usage-extractor table.
#[derive(Debug, Clone)]
pub struct Golden {
    function_schemas: HashMap<String, ToolSchema>,
    mcp_schemas: HashMap<String, ToolSchema>,
    output_schema: ToolSchema,
    document: Value,
}

impl Golden {
    /// Parse and index the embedded golden.
    pub fn load() -> Result<Self, GoldenError> {
        let document: Value =
            serde_json::from_str(GOLDEN).map_err(|err| GoldenError(err.to_string()))?;
        let section = |key: &str| {
            document
                .get(key)
                .and_then(Value::as_object)
                .ok_or_else(|| GoldenError(format!("missing object section `{key}`")))
        };
        let mut function_schemas = HashMap::new();
        for (name, def) in section("function_schemas")? {
            function_schemas.insert(name.clone(), tool_schema(name, def)?);
        }
        let mut mcp_schemas = HashMap::new();
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
