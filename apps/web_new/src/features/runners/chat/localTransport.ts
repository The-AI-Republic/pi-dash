// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Chat against the bundled local engine (NEWFRONT-553, RUN-033-036). The
// engine runs on this machine and is reached through native IPC; the
// transcript comes from a per-account local store, and the engine thread
// id is written back so a conversation resumes across restarts. Frames
// from the engine are folded into the same event shape the cloud feed
// delivers, so the chat page renders either path unchanged.
//
// Registered only when the platform exposes the local chat capability:
// the web and cloud bundles never construct this transport.

import type { ChatEvent, ChatMessage, ChatSession, Runner, RunnerStatus } from "@pidash/api-client";

import type { LocalChatApi, LocalChatDecision, LocalChatFrame } from "../../../core/platform/index.js";
import type { ChatSubscriptionHandlers, ChatTransport } from "./transport.js";

/** Route id of the bundled engine; it is not a cloud runner id. */
export const BUILTIN_CHAT_ID = "pidash-builtin";

/** Engine posture when no mode was chosen: no prompts. */
export const DEFAULT_APPROVAL_MODE = "full_access";

/** Error a second concurrent turn gets, before the daemon is even asked. */
export const TURN_BUSY_MESSAGE = "A reply is still streaming. Wait for it to finish or stop it first.";

/** A conversation title is what says which chat this is; blank is refused. */
export const CHAT_TITLE_REQUIRED_MESSAGE = "Give the conversation a name first.";

/** A rename or working-copy request for something this store does not carry. */
export const CHAT_SESSION_GONE_MESSAGE = "This conversation is no longer on this machine.";

/** The store counts unix milliseconds; the contract wants ISO-8601. */
function isoFromMillis(millis: number): string {
  return new Date(millis).toISOString();
}

/** Broadest engine roles map onto the transcript vocabulary the UI knows. */
export function chatRoleFor(role: string): ChatMessage["role"] {
  return role === "user" || role === "assistant" || role === "tool" ? role : "system";
}

/** Whether a frame ends the current turn for its session. */
export function endsTurn(frame: LocalChatFrame): boolean {
  return frame.result === "chat_message_completed" || frame.result === "chat_failed" || frame.result === "chat_closed";
}

/** The session a frame belongs to. */
export function frameSession(frame: LocalChatFrame): string {
  return frame.data.chat_session_id;
}

/**
 * Assistant text carried by a delta frame. The daemon nests engine payloads
 * as `{ params: { delta | text } }`; both spellings and a plain string delta
 * are accepted, because engine versions differ.
 */
export function deltaText(payload: Record<string, unknown>): string {
  const params = payload.params;
  const scope =
    params !== null && typeof params === "object" && !Array.isArray(params)
      ? (params as Record<string, unknown>)
      : payload;
  const delta = scope.delta;
  if (typeof delta === "string") return delta;
  if (delta !== null && typeof delta === "object" && !Array.isArray(delta)) {
    const text = (delta as Record<string, unknown>).text;
    if (typeof text === "string") return text;
  }
  const text = scope.text;
  return typeof text === "string" ? text : "";
}

/** One translated event, numbered from 1 for the subscription. */
export interface TranslatedFrame {
  event: ChatEvent;
}

/**
 * Turn one daemon frame into the shared event shape. Stateful: it counts
 * the sequence the page dedupes on and remembers which assistant message
 * the current deltas belong to.
 */
export function createFrameFolder(sessionId: string): (frame: LocalChatFrame) => ChatEvent | null {
  let seq = 0;
  let openMessage: string | null = null;
  return (frame) => {
    seq += 1;
    const event = (kind: string, message: string | null, payload: Record<string, unknown>): ChatEvent => ({
      id: seq,
      session: sessionId,
      message,
      seq,
      kind,
      payload,
      created_at: new Date().toISOString(),
    });
    switch (frame.result) {
      case "chat_started":
        return event("turn_started", null, {
          local_thread_id: frame.data.local_thread_id,
          local_session_id: frame.data.local_session_id ?? "",
        });
      case "chat_message_started":
        openMessage = frame.data.message_id;
        return event("message_started", frame.data.message_id, { turn_id: frame.data.turn_id ?? "" });
      case "chat_event":
        return event(frame.data.kind, frame.data.kind === "assistant_delta" ? openMessage : null, frame.data.payload);
      case "chat_approval_request":
        return event("chat_approval_request", null, {
          local_approval_id: frame.data.local_approval_id,
          approval_kind: frame.data.kind,
          reason: frame.data.reason ?? "",
          expires_at: frame.data.expires_at ?? null,
          ...frame.data.payload,
        });
      case "chat_message_completed": {
        const message = frame.data.message_id;
        openMessage = null;
        return event("turn_completed", message, {
          status: frame.data.status,
          assistant_message: frame.data.assistant_message ?? "",
          turn_id: frame.data.turn_id ?? "",
        });
      }
      case "chat_failed":
        return event("chat_failed", null, { code: frame.data.code, detail: frame.data.detail ?? "" });
      case "chat_closed":
        return event("chat_closed", null, {});
      default:
        return null;
    }
  };
}

/**
 * A conversation row with the store's own title (DESK-009). The server
 * contract names conversations by time, so the local title rides beside it
 * as an optional field the history column prefers when present.
 */
export type LocalChatSessionView = ChatSession & { local_title?: string };

/** A stored conversation, seen as the chat page's session row. */
export function localSessionRow(
  record: {
    id: string;
    title: string;
    workspace: string;
    project: string;
    working_dir: string;
    engine_thread_id: string | null;
    created_at: number;
    updated_at: number;
  },
  account: string,
  activeMessageId: string | null
): LocalChatSessionView {
  return {
    id: record.id,
    local_title: record.title,
    workspace: record.workspace,
    runner: record.project,
    runner_detail: { id: record.project, name: "", status: "online" },
    created_by: account,
    pod: "",
    status: "open",
    agent_kind: "managed_runner",
    local_thread_id: record.engine_thread_id ?? "",
    local_session_id: "",
    cwd: record.working_dir,
    model: "",
    active_turn_id: "",
    active_message_id: activeMessageId,
    close_requested: false,
    last_message_at: record.updated_at > record.created_at ? isoFromMillis(record.updated_at) : null,
    closed_at: null,
    error: "",
    created_at: isoFromMillis(record.created_at),
    updated_at: isoFromMillis(record.updated_at),
  };
}

/** A stored transcript row, seen as a chat message. */
export function localMessageRow(record: {
  id: string;
  session_id: string;
  seq: number;
  role: string;
  content: string;
  created_at: number;
}): ChatMessage {
  return {
    id: record.id,
    session: record.session_id,
    role: chatRoleFor(record.role),
    content: record.content,
    content_parts: [],
    status: "completed",
    local_item_id: "",
    local_turn_id: "",
    seq: record.seq,
    created_at: isoFromMillis(record.created_at),
    completed_at: isoFromMillis(record.created_at),
  };
}

/** The runner row the header renders for the bundled engine. */
export function builtinRunnerRow(): Runner {
  const status: RunnerStatus = "online";
  return {
    id: BUILTIN_CHAT_ID,
    name: "Built-in agent",
    status,
    host_label: "",
    provisioning: "manual",
    os: "",
    arch: "",
    runner_version: "",
    dev_metadata: {},
    protocol_version: 0,
    capabilities: [],
    last_heartbeat_at: null,
    owner: "",
    dev_machine: null,
    dev_machine_detail: null,
    visibility: 0,
    pod: "",
    pod_detail: { id: "", name: "", is_default: false, project: "", project_identifier: "" },
    live_state: null,
    // The bundled engine is always enrolled; a null would read as offline
    // to the status helper and shut the composer.
    enrolled_at: isoFromMillis(0),
    revoked_at: null,
    revoked_reason: "",
    created_at: isoFromMillis(0),
    updated_at: isoFromMillis(0),
  };
}

export interface LocalTransportDeps {
  api: LocalChatApi;
  /** Signed-in account id; the local store is keyed by it. */
  account: () => string;
  /** Workspace slug whose daemon serves the commands. */
  workspaceSlug: () => string;
  /** Bring the daemon up before the first turn; idempotent. */
  ensureRuntime: () => Promise<void>;
  /**
   * The mode last stored for a runner. Consulted when the page created a
   * conversation before it loaded its own choice, so the thread still
   * warms under what the person picked last time.
   */
  storedMode: (runnerId: string) => string;
}

/** The transport the desktop bootstrap installs in place of the cloud one. */
export interface LocalChatTransport extends ChatTransport {
  setApprovalMode(sessionId: string, mode: string): void;
  getApprovalMode(sessionId: string): string;
  setRunnerApprovalMode(runnerId: string, mode: string): void;
  decideChatApproval(sessionId: string, localApprovalId: string, decision: string): Promise<void>;
  /** Rename a stored conversation; refuses one this store does not carry. */
  renameChatSession(sessionId: string, title: string): Promise<void>;
  /** Delete a stored conversation and its messages; idempotent. */
  deleteChatSession(sessionId: string): Promise<void>;
  /** Wipe the signed-in account's whole on-machine history. */
  clearChatHistory(): Promise<void>;
  /** The working copy the bundled engine runs a chat in, under the sessions' workspace id. */
  chatWorkingDir(workspaceId: string, runnerId: string): Promise<string>;
  /** The bundled engine's row; every other contact belongs to the server. */
  chatRunner(runnerId: string): Promise<Runner | null>;
  ownsChatSession(sessionId: string): Promise<boolean>;
  ownsRunner(runnerId: string): boolean;
}

export function createLocalChatTransport(deps: LocalTransportDeps): LocalChatTransport {
  const { api } = deps;
  /** Mode per runner, chosen by the page. */
  const runnerModes = new Map<string, string>();
  /** Mode per session, for a page that hands it to a specific conversation. */
  const sessionModes = new Map<string, string>();
  /** Session id to the daemon message id of its in-flight turn. */
  const activeTurns = new Map<string, string>();
  const subscribers = new Map<string, (frame: LocalChatFrame) => void>();
  const errorSubscribers = new Map<string, (message: string) => void>();
  let listening = false;

  const account = (): string => {
    const value = deps.account();
    if (value === "") throw new Error("Sign in to use the built-in agent.");
    return value;
  };

  /** Start one process-wide listener pair; frames route by session id. */
  const listenOnce = (): void => {
    if (listening) return;
    listening = true;
    api.subscribe({
      onFrame: (frame) => {
        if (endsTurn(frame)) activeTurns.delete(frameSession(frame));
        subscribers.get(frameSession(frame))?.(frame);
      },
      onError: (event) => {
        activeTurns.delete(event.chat_session_id);
        errorSubscribers.get(event.chat_session_id)?.(event.message);
      },
    });
  };

  /**
   * The mode a thread warmed or spawned now should capture. What the page
   * chose for this session wins, then the runner's choice, then the
   * runner's stored preference, which covers a conversation the page
   * created after it loaded its own choice. The engine default is last.
   */
  const modeFor = async (
    sessionId: string,
    runnerId: string | undefined,
    stored: (runnerId: string) => string
  ): Promise<string> => {
    return (
      sessionModes.get(sessionId) ??
      (runnerId === undefined ? undefined : (runnerModes.get(runnerId) ?? stored(runnerId))) ??
      DEFAULT_APPROVAL_MODE
    );
  };

  const readSession = async (sessionId: string) => api.getSession(account(), sessionId);

  /** A conversation this store carries belongs to the bundled engine. */
  const isLocalSession = async (sessionId: string): Promise<boolean> =>
    (await readSession(sessionId))?.project === BUILTIN_CHAT_ID;

  return {
    setApprovalMode: (sessionId, mode) => sessionModes.set(sessionId, mode),
    getApprovalMode: (sessionId) => sessionModes.get(sessionId) ?? DEFAULT_APPROVAL_MODE,
    setRunnerApprovalMode: (runnerId, mode) => runnerModes.set(runnerId, mode),
    chatRunner: (runnerId) => Promise.resolve(runnerId === BUILTIN_CHAT_ID ? builtinRunnerRow() : null),

    listChatSessions: async (workspaceId, runnerId) => {
      // Only the bundled engine is served from this machine; the history
      // of a server runner stays a server read.
      if (runnerId !== BUILTIN_CHAT_ID) return null;
      const owner = account();
      const rows = await api.listSessions(owner);
      return rows
        .filter((row) => row.workspace === workspaceId && row.project === runnerId)
        .map((row) => localSessionRow(row, owner, activeTurns.get(row.id) ?? null));
    },

    createChatSession: async (input) => {
      if (input.runner !== BUILTIN_CHAT_ID) return null;
      const owner = account();
      const record = await api.createSession(owner, { workspace: input.workspace, runner: input.runner });
      return localSessionRow(record, owner, null);
    },

    listChatMessages: async (sessionId) => {
      if (!(await isLocalSession(sessionId))) return null;
      const owner = account();
      const rows = await api.listEvents(owner, sessionId);
      return rows.map(localMessageRow);
    },

    ownsChatSession: isLocalSession,
    ownsRunner: (runnerId) => runnerId === BUILTIN_CHAT_ID,

    warmChatSession: async (sessionId) => {
      const record = await readSession(sessionId);
      if (record === null) return;
      await deps.ensureRuntime();
      await api.warm({
        workspaceSlug: deps.workspaceSlug(),
        sessionId,
        cwd: record.working_dir,
        mode: await modeFor(sessionId, record.project, deps.storedMode),
        localThreadId: record.engine_thread_id ?? undefined,
      });
    },

    sendChatMessage: async (sessionId, content) => {
      const owner = account();
      if (activeTurns.has(sessionId)) throw new Error(TURN_BUSY_MESSAGE);
      const messageId = crypto.randomUUID();
      listenOnce();
      activeTurns.set(sessionId, messageId);
      try {
        // The user's turn is stored before the engine hears about it, so a
        // refresh during the reply still shows what was asked.
        const stored = await api.appendEvent(owner, sessionId, { role: "user", content });
        const record = await readSession(sessionId);
        await deps.ensureRuntime();
        await api.send({
          workspaceSlug: deps.workspaceSlug(),
          sessionId,
          messageId,
          content,
          cwd: record?.working_dir,
          mode: await modeFor(sessionId, record?.project, deps.storedMode),
          localThreadId: record?.engine_thread_id ?? undefined,
        });
        return localMessageRow(stored);
      } catch (error) {
        // Nothing reached the engine, so no terminal frame will release it.
        if (activeTurns.get(sessionId) === messageId) activeTurns.delete(sessionId);
        throw error;
      }
    },

    cancelChat: async (sessionId, reason) => {
      await api.cancel({ workspaceSlug: deps.workspaceSlug(), sessionId, reason });
      return { ok: true };
    },

    closeChat: async (sessionId) => {
      const owner = account();
      const before = await readSession(sessionId);
      await api.close({ workspaceSlug: deps.workspaceSlug(), sessionId });
      // Closing aborts the reader, so a cut-short turn never sends its
      // terminal frame; release the session here instead.
      activeTurns.delete(sessionId);
      const record = before ?? (await readSession(sessionId));
      if (record === null) throw new Error("This conversation is no longer in the local store.");
      return localSessionRow(record, owner, null);
    },

    decideChatApproval: async (sessionId, localApprovalId, decision) => {
      await api.decide({
        workspaceSlug: deps.workspaceSlug(),
        sessionId,
        localApprovalId,
        decision: decision as LocalChatDecision,
      });
    },

    renameChatSession: async (sessionId, title) => {
      const owner = account();
      const trimmed = title.trim();
      if (trimmed === "") throw new Error(CHAT_TITLE_REQUIRED_MESSAGE);
      // A conversation this store does not own is a server row; renaming
      // it through the local store would fail with a store-level error
      // that says nothing useful to the person who clicked.
      if (!(await isLocalSession(sessionId))) throw new Error(CHAT_SESSION_GONE_MESSAGE);
      await api.renameSession(owner, sessionId, trimmed);
    },

    deleteChatSession: async (sessionId) => {
      const owner = account();
      await api.deleteSession(owner, sessionId);
      // The turn, if any, was cut off with the row; no terminal frame will
      // arrive for a conversation that no longer exists.
      activeTurns.delete(sessionId);
      subscribers.delete(sessionId);
      errorSubscribers.delete(sessionId);
    },

    clearChatHistory: async () => {
      await api.clearHistory(account());
      // Every conversation went with the history, so nothing is in flight.
      activeTurns.clear();
    },

    chatWorkingDir: async (workspaceId, runnerId) => {
      if (runnerId !== BUILTIN_CHAT_ID) throw new Error(CHAT_SESSION_GONE_MESSAGE);
      const owner = account();
      // The id, not the route slug: the machine resolved the chat's own
      // directory from it at create time, and the native read creates a
      // different directory for any other value.
      if (workspaceId === "") throw new Error("Open a workspace to see the built-in agent's working copy.");
      return api.workingDir(owner, workspaceId, runnerId);
    },

    subscribeChatEvents: (sessionId, _after, handlers: ChatSubscriptionHandlers) => {
      listenOnce();
      const folder = createFrameFolder(sessionId);
      let assistantText = "";
      const receive = (frame: LocalChatFrame): void => {
        if (frame.result === "chat_message_started") {
          assistantText = "";
        } else if (frame.result === "chat_event" && frame.data.kind === "assistant_delta") {
          assistantText += deltaText(frame.data.payload);
        }
        if (frame.result === "chat_message_completed") {
          // The reply is stored before the page re-reads history on the
          // terminal frame; otherwise the streamed bubble vanishes.
          const reply = frame.data.assistant_message?.trim() ? frame.data.assistant_message : assistantText;
          const completion = folder(frame);
          const stored =
            reply.trim() === ""
              ? Promise.resolve()
              : api.appendEvent(account(), sessionId, { role: "assistant", content: reply }).then(
                  () => undefined,
                  () => undefined
                );
          void stored.then(() => {
            if (completion !== null) handlers.onEvent(completion);
          });
          return;
        }
        if (frame.result === "chat_started" && frame.data.local_thread_id !== "") {
          void api.setThreadId(account(), sessionId, frame.data.local_thread_id).catch(() => undefined);
        }
        const event = folder(frame);
        if (event !== null) handlers.onEvent(event);
      };
      subscribers.set(sessionId, receive);
      errorSubscribers.set(sessionId, (message) => handlers.onError?.(new Error(message)));
      return () => {
        if (subscribers.get(sessionId) === receive) subscribers.delete(sessionId);
        errorSubscribers.delete(sessionId);
      };
    },
  };
}
