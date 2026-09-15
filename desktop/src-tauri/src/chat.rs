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
use pidash_ipc::{ApprovalDecision, Client};
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
    streams: Arc<Mutex<HashMap<Uuid, JoinHandle<()>>>>,
}

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
/// `Chat*` frame is handed to `sink`; the terminal `Ack` ends the stream, and a
/// `Response::Error` becomes an `Err` the caller reports as a transport error.
async fn drive_stream<S: ChatSink>(
    mut client: Client,
    first: Response,
    sink: &S,
) -> Result<(), String> {
    let mut frame = first;
    loop {
        match frame {
            // The daemon's terminator after the last chat frame.
            Response::Ack => return Ok(()),
            Response::Error(err) => {
                return Err(format!("daemon error {}: {}", err.code, err.message));
            }
            other => sink.frame(&other),
        }
        match client.read_next().await.map_err(|e| e.to_string())? {
            Some(next) => frame = next,
            None => return Ok(()),
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
fn resolve_socket<R: Runtime>(app: &AppHandle<R>) -> Result<PathBuf, String> {
    let paths = ManagedPaths::resolve(app)?;
    Ok(socket_path(&paths.runtime_dir))
}

/// Spawn the background reader for a streaming request and register its handle
/// so [`chat_close`] can abort it. On any transport error the synthetic
/// [`CHAT_ERROR_EVENT`] tells the webview which session failed.
fn spawn_stream<R: Runtime>(
    app: AppHandle<R>,
    socket: PathBuf,
    chat_session_id: Uuid,
    req: Request,
) {
    let streams = app.state::<ChatState>().streams.clone();
    let sink = Arc::new(TauriSink { app });
    let handle = tauri::async_runtime::spawn(async move {
        if let Err(e) = stream_session(&socket, req, sink.as_ref()).await {
            sink.error(chat_session_id, &e);
        }
    });
    streams.lock().unwrap().insert(chat_session_id, handle);
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
    chat_session_id: Uuid,
    runner: Option<String>,
    cwd: Option<String>,
    model: Option<String>,
    local_thread_id: Option<String>,
    local_session_id: Option<String>,
) -> Result<(), String> {
    let socket = resolve_socket(&app)?;
    let req = Request::ChatWarm {
        chat_session_id,
        runner,
        cwd,
        model,
        local_thread_id,
        local_session_id,
    };
    spawn_stream(app, socket, chat_session_id, req);
    Ok(())
}

/// Submit a user turn. Streams the turn's `Chat*` frames to the webview until a
/// terminal `ChatMessageCompleted` / `ChatFailed`.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn chat_send<R: Runtime>(
    app: AppHandle<R>,
    chat_session_id: Uuid,
    message_id: Uuid,
    content: String,
    runner: Option<String>,
    cwd: Option<String>,
    model: Option<String>,
    local_thread_id: Option<String>,
    local_session_id: Option<String>,
) -> Result<(), String> {
    let socket = resolve_socket(&app)?;
    let req = Request::ChatSend {
        chat_session_id,
        message_id,
        content,
        runner,
        cwd,
        model,
        local_thread_id,
        local_session_id,
    };
    spawn_stream(app, socket, chat_session_id, req);
    Ok(())
}

/// Interrupt the in-flight turn of a local chat session.
#[tauri::command]
pub async fn chat_cancel<R: Runtime>(
    app: AppHandle<R>,
    chat_session_id: Uuid,
    runner: Option<String>,
    reason: Option<String>,
) -> Result<(), String> {
    let socket = resolve_socket(&app)?;
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
    chat_session_id: Uuid,
    runner: Option<String>,
    reason: Option<String>,
) -> Result<(), String> {
    let socket = resolve_socket(&app)?;
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
    if let Some(handle) = app
        .state::<ChatState>()
        .streams
        .lock()
        .unwrap()
        .remove(&chat_session_id)
    {
        handle.abort();
    }
    result
}

/// Answer a pending chat approval, routed into the runner's shared approval
/// router as `DecisionSource::Local` by the slice-1b handler.
#[tauri::command]
pub async fn chat_decide<R: Runtime>(
    app: AppHandle<R>,
    chat_session_id: Uuid,
    local_approval_id: String,
    decision: ApprovalDecision,
    runner: Option<String>,
) -> Result<(), String> {
    let socket = resolve_socket(&app)?;
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
            local_thread_id: None,
            local_session_id: None,
        }
    }

    /// A `ChatSend` streams every `Chat*` frame to the sink, in order, and the
    /// terminal `Ack` ends the stream cleanly without being surfaced.
    #[tokio::test]
    async fn streams_chat_frames_then_stops_on_ack() {
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
            Response::Ack,
        ];
        let (socket, server) = write_frames(dir.path(), frames);

        let sink = CollectSink::default();
        stream_session(&socket, sample_send(), &sink).await.unwrap();
        server.await.unwrap();

        let got = sink.frames.lock().unwrap();
        assert_eq!(got.len(), 3, "3 chat frames, Ack not forwarded");
        assert!(matches!(got[0], Response::ChatMessageStarted { .. }));
        assert!(matches!(got[1], Response::ChatEvent { .. }));
        assert!(matches!(got[2], Response::ChatMessageCompleted { .. }));
        assert!(sink.errors.lock().unwrap().is_empty());
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

    fn chrono_now() -> chrono::DateTime<chrono::Utc> {
        chrono::Utc::now()
    }
}
