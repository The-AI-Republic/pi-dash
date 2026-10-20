// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Pluggable runner-chat transport (NEWFRONT-549 RUN-032, messaging verbs
// NEWFRONT-551 RUN-027/029/030/031). The chat page never talks to a
// stream or to the chat endpoints directly: it reaches both through this
// seam, whose web default speaks the session's HTTP verbs and its
// server-sent event feed through `core/platform`, so a desktop build can
// replace the whole surface at bootstrap with native calls that deliver
// frames in the same shape. The page code does not change.

import {
  cancelChat,
  type CancelChatResult,
  createChatSession,
  ChatEvent,
  CHAT_EVENT_STREAM_NAME,
  chatEventsUrl,
  closeChat,
  getRunner,
  listChatMessages,
  listChatSessions,
  sendChatMessage,
  warmChatSession,
  type ApiClient,
  type ChatMessage,
  type ChatSession,
  type Runner,
} from "@pidash/api-client";

import { getAppClient } from "../../../core/api/client.js";
import { platform, type EventStream, type StreamInit } from "../../../core/platform/index.js";

/** One parsed frame from a session's event feed. */
export type ChatEventFrame = ChatEvent;

export interface ChatEventHandlers {
  /** Called once per well-formed frame, in arrival order. */
  onEvent: (event: ChatEventFrame) => void;
  /** An unreadable frame or a broken channel; the caller treats it as opaque. */
  onError?: (error: unknown) => void;
}

/** What a subscriber may want to know beyond the frames themselves. */
export interface ChatSubscriptionHandlers extends ChatEventHandlers {
  /**
   * The feed finished on its own (the server ended the response or the
   * channel dropped). The page may open a new subscription resuming past
   * the last frame it applied; an explicit unsubscribe never calls it.
   */
  onEnd?: () => void;
}

/**
 * The chat surface abstracted over its transport. `after` resumes past an
 * already-seen sequence (0 starts from the server's stored beginning).
 * The returned function unsubscribes and is safe to call twice.
 */
export interface ChatTransport {
  subscribeChatEvents(sessionId: string, after: number, handlers: ChatSubscriptionHandlers): () => void;
  /** Send the next user turn; answers the stored row. */
  sendChatMessage(sessionId: string, content: string): Promise<ChatMessage>;
  /** Interrupt the running turn, if any. */
  cancelChat(sessionId: string, reason?: string): Promise<CancelChatResult>;
  /** Close the conversation; answers the updated session. */
  closeChat(sessionId: string): Promise<ChatSession>;
  /**
   * Record the approval mode the engine should capture for this session's
   * next thread (RUN-035). Desktop transports keep it in memory; the
   * cloud path has no approval mode, so it is absent there.
   */
  setApprovalMode?(sessionId: string, mode: string): void;
  /**
   * Record the mode for every thread this runner spawns next (RUN-035).
   * A conversation the page creates after it loaded its own choice still
   * warms under it.
   */
  setRunnerApprovalMode?(runnerId: string, mode: string): void;
  /** Answer one inline approval (RUN-034); desktop only. */
  decideChatApproval?(sessionId: string, localApprovalId: string, decision: string): Promise<void>;
  /**
   * Read and write verbs. The cloud implementations are `@pidash/api-client`
   * calls; a desktop transport answers them from the local store and
   * daemon for the bundled engine only, which is why they are optional
   * and why every one of them may decline a contact or conversation that
   * belongs to the server by answering `null`.
   */
  chatRunner?(runnerId: string): Promise<Runner | null>;
  listChatSessions?(workspaceId: string, runnerId: string): Promise<ChatSession[] | null>;
  createChatSession?(input: { workspace: string; runner: string }): Promise<ChatSession | null>;
  listChatMessages?(sessionId: string): Promise<ChatMessage[] | null>;
  warmChatSession?(sessionId: string): Promise<void>;
  /**
   * Replace a local conversation's title (DESK-009). Absent on the cloud
   * path: the server names its own conversations.
   */
  renameChatSession?(sessionId: string, title: string): Promise<void>;
  /** Remove a local conversation and its messages; idempotent. */
  deleteChatSession?(sessionId: string): Promise<void>;
  /** Wipe every conversation this account keeps on the machine. */
  clearChatHistory?(): Promise<void>;
  /**
   * Resolve the working copy a local conversation runs in (DESK-009).
   * Absent on the cloud path.
   */
  chatWorkingDir?(runnerId: string): Promise<string>;
  /** Whether this transport's store carries the conversation. */
  ownsChatSession?(sessionId: string): Promise<boolean>;
  /** Whether this contact is served by this transport rather than the server. */
  ownsRunner?(runnerId: string): boolean;
}

/** Opens an event stream; injectable so the parser can be tested without a server. */
export type StreamOpener = (url: string, init: StreamInit) => EventStream;

const noop = (): void => undefined;

function parseFrame(data: string): ChatEventFrame | Error {
  let payload: unknown;
  try {
    payload = JSON.parse(data);
  } catch {
    return new Error("Unreadable chat frame");
  }
  const parsed = ChatEvent.safeParse(payload);
  return parsed.success ? parsed.data : new Error("Chat frame did not match its contract");
}

/** A source of the API client and of raw event streams, for tests and the desktop build. */
export interface CloudChatDeps {
  getClient?: () => ApiClient;
  openStream?: StreamOpener;
}

/**
 * The web default: a credentialed server-sent event feed for one session
 * and the session's own write verbs. Frames arrive under the named
 * `chat.event` type; other frame types are heartbeats and ignored.
 */
export function createCloudChatTransport(deps: CloudChatDeps = {}): ChatTransport {
  const getClient = deps.getClient ?? getAppClient;
  const openStream: StreamOpener = deps.openStream ?? ((url, init) => platform.stream(url, init));
  return {
    subscribeChatEvents(sessionId, after, { onEvent, onError = noop, onEnd = noop }) {
      const feed = openStream(chatEventsUrl(getClient(), sessionId, after), {
        headers: { Accept: "text/event-stream" },
      });
      let closed = false;
      const drain = async (): Promise<void> => {
        try {
          for await (const message of feed.messages()) {
            if (closed) return;
            if (message.event !== CHAT_EVENT_STREAM_NAME) continue;
            const frame = parseFrame(message.data);
            if (frame instanceof Error) {
              onError(frame);
              continue;
            }
            onEvent(frame);
          }
          if (!closed) onEnd();
        } catch (error) {
          if (!closed) {
            onError(error);
            onEnd();
          }
        }
      };
      void drain();
      return () => {
        if (closed) return;
        closed = true;
        feed.close();
      };
    },
    sendChatMessage: (sessionId, content) => sendChatMessage(getClient(), sessionId, content),
    cancelChat: (sessionId, reason) => cancelChat(getClient(), sessionId, reason),
    closeChat: (sessionId) => closeChat(getClient(), sessionId),
  };
}

const cloudTransport = createCloudChatTransport();

let activeTransport: ChatTransport = cloudTransport;

/** The transport the chat page subscribes through. */
export function getChatTransport(): ChatTransport {
  return activeTransport;
}

const transportListeners = new Set<() => void>();

/** Replace the default (desktop bootstrap, tests); `null` restores the default. */
export function setChatTransport(transport: ChatTransport | null | undefined): void {
  activeTransport = transport ?? cloudTransport;
  for (const listener of transportListeners) listener();
}

/**
 * The transport for one contact. A desktop transport that ships the bundled
 * engine serves only that contact; every runner the server owns keeps the
 * cloud path. A transport that does not say which contacts it serves is
 * the surface itself (tests, and a whole-surface replacement).
 */
export function chatTransportForRunner(runnerId: string): ChatTransport {
  if (activeTransport.ownsRunner === undefined) return activeTransport;
  return activeTransport.ownsRunner(runnerId) ? activeTransport : cloudTransport;
}

/**
 * Called after the active transport is replaced or restored. A page that
 * mounted before the desktop bootstrap finished re-reads the seam so its
 * controls and verbs follow the transport that is actually installed.
 */
export function subscribeChatTransport(listener: () => void): () => void {
  transportListeners.add(listener);
  return () => {
    transportListeners.delete(listener);
  };
}

/**
 * Runner detail for the chat header. A transport that ships the bundled
 * engine answers for it; anything it declines or any other contact is a
 * server row and is read through the API, so installing the desktop seam
 * never breaks a cloud runner's page.
 */
export async function fetchChatRunner(client: ApiClient, runnerId: string): Promise<Runner> {
  const local = await activeTransport.chatRunner?.(runnerId);
  return local ?? getRunner(client, runnerId);
}

/** History list for one runner; the same decline rule applies. */
export async function fetchChatSessions(
  client: ApiClient,
  workspaceId: string,
  runnerId: string
): Promise<ChatSession[]> {
  const local = await activeTransport.listChatSessions?.(workspaceId, runnerId);
  return local ?? listChatSessions(client, { workspace: workspaceId, runner: runnerId });
}

/** Open a conversation against the runner; the same decline rule applies. */
export async function openChatSession(
  client: ApiClient,
  input: { workspace: string; runner: string }
): Promise<ChatSession> {
  const local = await activeTransport.createChatSession?.(input);
  return local ?? createChatSession(client, input);
}

/**
 * One conversation's saved transcript. A local transport knows whether the
 * session came from its store before attempting to read a transcript that
 * the server would answer 404 for.
 */
export async function fetchChatMessages(client: ApiClient, sessionId: string): Promise<ChatMessage[]> {
  const owned = activeTransport.ownsChatSession === undefined || (await activeTransport.ownsChatSession(sessionId));
  if (!owned) return listChatMessages(client, sessionId);
  const local = await activeTransport.listChatMessages?.(sessionId);
  return local ?? listChatMessages(client, sessionId);
}

/**
 * Rename one conversation. A store that does not carry it (a server queue
 * conversation, or a plan that offers no rename) answers `false` so the
 * caller can report that nothing changed rather than claim success.
 */
export async function renameChatSession(sessionId: string, title: string): Promise<boolean> {
  if (activeTransport.renameChatSession === undefined) return false;
  await activeTransport.renameChatSession(sessionId, title);
  return true;
}

/**
 * Delete one conversation and its messages. Idempotent by contract, so a
 * double click or a stale row settles quietly.
 */
export async function deleteChatSession(sessionId: string): Promise<boolean> {
  if (activeTransport.deleteChatSession === undefined) return false;
  await activeTransport.deleteChatSession(sessionId);
  return true;
}

/** Wipe the on-machine history for the signed-in account, when one exists. */
export async function clearChatHistory(): Promise<boolean> {
  if (activeTransport.clearChatHistory === undefined) return false;
  await activeTransport.clearChatHistory();
  return true;
}

/**
 * The working copy a local chat runs in. `null` when the transport has no
 * working copy of its own (cloud, or a server contact on the desktop), so
 * the header keeps whatever the session row already carried.
 */
export async function fetchChatWorkingDir(runnerId: string): Promise<string | null> {
  if (activeTransport.chatWorkingDir === undefined) return null;
  if (activeTransport.ownsRunner !== undefined && !activeTransport.ownsRunner(runnerId)) return null;
  const dir = await activeTransport.chatWorkingDir(runnerId);
  return dir === "" ? null : dir;
}

/**
 * Prepare a session so its first turn streams promptly (RUN-028). A local
 * transport warms only the conversations its own store carries; a session
 * the server owns keeps the warm endpoint even while the desktop seam is
 * installed, exactly like the other read verbs.
 */
export async function prepareChatSession(client: ApiClient, sessionId: string): Promise<void> {
  const owned = activeTransport.ownsChatSession === undefined || (await activeTransport.ownsChatSession(sessionId));
  if (owned && activeTransport.warmChatSession) {
    await activeTransport.warmChatSession(sessionId);
    return;
  }
  await warmChatSession(client, sessionId);
}
