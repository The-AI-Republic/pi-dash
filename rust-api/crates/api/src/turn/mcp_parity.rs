//! Fixture-driven parity suite: every case in
//! `fixtures/assistant/mcp-transport/*.json` runs the Rust driver against
//! the stub and asserts Python's outcomes — tools, call results, failure
//! classes, and the request wire.
//!
//! Wire comparison drops the standalone `GET` stream entries on Streamable
//! fixtures (the client deliberately never opens one; see the module docs)
//! and asserts everything else exactly: method, path, header subset, and
//! byte-identical bodies.

use std::time::Duration;

use serde_json::Value;

use super::mcp_client::{CallResult, McpContent, McpValue, ResilientSession};
use super::mcp_stub::{StubDsl, StubServer};

const AUTH_HEADER: &str = "Bearer probe-token";

fn fixture_dir() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/assistant/mcp-transport")
}

fn load_fixture(name: &str) -> Value {
    let path = fixture_dir().join(format!("{name}.json"));
    let text = std::fs::read_to_string(&path).expect("fixture reads");
    serde_json::from_str(&text).expect("fixture parses")
}

fn opt_str(value: &Value) -> Option<String> {
    value.as_str().map(str::to_string)
}

async fn closed_port_url(path: &str) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("binds");
    let port = listener.local_addr().expect("addr").port();
    drop(listener);
    format!("http://127.0.0.1:{port}{path}")
}

async fn wait_refused(port: u16) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    loop {
        match tokio::net::TcpStream::connect(format!("127.0.0.1:{port}")).await {
            Err(_) => return,
            Ok(stream) => drop(stream),
        }
        if tokio::time::Instant::now() > deadline {
            panic!("port {port} never refused");
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

fn stub_port(base: &str) -> u16 {
    base.rsplit(':')
        .next()
        .expect("port")
        .parse()
        .expect("port parses")
}

/// The recorded run: failures after each phase plus op outputs.
#[derive(Default)]
struct Run {
    after_enter: Option<String>,
    after_list: Option<String>,
    after_call: Option<String>,
    after_list2: Option<String>,
    after_call2: Option<String>,
    at_exit: Option<String>,
    tools: Vec<(String, Option<String>, Value)>,
    tools2: Vec<String>,
    call: Option<CallResult>,
    call2: Option<CallResult>,
    instructions: Option<String>,
    last_message: Option<String>,
}

async fn run_fixture(name: &str) {
    let fixture = load_fixture(name);
    let flow: Vec<String> = fixture["flow"]
        .as_array()
        .expect("flow")
        .iter()
        .map(|step| step.as_str().expect("step").to_string())
        .collect();
    if flow == ["plan"] {
        run_plan_case(&fixture);
        return;
    }
    let toolset = &fixture["toolset"];
    let stub_value = &fixture["stub"];
    let serve = stub_value
        .get("serve")
        .and_then(Value::as_bool)
        .unwrap_or(true);
    let url_path = toolset["url_path"].as_str().unwrap_or("/mcp");
    let stub = if serve {
        let dsl: StubDsl = serde_json::from_value(stub_value.clone()).expect("stub DSL parses");
        Some(StubServer::start(dsl).await)
    } else {
        None
    };
    let base = match &stub {
        Some(stub) => stub.base.clone(),
        None => closed_port_url("").await,
    };
    let url = format!("{base}{url_path}");
    let auth = toolset["auth"].as_bool().unwrap_or(false);
    let spec = pidash_services::assistant::mcp::build_toolset_spec(
        &url,
        auth.then_some(AUTH_HEADER),
        toolset["prefix"].as_str(),
        toolset["include_instructions"].as_bool().unwrap_or(false),
        toolset["timeout"].as_f64().expect("timeout"),
        toolset["read_timeout"].as_f64().expect("read_timeout"),
        toolset["server_name"].as_str().unwrap_or("probe"),
    );
    let mut session = ResilientSession::new(spec).expect("valid spec");
    let mut run = Run::default();
    let call_spec = &fixture["call"];
    let mut calls_done = 0;
    let mut lists_done = 0;
    for step in &flow {
        match step.as_str() {
            "enter" => {
                session.enter().await;
                run.after_enter = session.failure().map(str::to_string);
                run.instructions = session.instructions().map(str::to_string);
                run.last_message = session.last_message().map(str::to_string);
            }
            "list" => {
                let defs = session.tools().await;
                lists_done += 1;
                if lists_done == 1 {
                    run.after_list = session.failure().map(str::to_string);
                    run.tools = defs
                        .into_iter()
                        .map(|def| (def.name, def.description, def.input_schema))
                        .collect();
                } else {
                    run.after_list2 = session.failure().map(str::to_string);
                    let mut names: Vec<String> = defs.into_iter().map(|def| def.name).collect();
                    names.sort();
                    run.tools2 = names;
                }
                if stub_value.get("kill_after").and_then(Value::as_str) == Some("tools/list")
                    && lists_done == 1
                {
                    let stub = stub.as_ref().expect("kill needs a stub");
                    stub.kill().await;
                    wait_refused(stub_port(&stub.base)).await;
                }
            }
            "call" => {
                let tool = call_spec["tool"].as_str().expect("call tool");
                let args = call_spec["args"].clone();
                let outcome = session.call(tool, &args).await;
                calls_done += 1;
                if calls_done == 1 {
                    run.after_call = session.failure().map(str::to_string);
                    run.call = Some(outcome);
                } else {
                    run.after_call2 = session.failure().map(str::to_string);
                    run.call2 = Some(outcome);
                }
            }
            other => panic!("unknown flow step: {other}"),
        }
    }
    session.close().await;
    run.at_exit = session.failure().map(str::to_string);
    if run.last_message.is_none() {
        run.last_message = session.last_message().map(str::to_string);
    }
    let requests = match stub {
        Some(stub) => {
            let requests = stub.requests().await;
            stub.shutdown().await;
            requests
        }
        None => Vec::new(),
    };
    assert_fixture(name, &fixture, &run, &requests);
}

/// The SSRF plan case: a blocked URL never builds, let alone connects.
fn run_plan_case(fixture: &Value) {
    use pidash_services::assistant::mcp::{plan_servers, ServerRow};
    use pidash_services::assistant::ssrf::{is_blocked, HostResolver, ResolveError};

    struct Loopback;
    impl HostResolver for Loopback {
        fn resolve(&self, _host: &str) -> Result<Vec<String>, ResolveError> {
            Ok(vec!["127.0.0.1".to_string()])
        }
    }
    let toolset = &fixture["toolset"];
    let url = toolset["url"].as_str().expect("url");
    let blocked = is_blocked(
        url,
        toolset["block_private_urls"].as_bool().unwrap_or(false),
        &Loopback,
    );
    assert!(blocked, "loopback URL is blocked");
    let (plans, skipped) = plan_servers(
        &[ServerRow {
            pk: "1".to_string(),
            name: toolset["server_name"]
                .as_str()
                .unwrap_or("probe")
                .to_string(),
            url: url.to_string(),
            has_auth_header: false,
            url_blocked: blocked,
        }],
        20,
    );
    assert!(plans.is_empty(), "blocked row never plans");
    let expect = &fixture["expect"]["skipped"];
    assert_eq!(skipped.len(), expect.as_array().expect("skipped").len());
    for (got, want) in skipped.iter().zip(expect.as_array().expect("skipped")) {
        assert_eq!(got.name, want["name"].as_str().expect("name"));
        assert_eq!(got.reason, want["reason"].as_str().expect("reason"));
    }
    // No stub exists for this case: zero requests by construction.
    assert_eq!(fixture["expect"]["requests"].as_u64(), Some(0));
}

fn assert_fixture(
    name: &str,
    fixture: &Value,
    run: &Run,
    requests: &[super::mcp_stub::LoggedRequest],
) {
    let mut expect = fixture["expect"].clone();
    // init_instructions compares tools and wire against the happy case.
    if expect["tools"].is_string() {
        let happy = load_fixture("streamable_json_happy");
        expect["tools"] = happy["expect"]["tools"].clone();
    }
    // Tools: names plus descriptions and schemas.
    let want_tools = expect["tools"].as_object().expect("tools object");
    assert_eq!(run.tools.len(), want_tools.len(), "{name}: tool count");
    for (got_name, got_desc, got_schema) in &run.tools {
        let want = want_tools
            .get(got_name)
            .unwrap_or_else(|| panic!("{name}: unexpected tool {got_name}"));
        assert_eq!(
            got_desc.as_deref(),
            want["description"].as_str(),
            "{name}: description of {got_name}"
        );
        assert_eq!(got_schema, &want["schema"], "{name}: schema of {got_name}");
    }
    // Calls.
    if !expect["call"].is_null() {
        assert_call(
            name,
            "call",
            &expect["call"],
            run.call.as_ref().expect("call ran"),
        );
    }
    if let Some(call2) = expect.get("call2") {
        assert_call2(name, call2, run.call2.as_ref().expect("second call ran"));
    }
    if let Some(tools2) = expect.get("tools2") {
        let mut want: Vec<String> = tools2
            .as_array()
            .expect("tools2")
            .iter()
            .map(|tool| tool.as_str().expect("tool").to_string())
            .collect();
        want.sort();
        assert_eq!(&run.tools2, &want, "{name}: second listing");
    }
    // Failures after each phase.
    let failures = &expect["failures"];
    assert_eq!(
        run.after_enter,
        opt_str(&failures["after_enter"]),
        "{name}: after_enter"
    );
    assert_eq!(
        run.after_list,
        opt_str(&failures["after_list"]),
        "{name}: after_list"
    );
    assert_eq!(
        run.after_call,
        opt_str(&failures["after_call"]),
        "{name}: after_call"
    );
    assert_eq!(
        run.at_exit,
        opt_str(&failures["at_exit"]),
        "{name}: at_exit"
    );
    if failures.get("after_list2").is_some() {
        assert_eq!(
            run.after_list2,
            opt_str(&failures["after_list2"]),
            "{name}: after_list2"
        );
    }
    if failures.get("after_call2").is_some() {
        assert_eq!(
            run.after_call2,
            opt_str(&failures["after_call2"]),
            "{name}: after_call2"
        );
    }
    // Instructions and close messages where the fixture pins them.
    if let Some(instructions) = expect.get("instructions").and_then(Value::as_str) {
        assert_eq!(
            run.instructions.as_deref(),
            Some(instructions),
            "{name}: instructions"
        );
    }
    if let Some(message) = expect.get("close_message").and_then(Value::as_str) {
        assert_eq!(
            run.last_message.as_deref(),
            Some(message),
            "{name}: close message"
        );
    }
    // Wire: exact method/path/body plus the header subset.
    let wire = if fixture.get("rust_wire").is_some() {
        &fixture["rust_wire"]
    } else if fixture["wire"].is_string() {
        &load_fixture("streamable_json_happy")["wire"]
    } else {
        &fixture["wire"]
    };
    let url_path = fixture["toolset"]["url_path"].as_str().unwrap_or("/mcp");
    let is_sse = url_path.starts_with("/sse");
    let want_entries: Vec<&Value> = wire
        .as_array()
        .expect("wire array")
        .iter()
        .filter(|entry| is_sse || entry["method"] != "GET")
        .collect();
    assert_eq!(requests.len(), want_entries.len(), "{name}: request count");
    for (index, (got, want)) in requests.iter().zip(want_entries).enumerate() {
        assert_eq!(
            got.method,
            want["method"].as_str().expect("method"),
            "{name}: req{index} method"
        );
        assert_eq!(
            got.path,
            want["path"].as_str().expect("path"),
            "{name}: req{index} path"
        );
        assert_eq!(
            got.body,
            want["body"].as_str().unwrap_or_default(),
            "{name}: req{index} body"
        );
        let want_headers = want["headers"].as_object().expect("headers");
        for (key, value) in want_headers {
            assert_eq!(
                got.headers.get(key).map(String::as_str),
                value.as_str(),
                "{name}: req{index} header {key}"
            );
        }
    }
    let delete_seen = requests.iter().any(|request| request.method == "DELETE");
    assert_eq!(
        delete_seen,
        expect["delete_seen"].as_bool().unwrap_or(false),
        "{name}: delete_seen"
    );
}

fn assert_call(name: &str, which: &str, expect: &Value, got: &CallResult) {
    if let Some(absorbed) = expect.get("absorbed") {
        match got {
            CallResult::Absorbed { text } => {
                assert_eq!(
                    text,
                    absorbed["text"].as_str().expect("text"),
                    "{name}: {which} text"
                );
            }
            other => panic!("{name}: {which} expected absorbed, got {other:?}"),
        }
        return;
    }
    if let Some(exception) = expect.get("exception") {
        assert_eq!(
            exception["class"].as_str(),
            Some("ModelRetry"),
            "{name}: {which} class"
        );
        match got {
            CallResult::Retry { message } => {
                assert_eq!(
                    message,
                    exception["message"].as_str().expect("message"),
                    "{name}: {which} message"
                );
            }
            other => panic!("{name}: {which} expected a retry, got {other:?}"),
        }
        return;
    }
    let want = &expect["result"];
    match got {
        CallResult::Value(value) => assert_value(name, which, want, value),
        other => panic!("{name}: {which} expected a value, got {other:?}"),
    }
}

fn assert_value(name: &str, which: &str, want: &Value, got: &McpValue) {
    match want["kind"].as_str().expect("kind") {
        "text" => match got {
            McpValue::Single(McpContent::Text(text)) => {
                assert_eq!(
                    text,
                    want["value"].as_str().expect("text"),
                    "{name}: {which}"
                );
            }
            other => panic!("{name}: {which} expected text, got {other:?}"),
        },
        "json" => match got {
            McpValue::Single(McpContent::Json(value)) => {
                assert_eq!(value, &want["value"], "{name}: {which}");
            }
            other => panic!("{name}: {which} expected json, got {other:?}"),
        },
        "structured" => match got {
            McpValue::Structured(value) => {
                assert_eq!(value, &want["value"], "{name}: {which}");
            }
            other => panic!("{name}: {which} expected structured, got {other:?}"),
        },
        "image" => match got {
            McpValue::Single(McpContent::Image { media_type, data }) => {
                assert_eq!(
                    media_type,
                    want["media_type"].as_str().expect("media type"),
                    "{name}: {which}"
                );
                assert_eq!(
                    data,
                    &base64_decode(want["data_b64"].as_str().expect("data")),
                    "{name}: {which}"
                );
            }
            other => panic!("{name}: {which} expected image, got {other:?}"),
        },
        "binary" => match got {
            McpValue::Single(McpContent::Binary { media_type, data }) => {
                assert_eq!(
                    media_type,
                    want["media_type"].as_str().expect("media type"),
                    "{name}: {which}"
                );
                assert_eq!(
                    data,
                    &base64_decode(want["data_b64"].as_str().expect("data")),
                    "{name}: {which}"
                );
            }
            other => panic!("{name}: {which} expected binary, got {other:?}"),
        },
        "list" => match got {
            McpValue::List(items) => {
                let want_items = want["items"].as_array().expect("items");
                assert_eq!(items.len(), want_items.len(), "{name}: {which} length");
                for (index, (got, want)) in items.iter().zip(want_items).enumerate() {
                    assert_content(name, &format!("{which}[{index}]"), want, got);
                }
            }
            other => panic!("{name}: {which} expected list, got {other:?}"),
        },
        kind => panic!("{name}: {which} unknown kind {kind}"),
    }
}

fn assert_content(name: &str, which: &str, want: &Value, got: &McpContent) {
    match want["kind"].as_str().expect("kind") {
        "text" => match got {
            McpContent::Text(text) => {
                assert_eq!(
                    text,
                    want["value"].as_str().expect("text"),
                    "{name}: {which}"
                );
            }
            other => panic!("{name}: {which} expected text, got {other:?}"),
        },
        "json" => match got {
            McpContent::Json(value) => assert_eq!(value, &want["value"], "{name}: {which}"),
            other => panic!("{name}: {which} expected json, got {other:?}"),
        },
        "image" => match got {
            McpContent::Image { media_type, data } => {
                assert_eq!(
                    media_type,
                    want["media_type"].as_str().expect("media type"),
                    "{name}: {which}"
                );
                assert_eq!(
                    data,
                    &base64_decode(want["data_b64"].as_str().expect("data")),
                    "{name}: {which}"
                );
            }
            other => panic!("{name}: {which} expected image, got {other:?}"),
        },
        "binary" => match got {
            McpContent::Binary { media_type, data } => {
                assert_eq!(
                    media_type,
                    want["media_type"].as_str().expect("media type"),
                    "{name}: {which}"
                );
                assert_eq!(
                    data,
                    &base64_decode(want["data_b64"].as_str().expect("data")),
                    "{name}: {which}"
                );
            }
            other => panic!("{name}: {which} expected binary, got {other:?}"),
        },
        kind => panic!("{name}: {which} unknown kind {kind}"),
    }
}

/// The wedge second call: absorbed text, checked against the probe's repr.
fn assert_call2(name: &str, expect: &Value, got: &CallResult) {
    if let Some(exception) = expect.get("exception") {
        assert_call(
            name,
            "call2",
            &Value::Object({
                let mut map = serde_json::Map::new();
                map.insert("exception".to_string(), exception.clone());
                map
            }),
            got,
        );
        return;
    }
    let repr = expect["result_text"].as_str().expect("result repr");
    // Python repr of these plain strings is double-quoted JSON text.
    let want: String = serde_json::from_str(repr).expect("repr parses");
    match got {
        CallResult::Absorbed { text } => assert_eq!(text, &want, "{name}: call2"),
        other => panic!("{name}: call2 expected absorbed, got {other:?}"),
    }
}

fn base64_decode(data: &str) -> Vec<u8> {
    use base64::Engine as _;
    base64::engine::general_purpose::STANDARD
        .decode(data)
        .expect("fixture b64")
}

macro_rules! parity {
    ($name:ident) => {
        #[tokio::test]
        async fn $name() {
            let case = stringify!($name);
            tokio::time::timeout(Duration::from_secs(60), run_fixture(case))
                .await
                .unwrap_or_else(|_| panic!("{case}: timed out"));
        }
    };
}

parity!(streamable_json_happy);
parity!(streamable_no_auth);
parity!(streamable_sse_framing);
parity!(legacy_sse);
parity!(legacy_sse_no_auth);
parity!(legacy_sse_unreachable);
parity!(legacy_sse_no_endpoint);
parity!(legacy_sse_post_500);
parity!(unreachable);
parity!(init_timeout);
parity!(init_error);
parity!(init_version_downgrade);
parity!(init_version_unsupported);
parity!(init_instructions);
parity!(init_returns_202);
parity!(list_error);
parity!(list_paginated);
parity!(list_dup_cursor);
parity!(list_timeout);
parity!(list_500_then_call);
parity!(list_250_cap);
parity!(list_returns_202);
parity!(list_bare_303);
parity!(http_404);
parity!(http_500);
parity!(redirect_307);
parity!(redirect_cross_origin);
parity!(notif_500);
parity!(notif_404);
parity!(notif_500_then_call);
parity!(wedge_ll);
parity!(wedge_cc);
parity!(wedge_cl);
parity!(die_on_delete);
parity!(delete_405);
parity!(call_timeout);
parity!(call_http_500);
parity!(mid_run_death);
parity!(kill_before_call);
parity!(bad_content_type);
parity!(bad_then_call);
parity!(invalid_json);
parity!(post_sse_empty);
parity!(unknown_content);
parity!(call_is_error);
parity!(call_is_error_multi);
parity!(call_is_error_structured);
parity!(call_is_error_image);
parity!(call_structured);
parity!(call_structured_empty);
parity!(call_result_unwrap);
parity!(call_json_text);
parity!(call_multi_text);
parity!(call_image);
parity!(call_audio);
parity!(call_embedded_text);
parity!(call_embedded_blob_png);
parity!(call_embedded_blob_bin);
parity!(call_resource_link);
parity!(call_schema_ok);
parity!(call_schema_missing);
parity!(call_schema_bad);
parity!(sse_silent);
parity!(sse_endpoint_cross_origin);
parity!(sse_redirect_cross_origin);
parity!(ssrf_blocked);
