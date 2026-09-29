//! MCP tool servers for the assistant runtime (D-06, stage 5).
//!
//! Ports `apps/api/pi_dash/assistant/runtime/mcp.py:1-309`: the connect/read
//! timeouts and server cap, `SkippedServer`, the fail-open `ResilientToolset`
//! policy, `build_toolset`, `unique_prefixes`, `_auth_header_for`,
//! `build_toolsets` and the settings readers. Fixture id F-A6-09
//! (`rust-api/fixtures/assistant/mcp.json`).
//!
//! Shape notes:
//!
//! * Building a pydantic-ai `MCPToolset` performs no I/O, and the rmcp
//!   transport lives with the handler layer, which owns network handles
//!   (the `llm.rs` precedent: this crate holds no provider client; the
//!   `services/Cargo.toml` of this foundation crate is read-only so no new
//!   dependency is added here). This module ports the pure decision surface:
//!   timeout/cap values, prefix assignment, the per-server skip planner, the
//!   toolset descriptor ([`ToolsetSpec`]), and the resilient failure policy
//!   as a pure state machine ([`ResilientState`]).
//! * `ResilientToolset` rebuild semantics (`mcp.py:96-105`): every field
//!   carries a default because pydantic-ai rebuilds wrappers through
//!   `__init__`; the Rust struct carries the same three carried fields
//!   (`server_name`, `prefix`, `failure`) plus the `_entered` bit, and a
//!   rebuilt wrapper starts with `failure: None`, exactly like Python's
//!   `init=False` run state.
//! * Control-flow exceptions (`mcp.py:57-64`) are resolved by name so a
//!   version that renames one degrades to treating it as a server failure;
//!   [`is_control_flow`] is that name check.

use std::collections::HashMap;

use pidash_types::assistant::errors::AssistantError;

/// Connect timeout, seconds (`mcp.py:41`, `DEFAULT_TIMEOUT_S`).
pub const DEFAULT_TIMEOUT_S: f64 = 10.0;
/// Read timeout for a single MCP call, seconds (`mcp.py:45`,
/// `DEFAULT_READ_TIMEOUT_S`). Stays generous: tool calls traverse a third
/// upstream and routinely need minutes.
pub const DEFAULT_READ_TIMEOUT_S: f64 = 300.0;
/// Ceiling on enabled servers per user (`mcp.py:51`, `DEFAULT_MAX_SERVERS`).
pub const DEFAULT_MAX_SERVERS: i64 = 10;

/// Overflow skip reason (`build_toolsets`, `mcp.py:263`).
pub const REASON_TOO_MANY_SERVERS: &str = "too_many_servers";
/// SSRF skip reason (`mcp.py:268`).
pub const REASON_URL_BLOCKED: &str = "url_blocked";
/// Undecryptable-header skip reason for non-`AssistantError` failures
/// (`mcp.py:278`).
pub const REASON_AUTH_HEADER_UNREADABLE: &str = "auth_header_unreadable";
/// Toolset-construction skip reason (`mcp.py:294`).
pub const REASON_TOOLSET_UNAVAILABLE: &str = "toolset_unavailable";

/// Connect-timeout setting (`_timeout_setting`, `mcp.py:299-300`):
/// `float(ASSISTANT_MCP_TIMEOUT_S or DEFAULT_TIMEOUT_S)`.
pub fn timeout_setting(override_value: Option<f64>) -> f64 {
    override_value.unwrap_or(DEFAULT_TIMEOUT_S)
}

/// Read-timeout setting (`_read_timeout_setting`, `mcp.py:303-304`).
pub fn read_timeout_setting(override_value: Option<f64>) -> f64 {
    override_value.unwrap_or(DEFAULT_READ_TIMEOUT_S)
}

/// Server-cap setting (`max_servers`, `mcp.py:307-309):
/// `int(ASSISTANT_MCP_MAX_SERVERS or DEFAULT_MAX_SERVERS)`.
///
/// A negative configured value saturates to zero here rather than
/// reproducing Python's negative-slice behaviour (`servers[:-N]`), which has
/// no exercised meaning: the cap is a non-negative operator setting.
pub fn max_servers(override_value: Option<i64>) -> usize {
    override_value.unwrap_or(DEFAULT_MAX_SERVERS).max(0) as usize
}

/// Slugified tool prefix for one server row (`tool_prefix`,
/// `models.py:226-235`):
/// `re.sub(r"[^a-z0-9]+", "_", name.strip().lower()).strip("_")` under the
/// reserved `mcp_` namespace, falling back to the row id (`nodash[:8]`) when
/// the name has no alphanumeric content.
///
/// `row_id` is the `nodash` form source: pass the raw UUID string; dashes
/// are stripped here, matching `str(self.id).replace('-', '')[:8]`.
/// (The `[:8]` slice counts ASCII hex digits, so byte slicing is exact.)
pub fn tool_prefix_for_name(name: &str, row_id: &str) -> String {
    let lowered = name.trim().to_lowercase();
    let mut slug = String::with_capacity(lowered.len());
    let mut last_was_underscore = true; // suppress a leading run, like strip("_")
    for ch in lowered.chars() {
        if ch.is_ascii_alphanumeric() {
            slug.push(ch);
            last_was_underscore = false;
        } else if !last_was_underscore {
            slug.push('_');
            last_was_underscore = true;
        }
    }
    if last_was_underscore {
        slug.pop();
    }
    if slug.is_empty() {
        let nodash: String = row_id.chars().filter(|c| *c != '-').collect();
        let head: String = nodash.chars().take(8).collect();
        return format!("mcp_{head}");
    }
    format!("mcp_{slug}")
}

/// Map each server to a tool prefix unique within the run
/// (`unique_prefixes`, `mcp.py:212-233`).
///
/// `servers` is `(pk, tool_prefix)` in `created_at` order (the model Meta
/// ordering); collisions append `_2`, `_3`, … so a server's prefix is stable
/// as long as the ones before it are unchanged. Keyed by server pk.
pub fn unique_prefixes(servers: &[(String, String)]) -> HashMap<String, String> {
    let mut used: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut assigned: HashMap<String, String> = HashMap::with_capacity(servers.len());
    for (pk, base) in servers {
        let mut candidate = base.clone();
        let mut n = 2u32;
        while used.contains(&candidate) {
            candidate = format!("{base}_{n}");
            n += 1;
        }
        used.insert(candidate.clone());
        assigned.insert(pk.clone(), candidate);
    }
    assigned
}

/// A server that could not be turned into a toolset, and why
/// (`SkippedServer`, `mcp.py:66-71`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkippedServer {
    pub name: String,
    pub reason: String,
}

impl SkippedServer {
    pub fn new(name: impl Into<String>, reason: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            reason: reason.into(),
        }
    }
}

/// Auth-header decrypt failure (`build_toolsets`, `mcp.py:271-279`):
/// an `AssistantError` (crypto not configured, retired key) versus any other
/// failure (a broken row that must not break the turn).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DecryptFailure {
    /// Yield `err.code()` as the skip reason.
    Assistant(AssistantError),
    /// Yield `"auth_header_unreadable"`.
    Opaque,
}

/// Resolve one row's `Authorization` header value (`_auth_header_for` +
/// its call site, `mcp.py:236-239,271-279`).
///
/// Returns `Ok(None)` without invoking `decrypt` when the row holds no
/// header (`if not server.has_auth_header: return None`); otherwise the
/// decrypted value, or the skip reason.
pub fn auth_header_for(
    has_auth_header: bool,
    decrypt: impl FnOnce() -> Result<String, DecryptFailure>,
) -> Result<Option<String>, String> {
    if !has_auth_header {
        return Ok(None);
    }
    match decrypt() {
        Ok(header) => Ok(Some(header)),
        Err(DecryptFailure::Assistant(err)) => Err(err.code().to_owned()),
        Err(DecryptFailure::Opaque) => Err(REASON_AUTH_HEADER_UNREADABLE.to_owned()),
    }
}

/// One enabled MCP server row as `build_toolsets` sees it (`mcp.py:250`):
/// the `filter(user=user, is_enabled=True)` rows in `created_at` order.
/// The SSRF verdict (`url_blocked`) is computed by the caller per row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerRow {
    pub pk: String,
    pub name: String,
    pub url: String,
    pub has_auth_header: bool,
    pub url_blocked: bool,
}

/// A row that survived the cap and SSRF gates and is ready to build.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerPlan {
    pub pk: String,
    pub name: String,
    pub url: String,
    pub prefix: String,
    pub needs_auth: bool,
}

/// Cap, prefix and SSRF gates (`build_toolsets`, `mcp.py:248-269`).
///
/// Prefixes are assigned over the full row list *before* the cap slice
/// (`prefixes = unique_prefixes(servers)` precedes the overflow split), so
/// an over-cap server's prefix is simply unused. Overflow rows yield
/// `"too_many_servers"` skips in `created_at` order; the defence-in-depth
/// comment (`mcp.py:258-261`) applies: rows can predate the cap or outlive
/// a lowered one, and the overflow is reported, never dropped silently.
/// SSRF-blocked rows yield `"url_blocked"`; both gates never raise.
pub fn plan_servers(rows: &[ServerRow], limit: usize) -> (Vec<ServerPlan>, Vec<SkippedServer>) {
    let bases: Vec<(String, String)> = rows
        .iter()
        .map(|row| (row.pk.clone(), tool_prefix_for_name(&row.name, &row.pk)))
        .collect();
    let prefixes = unique_prefixes(&bases);

    let mut skipped = Vec::new();
    if rows.len() > limit {
        for row in &rows[limit..] {
            skipped.push(SkippedServer::new(&row.name, REASON_TOO_MANY_SERVERS));
        }
    }
    let mut plans = Vec::new();
    for row in rows.iter().take(limit) {
        if row.url_blocked {
            skipped.push(SkippedServer::new(&row.name, REASON_URL_BLOCKED));
            continue;
        }
        plans.push(ServerPlan {
            pk: row.pk.clone(),
            name: row.name.clone(),
            url: row.url.clone(),
            prefix: prefixes.get(&row.pk).cloned().unwrap_or_default(),
            needs_auth: row.has_auth_header,
        });
    }
    (plans, skipped)
}

/// Auth + construction gates (`build_toolsets`, `mcp.py:270-296`).
///
/// For each planned row: resolve the auth header (skipping decrypt entirely
/// when the row holds none), then construct the toolset. Either failure
/// yields a skip — `err.code()` for `AssistantError` decrypt failures,
/// `"auth_header_unreadable"` otherwise, `"toolset_unavailable"` when
/// construction fails — and never raises for a per-server problem, so one
/// bad row cannot break every turn the user takes. `T` is the handler
/// layer's toolset handle (construction performs no I/O either way).
pub fn assemble_toolsets<T>(
    plans: &[ServerPlan],
    resolve_auth: impl Fn(&ServerPlan) -> Result<Option<String>, String>,
    construct: impl Fn(&ServerPlan, Option<String>) -> Result<T, ()>,
) -> (Vec<T>, Vec<SkippedServer>) {
    let mut toolsets = Vec::new();
    let mut skipped = Vec::new();
    for plan in plans {
        let auth_header = match resolve_auth(plan) {
            Ok(header) => header,
            Err(reason) => {
                skipped.push(SkippedServer::new(&plan.name, reason));
                continue;
            }
        };
        match construct(plan, auth_header) {
            Ok(toolset) => toolsets.push(toolset),
            Err(()) => skipped.push(SkippedServer::new(&plan.name, REASON_TOOLSET_UNAVAILABLE)),
        }
    }
    (toolsets, skipped)
}

/// One streamable-HTTP MCP toolset as `build_toolset` wires it
/// (`mcp.py:175-209`): `MCPToolset(url, headers, include_instructions,
/// init_timeout, read_timeout)` under the prefixing wrapper, outermost the
/// [`ResilientState`] wrapper so it also absorbs failures raised by the
/// prefix wrapper. The live transport handle stays with the handler layer;
///
/// this descriptor carries exactly what the constructor selects.
#[derive(Debug, Clone, PartialEq)]
pub struct ToolsetSpec {
    pub url: String,
    /// `{"Authorization": auth_header} if auth_header else None`
    /// (`mcp.py:190`): empty values read as absent, like Python truthiness.
    pub auth_header: Option<String>,
    pub tool_prefix: String,
    pub include_instructions: bool,
    pub timeout: f64,
    pub read_timeout: f64,
    /// `server_name or url` (`mcp.py:209`).
    pub server_name: String,
    /// `tool_prefix or ""`, carried on the outermost wrapper (`mcp.py:209`).
    pub prefix: String,
}

/// Build one toolset descriptor (`build_toolset`, `mcp.py:175-209`).
///
/// `include_instructions` forwards the server's `instructions` to the model
/// (default false in pydantic-ai; on when the server describes a discovery
/// protocol). Streamable HTTP is the transport for http URLs (the
/// deprecated `MCPServerStreamableHTTP` is not used).
pub fn build_toolset_spec(
    url: &str,
    auth_header: Option<&str>,
    tool_prefix: Option<&str>,
    include_instructions: bool,
    timeout: f64,
    read_timeout: f64,
    server_name: &str,
) -> ToolsetSpec {
    let header = auth_header.filter(|h| !h.is_empty()).map(str::to_owned);
    let prefix = tool_prefix.filter(|p| !p.is_empty()).unwrap_or("");
    ToolsetSpec {
        url: url.to_owned(),
        auth_header: header,
        tool_prefix: prefix.to_owned(),
        include_instructions,
        timeout,
        read_timeout,
        server_name: if server_name.is_empty() {
            url.to_owned()
        } else {
            server_name.to_owned()
        },
        prefix: prefix.to_owned(),
    }
}

/// pydantic-ai control-flow exceptions that pass through `call_tool`
/// untouched (`_CONTROL_FLOW_EXCEPTIONS`, `mcp.py:57-64`): decisions, not
/// outages, acted on by the tool manager. Resolved by name so a version
/// that renames or drops one degrades to treating it as a server failure
/// instead of failing at import.
pub const CONTROL_FLOW_EXCEPTIONS: &[&str] = &[
    "ModelRetry",
    "ToolRetryError",
    "SkipToolExecution",
    "CallDeferred",
    "ApprovalRequired",
];

/// Whether an exception class name is control flow (`except
/// _CONTROL_FLOW_EXCEPTIONS: raise`, `mcp.py:168-169`).
pub fn is_control_flow(exc_class: &str) -> bool {
    CONTROL_FLOW_EXCEPTIONS.contains(&exc_class)
}

/// Fail-open wrapper state (`ResilientToolset`, `mcp.py:74-172`).
///
/// A failing server degrades instead of failing the turn at all three
/// points where it can reach out — connecting, listing tools, and calling
/// one — plus teardown. `failure` records the exception class so the caller
/// can tell the user which server was dropped; a rebuilt wrapper starts
/// clean (`failure: None`), matching Python's `init=False` run state. The
/// handler layer drives these transitions around the live transport.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResilientState {
    pub server_name: String,
    pub prefix: String,
    pub failure: Option<String>,
    pub entered: bool,
}

impl ResilientState {
    pub fn new(server_name: impl Into<String>, prefix: impl Into<String>) -> Self {
        Self {
            server_name: server_name.into(),
            prefix: prefix.into(),
            failure: None,
            entered: false,
        }
    }

    /// Record a failure and keep the turn alive (`_record`, `mcp.py:113-120`).
    pub fn record(&mut self, exc_class: &str) {
        self.failure = Some(exc_class.to_owned());
    }

    /// Connect outcome (`__aenter__`, `mcp.py:122-127`): success marks the
    /// wrapper entered; a dead server records and the turn continues.
    pub fn connect_failed(&mut self, exc_class: &str) {
        self.record(exc_class);
    }

    pub fn connect_succeeded(&mut self) {
        self.entered = true;
    }

    /// Whether the wrapped `__aexit__` must run (`__aexit__`, `mcp.py:129-133`):
    /// never entered means nothing to unwind — calling it would raise on a
    /// half-built connection. Returns false when close is a no-op.
    pub fn should_close(&self) -> bool {
        self.entered
    }

    /// Teardown outcome (`__aexit__`, `mcp.py:134-145`): a transport that
    /// disappears before the close handshake is the same additive-server
    /// outage — record it, do not replace the turn's outcome.
    pub fn close_failed(&mut self, exc_class: &str) {
        self.record(exc_class);
    }

    /// Whether tools may be listed (`get_tools`, `mcp.py:147-154`): a
    /// recorded failure reads as no tools; a list failure records and reads
    /// as no tools under the same rule as connect.
    pub fn should_list_tools(&self) -> bool {
        self.failure.is_none()
    }

    pub fn list_failed(&mut self, exc_class: &str) {
        self.record(exc_class);
    }

    /// Mid-run tool-call failure (`call_tool`, `mcp.py:156-172`).
    ///
    /// The server was reachable at turn start or the tool would not be on
    /// offer, so a failure here is the server dying mid-turn: absorb it into
    /// the tool's own result and let the model react, rather than ending the
    /// turn or burning retries on a server that is not coming back.
    pub fn call_failed(&mut self, exc_class: &str) -> CallOutcome {
        if is_control_flow(exc_class) {
            return CallOutcome::Passthrough;
        }
        self.record(exc_class);
        CallOutcome::Absorbed(absorb_message(&self.server_name, exc_class))
    }
}

/// What a mid-run tool failure becomes (`call_tool`, `mcp.py:156-172`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CallOutcome {
    /// Control-flow exception: re-raise untouched.
    Passthrough,
    /// Server outage: the tool's result text, served to the model.
    Absorbed(String),
}

/// The absorbed-call result text (`mcp.py:172`):
/// `f"Tool server {server_name!r} was unavailable for this call ({Exc})."`
pub fn absorb_message(server_name: &str, exc_class: &str) -> String {
    format!(
        "Tool server {} was unavailable for this call ({}).",
        py_repr(server_name),
        exc_class
    )
}

/// CPython `repr` for `str`, for the `{server_name!r}` interpolation.
///
/// Single-quote form unless the value contains a lone `'` (then double
/// quotes, exactly like `repr("it's")`); backslash and C0 controls take
/// their short escapes, other non-printables take `\x`/`\u`/`\U`, and
/// printable non-ASCII passes through verbatim.
pub fn py_repr(value: &str) -> String {
    let use_double = value.contains('\'') && !value.contains('"');
    let (open, close) = if use_double { ('"', '"') } else { ('\'', '\'') };
    let mut out = String::with_capacity(value.len() + 2);
    out.push(open);
    for ch in value.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\'' if !use_double => out.push_str("\\'"),
            '"' if use_double => out.push_str("\\\""),
            c if (c < ' ' || c == '\u{7f}') && (c as u32) <= 0xff => {
                out.push_str(&format!("\\x{:02x}", c as u32));
            }
            c if (c < ' ' || c == '\u{7f}') && (c as u32) <= 0xffff => {
                out.push_str(&format!("\\u{:04x}", c as u32));
            }
            c if c < ' ' || c == '\u{7f}' => {
                out.push_str(&format!("\\U{:08x}", c as u32));
            }
            c => out.push(c),
        }
    }
    out.push(close);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> serde_json::Value {
        let path = format!(
            "{}/../../fixtures/assistant/mcp.json",
            env!("CARGO_MANIFEST_DIR")
        );
        serde_json::from_str(&std::fs::read_to_string(&path).expect("fixture exists"))
            .expect("fixture parses")
    }

    #[test]
    fn consts_match_python_defaults() {
        let fx = fixture();
        let values = &fx["consts"]["values"];
        assert_eq!(DEFAULT_TIMEOUT_S, values["timeout"].as_f64().unwrap());
        assert_eq!(
            DEFAULT_READ_TIMEOUT_S,
            values["read_timeout"].as_f64().unwrap()
        );
        assert_eq!(DEFAULT_MAX_SERVERS, values["max_servers"].as_i64().unwrap());
    }

    #[test]
    fn settings_override_vectors() {
        let fx = fixture();
        let vectors = fx["settings"]["override_vectors"].as_array().unwrap();
        assert_eq!(timeout_setting(Some(vectors[0].as_f64().unwrap())), 5.0);
        assert_eq!(
            read_timeout_setting(Some(vectors[1].as_f64().unwrap())),
            60.0
        );
        assert_eq!(max_servers(Some(vectors[2].as_i64().unwrap())), 3);
        assert_eq!(timeout_setting(None), DEFAULT_TIMEOUT_S);
        assert_eq!(read_timeout_setting(None), DEFAULT_READ_TIMEOUT_S);
        assert_eq!(max_servers(None), DEFAULT_MAX_SERVERS as usize);
    }

    #[test]
    fn slug_vectors_match_python() {
        // Lossy slugification: all three reduce to the same base.
        assert_eq!(tool_prefix_for_name("My Tools", "x"), "mcp_my_tools");
        assert_eq!(tool_prefix_for_name("my-tools", "x"), "mcp_my_tools");
        assert_eq!(tool_prefix_for_name("my_tools", "x"), "mcp_my_tools");
        // Id fallback when the name has no alphanumeric content.
        assert_eq!(
            tool_prefix_for_name("!!!", "12345678-aaaa-bbbb-cccc-dddddddddddd"),
            "mcp_12345678"
        );
        // Surrounding punctuation strips like Python's strip("_").
        assert_eq!(tool_prefix_for_name("  --Hi!!--  ", "x"), "mcp_hi");
        // Non-ASCII lowers then separates; runs collapse (CPython: mcp_h_llo_w_rld).
        assert_eq!(
            tool_prefix_for_name("  Héllo Wörld  ", "x"),
            "mcp_h_llo_w_rld"
        );
        assert_eq!(tool_prefix_for_name("a__b", "x"), "mcp_a_b");
        assert_eq!(
            tool_prefix_for_name("UPPER lower 123", "x"),
            "mcp_upper_lower_123"
        );
        // Fixture slug vectors replay.
        let fx = fixture();
        let slugs = fx["unique_prefixes"]["slug_vectors"].as_array().unwrap();
        assert_eq!(slugs[0].as_str().unwrap(), "mcp_my_tools");
        assert_eq!(slugs[1].as_str().unwrap(), "mcp_my_tools");
        assert_eq!(slugs[2].as_str().unwrap(), "mcp_12345678");
    }

    #[test]
    fn unique_prefixes_dedup_in_created_at_order() {
        let fx = fixture();
        // Fixture vectors: two colliding bases then a distinct one.
        let servers = vec![
            (
                "00000000-0000-0000-0000-000000000001".to_owned(),
                "mcp_my_tools".to_owned(),
            ),
            (
                "00000000-0000-0000-0000-000000000002".to_owned(),
                "mcp_my_tools".to_owned(),
            ),
            (
                "00000000-0000-0000-0000-000000000003".to_owned(),
                "mcp_other".to_owned(),
            ),
        ];
        let assigned = unique_prefixes(&servers);
        let vectors = &fx["unique_prefixes"]["vectors"];
        for (pk, prefix) in &assigned {
            assert_eq!(prefix, vectors[pk].as_str().unwrap());
        }
        // A third collision continues the counter.
        let servers = vec![
            ("a".to_owned(), "mcp_x".to_owned()),
            ("b".to_owned(), "mcp_x".to_owned()),
            ("c".to_owned(), "mcp_x".to_owned()),
        ];
        let assigned = unique_prefixes(&servers);
        assert_eq!(assigned["a"], "mcp_x");
        assert_eq!(assigned["b"], "mcp_x_2");
        assert_eq!(assigned["c"], "mcp_x_3");
    }

    fn row(pk: &str, name: &str, blocked: bool, auth: bool) -> ServerRow {
        ServerRow {
            pk: pk.to_owned(),
            name: name.to_owned(),
            url: "https://tools.example.com/mcp".to_owned(),
            has_auth_header: auth,
            url_blocked: blocked,
        }
    }

    #[test]
    fn plan_cap_overflow_reports_stable_order() {
        let rows = vec![
            row("pk1", "Alpha", false, false),
            row("pk2", "Beta", false, false),
            row("pk3", "Gamma", false, false),
            row("pk4", "Delta", false, false),
        ];
        let (plans, skipped) = plan_servers(&rows, 3);
        assert_eq!(plans.len(), 3);
        assert_eq!(
            skipped,
            vec![SkippedServer::new("Delta", REASON_TOO_MANY_SERVERS)]
        );
        assert_eq!(plans[0].prefix, "mcp_alpha");
    }

    #[test]
    fn plan_ssrf_blocked_skips() {
        let rows = vec![
            row("pk1", "Alpha", false, false),
            row("pk2", "Evil", true, false),
        ];
        let (plans, skipped) = plan_servers(&rows, 10);
        assert_eq!(plans.len(), 1);
        assert_eq!(
            skipped,
            vec![SkippedServer::new("Evil", REASON_URL_BLOCKED)]
        );
    }

    #[test]
    fn auth_header_absent_skips_decrypt() {
        let out = auth_header_for(false, || panic!("must not decrypt"));
        assert_eq!(out, Ok(None));
    }

    #[test]
    fn auth_header_decrypt_failures_map_to_reasons() {
        let ok = auth_header_for(true, || Ok("TestScheme example-token".to_owned()));
        assert_eq!(ok, Ok(Some("TestScheme example-token".to_owned())));
        let assistant = auth_header_for(true, || {
            Err(DecryptFailure::Assistant(
                AssistantError::AssistantNotConfigured("no backend".to_owned()),
            ))
        });
        assert_eq!(assistant, Err("assistant_not_configured".to_owned()));
        let opaque = auth_header_for(true, || Err(DecryptFailure::Opaque));
        assert_eq!(opaque, Err(REASON_AUTH_HEADER_UNREADABLE.to_owned()));
    }

    #[test]
    fn assemble_never_raises_per_server() {
        let rows = vec![
            row("pk1", "Good", false, false),
            row("pk2", "BadAuth", false, true),
            row("pk3", "BadBuild", false, false),
        ];
        let (plans, mut skipped) = plan_servers(&rows, 10);
        assert_eq!(plans.len(), 3);
        let (built, more_skipped) = assemble_toolsets(
            &plans,
            |plan| {
                if plan.name == "BadAuth" {
                    return Err("auth_header_unreadable".to_owned());
                }
                Ok(None)
            },
            |plan, _auth| {
                if plan.name == "BadBuild" {
                    return Err(());
                }
                Ok(plan.name.clone())
            },
        );
        assert_eq!(built, vec!["Good".to_owned()]);
        skipped.extend(more_skipped);
        assert_eq!(
            skipped,
            vec![
                SkippedServer::new("BadAuth", "auth_header_unreadable"),
                SkippedServer::new("BadBuild", REASON_TOOLSET_UNAVAILABLE),
            ]
        );
    }

    #[test]
    fn toolset_spec_shape_matches_constructor() {
        let fx = fixture();
        assert!(fx["build_toolset"]["shape"]
            .as_str()
            .unwrap()
            .contains("ResilientToolset"));
        let spec = build_toolset_spec(
            "https://tools.example.com/mcp",
            Some("TestScheme example-token"),
            Some("mcp_alpha"),
            false,
            DEFAULT_TIMEOUT_S,
            DEFAULT_READ_TIMEOUT_S,
            "Alpha",
        );
        assert_eq!(
            spec.auth_header,
            Some("TestScheme example-token".to_owned())
        );
        assert_eq!(spec.server_name, "Alpha");
        assert_eq!(spec.prefix, "mcp_alpha");
        // Empty header reads as absent (Python truthiness); empty names
        // fall back to the URL and the empty prefix.
        let bare = build_toolset_spec(
            "https://tools.example.com/mcp",
            Some(""),
            None,
            false,
            5.0,
            60.0,
            "",
        );
        assert_eq!(bare.auth_header, None);
        assert_eq!(bare.server_name, "https://tools.example.com/mcp");
        assert_eq!(bare.prefix, "");
    }

    #[test]
    fn resilient_never_entered_close_is_noop() {
        let fx = fixture();
        assert!(fx["resilient"]["enter_never_entered_close"]
            .as_str()
            .unwrap()
            .contains("no-op"));
        let mut state = ResilientState::new("Alpha", "mcp_alpha");
        state.connect_failed("TimeoutError");
        assert_eq!(state.failure.as_deref(), Some("TimeoutError"));
        assert!(!state.should_close());
        assert!(!state.should_list_tools());
    }

    #[test]
    fn resilient_close_failure_records_without_replacing_outcome() {
        let mut state = ResilientState::new("Alpha", "mcp_alpha");
        state.connect_succeeded();
        assert!(state.should_close());
        state.close_failed("ConnectionResetError");
        assert_eq!(state.failure.as_deref(), Some("ConnectionResetError"));
    }

    #[test]
    fn resilient_list_failure_reads_as_empty() {
        let mut state = ResilientState::new("Alpha", "mcp_alpha");
        state.connect_succeeded();
        assert!(state.should_list_tools());
        state.list_failed("MCPError");
        assert!(!state.should_list_tools());
        assert_eq!(state.failure.as_deref(), Some("MCPError"));
    }

    #[test]
    fn resilient_call_absorbs_with_exact_message() {
        let fx = fixture();
        assert_eq!(
            fx["resilient"]["call_tool_absorbs"].as_str().unwrap(),
            "mid-run failure -> string result 'Tool server <name> was unavailable for this call (<Exc>).' (mcp.py:166-172)"
        );
        let mut state = ResilientState::new("Alpha", "mcp_alpha");
        state.connect_succeeded();
        let outcome = state.call_failed("TimeoutError");
        assert_eq!(
            outcome,
            CallOutcome::Absorbed(
                "Tool server 'Alpha' was unavailable for this call (TimeoutError).".to_owned()
            )
        );
        assert_eq!(state.failure.as_deref(), Some("TimeoutError"));
    }

    #[test]
    fn resilient_control_flow_passes_through() {
        let fx = fixture();
        let names = fx["resilient"]["control_flow_passthrough"]
            .as_array()
            .unwrap();
        assert_eq!(CONTROL_FLOW_EXCEPTIONS.len(), names.len());
        for name in names {
            assert!(is_control_flow(name.as_str().unwrap()));
        }
        let mut state = ResilientState::new("Alpha", "mcp_alpha");
        state.connect_succeeded();
        for exc in CONTROL_FLOW_EXCEPTIONS {
            assert_eq!(state.call_failed(exc), CallOutcome::Passthrough);
        }
        assert_eq!(state.failure, None);
        assert!(is_control_flow("ModelRetry"));
        assert!(!is_control_flow("TimeoutError"));
    }

    #[test]
    fn py_repr_matches_cpython_quoting() {
        assert_eq!(py_repr("Alpha"), "'Alpha'");
        // Verified against live CPython: repr("it's") uses double quotes.
        assert_eq!(py_repr("it's"), "\"it's\"");
        assert_eq!(py_repr("say \"hi\""), "'say \"hi\"'");
        assert_eq!(py_repr("a\nb"), "'a\\nb'");
        assert_eq!(py_repr("a\\b"), "'a\\\\b'");
        // Both quote kinds: single-quote form with the apostrophe escaped.
        assert_eq!(py_repr("both\"and'quotes"), "'both\"and\\'quotes'");
        // Printable non-ASCII passes through verbatim.
        assert_eq!(py_repr("café"), "'café'");
    }
}
