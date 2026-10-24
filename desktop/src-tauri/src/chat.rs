// Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Desktop → managed-daemon **local chat** transport (PDASHOSS01-159, slice 2).
//!
//! This is the desktop-host half of the direct local-chat feature: the webview
//! talks to the bundled engine through the Tauri host and the daemon's IPC
//! socket, and never through the Pi Dash cloud chat relay. It builds directly on
//! the slice-1a wire protocol (`pidash_ipc::protocol::Chat*`) and the slice-1b
//! daemon handler (`runner/src/ipc/chat.rs`), which drives the built-in engine
//! and streams `Response::Chat*` frames back on the same connection.
//!
//! Two directions, matching the transport contract the `desktop-overlay/` seam
//! (slice 4, [`ChatTransport`]) expects:
//!
//! * **UI → engine** — [`chat_warm`], [`chat_send`], [`chat_cancel`],
//!   [`chat_close`], [`chat_decide`] are Tauri `invoke` commands. `warm`/`send`
//!   open a streaming connection; `cancel`/`close`/`decide` are single
//!   round-trips on a *second* connection (the streaming one is busy pumping the
//!   turn — this is exactly why slice 1b answers approvals out-of-band).
//! * **engine → UI** — each streamed `Response::Chat*` frame is re-emitted to the
//!   webview as a Tauri event named [`CHAT_FRAME_EVENT`]. Transport failures the
//!   webview can't otherwise see (daemon not running, connection dropped) surface
//!   as [`CHAT_ERROR_EVENT`]. Frames self-identify by `chat_session_id`, so the
//!   overlay routes them to the right session without a per-session channel.
//!
//! Scope note: this slice lands the *transport*. Local SQLite history
//! (slice 3, `chat_history`) lives on its own branch and is wired in where the
//! two integrate; the transport here neither reads nor writes history.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use pidash_ipc::protocol::{Request, Response};
use pidash_ipc::{ApprovalDecision, ApprovalMode, Client};
use serde::Serialize;
use tauri::async_runtime::JoinHandle;
use tauri::{AppHandle, Emitter, Manager, Runtime};
use uuid::Uuid;

use crate::ipc::socket_path;
use crate::managed_runner::ManagedPaths;

/// Tauri event carrying one streamed `Response::Chat*` frame to the webview.
/// The payload is the frame serialized with its usual `result`/`data` tagging.
pub const CHAT_FRAME_EVENT: &str = "chat://frame";

/// Tauri event carrying a transport-level failure (daemon unreachable, dropped
/// connection) that never produced an engine `ChatFailed` frame. Keyed by
/// `chat_session_id` so the overlay can fail exactly that session.
pub const CHAT_ERROR_EVENT: &str = "chat://error";

/// Payload for [`CHAT_ERROR_EVENT`].
#[derive(Debug, Clone, Serialize)]
pub struct ChatError {
    pub chat_session_id: Uuid,
    pub message: String,
}

/// Active per-session streaming tasks, so [`chat_close`] can abort the reader
/// once the daemon has torn the session down. Managed as Tauri state.
#[derive(Default)]
pub struct ChatState {
    streams: Arc<Mutex<StreamRegistry>>,
}

/// What a session's reader is pumping. A turn's reader carries the reply, so it
/// is never replaced; a warm's reader carries nothing the next send won't
/// produce again.
#[derive(Clone, Copy, PartialEq, Eq)]
enum StreamKind {
    Warm,
    Turn,
}

struct ActiveStream {
    handle: JoinHandle<()>,
    kind: StreamKind,
    /// Distinguishes this reader from a later one registered under the same
    /// session, so a finishing reader only ever deregisters itself.
    generation: u64,
}

/// The live reader of each session. A reader deregisters itself when its
/// stream ends, so an entry means the stream is still being pumped.
#[derive(Default)]
struct StreamRegistry {
    active: HashMap<Uuid, ActiveStream>,
    next_generation: u64,
}

/// Returned by [`chat_send`] when the session's previous turn is still
/// streaming. The daemon would refuse the turn as `runner_busy` anyway; what
/// matters here is that the in-flight turn's reader is left alone.
const TURN_IN_PROGRESS: &str =
    "a response is still in progress for this chat; wait for it to finish or stop it first";

/// Where streamed frames go. The production sink emits Tauri events; tests use
/// an in-memory collector so the streaming/translation loop is exercised
/// without a live `App`.
pub(crate) trait ChatSink: Send + Sync + 'static {
    fn frame(&self, frame: &Response);
    fn error(&self, chat_session_id: Uuid, message: &str);
}

struct TauriSink<R: Runtime> {
    app: AppHandle<R>,
}

impl<R: Runtime> ChatSink for TauriSink<R> {
    fn frame(&self, frame: &Response) {
        // A closed webview is not our problem to escalate — drop the frame.
        let _ = self.app.emit(CHAT_FRAME_EVENT, frame);
    }

    fn error(&self, chat_session_id: Uuid, message: &str) {
        let _ = self.app.emit(
            CHAT_ERROR_EVENT,
            ChatError {
                chat_session_id,
                message: message.to_string(),
            },
        );
    }
}

/// Consume the streamed response frames of a `ChatWarm`/`ChatSend` connection.
///
/// `first` is the first frame already read by the caller (so a synchronous
/// error — e.g. the daemon's `501` before slice 1b, or a connection refusal —
/// is surfaced to the `invoke` rather than only to the event channel). Every
/// `Chat*` frame is handed to `sink`; the stream ends on a terminator, and a
/// `Response::Error` becomes an `Err` the caller reports as a transport error.
///
/// A stream terminates on one of:
/// * `Ack` — the daemon's terminator for a `ChatWarm` turn;
/// * `ChatMessageCompleted` / `ChatFailed` / `ChatClosed` — the terminal frame
///   of a `ChatSend` / `ChatClose`. The daemon returns this frame and then keeps
///   the connection open for the next request without a trailing `Ack`, so the
///   terminal frame *is* the end of the turn. Without stopping here the reader
///   would block on `read_next` forever, leaking the task and connection every
///   turn.
///
/// EOF *before* any terminator means the daemon dropped the connection
/// mid-turn (a crash): that is surfaced as an `Err` so the UI fails the session
/// rather than hanging on a spinner.
async fn drive_stream<S: ChatSink>(
    mut client: Client,
    first: Response,
    sink: &S,
) -> Result<(), String> {
    let mut frame = first;
    loop {
        match frame {
            // The daemon's terminator after a warm turn's frames.
            Response::Ack => return Ok(()),
            Response::Error(err) => {
                return Err(format!("daemon error {}: {}", err.code, err.message));
            }
            // Terminal turn frames: forward, then end the stream — the daemon
            // sends no trailing `Ack` and holds the connection open, so this is
            // the turn's true end.
            terminal @ (Response::ChatMessageCompleted { .. }
            | Response::ChatFailed { .. }
            | Response::ChatClosed { .. }) => {
                sink.frame(&terminal);
                return Ok(());
            }
            other => sink.frame(&other),
        }
        match client.read_next().await.map_err(|e| e.to_string())? {
            Some(next) => frame = next,
            None => {
                return Err(
                    "managed daemon closed the chat stream before it completed".to_string(),
                );
            }
        }
    }
}

/// Open a streaming chat connection: connect, send `req`, then pump every frame
/// to `sink` until the terminal `Ack`/EOF. Split from the Tauri command so it
/// can be driven against a stub socket in tests.
pub(crate) async fn stream_session<S: ChatSink>(
    socket: &Path,
    req: Request,
    sink: &S,
) -> Result<(), String> {
    let mut client = Client::connect(socket)
        .await
        .map_err(|e| format!("connecting to managed daemon: {e}"))?;
    // `call` sends the request and reads the first response line; for a
    // streaming request that first line is the first `Chat*` frame (or a
    // synchronous `Error`), and the rest arrive via `read_next`.
    let first = client
        .call(req)
        .await
        .map_err(|e| format!("chat request failed: {e}"))?;
    drive_stream(client, first, sink).await
}

/// Single round-trip request on a fresh connection (cancel / close / decide).
/// The streaming connection is busy pumping the turn, so these ride a second
/// one, exactly as the slice-1b handler expects.
async fn call_once(socket: &Path, req: Request) -> Result<(), String> {
    let mut client = Client::connect(socket)
        .await
        .map_err(|e| format!("connecting to managed daemon: {e}"))?;
    match client
        .call(req)
        .await
        .map_err(|e| format!("chat request failed: {e}"))?
    {
        Response::Ack => Ok(()),
        // `handle_close` emits `ChatClosed` on the connection *before* the
        // dispatch layer writes the terminal `Ack`; `call` reads only that first
        // frame, so `ChatClosed` is the success signal for a close round-trip.
        Response::ChatClosed { .. } => Ok(()),
        Response::Error(err) => Err(format!("daemon error {}: {}", err.code, err.message)),
        other => Err(format!("unexpected response: {other:?}")),
    }
}

/// Resolve the managed daemon's control socket for this app.
/// The managed daemon's control socket **for this workspace**.
///
/// The daemon is started per workspace (`managed_start_daemon`) with its data
/// dir re-rooted under `managed/pidash/<workspace>/data`, and the runner binds
/// its socket at `<data_dir>/runtime/pidash.sock`. Resolving from the shared
/// `managed/runtime/` instead — which only holds the model token — is a socket
/// nothing listens on, and every chat send failed with "connecting to managed
/// daemon" even though the daemon was up.
fn resolve_socket<R: Runtime>(app: &AppHandle<R>, workspace: &str) -> Result<PathBuf, String> {
    let paths = ManagedPaths::for_workspace(app, workspace)?;
    Ok(socket_path(&paths.daemon_runtime_dir()))
}

/// Spawn the background reader for a streaming request and register its handle
/// so [`chat_close`] can abort it. On any transport error the synthetic
/// [`CHAT_ERROR_EVENT`] tells the webview which session failed.
fn spawn_stream<R: Runtime>(
    app: AppHandle<R>,
    socket: PathBuf,
    chat_session_id: Uuid,
    kind: StreamKind,
    req: Request,
) -> Result<(), String> {
    let streams = app.state::<ChatState>().streams.clone();
    let sink = Arc::new(TauriSink { app });
    start_stream(&streams, sink, socket, chat_session_id, kind, req)
}

/// The registry half of [`spawn_stream`], split from the Tauri handle so it can
/// be driven with an in-memory sink in tests.
///
/// A session whose turn is still streaming keeps its reader: aborting it drops
/// the rest of the turn's frames — the reply and its `ChatMessageCompleted` —
/// without any error reaching the webview, and the dropped connection makes
/// the daemon tear the turn down. So a second send is refused, and a warm is
/// skipped (the engine is evidently up).
fn start_stream<S: ChatSink>(
    streams: &Arc<Mutex<StreamRegistry>>,
    sink: Arc<S>,
    socket: PathBuf,
    chat_session_id: Uuid,
    kind: StreamKind,
    req: Request,
) -> Result<(), String> {
    // Held until the new reader is registered, so a reader that finishes
    // straight away cannot deregister before its own entry exists.
    let mut registry = streams.lock().unwrap();
    if registry
        .active
        .get(&chat_session_id)
        .is_some_and(|stream| stream.kind == StreamKind::Turn)
    {
        return match kind {
            StreamKind::Turn => Err(TURN_IN_PROGRESS.to_string()),
            StreamKind::Warm => Ok(()),
        };
    }
    registry.next_generation += 1;
    let generation = registry.next_generation;
    let task_streams = streams.clone();
    let handle = tauri::async_runtime::spawn(async move {
        if let Err(e) = stream_session(&socket, req, sink.as_ref()).await {
            sink.error(chat_session_id, &e);
        }
        let mut registry = task_streams.lock().unwrap();
        if registry
            .active
            .get(&chat_session_id)
            .is_some_and(|stream| stream.generation == generation)
        {
            registry.active.remove(&chat_session_id);
        }
    });
    // Replacing a session's warm reader with a send's must abort the old task:
    // dropping a `JoinHandle` detaches it, leaving the previous connection's
    // reader alive and unreachable.
    let stream = ActiveStream {
        handle,
        kind,
        generation,
    };
    if let Some(previous) = registry.active.insert(chat_session_id, stream) {
        previous.handle.abort();
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Tauri commands
// ---------------------------------------------------------------------------

/// Warm a local chat session's engine without submitting a turn. Streams
/// `chat_warmed` / `chat_warm_failed` `ChatEvent` frames to the webview.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn chat_warm<R: Runtime>(
    app: AppHandle<R>,
    workspace: String,
    chat_session_id: Uuid,
    runner: Option<String>,
    cwd: Option<String>,
    model: Option<String>,
    mode: Option<ApprovalMode>,
    local_thread_id: Option<String>,
    local_session_id: Option<String>,
) -> Result<(), String> {
    let socket = resolve_socket(&app, &workspace)?;
    let req = Request::ChatWarm {
        chat_session_id,
        runner,
        cwd,
        model,
        mode,
        local_thread_id,
        local_session_id,
    };
    spawn_stream(app, socket, chat_session_id, StreamKind::Warm, req)
}

/// Submit a user turn. Streams the turn's `Chat*` frames to the webview until a
/// terminal `ChatMessageCompleted` / `ChatFailed`. Refused while the session's
/// previous turn is still streaming.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn chat_send<R: Runtime>(
    app: AppHandle<R>,
    workspace: String,
    chat_session_id: Uuid,
    message_id: Uuid,
    content: String,
    runner: Option<String>,
    cwd: Option<String>,
    model: Option<String>,
    mode: Option<ApprovalMode>,
    local_thread_id: Option<String>,
    local_session_id: Option<String>,
) -> Result<(), String> {
    let socket = resolve_socket(&app, &workspace)?;
    let req = Request::ChatSend {
        chat_session_id,
        message_id,
        content,
        runner,
        cwd,
        model,
        mode,
        local_thread_id,
        local_session_id,
    };
    spawn_stream(app, socket, chat_session_id, StreamKind::Turn, req)
}

/// Interrupt the in-flight turn of a local chat session.
#[tauri::command]
pub async fn chat_cancel<R: Runtime>(
    app: AppHandle<R>,
    workspace: String,
    chat_session_id: Uuid,
    runner: Option<String>,
    reason: Option<String>,
) -> Result<(), String> {
    let socket = resolve_socket(&app, &workspace)?;
    call_once(
        &socket,
        Request::ChatCancel {
            chat_session_id,
            runner,
            reason,
        },
    )
    .await
}

/// Close a local chat session, releasing its engine runtime and aborting the
/// desktop-side reader task for it.
#[tauri::command]
pub async fn chat_close<R: Runtime>(
    app: AppHandle<R>,
    workspace: String,
    chat_session_id: Uuid,
    runner: Option<String>,
    reason: Option<String>,
) -> Result<(), String> {
    let socket = resolve_socket(&app, &workspace)?;
    let result = call_once(
        &socket,
        Request::ChatClose {
            chat_session_id,
            runner,
            reason,
        },
    )
    .await;
    // The daemon closes the stream on `ChatClose`, which ends the reader on its
    // own; abort defensively in case the socket lingers, and drop the handle.
    if let Some(stream) = app
        .state::<ChatState>()
        .streams
        .lock()
        .unwrap()
        .active
        .remove(&chat_session_id)
    {
        stream.handle.abort();
    }
    result
}

/// Answer a pending chat approval, routed into the runner's shared approval
/// router as `DecisionSource::Local` by the slice-1b handler.
#[tauri::command]
pub async fn chat_decide<R: Runtime>(
    app: AppHandle<R>,
    workspace: String,
    chat_session_id: Uuid,
    local_approval_id: String,
    decision: ApprovalDecision,
    runner: Option<String>,
) -> Result<(), String> {
    let socket = resolve_socket(&app, &workspace)?;
    call_once(
        &socket,
        Request::ChatDecide {
            chat_session_id,
            local_approval_id,
            decision,
            runner,
        },
    )
    .await
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use pidash_ipc::protocol::RpcError;
    use std::sync::Mutex as StdMutex;
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    use tokio::net::UnixListener;

    /// In-memory sink recording frames and errors for assertions.
    #[derive(Default)]
    struct CollectSink {
        frames: StdMutex<Vec<Response>>,
        errors: StdMutex<Vec<(Uuid, String)>>,
    }

    impl ChatSink for CollectSink {
        fn frame(&self, frame: &Response) {
            self.frames.lock().unwrap().push(frame.clone());
        }
        fn error(&self, chat_session_id: Uuid, message: &str) {
            self.errors
                .lock()
                .unwrap()
                .push((chat_session_id, message.to_string()));
        }
    }

    fn write_frames(dir: &Path, frames: Vec<Response>) -> (PathBuf, tokio::task::JoinHandle<()>) {
        let socket = socket_path(dir);
        let listener = UnixListener::bind(&socket).unwrap();
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut reader = BufReader::new(stream);
            // Consume the one request line the client sends.
            let mut line = String::new();
            reader.read_line(&mut line).await.unwrap();
            for frame in frames {
                let mut bytes = serde_json::to_vec(&frame).unwrap();
                bytes.push(b'\n');
                reader.get_mut().write_all(&bytes).await.unwrap();
            }
            reader.get_mut().flush().await.unwrap();
        });
        (socket, server)
    }

    fn sample_send() -> Request {
        Request::ChatSend {
            chat_session_id: Uuid::nil(),
            message_id: Uuid::nil(),
            content: "hi".into(),
            runner: None,
            cwd: None,
            model: None,
            mode: None,
            local_thread_id: None,
            local_session_id: None,
        }
    }

    /// A `ChatSend` streams every `Chat*` frame to the sink, in order, and the
    /// terminal `ChatMessageCompleted` ends the stream — the daemon sends no
    /// trailing `Ack` for a send turn, so the completed frame is the terminator.
    #[tokio::test]
    async fn send_stream_stops_on_completed_frame() {
        let dir = tempfile::tempdir().unwrap();
        let id = Uuid::new_v4();
        let frames = vec![
            Response::ChatMessageStarted {
                chat_session_id: id,
                message_id: Uuid::nil(),
                turn_id: None,
                started_at: chrono_now(),
            },
            Response::ChatEvent {
                chat_session_id: id,
                bridge_seq: 1,
                kind: "assistant_delta".into(),
                payload: serde_json::json!({ "text": "hello" }),
            },
            Response::ChatMessageCompleted {
                chat_session_id: id,
                message_id: Uuid::nil(),
                turn_id: None,
                assistant_message: Some("hello".into()),
                status: "completed".into(),
                completed_at: chrono_now(),
            },
        ];
        let (socket, server) = write_frames(dir.path(), frames);

        let sink = CollectSink::default();
        stream_session(&socket, sample_send(), &sink).await.unwrap();
        server.await.unwrap();

        let got = sink.frames.lock().unwrap();
        assert_eq!(got.len(), 3, "all 3 chat frames forwarded incl. completed");
        assert!(matches!(got[0], Response::ChatMessageStarted { .. }));
        assert!(matches!(got[1], Response::ChatEvent { .. }));
        assert!(matches!(got[2], Response::ChatMessageCompleted { .. }));
        assert!(sink.errors.lock().unwrap().is_empty());
    }

    /// M2: after the terminal `ChatMessageCompleted` the daemon keeps the
    /// connection open (it loops for the next request) and sends no `Ack`. The
    /// reader must stop on the completed frame rather than blocking on
    /// `read_next` forever — otherwise the task and connection leak every turn.
    #[tokio::test]
    async fn send_stream_returns_on_completed_without_trailing_ack() {
        let dir = tempfile::tempdir().unwrap();
        let id = Uuid::new_v4();
        let socket = socket_path(dir.path());
        let listener = UnixListener::bind(&socket).unwrap();
        // Server writes the terminal frame, then holds the connection open
        // (never sends Ack, never closes) — exactly what the real daemon does.
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut reader = BufReader::new(stream);
            let mut line = String::new();
            reader.read_line(&mut line).await.unwrap();
            let frame = Response::ChatMessageCompleted {
                chat_session_id: id,
                message_id: Uuid::nil(),
                turn_id: None,
                assistant_message: Some("done".into()),
                status: "completed".into(),
                completed_at: chrono_now(),
            };
            let mut bytes = serde_json::to_vec(&frame).unwrap();
            bytes.push(b'\n');
            reader.get_mut().write_all(&bytes).await.unwrap();
            reader.get_mut().flush().await.unwrap();
            // Hold the connection open long enough that a blocking reader would
            // still be parked when the assertion runs.
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        });

        let sink = CollectSink::default();
        // Must return promptly on the completed frame, not time out.
        tokio::time::timeout(
            std::time::Duration::from_secs(1),
            stream_session(&socket, sample_send(), &sink),
        )
        .await
        .expect("stream must not block after the completed frame")
        .expect("stream ok");
        assert_eq!(sink.frames.lock().unwrap().len(), 1);
        assert!(sink.errors.lock().unwrap().is_empty());
        server.abort();
    }

    /// M4: EOF before any terminal frame means the daemon dropped the
    /// connection mid-turn (a crash). That must surface as an `Err` — which the
    /// command maps to a `chat://error` — not a silent `Ok` that hangs the UI.
    #[tokio::test]
    async fn premature_eof_is_a_transport_error() {
        let dir = tempfile::tempdir().unwrap();
        let id = Uuid::new_v4();
        // Only a non-terminal frame, then the server closes → EOF mid-turn.
        let frames = vec![Response::ChatMessageStarted {
            chat_session_id: id,
            message_id: Uuid::nil(),
            turn_id: None,
            started_at: chrono_now(),
        }];
        let (socket, server) = write_frames(dir.path(), frames);

        let sink = CollectSink::default();
        let err = stream_session(&socket, sample_send(), &sink)
            .await
            .unwrap_err();
        server.await.unwrap();
        assert!(
            err.contains("before it completed"),
            "unexpected error: {err}"
        );
        // The started frame was still forwarded before the drop.
        assert_eq!(sink.frames.lock().unwrap().len(), 1);
    }

    /// A daemon that predates the slice-1b handler answers `Chat*` with a `501`
    /// `Response::Error`; the transport turns that into an `Err` (which the
    /// command reports on the error channel) rather than a forwarded frame.
    #[tokio::test]
    async fn daemon_error_frame_becomes_transport_error() {
        let dir = tempfile::tempdir().unwrap();
        let frames = vec![Response::Error(RpcError {
            code: 501,
            message: "not implemented".into(),
        })];
        let (socket, server) = write_frames(dir.path(), frames);

        let sink = CollectSink::default();
        let err = stream_session(&socket, sample_send(), &sink)
            .await
            .unwrap_err();
        server.await.unwrap();

        assert!(err.contains("501"), "error carries the code: {err}");
        assert!(sink.frames.lock().unwrap().is_empty());
    }

    /// A missing socket (daemon not running) is an `Err`, not a panic — the
    /// command maps it to a `chat://error` event.
    #[tokio::test]
    async fn missing_socket_is_a_transport_error() {
        let dir = tempfile::tempdir().unwrap();
        let socket = socket_path(dir.path()); // never bound
        let sink = CollectSink::default();
        assert!(stream_session(&socket, sample_send(), &sink).await.is_err());
    }

    /// `call_once` (cancel / close / decide) expects a single `Ack`.
    #[tokio::test]
    async fn call_once_accepts_ack() {
        let dir = tempfile::tempdir().unwrap();
        let (socket, server) = write_frames(dir.path(), vec![Response::Ack]);
        call_once(
            &socket,
            Request::ChatCancel {
                chat_session_id: Uuid::nil(),
                runner: None,
                reason: None,
            },
        )
        .await
        .unwrap();
        server.await.unwrap();
    }

    /// A `ChatClose` round-trip succeeds: `handle_close` emits `ChatClosed`
    /// (which `call` reads as the first frame) before the terminal `Ack`, so
    /// `ChatClosed` must be treated as success rather than an unexpected frame.
    #[tokio::test]
    async fn call_once_accepts_chat_closed() {
        let dir = tempfile::tempdir().unwrap();
        let (socket, server) = write_frames(
            dir.path(),
            vec![Response::ChatClosed {
                chat_session_id: Uuid::nil(),
                closed_at: chrono_now(),
            }],
        );
        call_once(
            &socket,
            Request::ChatClose {
                chat_session_id: Uuid::nil(),
                runner: None,
                reason: None,
            },
        )
        .await
        .unwrap();
        server.await.unwrap();
    }

    /// Poll until `cond` holds, failing the test after two seconds.
    async fn wait_for(what: &str, cond: impl Fn() -> bool) {
        for _ in 0..200 {
            if cond() {
                return;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        panic!("timed out waiting for {what}");
    }

    /// PIDESKAPP-32: a second `chat_send` (or a warm) for a session whose turn is
    /// still streaming must not abort that turn's reader. The stub mirrors the real
    /// daemon: the first connection streams a turn that completes only once
    /// released, and a send arriving meanwhile is refused with `runner_busy`.
    #[tokio::test]
    async fn second_send_does_not_abort_the_in_flight_turn() {
        let dir = tempfile::tempdir().unwrap();
        let id = Uuid::new_v4();
        let first_message = Uuid::new_v4();
        let socket = socket_path(dir.path());
        let listener = UnixListener::bind(&socket).unwrap();
        let release = Arc::new(tokio::sync::Notify::new());
        let server_release = release.clone();
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let first_turn = tokio::spawn(async move {
                let mut reader = BufReader::new(stream);
                let mut line = String::new();
                reader.read_line(&mut line).await.unwrap();
                let started = Response::ChatMessageStarted {
                    chat_session_id: id,
                    message_id: first_message,
                    turn_id: None,
                    started_at: chrono_now(),
                };
                let mut bytes = serde_json::to_vec(&started).unwrap();
                bytes.push(b'\n');
                reader.get_mut().write_all(&bytes).await.unwrap();
                server_release.notified().await;
                let completed = Response::ChatMessageCompleted {
                    chat_session_id: id,
                    message_id: first_message,
                    turn_id: None,
                    assistant_message: Some("first reply".into()),
                    status: "completed".into(),
                    completed_at: chrono_now(),
                };
                let mut bytes = serde_json::to_vec(&completed).unwrap();
                bytes.push(b'\n');
                // The peer may already be gone; that is the bug under test.
                let _ = reader.get_mut().write_all(&bytes).await;
            });
            // Any further connection is a send racing the in-flight turn.
            let busy = tokio::spawn(async move {
                while let Ok((stream, _)) = listener.accept().await {
                    let mut reader = BufReader::new(stream);
                    let mut line = String::new();
                    reader.read_line(&mut line).await.unwrap();
                    let failed = Response::ChatFailed {
                        chat_session_id: id,
                        code: "runner_busy".into(),
                        detail: None,
                        failed_at: chrono_now(),
                    };
                    let mut bytes = serde_json::to_vec(&failed).unwrap();
                    bytes.push(b'\n');
                    let _ = reader.get_mut().write_all(&bytes).await;
                }
            });
            first_turn.await.unwrap();
            busy.abort();
        });

        let send = |message_id: Uuid| Request::ChatSend {
            chat_session_id: id,
            message_id,
            content: "hi".into(),
            runner: None,
            cwd: None,
            model: None,
            mode: None,
            local_thread_id: None,
            local_session_id: None,
        };
        let state = ChatState::default();
        let sink = Arc::new(CollectSink::default());

        start_stream(
            &state.streams,
            sink.clone(),
            socket.clone(),
            id,
            StreamKind::Turn,
            send(first_message),
        )
        .expect("first send starts a turn");
        wait_for("the first turn to start streaming", || {
            !sink.frames.lock().unwrap().is_empty()
        })
        .await;

        let second = start_stream(
            &state.streams,
            sink.clone(),
            socket.clone(),
            id,
            StreamKind::Turn,
            send(Uuid::new_v4()),
        );
        let warm = Request::ChatWarm {
            chat_session_id: id,
            runner: None,
            cwd: None,
            model: None,
            mode: None,
            local_thread_id: None,
            local_session_id: None,
        };
        start_stream(
            &state.streams,
            sink.clone(),
            socket.clone(),
            id,
            StreamKind::Warm,
            warm,
        )
        .expect("a warm during a live turn is skipped, not an error");
        // Give a wrongly-spawned second reader time to reach the daemon.
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        release.notify_one();
        server.await.unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        let completed = sink.frames.lock().unwrap().iter().any(|f| {
            matches!(f, Response::ChatMessageCompleted { message_id, .. } if *message_id == first_message)
        });
        assert!(
            completed,
            "the in-flight turn's reply was dropped; frames: {:?}, errors: {:?}",
            sink.frames.lock().unwrap(),
            sink.errors.lock().unwrap()
        );
        let err = second.expect_err("a send during a live turn is refused");
        assert!(err.contains("still in progress"), "unexpected error: {err}");
        // Nothing but the one turn ever reached the daemon or the webview.
        assert_eq!(sink.frames.lock().unwrap().len(), 2);
        assert!(sink.errors.lock().unwrap().is_empty());
    }

    /// A finished turn releases its session: the reader deregisters itself on
    /// the terminal frame, so the next send is accepted rather than refused.
    #[tokio::test]
    async fn send_is_accepted_again_once_the_turn_has_ended() {
        let dir = tempfile::tempdir().unwrap();
        let id = Uuid::new_v4();
        let frames = vec![Response::ChatMessageCompleted {
            chat_session_id: id,
            message_id: Uuid::nil(),
            turn_id: None,
            assistant_message: Some("done".into()),
            status: "completed".into(),
            completed_at: chrono_now(),
        }];
        let (socket, server) = write_frames(dir.path(), frames);
        let state = ChatState::default();
        let sink = Arc::new(CollectSink::default());

        start_stream(
            &state.streams,
            sink.clone(),
            socket.clone(),
            id,
            StreamKind::Turn,
            sample_send(),
        )
        .unwrap();
        server.await.unwrap();
        wait_for("the finished reader to deregister", || {
            !state.streams.lock().unwrap().active.contains_key(&id)
        })
        .await;

        start_stream(
            &state.streams,
            sink.clone(),
            socket,
            id,
            StreamKind::Turn,
            sample_send(),
        )
        .expect("a send after the turn ended is accepted");
    }

    /// A send still replaces a warm's reader, as it always has.
    #[tokio::test]
    async fn send_replaces_a_warm_reader() {
        let dir = tempfile::tempdir().unwrap();
        let id = Uuid::new_v4();
        let socket = socket_path(dir.path()); // never bound: both readers fail fast
        let state = ChatState::default();
        let sink = Arc::new(CollectSink::default());
        // Register a warm reader that is still pumping when the send arrives.
        {
            let mut registry = state.streams.lock().unwrap();
            registry.next_generation += 1;
            let generation = registry.next_generation;
            let handle = tauri::async_runtime::spawn(std::future::pending::<()>());
            registry.active.insert(
                id,
                ActiveStream {
                    handle,
                    kind: StreamKind::Warm,
                    generation,
                },
            );
        }
        start_stream(
            &state.streams,
            sink.clone(),
            socket,
            id,
            StreamKind::Turn,
            sample_send(),
        )
        .expect("a send during a warm is accepted");
        let registry = state.streams.lock().unwrap();
        assert!(
            registry
                .active
                .get(&id)
                .is_none_or(|stream| stream.kind == StreamKind::Turn),
            "the warm reader was replaced"
        );
    }

    fn chrono_now() -> chrono::DateTime<chrono::Utc> {
        chrono::Utc::now()
    }
}
