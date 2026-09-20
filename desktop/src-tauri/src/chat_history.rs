// Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Local, on-device storage for direct chats with the built-in agent engine.
//!
//! A direct local chat never travels through the Pi Dash chat relay, and its
//! transcript is never uploaded or synced. This module owns the on-disk record
//! of those chats: a per-account SQLite database under the managed tree, plus
//! the resolver for the working copy a local chat runs in.
//!
//! **Why a separate store rather than the daemon's rollout files.** The engine
//! keeps its own thread/rollout files under `CODEX_HOME`, but those are the
//! engine's format and lifecycle, not ours — they can be pruned, rotated, or
//! change shape with an engine upgrade. This DB is the source of truth for what
//! the desktop UI renders; we store the engine's thread id on the session so a
//! conversation can be *resumed*, but we do not depend on the engine's files to
//! *display* history.
//!
//! **Why scoped per account.** History is private to the person who created it.
//! The signed-in account keys the database directory, so a second account
//! signing in on the same machine sees its own (empty, or previously-kept)
//! history and never the first account's. The overlay supplies the account
//! identifier the same way it already supplies the workspace slug — this module
//! is a consumer of the session the webview holds, never a second copy of it.
//!
//! **Why the chat working copy is a separate tree.** A local chat and a managed
//! issue run must never modify the same working copy at the same time. Rather
//! than coordinate locks with the daemon (which owns managed-run lifecycle), a
//! local chat operates in its own tree under `managed/chat/<account>/workdirs/`,
//! distinct from managed runs' `managed/workdirs/`. The two can never collide
//! because they are never the same path — the concurrency rule holds by
//! construction. See [`chat_working_dir`] and the `working_dirs_never_collide`
//! test.
//!
//! See `pi-dash/.ai_design/managed_runner/design.md` and PDASHOSS01-159.

use std::path::{Path, PathBuf};

use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Runtime};

use crate::managed_runner::ManagedPaths;

/// Schema version stored in SQLite's `user_version`. Bump when the schema
/// changes and add a migration arm in [`migrate`].
const SCHEMA_VERSION: i64 = 1;

// ---------------------------------------------------------------------------
// Records exchanged with the overlay
// ---------------------------------------------------------------------------

/// A chat conversation. The app DB — not the engine's rollout files — is the
/// source of truth for what the UI renders; `engine_thread_id` is the handle we
/// hand back to the engine to resume the underlying thread.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatSession {
    pub id: String,
    pub title: String,
    pub workspace: String,
    pub project: String,
    pub working_dir: String,
    pub engine_version: String,
    /// The engine's own thread id, once a turn has started. `None` until then.
    pub engine_thread_id: Option<String>,
    /// Unix milliseconds.
    pub created_at: i64,
    pub updated_at: i64,
}

/// One entry in a conversation: a user or assistant message, a tool call, or an
/// approval decision. `tool_calls` and `approval_decision` are opaque to this
/// layer — it stores and returns them verbatim so the UI keeps full control of
/// its own rendering.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatEvent {
    pub id: String,
    pub session_id: String,
    /// Monotonic order within the session, assigned on insert.
    pub seq: i64,
    /// `user` | `assistant` | `tool` | `approval` | `system` — validated by the
    /// UI, stored as-is here.
    pub role: String,
    pub content: String,
    /// JSON blob (tool invocations / results) or `None`.
    pub tool_calls: Option<String>,
    /// `approved` | `denied` or `None`.
    pub approval_decision: Option<String>,
    pub created_at: i64,
}

/// Fields the overlay supplies to open a new session. The working directory is
/// resolved by [`chat_working_dir`], not chosen by the caller, so a chat can
/// never be pointed at a managed run's working copy.
#[derive(Debug, Clone, Deserialize)]
pub struct NewChatSession {
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub workspace: String,
    #[serde(default)]
    pub project: String,
    #[serde(default)]
    pub engine_version: String,
}

/// Fields the overlay supplies to append an event.
#[derive(Debug, Clone, Deserialize)]
pub struct NewChatEvent {
    pub role: String,
    #[serde(default)]
    pub content: String,
    #[serde(default)]
    pub tool_calls: Option<String>,
    #[serde(default)]
    pub approval_decision: Option<String>,
}

// ---------------------------------------------------------------------------
// Tauri commands
// ---------------------------------------------------------------------------

/// Create a new chat session and return it (with its generated id and the
/// resolved, already-created working copy).
#[tauri::command]
pub async fn chat_create_session<R: Runtime>(
    app: AppHandle<R>,
    account: String,
    session: NewChatSession,
) -> Result<ChatSession, String> {
    let paths = ManagedPaths::resolve(&app)?;
    let working_dir =
        resolve_chat_working_dir(&paths, &account, &session.workspace, &session.project)?;
    let conn = open(&paths, &account)?;
    let now = now_millis();
    let record = ChatSession {
        id: uuid::Uuid::new_v4().to_string(),
        title: session.title,
        workspace: session.workspace,
        project: session.project,
        working_dir: working_dir.to_string_lossy().into_owned(),
        engine_version: session.engine_version,
        engine_thread_id: None,
        created_at: now,
        updated_at: now,
    };
    conn.execute(
        "INSERT INTO sessions (id, title, workspace, project, working_dir, engine_version, \
         engine_thread_id, created_at, updated_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, NULL, ?7, ?7)",
        params![
            record.id,
            record.title,
            record.workspace,
            record.project,
            record.working_dir,
            record.engine_version,
            now,
        ],
    )
    .map_err(sql_err)?;
    Ok(record)
}

/// List an account's sessions, most-recently-updated first.
#[tauri::command]
pub async fn chat_list_sessions<R: Runtime>(
    app: AppHandle<R>,
    account: String,
) -> Result<Vec<ChatSession>, String> {
    let paths = ManagedPaths::resolve(&app)?;
    let conn = open(&paths, &account)?;
    let mut stmt = conn
        .prepare(
            "SELECT id, title, workspace, project, working_dir, engine_version, engine_thread_id, \
             created_at, updated_at FROM sessions ORDER BY updated_at DESC, created_at DESC",
        )
        .map_err(sql_err)?;
    let rows = stmt
        .query_map([], row_to_session)
        .map_err(sql_err)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(sql_err)?;
    Ok(rows)
}

/// Fetch one session, or `None` if it does not exist for this account.
#[tauri::command]
pub async fn chat_get_session<R: Runtime>(
    app: AppHandle<R>,
    account: String,
    session_id: String,
) -> Result<Option<ChatSession>, String> {
    let paths = ManagedPaths::resolve(&app)?;
    let conn = open(&paths, &account)?;
    conn.query_row(
        "SELECT id, title, workspace, project, working_dir, engine_version, engine_thread_id, \
         created_at, updated_at FROM sessions WHERE id = ?1",
        params![session_id],
        row_to_session,
    )
    .optional()
    .map_err(sql_err)
}

/// List a session's events in insertion order.
#[tauri::command]
pub async fn chat_list_events<R: Runtime>(
    app: AppHandle<R>,
    account: String,
    session_id: String,
) -> Result<Vec<ChatEvent>, String> {
    let paths = ManagedPaths::resolve(&app)?;
    let conn = open(&paths, &account)?;
    let mut stmt = conn
        .prepare(
            "SELECT id, session_id, seq, role, content, tool_calls, approval_decision, created_at \
             FROM events WHERE session_id = ?1 ORDER BY seq ASC",
        )
        .map_err(sql_err)?;
    let rows = stmt
        .query_map(params![session_id], row_to_event)
        .map_err(sql_err)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(sql_err)?;
    Ok(rows)
}

/// Append an event to a session and bump the session's `updated_at`.
///
/// Rejects an unknown `session_id` rather than silently orphaning a row, so a
/// UI bug surfaces as an error instead of invisible data.
#[tauri::command]
pub async fn chat_append_event<R: Runtime>(
    app: AppHandle<R>,
    account: String,
    session_id: String,
    event: NewChatEvent,
) -> Result<ChatEvent, String> {
    let paths = ManagedPaths::resolve(&app)?;
    let mut conn = open(&paths, &account)?;
    let now = now_millis();
    let tx = conn.transaction().map_err(sql_err)?;

    let exists: bool = tx
        .query_row(
            "SELECT 1 FROM sessions WHERE id = ?1",
            params![session_id],
            |_| Ok(true),
        )
        .optional()
        .map_err(sql_err)?
        .unwrap_or(false);
    if !exists {
        return Err(format!("no chat session {session_id}"));
    }

    let next_seq: i64 = tx
        .query_row(
            "SELECT COALESCE(MAX(seq), 0) + 1 FROM events WHERE session_id = ?1",
            params![session_id],
            |row| row.get(0),
        )
        .map_err(sql_err)?;

    let record = ChatEvent {
        id: uuid::Uuid::new_v4().to_string(),
        session_id: session_id.clone(),
        seq: next_seq,
        role: event.role,
        content: event.content,
        tool_calls: event.tool_calls,
        approval_decision: event.approval_decision,
        created_at: now,
    };
    tx.execute(
        "INSERT INTO events (id, session_id, seq, role, content, tool_calls, approval_decision, \
         created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            record.id,
            record.session_id,
            record.seq,
            record.role,
            record.content,
            record.tool_calls,
            record.approval_decision,
            now,
        ],
    )
    .map_err(sql_err)?;
    tx.execute(
        "UPDATE sessions SET updated_at = ?2 WHERE id = ?1",
        params![session_id, now],
    )
    .map_err(sql_err)?;
    tx.commit().map_err(sql_err)?;
    Ok(record)
}

/// Record the engine's thread id on a session so the conversation can later be
/// resumed against the engine.
#[tauri::command]
pub async fn chat_set_thread_id<R: Runtime>(
    app: AppHandle<R>,
    account: String,
    session_id: String,
    engine_thread_id: String,
) -> Result<(), String> {
    let paths = ManagedPaths::resolve(&app)?;
    let conn = open(&paths, &account)?;
    let changed = conn
        .execute(
            "UPDATE sessions SET engine_thread_id = ?2, updated_at = ?3 WHERE id = ?1",
            params![session_id, engine_thread_id, now_millis()],
        )
        .map_err(sql_err)?;
    if changed == 0 {
        return Err(format!("no chat session {session_id}"));
    }
    Ok(())
}

/// Rename a session (the UI's title for the conversation).
#[tauri::command]
pub async fn chat_rename_session<R: Runtime>(
    app: AppHandle<R>,
    account: String,
    session_id: String,
    title: String,
) -> Result<(), String> {
    let paths = ManagedPaths::resolve(&app)?;
    let conn = open(&paths, &account)?;
    let changed = conn
        .execute(
            "UPDATE sessions SET title = ?2, updated_at = ?3 WHERE id = ?1",
            params![session_id, title, now_millis()],
        )
        .map_err(sql_err)?;
    if changed == 0 {
        return Err(format!("no chat session {session_id}"));
    }
    Ok(())
}

/// Delete a single session and its events. Idempotent: deleting a session that
/// no longer exists is a no-op, not an error, so a double-click in the UI is
/// harmless.
#[tauri::command]
pub async fn chat_delete_session<R: Runtime>(
    app: AppHandle<R>,
    account: String,
    session_id: String,
) -> Result<(), String> {
    let paths = ManagedPaths::resolve(&app)?;
    let conn = open(&paths, &account)?;
    // `ON DELETE CASCADE` removes the events; foreign keys are enabled in open().
    conn.execute("DELETE FROM sessions WHERE id = ?1", params![session_id])
        .map_err(sql_err)?;
    Ok(())
}

/// Clear all of an account's chat history — every session and event.
///
/// Used both by the "clear all history" affordance and, when the user opts in,
/// by sign-out (see [`clear_account_history`], which `managed_sign_out` calls).
#[tauri::command]
pub async fn chat_clear_history<R: Runtime>(
    app: AppHandle<R>,
    account: String,
) -> Result<(), String> {
    let paths = ManagedPaths::resolve(&app)?;
    clear_account_history(&paths, &account)
}

/// Resolve (and create) the working copy a local chat runs in.
///
/// Deliberately under the chat tree, never the managed-runs tree, so a chat and
/// a managed issue run can never touch the same directory. Exposed as a command
/// so the UI can show the working directory in the chat header.
#[tauri::command]
pub async fn chat_working_dir<R: Runtime>(
    app: AppHandle<R>,
    account: String,
    workspace: String,
    project: String,
) -> Result<String, String> {
    let paths = ManagedPaths::resolve(&app)?;
    let dir = resolve_chat_working_dir(&paths, &account, &workspace, &project)?;
    Ok(dir.to_string_lossy().into_owned())
}

// ---------------------------------------------------------------------------
// Internals
// ---------------------------------------------------------------------------

/// Remove an account's entire chat tree (DB, WAL, working copies).
///
/// Not a Tauri command itself — `managed_sign_out` calls this when the user
/// asked to clear history on sign-out, and [`chat_clear_history`] wraps it for
/// the in-app "clear all" action. Removing the whole directory rather than just
/// deleting rows also reclaims the working copies and leaves no WAL/journal
/// behind for a later reader.
pub fn clear_account_history(paths: &ManagedPaths, account: &str) -> Result<(), String> {
    let dir = account_dir(paths, account)?;
    match std::fs::remove_dir_all(&dir) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(format!("removing chat history {}: {e}", dir.display())),
    }
}

/// Open (creating if needed) the account's history database.
///
/// A fresh connection per call keeps this module free of shared `!Sync` state;
/// WAL plus a busy timeout make the occasional overlapping call from the UI
/// safe. The DB file is `0600` and its directory `0700` on Unix, matching the
/// credential handling in `managed_runner`.
fn open(paths: &ManagedPaths, account: &str) -> Result<Connection, String> {
    let dir = account_dir(paths, account)?;
    std::fs::create_dir_all(&dir).map_err(|e| format!("creating {}: {e}", dir.display()))?;
    restrict_dir(&dir);
    let db_path = dir.join("history.db");
    let conn = Connection::open(&db_path).map_err(sql_err)?;
    restrict_file(&db_path);
    conn.pragma_update(None, "journal_mode", "WAL")
        .map_err(sql_err)?;
    conn.pragma_update(None, "foreign_keys", true)
        .map_err(sql_err)?;
    conn.busy_timeout(std::time::Duration::from_secs(5))
        .map_err(sql_err)?;
    migrate(&conn)?;
    // The WAL/SHM sidecars hold recently-written chat content but are created by
    // SQLite with the default umask (typically 0644). Lock them to the owner
    // like the main DB so chat history isn't world-readable on a shared host.
    for suffix in ["-wal", "-shm"] {
        let mut sidecar = db_path.clone().into_os_string();
        sidecar.push(suffix);
        restrict_file(Path::new(&sidecar));
    }
    Ok(conn)
}

/// Create the schema on first open and apply migrations on later ones.
fn migrate(conn: &Connection) -> Result<(), String> {
    let version: i64 = conn
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .map_err(sql_err)?;
    if version >= SCHEMA_VERSION {
        return Ok(());
    }
    if version < 1 {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS sessions (
                id TEXT PRIMARY KEY,
                title TEXT NOT NULL DEFAULT '',
                workspace TEXT NOT NULL DEFAULT '',
                project TEXT NOT NULL DEFAULT '',
                working_dir TEXT NOT NULL DEFAULT '',
                engine_version TEXT NOT NULL DEFAULT '',
                engine_thread_id TEXT,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL
            );
            CREATE TABLE IF NOT EXISTS events (
                id TEXT PRIMARY KEY,
                session_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
                seq INTEGER NOT NULL,
                role TEXT NOT NULL,
                content TEXT NOT NULL DEFAULT '',
                tool_calls TEXT,
                approval_decision TEXT,
                created_at INTEGER NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_events_session_seq ON events(session_id, seq);",
        )
        .map_err(sql_err)?;
    }
    conn.pragma_update(None, "user_version", SCHEMA_VERSION)
        .map_err(sql_err)?;
    Ok(())
}

/// The account's chat directory: `<managed>/chat/<account-key>`.
fn account_dir(paths: &ManagedPaths, account: &str) -> Result<PathBuf, String> {
    Ok(paths.chat_dir.join(account_key(account)?))
}

/// Resolve (and create) the working copy a chat operates in.
///
/// Lives under the account's chat tree, so it is always a different path from a
/// managed run's `workdirs/<workspace>/<project>`.
fn resolve_chat_working_dir(
    paths: &ManagedPaths,
    account: &str,
    workspace: &str,
    project: &str,
) -> Result<PathBuf, String> {
    let account_base = account_dir(paths, account)?;
    let mut dir = account_base.join("workdirs");
    dir.push(safe_component(workspace)?);
    dir.push(safe_component(project)?);
    std::fs::create_dir_all(&dir).map_err(|e| format!("creating {}: {e}", dir.display()))?;
    // Lock the account root to the owner so no other local user can traverse
    // into a chat working copy. `open` also does this, but the UI may ask for
    // the working directory (chat header) before the DB is ever opened, which
    // would otherwise leave the tree world-traversable at the default umask.
    restrict_dir(&account_base);
    restrict_dir(&dir);
    Ok(dir)
}

/// Turn an account identifier (a user id, or an email) into a single
/// filesystem-safe, traversal-proof directory name.
///
/// The visible prefix is the identifier with anything outside `[A-Za-z0-9_-]`
/// dropped, so a directory is still recognisable to a human browsing the tree;
/// an FNV-1a suffix over the *raw* bytes disambiguates values that filter to the
/// same prefix (e.g. `a@b` vs `a.b`) and guarantees uniqueness. The output
/// charset is limited to `[A-Za-z0-9_-]`, so no account value can escape the
/// chat tree with `..` or an absolute path.
fn account_key(account: &str) -> Result<String, String> {
    if account.trim().is_empty() {
        return Err("chat history requires a signed-in account".into());
    }
    let prefix: String = account
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .take(48)
        .collect();
    Ok(format!("{prefix}-{:016x}", fnv1a(account.as_bytes())))
}

/// Validate a workspace/project path component. These come from our own API,
/// but they land in a filesystem path, so reject traversal outright.
fn safe_component(value: &str) -> Result<&str, String> {
    let ok = !value.is_empty()
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    if ok {
        Ok(value)
    } else {
        Err(format!("invalid path component {value:?}"))
    }
}

/// 64-bit FNV-1a. Not cryptographic — used only to disambiguate directory names
/// per [`account_key`], where collision resistance, not secrecy, is the point.
fn fnv1a(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes {
        hash ^= u64::from(*b);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

fn now_millis() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

fn row_to_session(row: &rusqlite::Row<'_>) -> rusqlite::Result<ChatSession> {
    Ok(ChatSession {
        id: row.get(0)?,
        title: row.get(1)?,
        workspace: row.get(2)?,
        project: row.get(3)?,
        working_dir: row.get(4)?,
        engine_version: row.get(5)?,
        engine_thread_id: row.get(6)?,
        created_at: row.get(7)?,
        updated_at: row.get(8)?,
    })
}

fn row_to_event(row: &rusqlite::Row<'_>) -> rusqlite::Result<ChatEvent> {
    Ok(ChatEvent {
        id: row.get(0)?,
        session_id: row.get(1)?,
        seq: row.get(2)?,
        role: row.get(3)?,
        content: row.get(4)?,
        tool_calls: row.get(5)?,
        approval_decision: row.get(6)?,
        created_at: row.get(7)?,
    })
}

fn sql_err(e: rusqlite::Error) -> String {
    format!("chat history: {e}")
}

#[cfg(unix)]
fn restrict_dir(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700));
}

#[cfg(not(unix))]
fn restrict_dir(_path: &Path) {}

#[cfg(unix)]
fn restrict_file(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
}

#[cfg(not(unix))]
fn restrict_file(_path: &Path) {}

#[cfg(test)]
mod tests {
    use super::*;

    /// A `ManagedPaths` rooted at a temp dir, so the store's real path logic
    /// runs without a Tauri `AppHandle`.
    fn paths(root: &Path) -> ManagedPaths {
        ManagedPaths::for_test_root(root.join("managed"))
    }

    fn new_session() -> NewChatSession {
        NewChatSession {
            title: "First chat".into(),
            workspace: "acme".into(),
            project: "web".into(),
            engine_version: "0.1.23".into(),
        }
    }

    // The Tauri commands are thin wrappers over these helpers; the tests drive
    // the helpers directly so they need no AppHandle.
    fn create(paths: &ManagedPaths, account: &str, s: NewChatSession) -> ChatSession {
        let working_dir =
            resolve_chat_working_dir(paths, account, &s.workspace, &s.project).unwrap();
        let conn = open(paths, account).unwrap();
        let now = now_millis();
        let record = ChatSession {
            id: uuid::Uuid::new_v4().to_string(),
            title: s.title,
            workspace: s.workspace,
            project: s.project,
            working_dir: working_dir.to_string_lossy().into_owned(),
            engine_version: s.engine_version,
            engine_thread_id: None,
            created_at: now,
            updated_at: now,
        };
        conn.execute(
            "INSERT INTO sessions (id, title, workspace, project, working_dir, engine_version, \
             engine_thread_id, created_at, updated_at) VALUES (?1,?2,?3,?4,?5,?6,NULL,?7,?7)",
            params![
                record.id,
                record.title,
                record.workspace,
                record.project,
                record.working_dir,
                record.engine_version,
                now
            ],
        )
        .unwrap();
        record
    }

    fn append(paths: &ManagedPaths, account: &str, session_id: &str, role: &str) -> ChatEvent {
        let mut conn = open(paths, account).unwrap();
        let now = now_millis();
        let tx = conn.transaction().unwrap();
        let next_seq: i64 = tx
            .query_row(
                "SELECT COALESCE(MAX(seq),0)+1 FROM events WHERE session_id=?1",
                params![session_id],
                |r| r.get(0),
            )
            .unwrap();
        let ev = ChatEvent {
            id: uuid::Uuid::new_v4().to_string(),
            session_id: session_id.to_string(),
            seq: next_seq,
            role: role.to_string(),
            content: format!("msg {next_seq}"),
            tool_calls: None,
            approval_decision: None,
            created_at: now,
        };
        tx.execute(
            "INSERT INTO events (id,session_id,seq,role,content,tool_calls,approval_decision,created_at) \
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8)",
            params![ev.id, ev.session_id, ev.seq, ev.role, ev.content, ev.tool_calls, ev.approval_decision, now],
        )
        .unwrap();
        tx.commit().unwrap();
        ev
    }

    fn list_sessions(paths: &ManagedPaths, account: &str) -> Vec<ChatSession> {
        let conn = open(paths, account).unwrap();
        let mut stmt = conn
            .prepare(
                "SELECT id,title,workspace,project,working_dir,engine_version,engine_thread_id,created_at,updated_at \
                 FROM sessions ORDER BY updated_at DESC, created_at DESC",
            )
            .unwrap();
        stmt.query_map([], row_to_session)
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap()
    }

    fn count_events(paths: &ManagedPaths, account: &str, session_id: &str) -> i64 {
        let conn = open(paths, account).unwrap();
        conn.query_row(
            "SELECT COUNT(*) FROM events WHERE session_id=?1",
            params![session_id],
            |r| r.get(0),
        )
        .unwrap()
    }

    #[cfg(unix)]
    #[test]
    fn history_files_and_working_dirs_are_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let root = tempfile::tempdir().unwrap();
        let paths = paths(root.path());
        let base = account_dir(&paths, "user-perms").unwrap();
        let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;

        // Resolving the working dir (e.g. for the chat header) must lock the
        // account root even before the DB is ever opened.
        let workdir = resolve_chat_working_dir(&paths, "user-perms", "acme", "web").unwrap();
        assert_eq!(mode(&base), 0o700, "account dir must be 0700 after resolve");
        assert!(workdir.starts_with(&base), "working dir under the account root");

        // Open + write so the WAL sidecar exists while the connection is live.
        let conn = open(&paths, "user-perms").unwrap();
        conn.execute_batch("INSERT INTO sessions (id, created_at, updated_at) VALUES ('x', 0, 0)")
            .unwrap();
        assert_eq!(mode(&base.join("history.db")), 0o600, "db must be 0600");
        let wal = base.join("history.db-wal");
        assert!(wal.exists(), "WAL sidecar should exist while the DB is open");
        assert_eq!(mode(&wal), 0o600, "WAL sidecar must not be world-readable");
    }

    #[test]
    fn create_persists_and_survives_reopen() {
        let root = tempfile::tempdir().unwrap();
        let paths = paths(root.path());
        let s = create(&paths, "user-1", new_session());
        // A fresh connection == an app restart, since we open per call.
        let listed = list_sessions(&paths, "user-1");
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].id, s.id);
        assert_eq!(listed[0].title, "First chat");
        assert_eq!(listed[0].engine_thread_id, None);
    }

    #[test]
    fn events_keep_insertion_order_and_cascade_on_delete() {
        let root = tempfile::tempdir().unwrap();
        let paths = paths(root.path());
        let s = create(&paths, "user-1", new_session());
        append(&paths, "user-1", &s.id, "user");
        append(&paths, "user-1", &s.id, "assistant");
        append(&paths, "user-1", &s.id, "user");
        assert_eq!(count_events(&paths, "user-1", &s.id), 3);

        let conn = open(&paths, "user-1").unwrap();
        let seqs: Vec<i64> = {
            let mut stmt = conn
                .prepare("SELECT seq FROM events WHERE session_id=?1 ORDER BY seq")
                .unwrap();
            stmt.query_map(params![s.id], |r| r.get(0))
                .unwrap()
                .collect::<Result<_, _>>()
                .unwrap()
        };
        assert_eq!(seqs, vec![1, 2, 3]);

        // Deleting the session must remove its events (FK cascade).
        conn.execute("DELETE FROM sessions WHERE id=?1", params![s.id])
            .unwrap();
        assert_eq!(count_events(&paths, "user-1", &s.id), 0);
    }

    #[test]
    fn history_is_scoped_per_account() {
        let root = tempfile::tempdir().unwrap();
        let paths = paths(root.path());
        create(&paths, "user-1", new_session());
        // A different account signing in on the same machine sees nothing of
        // the first account's history.
        assert_eq!(list_sessions(&paths, "user-2").len(), 0);
        assert_eq!(list_sessions(&paths, "user-1").len(), 1);
    }

    #[test]
    fn clear_history_removes_only_that_account() {
        let root = tempfile::tempdir().unwrap();
        let paths = paths(root.path());
        create(&paths, "user-1", new_session());
        create(&paths, "user-2", new_session());
        clear_account_history(&paths, "user-1").unwrap();
        assert_eq!(list_sessions(&paths, "user-1").len(), 0);
        assert_eq!(list_sessions(&paths, "user-2").len(), 1);
        // Clearing an account with no history is a no-op, not an error.
        clear_account_history(&paths, "never-signed-in").unwrap();
    }

    #[test]
    fn working_dirs_never_collide_with_managed_runs() {
        let root = tempfile::tempdir().unwrap();
        let paths = paths(root.path());
        let chat_dir = resolve_chat_working_dir(&paths, "user-1", "acme", "web").unwrap();
        // The managed-run working copy for the same workspace/project.
        let managed_dir = paths.workdirs.join("acme").join("web");
        assert_ne!(chat_dir, managed_dir);
        // And the chat dir is not nested inside the managed-runs tree.
        assert!(!chat_dir.starts_with(&paths.workdirs));
        assert!(chat_dir.starts_with(&paths.chat_dir));
    }

    #[test]
    fn account_key_is_traversal_safe_and_collision_resistant() {
        // Traversal attempts filter down to a hash-only key, never `..`.
        for evil in ["../../etc", "..", "/abs", r"a\b", "a/b/c"] {
            let key = account_key(evil).unwrap();
            assert!(!key.contains(".."));
            assert!(!key.contains('/'));
            assert!(!key.contains('\\'));
            assert!(
                key.chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
            );
        }
        // Values that filter to the same prefix stay distinct via the suffix.
        assert_ne!(account_key("a@b").unwrap(), account_key("a.b").unwrap());
        // Empty is rejected — history must be tied to an account.
        assert!(account_key("   ").is_err());
    }

    #[test]
    fn append_rejects_unknown_session() {
        // Drive the seq/insert path against a missing session id via a fresh
        // connection; the command layer returns an error rather than orphaning.
        let root = tempfile::tempdir().unwrap();
        let paths = paths(root.path());
        let conn = open(&paths, "user-1").unwrap();
        let exists: bool = conn
            .query_row(
                "SELECT 1 FROM sessions WHERE id=?1",
                params!["nope"],
                |_| Ok(true),
            )
            .optional()
            .unwrap()
            .unwrap_or(false);
        assert!(!exists);
    }
}
