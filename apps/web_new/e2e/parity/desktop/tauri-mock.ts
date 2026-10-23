// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Mocked desktop shell for the desktop area gate (NEWFRONT-595). The
// parity browser has no Tauri runtime, so desktop-mode runs install this
// bridge before navigating: it advertises the native HTTP origin, answers
// the native commands the page's desktop platform sends (proxying API
// traffic to fetch with the same frame codec), and records every invoke
// for assertions. Native-only behavior (tray, window, zoom, navigation
// guard, launch prompt, daily scheduler) stays in desktop/src-tauri and
// is out of this mock's reach by design.
import type { Page } from "@playwright/test";

/** Scenario knobs for the mocked native shell. All optional. */
export interface DesktopMockConfig {
  /** managed_doctor answer. Default: both binaries present, host "parity-host". */
  doctor?: { runnerPresent: boolean; enginePresent: boolean; hostLabel: string };
  /** desktop_pending_update answer. Default: null (nothing waiting). */
  pendingUpdate?: { version: string; currentVersion: string; installing: boolean } | null;
  /** Make desktop_install_update reject. Default: false. */
  installFails?: boolean;
  /** detect_pidash_cli answer. Default: missing tool. */
  cli?: { installed: boolean; path: string | null; version: string | null };
  /** Lines the mocked installer streams on pidash-install-log. */
  installLogLines?: string[];
  /** Frames chat_send emits after the send resolves. Default: a completed echo turn. */
  chatScript?: Array<Record<string, unknown>>;
  /** Text of the default send-completion assistant message. */
  chatEcho?: string;
  /** chat_approval_modes_load answer. Default: {}. */
  approvalModes?: Record<string, string>;
}

/** One recorded native invoke. */
export interface DesktopInvocation {
  command: string;
  args: unknown;
}

type MockHandle = {
  invocations: DesktopInvocation[];
  emit: (event: string, payload: unknown) => void;
  configure: (patch: DesktopMockConfig) => void;
};

declare global {
  interface Window {
    __PIDASH_DESKTOP_MOCK__?: MockHandle;
  }
}

/**
 * Install the mocked desktop bridge. Call before the first navigation:
 * the init script runs on every subsequent page load. The mock records
 * invokes, proxies API traffic through fetch, and serves the lifecycle,
 * chat, history, updater, CLI, opener and deep-link commands from memory.
 */
export async function installDesktopMock(page: Page, config: DesktopMockConfig = {}): Promise<void> {
  await page.addInitScript((seed: DesktopMockConfig) => {
    const store = {
      config: seed as Required<DesktopMockConfig>,
      invocations: [] as DesktopInvocation[],
      listeners: new Map<string, Set<(message: { payload: unknown }) => void>>(),
      chatSessions: new Map<string, Record<string, unknown>>(),
      chatEvents: new Map<string, Record<string, unknown>[]>(),
      chatSeq: 0,
      installInFlight: false,
      apiControllers: new Map<string, AbortController>(),
    };
    store.config.doctor ??= { runnerPresent: true, enginePresent: true, hostLabel: "parity-host" };
    store.config.pendingUpdate ??= null;
    store.config.installFails ??= false;
    store.config.cli ??= { installed: false, path: null, version: null };
    store.config.installLogLines ??= [];
    store.config.chatScript ??= [];
    store.config.chatEcho ??= "mocked assistant reply";
    store.config.approvalModes ??= {};

    const encoder = new TextEncoder();
    const decoder = new TextDecoder();

    function encodeHead(head: unknown, body: Uint8Array | null): Uint8Array {
      const headBytes = encoder.encode(JSON.stringify(head));
      const out = new Uint8Array(4 + headBytes.length + (body?.length ?? 0));
      new DataView(out.buffer).setUint32(0, headBytes.length);
      out.set(headBytes, 4);
      if (body) out.set(body, 4 + headBytes.length);
      return out;
    }

    function decodeHead(bytes: Uint8Array): { head: Record<string, unknown>; body: Uint8Array } {
      const headLength = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength).getUint32(0);
      const head = JSON.parse(decoder.decode(bytes.subarray(4, 4 + headLength))) as Record<string, unknown>;
      return { head, body: bytes.subarray(4 + headLength) };
    }

    function asBytes(raw: unknown): Uint8Array {
      if (raw instanceof Uint8Array) return raw;
      if (raw instanceof ArrayBuffer) return new Uint8Array(raw);
      if (Array.isArray(raw)) return new Uint8Array(raw as number[]);
      throw new Error("mock desktop_api_request needs a framed body");
    }

    function emit(event: string, payload: unknown): void {
      for (const handler of store.listeners.get(event) ?? []) {
        try {
          handler({ payload });
        } catch {
          // A failing page handler must not break the mock bus.
        }
      }
    }

    function sessionKey(account: unknown, sessionId: unknown): string {
      return `${String(account)}:${String(sessionId)}`;
    }

    async function apiRequest(frame: Uint8Array): Promise<Uint8Array> {
      const { head, body } = decodeHead(frame);
      const id = String(head["id"] ?? "");
      const url = String(head["url"] ?? "");
      const method = String(head["method"] ?? "GET");
      const pairs = (head["headers"] as Array<[string, string]> | undefined) ?? [];
      const headers = new Headers();
      for (const [name, value] of pairs) {
        try {
          headers.append(name, value);
        } catch {
          // Skip headers the page cannot represent.
        }
      }
      const timeoutMs = typeof head["timeoutMs"] === "number" ? (head["timeoutMs"] as number) : null;
      const controller = new AbortController();
      if (id !== "") store.apiControllers.set(id, controller);
      let timer: ReturnType<typeof setTimeout> | undefined;
      if (timeoutMs !== null && timeoutMs > 0) {
        timer = setTimeout(() => controller.abort(new DOMException("mock timeout", "TimeoutError")), timeoutMs);
      }
      try {
        const response = await fetch(url, {
          method,
          headers,
          body: head["hasBody"] === true ? (body as unknown as BodyInit) : undefined,
          credentials: "include",
          signal: controller.signal,
        });
        const responseBytes = new Uint8Array(await response.arrayBuffer());
        const responseHeaders: Array<[string, string]> = [];
        response.headers.forEach((value, name) => responseHeaders.push([name, value]));
        return encodeHead(
          { status: response.status, statusText: response.statusText, headers: responseHeaders },
          responseBytes
        );
      } catch (error) {
        if (controller.signal.aborted) {
          const reason = controller.signal.reason;
          if (reason instanceof DOMException && reason.name === "TimeoutError") {
            throw new Error("API request timed out");
          }
          throw new Error("API request canceled");
        }
        throw error;
      } finally {
        if (timer !== undefined) clearTimeout(timer);
        if (id !== "") store.apiControllers.delete(id);
      }
    }

    async function apiStream(
      args: Record<string, unknown>,
      channel: { onmessage: ((message: unknown) => void) | null }
    ): Promise<void> {
      const request = (args["request"] as Record<string, unknown> | undefined) ?? {};
      const url = String(request["url"] ?? "");
      const pairs = (request["headers"] as Array<[string, string]> | undefined) ?? [];
      const headers = new Headers();
      for (const [name, value] of pairs) headers.append(name, value);
      const post = (message: unknown): void => {
        try {
          channel.onmessage?.(message);
        } catch {
          // A failing page handler must not break the mock bus.
        }
      };
      let response: Response;
      try {
        response = await fetch(url, {
          method: String(request["method"] ?? "GET"),
          headers,
          credentials: "include",
        });
      } catch {
        // No live service behind the parity stack: fail the stream for
        // good instead of rejecting into a reconnect loop.
        post({ type: "head", status: 503, statusText: "mocked stream unavailable", headers: [] });
        return;
      }
      const responseHeaders: Array<[string, string]> = [];
      response.headers.forEach((value, name) => responseHeaders.push([name, value]));
      post({ type: "head", status: response.status, statusText: response.statusText, headers: responseHeaders });
      if (!response.ok || !String(response.headers.get("content-type") ?? "").startsWith("text/event-stream")) {
        return;
      }
      const reader = response.body?.getReader();
      if (!reader) {
        post({ type: "end" });
        return;
      }
      try {
        for (;;) {
          const { done, value } = await reader.read();
          if (done) break;
          post({ type: "data", text: decoder.decode(value, { stream: true }) });
        }
        post({ type: "data", text: decoder.decode() });
        post({ type: "end" });
      } finally {
        reader.releaseLock();
      }
    }

    function chatRecord(args: Record<string, unknown>): { account: string; sessionId: string } {
      return { account: String(args["account"] ?? ""), sessionId: String(args["sessionId"] ?? "") };
    }

    async function invoke(command: string, args?: unknown): Promise<unknown> {
      store.invocations.push({ command, args });
      const params = (args ?? {}) as Record<string, unknown>;
      switch (command) {
        case "desktop_api_request":
          return apiRequest(asBytes(args));
        case "desktop_api_cancel": {
          const id = String(params["id"] ?? "");
          store.apiControllers.get(id)?.abort(new DOMException("mock cancel", "AbortError"));
          return null;
        }
        case "desktop_api_stream": {
          const channel = params["channel"] as { onmessage: ((message: unknown) => void) | null };
          await apiStream(params, channel);
          return null;
        }
        case "plugin:opener|open_url":
        case "plugin:window|set_title":
        case "desktop_clear_web_data":
          return null;
        case "managed_doctor": {
          const doctor = store.config.doctor;
          return {
            runner_present: doctor.runnerPresent,
            engine_present: doctor.enginePresent,
            host_label: doctor.hostLabel,
          };
        }
        case "managed_write_engine_config":
        case "managed_write_model_token":
        case "managed_bootstrap":
        case "managed_enroll":
        case "managed_start_daemon":
        case "managed_stop_daemon":
        case "managed_sign_out":
          return null;
        case "chat_approval_modes_save":
          return null;
        case "chat_approval_modes_load":
          return { ...store.config.approvalModes };
        case "desktop_pending_update":
          return store.config.pendingUpdate;
        case "desktop_install_update":
          if (store.config.installFails) throw new Error("mocked install failure");
          return null;
        case "detect_pidash_cli": {
          const cli = store.config.cli;
          return { installed: cli.installed, path: cli.path, version: cli.version };
        }
        case "install_pidash_cli": {
          if (store.installInFlight) throw new Error("mocked install already running");
          store.installInFlight = true;
          try {
            for (const line of store.config.installLogLines) emit("pidash-install-log", { line });
            store.config.cli = { installed: true, path: "/mock/bin/pidash", version: "0.0.0-mock" };
            return null;
          } finally {
            store.installInFlight = false;
          }
        }
        case "pidash_cli_login":
          return null;
        case "chat_create_session": {
          const account = String(params["account"] ?? "");
          const session = (params["session"] as Record<string, unknown> | undefined) ?? {};
          const id = `mock-chat-${store.chatSessions.size + 1}`;
          const now = Date.now();
          const record = {
            id,
            title: "Mocked chat",
            workspace: String(session["workspace"] ?? ""),
            project: String(session["project"] ?? ""),
            working_dir: `/mock/chat/${id}`,
            engine_version: "mock",
            engine_thread_id: null,
            created_at: now,
            updated_at: now,
          };
          store.chatSessions.set(sessionKey(account, id), record);
          store.chatEvents.set(sessionKey(account, id), []);
          return record;
        }
        case "chat_list_sessions": {
          const account = String(params["account"] ?? "");
          const rows: Record<string, unknown>[] = [];
          for (const [key, record] of store.chatSessions) {
            if (key.startsWith(`${account}:`)) rows.push(record);
          }
          return rows;
        }
        case "chat_get_session": {
          const { account, sessionId } = chatRecord(params);
          return store.chatSessions.get(sessionKey(account, sessionId)) ?? null;
        }
        case "chat_list_events": {
          const { account, sessionId } = chatRecord(params);
          return store.chatEvents.get(sessionKey(account, sessionId)) ?? [];
        }
        case "chat_append_event": {
          const { account, sessionId } = chatRecord(params);
          const key = sessionKey(account, sessionId);
          const rows = store.chatEvents.get(key);
          if (!rows) throw new Error("mocked history has no such session");
          const event = (params["event"] as Record<string, unknown> | undefined) ?? {};
          store.chatSeq += 1;
          const row = {
            id: `mock-event-${store.chatSeq}`,
            session_id: sessionId,
            seq: store.chatSeq,
            role: String(event["role"] ?? "user"),
            content: String(event["content"] ?? ""),
            tool_calls: null,
            approval_decision: null,
            created_at: Date.now(),
          };
          rows.push(row);
          return row;
        }
        case "chat_set_thread_id": {
          const { account, sessionId } = chatRecord(params);
          const record = store.chatSessions.get(sessionKey(account, sessionId));
          if (record) record["engine_thread_id"] = String(params["engineThreadId"] ?? "");
          return null;
        }
        case "chat_rename_session": {
          const { account, sessionId } = chatRecord(params);
          const record = store.chatSessions.get(sessionKey(account, sessionId));
          if (record) {
            record["title"] = String(params["title"] ?? record["title"]);
            record["updated_at"] = Date.now();
          }
          return null;
        }
        case "chat_delete_session": {
          const { account, sessionId } = chatRecord(params);
          const key = sessionKey(account, sessionId);
          store.chatSessions.delete(key);
          store.chatEvents.delete(key);
          return null;
        }
        case "chat_clear_history": {
          const account = String(params["account"] ?? "");
          for (const key of [...store.chatSessions.keys()]) {
            if (key.startsWith(`${account}:`)) {
              store.chatSessions.delete(key);
              store.chatEvents.delete(key);
            }
          }
          return null;
        }
        case "chat_working_dir":
          return `/mock/chat/${String(params["account"] ?? "")}/${String(params["workspace"] ?? "")}`;
        case "chat_warm":
          return null;
        case "chat_send": {
          const chatSessionId = String(params["chatSessionId"] ?? "");
          const script =
            store.config.chatScript.length > 0
              ? store.config.chatScript
              : [
                  {
                    result: "chat_message_completed",
                    data: {
                      chat_session_id: chatSessionId,
                      message_id: `mock-message-${Date.now()}`,
                      assistant_message: store.config.chatEcho,
                      status: "completed",
                    },
                  },
                ];
          queueMicrotask(() => {
            for (const frame of script) emit("chat://frame", frame);
          });
          return null;
        }
        case "chat_cancel":
        case "chat_close":
        case "chat_decide":
          return null;
        default:
          throw new Error(`mock desktop shell has no handler for ${command}`);
      }
    }

    const w = window as unknown as Record<string, unknown>;
    w["__PIDASH_NATIVE_HTTP__"] = window.location.origin;
    w["__TAURI__"] = {
      core: {
        invoke,
        Channel: class Channel<T = unknown> {
          onmessage: ((message: T) => void) | null = null;
        },
      },
      event: {
        listen: async <T>(event: string, handler: (message: { payload: T }) => void): Promise<() => void> => {
          let handlers = store.listeners.get(event);
          if (!handlers) {
            handlers = new Set();
            store.listeners.set(event, handlers);
          }
          const wrapped = handler as (message: { payload: unknown }) => void;
          handlers.add(wrapped);
          return () => {
            handlers.delete(wrapped);
          };
        },
      },
    };
    w["__PIDASH_DESKTOP_MOCK__"] = {
      invocations: store.invocations,
      emit,
      configure: (patch: DesktopMockConfig) => {
        store.config = { ...store.config, ...patch };
      },
    };
  }, config);
}

/** Emit a native event the page subscribed to (updater, deep links, chat, install log). */
export async function desktopEmit(page: Page, event: string, payload: unknown): Promise<void> {
  await page.evaluate(
    ([name, body]) => {
      window.__PIDASH_DESKTOP_MOCK__?.emit(name as string, body);
    },
    [event, payload] as [string, unknown]
  );
}

/** Every native invoke recorded so far, oldest first. */
export async function desktopInvocations(page: Page, command?: string): Promise<DesktopInvocation[]> {
  return page.evaluate((name: string | undefined) => {
    const recorded = window.__PIDASH_DESKTOP_MOCK__?.invocations ?? [];
    if (name === undefined) return recorded;
    return recorded.filter((entry) => entry.command === name);
  }, command);
}

/** Patch the mock's scenario knobs after install (pending update, CLI, chat script, ...). */
export async function desktopConfigure(page: Page, patch: DesktopMockConfig): Promise<void> {
  await page.evaluate((body) => {
    window.__PIDASH_DESKTOP_MOCK__?.configure(body as DesktopMockConfig);
  }, patch);
}
