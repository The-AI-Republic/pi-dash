// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// New chat (NEWFRONT-549, RUN-026/028): creates, publishes then selects,
// reports the working state, and toasts with the server's reason on
// failure while leaving the history untouched. The page's warm-up is the
// only thing that prepares the created session.
import { act, renderHook, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const contracts = vi.hoisted(() => ({ createChatSession: vi.fn() }));
const toasts = vi.hoisted(() => ({ notifyError: vi.fn() }));
vi.mock("@pidash/api-client", async (importOriginal) => {
  const original = await importOriginal<typeof import("@pidash/api-client")>();
  return { ...original, ...contracts };
});
vi.mock("../../../shared/shell/index.js", async (importOriginal) => {
  const original = await importOriginal<typeof import("../../../shared/shell/index.js")>();
  return { ...original, ...toasts };
});

const { ApiError } = await import("@pidash/api-client");
const { createAppClient, resetAppClient } = await import("../../../core/api/client.js");
const { chatKeys } = await import("./keys.js");
const { makeSession, testClient, wrapper } = await import("./harness.js");
const { chatFailureReason, NEW_CHAT_FAILED_BODY, NEW_CHAT_FAILED_TITLE, useNewChat } = await import("./useNewChat.js");

beforeEach(() => {
  createAppClient("https://api.test", {} as never);
});

afterEach(() => {
  contracts.createChatSession.mockReset();
  toasts.notifyError.mockReset();
  resetAppClient();
});

function props(overrides: Record<string, unknown> = {}) {
  return {
    workspaceSlug: "acme",
    workspaceId: "ws-id",
    runnerId: "r-1",
    onSelect: vi.fn(),
    onCreated: vi.fn(),
    busy: false,
    ...overrides,
  };
}

function deferred<T>() {
  let resolve: (value: T) => void = () => undefined;
  let reject: (reason: unknown) => void = () => undefined;
  const promise = new Promise<T>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

describe("chatFailureReason", () => {
  it("prefers the transport's message and falls back to the generic line", () => {
    expect(chatFailureReason(new Error("Runner is not reachable"))).toBe("Runner is not reachable");
    expect(chatFailureReason(new Error(""))).toBe(NEW_CHAT_FAILED_BODY);
    expect(chatFailureReason("nope")).toBe(NEW_CHAT_FAILED_BODY);
  });
});

describe("useNewChat", () => {
  it("creates, publishes and selects without warming", async () => {
    const created = makeSession({ id: "fresh", last_message_at: null });
    contracts.createChatSession.mockResolvedValue(created);
    const client = testClient();
    client.setQueryData(chatKeys.sessions("acme", "r-1"), [makeSession({ id: "old" })]);
    const input = props();
    const { result } = renderHook(() => useNewChat(input), { wrapper: wrapper(client) });
    await act(async () => {
      await result.current.start();
    });
    expect(contracts.createChatSession).toHaveBeenCalledWith(expect.anything(), { workspace: "ws-id", runner: "r-1" });
    expect(input.onCreated).toHaveBeenCalledWith(created);
    expect(input.onSelect).toHaveBeenCalledWith("fresh");
    expect(client.getQueryData(chatKeys.sessions("acme", "r-1"))).toEqual([
      expect.objectContaining({ id: "fresh" }),
      expect.objectContaining({ id: "old" }),
    ]);
    expect(result.current.busy).toBe(false);
  });

  it("does not duplicate a session the list already holds", async () => {
    const created = makeSession({ id: "fresh" });
    contracts.createChatSession.mockResolvedValue(created);
    const client = testClient();
    client.setQueryData(chatKeys.sessions("acme", "r-1"), [created]);
    const { result } = renderHook(() => useNewChat(props()), { wrapper: wrapper(client) });
    await act(async () => {
      await result.current.start();
    });
    expect(client.getQueryData(chatKeys.sessions("acme", "r-1"))).toHaveLength(1);
  });

  it("shows the working state and refuses overlapping creates", async () => {
    let resolveCreate: (value: ReturnType<typeof makeSession>) => void = () => undefined;
    contracts.createChatSession.mockImplementation(() => new Promise((resolve) => (resolveCreate = resolve as never)));
    const input = props();
    const { result } = renderHook(() => useNewChat(input), { wrapper: wrapper(testClient()) });
    let first: Promise<void> = Promise.resolve();
    act(() => {
      first = result.current.start();
    });
    await waitFor(() => expect(result.current.busy).toBe(true));
    await act(async () => {
      await result.current.start();
    });
    expect(contracts.createChatSession).toHaveBeenCalledTimes(1);
    await act(async () => {
      resolveCreate(makeSession({ id: "fresh" }));
      await first;
    });
    expect(result.current.busy).toBe(false);
  });

  it("does nothing while the caller says a create is already running", async () => {
    const { result } = renderHook(() => useNewChat(props({ busy: true })), { wrapper: wrapper(testClient()) });
    expect(result.current.busy).toBe(true);
    await act(async () => {
      await result.current.start();
    });
    expect(contracts.createChatSession).not.toHaveBeenCalled();
  });

  it("drops a create that resolves after the viewer switched contacts", async () => {
    const pending = deferred<ReturnType<typeof makeSession>>();
    contracts.createChatSession.mockReturnValue(pending.promise);
    const client = testClient();
    client.setQueryData(chatKeys.sessions("acme", "r-1"), [makeSession({ id: "old" })]);
    const first = props();
    const { result, rerender } = renderHook((input: ReturnType<typeof props>) => useNewChat(input), {
      wrapper: wrapper(client),
      initialProps: first,
    });
    act(() => {
      void result.current.start();
    });
    await waitFor(() => expect(result.current.busy).toBe(true));

    const second = props({ runnerId: "r-2" });
    rerender(second);
    expect(result.current.busy).toBe(false);

    await act(async () => {
      pending.resolve(makeSession({ id: "fresh", runner: "r-1" }));
    });

    expect(first.onCreated).not.toHaveBeenCalled();
    expect(first.onSelect).not.toHaveBeenCalled();
    expect(client.getQueryData(chatKeys.sessions("acme", "r-1"))).toEqual([expect.objectContaining({ id: "old" })]);
    expect(toasts.notifyError).not.toHaveBeenCalled();
  });

  it("stays quiet when a failed create belongs to the contact left behind", async () => {
    const pending = deferred<ReturnType<typeof makeSession>>();
    contracts.createChatSession.mockReturnValue(pending.promise);
    const first = props();
    const { result, rerender } = renderHook((input: ReturnType<typeof props>) => useNewChat(input), {
      wrapper: wrapper(testClient()),
      initialProps: first,
    });
    act(() => {
      void result.current.start();
    });
    await waitFor(() => expect(result.current.busy).toBe(true));
    rerender(props({ runnerId: "r-2" }));

    await act(async () => {
      pending.reject(new ApiError({ status: 500, code: "http", message: "runner gone" }));
    });
    expect(toasts.notifyError).not.toHaveBeenCalled();
  });

  it("toasts the server's reason and keeps history unchanged on failure", async () => {
    contracts.createChatSession.mockRejectedValue(
      new ApiError({ status: 500, code: "http", message: "parity session-create failure" })
    );
    const client = testClient();
    const existing = [makeSession({ id: "old" })];
    client.setQueryData(chatKeys.sessions("acme", "r-1"), existing);
    const input = props();
    const { result } = renderHook(() => useNewChat(input), { wrapper: wrapper(client) });
    await act(async () => {
      await result.current.start();
    });
    expect(toasts.notifyError).toHaveBeenCalledWith(NEW_CHAT_FAILED_TITLE, "parity session-create failure");
    expect(input.onSelect).not.toHaveBeenCalled();
    expect(client.getQueryData(chatKeys.sessions("acme", "r-1"))).toEqual(existing);
    expect(result.current.busy).toBe(false);
  });
});
