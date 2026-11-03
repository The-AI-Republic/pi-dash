// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Local engine transport (NEWFRONT-553, RUN-033-036): rows come from the
// per-account store, a turn persists its user message before the engine is
// asked, frames translate into the shared event shape, the reply is stored
// before the page re-reads history, the thread id is kept for resume, a
// second turn is refused in memory, and a terminal frame releases the
// session even while nobody is subscribed.
import { afterEach, describe, expect, it, type Mock, vi } from "vitest";

import type { LocalChatApi, LocalChatFrame, LocalChatSessionRecord } from "../../../core/platform/index.js";
import {
  BUILTIN_CHAT_ID,
  builtinRunnerRow,
  chatRoleFor,
  createFrameFolder,
  createLocalChatTransport,
  DEFAULT_APPROVAL_MODE,
  deltaText,
  endsTurn,
  frameSession,
  localMessageRow,
  localSessionRow,
  CHAT_SESSION_GONE_MESSAGE,
  CHAT_TITLE_REQUIRED_MESSAGE,
  TURN_BUSY_MESSAGE,
} from "./localTransport.js";

const NOW = 1_760_000_000_000;

function record(overrides: Partial<LocalChatSessionRecord> = {}): LocalChatSessionRecord {
  return {
    id: "s-1",
    title: "",
    workspace: "ws-id",
    project: "r-1",
    working_dir: "/repo",
    engine_version: "",
    engine_thread_id: null,
    created_at: NOW,
    updated_at: NOW,
    ...overrides,
  };
}

function event(id: string, sessionId = "s-1") {
  return {
    id,
    session_id: sessionId,
    seq: 1,
    role: "user",
    content: "hi",
    tool_calls: null,
    approval_decision: null,
    created_at: NOW,
  };
}

interface Harness {
  api: LocalChatApi;
  fire: (frame: LocalChatFrame) => void;
  fail: (event: { chat_session_id: string; message: string }) => void;
  ensureRuntime: Mock<() => Promise<void>>;
}

function harness(overrides: Partial<LocalChatApi> = {}): Harness {
  let onFrame: (frame: LocalChatFrame) => void = () => undefined;
  let onError: (event: { chat_session_id: string; message: string }) => void = () => undefined;
  const ensureRuntime = vi.fn(async () => undefined);
  const api: LocalChatApi = {
    listSessions: vi.fn(async () => [record()]),
    createSession: vi.fn(async (_account, input) =>
      record({ id: "new", project: input.runner, workspace: input.workspace })
    ),
    getSession: vi.fn(async () => record()),
    listEvents: vi.fn(async () => [event("e-1")]),
    appendEvent: vi.fn(async (_account, sessionId, input) => ({
      ...event("e-2", sessionId),
      role: input.role,
      content: input.content,
    })),
    setThreadId: vi.fn(async () => undefined),
    warm: vi.fn(async () => undefined),
    send: vi.fn(async () => undefined),
    cancel: vi.fn(async () => undefined),
    close: vi.fn(async () => undefined),
    decide: vi.fn(async () => undefined),
    renameSession: vi.fn(async () => undefined),
    deleteSession: vi.fn(async () => undefined),
    clearHistory: vi.fn(async () => undefined),
    workingDir: vi.fn(async () => "/repo"),
    subscribe: (handlers) => {
      onFrame = handlers.onFrame;
      onError = handlers.onError;
      return () => undefined;
    },
    ...overrides,
  };
  return {
    api,
    ensureRuntime,
    fire: (frame) => onFrame(frame),
    fail: (payload) => onError(payload),
  };
}

function transportFor(h: Harness, account = "u-1", stored: (runnerId: string) => string = () => "full_access") {
  return createLocalChatTransport({
    api: h.api,
    account: () => account,
    workspaceSlug: () => "acme",
    ensureRuntime: h.ensureRuntime,
    storedMode: stored,
  });
}

/** The local transport always implements the read verbs; the seam types them optional. */
function complete<T extends ReturnType<typeof transportFor>>(
  transport: T
): T & Required<Pick<T, "listChatSessions" | "listChatMessages" | "warmChatSession" | "createChatSession">> {
  return transport as T &
    Required<Pick<T, "listChatSessions" | "listChatMessages" | "warmChatSession" | "createChatSession">>;
}

function handlers() {
  return { onEvent: vi.fn(), onError: vi.fn(), onEnd: vi.fn() };
}

const started = (sessionId = "s-1"): LocalChatFrame => ({
  result: "chat_started",
  data: { chat_session_id: sessionId, local_thread_id: "t-1", started_at: "x" },
});

describe("row mapping", () => {
  it("maps the engine's roles onto the transcript vocabulary", () => {
    expect(chatRoleFor("user")).toBe("user");
    expect(chatRoleFor("assistant")).toBe("assistant");
    expect(chatRoleFor("tool")).toBe("tool");
    expect(chatRoleFor("approval")).toBe("system");
    expect(chatRoleFor("system")).toBe("system");
    expect(chatRoleFor("mystery")).toBe("system");
  });

  it("always reads a stored conversation as open and carries its resume facts", () => {
    const row = localSessionRow(record({ engine_thread_id: "t-9", updated_at: NOW + 1000 }), "u-1", "m-1");
    expect(row.status).toBe("open");
    expect(row.cwd).toBe("/repo");
    expect(row.local_thread_id).toBe("t-9");
    expect(row.runner).toBe("r-1");
    expect(row.created_by).toBe("u-1");
    expect(row.last_message_at).toBe(new Date(NOW + 1000).toISOString());
    expect(row.active_message_id).toBe("m-1");
    expect(row.closed_at).toBeNull();
    expect(row.runner_detail.id).toBe("r-1");
  });

  it("reads an untouched conversation as having no turns", () => {
    expect(localSessionRow(record(), "u-1", null).last_message_at).toBeNull();
    expect(localSessionRow(record(), "u-1", null).active_message_id).toBeNull();
  });

  it("maps a stored transcript row", () => {
    const message = localMessageRow({ ...event("e-1"), role: "approval", content: "ok" });
    expect(message.role).toBe("system");
    expect(message.status).toBe("completed");
    expect(message.completed_at).toBe(new Date(NOW).toISOString());
    expect(message.session).toBe("s-1");
  });

  it("presents the built-in engine as an always-online runner", () => {
    const runner = builtinRunnerRow();
    expect(runner.id).toBe(BUILTIN_CHAT_ID);
    expect(runner.status).toBe("online");
    expect(runner.enrolled_at).not.toBeNull();
    expect(runner.name).not.toBe("");
  });
});

describe("frame folding", () => {
  it("numbers events and links deltas to the open assistant message", () => {
    const fold = createFrameFolder("s-1");
    expect(fold({ result: "chat_message_started", data: { chat_session_id: "s-1", message_id: "m-1" } })).toMatchObject(
      {
        kind: "message_started",
        message: "m-1",
        seq: 1,
      }
    );
    expect(
      fold({ result: "chat_event", data: { chat_session_id: "s-1", kind: "assistant_delta", payload: { text: "a" } } })
    ).toMatchObject({ kind: "assistant_delta", message: "m-1", seq: 2 });
    expect(
      fold({ result: "chat_event", data: { chat_session_id: "s-1", kind: "tool_call", payload: {} } })
    ).toMatchObject({ kind: "tool_call", message: null });
    expect(
      fold({
        result: "chat_message_completed",
        data: { chat_session_id: "s-1", message_id: "m-1", status: "ok" },
      })
    ).toMatchObject({ kind: "turn_completed", message: "m-1" });
    // After the turn closed a stray delta carries no message.
    expect(
      fold({ result: "chat_event", data: { chat_session_id: "s-1", kind: "assistant_delta", payload: {} } })
    ).toMatchObject({ message: null });
  });

  it("carries the thread ids on turn start and the request fields on an approval", () => {
    const fold = createFrameFolder("s-1");
    expect(fold(started())?.payload).toEqual({ local_thread_id: "t-1", local_session_id: "" });
    const approval = fold({
      result: "chat_approval_request",
      data: {
        chat_session_id: "s-1",
        local_approval_id: "a-1",
        kind: "network_access",
        payload: { url: "https://x" },
        reason: "why",
        expires_at: "2026-10-19T10:00:30Z",
      },
    });
    expect(approval?.kind).toBe("chat_approval_request");
    expect(approval?.payload).toEqual({
      local_approval_id: "a-1",
      approval_kind: "network_access",
      reason: "why",
      expires_at: "2026-10-19T10:00:30Z",
      url: "https://x",
    });
  });

  it("defaults a request with no reason or deadline and lets the target win", () => {
    const fold = createFrameFolder("s-1");
    const approval = fold({
      result: "chat_approval_request",
      data: {
        chat_session_id: "s-1",
        local_approval_id: "a-2",
        kind: "file_change",
        payload: { path: "/repo/a.txt" },
      },
    });
    expect(approval?.payload).toEqual({
      local_approval_id: "a-2",
      approval_kind: "file_change",
      reason: "",
      expires_at: null,
      path: "/repo/a.txt",
    });
    // A reason nested in the engine's own payload is not one of the
    // frame's request fields, so it survives untouched.
    const nested = fold({
      result: "chat_approval_request",
      data: {
        chat_session_id: "s-1",
        local_approval_id: "a-3",
        kind: "other",
        payload: { command: "ls" },
      },
    });
    expect(nested?.payload).toMatchObject({ local_approval_id: "a-3", reason: "", expires_at: null, command: "ls" });
  });

  it("translates failures and closes", () => {
    const fold = createFrameFolder("s-1");
    expect(
      fold({ result: "chat_failed", data: { chat_session_id: "s-1", code: "boom", detail: "why" } })?.payload
    ).toEqual({ code: "boom", detail: "why" });
    expect(fold({ result: "chat_failed", data: { chat_session_id: "s-1", code: "boom" } })?.payload).toEqual({
      code: "boom",
      detail: "",
    });
    expect(fold({ result: "chat_closed", data: { chat_session_id: "s-1" } })?.kind).toBe("chat_closed");
    // A newer daemon may send a frame this page does not know yet.
    expect(
      fold({ result: "chat_something_new", data: { chat_session_id: "s-1" } } as unknown as LocalChatFrame)
    ).toBeNull();
  });
});

describe("frame helpers", () => {
  it("names the session and whether a frame ends the turn", () => {
    expect(frameSession(started("s-9"))).toBe("s-9");
    expect(endsTurn(started())).toBe(false);
    expect(endsTurn({ result: "chat_closed", data: { chat_session_id: "s-1" } })).toBe(true);
    expect(endsTurn({ result: "chat_failed", data: { chat_session_id: "s-1", code: "x" } })).toBe(true);
    expect(
      endsTurn({ result: "chat_message_completed", data: { chat_session_id: "s-1", message_id: "m", status: "ok" } })
    ).toBe(true);
  });

  it("reads delta text from every engine spelling", () => {
    expect(deltaText({ params: { delta: "plain" } })).toBe("plain");
    expect(deltaText({ params: { delta: { text: "object" } } })).toBe("object");
    expect(deltaText({ params: { text: "text" } })).toBe("text");
    expect(deltaText({ delta: "bare" })).toBe("bare");
    expect(deltaText({ params: [] })).toBe("");
    expect(deltaText({})).toBe("");
    expect(deltaText({ params: { delta: 7, text: 8 } })).toBe("");
  });
});

describe("local transport", () => {
  afterEach(() => {
    vi.restoreAllMocks();
  });

  it("defaults the mode to the engine's full access and remembers per-session choices", () => {
    const transport = transportFor(harness());
    expect(DEFAULT_APPROVAL_MODE).toBe("full_access");
    expect(transport.getApprovalMode("s-1")).toBe("full_access");
    transport.setApprovalMode("s-1", "ask");
    expect(transport.getApprovalMode("s-1")).toBe("ask");
  });

  it("answers the built-in runner and declines a server contact", async () => {
    const transport = transportFor(harness());
    await expect(transport.chatRunner(BUILTIN_CHAT_ID)).resolves.toMatchObject({ status: "online" });
    await expect(transport.chatRunner("r-other")).resolves.toBeNull();
    expect(transport.ownsRunner(BUILTIN_CHAT_ID)).toBe(true);
    expect(transport.ownsRunner("r-other")).toBe(false);
  });

  it("refuses every store verb when signed out", async () => {
    const transport = complete(transportFor(harness(), ""));
    await expect(transport.listChatMessages("s-1")).rejects.toThrow(/Sign in/);
    await expect(transport.createChatSession({ workspace: "ws-id", runner: BUILTIN_CHAT_ID })).rejects.toThrow(
      /Sign in/
    );
    await expect(transport.closeChat("s-1")).rejects.toThrow(/Sign in/);
  });

  it("lists only this workspace's conversations for the bundled engine", async () => {
    const h = harness({
      listSessions: vi.fn(async () => [
        record({ id: "keep", project: BUILTIN_CHAT_ID }),
        record({ id: "other-runner", project: "r-2" }),
        record({ id: "other-workspace", workspace: "ws-2" }),
      ]),
    });
    const rows = await complete(transportFor(h)).listChatSessions("ws-id", BUILTIN_CHAT_ID);
    expect(rows?.map((row) => row.id)).toEqual(["keep"]);
    expect(h.api.listSessions).toHaveBeenCalledWith("u-1");
  });

  it("declines a server runner's history and a conversation for one", async () => {
    const h = harness();
    const transport = complete(transportFor(h));
    await expect(transport.listChatSessions("ws-id", "r-1")).resolves.toBeNull();
    await expect(transport.createChatSession({ workspace: "ws-id", runner: "r-1" })).resolves.toBeNull();
    // The store's own row names a server runner, so its transcript is the server's.
    await expect(transport.listChatMessages("s-1")).resolves.toBeNull();
    await expect(transport.ownsChatSession("s-1")).resolves.toBe(false);
    expect(h.api.listSessions).not.toHaveBeenCalled();
    expect(h.api.createSession).not.toHaveBeenCalled();
    expect(h.api.listEvents).not.toHaveBeenCalled();
  });

  it("opens a conversation in the store and maps its transcript", async () => {
    const h = harness({ getSession: vi.fn(async () => record({ project: BUILTIN_CHAT_ID })) });
    const transport = complete(transportFor(h));
    await expect(transport.createChatSession({ workspace: "ws-id", runner: BUILTIN_CHAT_ID })).resolves.toMatchObject({
      id: "new",
      runner: BUILTIN_CHAT_ID,
    });
    expect(h.api.createSession).toHaveBeenCalledWith("u-1", { workspace: "ws-id", runner: BUILTIN_CHAT_ID });
    const messages = await transport.listChatMessages("s-1");
    expect(messages).toHaveLength(1);
    expect(h.api.listEvents).toHaveBeenCalledWith("u-1", "s-1");
    await expect(transport.ownsChatSession("s-1")).resolves.toBe(true);
  });

  it("warms with the session's working directory, mode and thread", async () => {
    const h = harness({ getSession: vi.fn(async () => record({ engine_thread_id: "t-1" })) });
    const transport = complete(transportFor(h));
    transport.setApprovalMode("s-1", "workspace");
    await transport.warmChatSession("s-1");
    expect(h.ensureRuntime).toHaveBeenCalledTimes(1);
    expect(h.api.warm).toHaveBeenCalledWith({
      workspaceSlug: "acme",
      sessionId: "s-1",
      cwd: "/repo",
      mode: "workspace",
      localThreadId: "t-1",
    });
  });

  it("skips warming a conversation the store no longer carries", async () => {
    const h = harness({ getSession: vi.fn(async () => null) });
    await complete(transportFor(h)).warmChatSession("gone");
    expect(h.ensureRuntime).not.toHaveBeenCalled();
    expect(h.api.warm).not.toHaveBeenCalled();
  });

  it("persists the user turn before asking the engine, then releases the session at the end", async () => {
    const h = harness();
    const transport = transportFor(h);
    const order: string[] = [];
    vi.mocked(h.api.appendEvent).mockImplementation(async (_a, sessionId, input) => {
      order.push("append");
      return { ...event("e-2", sessionId), role: input.role, content: input.content };
    });
    vi.mocked(h.api.send).mockImplementation(async () => {
      order.push("send");
    });
    const sent = await transport.sendChatMessage("s-1", "hello");
    expect(order).toEqual(["append", "send"]);
    expect(sent).toMatchObject({ content: "hello", role: "user" });
    expect(h.api.send).toHaveBeenCalledWith({
      workspaceSlug: "acme",
      sessionId: "s-1",
      messageId: expect.any(String),
      content: "hello",
      cwd: "/repo",
      mode: "full_access",
      localThreadId: undefined,
    });
  });

  it("refuses a second turn while one runs", async () => {
    const h = harness();
    const transport = transportFor(h);
    await transport.sendChatMessage("s-1", "first");
    await expect(transport.sendChatMessage("s-1", "second")).rejects.toThrow(TURN_BUSY_MESSAGE);
    // A terminal frame releases the session, even with no page subscribed.
    h.fire({ result: "chat_closed", data: { chat_session_id: "s-1" } });
    await expect(transport.sendChatMessage("s-1", "again")).resolves.toBeDefined();
  });

  it("releases a session whose submit failed", async () => {
    const h = harness({ send: vi.fn(async () => Promise.reject(new Error("daemon gone"))) });
    const transport = transportFor(h);
    await expect(transport.sendChatMessage("s-1", "x")).rejects.toThrow("daemon gone");
    vi.mocked(h.api.send).mockResolvedValue(undefined);
    await expect(transport.sendChatMessage("s-1", "y")).resolves.toBeDefined();
  });

  it("sends at full access when the session row has vanished, because no runner is known", async () => {
    const h = harness({ getSession: vi.fn(async () => null) });
    const transport = transportFor(h);
    transport.setRunnerApprovalMode("r-1", "workspace");
    await transport.sendChatMessage("s-1", "hello");
    expect(h.api.send).toHaveBeenCalledWith(expect.objectContaining({ mode: "full_access", cwd: undefined }));
  });

  it("falls back to full access when neither a session nor a runner mode is known", async () => {
    const h = harness();
    const transport = complete(transportFor(h));
    await transport.warmChatSession("s-1");
    expect(h.api.warm).toHaveBeenCalledWith(expect.objectContaining({ mode: DEFAULT_APPROVAL_MODE }));
  });

  it("uses the runner's chosen mode on the next turn", async () => {
    const h = harness();
    const transport = transportFor(h);
    transport.setRunnerApprovalMode("r-1", "ask");
    await transport.sendChatMessage("s-1", "hi");
    expect(h.api.send).toHaveBeenCalledWith(expect.objectContaining({ mode: "ask" }));
  });

  it("falls back to the mode stored for the runner when the page has not handed one over", async () => {
    // The page created this conversation before it loaded its own choice.
    const h = harness();
    const transport = complete(transportFor(h, "u-1", (runnerId) => (runnerId === "r-1" ? "workspace" : "ask")));
    await transport.warmChatSession("s-1");
    expect(h.api.warm).toHaveBeenCalledWith(expect.objectContaining({ mode: "workspace" }));
    await transport.sendChatMessage("s-1", "hi");
    expect(h.api.send).toHaveBeenCalledWith(expect.objectContaining({ mode: "workspace" }));
  });

  it("prefers the page's runner and session choices over the stored one", async () => {
    const h = harness({ getSession: vi.fn(async () => record({ project: "r-2" })) });
    const transport = complete(transportFor(h, "u-1", (runnerId) => (runnerId === "r-2" ? "ask" : "full_access")));
    transport.setRunnerApprovalMode("r-2", "workspace");
    await transport.warmChatSession("s-1");
    expect(h.api.warm).toHaveBeenCalledWith(expect.objectContaining({ mode: "workspace" }));
    transport.setApprovalMode("s-1", "ask");
    await transport.sendChatMessage("s-1", "hi");
    expect(h.api.send).toHaveBeenCalledWith(expect.objectContaining({ mode: "ask" }));
  });

  it("cancels and decides with the workspace and the user's words", async () => {
    const h = harness();
    const transport = transportFor(h);
    await expect(transport.cancelChat("s-1", "stop")).resolves.toEqual({ ok: true });
    expect(h.api.cancel).toHaveBeenCalledWith({ workspaceSlug: "acme", sessionId: "s-1", reason: "stop" });
    await transport.decideChatApproval("s-1", "a-1", "accept_for_session");
    expect(h.api.decide).toHaveBeenCalledWith({
      workspaceSlug: "acme",
      sessionId: "s-1",
      localApprovalId: "a-1",
      decision: "accept_for_session",
    });
  });

  it("renames a stored conversation with the trimmed name", async () => {
    const h = harness({ getSession: vi.fn(async () => record({ project: BUILTIN_CHAT_ID })) });
    const transport = transportFor(h);
    await transport.renameChatSession("s-1", "  Planning  ");
    expect(h.api.renameSession).toHaveBeenCalledWith("u-1", "s-1", "Planning");
  });

  it("refuses a blank name without asking the store", async () => {
    const h = harness({ getSession: vi.fn(async () => record({ project: BUILTIN_CHAT_ID })) });
    await expect(transportFor(h).renameChatSession("s-1", "   ")).rejects.toThrow(CHAT_TITLE_REQUIRED_MESSAGE);
    expect(h.api.renameSession).not.toHaveBeenCalled();
  });

  it("refuses to rename a conversation this store does not carry", async () => {
    const h = harness({ getSession: vi.fn(async () => record({ project: "r-1" })) });
    await expect(transportFor(h).renameChatSession("s-1", "Planning")).rejects.toThrow(CHAT_SESSION_GONE_MESSAGE);
    expect(h.api.renameSession).not.toHaveBeenCalled();
  });

  it("deletes a conversation, its messages and its live listeners", async () => {
    const h = harness();
    const transport = transportFor(h);
    const next = handlers();
    transport.subscribeChatEvents("s-1", 0, next);
    h.fire(started());
    await transport.deleteChatSession("s-1");
    expect(h.api.deleteSession).toHaveBeenCalledWith("u-1", "s-1");
    // Nothing is delivered for a conversation that no longer exists.
    h.fire({ result: "chat_message_completed", data: { chat_session_id: "s-1", message_id: "m-1", status: "done" } });
    expect(next.onEvent).toHaveBeenCalledTimes(1);
    // A second delete is the store's own idempotent no-op and resolves.
    await expect(transport.deleteChatSession("s-1")).resolves.toBeUndefined();
  });

  it("clears the account's history and forgets every in-flight turn", async () => {
    const h = harness();
    const transport = transportFor(h);
    await transport.clearChatHistory();
    expect(h.api.clearHistory).toHaveBeenCalledWith("u-1");
    // With the in-flight turn forgotten, the same conversation takes a send.
    await transport.sendChatMessage("s-1", "again");
    expect(h.api.send).toHaveBeenCalledTimes(1);
  });

  it("clearing requires the same signed-in account as every other store verb", async () => {
    const h = harness();
    await expect(transportFor(h, "").clearChatHistory()).rejects.toThrow(/Sign in/);
    expect(h.api.clearHistory).not.toHaveBeenCalled();
  });

  it("asks for the working copy with the account, workspace id and engine id", async () => {
    const h = harness();
    await expect(transportFor(h).chatWorkingDir("ws-id", BUILTIN_CHAT_ID)).resolves.toBe("/repo");
    expect(h.api.workingDir).toHaveBeenCalledWith("u-1", "ws-id", BUILTIN_CHAT_ID);
    await expect(transportFor(h).chatWorkingDir("ws-id", "r-other")).rejects.toThrow(CHAT_SESSION_GONE_MESSAGE);
  });

  it("refuses a working copy without a workspace id", async () => {
    const h = harness();
    await expect(transportFor(h).chatWorkingDir("", BUILTIN_CHAT_ID)).rejects.toThrow(/Open a workspace/);
    expect(h.api.workingDir).not.toHaveBeenCalled();
  });

  it("closes, releases the session and answers the stored row", async () => {
    const h = harness();
    const transport = transportFor(h);
    await expect(transport.closeChat("s-1")).resolves.toMatchObject({ id: "s-1", status: "open" });
    expect(h.api.close).toHaveBeenCalledWith({ workspaceSlug: "acme", sessionId: "s-1" });
    expect(h.api.getSession).toHaveBeenCalledTimes(1);
  });

  it("re-reads a conversation that vanished to report the close honestly", async () => {
    const h = harness({ getSession: vi.fn(async () => null) });
    await expect(transportFor(h).closeChat("gone")).rejects.toThrow(/no longer in the local store/);
  });

  it("keeps the thread id written by a turn's start frame", async () => {
    const h = harness();
    const transport = transportFor(h);
    const next = handlers();
    transport.subscribeChatEvents("s-1", 0, next);
    h.fire(started());
    await Promise.resolve();
    expect(h.api.setThreadId).toHaveBeenCalledWith("u-1", "s-1", "t-1");
    expect(next.onEvent).toHaveBeenCalledWith(expect.objectContaining({ kind: "turn_started" }));
  });

  it("stores the reply before announcing the completed turn, falling back to the deltas", async () => {
    const h = harness();
    const transport = transportFor(h);
    const next = handlers();
    const order: string[] = [];
    vi.mocked(h.api.appendEvent).mockImplementation(async (a, sessionId, input) => {
      order.push(`store:${input.content}`);
      return { ...event("e-3", sessionId), role: input.role, content: input.content };
    });
    next.onEvent.mockImplementation((eventValue: { kind: string }) => {
      order.push(`event:${eventValue.kind}`);
    });
    transport.subscribeChatEvents("s-1", 0, next);
    h.fire({ result: "chat_message_started", data: { chat_session_id: "s-1", message_id: "m-1" } });
    h.fire({
      result: "chat_event",
      data: { chat_session_id: "s-1", kind: "assistant_delta", payload: { params: { delta: "hel" } } },
    });
    h.fire({
      result: "chat_event",
      data: { chat_session_id: "s-1", kind: "assistant_delta", payload: { params: { text: "lo" } } },
    });
    h.fire({
      result: "chat_message_completed",
      data: { chat_session_id: "s-1", message_id: "m-1", status: "ok" },
    });
    await vi.waitFor(() =>
      expect(next.onEvent).toHaveBeenCalledWith(expect.objectContaining({ kind: "turn_completed" }))
    );
    expect(order.at(-2)).toBe("store:hello");
    expect(order.at(-1)).toBe("event:turn_completed");
    expect(h.api.getSession).not.toHaveBeenCalled();
  });

  it("prefers the reply the engine echoed in its completed frame", async () => {
    const h = harness();
    const transport = transportFor(h);
    transport.subscribeChatEvents("s-1", 0, handlers());
    h.fire({
      result: "chat_message_completed",
      data: { chat_session_id: "s-1", message_id: "m-1", status: "ok", assistant_message: "final text" },
    });
    await vi.waitFor(() => expect(h.api.appendEvent).toHaveBeenCalled());
    expect(h.api.appendEvent).toHaveBeenCalledWith("u-1", "s-1", { role: "assistant", content: "final text" });
  });

  it("stores nothing for an empty reply and still announces the turn", async () => {
    const h = harness();
    const transport = transportFor(h);
    const next = handlers();
    transport.subscribeChatEvents("s-1", 0, next);
    h.fire({
      result: "chat_message_completed",
      data: { chat_session_id: "s-1", message_id: "m-1", status: "ok" },
    });
    await vi.waitFor(() =>
      expect(next.onEvent).toHaveBeenCalledWith(expect.objectContaining({ kind: "turn_completed" }))
    );
    expect(h.api.appendEvent).not.toHaveBeenCalled();
  });

  it("survives a failed reply write and still announces the turn", async () => {
    const h = harness({
      appendEvent: vi.fn(async () => Promise.reject(new Error("disk full"))),
    });
    const transport = transportFor(h);
    const next = handlers();
    transport.subscribeChatEvents("s-1", 0, next);
    h.fire({
      result: "chat_message_completed",
      data: { chat_session_id: "s-1", message_id: "m-1", status: "ok", assistant_message: "kept" },
    });
    await vi.waitFor(() =>
      expect(next.onEvent).toHaveBeenCalledWith(expect.objectContaining({ kind: "turn_completed" }))
    );
  });

  it("survives a failed thread-id write", async () => {
    const h = harness({ setThreadId: vi.fn(async () => Promise.reject(new Error("locked"))) });
    const transport = transportFor(h);
    const next = handlers();
    transport.subscribeChatEvents("s-1", 0, next);
    h.fire(started());
    await vi.waitFor(() =>
      expect(next.onEvent).toHaveBeenCalledWith(expect.objectContaining({ kind: "turn_started" }))
    );
  });

  it("routes frames and failures only to the session they name", () => {
    const h = harness();
    const transport = transportFor(h);
    const one = handlers();
    const two = handlers();
    transport.subscribeChatEvents("s-1", 0, one);
    transport.subscribeChatEvents("s-2", 0, two);
    h.fire({ result: "chat_event", data: { chat_session_id: "s-2", kind: "tool_call", payload: {} } });
    expect(one.onEvent).not.toHaveBeenCalled();
    expect(two.onEvent).toHaveBeenCalledTimes(1);
    h.fail({ chat_session_id: "s-1", message: "socket closed" });
    expect(one.onError).toHaveBeenCalledWith(new Error("socket closed"));
    expect(two.onError).not.toHaveBeenCalled();
  });

  it("stops delivering after unsubscribe, twice-safe", () => {
    const h = harness();
    const transport = transportFor(h);
    const next = handlers();
    const stop = transport.subscribeChatEvents("s-1", 0, next);
    stop();
    stop();
    h.fire({ result: "chat_closed", data: { chat_session_id: "s-1" } });
    expect(next.onEvent).not.toHaveBeenCalled();
  });

  it("holds a second turn back only until the turn's own completion", async () => {
    const h = harness();
    const transport = transportFor(h);
    await transport.sendChatMessage("s-1", "first");
    h.fire({
      result: "chat_message_completed",
      data: { chat_session_id: "s-1", message_id: "m-1", status: "ok" },
    });
    await expect(transport.sendChatMessage("s-1", "second")).resolves.toBeDefined();
  });

  it("holds listing back to the store until a turn or subscription opens the channels", async () => {
    const h = harness();
    const subscribe = vi.spyOn(h.api, "subscribe");
    await complete(transportFor(h)).listChatSessions("ws-id", "r-1");
    expect(subscribe).not.toHaveBeenCalled();
  });
});
