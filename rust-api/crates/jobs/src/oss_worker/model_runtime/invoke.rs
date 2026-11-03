//! `Agent.run` for the execute plane (PIDASHCONV-866).
//!
//! Ports the invocation half of `cloud_agent/runtime.py:18-76`
//! (`execute`): the drive ([`drive_execute`][crate::dispatch_execute])
//! already resolved the model, appended `model_started`, resolved the
//! limits, and wraps this call in the execution timeout — [`invoke`]
//! runs the agent loop itself and returns the structured output plus
//! the raw usage.
//!
//! Order of operations mirrors `llm.py:69-90` + `runtime.py:65-76`:
//!
//! * creator + ciphertext load, then key decrypt (resolve already ran
//!   in the drive; a vanished config mid-flight reports
//!   `llm_config_missing`, the `tasks.py:186-189` analog, whose `.code`
//!   the drive passes through to the failed row);
//! * lane build from the resolved model plus the decrypted key
//!   ([`lane_for`], `build_model` `llm.py:95-111` — anything but
//!   `anthropic` falls into the OpenAI-compatible branch);
//! * wire tools in registration order (granted fn names, already
//!   sorted, then the granted GitHub names, then `final_result`
//!   last), transformed per lane;
//! * [`run_loop`][super::agent::run_loop] with retries from the
//!   invocation, per-request settings, and the [`RunTools`] executor;
//! * the loop error maps onto [`InvokeError`] (`Timeout` stays a
//!   timeout, `UsageLimit` carries its message, everything else is a
//!   codeless `Failed` for the drive's classifier).
//!
//! The golden schemas and the usage-extractor table load once per
//! process (embedded, infallible after the test suite pins them).

use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

use sqlx::Row;
use uuid::Uuid;

use crate::dispatch_execute::{
    InvokeError, ModelFailure, ModelInvocation, RawUsage, CODE_LLM_CONFIG_MISSING,
};
use pidash_services::assistant::llm::MSG_NO_PROVIDER;
use pidash_services::dispatch::tools as plan;
use pidash_types::dispatch::CloudAgentOutput;

use super::agent::{run_loop, LoopConfig, LoopError};
use super::keys::KeyResolver;
use super::provider::{
    lane_for, transform_anthropic_schema, transform_openai_schema, Lane, WireTool,
};
use super::schemas::{Golden, ToolSchema};
use super::tools_exec::{RunTools, ToolCtx};
use super::usage::{ProviderTable, UsageNumber};

/// Static invoke inputs: the tool context (pool, settings, HTTP,
/// GitHub app) plus the key resolver. Clones share the key cache and
/// the pools (the seams type clones per registration).
#[derive(Clone)]
pub struct InvokeCtx {
    /// Pool, settings, HTTP client, GitHub app context.
    pub tools: ToolCtx,
    /// BYOK key decryptor (shared in-process cache).
    pub keys: KeyResolver,
}

/// The embedded golden, loaded once.
static GOLDEN: OnceLock<Golden> = OnceLock::new();

/// The loaded golden schemas.
fn golden() -> &'static Golden {
    GOLDEN.get_or_init(|| Golden::load().expect("model-runtime golden parses"))
}

/// The usage-extractor table, loaded once.
static PROVIDER_TABLE: OnceLock<ProviderTable> = OnceLock::new();

/// The loaded usage-extractor table.
fn provider_table() -> &'static ProviderTable {
    PROVIDER_TABLE
        .get_or_init(|| ProviderTable::load(golden()).expect("usage provider table loads"))
}

/// One model execution (`execute`'s `agent.run`, `runtime.py:64-76`).
pub async fn invoke(
    ctx: &InvokeCtx,
    invocation: ModelInvocation,
) -> Result<(CloudAgentOutput, RawUsage), InvokeError> {
    // CE resolves no deployment toolsets (the injected seam answers
    // none); the loop has no leg for them.
    debug_assert!(
        invocation.extra_toolsets.is_empty(),
        "CE resolves no extra toolsets"
    );
    let creator = creator_of(&ctx.tools.pool, invocation.run_id).await?;
    let row = crate::oss_worker::execute_seams::LiveExecuteSeams::llm_row(&ctx.tools.pool, creator)
        .await
        .map_err(|error| {
            InvokeError::Failed(ModelFailure {
                code: None,
                message: error.to_string(),
            })
        })?;
    let ciphertext = gate_row(row.as_ref()).map_err(InvokeError::Failed)?;
    let api_key = ctx.keys.decrypted_key(ciphertext).map_err(|failure| {
        InvokeError::Failed(ModelFailure {
            code: failure.code,
            message: failure.message,
        })
    })?;
    let lane = lane_for(&invocation.model, &api_key);
    let golden = golden();
    let mut wire_tools = Vec::new();
    let mut function_schemas: HashMap<String, ToolSchema> = HashMap::new();
    for name in &invocation.tools {
        let schema = golden
            .function_schema(name)
            .expect("golden covers the granted catalog");
        wire_tools.push(wire_tool(&lane, schema));
        function_schemas.insert(name.clone(), schema.clone());
    }
    let mut mcp_schemas: HashMap<String, ToolSchema> = HashMap::new();
    let mut mcp_names: HashSet<String> = HashSet::new();
    if let Some((mcp, _toolset)) = &invocation.github {
        for name in &mcp.granted {
            let schema = golden
                .mcp_schema(name)
                .expect("golden covers the github grant");
            wire_tools.push(wire_tool(&lane, schema));
            mcp_schemas.insert(name.clone(), schema.clone());
            mcp_names.insert(name.clone());
        }
    }
    wire_tools.push(wire_tool(&lane, golden.output_schema()));
    let executor = RunTools::new(ctx.tools.clone(), invocation.run_id);
    let config = LoopConfig {
        lane: &lane,
        client: &ctx.tools.http,
        extract: provider_table(),
        instructions: invocation.instructions,
        prompt: &invocation.prompt,
        wire_tools: &wire_tools,
        function_schemas: &function_schemas,
        mcp_schemas: &mcp_schemas,
        mcp_names: &mcp_names,
        max_tool_retries: u64::from(invocation.retries),
        max_output_retries: u64::from(invocation.retries),
        mcp_max_retries: u64::from(invocation.retries),
        limits: &invocation.usage_limits,
        max_tokens: invocation.model_settings.max_tokens,
        timeout_secs: invocation.model_settings.timeout_secs.max(0) as u64,
        mcp_timeout_secs: invocation.tool_timeout_secs.max(0) as u64,
    };
    match run_loop(&config, &executor).await {
        Ok(output) => {
            let usage = raw_usage(&output.usage);
            Ok((output.output, usage))
        }
        Err(LoopError::Timeout) => Err(InvokeError::Timeout),
        Err(LoopError::UsageLimit(message)) => Err(InvokeError::UsageLimit(
            pidash_types::dispatch::UsageLimitExceeded::new(message),
        )),
        Err(LoopError::Failed(message)) => Err(InvokeError::Failed(ModelFailure {
            code: None,
            message,
        })),
    }
}

/// The re-read gate (`resolve_byok_model`, `llm.py:69-76`): the
/// drive's resolve already ran, but the row is re-read for the
/// decrypt — a vanished row, or a key emptied between the two reads,
/// reports unconfigured (the `tasks.py:186-189` analog), with the
/// code the drive passes through to the failed row.
fn gate_row(row: Option<&crate::oss_worker::execute_seams::LlmRow>) -> Result<&[u8], ModelFailure> {
    let missing = || ModelFailure {
        code: Some(CODE_LLM_CONFIG_MISSING.to_owned()),
        message: MSG_NO_PROVIDER.to_owned(),
    };
    let Some(row) = row else {
        return Err(missing());
    };
    if !row.has_api_key() {
        return Err(missing());
    }
    Ok(row.api_key_encrypted.as_deref().unwrap_or(&[]))
}

/// One wire tool in registration order: the golden description plus
/// the lane-transformed parameters (`strict` inferred on the OpenAI
/// lane, omitted on the Anthropic lane).
fn wire_tool(lane: &Lane, schema: &ToolSchema) -> WireTool {
    match lane {
        Lane::Anthropic { .. } => WireTool {
            name: schema.name.clone(),
            description: schema.description.clone(),
            parameters: transform_anthropic_schema(&schema.parameters),
            strict: None,
        },
        Lane::OpenAICompatible { .. } => {
            let (parameters, strict) = transform_openai_schema(&schema.parameters);
            WireTool {
                name: schema.name.clone(),
                description: schema.description.clone(),
                parameters,
                strict: Some(strict),
            }
        }
    }
}

/// The run's creator id (`execute` receives the run with its creator;
/// the invocation carries the run id, so re-read it). A missing row
/// is the TOCTOU analog of the drive's `BindingGone`/`RefreshGone`:
/// generic path, no code.
async fn creator_of(pool: &sqlx::PgPool, run_id: Uuid) -> Result<Uuid, InvokeError> {
    let row: Option<sqlx::postgres::PgRow> = sqlx::query(plan::SCOPE_RUN_SQL)
        .bind(run_id)
        .fetch_optional(pool)
        .await
        .map_err(|error| {
            InvokeError::Failed(ModelFailure {
                code: None,
                message: error.to_string(),
            })
        })?;
    match row {
        Some(row) => row.try_get("created_by_id").map_err(|error| {
            InvokeError::Failed(ModelFailure {
                code: None,
                message: error.to_string(),
            })
        }),
        None => Err(InvokeError::Failed(ModelFailure {
            code: None,
            message: "AgentRun matching query does not exist.".to_owned(),
        })),
    }
}

/// The `_usage_report` inputs (`runtime.py:79-88`): the `asdict`
/// fields plus the three getters. Counters are ints in practice; a
/// float counter reports no getter so the `asdict` value (identical
/// in Python, where both read the same attribute) survives the
/// drive's overlay instead of a truncated int.
fn raw_usage(usage: &super::usage::RunUsage) -> RawUsage {
    fn getter(number: UsageNumber) -> Option<i64> {
        match number {
            UsageNumber::Int(value) => Some(value),
            UsageNumber::Float(_) => None,
        }
    }
    RawUsage {
        fields: usage.as_report(),
        input_tokens: getter(usage.input_tokens),
        output_tokens: getter(usage.output_tokens),
        total_tokens: getter(usage.total_tokens()),
    }
}

#[cfg(test)]
mod tests {
    use super::super::usage::RunUsage;
    use super::*;
    use serde_json::Value;

    fn llm_row_with(key: Option<Vec<u8>>) -> crate::oss_worker::execute_seams::LlmRow {
        crate::oss_worker::execute_seams::LlmRow {
            provider_kind: "openai".to_owned(),
            base_url: "https://t/v1".to_owned(),
            model_name: "m".to_owned(),
            api_key_encrypted: key,
        }
    }

    /// The re-read gate (`llm.py:69-76`): a missing row, a missing
    /// key, or an emptied key all report `llm_config_missing` — only
    /// a set non-empty key decrypts.
    #[test]
    fn gate_row_reports_unconfigured_without_key() {
        for row in [
            None,
            Some(llm_row_with(None)),
            Some(llm_row_with(Some(Vec::new()))),
        ] {
            let failure = gate_row(row.as_ref()).expect_err("missing");
            assert_eq!(failure.code.as_deref(), Some(CODE_LLM_CONFIG_MISSING));
            assert_eq!(failure.message, MSG_NO_PROVIDER);
        }
        let row = llm_row_with(Some(vec![7, 7]));
        assert_eq!(gate_row(Some(&row)).expect("key"), [7u8, 7].as_slice());
    }

    fn zero_usage() -> RunUsage {
        RunUsage {
            input_tokens: UsageNumber::Int(0),
            cache_write_tokens: UsageNumber::Int(0),
            cache_read_tokens: UsageNumber::Int(0),
            output_tokens: UsageNumber::Int(0),
            input_audio_tokens: UsageNumber::Int(0),
            cache_audio_read_tokens: UsageNumber::Int(0),
            output_audio_tokens: UsageNumber::Int(0),
            details: Default::default(),
            requests: 0,
            tool_calls: 0,
        }
    }

    /// Int counters report all three getters (the drive overlays them
    /// onto the `asdict` fields, as `_usage_report` does).
    #[test]
    fn raw_usage_reports_int_getters() {
        let mut usage = zero_usage();
        usage.input_tokens = UsageNumber::Int(12);
        usage.output_tokens = UsageNumber::Int(34);
        usage.requests = 2;
        usage.tool_calls = 1;
        let raw = raw_usage(&usage);
        assert_eq!(raw.input_tokens, Some(12));
        assert_eq!(raw.output_tokens, Some(34));
        assert_eq!(raw.total_tokens, Some(46));
        assert_eq!(raw.fields.get("requests"), Some(&Value::Number(2.into())));
        // The drive's overlay reproduces `_usage_report` exactly.
        let report = crate::dispatch_execute::usage_report(&raw);
        assert_eq!(report.get("input_tokens"), Some(&Value::Number(12.into())));
        assert_eq!(report.get("total_tokens"), Some(&Value::Number(46.into())));
    }

    /// A float counter reports no getter, so the `asdict` float (what
    /// Python's getter would return — same attribute) survives the
    /// drive's overlay instead of a truncated int.
    #[test]
    fn raw_usage_omits_float_getters() {
        let mut usage = zero_usage();
        usage.input_tokens = UsageNumber::Float(12.5);
        let raw = raw_usage(&usage);
        assert_eq!(raw.input_tokens, None);
        let report = crate::dispatch_execute::usage_report(&raw);
        assert_eq!(
            report.get("input_tokens"),
            Some(
                &serde_json::Number::from_f64(12.5)
                    .map(Value::Number)
                    .unwrap()
            )
        );
    }
}
