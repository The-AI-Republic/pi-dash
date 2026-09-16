/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { beforeEach, describe, expect, it, vi } from "vitest";

// agent-runtime.ts (imported transitively) constructs an APIService at module
// load, which reads API_BASE_URL. Stub it so importing the transport is cheap.
vi.mock("@pi-dash/constants", () => ({ API_BASE_URL: "http://localhost:18002" }));

import type { TauriBridge } from "@/services/local-chat-transport";
import { ChatFrameTranslator, LocalChatTransport } from "@/services/local-chat-transport";

const SESSION = "11111111-1111-1111-1111-111111111111";

type Frame = Parameters<ChatFrameTranslator["translate"]>[0];

function makeBridge(overrides: Partial<Record<string, unknown>> = {}) {
  const listeners: Record<string, (payload: unknown) => void> = {};
  const invoke = vi.fn(async (command: string) => {
    if (command in overrides) return overrides[command];
    if (command === "chat_get_session")
      return {
        id: SESSION,
        title: "",
        workspace: "ws",
        project: "pidash-builtin",
        working_dir: "/tmp/chat/ws/proj",
        engine_version: "0.1.23",
        engine_thread_id: "thread-9",
        created_at: 1000,
        updated_at: 2000,
      };
    return undefined;
  });
  const listen = vi.fn(async (event: string, handler: (payload: unknown) => void) => {
    listeners[event] = handler;
    return () => delete listeners[event];
  });
  const ensureRuntime = vi.fn(async () => {});
  const bridge: TauriBridge = { invoke, listen, getAccount: () => "acct-1", ensureRuntime };
  return { bridge, invoke, listen, listeners, ensureRuntime };
}

function emit(listeners: Record<string, (payload: unknown) => void>, event: string, payload: unknown) {
  listeners[event]?.(payload);
}

describe("ChatFrameTranslator", () => {
  it("maps chat_started to a turn_started event carrying the thread id", () => {
    const t = new ChatFrameTranslator(SESSION);
    const ev = t.translate({
      result: "chat_started",
      data: { chat_session_id: SESSION, local_thread_id: "thr-1", started_at: "2026-09-15T00:00:00Z" },
    } as Frame)!;
    expect(ev.kind).toBe("turn_started");
    expect(ev.session).toBe(SESSION);
    expect(ev.payload.local_thread_id).toBe("thr-1");
    expect(ev.seq).toBe(1);
  });

  it("groups assistant_delta onto the message id from chat_message_started", () => {
    const t = new ChatFrameTranslator(SESSION);
    const started = t.translate({
      result: "chat_message_started",
      data: { chat_session_id: SESSION, message_id: "msg-1", started_at: "2026-09-15T00:00:00Z" },
    } as Frame)!;
    expect(started.kind).toBe("message_started");
    expect(started.message).toBe("msg-1");

    const delta = t.translate({
      result: "chat_event",
      data: { chat_session_id: SESSION, bridge_seq: 3, kind: "assistant_delta", payload: { params: { delta: "hi" } } },
    } as Frame)!;
    expect(delta.kind).toBe("assistant_delta");
    expect(delta.message).toBe("msg-1");
    expect(delta.payload).toEqual({ params: { delta: "hi" } });
    // seq is monotonic across all frames, independent of bridge_seq.
    expect(delta.seq).toBe(2);
  });

  it("maps an approval request, keeping the local approval id and kind", () => {
    const t = new ChatFrameTranslator(SESSION);
    const ev = t.translate({
      result: "chat_approval_request",
      data: {
        chat_session_id: SESSION,
        local_approval_id: "ap-1",
        kind: "command_execution",
        payload: { command: "ls" },
        reason: "needs approval",
      },
    } as Frame)!;
    expect(ev.kind).toBe("chat_approval_request");
    expect(ev.payload.local_approval_id).toBe("ap-1");
    expect(ev.payload.approval_kind).toBe("command_execution");
    expect(ev.payload.command).toBe("ls");
  });

  it("maps completion, failure and close to their UI kinds and clears the message id", () => {
    const t = new ChatFrameTranslator(SESSION);
    t.translate({
      result: "chat_message_started",
      data: { chat_session_id: SESSION, message_id: "msg-1", started_at: "2026-09-15T00:00:00Z" },
    } as Frame);
    const done = t.translate({
      result: "chat_message_completed",
      data: {
        chat_session_id: SESSION,
        message_id: "msg-1",
        assistant_message: "all done",
        status: "completed",
        completed_at: "2026-09-15T00:00:01Z",
      },
    } as Frame)!;
    expect(done.kind).toBe("turn_completed");
    expect(done.payload.status).toBe("completed");

    // After completion, a stray assistant_delta must not re-attach to msg-1.
    const stray = t.translate({
      result: "chat_event",
      data: { chat_session_id: SESSION, bridge_seq: 9, kind: "assistant_delta", payload: {} },
    } as Frame)!;
    expect(stray.message).toBeNull();

    const failed = t.translate({
      result: "chat_failed",
      data: { chat_session_id: SESSION, code: "engine_crash", detail: "boom", failed_at: "2026-09-15T00:00:02Z" },
    } as Frame)!;
    expect(failed.kind).toBe("chat_failed");
    expect(failed.payload.code).toBe("engine_crash");

    const closed = t.translate({
      result: "chat_closed",
      data: { chat_session_id: SESSION, closed_at: "2026-09-15T00:00:03Z" },
    } as Frame)!;
    expect(closed.kind).toBe("chat_closed");
  });
});

describe("LocalChatTransport verbs", () => {
  let ctx: ReturnType<typeof makeBridge>;
  let transport: LocalChatTransport;

  beforeEach(() => {
    ctx = makeBridge();
    transport = new LocalChatTransport(ctx.bridge);
  });

  it("lists sessions scoped by workspace and runner, mapped to the UI shape", async () => {
    ctx = makeBridge({
      chat_list_sessions: [
        {
          id: "s1",
          title: "",
          workspace: "ws",
          project: "pidash-builtin",
          working_dir: "/w/1",
          engine_version: "",
          engine_thread_id: null,
          created_at: 1,
          updated_at: 5,
        },
        {
          id: "s2",
          title: "",
          workspace: "other",
          project: "pidash-builtin",
          working_dir: "/w/2",
          engine_version: "",
          engine_thread_id: null,
          created_at: 1,
          updated_at: 1,
        },
        {
          id: "s3",
          title: "",
          workspace: "ws",
          project: "other-runner",
          working_dir: "/w/3",
          engine_version: "",
          engine_thread_id: null,
          created_at: 1,
          updated_at: 1,
        },
      ],
    });
    transport = new LocalChatTransport(ctx.bridge);
    const sessions = await transport.listChatSessions("ws", "pidash-builtin");
    expect(sessions.map((s) => s.id)).toEqual(["s1"]);
    expect(sessions[0].cwd).toBe("/w/1");
    expect(sessions[0].runner).toBe("pidash-builtin");
    // updated_at > created_at ⇒ has activity ⇒ a last_message_at is surfaced.
    expect(sessions[0].last_message_at).not.toBeNull();
    expect(ctx.invoke).toHaveBeenCalledWith("chat_list_sessions", { account: "acct-1" });
  });

  it("creates a session, storing the runner id in the project column", async () => {
    ctx = makeBridge({
      chat_create_session: {
        id: "new",
        title: "",
        workspace: "ws",
        project: "pidash-builtin",
        working_dir: "/w/new",
        engine_version: "",
        engine_thread_id: null,
        created_at: 7,
        updated_at: 7,
      },
    });
    transport = new LocalChatTransport(ctx.bridge);
    const session = await transport.createChatSession({ workspace: "ws", runner: "pidash-builtin" });
    expect(session.id).toBe("new");
    expect(ctx.invoke).toHaveBeenCalledWith("chat_create_session", {
      account: "acct-1",
      session: { title: "", workspace: "ws", project: "pidash-builtin", engine_version: "" },
    });
  });

  it("lists messages, mapping the approval role to system", async () => {
    ctx = makeBridge({
      chat_list_events: [
        {
          id: "e1",
          session_id: SESSION,
          seq: 1,
          role: "user",
          content: "hi",
          tool_calls: null,
          approval_decision: null,
          created_at: 10,
        },
        {
          id: "e2",
          session_id: SESSION,
          seq: 2,
          role: "approval",
          content: "",
          tool_calls: null,
          approval_decision: "approved",
          created_at: 11,
        },
      ],
    });
    transport = new LocalChatTransport(ctx.bridge);
    const messages = await transport.listChatMessages(SESSION);
    expect(messages.map((m) => m.role)).toEqual(["user", "system"]);
    expect(ctx.invoke).toHaveBeenCalledWith("chat_list_events", { account: "acct-1", sessionId: SESSION });
  });

  it("sendChatMessage persists the user turn then submits chat_send", async () => {
    ctx = makeBridge({
      chat_append_event: {
        id: "u1",
        session_id: SESSION,
        seq: 3,
        role: "user",
        content: "hello",
        tool_calls: null,
        approval_decision: null,
        created_at: 20,
      },
    });
    transport = new LocalChatTransport(ctx.bridge);
    const message = await transport.sendChatMessage(SESSION, "hello");
    expect(message.role).toBe("user");
    expect(message.content).toBe("hello");
    expect(ctx.invoke).toHaveBeenCalledWith("chat_append_event", {
      account: "acct-1",
      sessionId: SESSION,
      event: { role: "user", content: "hello" },
    });
    const sendCall = ctx.invoke.mock.calls.find((c) => c[0] === "chat_send");
    expect(sendCall).toBeDefined();
    const args = sendCall![1] as Record<string, unknown>;
    expect(args.chatSessionId).toBe(SESSION);
    expect(args.content).toBe("hello");
    expect(args.cwd).toBe("/tmp/chat/ws/proj");
    expect(args.localThreadId).toBe("thread-9");
    expect(typeof args.messageId).toBe("string");
  });

  it("warm / cancel / close / decide reach their commands", async () => {
    await transport.warmChatSession(SESSION);
    expect(ctx.invoke).toHaveBeenCalledWith(
      "chat_warm",
      expect.objectContaining({ chatSessionId: SESSION, cwd: "/tmp/chat/ws/proj" })
    );

    await transport.cancelChat(SESSION, "user_cancelled");
    expect(ctx.invoke).toHaveBeenCalledWith(
      "chat_cancel",
      expect.objectContaining({ chatSessionId: SESSION, reason: "user_cancelled" })
    );

    const closed = await transport.closeChat(SESSION);
    expect(closed.id).toBe(SESSION);
    expect(ctx.invoke).toHaveBeenCalledWith("chat_close", expect.objectContaining({ chatSessionId: SESSION }));

    await transport.decideChatApproval(SESSION, "ap-1", "accept");
    expect(ctx.invoke).toHaveBeenCalledWith(
      "chat_decide",
      expect.objectContaining({ chatSessionId: SESSION, localApprovalId: "ap-1", decision: "accept" })
    );
  });

  it("brings the daemon up before warming or sending — a chat right after sign-in has no runtime yet", async () => {
    // Until this existed, the only thing that ever started the bundled daemon
    // was connecting a project, so a chat opened straight after sign-in failed
    // at "connecting to managed daemon" with nothing shown to the user.
    const order: string[] = [];
    ctx = makeBridge({
      chat_append_event: {
        id: "u1",
        session_id: SESSION,
        seq: 3,
        role: "user",
        content: "hi",
        tool_calls: null,
        approval_decision: null,
        created_at: 20,
      },
    });
    ctx.ensureRuntime.mockImplementation(async () => {
      order.push("ensureRuntime");
    });
    ctx.invoke.mockImplementation(async (command: string) => {
      if (command.startsWith("chat_warm") || command.startsWith("chat_send")) order.push(command);
      if (command === "chat_append_event")
        return {
          id: "u1",
          session_id: SESSION,
          seq: 3,
          role: "user",
          content: "hi",
          tool_calls: null,
          approval_decision: null,
          created_at: 20,
        };
      if (command === "chat_get_session")
        return {
          id: SESSION,
          title: "",
          workspace: "ws",
          project: "pidash-builtin",
          working_dir: "/tmp/chat/ws/proj",
          engine_version: "0.1.23",
          engine_thread_id: "thread-9",
          created_at: 1000,
          updated_at: 2000,
        };
      return undefined;
    });
    transport = new LocalChatTransport(ctx.bridge);

    await transport.warmChatSession(SESSION);
    await transport.sendChatMessage(SESSION, "hi");

    expect(ctx.ensureRuntime).toHaveBeenCalledTimes(2);
    expect(order).toEqual(["ensureRuntime", "chat_warm", "ensureRuntime", "chat_send"]);
  });

  it("never sends a runner selector — the stored id is not a daemon runner name", async () => {
    // The daemon resolves `runner` by the *name* in its own config
    // (`resolve_runner`, runner/src/ipc/server.rs). The id this transport holds
    // is the synthetic route id `pidash-builtin`, which no daemon is
    // configured under, so sending it failed every request with
    // `no runner named "pidash-builtin"`. Omitting it selects the daemon's
    // single managed runner.
    ctx = makeBridge({
      chat_append_event: {
        id: "u1",
        session_id: SESSION,
        seq: 3,
        role: "user",
        content: "hi",
        tool_calls: null,
        approval_decision: null,
        created_at: 20,
      },
    });
    transport = new LocalChatTransport(ctx.bridge);
    await transport.sendChatMessage(SESSION, "hi");
    await transport.warmChatSession(SESSION);
    await transport.cancelChat(SESSION);
    await transport.closeChat(SESSION);
    await transport.decideChatApproval(SESSION, "ap-1", "accept");
    const chatCalls = ctx.invoke.mock.calls.filter(([command]) => String(command).startsWith("chat_"));
    expect(chatCalls.length).toBeGreaterThan(0);
    for (const [command, args] of chatCalls) {
      expect((args as Record<string, unknown>) ?? {}, `${command} must not carry a runner selector`).not.toHaveProperty(
        "runner"
      );
    }
  });
});

describe("LocalChatTransport.subscribeChatEvents", () => {
  it("translates frames for the session, ignores other sessions, and unsubscribes", async () => {
    const ctx = makeBridge();
    const transport = new LocalChatTransport(ctx.bridge);
    const onEvent = vi.fn();
    const onError = vi.fn();
    const unsubscribe = transport.subscribeChatEvents(SESSION, 0, onEvent, onError);
    await Promise.resolve();

    // A frame for a different session is dropped.
    emit(ctx.listeners, "chat://frame", {
      result: "chat_event",
      data: { chat_session_id: "other", bridge_seq: 1, kind: "assistant_delta", payload: {} },
    });
    expect(onEvent).not.toHaveBeenCalled();

    emit(ctx.listeners, "chat://frame", {
      result: "chat_event",
      data: { chat_session_id: SESSION, bridge_seq: 1, kind: "assistant_delta", payload: { params: { delta: "yo" } } },
    });
    expect(onEvent).toHaveBeenCalledTimes(1);
    expect(onEvent.mock.calls[0][0].kind).toBe("assistant_delta");

    // A transport error for the session surfaces via onError.
    emit(ctx.listeners, "chat://error", { chat_session_id: SESSION, message: "daemon down" });
    expect(onError).toHaveBeenCalledTimes(1);
    expect(onError.mock.calls[0][0]).toBeInstanceOf(Error);

    unsubscribe();
    emit(ctx.listeners, "chat://frame", {
      result: "chat_event",
      data: { chat_session_id: SESSION, bridge_seq: 2, kind: "assistant_delta", payload: {} },
    });
    expect(onEvent).toHaveBeenCalledTimes(1);
  });

  it("persists the engine thread id and the assistant message as the turn streams", async () => {
    const ctx = makeBridge();
    const transport = new LocalChatTransport(ctx.bridge);
    const unsubscribe = transport.subscribeChatEvents(SESSION, 0, vi.fn(), vi.fn());
    await Promise.resolve();

    emit(ctx.listeners, "chat://frame", {
      result: "chat_started",
      data: { chat_session_id: SESSION, local_thread_id: "thread-42", started_at: "2026-09-15T00:00:00Z" },
    });
    emit(ctx.listeners, "chat://frame", {
      result: "chat_message_completed",
      data: {
        chat_session_id: SESSION,
        message_id: "m1",
        assistant_message: "final answer",
        status: "completed",
        completed_at: "2026-09-15T00:00:01Z",
      },
    });
    await Promise.resolve();

    expect(ctx.invoke).toHaveBeenCalledWith("chat_set_thread_id", {
      account: "acct-1",
      sessionId: SESSION,
      engineThreadId: "thread-42",
    });
    expect(ctx.invoke).toHaveBeenCalledWith("chat_append_event", {
      account: "acct-1",
      sessionId: SESSION,
      event: { role: "assistant", content: "final answer" },
    });
    unsubscribe();
  });

  it("falls back to accumulated deltas when a completed frame carries no assistant_message", async () => {
    // H2: the engine streamed a reply but its done payload didn't echo the text
    // under a key the daemon recognises, so `assistant_message` is empty. The
    // transport must persist the accumulated `assistant_delta` text instead, so
    // the reply survives the page's refetch-on-turn_completed rather than
    // vanishing from both the UI and history.
    const ctx = makeBridge();
    const transport = new LocalChatTransport(ctx.bridge);
    const onEvent = vi.fn();
    const unsubscribe = transport.subscribeChatEvents(SESSION, 0, onEvent, vi.fn());
    await Promise.resolve();

    emit(ctx.listeners, "chat://frame", {
      result: "chat_message_started",
      data: { chat_session_id: SESSION, message_id: "m9", started_at: "2026-09-15T00:00:00Z" },
    });
    emit(ctx.listeners, "chat://frame", {
      result: "chat_event",
      data: {
        chat_session_id: SESSION,
        bridge_seq: 1,
        kind: "assistant_delta",
        payload: { params: { delta: "Hello " } },
      },
    });
    emit(ctx.listeners, "chat://frame", {
      result: "chat_event",
      data: {
        chat_session_id: SESSION,
        bridge_seq: 2,
        kind: "assistant_delta",
        payload: { params: { delta: "world" } },
      },
    });
    emit(ctx.listeners, "chat://frame", {
      result: "chat_message_completed",
      data: {
        chat_session_id: SESSION,
        message_id: "m9",
        // No assistant_message — the fallback must supply the text.
        status: "completed",
        completed_at: "2026-09-15T00:00:01Z",
      },
    });
    await new Promise((resolve) => setTimeout(resolve));

    expect(ctx.invoke).toHaveBeenCalledWith("chat_append_event", {
      account: "acct-1",
      sessionId: SESSION,
      event: { role: "assistant", content: "Hello world" },
    });
    // turn_completed is only dispatched after the reply is persisted, so the
    // page's refetch finds it in history.
    const completedCall = onEvent.mock.calls.find((c) => c[0].kind === "turn_completed");
    expect(completedCall).toBeDefined();
    const appendIndex = ctx.invoke.mock.calls.findIndex((c) => c[0] === "chat_append_event");
    expect(appendIndex).toBeGreaterThanOrEqual(0);
    unsubscribe();
  });

  it("resets the delta accumulator between turns", async () => {
    // The accumulator must not bleed one turn's deltas into the next turn's
    // fallback — chat_message_started clears it.
    const ctx = makeBridge();
    const transport = new LocalChatTransport(ctx.bridge);
    const unsubscribe = transport.subscribeChatEvents(SESSION, 0, vi.fn(), vi.fn());
    await Promise.resolve();

    const started = (id: string) => ({
      result: "chat_message_started",
      data: { chat_session_id: SESSION, message_id: id, started_at: "2026-09-15T00:00:00Z" },
    });
    const delta = (seq: number, text: string) => ({
      result: "chat_event",
      data: {
        chat_session_id: SESSION,
        bridge_seq: seq,
        kind: "assistant_delta",
        payload: { params: { delta: text } },
      },
    });
    const completed = (id: string) => ({
      result: "chat_message_completed",
      data: { chat_session_id: SESSION, message_id: id, status: "completed", completed_at: "2026-09-15T00:00:01Z" },
    });

    emit(ctx.listeners, "chat://frame", started("m1"));
    emit(ctx.listeners, "chat://frame", delta(1, "first"));
    emit(ctx.listeners, "chat://frame", completed("m1"));
    emit(ctx.listeners, "chat://frame", started("m2"));
    emit(ctx.listeners, "chat://frame", delta(2, "second"));
    emit(ctx.listeners, "chat://frame", completed("m2"));
    await new Promise((resolve) => setTimeout(resolve));

    const appended = ctx.invoke.mock.calls
      .filter((c) => c[0] === "chat_append_event")
      .map((c) => (c[1] as { event: { content: string } }).event.content);
    expect(appended).toEqual(["first", "second"]);
    unsubscribe();
  });
});
