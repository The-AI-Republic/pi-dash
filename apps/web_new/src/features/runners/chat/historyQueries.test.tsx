// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// On-machine history writes (NEWFRONT-589, DESK-009): each verb reaches
// the installed transport, the history list is re-read afterwards whether
// the write landed or failed, a removed conversation's transcript leaves
// the cache, and a failure is reported instead of reading as success.
import { QueryClientProvider } from "@tanstack/react-query";
import { act, renderHook, waitFor } from "@testing-library/react";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const toasts = vi.hoisted(() => ({ notifyError: vi.fn(), notifySuccess: vi.fn() }));
vi.mock("../../../shared/shell/index.js", async (importOriginal) => {
  const original = await importOriginal<typeof import("../../../shared/shell/index.js")>();
  return { ...original, ...toasts };
});

const { testClient } = await import("./harness.js");
const { chatKeys } = await import("./keys.js");
const { setChatTransport } = await import("./transport.js");
type ChatTransport = import("./transport.js").ChatTransport;
const {
  CLEARED_TOAST,
  CLEAR_FAILED_TITLE,
  DELETE_FAILED_TITLE,
  HISTORY_FAILED_BODY,
  RENAMED_TOAST,
  supportsLocalHistory,
  useClearHistory,
  useDeleteChat,
  useRenameChat,
} = await import("./historyQueries.js");

const Provider = QueryClientProvider as unknown as React.ComponentType<{
  client: ReturnType<typeof testClient>;
  children?: React.ReactNode;
}>;

const local = (extra: Partial<ChatTransport> = {}): ChatTransport =>
  ({
    subscribeChatEvents: vi.fn(() => () => undefined),
    ownsRunner: (runnerId: string) => runnerId === "pidash-builtin",
    ...extra,
  }) as unknown as ChatTransport;

const SESSIONS_KEY = chatKeys.sessions("acme", "pidash-builtin");
const MESSAGES_KEY = chatKeys.messages("acme", "s-1");

function setup<T>(hook: () => T) {
  const client = testClient();
  const view = renderHook(hook, { wrapper: ({ children }) => <Provider client={client}>{children}</Provider> });
  return { client, view };
}

beforeEach(() => {
  toasts.notifyError.mockReset();
  toasts.notifySuccess.mockReset();
  setChatTransport(null);
});

afterEach(() => {
  setChatTransport(null);
});

describe("supportsLocalHistory", () => {
  it("is true only when the installed transport can delete a conversation", () => {
    expect(supportsLocalHistory("pidash-builtin")).toBe(false);
    setChatTransport(local({ deleteChatSession: vi.fn(async () => undefined) }));
    expect(supportsLocalHistory("pidash-builtin")).toBe(true);
    // A server runner keeps the cloud surface even on the desktop.
    expect(supportsLocalHistory("r-1")).toBe(false);
  });
});

describe("useRenameChat", () => {
  it("renames through the transport and re-reads the history", async () => {
    const renameChatSession = vi.fn(async () => undefined);
    setChatTransport(local({ renameChatSession }));
    const { client, view } = setup(() => useRenameChat("acme", "pidash-builtin"));
    const invalidate = vi.spyOn(client, "invalidateQueries");
    await act(async () => view.result.current.mutateAsync({ sessionId: "s-1", title: "Planning" }));
    expect(renameChatSession).toHaveBeenCalledWith("s-1", "Planning");
    expect(toasts.notifySuccess).toHaveBeenCalledWith(RENAMED_TOAST);
    expect(invalidate).toHaveBeenCalledWith({ queryKey: SESSIONS_KEY });
  });

  it("reports a surface that cannot rename instead of claiming success", async () => {
    const { view } = setup(() => useRenameChat("acme", "pidash-builtin"));
    await act(async () => {
      await expect(view.result.current.mutateAsync({ sessionId: "s-1", title: "Planning" })).rejects.toThrow(
        HISTORY_FAILED_BODY
      );
    });
    expect(toasts.notifySuccess).not.toHaveBeenCalled();
    expect(toasts.notifyError).toHaveBeenCalledWith("Chat not renamed", HISTORY_FAILED_BODY);
  });

  it("surfaces the store's own refusal and still re-reads the history", async () => {
    setChatTransport(local({ renameChatSession: vi.fn(async () => Promise.reject(new Error("gone"))) }));
    const { client, view } = setup(() => useRenameChat("acme", "pidash-builtin"));
    const invalidate = vi.spyOn(client, "invalidateQueries");
    await act(async () => {
      await expect(view.result.current.mutateAsync({ sessionId: "s-1", title: "Planning" })).rejects.toThrow("gone");
    });
    expect(toasts.notifyError).toHaveBeenCalledWith("Chat not renamed", "gone");
    expect(invalidate).toHaveBeenCalledWith({ queryKey: SESSIONS_KEY });
  });

  it("substitutes its own sentence for a failure that carries no message", async () => {
    setChatTransport(local({ renameChatSession: vi.fn(async () => Promise.reject("nope")) }));
    const { view } = setup(() => useRenameChat("acme", "pidash-builtin"));
    await act(async () => {
      await expect(view.result.current.mutateAsync({ sessionId: "s-1", title: "Planning" })).rejects.toBeDefined();
    });
    expect(toasts.notifyError).toHaveBeenCalledWith("Chat not renamed", HISTORY_FAILED_BODY);
  });
});

describe("useDeleteChat", () => {
  it("deletes, re-reads the history and drops the transcript", async () => {
    const deleteChatSession = vi.fn(async () => undefined);
    setChatTransport(local({ deleteChatSession }));
    const { client, view } = setup(() => useDeleteChat("acme", "pidash-builtin"));
    client.setQueryData(MESSAGES_KEY, [{ id: "m-1" }]);
    const invalidate = vi.spyOn(client, "invalidateQueries");
    const remove = vi.spyOn(client, "removeQueries");
    await act(async () => view.result.current.mutateAsync({ sessionId: "s-1" }));
    expect(deleteChatSession).toHaveBeenCalledWith("s-1");
    expect(invalidate).toHaveBeenCalledWith({ queryKey: SESSIONS_KEY });
    expect(remove).toHaveBeenCalledWith({ queryKey: MESSAGES_KEY });
    await waitFor(() => expect(client.getQueryData(MESSAGES_KEY)).toBeUndefined());
  });

  it("counts a second, already-gone delete as done", async () => {
    setChatTransport(local({ deleteChatSession: vi.fn(async () => undefined) }));
    const { view } = setup(() => useDeleteChat("acme", "pidash-builtin"));
    await act(async () => view.result.current.mutateAsync({ sessionId: "s-1" }));
    await act(async () => view.result.current.mutateAsync({ sessionId: "s-1" }));
    expect(toasts.notifySuccess).toHaveBeenCalledTimes(2);
  });

  it("reports a refusal with the store's words", async () => {
    setChatTransport(local({ deleteChatSession: vi.fn(async () => Promise.reject(new Error("locked"))) }));
    const { view } = setup(() => useDeleteChat("acme", "pidash-builtin"));
    await act(async () => {
      await expect(view.result.current.mutateAsync({ sessionId: "s-1" })).rejects.toThrow("locked");
    });
    expect(toasts.notifyError).toHaveBeenCalledWith(DELETE_FAILED_TITLE, "locked");
  });

  it("reports a surface that cannot delete instead of claiming success", async () => {
    const { view } = setup(() => useDeleteChat("acme", "pidash-builtin"));
    await act(async () => {
      await expect(view.result.current.mutateAsync({ sessionId: "s-1" })).rejects.toThrow(HISTORY_FAILED_BODY);
    });
    expect(toasts.notifyError).toHaveBeenCalledWith(DELETE_FAILED_TITLE, HISTORY_FAILED_BODY);
  });
});

describe("useClearHistory", () => {
  it("clears through the transport and re-reads the history", async () => {
    const clearChatHistory = vi.fn(async () => undefined);
    setChatTransport(local({ clearChatHistory }));
    const { client, view } = setup(() => useClearHistory("acme", "pidash-builtin"));
    const invalidate = vi.spyOn(client, "invalidateQueries");
    await act(async () => view.result.current.mutateAsync());
    expect(clearChatHistory).toHaveBeenCalledTimes(1);
    expect(toasts.notifySuccess).toHaveBeenCalledWith(CLEARED_TOAST);
    expect(invalidate).toHaveBeenCalledWith({ queryKey: SESSIONS_KEY });
  });

  it("reports a failure with the store's words", async () => {
    setChatTransport(local({ clearChatHistory: vi.fn(async () => Promise.reject(new Error("disk"))) }));
    const { view } = setup(() => useClearHistory("acme", "pidash-builtin"));
    await act(async () => {
      await expect(view.result.current.mutateAsync()).rejects.toThrow("disk");
    });
    expect(toasts.notifyError).toHaveBeenCalledWith(CLEAR_FAILED_TITLE, "disk");
  });

  it("reports a surface that cannot clear instead of claiming success", async () => {
    const { view } = setup(() => useClearHistory("acme", "pidash-builtin"));
    await act(async () => {
      await expect(view.result.current.mutateAsync()).rejects.toThrow(HISTORY_FAILED_BODY);
    });
    expect(toasts.notifySuccess).not.toHaveBeenCalled();
    expect(toasts.notifyError).toHaveBeenCalledWith(CLEAR_FAILED_TITLE, HISTORY_FAILED_BODY);
  });
});
