// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Desktop local chat bridge (NEWFRONT-553, RUN-033-036): every store, warm,
// send, cancel, close and decide verb reaches its Rust command with the
// daemon's argument names, and the event subscriptions filter nothing but
// deliver each payload to its handler.
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { CHAT_ERROR_EVENT, CHAT_FRAME_EVENT, platform } from "./tauri.js";

type InvokeFn = (command: string, args?: unknown) => Promise<unknown>;
type ListenFn = (event: string, handler: (message: { payload: unknown }) => void) => Promise<() => void>;

const NATIVE_ORIGIN = "http://localhost:8000";

let listeners: Map<string, (message: { payload: unknown }) => void>;

function installBridge(invoke: InvokeFn, listen?: ListenFn): void {
  listeners = new Map();
  const win = globalThis.window as unknown as Record<string, unknown>;
  win["__PIDASH_NATIVE_HTTP__"] = NATIVE_ORIGIN;
  win["__TAURI__"] = {
    core: { invoke },
    event: {
      listen:
        listen ??
        (async (event: string, handler: (message: { payload: unknown }) => void) => {
          listeners.set(event, handler);
          return () => listeners.delete(event);
        }),
    },
  };
}

function removeBridge(): void {
  const win = globalThis.window as unknown as Record<string, unknown>;
  delete win["__PIDASH_NATIVE_HTTP__"];
  delete win["__TAURI__"];
}

beforeEach(() => {
  removeBridge();
});

afterEach(() => {
  removeBridge();
  vi.restoreAllMocks();
});

describe("desktop local chat", () => {
  it("reaches every store command with the account and session args", async () => {
    const invoke = vi.fn(async () => []);
    installBridge(invoke);
    const chat = platform.localChat;
    expect(chat).toBeDefined();
    await chat?.listSessions("u1");
    await chat?.createSession("u1", { workspace: "ws-id", runner: "r-1" });
    await chat?.getSession("u1", "s-1");
    await chat?.listEvents("u1", "s-1");
    await chat?.appendEvent("u1", "s-1", { role: "user", content: "hi" });
    await chat?.setThreadId("u1", "s-1", "t-1");
    expect(invoke.mock.calls).toEqual([
      ["chat_list_sessions", { account: "u1" }],
      ["chat_create_session", { account: "u1", session: { workspace: "ws-id", project: "r-1" } }],
      ["chat_get_session", { account: "u1", sessionId: "s-1" }],
      ["chat_list_events", { account: "u1", sessionId: "s-1" }],
      ["chat_append_event", { account: "u1", sessionId: "s-1", event: { role: "user", content: "hi" } }],
      ["chat_set_thread_id", { account: "u1", sessionId: "s-1", engineThreadId: "t-1" }],
    ]);
  });

  it("speaks the daemon's request shape for engine verbs", async () => {
    const invoke = vi.fn(async () => undefined);
    installBridge(invoke);
    const chat = platform.localChat!;
    await chat.warm({ workspaceSlug: "acme", sessionId: "s-1", cwd: "/repo", mode: "ask", localThreadId: "t-1" });
    await chat.send({
      workspaceSlug: "acme",
      sessionId: "s-1",
      messageId: "m-1",
      content: "hi",
      cwd: "/repo",
      mode: "workspace",
      localThreadId: undefined,
    });
    await chat.cancel({ workspaceSlug: "acme", sessionId: "s-1", reason: "stop" });
    await chat.close({ workspaceSlug: "acme", sessionId: "s-1" });
    await chat.decide({
      workspaceSlug: "acme",
      sessionId: "s-1",
      localApprovalId: "a-1",
      decision: "accept_for_session",
    });
    expect(invoke.mock.calls).toEqual([
      ["chat_warm", { workspace: "acme", chatSessionId: "s-1", cwd: "/repo", mode: "ask", localThreadId: "t-1" }],
      [
        "chat_send",
        {
          workspace: "acme",
          chatSessionId: "s-1",
          messageId: "m-1",
          content: "hi",
          cwd: "/repo",
          mode: "workspace",
          localThreadId: undefined,
        },
      ],
      ["chat_cancel", { workspace: "acme", chatSessionId: "s-1", reason: "stop" }],
      ["chat_close", { workspace: "acme", chatSessionId: "s-1" }],
      [
        "chat_decide",
        { workspace: "acme", chatSessionId: "s-1", localApprovalId: "a-1", decision: "accept_for_session" },
      ],
    ]);
  });

  it("delivers frames and failures to the subscriber and stops on unsubscribe", async () => {
    installBridge(vi.fn(async () => undefined));
    const onFrame = vi.fn();
    const onError = vi.fn();
    const stop = platform.localChat!.subscribe({ onFrame, onError });
    await vi.waitFor(() => expect(listeners.size).toBe(2));
    const frame = { result: "chat_closed", data: { chat_session_id: "s-1" } };
    listeners.get(CHAT_FRAME_EVENT)?.({ payload: frame });
    listeners.get(CHAT_ERROR_EVENT)?.({ payload: { chat_session_id: "s-1", message: "gone" } });
    expect(onFrame).toHaveBeenCalledWith(frame);
    expect(onError).toHaveBeenCalledWith({ chat_session_id: "s-1", message: "gone" });
    stop();
    stop();
    expect(listeners.size).toBe(0);
    stop();
  });

  it("drops a subscription that resolves after the caller unsubscribed", async () => {
    const pending: { event: string; handler: (message: { payload: unknown }) => void }[] = [];
    installBridge(
      vi.fn(async () => undefined),
      (event, handler) => {
        pending.push({ event, handler });
        return Promise.resolve(vi.fn());
      }
    );
    const stop = platform.localChat!.subscribe({ onFrame: vi.fn(), onError: vi.fn() });
    stop();
    await Promise.resolve();
    expect(pending).toHaveLength(2);
  });

  it("is inert without an event bus", () => {
    const win = globalThis.window as unknown as Record<string, unknown>;
    win["__PIDASH_NATIVE_HTTP__"] = NATIVE_ORIGIN;
    win["__TAURI__"] = { core: { invoke: vi.fn(async () => undefined) } };
    const stop = platform.localChat!.subscribe({ onFrame: vi.fn(), onError: vi.fn() });
    expect(stop).toBeTypeOf("function");
    stop();
  });

  it("rejects without the native bridge", async () => {
    await expect(platform.localChat?.listSessions("u1")).rejects.toThrow("chat_list_sessions needs the native bridge");
  });

  it("is absent from the web platform", async () => {
    const { platform: webPlatform } = await import("./web.js");
    expect(webPlatform.localChat).toBeUndefined();
  });
});
