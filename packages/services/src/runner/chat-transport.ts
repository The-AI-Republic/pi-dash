/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

/**
 * Pluggable transport for the runner **chat** surface.
 *
 * Most callers (cloud web app, self-hosted, browser tests) get the
 * HTTP+SSE default — built on `RunnerService` for the request/response
 * verbs and a browser `EventSource` for the streamed event feed — which
 * hits the same `/api/runners/chat/...` endpoints as before. Nothing in
 * their behavior changes.
 *
 * The reason this seam exists: the Pi Dash desktop app bundles a
 * built-in agent engine (run by a same-machine `pidash` daemon) and we
 * want a direct chat mode where the traffic goes desktop UI → Tauri host
 * → local engine and back, never relayed through the Pi Dash chat server
 * (PDASHOSS01-159). The desktop wrapper registers a local transport at
 * app startup via `setChatTransport`; the shared chat UI keeps calling
 * `getChatTransport()` and is agnostic about which path served the
 * request. The local implementation (Tauri `invoke` + Tauri events)
 * lands with the desktop chat-transport slice; until then the cloud
 * default is the only registered transport, exactly as before.
 *
 * Why ONE cohesive interface here, when the sibling `runner-transport.ts`
 * seam deliberately prefers per-verb function registries: that guidance
 * is about not collapsing *unrelated* surfaces (runner detail, chat,
 * approvals, …) into a single opaque handler bag. Chat is a *single*
 * cohesive surface — a session lifecycle (create / list / warm / send /
 * cancel / close) plus one streamed event subscription — and it has two
 * genuinely different backend implementations: cloud (HTTP + SSE) and
 * local (Tauri `invoke` + Tauri events). The event subscription in
 * particular has no per-verb-function analogue (SSE vs a native event
 * channel), so an interface an implementer fills in wholesale is easier
 * to reason about than eight independent setters plus a stream setter.
 *
 * Why a module-level registry rather than DI: identical reasoning to
 * `runner-transport.ts` — the OSS UI is loaded by Vite (browser-only),
 * there is no shared root a native shell can decorate before the React
 * tree mounts, and a module-level `set...` call from the shell's init
 * script is the lowest-friction way to flip transport before the first
 * UI mount.
 *
 * Scope: browser-side consumption only. Register the override before the
 * first UI mount; SWR (and similar) cache by key, so a swap after a fetch
 * has populated the cache will not invalidate that entry — follow a
 * mid-session swap with an explicit `mutate(...)` for the affected keys.
 */

import type { IAgentChatEvent, IAgentChatMessage, IAgentChatSession } from "@pi-dash/types";

import { RunnerService } from "./runner.service";

/**
 * Handler invoked once per streamed chat event, in arrival order.
 */
export type ChatEventHandler = (event: IAgentChatEvent) => void;

/**
 * Handler invoked when the event stream fails to parse a frame or the
 * underlying channel errors. The argument shape is transport-specific
 * (an SSE `Event`, a parse error, …) and callers treat it as opaque.
 */
export type ChatEventErrorHandler = (error: unknown) => void;

/**
 * Unsubscribe from a chat event stream. Idempotent; safe to call from a
 * React effect cleanup.
 */
export type ChatEventUnsubscribe = () => void;

/**
 * The runner chat surface, abstracted over its transport. The cloud
 * default (`cloudChatTransport`) speaks HTTP + SSE; the desktop app
 * registers a local implementation that speaks Tauri `invoke` + Tauri
 * events. Method contracts (arguments, resolved shapes, and the
 * response-data-shaped rejections) mirror `RunnerService`'s chat methods
 * so consumers are transport-agnostic.
 */
export interface ChatTransport {
  listChatSessions(workspaceId: string, runnerId?: string): Promise<IAgentChatSession[]>;
  createChatSession(input: {
    workspace: string;
    runner: string;
    model?: string;
    cwd?: string;
  }): Promise<IAgentChatSession>;
  listChatMessages(sessionId: string): Promise<IAgentChatMessage[]>;
  sendChatMessage(sessionId: string, content: string): Promise<IAgentChatMessage>;
  warmChatSession(sessionId: string): Promise<{ ok: boolean; skipped?: string }>;
  cancelChat(sessionId: string, reason?: string): Promise<{ ok: boolean }>;
  closeChat(sessionId: string): Promise<IAgentChatSession>;
  /**
   * Subscribe to the session's streamed events, delivering each parsed
   * event to `onEvent` in arrival order. `after` resumes from a prior
   * sequence number (0 = from the start). Returns an unsubscribe that
   * tears the stream down.
   */
  subscribeChatEvents(
    sessionId: string,
    after: number,
    onEvent: ChatEventHandler,
    onError?: ChatEventErrorHandler
  ): ChatEventUnsubscribe;
}

/**
 * Cloud transport — the default. The request/response verbs delegate to
 * `RunnerService` (session cookie auth, `/api/runners/chat/...`); the
 * event feed is a browser `EventSource` over the same-origin SSE
 * endpoint. This is byte-for-byte the behavior the chat UI had before the
 * seam existed.
 *
 * Constructed eagerly at module load, mirroring `runner-transport.ts`:
 * `RunnerService`'s constructor reads `API_BASE_URL` and runs
 * `applyAxiosSetups`, so every axios interceptor registered up through
 * this module's evaluation is captured. Interceptors registered LATER
 * are not picked up by this singleton — register them before
 * `@pi-dash/services` is first imported.
 */
class CloudChatTransport implements ChatTransport {
  private readonly service = new RunnerService();

  listChatSessions(workspaceId: string, runnerId?: string): Promise<IAgentChatSession[]> {
    return this.service.listChatSessions(workspaceId, runnerId);
  }

  createChatSession(input: {
    workspace: string;
    runner: string;
    model?: string;
    cwd?: string;
  }): Promise<IAgentChatSession> {
    return this.service.createChatSession(input);
  }

  listChatMessages(sessionId: string): Promise<IAgentChatMessage[]> {
    return this.service.listChatMessages(sessionId);
  }

  sendChatMessage(sessionId: string, content: string): Promise<IAgentChatMessage> {
    return this.service.sendChatMessage(sessionId, content);
  }

  warmChatSession(sessionId: string): Promise<{ ok: boolean; skipped?: string }> {
    return this.service.warmChatSession(sessionId);
  }

  cancelChat(sessionId: string, reason?: string): Promise<{ ok: boolean }> {
    return this.service.cancelChat(sessionId, reason);
  }

  closeChat(sessionId: string): Promise<IAgentChatSession> {
    return this.service.closeChat(sessionId);
  }

  subscribeChatEvents(
    sessionId: string,
    after: number,
    onEvent: ChatEventHandler,
    onError?: ChatEventErrorHandler
  ): ChatEventUnsubscribe {
    // Same-origin SSE with credentials, matching the previous
    // `useAgentChatEvents` implementation exactly.
    const source = new EventSource(this.service.chatEventsUrl(sessionId, after), {
      withCredentials: true,
    });
    source.addEventListener("chat.event", (message) => {
      try {
        const event = JSON.parse((message as MessageEvent).data) as IAgentChatEvent;
        onEvent(event);
      } catch (error) {
        console.error("Failed to parse runner chat event", error);
        onError?.(error);
      }
    });
    source.addEventListener("error", (error) => {
      onError?.(error);
    });
    return () => source.close();
  }
}

/**
 * The cloud HTTP+SSE transport. Kept module-private (consumers reach it
 * only as the default via `getChatTransport()`) so override authors don't
 * accidentally bypass a registered local transport by importing it
 * directly.
 */
const cloudChatTransport: ChatTransport = new CloudChatTransport();

let activeTransport: ChatTransport = cloudChatTransport;

/**
 * The active chat transport. Chat UI should call this rather than
 * instantiating `RunnerService` (or constructing an `EventSource`)
 * directly, so non-HTTP environments can override transport without the
 * call site changing.
 */
export function getChatTransport(): ChatTransport {
  return activeTransport;
}

/**
 * Override the active chat transport. Pass `null` or `undefined` to reset
 * to the cloud default — convenient for test teardown.
 *
 * Intended for environment-specific bootstrap (the Tauri desktop wrapper
 * registering its local Tauri-IPC transport at boot). The OSS / cloud /
 * self-hosted web app never calls this.
 *
 * Register before the first UI mount. SWR caches by key only — swapping
 * transport after a fetch has populated the cache will not invalidate
 * that entry; follow a mid-session swap with an explicit `mutate(...)`.
 */
export function setChatTransport(transport: ChatTransport | null | undefined): void {
  activeTransport = transport ?? cloudChatTransport;
}

// Vite HMR: preserve the active transport across hot reloads in dev so a
// desktop shell that registered an override at boot doesn't silently lose
// it when a save triggers re-evaluation of this module. Mirrors the HMR
// handling in `runner-transport.ts`. `import.meta.hot` is a Vite-only
// no-op elsewhere; tsdown builds it out cleanly.
interface ViteHotContext {
  readonly data: { activeChatTransport?: ChatTransport };
  accept(cb: (mod: unknown) => void): void;
}

interface ImportMetaWithHot extends ImportMeta {
  hot?: ViteHotContext;
}

const _meta = import.meta as ImportMetaWithHot;
if (_meta.hot) {
  _meta.hot.data.activeChatTransport = activeTransport;
  _meta.hot.accept((mod) => {
    const previous = _meta.hot?.data.activeChatTransport;
    if (mod && previous) {
      (
        mod as {
          setChatTransport: typeof setChatTransport;
        }
      ).setChatTransport(previous);
    }
  });
}
