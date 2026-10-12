/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

/**
 * Desktop **local** chat transport (PDASHOSS01-159, slice-4 remainder).
 *
 * This is the browser-side half of direct local chat: it plugs into the
 * `ChatTransport` seam (`@pi-dash/services`) so the shared runner chat UI —
 * `RunnerChatPage`, `useAgentChatEvents` — drives the bundled agent engine on
 * the same machine, never the Pi Dash cloud chat relay. Two directions:
 *
 * * **UI → engine** — the transport's verbs call the slice-2 Tauri commands
 *   (`chat_warm` / `chat_send` / `chat_cancel` / `chat_close` / `chat_decide`)
 *   via `invoke`, and the slice-3 history commands (`chat_create_session`,
 *   `chat_list_sessions`, `chat_list_events`, `chat_append_event`, …) for the
 *   session/message list the UI renders.
 * * **engine → UI** — `subscribeChatEvents` listens to the slice-2 Tauri events
 *   `chat://frame` / `chat://error`, filters by chat-session id, and translates
 *   each daemon `Response::Chat*` frame (slice-1a `pidash-ipc` wire protocol)
 *   into the server-shaped `IAgentChatEvent` the UI already understands, so the
 *   page's `handleEvent` logic is unchanged between cloud and local.
 *
 * The app SQLite DB (slice 3) — not the engine's rollout files — is the source
 * of truth for what the UI renders, so each turn is persisted to history as it
 * streams: the user's message on `sendChatMessage`, the engine thread id on
 * `chat_started` (for resume), and the assistant's message on
 * `chat_message_completed`.
 *
 * Registered desktop-only via [`registerLocalChatTransport`], called from the
 * overlay boot seam. The shared cloud web app never imports this module.
 */

import {
  getRunnerDetailFetcher,
  setChatTransport,
  setRunnerDetailFetcher,
  type ChatEventErrorHandler,
  type ChatEventHandler,
  type ChatEventUnsubscribe,
  type ChatTransport,
  type RunnerDetailFetcher,
} from "@pi-dash/services";
import type {
  IAgentChatEvent,
  IAgentChatMessage,
  IAgentChatSession,
  IRunner,
  TAgentChatMessageRole,
  TApprovalDecision,
  TApprovalMode,
} from "@pi-dash/types";
import {
  ensureChatRuntime,
  getAgentAccount,
  getModelTopUpUrl,
  isDesktop,
  refreshModelCredential,
} from "@/services/agent-runtime";

/** Synthetic runner id for the bundled built-in engine (see the picker seam). */
export const BUILTIN_RUNNER_ID = "pidash-builtin";

/** Tauri event carrying one streamed `Response::Chat*` frame (see `chat.rs`). */
const CHAT_FRAME_EVENT = "chat://frame";
/** Tauri event carrying a transport-level failure keyed by chat-session id. */
const CHAT_ERROR_EVENT = "chat://error";

// --- Rust record shapes (serde snake_case) ---------------------------------

/** `chat_history::ChatSession` (slice 3). Timestamps are unix milliseconds. */
interface StoredSession {
  id: string;
  title: string;
  workspace: string;
  /** We store the runner id here (slice-3 has no dedicated runner column). */
  project: string;
  working_dir: string;
  engine_version: string;
  engine_thread_id: string | null;
  created_at: number;
  updated_at: number;
}

/** `chat_history::ChatEvent` (slice 3). */
interface StoredEvent {
  id: string;
  session_id: string;
  seq: number;
  role: string;
  content: string;
  tool_calls: string | null;
  approval_decision: string | null;
  created_at: number;
}

// --- Daemon `Response::Chat*` frames (pidash-ipc, tag=result/content=data) --

type ChatFrame =
  | { result: "chat_started"; data: FrameStarted }
  | { result: "chat_message_started"; data: FrameMessageStarted }
  | { result: "chat_event"; data: FrameEvent }
  | { result: "chat_approval_request"; data: FrameApproval }
  | { result: "chat_message_completed"; data: FrameCompleted }
  | { result: "chat_failed"; data: FrameFailed }
  | { result: "chat_closed"; data: FrameClosed };

interface FrameStarted {
  chat_session_id: string;
  local_thread_id: string;
  local_session_id?: string;
  started_at: string;
}
interface FrameMessageStarted {
  chat_session_id: string;
  message_id: string;
  turn_id?: string;
  started_at: string;
}
interface FrameEvent {
  chat_session_id: string;
  bridge_seq: number;
  kind: string;
  payload: Record<string, unknown>;
}
interface FrameApproval {
  chat_session_id: string;
  local_approval_id: string;
  kind: string;
  payload: Record<string, unknown>;
  reason?: string;
  expires_at?: string;
}
interface FrameCompleted {
  chat_session_id: string;
  message_id: string;
  turn_id?: string;
  assistant_message?: string;
  status: string;
  completed_at: string;
}
interface FrameFailed {
  chat_session_id: string;
  code: string;
  detail?: string;
  failed_at: string;
}
interface FrameClosed {
  chat_session_id: string;
  closed_at: string;
}

/**
 * `chat_failed` codes the daemon sets when the model gateway refuses a request
 * outright (`runner/src/ipc/chat.rs`). It stops the turn on the first refusal;
 * `detail` is the gateway's own text.
 */
const MODEL_CREDIT_EXHAUSTED = "model_credit_exhausted";
const MODEL_AUTH_FAILED = "model_auth_failed";

const SIGN_IN_AGAIN =
  "The model gateway did not accept this sign-in. Sign out and sign in again, then resend your message.";

/** What the transcript says when a turn ends because the account has no credit. */
function creditNote(detail: string | undefined, topUpUrl: string): string {
  const text = detail?.trim() || "Your model credit has run out.";
  return topUpUrl ? `${text}\nAdd credit: ${topUpUrl}` : text;
}

/** The chat-session id a frame belongs to, for routing to the right stream. */
function frameSessionId(frame: ChatFrame): string {
  return frame.data.chat_session_id;
}

/** Whether a frame ends its turn — the daemon sends exactly one per `chat_send`. */
function isTerminalFrame(frame: ChatFrame): boolean {
  return frame.result === "chat_message_completed" || frame.result === "chat_failed" || frame.result === "chat_closed";
}

/**
 * Pull the assistant text out of an `assistant_delta` frame payload. The daemon
 * wraps engine frames as `{ method, params }`; the text lives on `params.delta`
 * (a string, or `{ text }`) or `params.text`. Mirrors the runner-side
 * `assistant_delta_text` (runner/src/ipc/chat.rs) so the accumulated fallback
 * matches what streamed to the UI.
 */
function assistantDeltaText(payload: Record<string, unknown>): string {
  const params = (payload?.params as Record<string, unknown>) ?? payload ?? {};
  const delta = (params as Record<string, unknown>).delta;
  if (typeof delta === "string") return delta;
  if (delta && typeof (delta as Record<string, unknown>).text === "string") {
    return (delta as Record<string, unknown>).text as string;
  }
  const text = (params as Record<string, unknown>).text;
  return typeof text === "string" ? text : "";
}

/**
 * Access to the native host, injectable so the transport can be unit-tested
 * with `invoke` and the event channel mocked. The production factory
 * ([`tauriBridge`]) reads `window.__TAURI__`.
 */
export interface TauriBridge {
  invoke<T>(command: string, args?: Record<string, unknown>): Promise<T>;
  /** Subscribe to a Tauri event; resolves to an unlisten function. */
  listen<T>(event: string, handler: (payload: T) => void): Promise<() => void>;
  /** The signed-in account id that scopes local history. */
  getAccount(): string;
  /**
   * Make the bundled daemon ready to serve this chat: enrol the machine if
   * needed, write the engine config and model credential, and start the
   * daemon. Without it a chat opened straight after sign-in has nothing to
   * talk to, and the send fails at "connecting to managed daemon".
   */
  ensureRuntime(): Promise<void>;
  /**
   * Workspace slug. The daemon runs per workspace and binds its control
   * socket under that workspace's data dir, so every chat command carries the
   * slug — without it the host resolves a socket nothing is listening on.
   */
  workspaceSlug(): string;
  /**
   * Mint and store a new model credential, regardless of the current one's
   * expiry. Rejects when none can be had (the Pi Dash session is gone).
   */
  refreshModelCredential(): Promise<void>;
  /** Where the user adds model credit; empty when the server names no place. */
  topUpUrl(): string;
}

/** A turn in flight: what was asked, so it can be submitted once more. */
interface ActiveTurn {
  /** The daemon message id of the current submission. */
  messageId: string;
  content: string;
  /** Set once the turn has been resubmitted with a new model credential. */
  authRetried: boolean;
  /** Set when the user stopped the turn; it is not submitted again. */
  stopped?: boolean;
}

interface FrameSubscriber {
  onFrame(frame: ChatFrame): void;
  onError(error: Error): void;
}

/**
 * Translate the daemon's ordered `Response::Chat*` frame stream into the
 * server-shaped `IAgentChatEvent`s the shared UI consumes. Stateful: it assigns
 * a monotonic `seq` (the UI dedupes on it) and remembers the active assistant
 * `message_id` so streamed `assistant_delta`s group onto the right message.
 */
export class ChatFrameTranslator {
  private seq = 0;
  private currentMessageId: string | null = null;

  constructor(private readonly sessionId: string) {}

  translate(frame: ChatFrame): IAgentChatEvent | null {
    switch (frame.result) {
      case "chat_started":
        return this.event("turn_started", null, {
          local_thread_id: frame.data.local_thread_id,
          local_session_id: frame.data.local_session_id ?? "",
        });
      case "chat_message_started":
        this.currentMessageId = frame.data.message_id;
        return this.event("message_started", frame.data.message_id, {
          turn_id: frame.data.turn_id ?? "",
        });
      case "chat_event":
        return this.event(
          frame.data.kind,
          frame.data.kind === "assistant_delta" ? this.currentMessageId : null,
          frame.data.payload
        );
      case "chat_approval_request":
        return this.event("chat_approval_request", null, {
          local_approval_id: frame.data.local_approval_id,
          approval_kind: frame.data.kind,
          reason: frame.data.reason ?? "",
          expires_at: frame.data.expires_at ?? null,
          ...frame.data.payload,
        });
      case "chat_message_completed": {
        const message = frame.data.message_id;
        this.currentMessageId = null;
        return this.event("turn_completed", message, {
          status: frame.data.status,
          assistant_message: frame.data.assistant_message ?? "",
          turn_id: frame.data.turn_id ?? "",
        });
      }
      case "chat_failed":
        return this.event("chat_failed", null, {
          code: frame.data.code,
          detail: frame.data.detail ?? "",
        });
      case "chat_closed":
        return this.event("chat_closed", null, {});
      default:
        return null;
    }
  }

  private event(kind: string, message: string | null, payload: Record<string, unknown>): IAgentChatEvent {
    this.seq += 1;
    return {
      id: this.seq,
      session: this.sessionId,
      message,
      seq: this.seq,
      kind,
      payload,
      // A fresh timestamp keeps `assistant_delta`s classified as realtime by
      // the page's `isRealtimeEvent` check (which compares against the mount
      // time of the current session).
      created_at: new Date().toISOString(),
    };
  }
}

function toIso(millis: number): string {
  return new Date(millis).toISOString();
}

function mapRole(role: string): TAgentChatMessageRole {
  if (role === "assistant" || role === "tool" || role === "user") return role;
  // Slice-3 stores "approval"/"system"; the UI message role has no "approval".
  return "system";
}

function storedSessionToChatSession(
  s: StoredSession,
  account: string,
  activeMessageId: string | null = null
): IAgentChatSession {
  // `updated_at` is bumped whenever an event is appended, so a session that has
  // been written to since creation has had at least one message.
  const lastMessageAt = s.updated_at > s.created_at ? toIso(s.updated_at) : null;
  return {
    id: s.id,
    workspace: s.workspace,
    runner: s.project,
    runner_detail: null,
    created_by: account,
    pod: "",
    status: "open",
    agent_kind: "managed_runner",
    local_thread_id: s.engine_thread_id ?? "",
    local_session_id: "",
    cwd: s.working_dir,
    model: "",
    active_turn_id: "",
    // The daemon message id of the turn in flight, if any. The shared page
    // reads this as "Response in progress" and holds the composer until the
    // turn ends, exactly as it does for a cloud session.
    active_message_id: activeMessageId,
    close_requested: false,
    last_message_at: lastMessageAt,
    closed_at: null,
    error: "",
    created_at: toIso(s.created_at),
    updated_at: toIso(s.updated_at),
  };
}

function storedEventToMessage(e: StoredEvent): IAgentChatMessage {
  return {
    id: e.id,
    session: e.session_id,
    role: mapRole(e.role),
    content: e.content,
    content_parts: [],
    status: "completed",
    local_item_id: "",
    local_turn_id: "",
    seq: e.seq,
    created_at: toIso(e.created_at),
    completed_at: toIso(e.created_at),
  };
}

/**
 * `ChatTransport` backed by the bundled engine over Tauri IPC. Session and
 * message lists come from the slice-3 local SQLite store; live turns stream
 * through the slice-2 Tauri command + event surface.
 */
/** The engine's historical posture, used when no mode has been chosen. */
export const DEFAULT_APPROVAL_MODE: TApprovalMode = "full_access";

export class LocalChatTransport implements ChatTransport {
  constructor(private readonly bridge: TauriBridge) {}

  /**
   * Per-session approval mode. The runner captures the mode when it spawns the
   * session's engine thread and holds it for that thread's life, so the value
   * sent on each `chat_warm` / `chat_send` only takes effect for a session that
   * has not yet been warmed — changing it affects the next thread, never a turn
   * already in flight. In-memory, additive local-only state (like
   * `decideChatApproval`); the shared `ChatTransport` interface is untouched.
   */
  private readonly approvalModes = new Map<string, TApprovalMode>();

  /** Set the approval mode for a session's next thread. Local-only verb. */
  setApprovalMode(sessionId: string, mode: TApprovalMode): void {
    this.approvalModes.set(sessionId, mode);
  }

  /** The mode a session will warm under, defaulting to full access. */
  getApprovalMode(sessionId: string): TApprovalMode {
    return this.approvalModes.get(sessionId) ?? DEFAULT_APPROVAL_MODE;
  }

  /**
   * Turns in flight, by session id. The local store has no notion of an active
   * turn, and `chat_send` resolves as soon as the turn is submitted, so without
   * this the composer re-enabled mid-turn and a second send cut the first one
   * off. Set on send, cleared by the turn's terminal frame or a transport
   * error. In-memory: after a webview reload the host's own refusal of an
   * overlapping `chat_send` is the backstop.
   */
  private readonly activeTurns = new Map<string, ActiveTurn>();
  private readonly subscribers = new Map<string, Set<FrameSubscriber>>();
  private listening = false;

  /**
   * Listen to the host for as long as the transport lives, not just while a
   * session is on screen: `subscribeChatEvents` unsubscribes when the user
   * switches session, and a turn that finishes meanwhile must still release
   * its session — or be retried, or leave its failure in the transcript.
   */
  private listenToHost(): void {
    if (this.listening) return;
    this.listening = true;
    void this.bridge.listen<ChatFrame>(CHAT_FRAME_EVENT, (frame) => this.routeFrame(frame));
    void this.bridge.listen<{ chat_session_id: string; message: string }>(CHAT_ERROR_EVENT, (payload) => {
      this.activeTurns.delete(payload.chat_session_id);
      const error = new Error(payload.message);
      for (const subscriber of this.subscribers.get(payload.chat_session_id) ?? []) subscriber.onError(error);
    });
  }

  private deliver(frame: ChatFrame): void {
    for (const subscriber of this.subscribers.get(frameSessionId(frame)) ?? []) subscriber.onFrame(frame);
  }

  private routeFrame(frame: ChatFrame): void {
    const sessionId = frameSessionId(frame);
    const refusal = frame.result === "chat_failed" ? frame.data : null;
    if (refusal?.code === MODEL_AUTH_FAILED) {
      const turn = this.activeTurns.get(sessionId);
      if (turn && !turn.authRetried && !turn.stopped) {
        // Still the same turn as far as the page is concerned: it stays busy
        // and hears nothing unless the second attempt fails too.
        turn.authRetried = true;
        void this.retryWithNewCredential(sessionId, turn, frame);
        return;
      }
    }
    // Release the session before the page hears about the turn's end: it
    // refetches sessions on that event and must not read a stale turn.
    if (isTerminalFrame(frame)) this.activeTurns.delete(sessionId);
    if (refusal?.code === MODEL_CREDIT_EXHAUSTED) {
      void this.failWithNote(frame, creditNote(refusal.detail, this.bridge.topUpUrl()));
    } else if (refusal?.code === MODEL_AUTH_FAILED) {
      void this.failWithNote(frame, SIGN_IN_AGAIN);
    } else {
      this.deliver(frame);
    }
  }

  /**
   * Put `note` in the transcript, then let the page hear that the turn failed.
   * In that order because the page refetches history on the failure event, and
   * history is the only place it reads the reason from.
   */
  private async failWithNote(frame: ChatFrame, note: string): Promise<void> {
    await this.bridge
      .invoke<StoredEvent>("chat_append_event", {
        account: this.bridge.getAccount(),
        sessionId: frameSessionId(frame),
        event: { role: "system", content: note },
      })
      .catch(() => {});
    this.deliver(frame);
  }

  /**
   * The gateway refused the engine's credential: mint a new one and submit the
   * turn again, once. The user's message is already in history. If no new
   * credential can be had, the turn ends with the reason and a request to sign
   * in — in the transcript.
   */
  private async retryWithNewCredential(sessionId: string, turn: ActiveTurn, refused: ChatFrame): Promise<void> {
    let minted = false;
    try {
      await this.bridge.refreshModelCredential();
      minted = true;
      // Closed, or failed over the transport, while the credential was minted.
      if (this.activeTurns.get(sessionId) !== turn) return;
      if (turn.stopped) {
        // Stopped while the credential was minted: the daemon had no turn to
        // cancel, so end it here instead of starting it over.
        this.activeTurns.delete(sessionId);
        this.deliver(refused);
        return;
      }
      const session = await this.bridge.invoke<StoredSession | null>("chat_get_session", {
        account: this.bridge.getAccount(),
        sessionId,
      });
      turn.messageId = crypto.randomUUID();
      await this.submitToDaemon(sessionId, turn.messageId, turn.content, session);
    } catch (error) {
      if (this.activeTurns.get(sessionId) !== turn) return;
      this.activeTurns.delete(sessionId);
      const reason = error instanceof Error ? error.message : "";
      // Without a new credential the refused one is all there is, so the way
      // out is to sign in, whatever kept the new one from being minted.
      const note = minted ? reason || SIGN_IN_AGAIN : [reason, SIGN_IN_AGAIN].filter(Boolean).join("\n");
      await this.failWithNote(refused, note);
    }
  }

  private toChatSession(s: StoredSession, account: string): IAgentChatSession {
    return storedSessionToChatSession(s, account, this.activeTurns.get(s.id)?.messageId ?? null);
  }

  // No `runner` selector is sent with any chat request. The daemon's selector
  // is a *runner name* from its own config (`resolve_runner`, runner/src/ipc/
  // server.rs); the id this transport knows is the synthetic route id
  // `pidash-builtin`, which no daemon is configured under — passing it fails
  // every request with `no runner named "pidash-builtin"`. Omitting it lets
  // the daemon resolve its single managed runner, which is what the desktop
  // bundle always hosts.

  private account(): string {
    const account = this.bridge.getAccount();
    if (!account) throw new Error("Local chat requires a signed-in account.");
    return account;
  }

  async listChatSessions(workspaceId: string, runnerId?: string): Promise<IAgentChatSession[]> {
    const account = this.account();
    const rows = await this.bridge.invoke<StoredSession[]>("chat_list_sessions", { account });
    return rows
      .filter((s) => (workspaceId ? s.workspace === workspaceId : true))
      .filter((s) => (runnerId ? s.project === runnerId : true))
      .map((s) => this.toChatSession(s, account));
  }

  async createChatSession(input: {
    workspace: string;
    runner: string;
    model?: string;
    cwd?: string;
  }): Promise<IAgentChatSession> {
    const account = this.account();
    const row = await this.bridge.invoke<StoredSession>("chat_create_session", {
      account,
      session: {
        title: "",
        workspace: input.workspace,
        // Slice-3 has no runner column; the runner id rides in `project`.
        project: input.runner,
        engine_version: "",
      },
    });
    return this.toChatSession(row, account);
  }

  async listChatMessages(sessionId: string): Promise<IAgentChatMessage[]> {
    const account = this.account();
    const rows = await this.bridge.invoke<StoredEvent[]>("chat_list_events", {
      account,
      sessionId,
    });
    return rows.map(storedEventToMessage);
  }

  async sendChatMessage(sessionId: string, content: string): Promise<IAgentChatMessage> {
    const account = this.account();
    // One turn at a time per session: the daemon refuses an overlapping send,
    // so don't persist a message that can never get a reply.
    if (this.activeTurns.has(sessionId)) {
      throw new Error("A response is still in progress. Wait for it to finish, or stop it first.");
    }
    const turn: ActiveTurn = { messageId: crypto.randomUUID(), content, authRetried: false };
    this.listenToHost();
    this.activeTurns.set(sessionId, turn);
    try {
      return await this.submitTurn(account, sessionId, turn.messageId, content);
    } catch (error) {
      // Nothing was submitted, so no terminal frame will ever release it.
      if (this.activeTurns.get(sessionId) === turn) this.activeTurns.delete(sessionId);
      throw error;
    }
  }

  private async submitTurn(
    account: string,
    sessionId: string,
    messageId: string,
    content: string
  ): Promise<IAgentChatMessage> {
    // Persist the user's turn first so a refresh/restart shows it even if the
    // stream never completes.
    const stored = await this.bridge.invoke<StoredEvent>("chat_append_event", {
      account,
      sessionId,
      event: { role: "user", content },
    });
    const session = await this.bridge.invoke<StoredSession | null>("chat_get_session", {
      account,
      sessionId,
    });
    // The daemon owns the engine, so it has to be up before the turn is
    // submitted. Idempotent once it is.
    await this.bridge.ensureRuntime();
    await this.submitToDaemon(sessionId, messageId, content, session);
    return storedEventToMessage(stored);
  }

  /**
   * Hand a turn to the daemon. Streaming happens over `chat://frame`; the
   * command returns once the turn has been submitted.
   */
  private async submitToDaemon(
    sessionId: string,
    messageId: string,
    content: string,
    session: StoredSession | null
  ): Promise<void> {
    await this.bridge.invoke<void>("chat_send", {
      workspace: this.bridge.workspaceSlug(),
      chatSessionId: sessionId,
      messageId,
      content,
      cwd: session?.working_dir,
      mode: this.getApprovalMode(sessionId),
      localThreadId: session?.engine_thread_id ?? undefined,
    });
  }

  async warmChatSession(sessionId: string): Promise<{ ok: boolean; skipped?: string }> {
    const account = this.account();
    const session = await this.bridge.invoke<StoredSession | null>("chat_get_session", {
      account,
      sessionId,
    });
    await this.bridge.ensureRuntime();
    await this.bridge.invoke<void>("chat_warm", {
      workspace: this.bridge.workspaceSlug(),
      chatSessionId: sessionId,
      cwd: session?.working_dir,
      mode: this.getApprovalMode(sessionId),
      localThreadId: session?.engine_thread_id ?? undefined,
    });
    return { ok: true };
  }

  async cancelChat(sessionId: string, reason?: string): Promise<{ ok: boolean }> {
    // No session lookup: cancel carries only the session id now that the
    // runner selector is gone, and the daemon resolves its own runner.
    const turn = this.activeTurns.get(sessionId);
    if (turn) turn.stopped = true;
    await this.bridge.invoke<void>("chat_cancel", {
      workspace: this.bridge.workspaceSlug(),
      chatSessionId: sessionId,
      reason,
    });
    return { ok: true };
  }

  async closeChat(sessionId: string): Promise<IAgentChatSession> {
    const account = this.account();
    const session = await this.bridge.invoke<StoredSession | null>("chat_get_session", {
      account,
      sessionId,
    });
    await this.bridge.invoke<void>("chat_close", {
      workspace: this.bridge.workspaceSlug(),
      chatSessionId: sessionId,
    });
    // Closing aborts the host's reader, so a turn it cut short sends no
    // terminal frame to release the session.
    this.activeTurns.delete(sessionId);
    // Slice-3 has no closed state; return the session as-is so the UI keeps the
    // transcript readable.
    const refreshed =
      session ?? (await this.bridge.invoke<StoredSession | null>("chat_get_session", { account, sessionId }));
    if (!refreshed) throw new Error(`no local chat session ${sessionId}`);
    return this.toChatSession(refreshed, account);
  }

  /**
   * Answer a pending chat approval. Not part of the shared `ChatTransport`
   * interface (the cloud path decides approvals through its own service), so
   * this is an additive local-only verb the desktop approval UI calls.
   */
  async decideChatApproval(sessionId: string, localApprovalId: string, decision: TApprovalDecision): Promise<void> {
    await this.bridge.invoke<void>("chat_decide", {
      workspace: this.bridge.workspaceSlug(),
      chatSessionId: sessionId,
      localApprovalId,
      decision,
    });
  }

  subscribeChatEvents(
    sessionId: string,
    _after: number,
    onEvent: ChatEventHandler,
    onError?: ChatEventErrorHandler
  ): ChatEventUnsubscribe {
    const account = this.account();
    const translator = new ChatFrameTranslator(sessionId);
    // Accumulate streamed assistant text per turn so a completed frame whose
    // `assistant_message` is empty (the engine's done payload didn't carry text
    // under a key the daemon recognises) can still be persisted from the deltas.
    let assistantAccum = "";
    const subscriber: FrameSubscriber = {
      onFrame: (frame) => {
        if (frame.result === "chat_message_started") {
          assistantAccum = "";
        } else if (frame.result === "chat_event" && frame.data.kind === "assistant_delta") {
          assistantAccum += assistantDeltaText(frame.data.payload);
        }
        if (frame.result === "chat_message_completed") {
          // Persist the assistant reply (falling back to the accumulated
          // deltas) *before* dispatching `turn_completed`, because the page
          // refetches history on that event and replaces the streamed reply
          // with it — if the reply isn't in history yet it vanishes.
          const accum = assistantAccum;
          void (async () => {
            await this.persistFromFrame(account, sessionId, frame, accum).catch(() => {});
            const event = translator.translate(frame);
            if (event) onEvent(event);
          })();
          return;
        }
        // Persist the durable parts of the turn as they stream.
        void this.persistFromFrame(account, sessionId, frame, "").catch(() => {});
        const event = translator.translate(frame);
        if (event) onEvent(event);
      },
      onError: (error) => onError?.(error),
    };
    this.listenToHost();
    const subscribers = this.subscribers.get(sessionId) ?? new Set<FrameSubscriber>();
    this.subscribers.set(sessionId, subscribers.add(subscriber));

    return () => {
      subscribers.delete(subscriber);
      if (!subscribers.size && this.subscribers.get(sessionId) === subscribers) this.subscribers.delete(sessionId);
    };
  }

  /**
   * Write the durable parts of a streamed turn to local history: the engine
   * thread id (for resume) when the thread starts, and the assistant's message
   * text when the turn completes. The completed frame's `assistant_message` is
   * used when present, else `fallbackAssistant` (the accumulated deltas) — so a
   * reply the engine streamed but didn't echo in its done payload is never lost.
   */
  private async persistFromFrame(
    account: string,
    sessionId: string,
    frame: ChatFrame,
    fallbackAssistant: string
  ): Promise<void> {
    if (frame.result === "chat_started" && frame.data.local_thread_id) {
      await this.bridge.invoke<void>("chat_set_thread_id", {
        account,
        sessionId,
        engineThreadId: frame.data.local_thread_id,
      });
    } else if (frame.result === "chat_message_completed") {
      const content = frame.data.assistant_message?.trim() ? frame.data.assistant_message : fallbackAssistant;
      if (content) {
        await this.bridge.invoke<StoredEvent>("chat_append_event", {
          account,
          sessionId,
          event: { role: "assistant", content },
        });
      }
    }
  }
}

// --- Production wiring ------------------------------------------------------

interface TauriGlobal {
  core: { invoke<T>(command: string, args?: Record<string, unknown>): Promise<T> };
  event: {
    listen<T>(event: string, handler: (e: { payload: T }) => void): Promise<() => void>;
  };
}

function tauriBridge(): TauriBridge {
  const tauri = (window as unknown as { __TAURI__: TauriGlobal }).__TAURI__;
  return {
    invoke: (command, args) => tauri.core.invoke(command, args),
    listen: (event, handler) => tauri.event.listen(event, (e) => handler(e.payload)),
    getAccount: () => getAgentAccount(),
    // The workspace slug is the first path segment of every in-app route
    // (`/:workspaceSlug/...`); the chat page has no other handle on it, and
    // enrolment is keyed by slug rather than by the workspace id the chat
    // session stores.
    ensureRuntime: () => ensureChatRuntime(workspaceSlugFromLocation()),
    workspaceSlug: workspaceSlugFromLocation,
    refreshModelCredential: () => refreshModelCredential(),
    topUpUrl: () => getModelTopUpUrl(),
  };
}

/**
 * The workspace slug is the first path segment of every in-app route
 * (`/:workspaceSlug/...`). The chat session stores the workspace *id*, which
 * is not what the daemon's per-workspace tree is keyed by.
 */
function workspaceSlugFromLocation(): string {
  return decodeURIComponent(window.location.pathname.split("/")[1] ?? "");
}

/** A synthetic runner so the shared chat page renders the built-in engine. */
function builtinRunner(): IRunner {
  return {
    id: BUILTIN_RUNNER_ID,
    name: "Built-in agent",
    status: "online",
  } as IRunner;
}

let registered = false;

/**
 * Register the local chat transport and a runner-detail shim for the built-in
 * agent id. Desktop-only and idempotent (registering twice would chain the
 * runner-detail fetcher onto itself); a no-op off the desktop app.
 */
export function registerLocalChatTransport(): void {
  if (!isDesktop() || registered) return;
  registered = true;
  setChatTransport(new LocalChatTransport(tauriBridge()));
  const previous: RunnerDetailFetcher = getRunnerDetailFetcher();
  setRunnerDetailFetcher((runnerId) =>
    runnerId === BUILTIN_RUNNER_ID ? Promise.resolve(builtinRunner()) : previous(runnerId)
  );
}
