// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Runner-chat reads (NEWFRONT-549, RUN-026/032): keys stay in the runners
// subtree, the runner and session lists poll, and the transcript is read
// only for a selected session.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { renderHook, waitFor } from "@testing-library/react";
import type { ApiClient } from "@pidash/api-client";
import { createElement, type ReactNode } from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

const contracts = vi.hoisted(() => ({
  getRunner: vi.fn(),
  listChatSessions: vi.fn(),
  getChatSession: vi.fn(),
  listChatMessages: vi.fn(),
}));
vi.mock("@pidash/api-client", async (importOriginal) => {
  const original = await importOriginal<typeof import("@pidash/api-client")>();
  return { ...original, ...contracts };
});

const { createAppClient, resetAppClient } = await import("../../../core/api/client.js");
const { setChatTransport } = await import("./transport.js");
type ChatTransport = import("./transport.js").ChatTransport;
const { chatKeys } = await import("./keys.js");
const {
  CHAT_RUNNER_POLL_INTERVAL_MS,
  CHAT_SESSIONS_POLL_INTERVAL_MS,
  CHAT_WORKING_DIR_STALE_MS,
  useChatWorkingDir,
  chatMessagesQueryOptions,
  chatRunnerQueryOptions,
  chatSessionQueryOptions,
  chatSessionsQueryOptions,
  useChatMessages,
  useChatRunner,
  useChatSessions,
} = await import("./queries.js");

const client = {} as ApiClient;

afterEach(() => {
  Object.values(contracts).forEach((mock) => mock.mockReset());
  resetAppClient();
  setChatTransport(null);
});

describe("chatKeys", () => {
  it("hangs every chat entry inside the runners subtree", () => {
    expect(chatKeys.all("acme")).toEqual(["ws", "acme", "runners", "chat"]);
    expect(chatKeys.runner("acme", "r-1")).toEqual(["ws", "acme", "runners", "chat", "runner", "r-1"]);
    expect(chatKeys.sessions("acme", "r-1")).toEqual(["ws", "acme", "runners", "chat", "sessions", "r-1"]);
    expect(chatKeys.messages("acme", "s-1")).toEqual(["ws", "acme", "runners", "chat", "messages", "s-1"]);
    expect(chatKeys.workingDir("r-1")).toEqual(["runners", "chat", "working-dir", "r-1"]);
  });
});

describe("query options", () => {
  it("polls the runner detail and the session history", async () => {
    contracts.getRunner.mockResolvedValue({ id: "r-1" });
    contracts.listChatSessions.mockResolvedValue([]);
    const runner = chatRunnerQueryOptions(client, "acme", "r-1");
    expect(runner.refetchInterval).toBe(CHAT_RUNNER_POLL_INTERVAL_MS);
    expect(runner.staleTime).toBe(0);
    await runner.queryFn?.({} as never);
    expect(contracts.getRunner).toHaveBeenCalledWith(client, "r-1");

    const sessions = chatSessionsQueryOptions(client, "acme", "ws-id", "r-1");
    expect(sessions.refetchInterval).toBe(CHAT_SESSIONS_POLL_INTERVAL_MS);
    expect(sessions.queryKey).toEqual(chatKeys.sessions("acme", "r-1"));
    await sessions.queryFn?.({} as never);
    expect(contracts.listChatSessions).toHaveBeenCalledWith(client, { workspace: "ws-id", runner: "r-1" });
  });

  it("reads one session and one transcript", async () => {
    contracts.getChatSession.mockResolvedValue({ id: "s-1" });
    contracts.listChatMessages.mockResolvedValue([]);
    const session = chatSessionQueryOptions(client, "acme", "s-1");
    expect(session.queryKey).toEqual([...chatKeys.all("acme"), "session", "s-1"]);
    await session.queryFn?.({} as never);
    expect(contracts.getChatSession).toHaveBeenCalledWith(client, "s-1");
    const messages = chatMessagesQueryOptions(client, "acme", "s-1");
    expect(messages.queryKey).toEqual(chatKeys.messages("acme", "s-1"));
    await messages.queryFn?.({} as never);
    expect(contracts.listChatMessages).toHaveBeenCalledWith(client, "s-1");
  });
});

function provider(queryClient: QueryClient) {
  return ({ children }: { children: ReactNode }): React.ReactElement =>
    createElement(QueryClientProvider, { client: queryClient }, children);
}

describe("useChatRunner", () => {
  function mount(enabled: boolean) {
    const wrapper = provider(new QueryClient({ defaultOptions: { queries: { retry: false } } }));
    return renderHook(() => useChatRunner("acme", "r-1", enabled), { wrapper });
  }

  it("reads the runner through the app client when enabled", async () => {
    createAppClient("https://api.test", {} as never);
    contracts.getRunner.mockResolvedValue({ id: "r-1", name: "alpha" });
    const { result } = mount(true);
    await waitFor(() => expect(result.current.data).toMatchObject({ id: "r-1" }));
    expect(contracts.getRunner).toHaveBeenCalledTimes(1);
  });

  it("stays idle when disabled", async () => {
    createAppClient("https://api.test", {} as never);
    mount(false);
    await new Promise((resolve) => setTimeout(resolve, 10));
    expect(contracts.getRunner).not.toHaveBeenCalled();
  });

  it("takes the sessions hook view of the workspace scope and runner", async () => {
    createAppClient("https://api.test", {} as never);
    contracts.listChatSessions.mockResolvedValue([{ id: "s-1" }]);
    const wrapper = provider(new QueryClient({ defaultOptions: { queries: { retry: false } } }));
    const { result } = renderHook(() => useChatSessions("acme", "ws-id", "r-1"), { wrapper });
    await waitFor(() => expect(result.current.data).toEqual([{ id: "s-1" }]));
    expect(contracts.listChatSessions).toHaveBeenCalledWith(expect.anything(), { workspace: "ws-id", runner: "r-1" });
  });
});

describe("useChatMessages", () => {
  it("reads one transcript only once a session is selected", async () => {
    createAppClient("https://api.test", {} as never);
    contracts.listChatMessages.mockResolvedValue([{ id: "m-1" }]);
    const wrapper = provider(new QueryClient({ defaultOptions: { queries: { retry: false } } }));
    const { result } = renderHook(() => useChatMessages("acme", "s-1"), { wrapper });
    await waitFor(() => expect(result.current.data).toEqual([{ id: "m-1" }]));
    expect(contracts.listChatMessages).toHaveBeenCalledWith(expect.anything(), "s-1");
  });

  it("stays idle without a session or without access", async () => {
    createAppClient("https://api.test", {} as never);
    const wrapper = provider(new QueryClient({ defaultOptions: { queries: { retry: false } } }));
    const none = renderHook(() => useChatMessages("acme", null), { wrapper });
    const denied = renderHook(() => useChatMessages("acme", "s-1", false), { wrapper });
    await new Promise((resolve) => setTimeout(resolve, 10));
    expect(none.result.current.fetchStatus).toBe("idle");
    expect(denied.result.current.fetchStatus).toBe("idle");
    expect(contracts.listChatMessages).not.toHaveBeenCalled();
  });
});

describe("useChatWorkingDir", () => {
  function localTransport(workingDir: () => Promise<string>): ChatTransport {
    return {
      subscribeChatEvents: vi.fn(() => () => undefined),
      ownsRunner: (runnerId: string) => runnerId === "pidash-builtin",
      deleteChatSession: vi.fn(async () => undefined),
      chatWorkingDir: vi.fn(workingDir),
    } as unknown as ChatTransport;
  }

  it("reads the machine's directory for a local contact and caches it for a while", async () => {
    setChatTransport(localTransport(async () => "/home/me/chat/acme/PROJ"));
    const wrapper = provider(new QueryClient({ defaultOptions: { queries: { retry: false } } }));
    const { result } = renderHook(() => useChatWorkingDir("pidash-builtin"), { wrapper });
    await waitFor(() => expect(result.current.data).toBe("/home/me/chat/acme/PROJ"));
    expect(CHAT_WORKING_DIR_STALE_MS).toBeGreaterThan(0);
  });

  it("stays idle without the history capability or without access", async () => {
    setChatTransport(null);
    const wrapper = provider(new QueryClient({ defaultOptions: { queries: { retry: false } } }));
    const none = renderHook(() => useChatWorkingDir("pidash-builtin"), { wrapper });
    setChatTransport(localTransport(async () => "/repo"));
    const denied = renderHook(() => useChatWorkingDir("pidash-builtin", false), { wrapper });
    await new Promise((resolve) => setTimeout(resolve, 10));
    expect(none.result.current.fetchStatus).toBe("idle");
    expect(denied.result.current.fetchStatus).toBe("idle");
  });

  it("stays quiet when the machine cannot answer, leaving the header its own value", async () => {
    setChatTransport(localTransport(async () => Promise.reject(new Error("no store"))));
    const wrapper = provider(new QueryClient({ defaultOptions: { queries: { retry: false } } }));
    const { result } = renderHook(() => useChatWorkingDir("pidash-builtin"), { wrapper });
    await waitFor(() => expect(result.current.isError).toBe(true));
    expect(result.current.data).toBeUndefined();
  });
});
