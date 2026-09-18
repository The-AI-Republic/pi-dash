//! Idle-RSS recycle test for the shared codex engine.
//!
//! The engine samples its process's RSS on a timer and, when the process is
//! **idle** (zero live threads) and over a threshold, recycles it: the current
//! process is force-killed to free its memory and the next `warm`/`run`
//! respawns a fresh one. These tests drive that deterministically with a stub
//! RSS sampler (reports a fixed, over-threshold value) and a short check
//! interval, so no real process has to grow past a gibibyte.
//!
//! Asserts the acceptance criterion: a documented threshold recycles an idle
//! engine — and, crucially, the recycle is confined to the idle case, so a live
//! thread is never killed out from under an active session.

use pidash::codex::app_server::AppServer;
use pidash::codex::bridge::{Bridge, RunPayload};
use pidash::codex::engine::{RecyclePolicy, SharedCodexEngine};
use std::sync::Arc;
use std::time::Duration;
use tokio::process::Command;
use uuid::Uuid;

/// A fake app-server that just stays alive until it is killed. It never needs to
/// answer a JSON-RPC frame, because the engine is recycled while idle (before
/// any warm/run drives it). `read` blocks on stdin so the process lingers.
const SCRIPT_IDLE: &str = r#"
    while read _; do :; done
    sleep 100
"#;

/// A fresh app-server the respawn factory brings up: honours the first
/// `warm_session` (initialize + thread/start) so we can prove the post-recycle
/// process is a *new* one that serves the next request. Ids restart at 1.
const SCRIPT_WARM: &str = r#"
    set -e
    read _                                  # initialize
    printf '%s\n' '{"jsonrpc":"2.0","id":1,"result":{"capabilities":{}}}'
    read _                                  # initialized (no response)
    read _                                  # thread/start (id 2)
    printf '%s\n' '{"jsonrpc":"2.0","id":2,"result":{"threadId":"th-new"}}'
    sleep 100
"#;

/// A fake app-server that warms one thread and then runs a turn, streaming a
/// delta but never completing it, so the turn stays in flight and the session
/// counts as a live thread. Ids restart at 1.
const SCRIPT_LIVE_TURN: &str = r#"
    set -e
    read _                                  # initialize
    printf '%s\n' '{"jsonrpc":"2.0","id":1,"result":{"capabilities":{}}}'
    read _                                  # initialized (no response)
    read _                                  # thread/start (id 2)
    printf '%s\n' '{"jsonrpc":"2.0","id":2,"result":{"threadId":"th-live"}}'
    read _                                  # turn/start (id 3, no response)
    printf '%s\n' '{"jsonrpc":"2.0","method":"item/agentMessage/delta","params":{"threadId":"th-live","text":"streaming"}}'
    sleep 100
"#;

fn payload(prompt: &str) -> RunPayload {
    RunPayload {
        run_id: Uuid::new_v4(),
        prompt: prompt.into(),
        model: None,
    }
}

async fn spawn_fake(script: &'static str) -> AppServer {
    let mut cmd = Command::new("sh");
    cmd.arg("-c").arg(script);
    AppServer::spawn_command(cmd).await.expect("spawn fake codex")
}

/// A recycle policy that fires quickly and always reports an over-threshold
/// RSS, so a recycle happens as soon as the engine is idle.
fn always_recycle_policy() -> RecyclePolicy {
    RecyclePolicy {
        threshold_bytes: 1,
        check_interval: Duration::from_millis(50),
        sampler: Arc::new(|_pid| Some(u64::MAX)),
    }
}

/// True once the pid is no longer a live process (recycle force-killed it).
fn pid_is_dead(pid: u32) -> bool {
    // `kill -0` succeeds only while the process exists and is signalable.
    // stderr is silenced so a dead-pid probe doesn't print "No such process".
    !std::process::Command::new("kill")
        .args(["-0", &pid.to_string()])
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// An idle engine over the RSS threshold is force-killed, and the next warm
/// brings up a *fresh* process — the memory was actually reclaimed.
#[tokio::test]
async fn idle_engine_over_threshold_is_recycled() {
    let bridge = Bridge::from_server(spawn_fake(SCRIPT_IDLE).await, None);
    let engine = SharedCodexEngine::from_bridge_with_factory_and_recycle(
        bridge,
        || async { Ok(Bridge::from_server(spawn_fake(SCRIPT_WARM).await, None)) },
        always_recycle_policy(),
    );
    let handle = engine.handle();

    // The engine is idle from the start (no warm/run), so the first few recycle
    // ticks should kill the original process.
    let original_pid = handle
        .process_handle()
        .pid
        .expect("fake app-server has a pid");

    // Wait for the recycle to force-kill the original process.
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    loop {
        if pid_is_dead(original_pid) {
            break;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "original engine pid {original_pid} was never recycled"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    // The next warm respawns a fresh process and serves the request on it.
    let thread_id = handle
        .warm_session("chat-1", std::path::Path::new("/tmp"))
        .await
        .expect("warm after recycle respawns the engine");
    assert_eq!(thread_id, "th-new");

    let new_pid = handle
        .process_handle()
        .pid
        .expect("respawned app-server has a pid");
    assert_ne!(
        new_pid, original_pid,
        "post-recycle work must run on a new process, not the killed one"
    );
    assert!(
        !pid_is_dead(new_pid),
        "the respawned engine process should be alive"
    );
}

/// The recycle is confined to the idle case: while a thread is live (a turn is
/// in flight) an over-threshold engine is left running, so an active session is
/// never killed out from under itself.
#[tokio::test]
async fn busy_engine_over_threshold_is_not_recycled() {
    let bridge = Bridge::from_server(spawn_fake(SCRIPT_LIVE_TURN).await, None);
    let engine = SharedCodexEngine::from_bridge_with_factory_and_recycle(
        bridge,
        // If the guard were wrong and a recycle fired, this factory would bring
        // up a process reporting a *different* thread id, which we'd detect.
        || async { Ok(Bridge::from_server(spawn_fake(SCRIPT_WARM).await, None)) },
        always_recycle_policy(),
    );
    let handle = engine.handle();

    // Start a turn and leave it in flight — the session is now a live thread.
    let session = handle
        .run_session("chat-1", &payload("hi"), std::path::Path::new("/tmp"))
        .await
        .expect("start a turn on the shared engine");
    assert_eq!(session.thread_id, "th-live");

    let busy_pid = handle
        .process_handle()
        .pid
        .expect("fake app-server has a pid");
    assert_eq!(handle.live_thread_count().await, 1);

    // Let several recycle intervals elapse. The idle guard must keep the process
    // alive the whole time despite the over-threshold RSS.
    tokio::time::sleep(Duration::from_millis(400)).await;

    assert!(
        !pid_is_dead(busy_pid),
        "a busy engine must not be recycled out from under a live thread"
    );
    assert_eq!(
        handle.process_handle().pid,
        Some(busy_pid),
        "the process must be unchanged while a thread is live"
    );
    assert_eq!(handle.live_thread_count().await, 1);
}
