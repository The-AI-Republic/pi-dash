// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Write verbs (NEWFRONT-551, RUN-027/030/031): a send reuses or opens the
// conversation, clears what it re-reads, restores a failed draft and names
// the failure once; stop cancels with the user's reason and re-reads
// sessions; close shuts the conversation and re-reads sessions.
import { QueryClientProvider } from "@tanstack/react-query";
import type { ChatMessage, ChatSession } from "@pidash/api-client";
import { act, renderHook, waitFor } from "@testing-library/react";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const contracts = vi.hoisted(() => ({ createChatSession: vi.fn() }));
const notify = vi.hoisted(() => ({ notifyError: vi.fn() }));
const active = vi.hoisted(() => ({ transport: null as unknown as import("./transport.js").ChatTransport }));

vi.mock("@pidash/api-client", async (importOriginal) => {
  const original = await importOriginal<typeof import("@pidash/api-client")>();
  return { ...original, ...contracts };
});
vi.mock("../../../shared/shell/index.js", () => notify);
vi.mock("./transport.js", () => ({ getChatTransport: () => active.transport }));

const { createAppClient, resetAppClient } = await import("../../../core/api/client.js");
const { makeSession, testClient } = await import("./harness.js");
const { chatKeys } = await import("./keys.js");
const { chatFailureReason, SEND_FAILED_BODY, SEND_FAILED_TITLE, useChatActions } = await import("./useChatActions.js");

const Provider = QueryClientProvider as unknown as React.ComponentType<{
  client: ReturnType<typeof testClient>;
  children?: React.ReactNode;
}>;

const openSession = makeSession({ id: "open" });

function transport(): import("./transport.js").ChatTransport {
  return {
    subscribeChatEvents: vi.fn(() => () => undefined),
    sendChatMessage: vi.fn(async (_id: string, content: string) => ({ id: "m-1", content }) as ChatMessage),
    cancelChat: vi.fn(async () => ({ ok: true })),
    closeChat: vi.fn(async () => openSession),
    applyApprovalMode: vi.fn(),
  };
}

type Props = Parameters<typeof useChatActions>[0];

function setup(overrides: Partial<Props> = {}) {
  const client = testClient();
  const onSelectSession = vi.fn();
  const onCreated = vi.fn();
  const props: Props = {
    workspaceSlug: "acme",
    workspaceId: "ws-id",
    runnerId: "r-1",
    session: openSession,
    onSelectSession,
    onCreated,
    ...overrides,
  };
  const wrapper = ({ children }: { children: React.ReactNode }) => <Provider client={client}>{children}</Provider>;
  const view = renderHook((input: Props) => useChatActions(input), { initialProps: props, wrapper });
  return { client, onSelectSession, onCreated, view, props };
}

beforeEach(() => {
  createAppClient("https://api.test", {} as never);
  active.transport = transport();
  contracts.createChatSession.mockReset();
  notify.notifyError.mockReset();
});

afterEach(() => {
  vi.restoreAllMocks();
  resetAppClient();
});

describe("chatFailureReason", () => {
  it("prefers the error's own message, then a string, then the default", () => {
    expect(chatFailureReason(new Error("runner unreachable"))).toBe("runner unreachable");
    expect(chatFailureReason(new Error(""))).toBe(SEND_FAILED_BODY);
    expect(chatFailureReason("refused")).toBe("refused");
    expect(chatFailureReason("")).toBe(SEND_FAILED_BODY);
    expect(chatFailureReason(undefined)).toBe(SEND_FAILED_BODY);
    expect(chatFailureReason({ code: 1 })).toBe(SEND_FAILED_BODY);
  });
});

describe("send", () => {
  it("sends the trimmed draft on the open conversation and re-reads both reads", async () => {
    const { client, view, onSelectSession } = setup();
    const invalidate = vi.spyOn(client, "invalidateQueries");
    await act(async () => view.result.current.send("  hello  ", vi.fn()));
    expect(active.transport.sendChatMessage).toHaveBeenCalledWith("open", "hello");
    expect(onSelectSession).not.toHaveBeenCalled();
    expect(invalidate).toHaveBeenCalledWith({ queryKey: chatKeys.messages("acme", "open") });
    expect(invalidate).toHaveBeenCalledWith({ queryKey: chatKeys.sessions("acme", "r-1") });
    expect(view.result.current.sending).toBe(false);
    expect(contracts.createChatSession).not.toHaveBeenCalled();
  });

  it("opens a conversation when none is resolved and selects it", async () => {
    const created = makeSession({ id: "fresh" });
    contracts.createChatSession.mockResolvedValue(created);
    const { view, onSelectSession, onCreated, client } = setup({ session: null });
    client.setQueryData(chatKeys.sessions("acme", "r-1"), [closed()]);
    await act(async () => view.result.current.send("hello", vi.fn()));
    expect(contracts.createChatSession).toHaveBeenCalledWith(expect.anything(), { workspace: "ws-id", runner: "r-1" });
    expect(onSelectSession).toHaveBeenCalledWith("fresh");
    expect(onCreated).toHaveBeenCalledWith(created);
    expect(client.getQueryData<ChatSession[]>(chatKeys.sessions("acme", "r-1"))?.[0]?.id).toBe("fresh");
    expect(active.transport.applyApprovalMode).toHaveBeenCalledWith("fresh");
  });

  it("opens a conversation when the current one is closed", async () => {
    contracts.createChatSession.mockResolvedValue(makeSession({ id: "fresh" }));
    const shut = closed();
    const { view } = setup({ session: shut });
    await act(async () => view.result.current.send("again", vi.fn()));
    expect(contracts.createChatSession).toHaveBeenCalledTimes(1);
    expect(active.transport.sendChatMessage).toHaveBeenCalledWith("fresh", "again");
  });

  it("does not publish a conversation the list already carries", async () => {
    const created = makeSession({ id: "fresh" });
    contracts.createChatSession.mockResolvedValue(created);
    const { view, client } = setup({ session: null });
    client.setQueryData(chatKeys.sessions("acme", "r-1"), [created]);
    await act(async () => view.result.current.send("hello", vi.fn()));
    expect(client.getQueryData<ChatSession[]>(chatKeys.sessions("acme", "r-1"))).toEqual([created]);
  });

  it("shows the sending state while the round-trip runs", async () => {
    let release: () => void = () => undefined;
    active.transport.sendChatMessage = vi.fn(
      () => new Promise<ChatMessage>((resolve) => (release = () => resolve({} as ChatMessage)))
    );
    const { view } = setup();
    let pending: Promise<void> = Promise.resolve();
    act(() => {
      pending = view.result.current.send("slow", vi.fn());
    });
    await waitFor(() => expect(view.result.current.sending).toBe(true));
    await act(async () => {
      release();
      await pending;
    });
    await waitFor(() => expect(view.result.current.sending).toBe(false));
  });

  it("ignores a blank draft and a second send while one runs", async () => {
    let release: () => void = () => undefined;
    active.transport.sendChatMessage = vi.fn(
      () => new Promise<ChatMessage>((resolve) => (release = () => resolve({} as ChatMessage)))
    );
    const { view } = setup();
    await act(async () => view.result.current.send("   ", vi.fn()));
    expect(active.transport.sendChatMessage).not.toHaveBeenCalled();
    let pending: Promise<void> = Promise.resolve();
    act(() => {
      pending = view.result.current.send("first", vi.fn());
    });
    await act(async () => view.result.current.send("second", vi.fn()));
    expect(active.transport.sendChatMessage).toHaveBeenCalledTimes(1);
    await act(async () => {
      release();
      await pending;
    });
  });

  it("restores the draft, names the reason and toasts on failure", async () => {
    active.transport.sendChatMessage = vi.fn(async () => {
      throw new Error("runner refused the turn");
    });
    const onFailure = vi.fn();
    const { view } = setup();
    await act(async () => view.result.current.send("  keep me  ", onFailure));
    expect(onFailure).toHaveBeenCalledWith("  keep me  ");
    expect(view.result.current.sendError).toBe("runner refused the turn");
    expect(notify.notifyError).toHaveBeenCalledWith(SEND_FAILED_TITLE, "runner refused the turn");
    await act(async () => view.result.current.dismissSendError());
    expect(view.result.current.sendError).toBeNull();
  });

  it("puts a failed draft back and clears the error at the next attempt", async () => {
    active.transport.sendChatMessage = vi
      .fn()
      .mockRejectedValueOnce(new Error("first failure"))
      .mockResolvedValueOnce({} as ChatMessage);
    const onFailure = vi.fn();
    const { view } = setup();
    await act(async () => view.result.current.send("draft", onFailure));
    expect(view.result.current.sendError).toBe("first failure");
    await act(async () => view.result.current.send("draft", onFailure));
    expect(view.result.current.sendError).toBeNull();
  });

  it("drops the outcome of a send whose contact the viewer left", async () => {
    let reject: (error: Error) => void = () => undefined;
    active.transport.sendChatMessage = vi.fn(
      () => new Promise<ChatMessage>((_resolve, rejectIt) => (reject = rejectIt))
    );
    const onFailure = vi.fn();
    const { view, props } = setup();
    let pending: Promise<void> = Promise.resolve();
    act(() => {
      pending = view.result.current.send("to the old contact", onFailure);
    });
    await waitFor(() => expect(active.transport.sendChatMessage).toHaveBeenCalled());
    view.rerender({ ...props, runnerId: "r-2" });
    await act(async () => {
      reject(new Error("late failure"));
      await pending;
    });
    expect(onFailure).not.toHaveBeenCalled();
    expect(notify.notifyError).not.toHaveBeenCalled();
  });

  it("does not open a conversation when the contact changed while it opened", async () => {
    let resolve: (value: ChatSession) => void = () => undefined;
    contracts.createChatSession.mockReturnValue(new Promise<ChatSession>((done) => (resolve = done)));
    const { view, props, onSelectSession } = setup({ session: null });
    let pending: Promise<void> = Promise.resolve();
    act(() => {
      pending = view.result.current.send("hello", vi.fn());
    });
    await waitFor(() => expect(contracts.createChatSession).toHaveBeenCalled());
    view.rerender({ ...props, runnerId: "r-2" });
    await act(async () => {
      resolve(makeSession({ id: "stale" }));
      await pending;
    });
    expect(onSelectSession).not.toHaveBeenCalled();
    expect(active.transport.sendChatMessage).not.toHaveBeenCalled();
  });

  it("works with a transport that has no approval-mode step", async () => {
    const { applyApprovalMode: _approvalMode, ...withoutApproval } = transport();
    active.transport = withoutApproval;
    const { view } = setup();
    await act(async () => view.result.current.send("hello", vi.fn()));
    expect(active.transport.sendChatMessage).toHaveBeenCalledWith("open", "hello");
  });
});

describe("stop", () => {
  it("cancels with the user's reason and re-reads sessions only", async () => {
    const { client, view } = setup();
    const invalidate = vi.spyOn(client, "invalidateQueries");
    await act(async () => view.result.current.stop());
    expect(active.transport.cancelChat).toHaveBeenCalledWith("open", "user_cancelled");
    expect(invalidate).toHaveBeenCalledTimes(1);
    expect(invalidate).toHaveBeenCalledWith({ queryKey: chatKeys.sessions("acme", "r-1") });
  });

  it("clears a send failure before interrupting", async () => {
    active.transport.sendChatMessage = vi.fn(async () => {
      throw new Error("nope");
    });
    const { view } = setup();
    await act(async () => view.result.current.send("x", vi.fn()));
    expect(view.result.current.sendError).toBe("nope");
    await act(async () => view.result.current.stop());
    expect(view.result.current.sendError).toBeNull();
  });

  it("does nothing without a conversation", async () => {
    const { view } = setup({ session: null });
    await act(async () => view.result.current.stop());
    expect(active.transport.cancelChat).not.toHaveBeenCalled();
  });
});

describe("close", () => {
  it("closes the conversation and re-reads sessions only", async () => {
    const { client, view } = setup();
    const invalidate = vi.spyOn(client, "invalidateQueries");
    await act(async () => view.result.current.close());
    expect(active.transport.closeChat).toHaveBeenCalledWith("open");
    expect(invalidate).toHaveBeenCalledTimes(1);
    expect(invalidate).toHaveBeenCalledWith({ queryKey: chatKeys.sessions("acme", "r-1") });
  });

  it("does nothing without a conversation", async () => {
    const { view } = setup({ session: null });
    await act(async () => view.result.current.close());
    expect(active.transport.closeChat).not.toHaveBeenCalled();
  });
});

function closed(): ChatSession {
  return makeSession({ id: "closed", status: "closed" });
}
